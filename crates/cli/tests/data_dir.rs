//! The data directory stays out of every session on the real binary
//! (AUDIT S1, FIX_PLAN 2.1): a session folder that contains it is refused,
//! file tools cannot reach it from a project folder, and the API token is
//! scrubbed from what the shell prints.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Server {
    child: Child,
    addr: String,
    token: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(data: &Path) -> Server {
    start_in(data, data, Path::new("/"))
}

/// The server with `--data-dir <arg>` (relative to `cwd` when relative);
/// `data` is where that is.
fn start_in(data: &Path, arg: &Path, cwd: &Path) -> Server {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rizzy-harness"))
        .args(["--data-dir", arg.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .current_dir(cwd)
        .env("HARNESS_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn harness");
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let addr = loop {
        let line = lines.next().expect("server exited").unwrap();
        if let Some(rest) = line.split("listening on http://").nth(1) {
            break rest.trim().to_string();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    let token = std::fs::read_to_string(data.join("api-token")).unwrap();
    Server { child, addr, token }
}

impl Server {
    async fn post(&self, path: &str, body: Value) -> (reqwest::StatusCode, Value) {
        let r = reqwest::Client::new()
            .post(format!("http://{}{path}", self.addr))
            .bearer_auth(self.token.trim())
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn session(&self, cwd: &Path) -> String {
        let (st, v) = self
            .post(
                "/api/sessions",
                json!({"cwd": cwd.display().to_string(), "approval_mode": "auto"}),
            )
            .await;
        assert!(st.is_success(), "{st} {v}");
        v["session_id"].as_str().unwrap().to_string()
    }

    /// The output of the first tool call a prompt makes.
    async fn tool_output(&self, sid: &str, text: &str) -> (String, String) {
        let (st, v) = self
            .post(
                &format!("/api/sessions/{sid}/prompt"),
                json!({"text": text}),
            )
            .await;
        assert!(st.is_success(), "{st} {v}");
        let started = Instant::now();
        loop {
            let v: Value = reqwest::Client::new()
                .get(format!(
                    "http://{}/api/sessions/{sid}/events?limit=5000",
                    self.addr
                ))
                .bearer_auth(self.token.trim())
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let evs = v["events"].as_array().cloned().unwrap_or_default();
            if let Some(f) = evs.iter().find(|e| e["kind"] == "tool_call_finished") {
                let out = f["payload"]["output"].as_str().unwrap_or("").to_string();
                return (out, Value::Array(evs).to_string());
            }
            assert!(started.elapsed() < Duration::from_secs(30), "{evs:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// `<home>/data` is the data directory, `<home>/ws` a project; node.toml
/// allows all of `<home>`, as the default (`$HOME`) does.
fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let data = home.join("data");
    let ws = home.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        data.join("node.toml"),
        format!(
            "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n\
             allow_outside_workspace = true\n",
            home.display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
    (dir, data, ws)
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_and_tools_stay_out_of_the_data_dir() {
    let (dir, data, ws) = setup();
    let s = start(&data);
    let token = s.token.trim().to_string();
    assert!(token.len() >= 32);

    // The folder above the data directory, and the data directory itself.
    for cwd in [dir.path().to_path_buf(), data.clone()] {
        let (st, v) = s
            .post("/api/sessions", json!({"cwd": cwd.display().to_string()}))
            .await;
        assert!(!st.is_success(), "{} accepted: {v}", cwd.display());
        let msg = v.to_string();
        assert!(msg.contains("data directory"), "{msg}");
    }

    let sid = s.session(&ws).await;
    // `read` from the project folder, even with allow_outside_workspace.
    let (out, _) = s.tool_output(&sid, "read: ../data/api-token").await;
    assert!(out.contains("data directory"), "{out}");
    assert!(!out.contains(&token), "{out}");

    // bash runs in the Landlock sandbox (FIX_PLAN 2.7): the data directory
    // does not exist for it. (Before 2.7 the file was printed and only the
    // token itself was scrubbed.)
    let sid = s.session(&ws).await;
    let (out, all) = s
        .tool_output(&sid, "run: cat ../data/api-token; echo; echo done")
        .await;
    assert!(out.contains("done"), "{out}");
    assert!(out.contains("Permission denied"), "{out}");
    assert!(!out.contains("[REDACTED]"), "the file was read: {out}");
    assert!(
        !all.contains(&token),
        "the API token is in the events: {all}"
    );
}

/// S-4 (R.1): a relative `--data-dir` is made absolute once, where the
/// binary reads it, so the sandbox's temp folder and the protected folder
/// are absolute too; the wrapper runs in the session folder, where a
/// relative path would name a folder of the project's (and a link planted
/// there would lead anywhere). The shell's `TMPDIR` is the data
/// directory's, and the data directory stays closed to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_relative_data_dir_is_made_absolute_for_the_sandbox() {
    let (dir, data, ws) = setup();
    let home = dir.path().canonicalize().unwrap();
    // A planted link where a relative temp root would land.
    std::fs::create_dir_all(ws.join("data")).unwrap();
    std::os::unix::fs::symlink(&data, ws.join("data/sandbox-tmp")).unwrap();
    let s = start_in(&data, Path::new("data"), &home);
    let sid = s.session(&ws).await;
    let (out, _) = s
        .tool_output(
            &sid,
            "run: echo \"tmp=$TMPDIR\"; cat ../data/api-token; echo done",
        )
        .await;
    assert!(out.contains("done"), "{out}");
    let tmp = data.join("sandbox-tmp");
    assert!(
        out.contains(&format!("tmp={}/", tmp.display())),
        "TMPDIR is not the data directory's: {out}"
    );
    assert!(out.contains("Permission denied"), "{out}");
}

/// The shell's view of the home directory: a private key next to the
/// project is not readable, the project itself is (FIX_PLAN 2.7).
#[tokio::test(flavor = "multi_thread")]
async fn bash_cannot_read_keys_beside_the_project() {
    let (dir, data, ws) = setup();
    let ssh = dir.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("id_ed25519"), "PRIVATE-KEY-CANARY\n").unwrap();
    std::fs::write(ws.join("notes.txt"), "project-file-ok\n").unwrap();
    let s = start(&data);
    let sid = s.session(&ws).await;
    let (out, _) = s
        .tool_output(
            &sid,
            "run: cat ../.ssh/id_ed25519; cat notes.txt; echo built > out.txt && cat out.txt",
        )
        .await;
    assert!(!out.contains("PRIVATE-KEY-CANARY"), "{out}");
    assert!(out.contains("Permission denied"), "{out}");
    assert!(out.contains("project-file-ok"), "{out}");
    assert!(out.contains("built"), "{out}");
}

/// AUDIT L7: a file holding a secret that group or others can read stops
/// `serve` before it is read, naming the file and the `chmod`. Each in a
/// fresh data directory; a symlink is judged by its target.
#[cfg(unix)]
#[test]
fn open_secret_files_stop_the_server() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m));
    let cases: [(&str, &str); 4] = [
        ("api-token", "api-token"),
        ("secrets.toml", "secrets.toml"),
        ("keys/ui-key.der", "ui-key.der"),
        ("link", "api-token"),
    ];
    for (file, named) in cases {
        let data = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(data.path().join("keys")).unwrap();
        if file == "link" {
            // api-token -> elsewhere/token (0640): the target counts.
            let target = data.path().join("token-target");
            std::fs::write(&target, "a".repeat(64)).unwrap();
            mode(&target, 0o640).unwrap();
            std::os::unix::fs::symlink(&target, data.path().join("api-token")).unwrap();
        } else {
            let path = data.path().join(file);
            std::fs::write(&path, "x").unwrap();
            mode(&path, 0o644).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_rizzy-harness"))
            .args(["--data-dir", data.path().to_str().unwrap()])
            .args(["serve", "--bind", "127.0.0.1:0"])
            .env("HARNESS_LOG", "error")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn harness");
        let started = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            if started.elapsed() > Duration::from_secs(20) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{file}: the server started with a readable secret file");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut err = String::new();
        std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
        assert!(!status.success(), "{file}");
        assert!(err.contains("chmod 600"), "{file}: {err}");
        assert!(err.contains(named), "{file}: {err}");
    }
}

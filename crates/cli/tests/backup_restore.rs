//! `backup` and `restore` on the real binary (FIX_PLAN 4.4,
//! docs/backup-restore.md): an account's chat and stored file survive a
//! backup taken beside a running server and one taken with it stopped;
//! a file deleted between the copy of master.db and its link makes the
//! backup copy master.db again; the node logs are copied after the mirror,
//! so no restored chat freezes; and every refusal of the abuse list.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[path = "support/serve.rs"]
mod real;

use real::EXE;

const PASSWORD: &str = "a password of my own";

fn cmd(data: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(EXE);
    c.args(["--data-dir", data.to_str().unwrap()])
        .args(args)
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "warn")
        .env_remove("HARNESS_BACKUP_PAUSE_FILE")
        .env_remove("HARNESS_RESTORE_PAUSE_FILE")
        .stdin(Stdio::null());
    c
}

/// Any command here ends well within this, or the test fails saying so.
const DEADLINE: Duration = Duration::from_secs(120);
/// A `serve` that must be refused is refused within this.
const REFUSED_WITHIN: Duration = Duration::from_secs(10);

/// Wait for `child` at most `within`; past it, kill it (by its own pid)
/// and fail with `what` and everything it printed.
fn wait_within(mut child: Child, within: Duration, what: &str) -> Output {
    use std::io::Read;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_end(&mut b);
        }
        b
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut b);
        }
        b
    });
    let by = Instant::now() + within;
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if Instant::now() >= by {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (stdout, stderr) = (o.join().unwrap(), e.join().unwrap());
    let Some(status) = status else {
        panic!(
            "{what}: still running after {within:?}, killed; it printed:\n--- stdout\n{}--- stderr\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    };
    Output {
        status,
        stdout,
        stderr,
    }
}

fn run_within(data: &Path, args: &[&str], within: Duration, what: &str) -> Output {
    let child = cmd(data, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_within(child, within, what)
}

fn run(data: &Path, args: &[&str]) -> Output {
    run_within(
        data,
        args,
        DEADLINE,
        &format!("`{EXE} --data-dir {} {}`", data.display(), args.join(" ")),
    )
}

/// `serve` on `data`, which must be refused: one that stays up is killed
/// and fails the test with that sentence.
fn serve_refused(data: &Path) -> Output {
    run_within(
        data,
        &["serve", "--bind", "127.0.0.1:0"],
        REFUSED_WITHIN,
        &format!(
            "serve on {} started although it must be refused",
            data.display()
        ),
    )
}

fn text(o: &Output) -> String {
    format!(
        "status {:?}\n--- stdout\n{}--- stderr\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn refused(o: Output, why: &str) -> String {
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(t.contains(why), "wanted {why:?} in\n{t}");
    t
}

/// A backup started with the test pause (debug builds): it stops after its
/// first copy of master.db until `<pause>` exists. Returns once it waits.
fn paused_backup(data: &Path, dest: &Path, pause: &Path) -> Child {
    let mut c = cmd(data, &["backup", dest.to_str().unwrap()]);
    c.env("HARNESS_BACKUP_PAUSE_FILE", pause)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = c.spawn().unwrap();
    let mark = PathBuf::from(format!("{}.snapshotted", pause.display()));
    let by = Instant::now() + Duration::from_secs(60);
    while !mark.exists() {
        assert!(Instant::now() < by, "the backup never reached its pause");
        std::thread::sleep(Duration::from_millis(20));
    }
    child
}

fn release(child: Child, pause: &Path) -> Output {
    std::fs::write(pause, b"").unwrap();
    wait_within(child, DEADLINE, "the released backup")
}

/// Every folder under `root` (itself too) is 0700 and every file 0600.
fn all_private(root: &Path) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let m = std::fs::symlink_metadata(&d).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o700, "{}", d.display());
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            assert!(!m.file_type().is_symlink(), "{}", p.display());
            if m.is_dir() {
                stack.push(p);
            } else {
                assert_eq!(m.permissions().mode() & 0o777, 0o600, "{}", p.display());
            }
        }
    }
}

/// A PNG header of 2×2 (sniffed, never decoded), then `tail`.
fn png(tail: &[u8]) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(2u32.to_be_bytes());
    b.extend(2u32.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.extend(tail);
    b
}

async fn upload(s: &real::Serve, tail: &[u8]) -> String {
    let (st, body) = s
        .send_bytes(
            reqwest::Method::POST,
            "/api/media",
            &[("x-file-name", "photo.png"), ("content-type", "image/png")],
            png(tail),
        )
        .await;
    assert_eq!(st, 201, "{}", String::from_utf8_lossy(&body));
    let v: Value = serde_json::from_slice(&body).unwrap();
    v["media_id"].as_str().unwrap().to_string()
}

async fn content(s: &real::Serve, media: &str) -> (reqwest::StatusCode, Vec<u8>) {
    s.send_bytes(
        reqwest::Method::GET,
        &format!("/api/media/{media}/content"),
        &[],
        Vec::new(),
    )
    .await
}

fn count(evs: &[Value], kind: &str) -> usize {
    evs.iter().filter(|e| e["kind"] == kind).count()
}

/// A plain chat of the signed-in account with one finished turn.
async fn chat(s: &real::Serve, said: &str) -> String {
    let sid = s.post("/api/sessions", json!({"profile": "chat"})).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": said}),
    )
    .await;
    s.wait_for(&sid, |e| count(e, "turn_ended") == 1).await;
    sid
}

fn said(evs: &[Value], text: &str) -> bool {
    evs.iter()
        .any(|e| e["kind"] == "user_message" && e["payload"].to_string().contains(text))
}

/// Restore `backup` into a new `<root>/<name>`, start a server there, sign
/// in again as `user`; the server.
async fn restore_and_serve(
    root: &Path,
    name: &str,
    backup: &Path,
    user: &str,
) -> (PathBuf, real::Serve, String) {
    let data = root.join(name);
    let out = ok(run(&data, &["restore", backup.to_str().unwrap()]));
    assert!(
        out.contains("logins were dropped: everyone signs in again"),
        "{out}"
    );
    assert!(out.contains("Not in this backup:"), "{out}");
    all_private(&data);
    let mut s = real::serve(&data);
    s.sign_in(user, PASSWORD).await;
    (data, s, out)
}

/// The row's test (FIX_PLAN 4.4): an account's chat and stored file, a
/// backup taken while the server runs, a restore into a new directory, a
/// server started there; the old login is refused, a new one reads both
/// back, and the chat takes a new prompt. Nothing secret is printed; the
/// backup is 0700/0600; `--verify` passes.
#[tokio::test(flavor = "multi_thread")]
async fn a_running_server_is_backed_up_and_restored() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let old_login = s.login();
    let sid = chat(&s, "remember the blue door").await;
    let media = upload(&s, b"the photo's own bytes").await;
    let token = std::fs::read_to_string(data.join("api-token")).unwrap();

    let dest = root.join("backup");
    let o = run(&data, &["backup", dest.to_str().unwrap()]);
    let out = ok(o.clone());
    assert!(out.contains("beside the running server"), "{out}");
    assert!(out.contains("MANIFEST.json SHA-256 "), "{out}");
    assert!(
        out.contains("Not in this backup: the project folders"),
        "{out}"
    );
    let all = text(&o);
    assert!(!all.contains(token.trim()), "the API token was printed");
    assert!(!all.contains(PASSWORD));
    all_private(&dest);
    assert!(!data.join("backup-staging").exists(), "staging left behind");
    let v = ok(run(&data, &["backup", "--verify", dest.to_str().unwrap()]));
    assert!(v.contains("is a complete backup"), "{v}");
    let sha = out
        .split("SHA-256 ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap();
    assert!(v.contains(sha), "verify names the same manifest hash");

    // The original keeps running: the copy is checked on its own data.
    let (_d2, mut r, _) = restore_and_serve(&root, "restored", &dest, "ana").await;
    let mine = r.login();
    r.set_login(old_login);
    let (st, _) = content(&r, &media).await;
    assert_eq!(st, 401, "a login of the original works on the copy");
    r.set_login(mine);
    let evs = r.events(&sid).await;
    assert!(said(&evs, "remember the blue door"), "{evs:#?}");
    let (st, body) = content(&r, &media).await;
    assert_eq!(st, 200);
    assert_eq!(body, png(b"the photo's own bytes"));
    r.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "after the restore"}),
    )
    .await;
    r.wait_for(&sid, |e| count(e, "turn_ended") == 2).await;
    assert!(!r.log().contains("two writers"), "{}", r.log());
    drop((s, r, dir));
}

/// With the server stopped (killed: its WAL is left as a crash leaves it),
/// the backup holds the data directory's lock: a server started meanwhile
/// is refused naming the backup. The restore reads both back.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_server_is_backed_up_under_the_lock() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let sid = chat(&s, "the stopped one").await;
    let media = upload(&s, b"bytes kept offline").await;
    drop(s);

    let dest = root.join("backup");
    let pause = root.join("go");
    let b = paused_backup(&data, &dest, &pause);
    refused(serve_refused(&data), "is in use by backup (pid");
    let out = ok(release(b, &pause));
    assert!(out.contains("no server was running"), "{out}");

    let (_d2, r, _) = restore_and_serve(&root, "restored", &dest, "ana").await;
    assert!(said(&r.events(&sid).await, "the stopped one"));
    let (st, body) = content(&r, &media).await;
    assert_eq!((st.as_u16(), body), (200, png(b"bytes kept offline")));
    drop(dir);
}

/// A stored file deleted after the copy of master.db and before its link:
/// the backup copies master.db again and finishes; in the backup every file
/// master.db names is there and opens to its row's SHA-256 (`--verify`),
/// and the deleted one is gone from both.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_during_the_backup_makes_it_copy_master_db_again() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let keep = upload(&s, b"kept").await;
    let gone = upload(&s, b"deleted meanwhile").await;

    let dest = root.join("backup");
    let pause = root.join("go");
    let b = paused_backup(&data, &dest, &pause);
    let (st, _) = s
        .send_bytes(
            reqwest::Method::DELETE,
            &format!("/api/media/{gone}"),
            &[],
            Vec::new(),
        )
        .await;
    assert!(st.is_success(), "{st}");
    let out = ok(release(b, &pause));
    assert!(out.contains("(2 copies of master.db"), "{out}");
    ok(run(&data, &["backup", "--verify", dest.to_str().unwrap()]));

    let (_d2, r, _) = restore_and_serve(&root, "restored", &dest, "ana").await;
    assert_eq!(content(&r, &keep).await.0, 200);
    assert_eq!(content(&r, &gone).await.0, 404);
    drop((s, dir));
}

/// Events added between the copy of master.db and the node logs: in the
/// backup each chat's log (chat.db) is at or after the mirror's copy of it
/// (master.db), never behind; the restored server shows no "two writers"
/// line and every chat takes a new prompt.
#[tokio::test(flavor = "multi_thread")]
async fn the_node_logs_are_copied_after_the_mirror() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let a = chat(&s, "first chat").await;

    let dest = root.join("backup");
    let pause = root.join("go");
    let b = paused_backup(&data, &dest, &pause);
    // After master.db's copy: a new turn in the old chat and a new chat.
    s.post(
        &format!("/api/sessions/{a}/prompt"),
        json!({"text": "later"}),
    )
    .await;
    s.wait_for(&a, |e| count(e, "turn_ended") == 2).await;
    let late = chat(&s, "a chat made during the backup").await;
    let out = ok(release(b, &pause));
    assert!(
        out.contains("1 chats begun while the backup ran are not in it"),
        "{out}"
    );

    let heads = |db: &str| -> std::collections::BTreeMap<String, i64> {
        let c = rusqlite::Connection::open_with_flags(
            dest.join(db),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut st = c
            .prepare("SELECT session_id, max(seq) FROM events GROUP BY session_id")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let (mirror, log) = (heads("master.db"), heads("chat.db"));
    for (sid, seq) in &mirror {
        if let Some(l) = log.get(sid) {
            assert!(l >= seq, "{sid}: mirror {seq}, log {l}");
        }
    }
    assert!(
        log[&a] > mirror[&a],
        "the test did add events after master.db"
    );
    // Begun after master.db's copy: left out of every file, so it cannot
    // come back without its owner.
    assert!(!log.contains_key(&late) && !mirror.contains_key(&late));

    let (_d2, r, _) = restore_and_serve(&root, "restored", &dest, "ana").await;
    r.post(
        &format!("/api/sessions/{a}/prompt"),
        json!({"text": "again"}),
    )
    .await;
    let evs = r.wait_for(&a, |e| count(e, "turn_ended") == 3).await;
    assert!(
        said(&evs, "later"),
        "the events after master.db's copy are kept"
    );
    let listed = r.get("/api/sessions").await.to_string();
    assert!(!listed.contains(&late), "{listed}");
    assert!(!r.log().contains("two writers"), "{}", r.log());
    drop((s, dir));
}

/// Where a backup may not go, and what it may not take.
#[test]
fn backup_destinations_that_are_refused() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    // A data directory with a database (a server ran on it once).
    drop(real::serve(&data));
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
    refused(
        run(&data, &["backup", link.to_str().unwrap()]),
        "is a symbolic link",
    );
    std::fs::write(elsewhere.join("x"), b"").unwrap();
    refused(
        run(&data, &["backup", elsewhere.to_str().unwrap()]),
        "is not empty",
    );
    refused(
        run(&data, &["backup", data.join("b").to_str().unwrap()]),
        "inside the data directory",
    );
    refused(
        run(&data, &["backup", root.to_str().unwrap()]),
        "inside the data directory",
    );
    refused(
        run(
            &root.join("none"),
            &["backup", root.join("b").to_str().unwrap()],
        ),
        "no data directory there",
    );
    assert!(
        !root.join("none").exists(),
        "a backup made a data directory"
    );
    // A database of a newer build is refused, and nothing is kept.
    let c = rusqlite::Connection::open(data.join("node.db")).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let dest = root.join("newer");
    refused(
        run(&data, &["backup", dest.to_str().unwrap()]),
        "written by a newer rizzy-harness",
    );
    assert!(!dest.exists(), "a refused backup left {}", dest.display());
    assert!(!data.join("backup-staging").exists());
    drop(dir);
}

/// SIGTERM in the middle: the partial backup and the staging folder are
/// removed. SIGKILL: what is left has no manifest (restore refuses it), and
/// the staging folder left in the data directory neither stops a server
/// nor loses a stored file, and the next backup removes it.
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_backup_leaves_nothing_a_restore_takes() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let media = upload(&s, b"still here").await;

    let dest = root.join("termed");
    let pause = root.join("go1");
    let b = paused_backup(&data, &dest, &pause);
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(b.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let o = release(b, &pause);
    refused(o, "interrupted");
    assert!(!dest.exists(), "{}", dest.display());
    assert!(!data.join("backup-staging").exists());

    let dest = root.join("killed");
    let pause = root.join("go2");
    let mut b = paused_backup(&data, &dest, &pause);
    b.kill().unwrap();
    b.wait().unwrap();
    assert!(data.join("backup-staging").exists());
    refused(
        run(&root.join("r1"), &["restore", dest.to_str().unwrap()]),
        "it is not a finished backup",
    );
    drop(s);
    let s = real::serve(&data);
    let mut s = s;
    s.sign_in("ana", PASSWORD).await;
    assert_eq!(content(&s, &media).await.0, 200);
    let dest = root.join("next");
    ok(run(&data, &["backup", dest.to_str().unwrap()]));
    assert!(!data.join("backup-staging").exists());
    drop((s, dir));
}

/// A finished backup to break, in `<root>/backup`.
fn a_backup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut s = real::serve(&data);
        s.as_member("ana").await;
        upload(&s, b"one stored file").await;
    });
    let dest = root.join("backup");
    ok(run(&data, &["backup", dest.to_str().unwrap()]));
    (dir, root, dest)
}

fn manifest(b: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(b.join("MANIFEST.json")).unwrap()).unwrap()
}

fn write_manifest(b: &Path, v: &Value) {
    let p = b.join("MANIFEST.json");
    std::fs::remove_file(&p).unwrap();
    std::fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}

fn a_blob(b: &Path) -> String {
    manifest(b)["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap().to_string())
        .find(|p| p.starts_with("blobs/"))
        .unwrap()
}

/// A restore into `<root>/<name>` that must be refused with `why`; the
/// target is left empty (only the lock file), never half restored.
fn restore_refused(root: &Path, name: &str, b: &Path, why: &str) -> String {
    let data = root.join(name);
    let t = refused(run(&data, &["restore", b.to_str().unwrap()]), why);
    let left: Vec<_> = std::fs::read_dir(&data)
        .map(|r| r.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(
        left.iter().all(|n| n == "lock"),
        "{name}: left {left:?} after\n{t}"
    );
    t
}

/// Each damaged or hostile backup is refused with a sentence naming the
/// file, and nothing is restored.
#[test]
fn damaged_and_hostile_backups_are_refused() {
    let (dir, root, b) = a_backup();
    let fresh = || {
        let n = root.join(format!("copy-{}", rand_name()));
        let o = Command::new("cp")
            .args(["-a", b.to_str().unwrap(), n.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(o.status.success());
        n
    };

    // A flipped byte in a stored file.
    let c = fresh();
    let blob = a_blob(&c);
    let mut bytes = std::fs::read(c.join(&blob)).unwrap();
    let i = bytes.len() / 2;
    bytes[i] ^= 1;
    std::fs::write(c.join(&blob), bytes).unwrap();
    let t = restore_refused(&root, "t1", &c, "its content does not match the manifest");
    assert!(t.contains(&blob), "{t}");
    refused(
        run(&root, &["backup", "--verify", c.to_str().unwrap()]),
        &blob,
    );

    // A truncated database.
    let c = fresh();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(c.join("chat.db"))
        .unwrap();
    f.set_len(4096).unwrap();
    restore_refused(&root, "t2", &c, "chat.db: the backup's file is 4096 bytes");

    // No manifest (an interrupted backup).
    let c = fresh();
    std::fs::remove_file(c.join("MANIFEST.json")).unwrap();
    restore_refused(&root, "t3", &c, "it is not a finished backup");

    // Paths outside the fixed shape.
    for bad in [
        "../outside",
        "/etc/passwd",
        "keys/../master.db",
        "claim-code",
    ] {
        let c = fresh();
        let mut m = manifest(&c);
        m["files"].as_array_mut().unwrap().push(json!({
            "path": bad, "size": 1,
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        }));
        write_manifest(&c, &m);
        restore_refused(&root, "t4", &c, "names a path a backup never holds");
        assert!(!root.join("outside").exists());
    }

    // A listed file replaced by a link to another file of the same size.
    let c = fresh();
    let blob = a_blob(&c);
    let other = root.join("planted");
    std::fs::copy(c.join(&blob), &other).unwrap();
    std::fs::remove_file(c.join(&blob)).unwrap();
    std::os::unix::fs::symlink(&other, c.join(&blob)).unwrap();
    restore_refused(&root, "t5", &c, "a link or not a regular file");

    // A folder on the way replaced by a link.
    let c = fresh();
    let moved = root.join("blobs-elsewhere");
    std::fs::rename(c.join("blobs"), &moved).unwrap();
    std::os::unix::fs::symlink(&moved, c.join("blobs")).unwrap();
    restore_refused(&root, "t6", &c, "in the backup is not a folder");

    // A newer schema, a newer format.
    let c = fresh();
    let mut m = manifest(&c);
    m["databases"]["master.db"] = json!(99);
    write_manifest(&c, &m);
    restore_refused(
        &root,
        "t7",
        &c,
        "master.db in the backup has schema version 99",
    );
    let c = fresh();
    let mut m = manifest(&c);
    m["format"] = json!(2);
    write_manifest(&c, &m);
    restore_refused(&root, "t8", &c, "the backup's format is 2");

    // A planted file nobody listed: not restored, and said.
    let c = fresh();
    std::fs::write(c.join("evil.sh"), b"#!/bin/sh\n").unwrap();
    std::fs::write(c.join("keys").join("extra.key"), b"x").ok();
    let data = root.join("t9");
    let out = ok(run(&data, &["restore", c.to_str().unwrap()]));
    assert!(
        out.contains("files in the backup are not in its manifest"),
        "{out}"
    );
    assert!(!data.join("evil.sh").exists());
    assert!(!data.join("keys/extra.key").exists());
    assert!(!data.join("RESTORING").exists());
    drop(dir);
}

fn rand_name() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::SeqCst).to_string()
}

/// Where a restore may not go: a directory that is not empty (a lone
/// `lost+found` too), a running server's (its lock is held). A directory
/// holding `RESTORING` (an interrupted restore) does not start.
#[test]
fn restore_targets_that_are_refused() {
    let (dir, root, b) = a_backup();
    let t = root.join("lf");
    std::fs::create_dir_all(t.join("lost+found")).unwrap();
    refused(run(&t, &["restore", b.to_str().unwrap()]), "is not empty");
    let (_d, live, _) = real::setup();
    let s = real::serve(&live);
    refused(
        run(&live, &["restore", b.to_str().unwrap()]),
        "is in use by serve (pid",
    );
    drop(s);
    let done = root.join("done");
    ok(run(&done, &["restore", b.to_str().unwrap()]));
    std::fs::write(done.join("RESTORING"), b"").unwrap();
    refused(serve_refused(&done), "holds an interrupted restore");
    drop(dir);
}

/// `--pause-routines` switches every routine off in the copy, and says so.
#[tokio::test(flavor = "multi_thread")]
async fn pause_routines_restores_them_off() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    {
        let mut s = real::serve(&data);
        s.as_member("ana").await;
    }
    // A routine row of ana's as the server keeps it, written while it is
    // stopped.
    let c = rusqlite::Connection::open(data.join("master.db")).unwrap();
    c.execute(
        "INSERT INTO routines (routine_id, workspace_id, owner_id, name, spec, created_ms, \
         updated_ms) SELECT 'r1', m.workspace_id, m.user_id, 'nightly', '{}', 0, 0 \
         FROM workspace_members m JOIN users u ON u.user_id = m.user_id \
         WHERE u.username = 'ana' LIMIT 1",
        [],
    )
    .unwrap();
    drop(c);
    let dest = root.join("backup");
    ok(run(&data, &["backup", dest.to_str().unwrap()]));
    let t = root.join("restored");
    let out = ok(run(
        &t,
        &["restore", dest.to_str().unwrap(), "--pause-routines"],
    ));
    assert!(out.contains("1 routines were paused"), "{out}");
    let c = rusqlite::Connection::open(t.join("master.db")).unwrap();
    let on: i64 = c
        .query_row("SELECT enabled FROM routines", [], |r| r.get(0))
        .unwrap();
    assert_eq!(on, 0);
    drop(dir);
}

/// A backup whose databases are at an older schema version (made here
/// with the recorded migrations up to version 1): restored, and the first
/// start migrates them forward.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_backup_is_migrated_at_the_first_start() {
    use sha2::Digest;
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let b = root.join("old-backup");
    std::fs::create_dir(&b).unwrap();
    let mut files = Vec::new();
    let mut dbs = serde_json::Map::new();
    for name in ["master.db", "chat.db", "node.db"] {
        let mut c = rusqlite::Connection::open(b.join(name)).unwrap();
        harness_store::migrate::migrate(&mut c, name, &harness_store::DB_SCHEMA.steps[..1])
            .unwrap();
        drop(c);
        let bytes = std::fs::read(b.join(name)).unwrap();
        files.push(json!({"path": name, "size": bytes.len(),
            "sha256": hex_of(&sha2::Sha256::digest(&bytes))}));
        dbs.insert(name.into(), json!(1));
    }
    let m = json!({"format": 1, "harness": "0", "created_ms": 0, "online": false,
        "databases": dbs, "files": files});
    std::fs::write(b.join("MANIFEST.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let t = root.join("restored");
    ok(run(&t, &["restore", b.to_str().unwrap()]));
    let s = real::serve(&t);
    let v = s.get("/api/sessions").await;
    assert!(v["sessions"].is_array(), "{v}");
    let c = rusqlite::Connection::open(t.join("master.db")).unwrap();
    let ver: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ver, harness_store::DB_SCHEMA.steps.len() as i64);
    drop((s, dir));
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Checkpoints are not in a backup: on the restored server, undoing a
/// turn made before the backup is refused with a sentence, the folder is
/// left as it is, and the server goes on.
#[tokio::test(flavor = "multi_thread")]
async fn undo_after_a_restore_without_checkpoints_is_refused_in_words() {
    let (dir, data, ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    std::fs::write(ws.join("f.txt"), "one\n").unwrap();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let sid = s
        .post(
            "/api/sessions",
            json!({"cwd": ws.display().to_string(), "approval_mode": "auto"}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "run: echo two > f.txt"}),
    )
    .await;
    let evs = s.wait_for(&sid, |e| count(e, "turn_ended") == 1).await;
    let turn = evs
        .iter()
        .find(|e| e["kind"] == "turn_started")
        .and_then(|e| e["payload"]["turn_id"].as_str())
        .unwrap()
        .to_string();
    // The after-turn checkpoint is written once the turn has ended.
    let by = Instant::now() + Duration::from_secs(60);
    while !std::fs::read_dir(data.join("checkpoints")).is_ok_and(|mut d| d.next().is_some()) {
        assert!(Instant::now() < by, "no checkpoint\n{}", s.log());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(s);
    let dest = root.join("backup");
    ok(run(&data, &["backup", dest.to_str().unwrap()]));
    assert!(!dest.join("checkpoints").exists());

    let (_d2, r, _) = restore_and_serve(&root, "restored", &dest, "ana").await;
    assert_eq!(std::fs::read_to_string(ws.join("f.txt")).unwrap(), "two\n");
    let (st, body) = r
        .raw(
            reqwest::Method::POST,
            &format!("/api/sessions/{sid}/turns/{turn}/restore"),
            Some(json!({})),
        )
        .await;
    assert!(!st.is_success(), "{st} {body}");
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let why = v["error"].as_str().unwrap_or_default();
    assert!(why.len() > 10 && !why.contains("panick"), "{st} {body}");
    assert_eq!(std::fs::read_to_string(ws.join("f.txt")).unwrap(), "two\n");
    assert!(r.get("/api/sessions").await["sessions"].is_array());
    eprintln!("undo after restore answered {st}: {why}");
    drop((r, dir));
}

/// A copy of backup `b` to break, beside it.
fn copy_of(root: &Path, b: &Path) -> PathBuf {
    let n = root.join(format!("copy-{}", rand_name()));
    let o = Command::new("cp")
        .args(["-a", b.to_str().unwrap(), n.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success());
    n
}

fn fifo(p: &Path) {
    if std::fs::symlink_metadata(p).is_ok() {
        std::fs::remove_file(p).unwrap();
    }
    nix::unistd::mkfifo(p, nix::sys::stat::Mode::S_IRWXU).unwrap();
}

/// A character device at `p` (`/dev/null`'s numbers), if this user may
/// make one (usually not: then the test says so and goes on).
fn device(p: &Path) -> bool {
    use nix::sys::stat::{Mode, SFlag, makedev, mknod};
    if std::fs::symlink_metadata(p).is_ok() {
        std::fs::remove_file(p).unwrap();
    }
    match mknod(p, SFlag::S_IFCHR, Mode::S_IRWXU, makedev(1, 3)) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("note: no device node test here: mknod is refused to this user ({e})");
            false
        }
    }
}

const QUICK: Duration = Duration::from_secs(30);

fn empty_but_lock(data: &Path) {
    let left: Vec<_> = std::fs::read_dir(data)
        .map(|r| r.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(left.iter().all(|n| n == "lock"), "left {left:?}");
}

/// A FIFO (or a device) in place of a file of a backup: `--verify` and
/// `restore` refuse it at once, naming it, and the restore leaves the
/// target empty. A FIFO the manifest does not list (a `-wal` beside a
/// database) is refused by `--verify`, which would open it through SQLite.
#[test]
fn a_fifo_or_device_in_a_backup_is_refused_at_once() {
    let (dir, root, b) = a_backup();
    let blob = a_blob(&b);
    for (i, victim) in ["api-token", "chat.db", blob.as_str()].iter().enumerate() {
        for kind in ["fifo", "device"] {
            let c = copy_of(&root, &b);
            if kind == "fifo" {
                fifo(&c.join(victim));
            } else if !device(&c.join(victim)) {
                continue;
            }
            let o = run_within(
                &root,
                &["backup", "--verify", c.to_str().unwrap()],
                QUICK,
                &format!("backup --verify with a {kind} as {victim}"),
            );
            refused(o, victim);
            let t = root.join(format!("{kind}-{i}"));
            let o = run_within(
                &t,
                &["restore", c.to_str().unwrap()],
                QUICK,
                &format!("restore with a {kind} as {victim}"),
            );
            refused(o, victim);
            empty_but_lock(&t);
        }
    }
    let c = copy_of(&root, &b);
    fifo(&c.join("master.db-wal"));
    let o = run_within(
        &root,
        &["backup", "--verify", c.to_str().unwrap()],
        QUICK,
        "backup --verify with a FIFO as master.db-wal",
    );
    refused(o, "master.db-wal");
    drop(dir);
}

/// A FIFO (or a device) in the data directory in place of a file a backup
/// reads: refused at once, naming it, nothing kept.
#[test]
fn a_fifo_or_device_in_the_data_directory_is_refused_at_once() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    drop(real::serve(&data));
    for (i, victim) in [
        "config.toml",
        "keys/a.key",
        "agents/a.md",
        "master.db-wal",
        "chat.db",
    ]
    .iter()
    .enumerate()
    {
        for kind in ["fifo", "device"] {
            let d = copy_of(&root, &data);
            std::fs::create_dir_all(d.join("keys")).unwrap();
            std::fs::create_dir_all(d.join("agents")).unwrap();
            if kind == "fifo" {
                fifo(&d.join(victim));
            } else if !device(&d.join(victim)) {
                continue;
            }
            let dest = root.join(format!("b-{kind}-{i}"));
            let o = run_within(
                &d,
                &["backup", dest.to_str().unwrap()],
                QUICK,
                &format!("backup with a {kind} as {victim}"),
            );
            refused(o, victim);
            assert!(!dest.exists(), "{kind} {victim}: left {}", dest.display());
            assert!(!d.join("backup-staging").exists());
        }
    }
    drop(dir);
}

/// A directory holding `RESTORING` is refused by every command that reads
/// or writes a data directory, before it touches anything (no
/// `backup.lock`, no destination); `doctor` reports it.
#[test]
fn every_command_refuses_a_half_restored_directory() {
    let (dir, root, b) = a_backup();
    let t = root.join("half");
    ok(run(&t, &["restore", b.to_str().unwrap()]));
    std::fs::write(t.join("RESTORING"), b"").unwrap();
    let before: Vec<_> = {
        let mut v: Vec<_> = std::fs::read_dir(&t)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let dest = root.join("from-half");
    for args in [
        vec!["backup", dest.to_str().unwrap()],
        vec!["token"],
        vec!["claim-code"],
        vec!["graphify", "status"],
        vec!["node", "run"],
    ] {
        refused(run(&t, &args), "holds an interrupted restore");
    }
    refused(serve_refused(&t), "holds an interrupted restore");
    assert!(!dest.exists());
    let mut after: Vec<_> = std::fs::read_dir(&t)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    after.sort();
    assert_eq!(before, after, "a refused command changed the directory");
    let o = run(&t, &["doctor"]);
    assert!(text(&o).contains("interrupted restore"), "{}", text(&o));
    drop(dir);
}

/// A restore killed halfway: the server refuses the directory with advice;
/// following the advice word for word (delete everything in it, `lock`
/// included) and restoring again works, and the server starts.
#[test]
fn the_advice_after_an_interrupted_restore_works() {
    let (dir, root, b) = a_backup();
    let t = root.join("half");
    let pause = root.join("rgo");
    let mut c = cmd(&t, &["restore", b.to_str().unwrap()]);
    c.env("HARNESS_RESTORE_PAUSE_FILE", &pause)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = c.spawn().unwrap();
    let mark = PathBuf::from(format!("{}.snapshotted", pause.display()));
    let by = Instant::now() + DEADLINE;
    while !mark.exists() {
        assert!(Instant::now() < by, "the restore never reached its pause");
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(t.join("RESTORING").exists());
    let why = refused(serve_refused(&t), "holds an interrupted restore");
    assert!(
        why.contains("delete everything in it (lock and backup.lock too)"),
        "{why}"
    );
    for e in std::fs::read_dir(&t).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            std::fs::remove_dir_all(p).unwrap();
        } else {
            std::fs::remove_file(p).unwrap();
        }
    }
    ok(run(&t, &["restore", b.to_str().unwrap()]));
    let s = real::serve(&t);
    let rt = tokio::runtime::Runtime::new().unwrap();
    assert!(rt.block_on(s.get("/api/sessions"))["sessions"].is_array());
    drop((s, dir));
}

fn hashes(dir: &Path) -> std::collections::BTreeMap<String, String> {
    use sha2::Digest;
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(dir).unwrap().display().to_string();
            // The lock files carry the holder's hint (not data).
            if rel == "lock" || rel == "backup.lock" {
                continue;
            }
            out.insert(
                rel,
                hex_of(&sha2::Sha256::digest(std::fs::read(&p).unwrap())),
            );
        }
    }
    out
}

/// An offline backup of a killed server's directory reads only: every file
/// of it (the `-wal` and `-shm` too) is byte-identical afterwards, and the
/// backup holds a row that was only in the WAL.
#[tokio::test(flavor = "multi_thread")]
async fn an_offline_backup_changes_nothing_and_keeps_the_wal() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let sid = chat(&s, "a chat").await;
    let title = "a title only the WAL holds";
    let r = s
        .raw(
            reqwest::Method::PUT,
            &format!("/api/sessions/{sid}/title"),
            Some(json!({"title": title})),
        )
        .await;
    assert!(r.0.is_success(), "{r:?}");
    drop(s); // SIGKILL: the WAL stays as a crash leaves it
    assert!(std::fs::metadata(data.join("master.db-wal")).unwrap().len() > 0);
    // Without its WAL the main file does not have the title.
    let alone = root.join("alone.db");
    std::fs::copy(data.join("master.db"), &alone).unwrap();
    let c = rusqlite::Connection::open(&alone).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sessions WHERE title = ?1",
            [title],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        n, 0,
        "the title is in the main file already: no WAL-only row"
    );
    let before = hashes(&data);
    let dest = root.join("backup");
    let out = ok(run(&data, &["backup", dest.to_str().unwrap()]));
    assert!(out.contains("no server was running"), "{out}");
    assert_eq!(
        before,
        hashes(&data),
        "the offline backup changed its source"
    );
    let c = rusqlite::Connection::open(dest.join("master.db")).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sessions WHERE title = ?1",
            [title],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "the backup lost the WAL's row");
    drop(dir);
}

/// A database of a newer build: refused before anything is opened to copy
/// it, and the source is byte-identical.
#[test]
fn a_newer_database_is_refused_with_the_source_untouched() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    drop(real::serve(&data));
    let c = rusqlite::Connection::open(data.join("master.db")).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let before = hashes(&data);
    let dest = root.join("b");
    refused(
        run(&data, &["backup", dest.to_str().unwrap()]),
        "written by a newer rizzy-harness",
    );
    assert_eq!(before, hashes(&data));
    drop(dir);
}

/// A chat deleted between the copy of master.db and the node logs: it is
/// not in the restored server's list, and opening it says it is unknown.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_deleted_during_the_backup_is_not_restored() {
    let (dir, data, _ws) = real::setup();
    let root = data.parent().unwrap().to_path_buf();
    let mut s = real::serve(&data);
    s.as_member("ana").await;
    let gone = chat(&s, "deleted meanwhile").await;
    let kept = chat(&s, "kept").await;
    let dest = root.join("backup");
    let pause = root.join("go");
    let b = paused_backup(&data, &dest, &pause);
    let (st, _) = s
        .raw(
            reqwest::Method::DELETE,
            &format!("/api/sessions/{gone}"),
            None,
        )
        .await;
    assert!(st.is_success(), "{st}");
    ok(release(b, &pause));
    let (_d2, r, _) = restore_and_serve(&root, "restored", &dest, "ana").await;
    let listed = r.get("/api/sessions").await.to_string();
    assert!(listed.contains(&kept), "{listed}");
    assert!(
        !listed.contains(&gone),
        "a deleted chat came back: {listed}"
    );
    drop((s, r, dir));
}

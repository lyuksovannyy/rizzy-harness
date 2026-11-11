//! A sub-agent's shell in its worktree, through the real `rizzy-harness
//! sandbox-exec` wrapper and the real tools (R.1, S-1 and S-2 of
//! docs/security-review/2026-11-gate3.md). On a kernel without Landlock the
//! tests skip with a printed reason.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::{Bash, Grep, Read, Tool, ToolCtx, Write};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A data directory, a fake home with an ssh key, and a project folder that
/// is a git repository with one sub-agent worktree, made the way
/// `Session::isolate` makes it (`<repo>/.git/harness-worktrees/<id>`).
struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    home: PathBuf,
    repo: PathBuf,
    worktree: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let home = root.join("home");
    let repo = home.join("proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(home.join(".ssh/id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-qm", "base"]);
    let worktree = repo.join(".git/harness-worktrees/child1");
    git(&[
        "worktree",
        "add",
        "--detach",
        "-q",
        worktree.to_str().unwrap(),
    ]);
    Fixture {
        _dir: dir,
        data,
        home,
        repo,
        worktree,
    }
}

impl Fixture {
    fn sandbox(&self) -> Arc<Sandbox> {
        Arc::new(
            Sandbox::new(
                PathBuf::from(EXE),
                &Settings::default(),
                Some(self.data.join("sandbox-tmp")),
                std::slice::from_ref(&self.data),
            )
            .unwrap(),
        )
    }

    fn ctx(&self, cwd: &Path) -> ToolCtx {
        let mut ctx = ToolCtx::new(cwd);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: self.sandbox(),
            tmp: None,
        }
        .for_session("s1");
        ctx
    }

    async fn bash(&self, cwd: &Path, cmd: &str) -> String {
        Bash.run(&self.ctx(cwd), json!({ "command": cmd }))
            .await
            .output
    }

    /// Whether Landlock works here (else the test skips).
    fn landlock(&self) -> bool {
        match self
            .sandbox()
            .probe(&self.data.join("sandbox-tmp/probe"), &self.data)
        {
            Ok(_) => true,
            Err(e) => {
                eprintln!("SKIPPED: Landlock does not work here: {e}");
                false
            }
        }
    }
}

/// S-1: inside the real Landlock sandbox, a sub-agent's shell cannot write
/// the main repository's `.git/config`, its hooks or `info/attributes`: the
/// child's grant on the main `.git` is read-only except for `objects` and
/// its own `worktrees/<id>`.
#[tokio::test]
async fn a_sandboxed_sub_agent_cannot_write_the_shared_git_config() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let g = f.repo.join(".git");
    let out = f
        .bash(
            &f.worktree,
            &format!(
                "export HOME={home} GIT_CONFIG_GLOBAL=/dev/null; \
                 git config filter.x.clean 'sh -c \"id > /tmp/x; cat\"'; echo config=$?; \
                 echo x > {g}/hooks/pre-commit; echo hook=$?; \
                 echo '* filter=x' > {g}/info/attributes; echo attr=$?; \
                 echo x > {g}/packed-refs; echo refs=$?",
                home = f.home.display(),
                g = g.display()
            ),
        )
        .await;
    for step in ["config", "hook", "attr", "refs"] {
        assert!(!out.contains(&format!("{step}=0")), "{step}: {out}");
    }
    let config = std::fs::read_to_string(g.join("config")).unwrap();
    assert!(!config.contains("[filter \"x\"]"), "{config}");
    assert!(!g.join("hooks/pre-commit").exists());
    assert!(!g.join("info/attributes").exists());
}

/// S-2: the child cannot move its own folder away or put a link in its
/// place (the directory that holds its worktree is not writable to it), so
/// the file tools and the next shell stay in the worktree, and the user's
/// key stays out of reach.
#[tokio::test]
async fn a_sandboxed_sub_agent_cannot_point_its_folder_at_another_one() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let wt = &f.worktree;
    let victim = f.home.join(".ssh");
    let out = f
        .bash(
            wt,
            &format!(
                "cd / && mv {w} {w}.r; echo mv=$?; ln -s {v} {w}.l; echo ln=$?; \
                 cat {v}/id_ed25519; echo cat=$?",
                w = wt.display(),
                v = victim.display()
            ),
        )
        .await;
    for step in ["mv", "ln", "cat"] {
        assert!(!out.contains(&format!("{step}=0")), "{step}: {out}");
    }
    assert!(!out.contains("SSH-KEY-CANARY"), "{out}");
    assert!(wt.is_dir() && !wt.is_symlink());
    let r = Read.run(&f.ctx(wt), json!({ "path": "id_ed25519" })).await;
    assert!(!r.output.contains("SSH-KEY-CANARY"), "{}", r.output);
}

/// S-2, when someone else replaces the folder (here the test, as a process
/// of the parent's left running in the parent's folder could): with the
/// session's pin, `read`, `write`, `grep` and the next shell command all
/// refuse, and nothing of the other folder is read or written.
#[tokio::test]
async fn the_file_tools_and_the_shell_refuse_a_replaced_folder() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let wt = &f.worktree;
    let victim = f.home.join(".ssh");
    let pin = harness_core::folder::FolderPin::at(wt).unwrap();
    std::fs::rename(wt, wt.with_extension("r")).unwrap();
    std::os::unix::fs::symlink(&victim, wt).unwrap();
    let mut ctx = f.ctx(wt);
    ctx.pin = Some(pin);
    let r = Read.run(&ctx, json!({ "path": "id_ed25519" })).await;
    assert!(
        !r.ok && r.output.contains("folder was replaced"),
        "{}",
        r.output
    );
    let w = Write
        .run(
            &ctx,
            json!({ "path": "authorized_keys", "content": "attacker-key\n" }),
        )
        .await;
    assert!(
        !w.ok && w.output.contains("folder was replaced"),
        "{}",
        w.output
    );
    assert!(!victim.join("authorized_keys").exists());
    let g = Grep.run(&ctx, json!({ "pattern": "CANARY" })).await;
    assert!(!g.output.contains("SSH-KEY-CANARY"), "{}", g.output);
    let b = Bash
        .run(
            &ctx,
            json!({ "command": "cat id_ed25519; echo x > planted" }),
        )
        .await;
    assert!(b.output.contains("folder was replaced"), "{}", b.output);
    assert!(!b.output.contains("SSH-KEY-CANARY"), "{}", b.output);
    assert!(!victim.join("planted").exists());
}

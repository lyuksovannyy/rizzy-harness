//! A sub-agent's shell in its node-owned folder, through the real
//! `rizzy-harness sandbox-exec` wrapper and the real tools (R.1: S-1, S-2
//! of docs/security-review/2026-11-gate3.md, R1-1 to R1-3 of
//! docs/security-review/2026-11-gate3-fixes.md). On a kernel without
//! Landlock the tests skip with a printed reason.
//!
//! The child's folder is laid out as the node lays it out
//! (`git::SubRepo::make_folder`): `<data>/sessions/<id>/files`, a copy of
//! the parent's folder with a git directory of its own whose alternates
//! name the user's object store, and the grants are those the node passes
//! (`harness_core::folder::OwnedFolder`).

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::folder::OwnedFolder;
use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::{Bash, Grep, Read, Tool, ToolCtx, Write};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    home: PathBuf,
    /// The parent's repository.
    repo: PathBuf,
    /// Another repository of the same user.
    other: PathBuf,
    /// The child's folder, `<data>/sessions/child1/files`.
    child: PathBuf,
    /// A sibling child's folder.
    sibling: PathBuf,
    /// The child's folder as the node passes it: identities taken once,
    /// when the folder was made and the object store accepted (R1c-1).
    owned: OwnedFolder,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A child folder as the node makes it, for the repository `repo`.
fn child_folder(repo: &Path, files: &Path) {
    std::fs::create_dir_all(files).unwrap();
    std::fs::copy(repo.join("a.txt"), files.join("a.txt")).unwrap();
    let g = files.join(".git");
    for d in [
        "objects/info",
        "objects/pack",
        "refs/heads",
        "refs/tags",
        "info",
    ] {
        std::fs::create_dir_all(g.join(d)).unwrap();
    }
    let head = git(repo, &["rev-parse", "HEAD"]);
    std::fs::write(g.join("HEAD"), head).unwrap();
    std::fs::write(
        g.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n",
    )
    .unwrap();
    std::fs::write(
        g.join("objects/info/alternates"),
        format!("{}\n", repo.join(".git/objects").display()),
    )
    .unwrap();
    git(files, &["read-tree", "HEAD"]);
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let home = root.join("home");
    let repo = home.join("proj");
    let other = home.join("other");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(home.join(".ssh/id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
    git(&other, &["init", "-q"]);
    std::fs::write(other.join("s.txt"), "OTHER-REPOSITORY-SECRET\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "other"]);
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-qm", "base"]);
    let child = data.join("sessions/child1/files");
    let sibling = data.join("sessions/child2/files");
    child_folder(&repo, &child);
    child_folder(&repo, &sibling);
    std::fs::write(sibling.join("sibling.txt"), "SIBLING-WORK\n").unwrap();
    let id = |p: &Path| harness_core::folder::Identity::of(p).unwrap();
    let owned = OwnedFolder {
        top: child.clone(),
        top_id: id(&child),
        git: child.join(".git"),
        git_id: id(&child.join(".git")),
        objects: Some((repo.join(".git/objects"), id(&repo.join(".git/objects")))),
    };
    Fixture {
        _dir: dir,
        data,
        home,
        repo,
        other,
        child,
        sibling,
        owned,
    }
}

impl Fixture {
    /// The sandbox as a node sets it up: inside the shell's containment
    /// where bubblewrap works (T.8a), so every test here also checks that
    /// the identity grants of a sub-agent hold inside bubblewrap.
    fn sandbox(&self) -> Arc<Sandbox> {
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            &Settings::default(),
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap();
        Arc::new(
            match bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain")) {
                Ok(sb) | Err(sb) => sb,
            },
        )
    }

    /// A session in `cwd`; with `owned`, a sub-agent in its own folder.
    fn ctx(&self, cwd: &Path, owned: Option<&Path>) -> ToolCtx {
        let mut ctx = ToolCtx::new(cwd);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: self.sandbox(),
            tmp: None,
        }
        .for_session("s1");
        ctx.owned = owned.map(|top| {
            assert_eq!(top, self.owned.top);
            self.owned.clone()
        });
        ctx.pin = Some(harness_core::folder::FolderPin::at(cwd).unwrap());
        ctx
    }

    async fn bash(&self, cwd: &Path, owned: Option<&Path>, cmd: &str) -> String {
        Bash.run(&self.ctx(cwd, owned), json!({ "command": cmd }))
            .await
            .output
    }

    async fn child(&self, cmd: &str) -> String {
        let cmd = format!(
            "export HOME={} GIT_CONFIG_GLOBAL=/dev/null; {cmd}",
            self.home.display()
        );
        self.bash(&self.child, Some(&self.child), &cmd).await
    }

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

/// A sub-agent's shell goes through the same path as its parent's: it is
/// contained too (its own PID namespace, `$$` is 2), started by the node
/// like every other command, never nested in the parent's namespace.
#[tokio::test]
async fn a_sub_agents_shell_is_contained() {
    let f = fixture();
    if !f.landlock() || f.sandbox().containment().is_none() {
        return;
    }
    let out = f
        .child("echo self=$$; echo pids=$(ls -d /proc/[0-9]* | wc -l)")
        .await;
    assert!(out.contains("self=2"), "{out}");
}

/// The child's ordinary git works in its own folder: `status`, `add`,
/// `commit`, `diff`, and `log` with the user's history (read-only through
/// its alternates).
#[tokio::test]
async fn a_sub_agents_git_works_in_its_own_folder() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let out = f
        .child(
            "echo b > b.txt && git status --short && git add b.txt && \
             git -c user.name=a -c user.email=a@a commit -qm sub && \
             git log --oneline | wc -l && git diff HEAD~1 --stat && echo end",
        )
        .await;
    assert!(out.contains("?? b.txt"), "{out}");
    assert!(
        out.lines().any(|l| l.trim() == "2"),
        "commit or log failed: {out}"
    );
    assert!(out.contains("b.txt | 1 +") && out.contains("end"), "{out}");
}

/// S-1 and R1-3: the child cannot write anything under the user's `.git`
/// (config, hooks, `info/attributes`, refs, its object store); its grant
/// there is read-only `objects` at most.
#[tokio::test]
async fn a_sub_agent_cannot_write_anything_in_the_users_git_directory() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let g = f.repo.join(".git");
    let out = f
        .child(&format!(
            "echo x >> {g}/config; echo config=$?; \
             echo x > {g}/hooks/pre-commit; echo hook=$?; \
             echo '* filter=x' > {g}/info/attributes; echo attr=$?; \
             echo x > {g}/packed-refs; echo refs=$?; \
             mkdir {g}/objects/zz; echo objects=$?; \
             rm -rf {g}/objects/pack; echo rmobjects=$?; \
             cat {g}/config >/dev/null; echo readconfig=$?",
            g = g.display()
        ))
        .await;
    for step in [
        "config",
        "hook",
        "attr",
        "refs",
        "objects",
        "rmobjects",
        "readconfig",
    ] {
        assert!(!out.contains(&format!("{step}=0")), "{step}: {out}");
    }
    assert!(!g.join("objects/zz").exists() && !g.join("hooks/pre-commit").exists());
}

/// R1-1: the child reaches no other repository, through its alternates or
/// otherwise; and S-2/R1-2: it cannot move its own folder away, and no
/// other session reaches it (the parent, a sibling child).
#[tokio::test]
async fn a_sub_agents_folder_is_its_own_and_nothing_else_is() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    // The child points its own git at the other repository: read refused.
    let out = f
        .child(&format!(
            "echo {o}/.git/objects > .git/objects/info/alternates; \
             git --no-pager log --all --oneline 2>&1; git show $(git -C {o} rev-parse HEAD 2>/dev/null || echo x):s.txt 2>&1; \
             cat {o}/s.txt; echo cat=$?; cat {s}/sibling.txt; echo sib=$?; \
             echo x > {s}/planted; echo sibw=$?; \
             cd / && mv {c} {c}.r; echo mv=$?",
            o = f.other.display(),
            s = f.sibling.display(),
            c = f.child.display()
        ))
        .await;
    assert!(!out.contains("OTHER-REPOSITORY-SECRET"), "{out}");
    assert!(!out.contains("SIBLING-WORK"), "{out}");
    for step in ["cat", "sib", "sibw", "mv"] {
        assert!(!out.contains(&format!("{step}=0")), "{step}: {out}");
    }
    assert!(f.child.is_dir() && !f.sibling.join("planted").exists());
    // The parent, in its repository: the child's folder does not exist for
    // it, and it cannot rename its own folder (its parent is not granted).
    let out = f
        .bash(
            &f.repo,
            None,
            &format!(
                "ls {c}; echo ls=$?; echo x > {c}/planted; echo w=$?; \
                 cd / && mv {r} {r}.r; echo mvself=$?",
                c = f.child.display(),
                r = f.repo.display()
            ),
        )
        .await;
    for step in ["ls", "w", "mvself"] {
        assert!(!out.contains(&format!("{step}=0")), "{step}: {out}");
    }
    assert!(!f.child.join("planted").exists() && f.repo.is_dir());
    // The file tools: the child reads its own folder, not a sibling's.
    let ctx = f.ctx(&f.child, Some(&f.child));
    let r = Read.run(&ctx, json!({ "path": "a.txt" })).await;
    assert!(r.ok, "{}", r.output);
    let g = Grep.run(&ctx, json!({ "pattern": "^a$" })).await;
    assert!(g.output.contains("a.txt"), "{}", g.output);
    let r = Read
        .run(
            &ctx,
            json!({ "path": f.sibling.join("sibling.txt").display().to_string() }),
        )
        .await;
    assert!(!r.ok && !r.output.contains("SIBLING-WORK"), "{}", r.output);
    // A parent's file tools cannot reach a child's folder either.
    let ctx = f.ctx(&f.repo, None);
    let r = Read
        .run(
            &ctx,
            json!({ "path": f.child.join("a.txt").display().to_string() }),
        )
        .await;
    assert!(!r.ok, "{}", r.output);
}

/// S-2, when someone else replaces a session's folder: with the session's
/// pin, `read`, `write`, `grep` and the next shell command all refuse, and
/// nothing of the other folder is read or written.
#[tokio::test]
async fn the_file_tools_and_the_shell_refuse_a_replaced_folder() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let ws = f.repo.join("sub");
    std::fs::create_dir(&ws).unwrap();
    let victim = f.home.join(".ssh");
    let ctx = f.ctx(&ws, None);
    std::fs::rename(&ws, ws.with_extension("r")).unwrap();
    std::os::unix::fs::symlink(&victim, &ws).unwrap();
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

/// R1c-1, inverted from the fix review's proof: a process the parent left
/// running replaces the user's object store, or the `.git` that holds it,
/// by a link to the user's `~/.ssh` or to the node's data directory. The
/// child's next commands are never granted the link's target: the node
/// leaves out a store that is not the one it accepted (the child loses
/// history), and the wrapper checks the identity on the descriptor it
/// opens.
#[tokio::test]
async fn a_replaced_object_store_is_never_granted_to_a_child() {
    for (what, target) in [
        ("objects", "ssh"),
        ("objects", "data"),
        (".git", "ssh"),
        (".git", "data"),
    ] {
        let f = fixture();
        if !f.landlock() {
            return;
        }
        let key = f.home.join(".ssh/id_ed25519");
        std::fs::write(f.data.join("node-key"), "NODE-KEY-CANARY\n").unwrap();
        let out = f.child(&format!("cat {}; echo rc=$?", key.display())).await;
        assert!(!out.contains("SSH-KEY-CANARY"), "{out}");
        let dest = match target {
            "ssh" => f.home.join(".ssh"),
            _ => f.data.clone(),
        };
        // The parent's own shell may do this in its folder; for `.git`, the
        // fake one beside it is made by the test (outside the parent's grant).
        let cmd = if what == "objects" {
            format!(
                "mv .git/objects .git/objects.r && ln -s {} .git/objects; echo rc=$?",
                dest.display()
            )
        } else {
            let evil = f.repo.parent().unwrap().join("evil");
            std::fs::create_dir(&evil).unwrap();
            std::os::unix::fs::symlink(&dest, evil.join("objects")).unwrap();
            "mv .git .git.r && ln -s ../evil .git; echo rc=$?".to_string()
        };
        let out = f.bash(&f.repo, None, &cmd).await;
        assert!(out.contains("rc=0"), "{what}->{target}: {out}");
        let out = f
            .child(&format!(
                "cat {k}; cat {d}/node-key; ls {r}/.git/objects/; echo end",
                k = key.display(),
                d = f.data.display(),
                r = f.repo.display()
            ))
            .await;
        assert!(out.contains("end"), "{what}->{target}: {out}");
        assert!(
            !out.contains("SSH-KEY-CANARY") && !out.contains("NODE-KEY-CANARY"),
            "{what}->{target}: the child read the link's target: {out}"
        );
        // Nor listed it (`ls` of the replaced store prints no names).
        assert!(
            !out.lines().any(|l| l == "id_ed25519" || l == "node-key"),
            "{what}->{target}: {out}"
        );
    }
}

/// The wrapper itself refuses a grant whose folder is not the one the node
/// expects, naming it, and runs nothing: here the session's folder,
/// replaced between the node's pin check and the wrapper's open.
#[tokio::test]
async fn the_wrapper_refuses_a_grant_that_was_replaced_and_names_it() {
    use harness_tools::sandbox::Grant;
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let ws = f.repo.join("sub");
    std::fs::create_dir(&ws).unwrap();
    let pin = harness_core::folder::FolderPin::at(&ws).unwrap();
    pin.check().unwrap();
    // The swap, after the check.
    std::fs::rename(&ws, ws.with_extension("r")).unwrap();
    std::os::unix::fs::symlink(f.home.join(".ssh"), &ws).unwrap();
    let out = f
        .sandbox()
        .helper(
            &[Grant::identified(
                &ws,
                pin.identity(),
                "the session's folder",
            )],
            &[],
        )
        .arg("true")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    // 125: the wrapper refused before running anything.
    assert_eq!(out.status.code(), Some(125), "{err}");
    assert!(
        err.contains("the folder granted as the session's folder was replaced"),
        "{err}"
    );
    // The same grant, not replaced: it runs.
    std::fs::remove_file(&ws).unwrap();
    std::fs::rename(ws.with_extension("r"), &ws).unwrap();
    let out = f
        .sandbox()
        .helper(
            &[Grant::identified(
                &ws,
                pin.identity(),
                "the session's folder",
            )],
            &[],
        )
        .arg("sh")
        .arg("-c")
        .arg(format!("touch {}/ok", ws.display()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ws.join("ok").exists());
}

// ---- Through the node's real delegation (the owned folder reaches the
// child's tools through the agent loop) ----

/// Calls what its latest user message lists, one per request (a JSON
/// array of `[tool, input]`), then answers "done". Parent and child alike:
/// the child's task is its user message.
struct Script;

#[async_trait::async_trait]
impl harness_agent_native::Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &harness_agent_native::ChatRequest,
        _: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<harness_agent_native::Completion, harness_agent_native::ProviderError> {
        use harness_agent_native::ChatMsg;
        let since = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .unwrap_or(0);
        let user = req.messages[since].user_text().unwrap_or("");
        let calls: Vec<(String, serde_json::Value)> = user
            .find('[')
            .zip(user.rfind(']'))
            .and_then(|(a, b)| serde_json::from_str(&user[a..=b]).ok())
            .unwrap_or_default();
        let done = req.messages[since + 1..]
            .iter()
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        Ok(harness_agent_native::Completion {
            text: "done".into(),
            tool_calls: calls
                .get(done)
                .map(|(tool, input)| harness_core::event::ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: tool.clone(),
                    input: input.clone(),
                })
                .into_iter()
                .collect(),
            usage: None,
            stop: Default::default(),
        })
    }
}

fn spec(
    id: &str,
    tools: &[&str],
    delegates: Vec<harness_core::agent::AgentSpec>,
) -> harness_core::agent::AgentSpec {
    harness_core::agent::AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(
            tools.iter().map(|t| t.to_string()).collect(),
        ),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        budget: Default::default(),
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates,
        caps: None,
    }
}

/// T32 end to end, on a node set up as the binary sets one up: the data
/// directory protected, every shell command through the real
/// `sandbox-exec` inside bubblewrap where it works. A sub-agent that can
/// change files is delegated to by a real turn; its `write` and its `bash`
/// work in its own folder under `<data>/sessions`, its changes are merged
/// back into the parent's folder, and its shell still reaches neither the
/// user's `~/.ssh`, nor the node's data directory, nor a sibling's folder,
/// nor the parent's folder for writing.
#[tokio::test]
async fn a_delegated_sub_agent_gets_its_folder_through_the_node() {
    use harness_core::event::EventKind;
    use harness_protocol::{MasterToNode, NodeToMaster};
    let f = fixture();
    if !f.landlock() {
        return;
    }
    std::fs::write(f.data.join("node-key"), "NODE-KEY-CANARY\n").unwrap();
    let sandbox = f.sandbox();
    let mut policy = harness_node::NodePolicy {
        workspace_roots: vec![f.home.clone()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        ..Default::default()
    };
    policy.protect(&f.data);
    let backend = harness_agent_native::NativeBackend::new(
        Arc::new(Script),
        harness_tools::builtin(),
        harness_agent_native::NativeConfig {
            protected_dirs: Arc::new(policy.protected_dirs.clone()),
            sandbox: Confinement::Confined {
                sandbox: sandbox.clone(),
                tmp: None,
            },
            ..Default::default()
        },
    );
    let node = harness_node::NodeRuntime::start(harness_node::NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store: harness_store::Store::open(&f.data.join("node.db"), harness_store::Durability::Full)
            .unwrap(),
        backend: Arc::new(backend),
        default_model: "script".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(f.data.join("checkpoints")),
        git_sandbox: Some(sandbox.clone()),
        artifacts: None,
    })
    .await
    .unwrap();
    let mut link = node.attach().await;
    let mut seen: Vec<harness_core::RawEvent> = vec![];
    let sid = harness_core::SessionId::new();
    link.to_node
        .try_send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: f.repo.display().to_string(),
            model: None,
            approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
        })
        .unwrap();
    let shell = format!(
        "pwd; echo b > a.txt; cat made.txt; \
         cat {home}/.ssh/id_ed25519; echo ssh=$?; cat {data}/node-key; echo key=$?; \
         cat {sib}/sibling.txt; echo sib=$?; echo x > {repo}/planted; echo par=$?; \
         ls {data} >/dev/null 2>&1; echo lsdata=$?; ls {home}; echo view=$(ls / | tr '\\n' ' ')",
        home = f.home.display(),
        data = f.data.display(),
        sib = f.sibling.display(),
        repo = f.repo.display(),
    );
    let child_calls = json!([
        ["write", {"path": "made.txt", "content": "by the child\n"}],
        ["bash", {"command": shell}],
    ]);
    let task = json!([["delegate", {"agent": "dev", "task": child_calls.to_string()}]]);
    link.to_node
        .try_send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: Some(spec(
                "lead",
                &["bash"],
                vec![spec("dev", &["write", "bash"], vec![])],
            )),
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text: task.to_string(),
            images: vec![],
            agent_named: false,
        })
        .unwrap();
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(60), link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed");
        match msg {
            NodeToMaster::Event { event } => {
                let parent_ended =
                    event.session_id == sid && matches!(event.typed(), Ok(EventKind::TurnEnded(_)));
                seen.push(event);
                if parent_ended {
                    break;
                }
            }
            NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
            _ => {}
        }
    }
    let finished = |session: Option<harness_core::SessionId>| -> Vec<(bool, String)> {
        seen.iter()
            .filter(|e| match session {
                Some(s) => e.session_id == s,
                None => e.session_id != sid,
            })
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ToolCallFinished(r)) => Some((r.ok, r.output)),
                _ => None,
            })
            .collect()
    };
    let child = finished(None);
    let parent = finished(Some(sid));
    eprintln!("child: {child:#?}\nparent: {parent:#?}");
    assert_eq!(child.len(), 2, "{child:#?} / {parent:#?}");
    assert!(child[0].0, "the child's write was refused: {}", child[0].1);
    let out = &child[1].1;
    let sessions = f.data.join("sessions").display().to_string();
    assert!(
        out.lines()
            .next()
            .is_some_and(|l| l.starts_with(&sessions) && l.ends_with("/files")),
        "the child's shell is not in its own folder: {out}"
    );
    assert!(out.contains("by the child"), "{out}");
    for leak in ["SSH-KEY-CANARY", "NODE-KEY-CANARY", "SIBLING-WORK"] {
        assert!(!out.contains(leak), "{leak}: {out}");
    }
    for step in ["ssh", "key", "sib", "par", "lsdata"] {
        assert!(!out.contains(&format!("{step}=0")), "{step}: {out}");
    }
    assert!(!f.repo.join("planted").exists());
    let merged = &parent.last().expect("the delegate call").1;
    assert!(merged.contains("2 changed file(s) applied"), "{merged}");
    assert_eq!(
        std::fs::read_to_string(f.repo.join("made.txt")).unwrap(),
        "by the child\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "b\n"
    );
}

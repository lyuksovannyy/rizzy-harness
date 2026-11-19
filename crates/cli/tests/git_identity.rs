//! Git works unattended in an agent's shell, with the person's own identity
//! (PLAN.md T88; the 2026-11-19 readiness trial, item 3).
//!
//! Through the real node runtime on a node started as the binary starts
//! one (`sandbox::start_with_exe`, bubblewrap and Landlock), with a fake
//! provider that runs the `run:` command of a prompt. The node's `HOME` is a
//! fake home of the test's own with a `.gitconfig` in it (a developer's real
//! one is never read): in a host chat Landlock denies it, which is what made
//! every git command warn and fail in the trial.
//!
//! Needs bubblewrap; with `HARNESS_REQUIRE_BWRAP=1` a missing one fails.
//! Temp folders are under the target directory.

#[path = "support/bwrap.rs"]
#[allow(dead_code)]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::agent::{AgentSpec, AllTools, ToolSelection};
use harness_core::event::{EventKind, Profile};
use harness_core::identity::CommitIdentity;
use harness_core::policy::{ApprovalMode, ExecMode};
use harness_protocol::{MasterToNode, NodeToMaster};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
/// An address no event, audit line or log line may ever carry.
const CANARY: &str = "canary-8f3a1c@example.org";
const HOME_PERSON: &str = "Home Person|home-person@example.org";

static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static ONCE: Once = Once::new();

#[derive(Clone)]
struct Sink;
impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        LOG.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        Sink
    }
}

/// Every log line this test process wrote, at TRACE.
fn logs() -> String {
    String::from_utf8_lossy(&LOG.lock().unwrap()).into_owned()
}

struct Node {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    rt: harness_node::NodeRuntime,
    link: harness_node::NodeLink,
    next: u32,
}

/// One tool result of a turn, from any of its sessions.
struct Out {
    session: SessionId,
    tool: String,
    text: String,
}

/// A node offering `sandbox` and `host`. `read_home`: node.toml's
/// `[sandbox] read` names the fake home, so the person's `~/.gitconfig` is
/// readable in a host chat.
async fn node(read_home: bool) -> Option<Node> {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    ONCE.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(Sink)
            .with_ansi(false)
            .try_init();
    });
    let dir = tempfile::Builder::new()
        .prefix("git-identity-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (data, home) = (root.join("data"), root.join("home"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    // The person's own configuration: an identity and a setting git needs
    // to read (a pager of `cat`).
    std::fs::write(
        home.join(".gitconfig"),
        "[user]\n\tname = Home Person\n\temail = home-person@example.org\n[core]\n\tpager = cat\n",
    )
    .unwrap();
    let settings = harness_tools::sandbox::Settings {
        read: if read_home {
            vec![home.clone()]
        } else {
            vec![]
        },
        ..Default::default()
    };
    let s = harness_tools::sandbox::start_with_exe(
        PathBuf::from(EXE),
        &settings,
        &data,
        std::slice::from_ref(&data),
        &data.join("node.toml"),
    )
    .unwrap();
    if let Some(why) = s.sandbox_refusal() {
        assert!(!bwrap::required(), "the sandbox mode is not offered: {why}");
        eprintln!("SKIPPED: the sandbox mode is not offered here: {why}");
        return None;
    }
    let mut policy = harness_node::NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: ApprovalMode::Auto,
        exec: harness_node::exec::ExecOffers::new(Ok(()), Ok(()), vec![]),
        sandbox_tmp: s.tmp_root.clone(),
        ..Default::default()
    };
    policy.skills.roots.clear();
    policy.protect(&data);
    let tool_env = harness_tools::ToolEnv::default().with_fixed(&[
        ("HOME", home.to_str().unwrap()),
        ("XDG_CONFIG_HOME", home.join(".xdg").to_str().unwrap()),
    ]);
    let backend = NativeBackend::new(
        Arc::new(Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig {
            sandbox: s.confinement,
            host_shell: s.uncontained,
            protected_dirs: Arc::new(vec![data.clone()]),
            tool_env,
            ..Default::default()
        },
    );
    let rt = harness_node::NodeRuntime::start(harness_node::NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store: harness_store::Store::open(&data.join("node.db"), harness_store::Durability::Full)
            .unwrap(),
        backend: Arc::new(backend),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(data.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut link = rt.attach().await;
    assert!(matches!(
        link.from_node.recv().await.unwrap(),
        NodeToMaster::Hello { .. }
    ));
    Some(Node {
        _dir: dir,
        root,
        data,
        rt,
        link,
        next: 0,
    })
}

/// `git` as the test's own set-up (never the developer's configuration).
fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn alex() -> CommitIdentity {
    CommitIdentity {
        login: "alex".into(),
        name: Some("Alex Example".into()),
        email: Some(CANARY.into()),
    }
}

/// `git commit` with no flags in a new repository, then who it names.
const COMMIT: &str = "run: git init -q -b main . && echo a > a.txt && git add a.txt && \
                      git commit -q -m one && git log -1 --format='%an|%ae|%cn|%ce'";

impl Node {
    async fn ack(&mut self) -> Result<(), String> {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            if let NodeToMaster::Ack { ok, error, .. } = m {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    /// A chat in a new folder of its own, in `mode`; with `repo`, the
    /// folder is a repository with one commit.
    async fn create(&mut self, mode: ExecMode, repo: bool) -> (SessionId, PathBuf) {
        self.next += 1;
        let ws = self.root.join(format!("home/proj{}", self.next));
        std::fs::create_dir_all(&ws).unwrap();
        if repo {
            git(&ws, &["init", "-q", "-b", "main"]);
            std::fs::write(ws.join("lib.rs"), "fn one() {}\n").unwrap();
            git(&ws, &["add", "."]);
            git(&ws, &["commit", "-q", "-m", "base"]);
        }
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
                profile: None,
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode: Some(mode),
            })
            .unwrap();
        self.ack().await.unwrap();
        (sid, ws)
    }

    /// One prompt to its turn's end, as the master sends it for a chat of
    /// `owner` with `identity`; the tool results of every session it made.
    async fn turn(
        &mut self,
        sid: SessionId,
        agent: Option<AgentSpec>,
        owner: Option<&str>,
        identity: Option<CommitIdentity>,
        text: &str,
    ) -> Vec<Out> {
        let known: std::collections::HashSet<SessionId> = self
            .rt
            .store()
            .sessions()
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .filter(|s| *s != sid)
            .collect();
        let from = self.rt.store().events_after(sid, 0, 10_000).unwrap().len();
        self.link
            .to_node
            .try_send(MasterToNode::Prompt {
                req_id: "p".into(),
                session_id: sid,
                text: text.into(),
                agent,
                attachments: vec![],
                compare: vec![],
                reply: None,
                savings: None,
                context: None,
                images: vec![],
                agent_named: false,
                files: vec![],
                owner: owner.map(Into::into),
                identity,
            })
            .unwrap();
        self.ack().await.unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(90);
        loop {
            let log = self.rt.store().events_after(sid, 0, 10_000).unwrap();
            if log.len() > from && log.last().is_some_and(|e| e.kind == "turn_ended") {
                let mut outs = Vec::new();
                for s in self
                    .rt
                    .store()
                    .sessions()
                    .unwrap()
                    .into_iter()
                    .map(|s| s.session_id)
                    .filter(|s| !known.contains(s))
                {
                    let skip = if s == sid { from } else { 0 };
                    let mut names = std::collections::HashMap::new();
                    for e in self
                        .rt
                        .store()
                        .events_after(s, 0, 10_000)
                        .unwrap()
                        .into_iter()
                        .skip(skip)
                    {
                        match e.typed() {
                            Ok(EventKind::ToolCallStarted(c)) => {
                                names.insert(c.call_id, c.tool);
                            }
                            Ok(EventKind::ToolCallFinished(f)) => outs.push(Out {
                                session: s,
                                tool: names.get(&f.call_id).cloned().unwrap_or_default(),
                                text: f.output,
                            }),
                            _ => {}
                        }
                    }
                }
                return outs;
            }
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The text of the turn's `bash` results.
    async fn bash(
        &mut self,
        sid: SessionId,
        owner: Option<&str>,
        identity: Option<CommitIdentity>,
        text: &str,
    ) -> String {
        self.turn(sid, None, owner, identity, text)
            .await
            .into_iter()
            .filter(|o| o.tool == "bash")
            .map(|o| o.text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// What git printed that it must never print for the person: its warning
/// about an unreadable configuration, its failure, its demand for an
/// identity.
fn assert_quiet(out: &str) {
    for bad in [
        "warning",
        "fatal",
        "unable to access",
        "Permission denied",
        "Please tell me who you are",
        "unable to auto-detect",
    ] {
        assert!(!out.contains(bad), "git said {bad:?}: {out}");
    }
}

fn author_line(out: &str) -> String {
    out.lines()
        .find(|l| l.matches('|').count() == 3)
        .unwrap_or_else(|| panic!("no commit line in: {out}"))
        .to_string()
}

/// A sandbox chat: `git commit` with no flags works, and author and
/// committer are the person's name and e-mail. This is the trial's first
/// finding (4 of 4 coding chats failed here).
#[tokio::test]
async fn a_sandbox_chats_commit_carries_the_owners_name_and_email() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let (sid, _) = n.create(ExecMode::Sandbox, false).await;
    let out = n.bash(sid, Some("u-alex"), Some(alex()), COMMIT).await;
    assert_quiet(&out);
    assert_eq!(
        author_line(&out),
        format!("Alex Example|{CANARY}|Alex Example|{CANARY}")
    );
}

/// No e-mail set: the commit still works, with the display name (the login
/// when there is none) and the no-reply address built from the login; a chat
/// nobody owns gets the node's fallback.
#[tokio::test]
async fn without_an_email_commits_carry_the_no_reply_address() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let noreply = |l: &str| format!("{l}@users.noreply.rizzy.invalid");
    let cases = [
        (
            Some(CommitIdentity {
                login: "alex".into(),
                name: Some("Alex Example".into()),
                email: None,
            }),
            format!("Alex Example|{0}|Alex Example|{0}", noreply("alex")),
        ),
        (
            Some(CommitIdentity {
                login: "bea".into(),
                name: None,
                email: None,
            }),
            format!("bea|{0}|bea|{0}", noreply("bea")),
        ),
        // Nobody owns the chat.
        (
            None,
            format!("Harness agent|{0}|Harness agent|{0}", noreply("agent")),
        ),
    ];
    for mode in [ExecMode::Sandbox, ExecMode::Host] {
        for (id, want) in &cases {
            let (sid, _) = n.create(mode, false).await;
            let out = n.bash(sid, Some("u-x"), id.clone(), COMMIT).await;
            assert_quiet(&out);
            assert_eq!(&author_line(&out), want, "{mode:?}");
        }
    }
}

/// The node decides: what a hostile master sends never reaches git. A bad
/// e-mail is replaced by the no-reply form, a bad name by the login, and a
/// login that is not a login by `user`; none of it ends a header or a line
/// of the configuration file.
#[tokio::test]
async fn the_node_checks_what_the_master_sends_again() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let long = format!("{}@example.org", "a".repeat(300));
    let hostile_emails = [
        "a@b.org\nGIT_COMMITTER_NAME: evil".to_string(),
        "Evil <evil@example.org>".to_string(),
        "evil@example.org>".to_string(),
        "two@@example.org".to_string(),
        "sp ace@example.org".to_string(),
        "tab\t@example.org".to_string(),
        "a\u{0}b@example.org".to_string(),
        "ünï@example.org".to_string(),
        "a\"\n[core]\n\tsshCommand = id\n#@example.org".to_string(),
        long,
    ];
    let (sid, ws) = n.create(ExecMode::Sandbox, false).await;
    for e in hostile_emails {
        let id = CommitIdentity {
            login: "alex".into(),
            name: Some("A\nB <x@y.z> \"q\" \\".into()),
            email: Some(e.clone()),
        };
        let out = n.bash(sid, Some("u-alex"), Some(id), COMMIT).await;
        assert_quiet(&out);
        let noreply = "alex@users.noreply.rizzy.invalid";
        assert_eq!(
            author_line(&out),
            format!("alex|{noreply}|alex|{noreply}"),
            "{e:?}"
        );
        // The repository is fresh each time.
        let _ = std::fs::remove_dir_all(ws.join(".git"));
    }
    // A non-ASCII display name is allowed.
    let id = CommitIdentity {
        login: "zoe".into(),
        name: Some("Zoë Łukasiewicz".into()),
        email: None,
    };
    let out = n.bash(sid, Some("u-zoe"), Some(id), COMMIT).await;
    assert!(
        author_line(&out).starts_with("Zoë Łukasiewicz|zoe@"),
        "{out}"
    );
}

/// A host chat: the person's `~/.gitconfig` exists but Landlock denies the
/// shell reading it (the deliberate rule of the host mode, which stays), so
/// before this change every git command printed `warning: unable to access
/// '~/.gitconfig': Permission denied` and the next line `fatal: unknown
/// error occurred while reading the configuration files`. Now git runs
/// quietly, and the commit carries the person's name and e-mail from their
/// account, since their own configuration cannot be read there.
#[tokio::test]
async fn a_host_chats_git_runs_without_the_warning_and_commits_as_the_owner() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let (sid, _) = n.create(ExecMode::Host, false).await;
    let commit = n.bash(sid, Some("u-alex"), Some(alex()), COMMIT).await;
    assert_quiet(&commit);
    assert_eq!(
        author_line(&commit),
        format!("Alex Example|{CANARY}|Alex Example|{CANARY}")
    );
    // Every kind of git command reads the configuration: none warns.
    let out = n
        .bash(
            sid,
            Some("u-alex"),
            Some(alex()),
            "run: git status 2>&1; git log --oneline 2>&1; git branch 2>&1; git diff 2>&1; \
             git config --get user.name 2>&1; echo done",
        )
        .await;
    assert_quiet(&out);
    assert!(
        out.contains("done") && out.contains("Alex Example"),
        "{out}"
    );
}

/// Where the person made their own `~/.gitconfig` readable to the shell
/// (`[sandbox] read`), git reads it and the identity they configured there
/// wins over the account's: the harness's identity is a default.
#[tokio::test]
async fn an_identity_the_person_configured_is_not_overridden() {
    let Some(mut n) = node(true).await else {
        return;
    };
    let (sid, _) = n.create(ExecMode::Host, false).await;
    let out = n.bash(sid, Some("u-alex"), Some(alex()), COMMIT).await;
    assert_quiet(&out);
    let want = HOME_PERSON;
    assert_eq!(author_line(&out), format!("{want}|{want}"));
    // A sandbox chat on the same node reads the home too (it is granted
    // there): the same rule, not a special case of the host mode.
    let (sid, _) = n.create(ExecMode::Sandbox, false).await;
    let out = n.bash(sid, Some("u-alex"), Some(alex()), COMMIT).await;
    assert_eq!(author_line(&out), format!("{want}|{want}"), "{out}");
}

/// An identity set where the agent can set one (the repository's own
/// configuration, `git -c`) wins over the default too, in a sandbox chat.
#[tokio::test]
async fn a_repository_or_command_line_identity_wins_over_the_default() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let (sid, _) = n.create(ExecMode::Sandbox, false).await;
    let out = n
        .bash(
            sid,
            Some("u-alex"),
            Some(alex()),
            "run: git init -q -b main . && git config user.name Local && \
             git config user.email local@example.org && echo a > a && git add a && \
             git commit -q -m one && git log -1 --format='%an|%ae|%cn|%ce'",
        )
        .await;
    assert_quiet(&out);
    assert_eq!(
        author_line(&out),
        "Local|local@example.org|Local|local@example.org"
    );
    let out = n
        .bash(
            sid,
            Some("u-alex"),
            Some(alex()),
            "run: git config --unset user.name; git config --unset user.email; echo b > b && \
             git add b && git -c user.name=Dash -c user.email=dash@example.org commit -q -m two \
             && git log -1 --format='%an|%ae|%cn|%ce'",
        )
        .await;
    assert_eq!(
        author_line(&out),
        "Dash|dash@example.org|Dash|dash@example.org",
        "{out}"
    );
}

fn scout() -> AgentSpec {
    harness_agents::builtins()
        .into_iter()
        .find(|a| a.id == "scout")
        .unwrap()
        .spec
}

/// A lead that may delegate to a worker that changes things.
fn lead() -> AgentSpec {
    let worker = AgentSpec {
        agent_id: "worker".into(),
        name: "Worker".into(),
        tools: ToolSelection::All(AllTools::All),
        ..scout()
    };
    AgentSpec {
        agent_id: "lead".into(),
        name: "Lead".into(),
        tools: ToolSelection::All(AllTools::All),
        delegates: vec![worker],
        caps: Some(Default::default()),
        ..scout()
    }
}

/// A sub-agent commits in its own folder with the identity of the root
/// chat's owner (not its own turn's sender, which is the lead): the chat is
/// the person's, so are its sub-agents' commits.
#[tokio::test]
async fn a_sub_agents_commit_carries_the_root_chats_owner() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let task = "run: echo b > b.txt && git add b.txt && git commit -q -m sub && \
                git log -1 --format='%an|%ae|%cn|%ce'";
    let call = json!([{"tool": "delegate", "input": {"agent": "worker", "task": task}}]);
    for (id, want) in [
        (
            Some(alex()),
            format!("Alex Example|{CANARY}|Alex Example|{CANARY}"),
        ),
        (
            None,
            "Harness agent|agent@users.noreply.rizzy.invalid|Harness agent|agent@users.noreply.rizzy.invalid"
                .to_string(),
        ),
    ] {
        let (sid, _) = n.create(ExecMode::Sandbox, true).await;
        let outs = n
            .turn(sid, Some(lead()), id.as_ref().map(|_| "u-alex"), id, &format!("script: {call}"))
            .await;
        let child: Vec<&Out> = outs
            .iter()
            .filter(|o| o.session != sid && o.tool == "bash")
            .collect();
        assert!(!child.is_empty(), "the worker ran no command");
        let text = child
            .iter()
            .map(|o| o.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_quiet(&text);
        assert_eq!(author_line(&text), want);
    }
}

/// Two people on one node, turns interleaved: each chat commits as its own
/// owner, whoever the node served before.
#[tokio::test]
async fn two_users_on_one_node_each_get_their_own_identity() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let bea = CommitIdentity {
        login: "bea".into(),
        name: Some("Bea Builder".into()),
        email: Some("bea@example.net".into()),
    };
    let (a, _) = n.create(ExecMode::Sandbox, false).await;
    let (b, _) = n.create(ExecMode::Host, false).await;
    for _ in 0..2 {
        let out = n.bash(a, Some("u-alex"), Some(alex()), COMMIT).await;
        assert_eq!(
            author_line(&out),
            format!("Alex Example|{CANARY}|Alex Example|{CANARY}")
        );
        let out = n.bash(b, Some("u-bea"), Some(bea.clone()), COMMIT).await;
        assert_eq!(
            author_line(&out),
            "Bea Builder|bea@example.net|Bea Builder|bea@example.net"
        );
        // Each turn names its chat's owner again: a later turn of A, with a
        // different identity from the master, is what the node uses.
        for ws in ["proj1", "proj2"] {
            let _ = std::fs::remove_dir_all(n.root.join("home").join(ws).join(".git"));
        }
    }
    // The e-mail the person changes in their profile applies from their next
    // turn.
    let changed = CommitIdentity {
        email: Some("alex-new@example.org".into()),
        ..alex()
    };
    let out = n.bash(a, Some("u-alex"), Some(changed), COMMIT).await;
    assert!(out.contains("|alex-new@example.org|"), "{out}");
}

/// The node's own git (checkpoints, snapshots) does not take the person's
/// identity: every commit in its stores is the harness's own, and the
/// person's e-mail is in none of them.
#[tokio::test]
async fn the_nodes_own_checkpoint_commits_are_unchanged() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let (sid, _) = n.create(ExecMode::Sandbox, false).await;
    let out = n.bash(sid, Some("u-alex"), Some(alex()), COMMIT).await;
    assert_eq!(
        author_line(&out),
        format!("Alex Example|{CANARY}|Alex Example|{CANARY}")
    );
    // The checkpoint stores, once the turn's checkpoints are written.
    let by = std::time::Instant::now() + Duration::from_secs(30);
    let mut commits = Vec::new();
    while std::time::Instant::now() < by && commits.is_empty() {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut stack = vec![n.data.join("checkpoints")];
        while let Some(d) = stack.pop() {
            if d.join("HEAD").is_file() && d.join("objects").is_dir() {
                let o = std::process::Command::new("git")
                    .arg("--git-dir")
                    .arg(&d)
                    .args(["log", "--all", "--format=%an|%ae|%cn|%ce"])
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .output()
                    .unwrap();
                commits.extend(
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .map(str::to_string),
                );
                continue;
            }
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(e.path());
                }
            }
        }
    }
    assert!(!commits.is_empty(), "no checkpoint commit was written");
    for c in &commits {
        assert!(
            !c.contains("canary") && !c.contains("Alex Example") && !c.contains("alex"),
            "the node's own commit took the person's identity: {c}"
        );
    }
}

/// The e-mail is personal data: in no event of the chat, no audit line of
/// the node and no log line at TRACE, for a turn that runs `git commit`.
/// (The agent's own shell may show it: `git log` prints it.)
#[tokio::test]
async fn the_email_is_in_no_event_audit_line_or_log_line() {
    let Some(mut n) = node(false).await else {
        return;
    };
    let (sid, _) = n.create(ExecMode::Sandbox, false).await;
    // A commit whose output the agent prints would carry the address into
    // the tool result by the agent's own doing: print only what is not it.
    let out = n
        .bash(
            sid,
            Some("u-alex"),
            Some(alex()),
            "run: git init -q -b main . && echo a > a.txt && git add a.txt && git commit -q -m one \
             && git log -1 --format=%an",
        )
        .await;
    assert!(out.contains("Alex Example"), "{out}");
    let (host, _) = n.create(ExecMode::Host, false).await;
    n.bash(
        host,
        Some("u-alex"),
        Some(alex()),
        "run: git init -q . && git status",
    )
    .await;
    for s in [sid, host] {
        let events =
            serde_json::to_string(&n.rt.store().events_after(s, 0, 10_000).unwrap()).unwrap();
        assert!(!events.contains("canary-8f3a1c"), "an event holds it");
    }
    let audit = n.rt.store().audit_lines("").unwrap();
    assert!(!audit.is_empty(), "no audit line to look in");
    assert!(
        !format!("{audit:?}").contains("canary-8f3a1c"),
        "an audit line holds it"
    );
    // What a trace line prints when a command or an identity is formatted
    // with `Debug`: the sink is alive, and the address is hidden.
    tracing::trace!(identity = ?alex(), "sink alive");
    let log = logs();
    assert!(
        log.contains("sink alive") && log.contains("<hidden>"),
        "no log was captured"
    );
    assert!(!log.contains("canary-8f3a1c"), "a log line holds it");
    // Nor the on-disk logs of the node.
    let mut stack = vec![n.data.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if name != "checkpoints" && name != "sandbox-tmp" {
                    stack.push(p);
                }
            } else if name.ends_with(".log") || name.contains("audit") {
                let text = std::fs::read(&p).unwrap_or_default();
                assert!(
                    !String::from_utf8_lossy(&text).contains("canary-8f3a1c"),
                    "{}",
                    p.display()
                );
            }
        }
    }
}

// ============ review of PR 167 ============

/// The node's peak resident memory so far, in kB.
fn vm_hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

/// A hang fails the run instead of freezing it: a thread of its own (a
/// blocked runtime cannot starve it) ends the process after `secs`.
fn watchdog(secs: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        eprintln!("WATCHDOG: the test hung for {secs} s");
        std::process::exit(101);
    });
}

/// What an agent can plant at the identity file's old, fixed name, in a
/// folder its own commands write.
const PLANTS: [(&str, &str); 4] = [
    ("a FIFO", "mkfifo $TMPDIR/.harness-git-identity"),
    (
        "a link to a 400 MB sparse file",
        "truncate -s 400M $TMPDIR/big && ln -sf $TMPDIR/big $TMPDIR/.harness-git-identity",
    ),
    (
        "a link to /dev/zero",
        "ln -sf /dev/zero $TMPDIR/.harness-git-identity",
    ),
    ("a directory", "mkdir $TMPDIR/.harness-git-identity"),
];

/// The node never opens, reads or stats anything at a name in a folder the
/// agent writes: a FIFO, a link to a huge file or to `/dev/zero`, a
/// directory planted where the identity file used to be changes nothing:
/// the next command returns at once with the right identity, and the node's
/// memory does not jump (the old code read the entry on every `bash` call:
/// a FIFO hung it, 400 MB of file took 400 MB of the node).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_planted_at_the_identity_files_name_hangs_or_bloats_the_node() {
    watchdog(240);
    let Some(mut n) = node(false).await else {
        return;
    };
    let before = vm_hwm_kb();
    for (what, plant) in PLANTS {
        for mode in [ExecMode::Sandbox, ExecMode::Host] {
            let (sid, _) = n.create(mode, false).await;
            // One command to plant, then the commit.
            n.bash(
                sid,
                Some("u-alex"),
                Some(alex()),
                &format!("run: {plant}; echo planted"),
            )
            .await;
            let out = tokio::time::timeout(
                Duration::from_secs(60),
                n.bash(sid, Some("u-alex"), Some(alex()), COMMIT),
            )
            .await
            .unwrap_or_else(|_| panic!("{what} in {mode:?}: the next command did not return"));
            assert_quiet(&out);
            assert_eq!(
                author_line(&out),
                format!("Alex Example|{CANARY}|Alex Example|{CANARY}"),
                "{what} in {mode:?}"
            );
        }
    }
    let grew = vm_hwm_kb().saturating_sub(before);
    assert!(
        grew < 150 * 1024,
        "the node's peak memory grew by {grew} kB"
    );
}

/// Four chats plant a FIFO and run a command at once: a 4-worker runtime
/// is not starved (a blocking read on each would hold all four workers).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_chats_planting_fifos_do_not_starve_the_runtime() {
    watchdog(240);
    let Some(mut n) = node(false).await else {
        return;
    };
    let mut chats = Vec::new();
    for _ in 0..4 {
        let (sid, _) = n.create(ExecMode::Sandbox, false).await;
        n.bash(
            sid,
            Some("u-alex"),
            Some(alex()),
            "run: mkfifo $TMPDIR/.harness-git-identity; echo planted",
        )
        .await;
        chats.push(sid);
    }
    let from: Vec<usize> = chats
        .iter()
        .map(|s| n.rt.store().events_after(*s, 0, 10_000).unwrap().len())
        .collect();
    for sid in &chats {
        n.link
            .to_node
            .try_send(MasterToNode::Prompt {
                req_id: "p".into(),
                session_id: *sid,
                text: COMMIT.into(),
                agent: None,
                attachments: vec![],
                compare: vec![],
                reply: None,
                savings: None,
                context: None,
                images: vec![],
                agent_named: false,
                files: vec![],
                owner: Some("u-alex".into()),
                identity: Some(alex()),
            })
            .unwrap();
    }
    for _ in 0..4 {
        n.ack().await.unwrap();
    }
    let by = std::time::Instant::now() + Duration::from_secs(90);
    for (sid, from) in chats.iter().zip(from) {
        loop {
            let log = n.rt.store().events_after(*sid, 0, 10_000).unwrap();
            if log.len() > from && log.last().is_some_and(|e| e.kind == "turn_ended") {
                break;
            }
            assert!(std::time::Instant::now() < by, "a chat did not finish");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// A display name made only of what git strips (`; ' , :`) would make every
/// commit fail ("name consists only of disallowed characters"): the login is
/// the name then, through the real node.
#[tokio::test]
async fn a_name_git_would_strip_to_nothing_does_not_fail_the_commit() {
    let Some(mut n) = node(false).await else {
        return;
    };
    for crud in [";", "'", ",", ":"] {
        let (sid, _) = n.create(ExecMode::Sandbox, false).await;
        let id = CommitIdentity {
            name: Some(crud.into()),
            ..alex()
        };
        let out = n.bash(sid, Some("u-alex"), Some(id), COMMIT).await;
        assert_quiet(&out);
        assert_eq!(
            author_line(&out),
            format!("alex|{CANARY}|alex|{CANARY}"),
            "{crud:?}"
        );
    }
}

/// On a node whose `[sandbox] read` makes the node user's `~/.gitconfig`
/// readable, every chat on it reads THAT configuration, whoever owns the
/// chat: a second person's commits carry the node user's identity, not their
/// own (documented in the README and §4.7). Pinned here.
#[tokio::test]
async fn a_shared_global_config_wins_for_every_chat_on_the_node() {
    let Some(mut n) = node(true).await else {
        return;
    };
    let bea = CommitIdentity {
        login: "bea".into(),
        name: Some("Bea Builder".into()),
        email: Some("bea@example.net".into()),
    };
    for (owner, id) in [("u-alex", alex()), ("u-bea", bea)] {
        for mode in [ExecMode::Sandbox, ExecMode::Host] {
            let (sid, _) = n.create(mode, false).await;
            let out = n.bash(sid, Some(owner), Some(id.clone()), COMMIT).await;
            assert_quiet(&out);
            assert_eq!(
                author_line(&out),
                format!("{HOME_PERSON}|{HOME_PERSON}"),
                "{owner} {mode:?}"
            );
        }
    }
}

/// The node says so at start when its configuration makes the node user's
/// git configuration readable to the agents' commands: decided from
/// node.toml and the node's own `HOME`, never from anything an agent writes.
#[test]
fn the_node_names_a_readable_git_config_at_start() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let with = |read: Vec<PathBuf>| harness_tools::sandbox::Settings {
        read,
        ..Default::default()
    };
    assert!(harness_tools::sandbox::shared_git_config(&with(vec![])).is_none());
    assert!(
        harness_tools::sandbox::shared_git_config(&with(vec![PathBuf::from("/usr")])).is_none()
    );
    for entry in [home.clone(), home.join(".gitconfig")] {
        let f = harness_tools::sandbox::shared_git_config(&with(vec![entry.clone()]))
            .unwrap_or_else(|| panic!("{entry:?} was not named"));
        assert!(f.starts_with(&home));
    }
}

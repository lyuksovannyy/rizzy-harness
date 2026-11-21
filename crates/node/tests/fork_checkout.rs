//! A fork's own folder on a real node (ADOPTION_PLAN C.2 part 2,
//! docs/fork.md, PLAN.md T92): `session.fork` with `checkout: "own"` makes
//! a clone of the parent's repository (from outside it) in the node's
//! projects folder, writes the cut's files from the checkpoint, and gives
//! the new chat that folder. Everything is on temporary folders and
//! temporary repositories; nothing reaches a network.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{ForkCheckout, MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
    String::from_utf8_lossy(&o.stdout)
        .trim_end_matches('\n')
        .to_string()
}

struct Opts {
    /// `[projects] dir`: `None` leaves it out (and turns the default off).
    projects: bool,
    /// The parent's folder is a git repository.
    repo: bool,
}

struct H {
    node: Option<NodeRuntime>,
    link: NodeLink,
    dir: tempfile::TempDir,
    /// The parent's folder.
    repo: PathBuf,
    projects: PathBuf,
    data: PathBuf,
    sid: SessionId,
    caps: Vec<String>,
    n: u32,
}

fn config(dir: &Path, o: &Opts) -> NodeConfig {
    let ws = dir.join("ws");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let mut policy = NodePolicy {
        workspace_roots: vec![ws.clone()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    policy.skills.roots.clear();
    policy.projects.dir = Some(if o.projects {
        ws.join("projects")
    } else {
        PathBuf::new()
    });
    policy.protect(&data);
    policy.locate_projects(&data).unwrap();
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&data.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![]),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(data.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    }
}

impl H {
    async fn new(o: Opts) -> H {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let repo = ws.join("proj");
        std::fs::create_dir_all(&repo).unwrap();
        if o.repo {
            git(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join(".gitignore"), "ignored.log\n").unwrap();
            std::fs::write(repo.join("tracked.txt"), "committed\n").unwrap();
            std::fs::write(repo.join("gone.txt"), "will go\n").unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", "init"]);
        } else {
            std::fs::write(repo.join("tracked.txt"), "committed\n").unwrap();
        }
        let node = NodeRuntime::start(config(dir.path(), &o)).await.unwrap();
        let mut link = node.attach().await;
        let caps = match link.from_node.recv().await.unwrap() {
            NodeToMaster::Hello { capabilities, .. } => capabilities,
            other => panic!("expected hello, got {other:?}"),
        };
        let mut h = H {
            node: Some(node),
            link,
            projects: ws.join("projects"),
            data: dir.path().join("data"),
            dir,
            repo,
            sid: SessionId::new(),
            caps,
            n: 0,
        };
        let (sid, cwd) = (h.sid, h.repo.display().to_string());
        h.create(sid, cwd).await;
        h
    }

    async fn create(&mut self, sid: SessionId, cwd: String) {
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                exec_mode: None,
                no_callbacks: false,
                profile: None,
                incognito: false,
                agent: None,
                req_id: "c".into(),
                session_id: sid,
                title: "Parent".into(),
                cwd,
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
            })
            .unwrap();
        self.until(sid, |k| matches!(k, EventKind::SessionCreated(_)))
            .await;
    }

    fn store(&self) -> &harness_store::Store {
        self.node.as_ref().unwrap().store()
    }

    async fn recv(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(120), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn until(&mut self, sid: SessionId, pred: impl Fn(&EventKind) -> bool) {
        loop {
            if let NodeToMaster::Event { event } = self.recv().await
                && event.session_id == sid
                && pred(&event.typed().unwrap())
            {
                return;
            }
        }
    }

    fn log_of(&self, sid: SessionId) -> Vec<RawEvent> {
        self.store().events_after(sid, 0, 100_000).unwrap()
    }

    async fn turn_of(&mut self, sid: SessionId, text: &str) -> String {
        let mark = self
            .log_of(sid)
            .iter()
            .filter(|e| e.kind == "turn_ended")
            .count();
        self.n += 1;
        self.link
            .to_node
            .try_send(MasterToNode::Prompt(PromptMsg {
                req_id: format!("p{}", self.n),
                ..PromptMsg::test(sid, text)
            }))
            .unwrap();
        loop {
            self.until(sid, |k| matches!(k, EventKind::TurnEnded(_)))
                .await;
            let ended = ended(&self.log_of(sid));
            if ended.len() > mark {
                return ended.last().unwrap().clone();
            }
        }
    }

    /// `session.fork` for each of `cuts` (all sent before any answer is
    /// read); the new ids with the node's answer for each.
    async fn fork_all(
        &mut self,
        parent: SessionId,
        cuts: &[(&str, ForkCheckout)],
    ) -> Vec<(SessionId, Result<(), String>)> {
        let mut asked = Vec::new();
        for (turn, checkout) in cuts {
            let sid = SessionId::new();
            self.n += 1;
            let req = format!("f{}", self.n);
            self.link
                .to_node
                .try_send(MasterToNode::Fork {
                    req_id: req.clone(),
                    session_id: sid,
                    parent,
                    turn_id: (*turn).into(),
                    title: "Parent (fork)".into(),
                    checkout: *checkout,
                })
                .unwrap();
            asked.push((req, sid));
        }
        let mut done = BTreeMap::new();
        while done.len() < asked.len() {
            if let NodeToMaster::Ack {
                req_id, ok, error, ..
            } = self.recv().await
                && asked.iter().any(|(r, _)| *r == req_id)
            {
                done.insert(
                    req_id,
                    if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_default())
                    },
                );
            }
        }
        asked
            .into_iter()
            .map(|(r, sid)| (sid, done.remove(&r).unwrap()))
            .collect()
    }

    async fn fork(
        &mut self,
        turn: &str,
        checkout: ForkCheckout,
    ) -> (SessionId, Result<(), String>) {
        let parent = self.sid;
        self.fork_all(parent, &[(turn, checkout)]).await.remove(0)
    }

    async fn restore(&mut self, sid: SessionId, turn: &str) -> Result<(), String> {
        self.n += 1;
        let req = format!("r{}", self.n);
        self.link
            .to_node
            .try_send(MasterToNode::Restore {
                req_id: req.clone(),
                session_id: sid,
                turn_id: turn.into(),
                force: false,
                rewind: false,
            })
            .unwrap();
        loop {
            if let NodeToMaster::Ack {
                req_id, ok, error, ..
            } = self.recv().await
                && req_id == req
            {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    /// The folder the chat `sid` works in, from its creation.
    fn cwd_of(&self, sid: SessionId) -> PathBuf {
        let log = self.log_of(sid);
        match log[0].typed() {
            Ok(EventKind::SessionCreated(c)) => PathBuf::from(c.cwd),
            other => panic!("{other:?}"),
        }
    }

    fn staging(&self) -> Vec<String> {
        names(&self.data.join("projects-staging"))
    }

    fn forks_folders(&self) -> Vec<String> {
        names(&self.projects)
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn ended(log: &[RawEvent]) -> Vec<String> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::TurnEnded(t)) => Some(t.turn_id),
            _ => None,
        })
        .collect()
}

fn digest(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut m = BTreeMap::new();
    for e in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if e.file_type().is_file() {
            m.insert(
                e.path().strip_prefix(dir).unwrap().display().to_string(),
                std::fs::read(e.path()).unwrap(),
            );
        }
    }
    m
}

/// Turn 1 changes a tracked file, adds an untracked one and (outside any
/// turn) an ignored one; turn 2 is the cut; turn 3 goes on after it:
/// commits, deletes, adds. Returns the turn ids and the commit at the cut.
async fn parent_work(h: &mut H) -> (Vec<String>, String) {
    let head0 = git(&h.repo, &["rev-parse", "HEAD"]);
    let sid = h.sid;
    h.turn_of(sid, "run: echo changed > tracked.txt; echo new > new.txt")
        .await;
    std::fs::write(h.repo.join("ignored.log"), "ignored\n").unwrap();
    h.turn_of(sid, "hello two").await;
    h.turn_of(
        sid,
        "run: echo later > later.txt; git add later.txt; git -c user.name=t -c user.email=t@t \
         commit -qm later; rm new.txt gone.txt",
    )
    .await;
    (ended(&h.log_of(sid)), head0)
}

#[tokio::test]
async fn the_node_announces_what_it_can_do_only_with_a_projects_folder() {
    let h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    assert!(h.caps.iter().any(|c| c == harness_protocol::CAP_FORK_OWN));
    let h = H::new(Opts {
        projects: false,
        repo: true,
    })
    .await;
    assert!(!h.caps.iter().any(|c| c == harness_protocol::CAP_FORK_OWN));
    assert!(h.caps.iter().any(|c| c == harness_protocol::CAP_FORK));
}

/// The cut's files in a folder of its own: the parent's uncommitted and
/// untracked files at the cut appear uncommitted, an ignored file does not,
/// a file the parent deleted after the cut is there, the parent's later
/// commit is not in the fork's branch, and the parent's folder is as it
/// was.
#[tokio::test]
async fn an_own_fork_has_the_files_of_the_cut_in_a_folder_of_its_own() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    let (turns, head0) = parent_work(&mut h).await;
    let later_head = git(&h.repo, &["rev-parse", "HEAD"]);
    assert_ne!(later_head, head0);
    let parent_files = digest(&h.repo);
    let (fork, res) = h.fork(&turns[1], ForkCheckout::Own).await;
    res.unwrap();
    // The parent was only read.
    assert_eq!(digest(&h.repo), parent_files);
    let cwd = h.cwd_of(fork);
    let top = h.projects.canonicalize().unwrap();
    assert!(cwd.starts_with(&top), "{cwd:?} is not in {top:?}");
    assert_ne!(cwd, h.repo);
    assert!(
        cwd.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("proj-fork-"),
        "{cwd:?}"
    );
    assert_eq!(h.staging(), Vec::<String>::new(), "nothing left in staging");
    // The files of the cut.
    let read = |p: &str| std::fs::read_to_string(cwd.join(p)).unwrap();
    assert_eq!(read("tracked.txt"), "changed\n", "uncommitted work");
    assert_eq!(read("new.txt"), "new\n", "an untracked file");
    assert_eq!(read("gone.txt"), "will go\n", "deleted by the parent later");
    assert!(!cwd.join("ignored.log").exists(), "an ignored file");
    assert!(!cwd.join("later.txt").exists(), "a later commit's file");
    // The branch is the cut's commit, not the parent's later one.
    let log = h.log_of(fork);
    let short = git(&cwd, &["symbolic-ref", "--short", "HEAD"]);
    assert!(short.starts_with("fork/"), "{short}");
    assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head0);
    assert!(!git(&cwd, &["rev-list", "HEAD"]).contains(&later_head));
    assert_eq!(
        git(&cwd, &["status", "--porcelain"]),
        " M tracked.txt\n?? new.txt"
    );
    assert_eq!(
        git(&cwd, &["remote", "get-url", "origin"]),
        h.repo.display().to_string()
    );
    // The log says where it works and the divider follows the copy.
    assert!(matches!(log[0].typed(), Ok(EventKind::SessionCreated(c)) if c.forked.is_some()));
    assert!(log.iter().any(|e| e.kind == "session_forked"));
    // HEAD at the cut is known from the log: no note about it.
    assert!(
        !log.iter().any(|e| matches!(
            e.typed(),
            Ok(EventKind::Error(ErrorEvent { notice: true, .. }))
        )),
        "{log:?}"
    );
}

/// The fork's first agentic turn checkpoints in its own shadow and Undo
/// works there; the parent's Undo is not affected by the fork.
#[tokio::test]
async fn the_forks_turns_checkpoint_and_undo_in_its_own_folder() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    let (turns, _) = parent_work(&mut h).await;
    let (fork, res) = h.fork(&turns[1], ForkCheckout::Own).await;
    res.unwrap();
    let cwd = h.cwd_of(fork);
    let shadows_before = names(&h.data.join("checkpoints"));
    let t = h.turn_of(fork, "run: echo fork > tracked.txt").await;
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "fork\n"
    );
    // Its own shadow repository (one more than the parent's).
    let shadows = names(&h.data.join("checkpoints"));
    assert_eq!(shadows.len(), shadows_before.len() + 1, "{shadows:?}");
    h.restore(fork, &t).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "changed\n"
    );
    // The parent's folder was not touched by the fork's turn or undo, and
    // its own Undo works.
    assert_eq!(
        std::fs::read_to_string(h.repo.join("tracked.txt")).unwrap(),
        "changed\n"
    );
    let sid = h.sid;
    assert!(h.repo.join("later.txt").exists());
    h.restore(sid, &turns[2]).await.unwrap();
    assert!(
        !h.repo.join("later.txt").exists(),
        "the parent's turn is undone"
    );
    assert!(h.repo.join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "changed\n"
    );
}

/// Two forks made together get folders of their own, different ones.
#[tokio::test]
async fn two_forks_at_once_get_different_folders() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    let (turns, _) = parent_work(&mut h).await;
    let sid = h.sid;
    let forks = h
        .fork_all(
            sid,
            &[
                (&turns[1], ForkCheckout::Own),
                (&turns[1], ForkCheckout::Own),
                (&turns[0], ForkCheckout::Own),
            ],
        )
        .await;
    let mut folders = Vec::new();
    for (f, r) in &forks {
        r.clone().unwrap();
        folders.push(h.cwd_of(*f));
    }
    let mut unique = folders.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 3, "{folders:?}");
    // The fork of the first turn has what that turn's checkpoint had.
    let first = &folders[2];
    assert_eq!(
        std::fs::read_to_string(first.join("tracked.txt")).unwrap(),
        "changed\n"
    );
    assert_eq!(h.staging(), Vec::<String>::new());
}

/// A shared fork is what part 1 made: the parent's folder, no folder made.
#[tokio::test]
async fn a_shared_fork_uses_the_parents_folder_and_makes_none() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    let (turns, _) = parent_work(&mut h).await;
    let (fork, res) = h.fork(&turns[1], ForkCheckout::Shared).await;
    res.unwrap();
    assert_eq!(h.cwd_of(fork), h.repo.canonicalize().unwrap());
    assert_eq!(h.forks_folders(), Vec::<String>::new());
}

/// Refusals say which, say that "shared" is possible, and leave no chat
/// and no folder.
#[tokio::test]
async fn an_own_fork_is_refused_where_it_cannot_be_made_and_nothing_is_left() {
    // A folder that is not a git repository.
    let mut h = H::new(Opts {
        projects: true,
        repo: false,
    })
    .await;
    let sid = h.sid;
    let t = h.turn_of(sid, "hello").await;
    let (fork, res) = h.fork(&t, ForkCheckout::Own).await;
    let e = res.unwrap_err();
    assert!(
        e.contains("not a git repository") && e.contains("share"),
        "{e}"
    );
    assert!(h.log_of(fork).is_empty(), "no chat");
    assert_eq!(h.forks_folders(), Vec::<String>::new());
    assert_eq!(h.staging(), Vec::<String>::new());
    // The same chat can still fork into its own folder.
    let (shared, res) = h.fork(&t, ForkCheckout::Shared).await;
    res.unwrap();
    assert!(!h.log_of(shared).is_empty());

    // No projects folder.
    let mut h = H::new(Opts {
        projects: false,
        repo: true,
    })
    .await;
    let sid = h.sid;
    let t = h.turn_of(sid, "hello").await;
    let (fork, res) = h.fork(&t, ForkCheckout::Own).await;
    let e = res.unwrap_err();
    assert!(e.contains("[projects] dir") && e.contains("share"), "{e}");
    assert!(h.log_of(fork).is_empty());
}

/// A pruned checkpoint: refused with the reason, and "shared" is said.
#[tokio::test]
async fn an_own_fork_of_a_pruned_checkpoint_is_refused() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    let (turns, _) = parent_work(&mut h).await;
    // The node kept only the latest checkpoints: drop every ref in the
    // shadow, as its pruning does.
    for shadow in names(&h.data.join("checkpoints")) {
        let dir = h.data.join("checkpoints").join(&shadow);
        if dir.extension().is_some_and(|e| e == "git") {
            let refs = git(
                &dir,
                &["for-each-ref", "--format=%(refname)", "refs/harness/"],
            );
            for r in refs.lines() {
                git(&dir, &["update-ref", "-d", r]);
            }
        }
    }
    let (fork, res) = h.fork(&turns[1], ForkCheckout::Own).await;
    let e = res.unwrap_err();
    assert!(e.contains("checkpoint") && e.contains("share"), "{e}");
    assert!(h.log_of(fork).is_empty());
    assert_eq!(h.forks_folders(), Vec::<String>::new());
    assert_eq!(h.staging(), Vec::<String>::new());
}

/// The chat before any agentic turn: the branch's commit is written as it
/// is, and a log that says nothing of HEAD is told in the chat.
#[tokio::test]
async fn a_cut_with_no_checkpoint_writes_the_committed_files_and_says_where_head_is() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    let sid = h.sid;
    let t = h.turn_of(sid, "hello").await;
    std::fs::write(h.repo.join("tracked.txt"), "edited after\n").unwrap();
    let (fork, res) = h.fork(&t, ForkCheckout::Own).await;
    res.unwrap();
    let cwd = h.cwd_of(fork);
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "committed\n"
    );
    assert_eq!(git(&cwd, &["status", "--porcelain"]), "");
    let notes: Vec<_> = h
        .log_of(fork)
        .into_iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Error(ErrorEvent {
                notice: true,
                message,
                ..
            })) => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("does not say where"), "{notes:?}");
}

/// A fork of a sub-folder of the repository works in the same sub-folder of
/// its copy.
#[tokio::test]
async fn a_chat_in_a_subfolder_forks_into_the_same_subfolder() {
    let mut h = H::new(Opts {
        projects: true,
        repo: true,
    })
    .await;
    std::fs::create_dir_all(h.repo.join("pkg/inner")).unwrap();
    std::fs::write(h.repo.join("pkg/inner/f.txt"), "f\n").unwrap();
    git(&h.repo, &["add", "."]);
    git(&h.repo, &["commit", "-qm", "pkg"]);
    let sub = SessionId::new();
    let cwd = h.repo.join("pkg/inner").display().to_string();
    h.create(sub, cwd).await;
    let t = h.turn_of(sub, "run: echo x > g.txt").await;
    let (fork, res) = h.fork_all(sub, &[(&t, ForkCheckout::Own)]).await.remove(0);
    res.unwrap();
    let at = h.cwd_of(fork);
    assert!(at.ends_with("pkg/inner"), "{at:?}");
    assert_eq!(std::fs::read_to_string(at.join("g.txt")).unwrap(), "x\n");
    let _ = &h.dir;
}

//! A fork's own folder (ADOPTION_PLAN C.2 part 2, docs/fork.md, PLAN.md
//! T92): where it goes, what must hold before anything is written, and
//! what is left behind on no path.
//!
//! The copy itself is `git::fork_copy` (a sanitised copy of the parent's
//! history files made by our own walk, then a clone of that; the node's git
//! never opens the parent's repository).
//! This module decides the rest, on the node's own policy:
//!
//! - **Where.** In the node's projects folder (`[projects] dir`, where forge
//!   clones go), under `<repository name>-fork-<short id>`: built in the
//!   node's staging folder (0700, protected, emptied at every start, so a
//!   copy half made when the node was killed goes at the next start) and
//!   moved in with `renameat2(RENAME_NOREPLACE)`, which never replaces or
//!   follows what holds the name, then checked again as a session folder
//!   (`policy.check_cwd`). No projects folder: refused, naming the setting.
//! - **Before anything is written:** the node's git is new enough, the
//!   projects folder and the staging folder are the ones recorded at start,
//!   the disk has room for the repository's `.git` and the tree (with a
//!   margin), the copy fits `[limits] clone_max_mb`, and a copy per hour
//!   counts like a project folder (`[limits] projects_per_hour`).
//! - **On every failure** the build folder is removed; a copy whose chat
//!   could not be written is removed by [`Copy::discard`] (only while it is
//!   still the folder that was made).
//!
//! Nothing here can be started by a model, a tool or a routine: only
//! `session.fork` with `checkout: "own"`, which the master sends for the
//! parent's owner.

use std::path::{Path, PathBuf};
use std::time::Duration;

use harness_core::RawEvent;
use harness_core::event::{Checkpoint, CheckpointPhase, EventKind};
use harness_core::folder::Identity;

use crate::git::fork_copy::{self, Plan, Source};
use crate::git::{Bounds, Repo};
use crate::policy::NodePolicy;
use crate::projects::{self, Build, Made};

/// Why a node makes no own copy for a fork: no projects folder.
pub const NO_PROJECTS: &str = "this node makes no folders for forks: it has no projects folder \
     ([projects] dir in node.toml, inside workspace_roots)";

/// What a fork's own copy is made from, read from the parent's log.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CutState {
    /// The cut's snapshot: the `after` checkpoint of the last turn at or
    /// before the cut that has one.
    pub snapshot: Option<String>,
    /// The commit the parent's HEAD was on at the cut, when its log says:
    /// where the last turn that moved HEAD (at or before the cut) left it,
    /// else where the first turn that moved it after the cut found it.
    pub head: Option<String>,
}

/// `checkpoints` are the parent's `checkpoint` events (all of the log).
pub(crate) fn cut_state(checkpoints: &[RawEvent], through: u64) -> CutState {
    let mut state = CutState::default();
    let mut after_the_cut = None;
    for e in checkpoints {
        let Ok(EventKind::Checkpoint(Checkpoint {
            phase: CheckpointPhase::After,
            git_ref,
            head_moved,
            ..
        })) = e.typed()
        else {
            continue;
        };
        if e.seq <= through {
            state.snapshot = Some(git_ref);
            if let Some(m) = head_moved {
                state.head = Some(m.to.commit);
            }
        } else if after_the_cut.is_none()
            && let Some(m) = head_moved
        {
            after_the_cut = Some(m.from.commit);
        }
    }
    if state.head.is_none() {
        state.head = after_the_cut;
    }
    state
}

/// What a copy is made for.
pub(crate) struct Request {
    /// The parent's folder, as its session checked it (canonical).
    pub cwd: PathBuf,
    pub bounds: Bounds,
    /// The parent's checkpoint repository.
    pub shadow: Repo,
    pub cut: CutState,
    /// The fork's short id (the branch and the folder's name).
    pub short: String,
}

/// A fork's own folder, made.
#[derive(Debug)]
pub(crate) struct Copy {
    /// The fork's working folder: the copy's root, or the sub-folder of it
    /// the parent worked in.
    pub folder: PathBuf,
    /// The copy's root (where its `.git` is).
    pub root: PathBuf,
    pub branch: String,
    pub head_exact: bool,
    /// Whether the copy has a remote `upstream` (the parent's `origin`).
    pub upstream: bool,
    /// What the person is told in the fork (a sentence per line), besides
    /// the folder itself.
    pub notes: Vec<String>,
    made: Identity,
    /// The marker that says the folder has no chat yet ([`Copy::created`]).
    pending: Option<projects::Pending>,
}

impl Copy {
    /// The fork's chat is stored: a start no longer removes the folder.
    pub(crate) fn created(&mut self) {
        self.pending = None;
    }

    /// Remove this copy: its chat could not be written. Only while the
    /// path is still the folder that was made (never what replaced it).
    pub(crate) async fn discard(self) {
        let (root, made) = (self.root, self.made);
        let _ = tokio::task::spawn_blocking(move || {
            if Identity::of(&root).ok() == Some(made) {
                let _ = std::fs::remove_dir_all(&root);
            }
        })
        .await;
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: the free bytes on the projects folder's disk.
    static FREE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn pretend_free(bytes: Option<u64>) {
    FREE.with(|c| c.set(bytes));
}

fn free_bytes(dir: &Path) -> Option<u64> {
    #[cfg(test)]
    if let Some(n) = FREE.with(|c| c.get()) {
        return Some(n);
    }
    let st = nix::sys::statvfs::statvfs(dir).ok()?;
    Some((st.blocks_available() as u64).saturating_mul(st.fragment_size() as u64))
}

/// The most of the `.git` and the tree a copy is allowed to be counted
/// before the walk stops (the check against `clone_max_mb` then refuses).
const MARGIN_MIN: u64 = 64 * 1024 * 1024;

/// Make the fork's own folder. `Err` is a sentence for the person; nothing
/// is left behind.
pub(crate) async fn make(
    policy: &NodePolicy,
    made: &Made,
    req: Request,
    make_suffix: &(dyn Fn() -> String + Sync),
) -> Result<Copy, String> {
    let pp = &policy.projects;
    let (Some(dir), Some(_), Some(_)) = (pp.located.clone(), pp.parent_id, pp.staging.clone())
    else {
        return Err(NO_PROJECTS.into());
    };
    if policy.limits.projects_per_hour == 0 {
        return Err(NO_PROJECTS.into());
    }
    let source = match fork_copy::source_of(&req.cwd, &req.bounds).await? {
        Source::Repo(top) => top,
        Source::NotRepo => {
            return Err(
                "this chat's folder is not a git repository, so there is nothing to \
                        copy"
                    .into(),
            );
        }
    };
    fork_copy::version_ok().await?;
    // Reserved now and given back on any failure: copies that run at once
    // cannot all pass the hour's limit before one records.
    let slot = made.reserve(policy.limits.projects_per_hour)?;
    let max = policy.limits.clone_max_mb.saturating_mul(1024 * 1024);
    // Room, before anything is made.
    let git_bytes = fork_copy::git_dir_bytes(&source, max.saturating_add(1)).await;
    let tree_bytes = match &req.cut.snapshot {
        Some(s) => fork_copy::tree_bytes(&req.shadow, s).await?,
        None => git_bytes,
    };
    let need = git_bytes.saturating_add(tree_bytes);
    if need > max {
        return Err(format!(
            "a copy of this repository would need about {} MB, more than this node allows \
             ([limits] clone_max_mb = {} MB)",
            need / (1024 * 1024),
            max / (1024 * 1024)
        ));
    }
    let margin = (need / 8).max(MARGIN_MIN);
    if let Some(free) = free_bytes(&dir)
        && free < need.saturating_add(margin)
    {
        return Err(format!(
            "there is not enough free disk space for a copy of this repository: it needs about \
             {} MB and {} has {} MB",
            need.saturating_add(margin) / (1024 * 1024),
            dir.display(),
            free / (1024 * 1024)
        ));
    }
    let name = source
        .file_name()
        .map(|n| projects::slug(&n.to_string_lossy()))
        .unwrap_or_else(|| projects::EMPTY_SLUG.to_string());
    let base = format!("{name}-fork-{}", req.short);
    let mut names = vec![base.clone()];
    names.extend((0..projects::TRIES - 1).map(|_| format!("{base}-{}", make_suffix())));
    let p = policy.clone();
    let (top, build) = tokio::task::spawn_blocking(move || {
        let top = projects::open_projects(&p, &dir)?;
        let build = Build::new(&p)?;
        Ok::<_, String>((top, build))
    })
    .await
    .map_err(|e| format!("making the folder: {e}"))??;
    let branch = format!("fork/{}", req.short);
    let sub = req
        .cwd
        .strip_prefix(&source)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let outside = build.path().parent().map(Path::to_path_buf);
    let built = async {
        let outside = outside.ok_or("the node's staging folder is missing")?;
        let filled = fork_copy::fill(&Plan {
            source: &source,
            dest: build.path(),
            outside: &outside,
            branch: &branch,
            head: req.cut.head.as_deref(),
            shadow: &req.shadow,
            snapshot: req.cut.snapshot.as_deref(),
            timeout: Duration::from_secs(policy.limits.clone_timeout_s),
            max_bytes: max,
            max_entries: crate::git::fork_copy::ENTRY_MAX,
        })
        .await?;
        build
            .check(false)
            .map_err(|e| format!("the copy is not usable: {e}"))?;
        let at_sub = sub.as_os_str().is_empty() || build.path().join(&sub).is_dir();
        Ok::<_, String>((filled, at_sub))
    }
    .await;
    let (filled, at_sub) = match built {
        Ok(v) => v,
        Err(e) => {
            tokio::task::spawn_blocking(move || build.discard())
                .await
                .ok();
            return Err(e);
        }
    };
    // A kill from here to the chat's creation leaves a complete, unused
    // folder: the marker has the next start remove it.
    let pending = build.mark_pending()?;
    let p = policy.clone();
    let root = tokio::task::spawn_blocking(move || build.move_into(&p, &top, &names))
        .await
        .map_err(|e| format!("moving the folder: {e}"))??;
    slot.keep();
    let mut notes = Vec::new();
    let folder = if at_sub {
        root.join(&sub)
    } else {
        notes.push(format!(
            "The folder this chat worked in ({}) is not in the copy, so the fork works at the \
             copy's top.",
            sub.display()
        ));
        root.clone()
    };
    let identity = Identity::of(&root);
    // The folder is a session folder like any other.
    let checked = identity
        .and_then(|id| policy.check_cwd(&folder.to_string_lossy()).map(|f| (f, id)))
        .map_err(|e| format!("the copy is not a usable folder: {e}"));
    let (folder, id) = match checked {
        Ok(v) => v,
        Err(e) => {
            let r = root.clone();
            let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(r)).await;
            return Err(e);
        }
    };
    if !filled.head_exact {
        notes.push(format!(
            "The chat's log does not say where the repository's HEAD was at this point, so \
             branch {branch} starts at the commit it is on now ({}).",
            &filled.commit[..filled.commit.len().min(12)]
        ));
    }
    Ok(Copy {
        folder,
        root,
        branch,
        head_exact: filled.head_exact,
        upstream: filled.upstream.is_some(),
        notes,
        made: id,
        pending: Some(pending),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::SessionId;
    use harness_core::event::{HeadAt, HeadMoved};

    fn ev(seq: u64, phase: CheckpointPhase, sha: &str, moved: Option<(&str, &str)>) -> RawEvent {
        let (kind, payload) = EventKind::Checkpoint(Checkpoint {
            turn_id: format!("t{seq}"),
            phase,
            git_ref: sha.into(),
            files: vec![],
            excluded: vec![],
            excluded_more: 0,
            head_moved: moved.map(|(a, b)| HeadMoved {
                from: HeadAt {
                    branch: None,
                    commit: a.repeat(40),
                },
                to: HeadAt {
                    branch: None,
                    commit: b.repeat(40),
                },
            }),
        })
        .into_parts();
        RawEvent {
            session_id: SessionId::new(),
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    }

    // ---- the operation, on temporary folders and repositories ----

    struct Rig {
        _d: tempfile::TempDir,
        ws: PathBuf,
        repo: PathBuf,
        data: PathBuf,
        policy: NodePolicy,
        made: Made,
        shadow: Repo,
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    async fn rig(projects: bool) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let ws = top.join("ws");
        let repo = ws.join("proj");
        let data = top.join("data");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        sh(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-qm", "init"]);
        let mut policy = NodePolicy {
            workspace_roots: vec![ws.clone()],
            ..Default::default()
        };
        policy.skills.roots.clear();
        policy.projects.dir = Some(if projects {
            ws.join("projects")
        } else {
            PathBuf::new()
        });
        policy.protect(&data);
        policy.locate_projects(&data).unwrap();
        let shadow = crate::git::repo_for(&repo, Some(&data.join("checkpoints")))
            .await
            .unwrap();
        Rig {
            _d: d,
            ws,
            repo,
            data,
            policy,
            made: Made::default(),
            shadow,
        }
    }

    impl Rig {
        fn request(&self) -> Request {
            Request {
                cwd: self.repo.clone(),
                bounds: Bounds {
                    ceiling: Some(self.ws.clone()),
                    protected: self.policy.protected_dirs.clone(),
                    sandbox: None,
                },
                shadow: self.shadow.clone(),
                cut: CutState::default(),
                short: "ab12cd34".into(),
            }
        }

        async fn make(&self) -> Result<Copy, String> {
            make(&self.policy, &self.made, self.request(), &|| "zzzz".into()).await
        }

        fn staging(&self) -> Vec<String> {
            names(&self.data.join("projects-staging"))
        }

        fn projects(&self) -> Vec<String> {
            names(&self.ws.join("projects"))
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

    #[tokio::test]
    async fn a_copy_is_made_in_the_projects_folder_and_discard_removes_only_it() {
        let r = rig(true).await;
        let mut c = r.make().await.unwrap();
        let projects = r.ws.join("projects").canonicalize().unwrap();
        assert_eq!(c.root, projects.join("proj-fork-ab12cd34"));
        assert_eq!(c.folder, c.root);
        assert_eq!(c.branch, "fork/ab12cd34");
        assert!(c.root.join("a.txt").is_file() && c.root.join(".git").is_dir());
        // Until the chat is stored the folder has a marker in staging.
        assert_eq!(r.staging().len(), 1, "{:?}", r.staging());
        assert!(r.staging()[0].starts_with("pending-fork-"));
        c.created();
        assert_eq!(r.staging(), Vec::<String>::new());
        // A folder swapped in at the path is not what was made: not removed.
        let root = c.root.clone();
        let moved = r.ws.join("moved-away");
        std::fs::rename(&root, &moved).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("theirs"), "x").unwrap();
        c.discard().await;
        assert!(root.join("theirs").is_file(), "a replaced folder is kept");
        assert!(moved.join("a.txt").is_file());
        // The folder that was made is removed.
        let r2 = rig(true).await;
        let c = r2.make().await.unwrap();
        let root = c.root.clone();
        c.discard().await;
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn no_projects_folder_refuses_naming_the_setting_and_writes_nothing() {
        let r = rig(false).await;
        let e = r.make().await.unwrap_err();
        assert_eq!(e, NO_PROJECTS);
        assert!(e.contains("[projects] dir"));
        assert_eq!(r.staging(), Vec::<String>::new());
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_folder_that_is_no_repository_is_refused() {
        let mut r = rig(true).await;
        let plain = r.ws.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        r.shadow = crate::git::repo_for(&plain, Some(&r.data.join("checkpoints")))
            .await
            .unwrap();
        let mut req = r.request();
        req.cwd = plain;
        let e = make(&r.policy, &r.made, req, &|| "zzzz".into())
            .await
            .unwrap_err();
        assert!(e.contains("not a git repository"), "{e}");
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_full_disk_refuses_before_anything_is_made() {
        let r = rig(true).await;
        pretend_free(Some(1024));
        let e = r.make().await.unwrap_err();
        pretend_free(None);
        assert!(e.contains("not enough free disk space"), "{e}");
        assert_eq!(r.staging(), Vec::<String>::new());
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_copy_over_the_limit_is_refused_before_anything_is_made() {
        let mut r = rig(true).await;
        r.policy.limits.clone_max_mb = 0;
        let e = r.make().await.unwrap_err();
        assert!(e.contains("clone_max_mb"), "{e}");
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    /// A git that fails in the middle of the clone, as a full disk does,
    /// after it wrote part of the copy.
    #[tokio::test]
    async fn a_clone_that_dies_midway_leaves_no_folder_and_nothing_in_staging() {
        let r = rig(true).await;
        let real = String::from_utf8(
            std::process::Command::new("sh")
                .args(["-c", "command -v git"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let script = r.data.join("fake-git/git");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        crate::test_script::script(
            &script,
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do if [ \"$a\" = clone ]; then for d; do :; done; \
                 mkdir -p \"$d/.git/objects\"; echo part > \"$d/.git/objects/x\"; \
                 echo 'fatal: No space left on device' >&2; exit 128; fi; done\n\
                 exec {} \"$@\"\n",
                real.trim()
            ),
        );
        crate::git::use_test_git(Some(script));
        let e = r.make().await.unwrap_err();
        crate::git::use_test_git(None);
        assert!(
            e.contains("could not copy") && e.contains("No space left"),
            "{e}"
        );
        assert_eq!(r.staging(), Vec::<String>::new());
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    /// A cut whose snapshot cannot be listed has no known size: refused
    /// before anything is made, not copied on "0 bytes".
    #[tokio::test]
    async fn a_snapshot_that_cannot_be_listed_refuses_the_copy_before_anything_is_made() {
        let r = rig(true).await;
        let mut req = r.request();
        req.cut.snapshot = Some("1".repeat(40));
        let e = make(&r.policy, &r.made, req, &|| "zzzz".into())
            .await
            .unwrap_err();
        assert!(e.contains("file list could not be read"), "{e}");
        assert_eq!(r.staging(), Vec::<String>::new());
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    /// Copies that run at once cannot all pass the hour's limit: the place
    /// is taken before the copy starts, not recorded after it ends.
    #[tokio::test]
    async fn copies_at_once_cannot_pass_the_hourly_limit_together() {
        let mut r = rig(true).await;
        r.policy.limits.projects_per_hour = 1;
        let (a, b) = tokio::join!(
            make(&r.policy, &r.made, r.request(), &|| "aaaa".into()),
            make(&r.policy, &r.made, r.request(), &|| "bbbb".into()),
        );
        assert_eq!(
            [a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(),
            1,
            "{a:?} / {b:?}"
        );
    }

    /// A copy that fails gives its place back; one that is cancelled gives
    /// it back and removes its folders from staging at once.
    #[tokio::test]
    async fn a_failed_copy_gives_its_place_back_and_a_cancelled_one_leaves_no_folder() {
        let mut r = rig(true).await;
        r.policy.limits.projects_per_hour = 1;
        let real = String::from_utf8(
            std::process::Command::new("sh")
                .args(["-c", "command -v git"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let script = r.data.join("slow-git/git");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        // The clone hangs (a huge repository, a slow disk).
        crate::test_script::script(
            &script,
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do if [ \"$a\" = clone ]; then sleep 30; fi; done\n\
                 exec {} \"$@\"\n",
                real.trim()
            ),
        );
        crate::git::use_test_git(Some(script));
        let res = tokio::time::timeout(Duration::from_millis(1500), r.make()).await;
        assert!(res.is_err(), "the slow copy finished: {res:?}");
        // Nothing waits for the next start: the future's drop removed it.
        assert_eq!(r.staging(), Vec::<String>::new());
        crate::git::use_test_git(None);
        // The cancelled copy gave its place back: a limit of one still has room.
        r.make().await.unwrap();
        assert_eq!(r.staging(), Vec::<String>::new());
    }

    /// A name that is taken (a folder, or a link) is never replaced or
    /// followed: the copy takes another.
    #[tokio::test]
    async fn a_taken_name_is_never_replaced_or_followed() {
        let r = rig(true).await;
        let projects = r.ws.join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::create_dir(projects.join("proj-fork-ab12cd34")).unwrap();
        std::fs::write(projects.join("proj-fork-ab12cd34/mine"), "m").unwrap();
        let elsewhere = r.ws.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, projects.join("proj-fork-ab12cd34-zzzz")).unwrap();
        // Every suffix is "zzzz": all names tried are taken.
        let e = r.make().await.unwrap_err();
        assert!(e.contains("taken"), "{e}");
        assert_eq!(
            std::fs::read_to_string(projects.join("proj-fork-ab12cd34/mine")).unwrap(),
            "m"
        );
        assert_eq!(names(&elsewhere), Vec::<String>::new());
        assert_eq!(r.staging(), Vec::<String>::new());
        // With another suffix available the copy is made beside them.
        let c = make(&r.policy, &r.made, r.request(), &|| "yyyy".into())
            .await
            .unwrap();
        assert!(c.root.ends_with("proj-fork-ab12cd34-yyyy"), "{:?}", c.root);
        assert_eq!(names(&elsewhere), Vec::<String>::new());
    }

    /// A node.toml narrowed since the start: the projects folder is outside
    /// `workspace_roots` now, and nothing is written.
    #[tokio::test]
    async fn a_projects_folder_outside_the_roots_is_refused_before_anything_is_written() {
        let r = rig(true).await;
        let mut p = r.policy.clone();
        p.workspace_roots = vec![r.ws.join("proj")];
        let e = make(&p, &r.made, r.request(), &|| "zzzz".into())
            .await
            .unwrap_err();
        assert!(e.contains("workspace_roots"), "{e}");
        assert_eq!(r.staging(), Vec::<String>::new());
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    /// A copy half made when the node was killed is in the staging folder,
    /// which every start empties (`NodePolicy::locate_projects`).
    #[tokio::test]
    async fn a_copy_half_made_at_a_kill_is_swept_at_the_next_start() {
        let mut r = rig(true).await;
        let build = r.data.join("projects-staging/build-0123456789abcdef");
        std::fs::create_dir_all(build.join(".git/objects")).unwrap();
        std::fs::write(build.join(".git/objects/x"), "part").unwrap();
        r.policy.locate_projects(&r.data).unwrap();
        assert_eq!(r.staging(), Vec::<String>::new());
        assert_eq!(r.projects(), Vec::<String>::new());
    }

    /// A kill between the move into the projects folder and the chat's
    /// creation leaves a complete, unused folder and its marker; the next
    /// start removes that folder (and only it). Once the chat exists the
    /// marker is gone and a start leaves the folder alone.
    #[tokio::test]
    async fn a_copy_whose_chat_was_never_written_is_removed_at_the_next_start() {
        let mut r = rig(true).await;
        let killed = r.make().await.unwrap();
        let kept = make(&r.policy, &r.made, r.request(), &|| "yyyy".into()).await;
        let mut kept = kept.unwrap();
        // A folder of the person's, in the same place, which nothing names.
        let theirs = r.ws.join("projects/mine");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(theirs.join("f"), "x").unwrap();
        let (gone, stays) = (killed.root.clone(), kept.root.clone());
        assert!(gone.is_dir() && stays.is_dir());
        // The second one's chat was stored; the first one's process was killed.
        kept.created();
        std::mem::forget(killed);
        assert!(
            names(&r.data.join("projects-staging"))
                .iter()
                .any(|n| n.starts_with("pending-fork-")),
            "no marker was written"
        );
        r.policy.locate_projects(&r.data).unwrap();
        assert!(!gone.exists(), "the unused folder was left");
        assert!(
            stays.join("a.txt").is_file(),
            "a folder with a chat was removed"
        );
        assert!(theirs.join("f").is_file());
        assert_eq!(r.staging(), Vec::<String>::new());
    }

    /// A copy counts against the folders-per-hour limit like a project.
    #[tokio::test]
    async fn copies_count_against_the_projects_per_hour_limit() {
        let mut r = rig(true).await;
        r.policy.limits.projects_per_hour = 1;
        r.make().await.unwrap();
        let again = make(&r.policy, &r.made, r.request(), &|| "yyyy".into()).await;
        assert!(again.is_err(), "{again:?}");
    }

    #[test]
    fn the_cut_is_the_last_after_checkpoint_at_or_before_it() {
        let log = [
            ev(3, CheckpointPhase::Before, "b1", None),
            ev(5, CheckpointPhase::After, "a1", None),
            ev(9, CheckpointPhase::After, "a2", Some(("1", "2"))),
            ev(12, CheckpointPhase::After, "a3", Some(("2", "3"))),
        ];
        // Cut after the second turn: its snapshot and where it left HEAD.
        let s = cut_state(&log, 10);
        assert_eq!(s.snapshot.as_deref(), Some("a2"));
        assert_eq!(s.head, Some("2".repeat(40)));
        // Cut after the first turn, which moved nothing: HEAD is where the
        // next turn that moved it found it.
        let s = cut_state(&log, 6);
        assert_eq!(s.snapshot.as_deref(), Some("a1"));
        assert_eq!(s.head, Some("1".repeat(40)));
        // No turn with a checkpoint yet, nothing moved: nothing known.
        assert_eq!(cut_state(&log[..1], 6), CutState::default());
        let none = cut_state(&[], 4);
        assert_eq!((none.snapshot, none.head), (None, None));
    }
}

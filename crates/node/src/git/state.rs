//! The branch and the count of changed files of a chat's folder, for its
//! header (ADOPTION_PLAN X.64, PLAN.md T19).
//!
//! **No git process runs in the person's repository, here as everywhere
//! else in this module.** The branch and the commit are read from the
//! repository's files by our own code ([`user_repo::head_label`]); the
//! count is asked of the node's shadow repository, with a private copy of
//! the user's index (its stat cache, so unchanged files are not read), the
//! way [`describe`] asks whether tracked files changed. Nothing in the
//! repository's `.git/config`, hooks or attributes is read by any process
//! of ours. What this is not: `git status`. It cannot see a global ignore
//! file (the node does not follow it, X.31), counts a file whose bytes an
//! attribute filter would change as changed, and leaves submodules and
//! nested repositories as the one entry they are.
//!
//! The count is the number of changed tracked files (from the last commit,
//! staged or not: one entry per path) plus the untracked entries no ignore
//! rule hides, where a folder that holds nothing else is one entry (what
//! `git status` prints). The whole repository is counted, not only the
//! chat's folder inside it. Untracked files count because a coding agent's
//! main work is making new files; the answer is capped at
//! [`MAX_GIT_CHANGED`] and absent (not zero) when it cannot be told in
//! time.

use super::*;
use harness_protocol::{GitState, MAX_GIT_CHANGED};

/// The longest any one of the count's git processes may run.
const STATE_TIMEOUT: Duration = Duration::from_secs(5);
/// ...and the three of them together; the answer is the branch alone when
/// this passes.
const COUNT_TIMEOUT: Duration = Duration::from_secs(12);
/// The largest user index copied (a repository of some 600,000 files).
const INDEX_MAX: u64 = 64 << 20;

/// What the folder `dir` is in: `None` when it is in no plain repository
/// directory the node reads (no repository, a gitfile or a link, a
/// `commondir`, a bare repository, a corrupt `.git`, a `HEAD` that names
/// neither a branch nor a commit).
pub async fn state(dir: &Path, shadow_base: Option<&Path>, bounds: &Bounds) -> Option<GitState> {
    let dir = dir.canonicalize().ok()?;
    let user_repo::Found::Repo(r) = find_repo(&dir, bounds).await else {
        return None;
    };
    let top = r.top.clone();
    let (branch, commit) = tokio::task::spawn_blocking(move || user_repo::head_label(&top))
        .await
        .ok()??;
    let changed = match (&commit, &r.index) {
        (Some(commit), Some(index)) => match checkpoint_repo(&dir, shadow_base, bounds).await {
            Ok(Some(repo)) if repo.root == r.top => tokio::time::timeout(
                COUNT_TIMEOUT,
                repo.count_changes(index, commit, STATE_TIMEOUT),
            )
            .await
            .ok()
            .flatten(),
            _ => None,
        },
        _ => None,
    };
    Some(GitState {
        branch: branch.as_deref().map(shown_branch),
        commit: commit.map(|c| c.chars().take(7).collect()),
        changed,
    })
}

impl Repo {
    /// Changed tracked files plus unhidden untracked entries, capped at
    /// [`MAX_GIT_CHANGED`], or `None` when any step failed or timed out
    /// (`each`: the longest one git process may run).
    async fn count_changes(&self, user_index: &Path, commit: &str, each: Duration) -> Option<u32> {
        let dir = self.shadow.as_ref()?;
        let path = user_index.to_path_buf();
        let bytes = tokio::task::spawn_blocking(move || user_repo::read_bounded(&path, INDEX_MAX))
            .await
            .ok()??;
        let tmp = dir.join(format!("harness-index-{}", harness_core::new_id()));
        let _files = IndexFiles(tmp.clone());
        tokio::fs::write(&tmp, bytes).await.ok()?;
        // A file only touched (a checkout, an editor's save) has another
        // stat than the cache holds: refresh the private copy so only
        // content that differs counts. Its exit status says only that
        // something needs an update. (Probed with a gitlink in the index:
        // it runs nothing there, as `ls-files --others` runs nothing.)
        let mut c = command(self, Some(&tmp));
        c.args(["update-index", "-q", "--refresh"]);
        exec(c, None, each).await.ok()?;
        // `--ignore-submodules=all`: a gitlink in the index (the user's
        // own, or a 160000 entry the folder's owner wrote) otherwise makes
        // git run `git status` inside that folder, which runs a clean
        // filter its agent-written config names (T19). With the flag git
        // still opens that repository and parses its config, so a broken
        // one makes this call fail (exit 128): then there is no count.
        let mut c = command(self, Some(&tmp));
        c.args([
            "diff-index",
            "--name-only",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            commit,
        ]);
        let tracked = entries(c, each).await?;
        let mut c = command(self, Some(&tmp));
        c.args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
            "-z",
        ]);
        let untracked = entries(c, each).await?;
        Some(tracked.saturating_add(untracked).min(MAX_GIT_CHANGED))
    }
}

/// The most bytes of a `-z` listing read: a path is at most 4,096 bytes, and
/// [`MAX_GIT_CHANGED`] of them is the most that can matter.
const LISTING_MAX: usize = (MAX_GIT_CHANGED as usize + 1) * 4097;

/// The number of names a successful `-z` listing prints, counted as it is
/// read: at [`MAX_GIT_CHANGED`] names or [`LISTING_MAX`] bytes the count is
/// the cap and git is killed with its process group, so a folder of a
/// million new files costs neither memory nor the time to list them. `None`
/// when git failed or ran past `each`.
async fn entries(mut cmd: Command, each: Duration) -> Option<u32> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().ok()?;
    let mut group = Group(child.id());
    let mut out = child.stdout.take()?;
    let work = async {
        let (mut names, mut bytes, mut buf) = (0u32, 0usize, vec![0u8; 16 << 10]);
        loop {
            let n = out.read(&mut buf).await.ok()?;
            if n == 0 {
                break;
            }
            names = names.saturating_add(buf[..n].iter().filter(|b| **b == 0).count() as u32);
            bytes += n;
            if names >= MAX_GIT_CHANGED || bytes > LISTING_MAX {
                // Enough: the group is killed when `group` drops.
                return Some(MAX_GIT_CHANGED);
            }
        }
        let status = child.wait().await.ok()?;
        // Reaped: nothing left to kill, and its pid must not be signalled.
        group.0 = None;
        status.success().then_some(names)
    };
    tokio::time::timeout(each, work).await.ok()?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::sh;

    struct Rig {
        _d: tempfile::TempDir,
        dir: PathBuf,
        base: PathBuf,
    }

    /// A repository on `main` with two committed files.
    fn rig() -> Rig {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let dir = root.join("repo");
        let base = root.join("checkpoints");
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["commit", "-qm", "init"]);
        Rig { _d: d, dir, base }
    }

    impl Rig {
        async fn state(&self) -> Option<GitState> {
            state(&self.dir, Some(&self.base), &Bounds::default()).await
        }
    }

    #[tokio::test]
    async fn a_clean_repository_is_its_branch_and_no_changes() {
        let r = rig();
        let s = r.state().await.unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        let head = std::fs::read_to_string(r.dir.join(".git/refs/heads/main")).unwrap();
        assert_eq!(s.commit.as_deref(), Some(&head.trim()[..7]));
        assert_eq!(s.changed, Some(0));
    }

    /// A modified file, a new file and a new folder count once each; a file
    /// that was only touched, and an ignored one, do not.
    #[tokio::test]
    async fn changes_untracked_files_and_folders_count_once_and_touches_and_ignored_do_not() {
        let r = rig();
        std::fs::write(r.dir.join(".gitignore"), "ignored.log\n").unwrap();
        sh(&r.dir, &["add", ".gitignore"]);
        sh(&r.dir, &["commit", "-qm", "ignore"]);
        assert_eq!(r.state().await.unwrap().changed, Some(0));
        std::thread::sleep(Duration::from_millis(20));
        let a = std::fs::read(r.dir.join("a.txt")).unwrap();
        std::fs::write(r.dir.join("a.txt"), a).unwrap(); // touched, same bytes
        std::fs::write(r.dir.join("b.txt"), "changed\n").unwrap();
        std::fs::write(r.dir.join("new.txt"), "n\n").unwrap();
        std::fs::create_dir_all(r.dir.join("fresh/deep")).unwrap();
        std::fs::write(r.dir.join("fresh/deep/x"), "x\n").unwrap();
        std::fs::write(r.dir.join("fresh/y"), "y\n").unwrap();
        std::fs::write(r.dir.join("ignored.log"), "log\n").unwrap();
        assert_eq!(r.state().await.unwrap().changed, Some(3));
        // A staged change is one entry, as is a deleted file.
        sh(&r.dir, &["add", "b.txt"]);
        std::fs::remove_file(r.dir.join("a.txt")).unwrap();
        assert_eq!(r.state().await.unwrap().changed, Some(4));
    }

    #[tokio::test]
    async fn a_folder_inside_the_repository_shows_the_repositorys_state() {
        let r = rig();
        std::fs::create_dir_all(r.dir.join("src/deep")).unwrap();
        std::fs::write(r.dir.join("top.txt"), "t\n").unwrap();
        let sub = r.dir.join("src/deep");
        let s = state(&sub, Some(&r.base), &Bounds::default())
            .await
            .unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(
            s.changed,
            Some(1),
            "top.txt; the empty folders hold nothing"
        );
    }

    #[tokio::test]
    async fn detached_head_has_a_commit_and_no_branch() {
        let r = rig();
        sh(&r.dir, &["checkout", "-q", "--detach"]);
        let s = r.state().await.unwrap();
        assert_eq!(s.branch, None);
        assert_eq!(s.commit.as_deref().map(str::len), Some(7));
        assert_eq!(s.changed, Some(0));
    }

    /// A repository just made: the branch is known, the count is not.
    #[tokio::test]
    async fn a_repository_with_no_commit_has_a_branch_and_no_count() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().canonicalize().unwrap();
        sh(&dir, &["init", "-q", "-b", "trunk"]);
        std::fs::write(dir.join("f"), "f").unwrap();
        let s = state(&dir, Some(&dir.join("cp")), &Bounds::default())
            .await
            .unwrap();
        assert_eq!(s.branch.as_deref(), Some("trunk"));
        assert_eq!((s.commit, s.changed), (None, None));
    }

    /// A node with no checkpoint store still shows the branch.
    #[tokio::test]
    async fn without_a_checkpoint_store_the_count_is_absent_not_zero() {
        let r = rig();
        let s = state(&r.dir, None, &Bounds::default()).await.unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.changed, None);
    }

    #[tokio::test]
    async fn no_repository_a_gitfile_a_corrupt_git_and_a_bare_one_show_nothing() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let cp = root.join("cp");
        let at = |n: &str| {
            let p = root.join(n);
            std::fs::create_dir_all(&p).unwrap();
            p
        };
        let none = |p: PathBuf| {
            let cp = cp.clone();
            async move { state(&p, Some(&cp), &Bounds::default()).await }
        };
        assert!(none(at("plain")).await.is_none());
        // A linked worktree or a submodule: `.git` is a file.
        let linked = at("linked");
        std::fs::write(linked.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert!(none(linked).await.is_none());
        // A `.git` with a HEAD that is not one.
        let bad = at("bad");
        std::fs::create_dir_all(bad.join(".git")).unwrap();
        std::fs::write(bad.join(".git/HEAD"), "garbage\n").unwrap();
        assert!(none(bad.clone()).await.is_none());
        std::fs::remove_file(bad.join(".git/HEAD")).unwrap();
        assert!(none(bad).await.is_none(), "no HEAD at all");
        // `.git` a link.
        let linked = at("link");
        let real = at("real");
        sh(&real, &["init", "-q"]);
        std::os::unix::fs::symlink(real.join(".git"), linked.join(".git")).unwrap();
        assert!(none(linked).await.is_none());
        // A bare repository has no `.git` entry to find.
        let bare = at("bare");
        sh(&bare, &["init", "-q", "--bare"]);
        assert!(none(bare).await.is_none());
    }

    /// The branch is the repository's text: bounded and cleaned here, the
    /// page treats it as text again.
    #[tokio::test]
    async fn a_hostile_branch_name_is_cleaned_and_cut() {
        let r = rig();
        let head = std::fs::read_to_string(r.dir.join(".git/refs/heads/main")).unwrap();
        let names = [
            "a\u{1b}[31mred\u{7}\rb\u{202e}c\u{2028}d\u{2066}e".to_string(),
            "x".repeat(4000),
            format!("{}\u{e9}", "y".repeat(199)),
            "<script>alert(1)</script>".to_string(),
            "--upload-pack=touch /tmp/owned".to_string(),
            "-rf".to_string(),
        ];
        let (count, mut done) = (names.len(), 0);
        for name in names {
            // A name git itself would refuse: the repository is hostile. A
            // loose ref file where the file system takes the name, else a
            // line of `packed-refs`.
            let file = r.dir.join(".git/refs/heads").join(&name);
            let loose = std::fs::create_dir_all(file.parent().unwrap())
                .and_then(|()| std::fs::write(&file, &head))
                .is_ok();
            if !loose {
                std::fs::write(
                    r.dir.join(".git/packed-refs"),
                    format!("{} refs/heads/{name}\n", head.trim()),
                )
                .unwrap();
            }
            std::fs::write(r.dir.join(".git/HEAD"), format!("ref: refs/heads/{name}\n")).unwrap();
            let s = r
                .state()
                .await
                .unwrap_or_else(|| panic!("no state for {name:?}"));
            let shown = s.branch.unwrap();
            assert!(shown.len() <= 200, "{}", shown.len());
            assert!(
                !shown.chars().any(hides_text),
                "{shown:?} still holds a hidden character"
            );
            if name.starts_with('<') || name.starts_with('-') {
                assert_eq!(shown, name, "plain text is shown as it is");
            }
            assert_eq!(s.commit.as_deref(), Some(&head.trim()[..7]), "{name:?}");
            let _ = std::fs::remove_file(file);
            let _ = std::fs::remove_file(r.dir.join(".git/packed-refs"));
            done += 1;
        }
        assert_eq!(done, count, "every name was tried");
    }

    /// A user index entry that is a gitlink, over a nested repository the
    /// agent wrote: its config names a clean filter and it holds a file whose
    /// stat changed. Without `--ignore-submodules=all` git runs `git status`
    /// inside it and the filter runs on the node (T19).
    fn hostile_submodule(r: &Rig) -> PathBuf {
        let marker = r.dir.parent().unwrap().join("marker");
        let sm = r.dir.join("sm");
        std::fs::create_dir_all(&sm).unwrap();
        sh(&sm, &["init", "-q", "-b", "main"]);
        std::fs::write(sm.join("f.txt"), "aaaa\n").unwrap();
        sh(&sm, &["add", "f.txt"]);
        sh(&sm, &["commit", "-qm", "sm"]);
        let head = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&sm)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        sh(
            &r.dir,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},sm", head.trim()),
            ],
        );
        sh(&r.dir, &["commit", "-qm", "gitlink"]);
        let script = r.dir.parent().unwrap().join("filter.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho ran >> {}\ncat\n", marker.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut config = std::fs::read_to_string(sm.join(".git/config")).unwrap();
        config.push_str(&format!("[filter \"x\"]\n\tclean = {}\n", script.display()));
        std::fs::write(sm.join(".git/config"), config).unwrap();
        std::fs::create_dir_all(sm.join(".git/info")).unwrap();
        std::fs::write(sm.join(".git/info/attributes"), "* filter=x\n").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(sm.join("f.txt"), "bbbb\n").unwrap();
        marker
    }

    #[tokio::test]
    async fn a_gitlink_over_a_hostile_nested_repository_runs_nothing() {
        let r = rig();
        let marker = hostile_submodule(&r);
        let s = r.state().await.unwrap();
        assert!(
            !marker.exists(),
            "the nested repository's filter ran on the node"
        );
        // What changed inside a nested repository is not counted.
        assert_eq!((s.branch.as_deref(), s.changed), (Some("main"), Some(0)));
        std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
        assert_eq!(r.state().await.unwrap().changed, Some(1));
        assert!(!marker.exists());
    }

    /// With the flag git still opens the nested repository and parses its
    /// config: a broken one fails the call. The chip shows the branch and no
    /// count, never an error.
    #[tokio::test]
    async fn a_broken_nested_config_gives_the_branch_and_no_count() {
        let r = rig();
        let marker = hostile_submodule(&r);
        std::fs::write(r.dir.join("sm/.git/config"), "[broken\n\tx = \n").unwrap();
        let s = r.state().await.unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.changed, None, "{s:?}");
        assert!(!marker.exists());
    }

    /// The folder's own `.gitattributes` names filters, and the user's
    /// `.git/config` defines them: neither is read by the node's git (the
    /// attributes come from the empty tree, the config is the shadow's).
    #[tokio::test]
    async fn a_filter_the_worktree_and_the_user_config_name_runs_nothing() {
        let r = rig();
        let marker = r.dir.parent().unwrap().join("marker");
        let script = r.dir.parent().unwrap().join("f.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho ran >> {}\ncat\n", marker.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut config = std::fs::read_to_string(r.dir.join(".git/config")).unwrap();
        config.push_str(&format!(
            "[filter \"f\"]\n\tclean = {0}\n\tsmudge = {0}\n[diff \"f\"]\n\ttextconv = {0}\n\
             [core]\n\tfsmonitor = {0}\n",
            script.display()
        ));
        std::fs::write(r.dir.join(".git/config"), config).unwrap();
        std::fs::write(r.dir.join(".gitattributes"), "* filter=f diff=f\n").unwrap();
        std::fs::create_dir_all(r.dir.join(".git/info")).unwrap();
        std::fs::write(r.dir.join(".git/info/attributes"), "* filter=f\n").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(r.dir.join("a.txt"), "A\n").unwrap();
        let s = r.state().await.unwrap();
        // a.txt changed, .gitattributes is new.
        assert_eq!(s.changed, Some(2), "{s:?}");
        assert!(!marker.exists(), "a program of the repository's config ran");
    }

    /// An index the agent wrote by hand, with entries named `../x` and
    /// `.git/config`: loaded and counted at worst, never written through.
    #[tokio::test]
    async fn a_crafted_user_index_writes_nothing_and_runs_nothing() {
        let r = rig();
        let blob = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&r.dir)
                .args(["rev-parse", "HEAD:a.txt"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let id: Vec<u8> = (0..40)
            .step_by(2)
            .map(|i| u8::from_str_radix(&blob.trim()[i..i + 2], 16).unwrap())
            .collect();
        let mut index = b"DIRC".to_vec();
        index.extend(2u32.to_be_bytes());
        let names: [&[u8]; 4] = [b".git/config", b"../x", b"a.txt", b"b.txt"];
        index.extend((names.len() as u32).to_be_bytes());
        let mut sorted = names.to_vec();
        sorted.sort();
        for name in sorted {
            let mut e = vec![0u8; 24]; // times, dev, ino
            e.extend(0o100644u32.to_be_bytes());
            e.extend([0u8; 8]); // uid, gid
            e.extend(0u32.to_be_bytes()); // size
            e.extend(&id);
            e.extend((name.len() as u16).to_be_bytes());
            e.extend(name);
            e.push(0);
            while !e.len().is_multiple_of(8) {
                e.push(0);
            }
            index.extend(e);
        }
        let sum = {
            use std::io::Write;
            let mut c = std::process::Command::new("sha1sum")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            c.stdin.take().unwrap().write_all(&index).unwrap();
            let out = c.wait_with_output().unwrap().stdout;
            let hex = String::from_utf8(out).unwrap();
            (0..40)
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<u8>>()
        };
        index.extend(sum);
        std::fs::write(r.dir.join(".git/index"), index).unwrap();
        let config_before = std::fs::read(r.dir.join(".git/config")).unwrap();
        let s = r.state().await;
        let s = s.expect("the branch is still read");
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(!r.dir.parent().unwrap().join("x").exists());
        assert_eq!(
            std::fs::read(r.dir.join(".git/config")).unwrap(),
            config_before
        );
    }

    /// A stand-in `git` for the count's three calls: `diff` and `ls` are the
    /// shell for `diff-index` and `ls-files`; it notes its pid first.
    fn fake_git(r: &Rig, diff: &str, ls: &str) -> (PathBuf, PathBuf) {
        let root = r.dir.parent().unwrap();
        let (script, pids) = (root.join("fake-git.sh"), root.join("pids"));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in\n\
                 update-index) exit 0;;\n\
                 diff-index) echo $$ >> {pids}; {diff};;\n\
                 ls-files) echo $$ >> {pids}; {ls};;\n\
                 esac; done\nexit 0\n",
                pids = pids.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (script, pids)
    }

    fn dead(pid: &str) -> bool {
        for _ in 0..100 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(t) if t.contains(") Z ") => return true,
                Ok(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        false
    }

    /// A git that runs past its timeout: no count, the process and its group
    /// dead, no private index or lock left in the shadow.
    #[tokio::test]
    async fn a_count_that_times_out_is_absent_and_leaves_nothing() {
        let r = rig();
        let (script, pids) = fake_git(&r, "exec sleep 30", "exit 0");
        let repo = repo_for(&r.dir, Some(&r.base)).await.unwrap();
        let head = std::fs::read_to_string(r.dir.join(".git/refs/heads/main")).unwrap();
        crate::git::use_test_git(Some(script));
        let t = std::time::Instant::now();
        let got = repo
            .count_changes(
                &r.dir.join(".git/index"),
                head.trim(),
                Duration::from_millis(400),
            )
            .await;
        crate::git::use_test_git(None);
        assert_eq!(got, None);
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        let pid = std::fs::read_to_string(pids).unwrap();
        assert!(
            dead(pid.lines().next().unwrap()),
            "the git process outlived the timeout"
        );
        let left: Vec<String> = std::fs::read_dir(repo.shadow.as_ref().unwrap())
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("harness-index-") || n.ends_with(".lock"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    /// A listing that never ends is cut at the cap: the count is the cap, git
    /// is killed, memory is not the listing's size.
    #[tokio::test]
    async fn an_endless_listing_is_cut_at_the_cap_and_git_is_killed() {
        let r = rig();
        let (script, pids) = fake_git(&r, "exit 0", "while :; do printf 'f\\0'; done");
        let repo = repo_for(&r.dir, Some(&r.base)).await.unwrap();
        let head = std::fs::read_to_string(r.dir.join(".git/refs/heads/main")).unwrap();
        crate::git::use_test_git(Some(script));
        let got = repo
            .count_changes(
                &r.dir.join(".git/index"),
                head.trim(),
                Duration::from_secs(10),
            )
            .await;
        crate::git::use_test_git(None);
        assert_eq!(got, Some(MAX_GIT_CHANGED));
        let pids = std::fs::read_to_string(pids).unwrap();
        assert!(
            dead(pids.lines().last().unwrap()),
            "the listing's git was not killed"
        );
    }

    /// A thousand and more entries: the count stops at the cap.
    #[tokio::test]
    async fn the_count_is_capped() {
        let r = rig();
        for i in 0..1100 {
            std::fs::write(r.dir.join(format!("n{i}")), "x").unwrap();
        }
        assert_eq!(r.state().await.unwrap().changed, Some(MAX_GIT_CHANGED));
    }

    /// A big untracked tree in one folder is one entry and does not take
    /// the time of listing it.
    #[tokio::test]
    async fn a_large_untracked_folder_counts_once() {
        let r = rig();
        let big = r.dir.join("big");
        std::fs::create_dir(&big).unwrap();
        for i in 0..20_000 {
            std::fs::write(big.join(format!("f{i}")), "").unwrap();
        }
        let t = std::time::Instant::now();
        let s = r.state().await.unwrap();
        assert_eq!(s.changed, Some(1));
        assert!(t.elapsed() < COUNT_TIMEOUT, "{:?}", t.elapsed());
    }

    /// 100,000 files, measured by hand (`cargo test ... -- --ignored
    /// --nocapture`): in a folder of their own (one entry), and the worst
    /// case, in a folder that holds a tracked file (each one an entry, so the
    /// count hits its cap): bounded by the timeout whatever the disk.
    #[tokio::test]
    #[ignore = "makes 200,000 files; run by hand"]
    async fn a_hundred_thousand_untracked_files_are_bounded() {
        let r = rig();
        let big = r.dir.join("big");
        std::fs::create_dir(&big).unwrap();
        for i in 0..100_000 {
            std::fs::write(big.join(format!("f{i}")), "").unwrap();
        }
        let t = std::time::Instant::now();
        let s = r.state().await.unwrap();
        eprintln!(
            "100,000 untracked files in a new folder: {:?} -> {s:?}",
            t.elapsed()
        );
        assert!(t.elapsed() < COUNT_TIMEOUT + Duration::from_secs(5));
        let tracked = r.dir.join("tracked");
        std::fs::create_dir(&tracked).unwrap();
        std::fs::write(tracked.join("kept"), "k").unwrap();
        sh(&r.dir, &["add", "tracked"]);
        sh(&r.dir, &["commit", "-qm", "t"]);
        for i in 0..100_000 {
            std::fs::write(tracked.join(format!("f{i}")), "").unwrap();
        }
        let t = std::time::Instant::now();
        let s = r.state().await.unwrap();
        eprintln!(
            "100,000 untracked files beside a tracked one: {:?} -> {s:?}",
            t.elapsed()
        );
        assert_eq!(s.changed, Some(MAX_GIT_CHANGED));
        assert!(t.elapsed() < COUNT_TIMEOUT + Duration::from_secs(5));
    }
}

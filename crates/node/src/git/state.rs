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
            Ok(Some(repo)) if repo.root == r.top => {
                tokio::time::timeout(COUNT_TIMEOUT, repo.count_changes(index, commit))
                    .await
                    .ok()
                    .flatten()
            }
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
    /// [`MAX_GIT_CHANGED`], or `None` when any step failed or timed out.
    async fn count_changes(&self, user_index: &Path, commit: &str) -> Option<u32> {
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
        // something needs an update.
        let mut c = command(self, Some(&tmp));
        c.args(["update-index", "-q", "--refresh"]);
        exec(c, None, STATE_TIMEOUT).await.ok()?;
        let mut c = command(self, Some(&tmp));
        c.args([
            "diff-index",
            "--name-only",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            commit,
        ]);
        let tracked = entries(exec(c, None, STATE_TIMEOUT).await.ok()?)?;
        let mut c = command(self, Some(&tmp));
        c.args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
            "-z",
        ]);
        let untracked = entries(exec(c, None, STATE_TIMEOUT).await.ok()?)?;
        Some(tracked.saturating_add(untracked).min(MAX_GIT_CHANGED))
    }
}

/// The names a successful `-z` listing holds.
fn entries(out: std::process::Output) -> Option<u32> {
    out.status.success().then(|| {
        let n = out
            .stdout
            .split(|b| *b == 0)
            .filter(|e| !e.is_empty())
            .count();
        u32::try_from(n).unwrap_or(u32::MAX)
    })
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

    /// Nothing the repository's config names runs: not an fsmonitor, a hook,
    /// a pager, an alias, a filter or an external diff; and its
    /// `status.showUntrackedFiles` does not hide what the count shows.
    #[tokio::test]
    async fn nothing_the_repository_configures_runs_and_its_settings_do_not_apply() {
        let r = rig();
        let marker = r.dir.parent().unwrap().join("marker");
        let make = |n: &str| {
            let p = r.dir.parent().unwrap().join(format!("{n}.sh"));
            std::fs::write(&p, format!("#!/bin/sh\necho {n} >> {}\n", marker.display())).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p.display().to_string()
        };
        let cfg = format!(
            "\n[core]\n\tfsmonitor = {}\n\tpager = {}\n\thooksPath = {}\n\tuntrackedCache = true\n\
             [status]\n\tshowUntrackedFiles = no\n[alias]\n\tdiff-index = !{}\n\tls-files = !{}\n\
             [filter \"f\"]\n\tclean = {}\n\tsmudge = {}\n[diff]\n\texternal = {}\n\
             [core2]\n\tx = y\n",
            make("fsmonitor"),
            make("pager"),
            r.dir.parent().unwrap().display(),
            make("alias1"),
            make("alias2"),
            make("clean"),
            make("smudge"),
            make("external"),
        );
        let config = r.dir.join(".git/config");
        let mut text = std::fs::read_to_string(&config).unwrap();
        text.push_str(&cfg);
        std::fs::write(&config, text).unwrap();
        std::fs::write(r.dir.join(".gitattributes"), "* filter=f diff=f\n").unwrap();
        // The same hooks a repository could hold.
        std::fs::create_dir_all(r.dir.join(".git/hooks")).unwrap();
        for h in ["fsmonitor-watchman", "post-index-change", "pre-commit"] {
            let p = r.dir.join(".git/hooks").join(h);
            std::fs::copy(make("hook"), &p).unwrap();
        }
        std::fs::write(r.dir.join("untracked.txt"), "u\n").unwrap();
        std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
        let s = r.state().await.unwrap();
        // a.txt, untracked.txt and .gitattributes: all three show, though the
        // config says `showUntrackedFiles = no`.
        assert_eq!(s.changed, Some(3), "{s:?}");
        assert!(!marker.exists(), "a program of the repository's config ran");
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

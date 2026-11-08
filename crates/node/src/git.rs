//! Turn checkpoints in git. In a project repository they live there, under
//! `refs/harness/`, written through a temporary index so the user's index,
//! HEAD and branches are never touched. A folder without git gets a private
//! *shadow* repository in the node's data directory instead. Either way only
//! the latest N checkpoints are kept.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

/// Where checkpoints for one working folder are stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The folder whose files are snapshotted.
    pub root: PathBuf,
    /// The private git dir for a folder that is not a repository.
    shadow: Option<PathBuf>,
}

/// Build output and dependency folders a shadow repository never snapshots.
const SHADOW_EXCLUDES: &str =
    "node_modules/\ntarget/\n.venv/\nvenv/\n__pycache__/\ndist/\nbuild/\n.cache/\n*.pyc\n";

/// A snapshot that takes longer than this is abandoned (huge folders).
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(60);

impl Repo {
    pub fn project(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            shadow: None,
        }
    }

    pub fn is_shadow(&self) -> bool {
        self.shadow.is_some()
    }
}

async fn run(repo: &Repo, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    match &repo.shadow {
        Some(dir) => {
            c.arg("--git-dir")
                .arg(dir)
                .arg("--work-tree")
                .arg(&repo.root);
        }
        None => {
            c.arg("-C").arg(&repo.root);
        }
    }
    c.args(args)
        .env("GIT_AUTHOR_NAME", "harness")
        .env("GIT_AUTHOR_EMAIL", "harness@localhost")
        .env("GIT_COMMITTER_NAME", "harness")
        .env("GIT_COMMITTER_EMAIL", "harness@localhost")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    if let Some(i) = index {
        c.env("GIT_INDEX_FILE", i);
    }
    let out = c.output().await.map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

async fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    run(&Repo::project(dir), args, index).await
}

/// The commit, branch and whether tracked files changed, for the folder a
/// published file came from (display only).
pub async fn describe(dir: &Path) -> Option<(String, String, bool)> {
    let commit = git(dir, &["rev-parse", "HEAD"], None).await.ok()?;
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"], None)
        .await
        .unwrap_or_default();
    let status = git(
        dir,
        &["status", "--porcelain", "--untracked-files=no"],
        None,
    )
    .await
    .unwrap_or_default();
    Some((commit, branch, !status.is_empty()))
}

/// The repository containing `cwd`, if any.
pub async fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git(cwd, &["rev-parse", "--show-toplevel"], None)
        .await
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Where to checkpoint `cwd`: its own repository, or else a shadow
/// repository under `shadow_base` (created on first use). `None` when the
/// folder is not a repository and no shadow base is configured.
pub async fn repo_for(cwd: &Path, shadow_base: Option<&Path>) -> Option<Repo> {
    if let Some(root) = repo_root(cwd).await {
        return Some(Repo::project(&root));
    }
    let base = shadow_base?;
    let root = cwd.canonicalize().ok()?;
    let key = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(root.to_string_lossy().as_bytes()))[..16].to_string()
    };
    let dir = base.join(format!("{key}.git"));
    if !dir.join("HEAD").exists() {
        let ok = Command::new("git")
            .args(["init", "--bare", "-q"])
            .arg(&dir)
            .output()
            .await
            .ok()?
            .status
            .success();
        if !ok {
            return None;
        }
        let _ = tokio::fs::create_dir_all(dir.join("info")).await;
        let _ = tokio::fs::write(dir.join("info/exclude"), SHADOW_EXCLUDES).await;
        let _ = tokio::fs::write(
            dir.join("harness-folder"),
            root.to_string_lossy().as_bytes(),
        )
        .await;
    }
    Some(Repo {
        root,
        shadow: Some(dir),
    })
}

/// A private index seeded from the real one (keeps its stat cache, so
/// `add -A` only rehashes what changed).
async fn temp_index(repo: &Repo) -> Result<PathBuf, String> {
    let real = PathBuf::from(
        run(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
            None,
        )
        .await?,
    );
    let tmp = real.with_file_name(format!("harness-index-{}", harness_core::new_id()));
    if real.exists() {
        tokio::fs::copy(&real, &tmp)
            .await
            .map_err(|e| e.to_string())?;
        // Keep the original mtime: git's "racy clean" check compares file
        // times with the index file's own mtime. A fresh mtime lets a
        // same-size edit made in the same second look unchanged.
        let modified = tokio::fs::metadata(&real)
            .await
            .and_then(|m| m.modified())
            .map_err(|e| e.to_string())?;
        let t = tmp.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&t)
                .and_then(|f| f.set_modified(modified))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    }
    Ok(tmp)
}

/// Tree of the working directory as it is now (tracked + untracked, minus ignored).
async fn worktree_tree(repo: &Repo) -> Result<String, String> {
    let idx = temp_index(repo).await?;
    let res = async {
        run(repo, &["add", "-A", "--", "."], Some(&idx)).await?;
        run(repo, &["write-tree"], Some(&idx)).await
    }
    .await;
    let _ = tokio::fs::remove_file(&idx).await;
    res
}

/// Snapshot the working directory into a commit stored at `ref_name`.
pub async fn snapshot(repo: &Repo, ref_name: &str, msg: &str) -> Result<String, String> {
    tokio::time::timeout(SNAPSHOT_TIMEOUT, async {
        let tree = worktree_tree(repo).await?;
        let head = run(repo, &["rev-parse", "--verify", "-q", "HEAD"], None)
            .await
            .ok();
        let mut args = vec!["commit-tree", tree.as_str(), "-m", msg];
        if let Some(h) = head.as_deref().filter(|h| !h.is_empty()) {
            args.extend(["-p", h]);
        }
        let sha = run(repo, &args, None).await?;
        run(repo, &["update-ref", ref_name, &sha], None).await?;
        record_order(repo, ref_name).await;
        Ok(sha)
    })
    .await
    .map_err(|_| "snapshot took too long (folder too large?)".to_string())?
}

/// Paths that differ between two snapshots, with git's status letter.
pub async fn changed(repo: &Repo, a: &str, b: &str) -> Result<Vec<(char, String)>, String> {
    let out = run(
        repo,
        &["diff", "--name-status", "--no-renames", "-z", a, b],
        None,
    )
    .await?;
    let parts: Vec<&str> = out.split('\0').filter(|s| !s.is_empty()).collect();
    Ok(parts
        .chunks(2)
        .filter_map(|c| match c {
            [st, path] => Some((st.chars().next().unwrap_or('M'), path.to_string())),
            _ => None,
        })
        .collect())
}

/// Whether a snapshot still exists (old ones are pruned).
pub async fn exists(repo: &Repo, sha: &str) -> bool {
    run(
        repo,
        &["cat-file", "-e", &format!("{sha}^{{commit}}")],
        None,
    )
    .await
    .is_ok()
}

/// Put the folder's files back to snapshot `from` for every path that
/// differs between `from` and `expected` (what the folder should look like
/// now). Refuses, listing them, if any of those files changed since
/// `expected`, unless `force`. Undo: `from` = before the turn, `expected`
/// = after it. Rewind: `expected` = the latest state.
pub async fn restore(
    repo: &Repo,
    from: &str,
    expected: &str,
    force: bool,
) -> Result<Vec<String>, String> {
    let touched = changed(repo, from, expected).await?;
    if touched.is_empty() {
        return Ok(vec![]);
    }
    let paths: Vec<String> = touched.iter().map(|(_, p)| p.clone()).collect();
    if !force {
        let now = worktree_tree(repo).await?;
        let mut args = vec![
            "diff".to_string(),
            "--name-only".into(),
            expected.into(),
            now,
            "--".into(),
        ];
        args.extend(paths.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let since = run(repo, &refs, None).await?;
        if !since.is_empty() {
            return Err(format!(
                "these files changed after the turn; undo would overwrite that: {}",
                since.lines().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    // Write the old versions through a private index.
    let idx = temp_index(repo).await?;
    let res = async {
        run(repo, &["read-tree", from], Some(&idx)).await?;
        let restore: Vec<&str> = touched
            .iter()
            .filter(|(st, _)| *st != 'A')
            .map(|(_, p)| p.as_str())
            .collect();
        if !restore.is_empty() {
            let mut args = vec!["checkout-index", "-f", "--"];
            args.extend(restore);
            run(repo, &args, Some(&idx)).await?;
        }
        for (st, p) in &touched {
            if *st == 'A' {
                let _ = tokio::fs::remove_file(repo.root.join(p)).await;
            }
        }
        Ok::<_, String>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&idx).await;
    res?;
    Ok(paths)
}

/// A checkpoint group: a turn's refs (`.../before`, `/after`, `/restored`)
/// or a sub-agent's (`/base`, `/result`).
fn group_of(ref_name: &str) -> &str {
    ref_name
        .rsplit_once('/')
        .map(|(g, _)| g)
        .unwrap_or(ref_name)
}

/// Checkpoints in creation order, kept in the git dir (commit dates only
/// have one-second resolution, too coarse to order turns).
async fn order_file(repo: &Repo) -> Option<PathBuf> {
    run(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        None,
    )
    .await
    .ok()
    .map(|d| PathBuf::from(d).join("harness-checkpoints"))
}

static ORDER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn record_order(repo: &Repo, ref_name: &str) {
    let Some(f) = order_file(repo).await else {
        return;
    };
    let _g = ORDER.lock().await;
    let mut text = tokio::fs::read_to_string(&f).await.unwrap_or_default();
    let g = group_of(ref_name);
    if !text.lines().any(|l| l == g) {
        text.push_str(g);
        text.push('\n');
        let _ = tokio::fs::write(&f, text).await;
    }
}

/// Keep only the `keep` most recent checkpoints (groups). In a shadow
/// repository the dropped snapshots are garbage-collected right away; in a
/// project repository git's own gc removes them later.
pub async fn prune(repo: &Repo, keep: usize) -> usize {
    let Some(f) = order_file(repo).await else {
        return 0;
    };
    let Ok(refs) = run(
        repo,
        &["for-each-ref", "--format=%(refname)", "refs/harness/"],
        None,
    )
    .await
    else {
        return 0;
    };
    let _g = ORDER.lock().await;
    let order: Vec<String> = tokio::fs::read_to_string(&f)
        .await
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let kept: Vec<String> = order.iter().rev().take(keep).rev().cloned().collect();
    let doomed: Vec<&str> = refs
        .lines()
        .filter(|r| !kept.iter().any(|k| k == group_of(r)))
        .collect();
    for r in &doomed {
        let _ = run(repo, &["update-ref", "-d", r], None).await;
    }
    let mut text = kept.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    let _ = tokio::fs::write(&f, text).await;
    if !doomed.is_empty() && repo.is_shadow() {
        let _ = run(repo, &["gc", "--prune=now", "--quiet"], None).await;
    }
    doomed.len()
}

/// Serializes applying sub-agent results to one working directory.
static APPLY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A detached worktree at `path` checked out at `commit`.
pub async fn worktree_add(root: &Path, path: &Path, commit: &str) -> Result<(), String> {
    let p = path.to_string_lossy();
    git(
        root,
        &["worktree", "add", "--detach", "-q", &p, commit],
        None,
    )
    .await
    .map(|_| ())
}

pub async fn worktree_remove(root: &Path, path: &Path) {
    let p = path.to_string_lossy();
    let _ = git(root, &["worktree", "remove", "--force", &p], None).await;
    let _ = git(root, &["worktree", "prune"], None).await;
}

/// Where sub-agent worktrees live: inside the repository's git directory,
/// so they never show up as untracked files.
pub async fn worktrees_dir(root: &Path) -> Result<PathBuf, String> {
    Ok(PathBuf::from(
        git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            None,
        )
        .await?,
    )
    .join("harness-worktrees"))
}

/// Apply the change `base..result` to the working directory at `root`
/// (files only; index and HEAD untouched). All or nothing.
pub async fn apply(root: &Path, base: &str, result: &str) -> Result<usize, String> {
    let _guard = APPLY.lock().await;
    let n = changed(&Repo::project(root), base, result).await?.len();
    if n == 0 {
        return Ok(0);
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--binary", base, result])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["apply", "--whitespace=nowarn", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        stdin
            .write_all(&out.stdout)
            .await
            .map_err(|e| e.to_string())?;
    }
    let res = child.wait_with_output().await.map_err(|e| e.to_string())?;
    if res.status.success() {
        Ok(n)
    } else {
        Err(String::from_utf8_lossy(&res.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[tokio::test]
    async fn snapshot_restore_round_trip_leaves_user_state_alone() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        sh(r, &["init", "-q", "-b", "main"]);
        std::fs::write(r.join("a.txt"), "one\n").unwrap();
        std::fs::write(r.join(".gitignore"), "secret.env\n").unwrap();
        sh(r, &["add", "."]);
        sh(r, &["commit", "-qm", "init"]);
        std::fs::write(r.join("staged.txt"), "user work\n").unwrap();
        sh(r, &["add", "staged.txt"]);
        std::fs::write(r.join("secret.env"), "KEY=1\n").unwrap();
        let root = Repo::project(&repo_root(r).await.unwrap());
        let index_before = std::fs::read(r.join(".git/index")).unwrap();

        let before = snapshot(&root, "refs/harness/s/t/before", "before")
            .await
            .unwrap();
        // The "turn": edit, delete, create.
        std::fs::write(r.join("a.txt"), "two\n").unwrap();
        std::fs::write(r.join("new.txt"), "made by agent\n").unwrap();
        std::fs::remove_file(r.join("staged.txt")).unwrap();
        let after = snapshot(&root, "refs/harness/s/t/after", "after")
            .await
            .unwrap();
        let ch = changed(&root, &before, &after).await.unwrap();
        assert_eq!(ch.len(), 3, "{ch:?}");
        assert!(
            !ch.iter().any(|(_, p)| p == "secret.env"),
            "ignored files are not captured"
        );

        // User's index and HEAD are untouched by snapshots.
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index_before);

        // Someone edits a touched file after the turn: undo refuses.
        std::fs::write(r.join("a.txt"), "three\n").unwrap();
        let e = restore(&root, &before, &after, false).await.unwrap_err();
        assert!(e.contains("a.txt"), "{e}");
        std::fs::write(r.join("a.txt"), "two\n").unwrap();
        // An unrelated later edit does not block it and survives.
        std::fs::write(r.join(".gitignore"), "secret.env\n*.log\n").unwrap();

        let files = restore(&root, &before, &after, false).await.unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "one\n");
        assert_eq!(
            std::fs::read_to_string(r.join("staged.txt")).unwrap(),
            "user work\n"
        );
        assert!(!r.join("new.txt").exists());
        assert!(r.join("secret.env").exists());
        assert!(
            std::fs::read_to_string(r.join(".gitignore"))
                .unwrap()
                .contains("*.log")
        );
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index_before);
        assert!(
            repo_root(&std::env::temp_dir().join("definitely-not-a-repo-xyz"))
                .await
                .is_none()
        );
    }
}

//! Turn checkpoints in git, without touching the user's index, HEAD or
//! branches: snapshots are commits written through a temporary index and
//! kept under `refs/harness/`.

use std::path::{Path, PathBuf};

use tokio::process::Command;

async fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    c.env("GIT_AUTHOR_NAME", "harness")
        .env("GIT_AUTHOR_EMAIL", "harness@localhost")
        .env("GIT_COMMITTER_NAME", "harness")
        .env("GIT_COMMITTER_EMAIL", "harness@localhost")
        .env("GIT_TERMINAL_PROMPT", "0");
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

/// The repository containing `cwd`, if any.
pub async fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git(cwd, &["rev-parse", "--show-toplevel"], None)
        .await
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// A private index seeded from the real one (keeps its stat cache, so
/// `add -A` only rehashes what changed).
async fn temp_index(root: &Path) -> Result<PathBuf, String> {
    let real = PathBuf::from(
        git(
            root,
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
    }
    Ok(tmp)
}

/// Tree of the working directory as it is now (tracked + untracked, minus ignored).
async fn worktree_tree(root: &Path) -> Result<String, String> {
    let idx = temp_index(root).await?;
    let res = async {
        git(root, &["add", "-A", "--", "."], Some(&idx)).await?;
        git(root, &["write-tree"], Some(&idx)).await
    }
    .await;
    let _ = tokio::fs::remove_file(&idx).await;
    res
}

/// Snapshot the working directory into a commit stored at `ref_name`.
pub async fn snapshot(root: &Path, ref_name: &str, msg: &str) -> Result<String, String> {
    let tree = worktree_tree(root).await?;
    let head = git(root, &["rev-parse", "--verify", "-q", "HEAD"], None)
        .await
        .ok();
    let mut args = vec!["commit-tree", tree.as_str(), "-m", msg];
    if let Some(h) = head.as_deref().filter(|h| !h.is_empty()) {
        args.extend(["-p", h]);
    }
    let sha = git(root, &args, None).await?;
    git(root, &["update-ref", ref_name, &sha], None).await?;
    Ok(sha)
}

/// Paths that differ between two snapshots, with git's status letter.
pub async fn changed(root: &Path, a: &str, b: &str) -> Result<Vec<(char, String)>, String> {
    let out = git(
        root,
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

/// Undo what a turn changed: put back the `before` version of every path
/// the turn touched. Refuses (listing them) if any of those files changed
/// again since `after`, unless `force`.
pub async fn restore(
    root: &Path,
    before: &str,
    after: &str,
    force: bool,
) -> Result<Vec<String>, String> {
    let touched = changed(root, before, after).await?;
    if touched.is_empty() {
        return Ok(vec![]);
    }
    let paths: Vec<String> = touched.iter().map(|(_, p)| p.clone()).collect();
    if !force {
        let now = worktree_tree(root).await?;
        let mut args = vec![
            "diff".to_string(),
            "--name-only".into(),
            after.into(),
            now,
            "--".into(),
        ];
        args.extend(paths.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let since = git(root, &refs, None).await?;
        if !since.is_empty() {
            return Err(format!(
                "these files changed after the turn; undo would overwrite that: {}",
                since.lines().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    // Write the old versions through a private index.
    let idx = temp_index(root).await?;
    let res = async {
        git(root, &["read-tree", before], Some(&idx)).await?;
        let restore: Vec<&str> = touched
            .iter()
            .filter(|(st, _)| *st != 'A')
            .map(|(_, p)| p.as_str())
            .collect();
        if !restore.is_empty() {
            let mut args = vec!["checkout-index", "-f", "--"];
            args.extend(restore);
            git(root, &args, Some(&idx)).await?;
        }
        for (st, p) in &touched {
            if *st == 'A' {
                let _ = tokio::fs::remove_file(root.join(p)).await;
            }
        }
        Ok::<_, String>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&idx).await;
    res?;
    Ok(paths)
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
    let n = changed(root, base, result).await?.len();
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
        let root = repo_root(r).await.unwrap();
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

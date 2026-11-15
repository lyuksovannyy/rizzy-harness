//! Bringing a writing sub-agent's result back into the chat's folder
//! (fix-plan A.4, PLAN.md T32): per file, three ways, all or nothing.
//!
//! Until 2026-11-15 a child's whole result was refused when any file it
//! changed had also changed in the folder since its base (the parent, or a
//! sibling that landed first), even when the two changes touched different
//! lines; the refused work then sat in the checkpoint store where neither
//! the agent nor the person could reach it. Now, for every path the child
//! changed (`B` the base it started from, `N` the folder's snapshot now,
//! `C` the child's result; gitlinks are left alone as before):
//!
//! | case | what happens |
//! |---|---|
//! | `N == B` (only the child changed it) | the child's version is taken, a deletion is a deletion |
//! | `N == C` (both made the same change) | nothing to write |
//! | only the folder changed it | not in the child's diff: the folder's version stays |
//! | both changed text content | `git merge-file --object-id` on the three blobs in the shadow: clean is taken, overlapping hunks are a conflict |
//! | deleted on one side, changed on the other | conflict |
//! | added on both sides with different content | conflict |
//! | a symbolic link (or a nested repository) on any side, both changed | conflict |
//! | both changed the mode, to different modes | conflict; one side's mode change is kept with the other's content |
//! | binary content, both changed | conflict (git's own test for binary) |
//! | a side over [`MERGE_BYTES`], both changed | conflict |
//! | a rename | git's raw diff without rename detection: a deletion and an addition, each judged as above |
//! | the child writes a path where the folder now has a folder, or below a path the folder made a file | conflict |
//! | a file the folder changed after the merge was computed and before the write | conflict (checked against a second snapshot) |
//!
//! Any conflict: nothing of that child is written. Otherwise one target
//! commit (the folder's snapshot with the child's and the merged versions
//! put in) is written with the restore machinery (`restore_plan`,
//! `secure_blobs`, `restore_paths`: paths checked inside the folder,
//! written relative to directories opened without following links, bounded
//! by `WriteLimits::SUB_AGENT`), so the turn's own checkpoints, its Undo
//! and its diff see the merge like any other change of the turn.
//!
//! Every git call is the shadow's (`command`): cleared environment, no
//! user or system config, the overrides, attributes from the empty tree,
//! the jail where the node has one. `merge-file` gets object ids, never a
//! path, so no attribute, merge driver, filter or `textconv` of the
//! repository applies, and a file named like an option is never an
//! argument; index paths go in NUL-terminated on stdin.

use std::collections::{HashMap, HashSet};

use super::{
    APPLY, DiffRow, GIT_TIMEOUT, IndexFiles, Names, Repo, SHOWN_MAX, WriteLimits, command,
    diff_raw, exec, fresh_index, hold, parents, restore_diff, restore_paths, restore_plan, run,
    run_raw, secure_blobs, shown, snapshot,
};

/// A file larger than this on any side is not merged line by line when
/// both changed it (a conflict instead): git's merge holds all three in
/// memory, and a text file this size is generated, not written by hand.
pub const MERGE_BYTES: u64 = 1 << 20;

/// What applying a child's result did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Files written or deleted in the folder.
    pub written: usize,
    /// Of those, the files whose content was merged with the folder's own
    /// changes.
    pub merged: Names,
    /// Files the folder already held exactly as the child left them.
    pub same: usize,
}

/// Why nothing of a child's result was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotApplied {
    /// Files both sides changed in ways that do not combine, with why
    /// (shown form, in git's path order).
    Conflict(Vec<(String, &'static str)>),
    /// Anything else (a git failure, a path outside the folder, a limit).
    Failed(String),
}

impl NotApplied {
    /// In words for the parent model and the person (one line).
    pub fn text(&self) -> String {
        match self {
            Self::Failed(e) => e.clone(),
            Self::Conflict(files) => {
                let mut s = files
                    .iter()
                    .take(SHOWN_MAX)
                    .map(|(p, why)| format!("`{p}` ({why})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if files.len() > SHOWN_MAX {
                    s.push_str(&format!(" and {} more", files.len() - SHOWN_MAX));
                }
                format!("these files changed in the folder too, in ways that do not combine: {s}")
            }
        }
    }
}

/// One side of a path: git's mode (`000000` when absent) and object id.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Side {
    mode: String,
    id: String,
}

impl Side {
    fn old(r: &DiffRow) -> Self {
        Self {
            mode: r.old_mode.clone(),
            id: r.old_id.clone(),
        }
    }
    fn new(r: &DiffRow) -> Self {
        Self {
            mode: r.new_mode.clone(),
            id: r.new_id.clone(),
        }
    }
    fn absent(&self) -> bool {
        self.mode.bytes().all(|b| b == b'0')
    }
    fn special(&self) -> bool {
        self.mode == "120000" || self.mode == "160000"
    }
}

const BOTH_LINES: &str = "both changed the same lines";
const DELETED: &str = "deleted on one side, changed on the other";
const ADDED: &str = "added on both sides with different content";
const LINK: &str = "a link or nested repository changed on both sides";
const MODE: &str = "both changed its mode";
const BINARY: &str = "binary content changed on both sides";
const LARGE: &str = "too large to merge line by line (over 1 MiB) and changed on both sides";
const CLASH: &str = "a file and a folder of the same name";
const NO_MERGE: &str = "git could not merge it";
const DURING: &str = "changed in the folder while the merge ran";

/// Apply what changed from `base` to `result` to the folder (see the
/// module's table). `between` runs after the merge is computed and before
/// the folder is checked again (tests: an edit during the merge).
pub(super) async fn apply(
    repo: &Repo,
    base: &str,
    result: &str,
    child: &str,
    between: Option<&(dyn Fn() + Send + Sync)>,
) -> Result<Applied, NotApplied> {
    let _guard = APPLY.lock().await;
    let _busy = hold(repo).await;
    let failed = NotApplied::Failed;
    let rows: Vec<DiffRow> = diff_raw(repo, base, result)
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|r| !r.gitlink)
        .collect();
    if rows.is_empty() {
        return Ok(Applied::default());
    }
    let now = snapshot(
        repo,
        &format!("refs/harness/{child}/now"),
        "harness: folder before a sub-agent's result",
    )
    .await
    .map_err(failed)?;
    let folder: HashMap<Vec<u8>, DiffRow> = diff_raw(repo, base, &now)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|r| (r.path.clone(), r))
        .collect();
    // Every folder above a path the folder changed, to tell a path the
    // child writes where the folder now has a folder.
    let folder_dirs: HashSet<&[u8]> = folder.keys().flat_map(|p| parents(p)).collect();
    let mut entries: Vec<(Side, Vec<u8>)> = Vec::new();
    let mut merged: Vec<Vec<u8>> = Vec::new();
    let mut same = 0usize;
    let mut conflicts: Vec<(String, &'static str)> = Vec::new();
    for r in &rows {
        let (b, c) = (Side::old(r), Side::new(r));
        let n = folder
            .get(&r.path)
            .map(Side::new)
            .unwrap_or_else(|| b.clone());
        if n == c {
            same += 1;
            continue;
        }
        if n == b {
            let clash = folder_dirs.contains(r.path.as_slice())
                || parents(&r.path).any(|d| folder.contains_key(d));
            if clash {
                conflicts.push((shown(&r.path), CLASH));
            } else {
                entries.push((c, r.path.clone()));
            }
            continue;
        }
        match three_way(repo, &b, &n, &c).await {
            Ok(m) if m == n => same += 1,
            Ok(m) => {
                merged.push(r.path.clone());
                entries.push((m, r.path.clone()));
            }
            Err(why) => conflicts.push((shown(&r.path), why)),
        }
    }
    if !conflicts.is_empty() {
        return Err(NotApplied::Conflict(conflicts));
    }
    if entries.is_empty() {
        return Ok(Applied {
            same,
            ..Default::default()
        });
    }
    let target = target_commit(repo, &now, &entries, child)
        .await
        .map_err(failed)?;
    let diff = restore_diff(repo, &target, &now).await.map_err(failed)?;
    if let Some(f) = between {
        f();
    }
    // Changed since: a file the person, another chat or a process changed
    // after `now` is not overwritten (and nothing else of this child is
    // written either).
    let again = snapshot(
        repo,
        &format!("refs/harness/{child}/check"),
        "harness: folder right before a sub-agent's result is written",
    )
    .await
    .map_err(failed)?;
    let moved: HashSet<Vec<u8>> = diff_raw(repo, &now, &again)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|r| r.path)
        .collect();
    let during: Vec<(String, &'static str)> = diff
        .rows
        .iter()
        .filter(|r| moved.contains(&r.path))
        .map(|r| (shown(&r.path), DURING))
        .collect();
    if !during.is_empty() {
        return Err(NotApplied::Conflict(during));
    }
    let plan = restore_plan(repo, &diff, &again, true)
        .await
        .map_err(failed)?;
    secure_blobs(repo, &plan).await.map_err(failed)?;
    restore_paths(repo, &plan, WriteLimits::SUB_AGENT)
        .await
        .map_err(failed)?;
    Ok(Applied {
        written: plan.touched.len(),
        merged: Names::of(&merged),
        same,
    })
}

/// The version of a path both the folder (`n`) and the child (`c`)
/// changed from `b`, or why there is none.
async fn three_way(repo: &Repo, b: &Side, n: &Side, c: &Side) -> Result<Side, &'static str> {
    if n.absent() || c.absent() {
        return Err(DELETED);
    }
    if b.absent() {
        return Err(ADDED);
    }
    if b.special() || n.special() || c.special() {
        return Err(LINK);
    }
    let mode = if c.mode != b.mode {
        if n.mode != b.mode && n.mode != c.mode {
            return Err(MODE);
        }
        c.mode.clone()
    } else {
        n.mode.clone()
    };
    if n.id == b.id || n.id == c.id {
        return Ok(Side {
            mode,
            id: c.id.clone(),
        });
    }
    if c.id == b.id {
        return Ok(Side {
            mode,
            id: n.id.clone(),
        });
    }
    // All three differ: sizes first, so nothing large is merged.
    let ids = format!("{}\n{}\n{}\n", b.id, n.id, c.id);
    let sizes = run_raw(
        repo,
        &["cat-file", "--batch-check=%(objecttype) %(objectsize)"],
        None,
        Some(ids.into_bytes()),
    )
    .await
    .map_err(|_| NO_MERGE)?;
    let text = String::from_utf8_lossy(&sizes);
    let mut seen = 0;
    for l in text.lines() {
        match l.split_once(' ') {
            Some(("blob", n)) => match n.parse::<u64>() {
                Ok(n) if n <= MERGE_BYTES => seen += 1,
                Ok(_) => return Err(LARGE),
                Err(_) => return Err(NO_MERGE),
            },
            _ => return Err(NO_MERGE),
        }
    }
    if seen != 3 {
        return Err(NO_MERGE);
    }
    // Git's own test for binary content (a NUL in the first 8,000 bytes),
    // asked here so the reason is ours and not read from git's message.
    for id in [&b.id, &n.id, &c.id] {
        let bytes = run_raw(repo, &["cat-file", "blob", id], None, None)
            .await
            .map_err(|_| NO_MERGE)?;
        if bytes.iter().take(8000).any(|x| *x == 0) {
            return Err(BINARY);
        }
    }
    // Object ids only: no path of the folder is an argument, and git's
    // merge of three blobs reads no attribute and runs no merge driver.
    let mut cmd = command(repo, None);
    cmd.args(["merge-file", "--object-id", "-q", &n.id, &b.id, &c.id]);
    let out = exec(cmd, None, GIT_TIMEOUT).await.map_err(|_| NO_MERGE)?;
    match out.status.code() {
        Some(0) => {
            let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if id.len() == b.id.len() && id.bytes().all(|x| x.is_ascii_hexdigit()) {
                Ok(Side { mode, id })
            } else {
                Err(NO_MERGE)
            }
        }
        Some(k) if (1..128).contains(&k) => Err(BOTH_LINES),
        _ => Err(NO_MERGE),
    }
}

/// The folder's snapshot `now` with `entries` put in (an absent side
/// deletes the path), as a commit at `refs/harness/<child>/merged`.
async fn target_commit(
    repo: &Repo,
    now: &str,
    entries: &[(Side, Vec<u8>)],
    child: &str,
) -> Result<String, String> {
    let idx = fresh_index(repo).await?;
    let _files = IndexFiles(idx.clone());
    run(repo, &["read-tree", now], Some(&idx)).await?;
    let zero = "0".repeat(now.len());
    // Deletions first: a file the child put where it removed a folder (or
    // the other way round) then finds the place free.
    let mut input = Vec::new();
    let (gone, put): (Vec<_>, Vec<_>) = entries.iter().partition(|(s, _)| s.absent());
    for (s, p) in gone.iter().chain(put.iter()) {
        if s.absent() {
            input.extend_from_slice(format!("0 {zero}\t").as_bytes());
        } else {
            input.extend_from_slice(format!("{} {}\t", s.mode, s.id).as_bytes());
        }
        input.extend_from_slice(p);
        input.push(0);
    }
    run_raw(
        repo,
        &["update-index", "--replace", "-z", "--index-info"],
        Some(&idx),
        Some(input),
    )
    .await?;
    let tree = run(repo, &["write-tree"], Some(&idx)).await?;
    let commit = run(
        repo,
        &[
            "commit-tree",
            "--no-gpg-sign",
            &tree,
            "-m",
            "harness: sub-agent result merged with the folder",
        ],
        None,
    )
    .await?;
    run(
        repo,
        &[
            "update-ref",
            &format!("refs/harness/{child}/merged"),
            &commit,
        ],
        None,
    )
    .await?;
    Ok(commit)
}

/// The most characters of an unapplied child's diff put into the parent's
/// `delegate` result (the report itself may take 24,000).
pub const UNAPPLIED_CAP: usize = 16_000;

/// A child's diff against its base as plain lines for the parent model
/// (each later quoted by `harness_core::subagent::render`), at most
/// [`UNAPPLIED_CAP`] characters, files in git's path order; and, when
/// something was left out, a sentence saying what (the harness's words,
/// for the note). In order rather than largest first: the model sees the
/// files as `git diff` would list them, and a cut always falls at a file's
/// boundary, the rest named with their counts.
pub fn unapplied_text(d: &harness_protocol::TurnDiff) -> (String, Option<String>) {
    use harness_protocol::{DiffContent, DiffStatus};
    let mut out = String::new();
    let mut left: Vec<String> = Vec::new();
    for f in &d.files {
        let status = match f.status {
            DiffStatus::Added => "added",
            DiffStatus::Deleted => "deleted",
            DiffStatus::Modified => "modified",
            DiffStatus::Renamed => "renamed",
            DiffStatus::TypeChanged => "type changed",
        };
        let mut s = match &f.old_path {
            Some(o) => format!("--- {o} -> {} ({status}", f.path),
            None => format!("--- {} ({status}", f.path),
        };
        s.push_str(&format!(", +{} -{})\n", f.added, f.removed));
        match f.content {
            DiffContent::Text => {}
            DiffContent::Binary => s.push_str("(binary content, not shown)\n"),
            DiffContent::TooLarge => s.push_str("(too large to show)\n"),
            DiffContent::Omitted => s.push_str("(left out of the diff)\n"),
        }
        for h in &f.hunks {
            s.push_str(&format!(
                "@@ -{},{} +{},{} @@",
                h.old_start, h.old_lines, h.new_start, h.new_lines
            ));
            if !h.section.is_empty() {
                s.push(' ');
                s.push_str(&h.section);
            }
            s.push('\n');
            for l in &h.lines {
                s.push_str(l);
                s.push('\n');
            }
        }
        if left.is_empty() && out.chars().count() + s.chars().count() <= UNAPPLIED_CAP {
            out.push_str(&s);
        } else {
            left.push(format!("`{}` (+{} -{})", f.path, f.added, f.removed));
        }
    }
    let mut cut = Vec::new();
    if !left.is_empty() {
        let n = left.len();
        let mut named: Vec<String> = left.into_iter().take(SHOWN_MAX).collect();
        if n > SHOWN_MAX {
            named.push(format!("and {} more", n - SHOWN_MAX));
        }
        cut.push(format!(
            "{n} of its {} files are not shown (over {UNAPPLIED_CAP} characters): {}",
            d.files.len(),
            named.join(", ")
        ));
    }
    if d.more_files > 0 {
        cut.push(format!(
            "{} more changed files are not listed",
            d.more_files
        ));
    }
    if d.cut {
        cut.push("some files' lines were cut by the diff's own limits".to_string());
    }
    let out = out.trim_end_matches('\n').to_string();
    (out, (!cut.is_empty()).then(|| cut.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_protocol::{DiffContent, DiffFile, DiffHunk, DiffStatus, TurnDiff};

    fn file(path: &str, lines: usize) -> DiffFile {
        DiffFile {
            path: path.into(),
            old_path: None,
            status: DiffStatus::Modified,
            content: DiffContent::Text,
            old_mode: Some("100644".into()),
            new_mode: Some("100644".into()),
            old_size: None,
            new_size: None,
            added: lines as u32,
            removed: 0,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: lines as u32,
                section: String::new(),
                lines: (0..lines).map(|i| format!("+line {i} of {path}")).collect(),
            }],
            cut: false,
        }
    }

    /// Files in order until the cap; the cut falls at a file's boundary and
    /// the rest are named with their counts, in the harness's sentence.
    #[test]
    fn an_unapplied_diff_is_cut_at_a_file_boundary_and_says_so() {
        let d = TurnDiff {
            files: vec![file("a.rs", 3), file("big.rs", 2000), file("c.rs", 1)],
            ..Default::default()
        };
        let (text, cut) = unapplied_text(&d);
        assert!(text.starts_with("--- a.rs (modified, +3 -0)\n@@ -1,0 +1,3 @@\n+line 0"));
        assert!(text.chars().count() <= UNAPPLIED_CAP);
        assert!(!text.contains("big.rs") && !text.contains("c.rs"), "{text}");
        let cut = cut.expect("a cut");
        assert!(
            cut.contains("2 of its 3 files are not shown")
                && cut.contains("`big.rs` (+2000 -0), `c.rs` (+1 -0)"),
            "{cut}"
        );
        let (all, none) = unapplied_text(&TurnDiff {
            files: vec![file("a.rs", 1)],
            ..Default::default()
        });
        assert_eq!(
            all,
            "--- a.rs (modified, +1 -0)\n@@ -1,0 +1,1 @@\n+line 0 of a.rs"
        );
        assert_eq!(none, None);
    }
}

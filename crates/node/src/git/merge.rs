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

use std::time::Duration;

use super::{
    APPLY, DiffRow, GIT_TIMEOUT, IndexFiles, Names, Repo, SHOWN_MAX, WriteLimits, command,
    diff_raw, exec, fresh_index, hold, parents, rel_path, restore_diff, restore_paths,
    restore_plan, run, run_raw, secure_blobs, shown, snapshot,
};

/// A file larger than this on any side is not merged line by line when
/// both changed it (a conflict instead): git's merge holds all three in
/// memory, and a text file this size is generated, not written by hand.
pub const MERGE_BYTES: u64 = 1 << 20;

/// The most files changed on both sides that one child's merge-back
/// merges line by line (three git processes each: about 4 ms a file
/// unjailed, measured in the review of PR 118, several times that in the
/// jail). Past it the child is a conflict: the node-wide lock that
/// serialises merge-backs is not held for minutes.
pub const MAX_TWO_SIDED: usize = 500;

/// The longest the line-by-line merging of one child may take (the lock
/// is held meanwhile); past it nothing is written and the parent gets the
/// child's diff.
pub const MERGE_DEADLINE: Duration = Duration::from_secs(60);

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
    /// Nested repositories (gitlinks) the child changed, never applied.
    pub nested_skipped: Names,
}

/// Why a child's result was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotApplied {
    /// Files both sides changed in ways that do not combine, with why
    /// (shown form, in git's path order). Nothing was written.
    Conflict(Vec<(String, &'static str)>),
    /// Anything else (a git failure, a path outside the folder, a limit).
    /// Nothing was written: a write that failed partway was put back.
    Failed(String),
    /// A write failed partway and putting the folder back failed too: these
    /// files hold the child's version, and `before` (a snapshot in the
    /// node's sub-agent checkpoint store) holds the folder as it was right
    /// before the write.
    Partial {
        error: String,
        put_back: String,
        written: Names,
        before: String,
    },
}

impl NotApplied {
    /// In words for the parent model and the person (one line).
    pub fn text(&self) -> String {
        match self {
            Self::Failed(e) => e.clone(),
            Self::Partial {
                error,
                put_back,
                written,
                before,
            } => format!(
                "writing them failed partway ({error}) and putting the folder back failed too \
                 ({put_back}): these files now hold the sub-agent's version: {}; the folder as \
                 it was right before is snapshot {before} in this node's checkpoint store \
                 (sub-agent snapshots)",
                if written.is_empty() {
                    "none found".to_string()
                } else {
                    written.listed()
                }
            ),
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

    /// Whether the folder holds nothing of the child (all but `Partial`).
    pub fn nothing_written(&self) -> bool {
        !matches!(self, Self::Partial { .. })
    }
}

/// Test hooks: `between` runs after the merge is computed and before the
/// folder is checked again; `before_put_back` after a write failed and
/// before the folder is put back.
#[derive(Default, Clone, Copy)]
pub(crate) struct Hooks<'a> {
    pub between: Option<&'a (dyn Fn() + Send + Sync)>,
    pub before_put_back: Option<&'a (dyn Fn() + Send + Sync)>,
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
const IGNORED: &str = "a file of yours that git ignores is in the way";
const TOO_MANY: &str = "one of more than 500 files changed on both sides, too many to merge";

/// Apply what changed from `base` to `result` to the folder (see the
/// module's table).
pub(super) async fn apply(
    repo: &Repo,
    base: &str,
    result: &str,
    child: &str,
    hooks: Hooks<'_>,
) -> Result<Applied, NotApplied> {
    let _guard = APPLY.lock().await;
    let _busy = hold(repo).await;
    let failed = NotApplied::Failed;
    let (gitlinks, rows): (Vec<DiffRow>, Vec<DiffRow>) = diff_raw(repo, base, result)
        .await
        .map_err(failed)?
        .into_iter()
        .partition(|r| r.gitlink);
    let nested_skipped = Names::of(&gitlinks.iter().map(|r| r.path.clone()).collect::<Vec<_>>());
    if rows.is_empty() {
        return Ok(Applied {
            nested_skipped,
            ..Default::default()
        });
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
    let side_n = |r: &DiffRow| {
        folder
            .get(&r.path)
            .map(Side::new)
            .unwrap_or_else(|| Side::old(r))
    };
    // Bounded work under the lock (review of PR 118, M2).
    let two_sided: Vec<&DiffRow> = rows
        .iter()
        .filter(|r| {
            let n = side_n(r);
            n != Side::old(r) && n != Side::new(r)
        })
        .collect();
    if two_sided.len() > MAX_TWO_SIDED {
        return Err(NotApplied::Conflict(
            two_sided
                .iter()
                .map(|r| (shown(&r.path), TOO_MANY))
                .collect(),
        ));
    }
    let mut entries: Vec<(Side, Vec<u8>)> = Vec::new();
    let mut merged: Vec<Vec<u8>> = Vec::new();
    let mut same = 0usize;
    let mut conflicts: Vec<(String, &'static str)> = Vec::new();
    let merging = async {
        for r in &rows {
            let (b, c) = (Side::old(r), Side::new(r));
            let n = side_n(r);
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
    };
    if tokio::time::timeout(MERGE_DEADLINE, merging).await.is_err() {
        return Err(failed(format!(
            "merging the files both sides changed took longer than {} s",
            MERGE_DEADLINE.as_secs()
        )));
    }
    if !conflicts.is_empty() {
        return Err(NotApplied::Conflict(conflicts));
    }
    if entries.is_empty() {
        return Ok(Applied {
            same,
            nested_skipped,
            ..Default::default()
        });
    }
    let target = target_commit(repo, &now, &entries, child)
        .await
        .map_err(failed)?;
    let diff = restore_diff(repo, &target, &now).await.map_err(failed)?;
    if let Some(f) = hooks.between {
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
    // A path the plan adds that is on disk all the same is a file no
    // snapshot holds: one git ignores (the person's `.env`, which a child
    // that un-ignored it would overwrite with nothing to bring it back;
    // review of PR 118, H3). So is a file in the way of a folder it adds.
    let in_way = ignored_in_the_way(repo, &diff.rows).await;
    if !in_way.is_empty() {
        return Err(NotApplied::Conflict(
            in_way.iter().map(|p| (shown(p), IGNORED)).collect(),
        ));
    }
    let plan = restore_plan(repo, &diff, &again, true)
        .await
        .map_err(failed)?;
    secure_blobs(repo, &plan).await.map_err(failed)?;
    if let Err(e) = restore_paths(repo, &plan, WriteLimits::SUB_AGENT).await {
        // Not atomic across files: what was written is put back from the
        // snapshot taken right before, so "nothing was written" is true
        // (review of PR 118, H2).
        if let Some(f) = hooks.before_put_back {
            f();
        }
        return Err(match put_back(repo, &again, &target, child).await {
            Ok(()) => failed(format!(
                "{e}; the files written before that were put back as they were"
            )),
            Err(put_back) => {
                let touched: HashSet<&[u8]> = diff.rows.iter().map(|r| r.path.as_slice()).collect();
                let written = match snapshot(
                    repo,
                    &format!("refs/harness/{child}/broken"),
                    "harness: folder after a failed sub-agent write",
                )
                .await
                {
                    Ok(after) => diff_raw(repo, &again, &after)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|r| touched.contains(r.path.as_slice()))
                        .map(|r| r.path)
                        .collect(),
                    Err(_) => Vec::new(),
                };
                NotApplied::Partial {
                    error: e,
                    put_back,
                    written: Names::of(&written),
                    before: again,
                }
            }
        });
    }
    Ok(Applied {
        written: plan.touched.len(),
        merged: Names::of(&merged),
        same,
        nested_skipped,
    })
}

/// Put back every path `target` changed against `again` (the folder right
/// before the write) as `again` holds it.
async fn put_back(repo: &Repo, again: &str, target: &str, child: &str) -> Result<(), String> {
    let d = restore_diff(repo, again, target).await?;
    let now = snapshot(
        repo,
        &format!("refs/harness/{child}/failed"),
        "harness: folder after a sub-agent write failed partway",
    )
    .await?;
    let plan = restore_plan(repo, &d, &now, false).await?;
    secure_blobs(repo, &plan).await?;
    restore_paths(repo, &plan, WriteLimits::UNDO)
        .await
        .map(|_| ())
}

/// The paths of `rows` (a restore of the target over the folder's
/// snapshot) that the snapshot does not hold (`new` side absent) but that
/// are on disk, or below a parent that is on disk as something other than
/// a folder: files no snapshot holds, which the write would replace.
async fn ignored_in_the_way(repo: &Repo, rows: &[DiffRow]) -> Vec<Vec<u8>> {
    let root = repo.root.clone();
    let added: Vec<Vec<u8>> = rows
        .iter()
        .filter(|r| !r.gitlink && Side::new(r).absent())
        .map(|r| r.path.clone())
        .collect();
    if added.is_empty() {
        return Vec::new();
    }
    // A parent the plan itself deletes (a file the child replaced by a
    // folder) is not in the way.
    let planned: HashSet<Vec<u8>> = rows.iter().map(|r| r.path.clone()).collect();
    tokio::task::spawn_blocking(move || {
        added
            .into_iter()
            .filter(|p| {
                std::fs::symlink_metadata(root.join(rel_path(p))).is_ok()
                    || parents(p).any(|d| {
                        !planned.contains(d)
                            && std::fs::symlink_metadata(root.join(rel_path(d)))
                                .is_ok_and(|m| !m.is_dir())
                    })
            })
            .collect()
    })
    .await
    .unwrap_or_default()
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
    // One `cat-file --batch` for the three (each at most `MERGE_BYTES`).
    let raw = run_raw(
        repo,
        &["cat-file", "--batch"],
        None,
        Some(format!("{}\n{}\n{}\n", b.id, n.id, c.id).into_bytes()),
    )
    .await
    .map_err(|_| NO_MERGE)?;
    let mut rest: &[u8] = &raw;
    for _ in 0..3 {
        // `<id> blob <size>\n<bytes>\n`
        let nl = rest.iter().position(|x| *x == b'\n').ok_or(NO_MERGE)?;
        let size: usize = String::from_utf8_lossy(&rest[..nl])
            .rsplit(' ')
            .next()
            .and_then(|x| x.parse().ok())
            .ok_or(NO_MERGE)?;
        rest = &rest[nl + 1..];
        if rest.len() < size + 1 {
            return Err(NO_MERGE);
        }
        if rest[..size].iter().take(8000).any(|x| *x == 0) {
            return Err(BINARY);
        }
        rest = &rest[size + 1..];
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
/// `delegate` result, counted as the parent receives them: each line with
/// its `| ` and its line break (the report itself may take 24,000).
pub const UNAPPLIED_CAP: usize = 16_000;

/// The most file names listed at the head of an unapplied diff.
const LISTED_MAX: usize = 200;

/// A child's diff against its base as plain lines for the parent model
/// (each later quoted by `harness_core::subagent::render`), at most
/// [`UNAPPLIED_CAP`] characters as quoted; and, when something was left
/// out, a sentence saying what (the harness's words, for the note).
///
/// First every changed file's name with its counts (up to 200), so one
/// large file never hides the others; then the diffs, in git's path order,
/// each whole or not at all (a cut never falls inside a hunk; one that
/// does not fit is left out and the smaller ones after it still go).
pub fn unapplied_text(d: &harness_protocol::TurnDiff) -> (String, Option<String>) {
    use harness_protocol::{DiffContent, DiffStatus};
    // As quoted: `| ` before and a line break after every line.
    let cost = |s: &str| s.lines().map(|l| l.chars().count() + 3).sum::<usize>();
    let status = |s: DiffStatus| match s {
        DiffStatus::Added => "added",
        DiffStatus::Deleted => "deleted",
        DiffStatus::Modified => "modified",
        DiffStatus::Renamed => "renamed",
        DiffStatus::TypeChanged => "type changed",
    };
    let name = |f: &harness_protocol::DiffFile| match &f.old_path {
        Some(o) => format!("{o} -> {}", f.path),
        None => f.path.clone(),
    };
    let mut out = format!("changed files ({}):\n", d.files.len());
    for f in d.files.iter().take(LISTED_MAX) {
        let line = format!(
            "  {} ({}, +{} -{})\n",
            name(f),
            status(f.status),
            f.added,
            f.removed
        );
        if cost(&out) + cost(&line) > UNAPPLIED_CAP / 2 {
            break;
        }
        out.push_str(&line);
    }
    let listed = out.lines().count() - 1;
    let mut shown = 0usize;
    for f in &d.files {
        let mut s = format!("--- {} ({})\n", name(f), status(f.status));
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
        // One that does not fit is left out; smaller ones after it still go.
        if cost(&out) + cost(&s) > UNAPPLIED_CAP {
            continue;
        }
        out.push_str(&s);
        shown += 1;
    }
    let mut cut = Vec::new();
    if listed < d.files.len() {
        cut.push(format!(
            "{} of its {} files are not named",
            d.files.len() - listed,
            d.files.len()
        ));
    }
    if shown < d.files.len() {
        cut.push(format!(
            "the diffs of {} of its {} files are not shown (over {UNAPPLIED_CAP} characters)",
            d.files.len() - shown,
            d.files.len()
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
    if d.files.is_empty() {
        out.clear();
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

    /// Every name first, then the diffs in order until the cap, counted as
    /// quoted; one large file does not hide the others' names.
    #[test]
    fn an_unapplied_diff_names_every_file_and_is_cut_at_a_file_boundary() {
        let d = TurnDiff {
            files: vec![file("big.rs", 7000), file("b.rs", 3), file("c.rs", 1)],
            ..Default::default()
        };
        let (text, cut) = unapplied_text(&d);
        assert!(
            text.starts_with(
                "changed files (3):\n  big.rs (modified, +7000 -0)\n  b.rs (modified, +3 -0)\n  \
                 c.rs (modified, +1 -0)\n"
            ),
            "{text}"
        );
        let quoted: usize = text.lines().map(|l| l.chars().count() + 3).sum();
        assert!(quoted <= UNAPPLIED_CAP, "{quoted}");
        assert!(!text.contains("+line 0 of big.rs"), "{text}");
        assert!(
            text.contains("+line 2 of b.rs") && text.contains("+line 0 of c.rs"),
            "{text}"
        );
        let cut = cut.expect("a cut");
        assert!(
            cut.contains("the diffs of 1 of its 3 files are not shown"),
            "{cut}"
        );
        let (all, none) = unapplied_text(&TurnDiff {
            files: vec![file("a.rs", 1)],
            ..Default::default()
        });
        assert_eq!(
            all,
            "changed files (1):\n  a.rs (modified, +1 -0)\n--- a.rs (modified)\n@@ -1,0 +1,1 @@\n+line 0 of a.rs"
        );
        assert_eq!(none, None);
        // 7,000 one-character lines: within the cap once quoted.
        let mut one = file("m.rs", 0);
        one.hunks[0].lines = (0..7000).map(|_| "+".to_string()).collect();
        let (text, _) = unapplied_text(&TurnDiff {
            files: vec![one],
            ..Default::default()
        });
        assert!(text.lines().map(|l| l.chars().count() + 3).sum::<usize>() <= UNAPPLIED_CAP);
    }
}

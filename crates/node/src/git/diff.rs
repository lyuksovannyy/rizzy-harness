//! What a turn changed, as a diff (X.12, PLAN.md T53): read from the two
//! checkpoints the turn already has, in the shadow repository, by the same
//! confined git as undo (`command`: cleared environment, overrides, the
//! jail, attributes from the empty tree, so no filter, driver or textconv
//! of the folder's runs). Nothing is written: no ref, no index, no object.
//!
//! Three git calls at most, each bounded:
//! 1. `diff --raw -z -M` of the two commits: every changed path as bytes,
//!    renames paired (git's own rename limit applies), with both blob ids;
//! 2. `cat-file --batch-check` of those ids: their sizes, so nothing large
//!    is ever asked for line by line;
//! 3. `diff -p` of the two commits limited to the files that fit the
//!    budgets, by literal pathspec after `--` (a name that looks like an
//!    option or a glob is a name). Its sections are matched to the rows by
//!    their full `index <old>..<new>` ids, so no path is parsed out of a
//!    patch header.
//!
//! The rows are the ones `changed` lists for "N files changed" (nested
//! repositories and gitlinks left out; ignored files are in no checkpoint),
//! except that a rename is one row naming both paths.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use harness_protocol::{DiffContent, DiffFile, DiffHunk, DiffStatus, MAX_DIFF_JSON, TurnDiff};

use super::{GIT_TIMEOUT, Names, Repo, command, exec, nested_in, rel_path, result_text, within};

/// Files listed by name; the rest are counted (`TurnDiff::more_files`).
pub const MAX_FILES: usize = 1000;
/// A side larger than this is named with its size, not compared.
pub const FILE_BYTES: u64 = 512 * 1024;
/// The sizes of both sides of every file compared, together: what git
/// reads and writes for one answer.
pub const BLOB_BUDGET: u64 = 4 * 1024 * 1024;
/// Line text in one answer, and in one file.
pub const TEXT_BUDGET: usize = 1024 * 1024;
pub const FILE_TEXT: usize = 256 * 1024;
/// One line longer than this many bytes is cut (minified files).
pub const LINE_BYTES: usize = 2000;
/// Paths on the third call's command line, together.
const PATHSPEC_BYTES: usize = 128 * 1024;
/// The whole diff, all three calls.
pub const DIFF_TIMEOUT: Duration = Duration::from_secs(20);

/// What a cut line ends with.
pub const LINE_CUT: &str = " ⟨line cut⟩";

/// One changed path of the raw diff.
#[derive(Debug, Clone)]
struct Row {
    status: char,
    old_mode: String,
    new_mode: String,
    old_id: String,
    new_id: String,
    path: Vec<u8>,
    /// The source of a rename.
    old_path: Option<Vec<u8>>,
}

fn is_zero(id: &str) -> bool {
    id.bytes().all(|b| b == b'0')
}

/// `diff --raw -z -M` output: `:om nm oid nid S\0path\0`, and for a rename
/// or copy `:om nm oid nid R086\0from\0to\0`.
fn parse_raw(out: &[u8]) -> Vec<Row> {
    let mut parts = out.split(|b| *b == 0);
    let mut rows = Vec::new();
    while let Some(meta) = parts.next() {
        let Some(meta) = meta.strip_prefix(b":") else {
            continue;
        };
        let meta = String::from_utf8_lossy(meta);
        let f: Vec<&str> = meta.split(' ').collect();
        let (Some(om), Some(nm), Some(oid), Some(nid), Some(st)) =
            (f.first(), f.get(1), f.get(2), f.get(3), f.get(4))
        else {
            break;
        };
        let Some(status) = st.chars().next() else {
            break;
        };
        let (path, old_path) = if matches!(status, 'R' | 'C') {
            let (Some(from), Some(to)) = (parts.next(), parts.next()) else {
                break;
            };
            (to.to_vec(), Some(from.to_vec()))
        } else {
            let Some(p) = parts.next() else { break };
            (p.to_vec(), None)
        };
        rows.push(Row {
            status,
            old_mode: om.to_string(),
            new_mode: nm.to_string(),
            old_id: oid.to_string(),
            new_id: nid.to_string(),
            path,
            old_path,
        });
    }
    rows
}

/// A git call with byte arguments (paths may be any bytes) and optional
/// input; stdout as git wrote it.
async fn git_bytes(
    repo: &Repo,
    args: &[std::ffi::OsString],
    input: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let mut c = command(repo, None);
    c.args(args);
    let out = exec(c, input, GIT_TIMEOUT).await?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let shown: Vec<&str> = args.iter().filter_map(|a| a.to_str()).collect();
        Err(result_text(&shown, out))
    }
}

fn os(args: &[&str]) -> Vec<std::ffi::OsString> {
    args.iter().map(Into::into).collect()
}

/// The size of each blob in `ids` (the ones git has).
async fn sizes(repo: &Repo, ids: &HashSet<&str>) -> Result<HashMap<String, u64>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut input = Vec::new();
    for id in ids {
        input.extend_from_slice(id.as_bytes());
        input.push(b'\n');
    }
    let out = git_bytes(
        repo,
        &os(&[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ]),
        Some(input),
    )
    .await?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| {
            let mut f = l.split(' ');
            let (id, kind, size) = (f.next()?, f.next()?, f.next()?);
            (kind == "blob").then(|| Some((id.to_string(), size.parse().ok()?)))?
        })
        .collect())
}

/// One file's part of the patch.
#[derive(Debug, Default, Clone)]
struct Patch {
    binary: bool,
    hunks: Vec<DiffHunk>,
    added: u32,
    removed: u32,
    cut: bool,
    /// Nothing of it fitted the answer's budget.
    omitted: bool,
}

/// `@@ -a[,b] +c[,d] @@ section`.
fn hunk_head(line: &str) -> Option<DiffHunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (old_start, old_lines) = range(old)?;
    let (new_start, new_lines) = range(new)?;
    Some(DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        section: section.strip_prefix(' ').unwrap_or(section).to_string(),
        lines: Vec::new(),
    })
}

/// Cut `s` to at most `max` bytes on a character boundary.
fn cut_at(s: &mut String, max: usize) {
    let mut at = max.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    s.truncate(at);
}

/// The patch's sections by their `(old id, new id)`, in order, each within
/// the per-file and total line budgets (`budget` is what is left).
///
/// Three different stops (review D1): a line over [`LINE_BYTES`] is cut and
/// marked, and parsing goes on; a file past [`FILE_TEXT`] keeps its first
/// lines and is marked cut; once the answer's budget is spent, no later file
/// gets a line. Every line and section is scrubbed whole before it is cut
/// (review D2), so a secret across the cut never leaves half of itself.
fn parse_patch(
    out: &[u8],
    scrub: &dyn Fn(&str) -> String,
    budget: &mut usize,
) -> HashMap<(String, String), Patch> {
    let mut map = HashMap::new();
    let mut key: Option<(String, String)> = None;
    let mut cur = Patch::default();
    let mut in_hunk = false;
    let mut file_bytes = 0usize;
    // This file's line budget is spent: its later lines are skipped.
    let mut file_full = false;
    // Lines the current hunk still has, old and new side, by its header:
    // where it ends is counted, never guessed from what a line looks like.
    let mut left = (0u32, 0u32);
    let finish = |key: &mut Option<(String, String)>,
                  cur: &mut Patch,
                  map: &mut HashMap<(String, String), Patch>| {
        if let Some(k) = key.take() {
            map.entry(k).or_insert(std::mem::take(cur));
        }
        *cur = Patch::default();
    };
    for raw in out.split(|b| *b == b'\n') {
        if raw.starts_with(b"diff --git ") {
            finish(&mut key, &mut cur, &mut map);
            in_hunk = false;
            file_bytes = 0;
            file_full = false;
            continue;
        }
        if !in_hunk {
            if let Some(ids) = raw.strip_prefix(b"index ") {
                let ids = String::from_utf8_lossy(ids);
                let ids = ids.split(' ').next().unwrap_or_default();
                if let Some((a, b)) = ids.split_once("..") {
                    key = Some((a.to_string(), b.to_string()));
                }
            } else if raw.starts_with(b"Binary files ") {
                cur.binary = true;
            } else if raw.starts_with(b"@@ -") {
                in_hunk = true;
            } else {
                continue;
            }
            if !in_hunk {
                continue;
            }
        }
        // In a hunk: a header, or a line with its mark (an empty line is
        // an empty context line, if git was told to suppress the blank).
        if raw.starts_with(b"@@ -") {
            let head = hunk_head(&String::from_utf8_lossy(raw));
            left = head.as_ref().map_or((0, 0), |h| (h.old_lines, h.new_lines));
            if file_full || cur.omitted {
                continue;
            }
            if *budget == 0 {
                // Nothing of this file fits; if earlier hunks of it did,
                // it is cut, else it is left out whole.
                if cur.hunks.is_empty() {
                    cur.omitted = true;
                } else {
                    cur.cut = true;
                    file_full = true;
                }
                continue;
            }
            if let Some(h) = head {
                let mut section = scrub(&h.section);
                cut_at(&mut section, LINE_BYTES);
                cur.hunks.push(DiffHunk { section, ..h });
            }
            continue;
        }
        let mark = raw.first().copied().unwrap_or(b' ');
        // A "no newline" note belongs to the line before it; anything else
        // past the hunk's counted lines is not part of it.
        let counted = match mark {
            b'\\' => true,
            b'-' if left.0 > 0 => {
                left.0 -= 1;
                true
            }
            b'+' if left.1 > 0 => {
                left.1 -= 1;
                true
            }
            b' ' if left.0 > 0 && left.1 > 0 => {
                left = (left.0 - 1, left.1 - 1);
                true
            }
            _ => false,
        };
        if !counted || file_full || cur.omitted {
            continue;
        }
        let Some(h) = cur.hunks.last_mut() else {
            continue;
        };
        // Scrubbed whole, then cut (review D2).
        let mut text = scrub(&String::from_utf8_lossy(raw));
        if text.is_empty() {
            text.push(' ');
        }
        if text.len() > LINE_BYTES {
            cut_at(&mut text, LINE_BYTES);
            text.push_str(LINE_CUT);
            cur.cut = true;
        }
        let n = text.len() + 1;
        if n > *budget {
            // The answer is full: no later file gets a line either.
            *budget = 0;
            cur.cut = true;
            file_full = true;
            continue;
        }
        if file_bytes + n > FILE_TEXT {
            cur.cut = true;
            file_full = true;
            continue;
        }
        file_bytes += n;
        *budget -= n;
        match mark {
            b'+' => cur.added += 1,
            b'-' => cur.removed += 1,
            _ => {}
        }
        h.lines.push(text);
    }
    finish(&mut key, &mut cur, &mut map);
    map
}

fn status_of(r: &Row) -> DiffStatus {
    match r.status {
        'A' => DiffStatus::Added,
        'D' => DiffStatus::Deleted,
        'R' | 'C' => DiffStatus::Renamed,
        'T' => DiffStatus::TypeChanged,
        _ => DiffStatus::Modified,
    }
}

/// Why the content of a checkpoint cannot be read, in words for the person.
pub fn gone(repo: &Repo) -> String {
    if super::borrows(repo) {
        "the changes cannot be shown: this checkpoint depends on content that the git \
         repository in this folder no longer has (its history was rewritten and cleaned up)"
            .to_string()
    } else {
        "the changes cannot be shown: content this checkpoint needs is missing from the \
         checkpoint store"
            .to_string()
    }
}

/// The changes from snapshot `a` to snapshot `b` (both kept: the caller
/// checked), within [`DIFF_TIMEOUT`]. `scrub` takes the node's secrets out
/// of every path and line (the rule events follow).
pub async fn turn_diff(
    repo: &Repo,
    a: &str,
    b: &str,
    scrub: &(dyn Fn(&str) -> String + Sync),
) -> Result<TurnDiff, String> {
    tokio::time::timeout(DIFF_TIMEOUT, turn_diff_inner(repo, a, b, scrub))
        .await
        .map_err(|_| {
            format!(
                "the changes took longer than {} s to read (a very large turn?)",
                DIFF_TIMEOUT.as_secs()
            )
        })?
}

async fn turn_diff_inner(
    repo: &Repo,
    a: &str,
    b: &str,
    scrub: &(dyn Fn(&str) -> String + Sync),
) -> Result<TurnDiff, String> {
    let raw = match git_bytes(
        repo,
        &os(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--raw",
            "-M",
            "--no-abbrev",
            "-z",
            a,
            b,
        ]),
        None,
    )
    .await
    {
        Ok(r) => r,
        Err(e) if super::trees_missing(repo, &[a, b]).await => {
            tracing::debug!(error = %e, "checkpoint tree missing");
            return Err(gone(repo));
        }
        Err(e) => return Err(e),
    };
    // As `changed` lists them: nothing in or at a repository either side
    // left out, and no gitlink.
    let mut nested = nested_in(repo, a).await;
    nested.extend(nested_in(repo, b).await);
    nested.sort();
    nested.dedup();
    let set: HashSet<&[u8]> = nested.iter().map(Vec::as_slice).collect();
    let rows: Vec<Row> = parse_raw(&raw)
        .into_iter()
        .filter(|r| r.old_mode != "160000" && r.new_mode != "160000")
        .filter(|r| {
            !within(&r.path, &set) && r.old_path.as_deref().is_none_or(|p| !within(p, &set))
        })
        .collect();
    drop(raw);
    let more_files = rows.len().saturating_sub(MAX_FILES) as u64;
    let rows = &rows[..rows.len().min(MAX_FILES)];

    let ids: HashSet<&str> = rows
        .iter()
        .flat_map(|r| [r.old_id.as_str(), r.new_id.as_str()])
        .filter(|id| !is_zero(id))
        .collect();
    let size = sizes(repo, &ids).await?;
    let size_of = |id: &str| (!is_zero(id)).then(|| size.get(id).copied()).flatten();

    // Which files are compared line by line: both sides small enough, in
    // order, while the budgets last.
    let mut blob_left = BLOB_BUDGET;
    let mut spec_left = PATHSPEC_BYTES;
    let mut contents: Vec<DiffContent> = Vec::with_capacity(rows.len());
    let mut spec: Vec<std::ffi::OsString> = Vec::new();
    let mut cut = false;
    for r in rows {
        let (os_, ns) = (
            size_of(&r.old_id).unwrap_or(0),
            size_of(&r.new_id).unwrap_or(0),
        );
        let same = r.old_id == r.new_id;
        let content = if same {
            DiffContent::Text
        } else if os_ > FILE_BYTES || ns > FILE_BYTES {
            DiffContent::TooLarge
        } else {
            let paths: Vec<&[u8]> = std::iter::once(r.path.as_slice())
                .chain(r.old_path.as_deref())
                .collect();
            let need: usize = paths.iter().map(|p| p.len() + 1).sum();
            if os_ + ns > blob_left || need > spec_left {
                cut = true;
                // Nothing after this one either: the files keep their order.
                blob_left = 0;
                DiffContent::Omitted
            } else {
                blob_left -= os_ + ns;
                spec_left -= need;
                spec.extend(paths.iter().map(|p| rel_path(p).into_os_string()));
                DiffContent::Text
            }
        };
        contents.push(content);
    }

    let mut patches = HashMap::new();
    if !spec.is_empty() {
        let mut args = os(&[
            // Every path below is a name: no glob, no `:(magic)`.
            "--literal-pathspecs",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "-M",
            "--full-index",
            "-U3",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            a,
            b,
            "--",
        ]);
        args.extend(spec);
        let out = git_bytes(repo, &args, None).await?;
        let mut budget = TEXT_BUDGET;
        patches = parse_patch(&out, scrub, &mut budget);
    }

    let name = |p: &[u8]| scrub(&String::from_utf8_lossy(p));
    let mode = |m: &str| (m != "000000").then(|| m.to_string());
    let mut files: Vec<DiffFile> = rows
        .iter()
        .zip(contents)
        .map(|(r, content)| {
            let mut f = DiffFile {
                path: name(&r.path),
                old_path: r.old_path.as_deref().map(name),
                status: status_of(r),
                content,
                old_mode: mode(&r.old_mode),
                new_mode: mode(&r.new_mode),
                old_size: size_of(&r.old_id),
                new_size: size_of(&r.new_id),
                added: 0,
                removed: 0,
                hunks: Vec::new(),
                cut: false,
            };
            if content == DiffContent::Text && r.old_id != r.new_id {
                // A twin (the same change in two files) has the same lines.
                // A type change (a file became a link, or the reverse) is
                // two sections in git's patch, the old side removed and the
                // new one added: joined here (review D4).
                let found = if r.status == 'T' {
                    let zero = "0".repeat(r.old_id.len());
                    let gone = patches.get(&(r.old_id.clone(), zero.clone()));
                    let made = patches.get(&(zero, r.new_id.clone()));
                    match (gone, made) {
                        (None, None) => None,
                        (g, m) => {
                            let mut p = Patch::default();
                            for part in [g, m].into_iter().flatten() {
                                p.binary |= part.binary;
                                p.omitted |= part.omitted && part.hunks.is_empty();
                                p.cut |= part.cut;
                                p.hunks.extend(part.hunks.iter().cloned());
                                p.added += part.added;
                                p.removed += part.removed;
                            }
                            p.omitted &= p.hunks.is_empty();
                            Some(p)
                        }
                    }
                } else {
                    patches.get(&(r.old_id.clone(), r.new_id.clone())).cloned()
                };
                match found {
                    Some(p) if p.binary => f.content = DiffContent::Binary,
                    Some(p) if p.omitted => {
                        f.content = DiffContent::Omitted;
                        f.cut = true;
                    }
                    Some(p) => {
                        f.hunks = p.hunks;
                        f.added = p.added;
                        f.removed = p.removed;
                        f.cut = p.cut;
                    }
                    // Paired differently by the limited diff (a rename).
                    None => {
                        f.content = DiffContent::Omitted;
                        f.cut = true;
                    }
                }
            }
            if f.cut {
                cut = true;
            }
            f
        })
        .collect();
    let mut more_files = more_files;
    fit(&mut files, &mut more_files, &mut cut);
    Ok(TurnDiff {
        files,
        more_files,
        cut: cut || more_files > 0,
        nested: Names::of(&nested).shown.iter().map(|n| scrub(n)).collect(),
        others_wrote: vec![],
        others_may: vec![],
        since_pruned: false,
    })
}

/// Keep the serialized answer within [`MAX_DIFF_JSON`] (escaping can make
/// it six times its text): the last files' lines go first, then the last
/// files' names (counted in `more_files`).
fn fit(files: &mut Vec<DiffFile>, more_files: &mut u64, cut: &mut bool) {
    // Room for the envelope and the nested names.
    let limit = MAX_DIFF_JSON - 64 * 1024;
    let size = |f: &DiffFile| serde_json::to_vec(f).map(|v| v.len() + 1).unwrap_or(0);
    let mut sizes: Vec<usize> = files.iter().map(size).collect();
    let mut total: usize = sizes.iter().sum();
    let mut i = files.len();
    while total > limit && i > 0 {
        i -= 1;
        if files[i].hunks.is_empty() {
            continue;
        }
        files[i].hunks.clear();
        files[i].added = 0;
        files[i].removed = 0;
        files[i].content = DiffContent::Omitted;
        files[i].cut = true;
        *cut = true;
        let s = size(&files[i]);
        total = total - sizes[i] + s;
        sizes[i] = s;
    }
    while total > limit {
        let Some(s) = sizes.pop() else { break };
        files.pop();
        total -= s;
        *more_files += 1;
        *cut = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_rows_with_renames_and_odd_names() {
        let z = "0".repeat(40);
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let mut out = Vec::new();
        out.extend_from_slice(format!(":100644 100644 {a} {b} M").as_bytes());
        out.extend_from_slice(b"\0x\nname\0");
        out.extend_from_slice(format!(":100644 100644 {a} {b} R086").as_bytes());
        out.extend_from_slice(b"\0old\0--new\0");
        out.extend_from_slice(format!(":000000 100644 {z} {b} A").as_bytes());
        out.extend_from_slice(b"\0\xff.bin\0");
        let rows = parse_raw(&out);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].path, b"x\nname");
        assert_eq!(rows[1].path, b"--new");
        assert_eq!(rows[1].old_path.as_deref(), Some(&b"old"[..]));
        assert_eq!(rows[1].status, 'R');
        assert_eq!(rows[2].path, b"\xff.bin");
        assert!(is_zero(&rows[2].old_id));
    }

    #[test]
    fn hunk_heads() {
        let h = hunk_head("@@ -1,3 +1,4 @@ fn main() {").unwrap();
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (1, 3, 1, 4)
        );
        assert_eq!(h.section, "fn main() {");
        let h = hunk_head("@@ -0,0 +1 @@").unwrap();
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (0, 0, 1, 1)
        );
        assert!(hunk_head("@@ nonsense").is_none());
    }

    #[test]
    fn a_removed_line_that_looks_like_a_header_stays_a_line() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let patch = format!(
            "diff --git a/f b/f\nindex {a}..{b} 100644\n--- a/f\n+++ b/f\n@@ -1,2 +1,1 @@\n--- not a header\n-diff --git x\n+@@ -9 +9 @@\n"
        );
        let mut budget = TEXT_BUDGET;
        let map = parse_patch(patch.as_bytes(), &|s| s.to_string(), &mut budget);
        let p = &map[&(a, b)];
        // `-diff --git x` starts a section only at a line's start with no
        // mark; it has a mark.
        assert_eq!(p.hunks.len(), 1);
        assert_eq!(
            p.hunks[0].lines,
            ["--- not a header", "-diff --git x", "+@@ -9 +9 @@"]
        );
        assert_eq!((p.added, p.removed), (1, 2));
    }
}

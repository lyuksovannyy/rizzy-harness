//! The line form of `edit` (PLAN.md T45): the model names the tag of a file
//! version it was shown and row numbers, and gives only the new text. The
//! idea is oh-my-pi's (MIT); this is our own, and takes only the core of it:
//! a tag per file version, row numbers, a record of which rows were shown,
//! and a refusal that prints the rows.
//!
//! * `lines` is a list of `{from, to, text}` (replace the inclusive rows
//!   `from..to`; empty `text` deletes them) and `{after, text}` (insert after
//!   row `after`; 0 is the head of the file). Every number refers to the
//!   tagged version, never to an earlier operation of the same call; the
//!   operations may come in any order, must not overlap and are all made or
//!   none.
//! * `text` is whole lines: a final newline in it is not needed and not
//!   doubled (`"a\nb"` and `"a\nb\n"` are two lines), `""` is no line at all
//!   (a deletion), `"\n"` is one empty line. Its line breaks become the
//!   file's: the line ending of the row replaced, or of the row before an
//!   insertion (the file's first line ending, else LF). A file that ends
//!   without a final newline still does after the edit; an empty file gets
//!   one with its first inserted lines. A leading BOM is not a row's text and
//!   stays where it is.
//! * Checks, in this order, under the path's lock: a version of that tag is
//!   kept (else the rows named, with context, are printed from the file as it
//!   is, and count as shown); every row named was shown whole (else those
//!   rows are printed from the tagged version and count as shown; a row
//!   longer than `read` shows is refused, the old form takes it); the file
//!   has not changed since the tagged version, or the rows named (and both
//!   neighbours of an insertion point) did not and each replaced range is
//!   still one run of rows, the same length, with nothing inserted into it
//!   (a range that swallowed a row someone inserted into it would delete
//!   work the model never saw), mapped with `edit`'s own changed-lines logic
//!   and accepted only where a diff from the other end of the file reads
//!   them the same way (a block pasted next to a copy of itself is refused,
//!   not guessed at); the edit changes something.

use std::path::Path;

use serde_json::Value;

use crate::edit::{changed, show};
use crate::fs::{header, is_cut, line_style, number_line, read_nofollow, write_nofollow};
use crate::seen::{Version, sha256, tag};
use crate::{ToolCtx, ToolOutcome};

/// The highest row number a call may name: a file the line form handles is
/// at most [`crate::seen::SNAPSHOT_MAX_FILE`] bytes, so this is far above any
/// real row, and low enough that no sum of two such numbers can overflow.
pub(crate) const MAX_ROW: usize = 1_000_000;
/// Operations one call may hold, at most.
pub(crate) const MAX_OPS: usize = 256;

/// Rows of context around the rows a refusal prints.
const CONTEXT: usize = 3;
/// Rows a refusal prints, at most.
const MAX_ROWS: usize = 40;
/// Rows of replaced text the result repeats for the person, at most.
const MAX_OLD_ROWS: usize = 20;

const OLD_FORM: &str = "use the `old_string` form (or `edits`) for it";

/// One operation, 1-based and inclusive as the model wrote it.
#[derive(Debug, Clone)]
enum Op {
    Replace {
        from: usize,
        to: usize,
        text: Vec<String>,
    },
    Insert {
        after: usize,
        text: Vec<String>,
    },
}

impl Op {
    /// Where it starts as a boundary between rows (0 is above row 1); an
    /// insertion at a boundary sorts before a replacement starting there.
    fn key(&self) -> (usize, u8) {
        match self {
            Op::Replace { from, .. } => (from - 1, 1),
            Op::Insert { after, .. } => (*after, 0),
        }
    }
}

/// A call in the line form.
#[derive(Debug)]
pub(crate) struct Call {
    /// `None`: the call named none; it is refused with the rows and the tag
    /// of the file as it is.
    tag: Option<String>,
    ops: Vec<Op>,
}

/// The call is in the line form, or tries to be (it names `tag` or `lines`).
pub(crate) fn wants(input: &Value) -> bool {
    ["tag", "lines"]
        .iter()
        .any(|k| input.get(k).is_some_and(|v| !v.is_null()))
}

/// The four characters of a tag as the model may send them: bare, or the
/// whole header line, any letter case.
pub(crate) fn norm_tag(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s.rsplit('#').next().unwrap_or(s);
    let s = s.trim().trim_end_matches(']').trim();
    (s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_uppercase())
}

/// What the call says, before any file is looked at.
pub(crate) fn parse(input: &Value) -> Result<Call, String> {
    let old_form = ["old_string", "new_string", "edits", "replace_all"]
        .iter()
        .any(|k| input.get(k).is_some_and(|v| !v.is_null()));
    if old_form {
        return Err("give either old_string/new_string (or `edits`) or tag/lines, not both".into());
    }
    let tag = match input.get("tag") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let Some(tag) = v.as_str().and_then(norm_tag) else {
                return Err(format!(
                    "`tag` must be the 4 hex characters after # in the header line of a `read`, \
                     `edit` or `write` result, not {v}"
                ));
            };
            Some(tag)
        }
    };
    let list = match input.get("lines") {
        None | Some(Value::Null) => {
            return Err(
                "the line form needs `lines`: a list of {from, to, text} or {after, text}".into(),
            );
        }
        Some(Value::String(s)) => serde_json::from_str::<Value>(s)
            .map_err(|e| format!("`lines` is a string that is not a JSON list: {e}"))?,
        Some(v) => v.clone(),
    };
    let Value::Array(items) = shape_lines(list) else {
        return Err("`lines` must be a list of {from, to, text} or {after, text}".into());
    };
    if items.is_empty() {
        return Err("`lines` is empty".into());
    }
    if items.len() > MAX_OPS {
        return Err(format!(
            "{} operations in one call: at most {MAX_OPS} are taken; split them into several \
             calls, nothing was changed",
            items.len()
        ));
    }
    let num = |v: &Value, k: &str, label: &str| -> Result<Option<usize>, String> {
        match v.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(n) => match n.as_u64() {
                Some(n) if n > MAX_ROW as u64 => Err(format!(
                    "{label}`{k}` is {n}, but a file has at most {MAX_ROW} rows here; use the \
                     row numbers of the tagged version"
                )),
                Some(n) => Ok(Some(n as usize)),
                None => Err(format!("{label}`{k}` must be a whole number")),
            },
        }
    };
    let mut ops = Vec::new();
    for (i, v) in items.iter().enumerate() {
        let label = format!("lines[{i}]: ");
        let text = v
            .get("text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| text_missing(i))?;
        let text = text_lines(text);
        let (from, to, after) = (
            num(v, "from", &label)?,
            num(v, "to", &label)?,
            num(v, "after", &label)?,
        );
        ops.push(match (from, to, after) {
            (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
                return Err(format!(
                    "{label}give either `from` and `to`, or `after`, not both"
                ));
            }
            (Some(from), Some(to), None) => {
                if from == 0 {
                    return Err(format!(
                        "{label}rows are numbered from 1 (`after` 0 inserts at the head)"
                    ));
                }
                if to < from {
                    return Err(format!(
                        "{label}`to` ({to}) is before `from` ({from}); the range is inclusive and ordered"
                    ));
                }
                Op::Replace { from, to, text }
            }
            (Some(_), None, None) => {
                return Err(format!(
                    "{label}`from` needs `to`, the last row; to replace one row send the same \
                     number as both"
                ));
            }
            (None, Some(_), None) => {
                return Err(format!("{label}`to` needs `from`, the first row"));
            }
            (None, None, Some(after)) => Op::Insert { after, text },
            (None, None, None) => {
                return Err(format!("{label}give `from` and `to`, or `after`"));
            }
        });
    }
    ops.sort_by_key(Op::key);
    for w in ops.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let clash = match (a, b) {
            (Op::Insert { after: x, .. }, Op::Insert { after: y, .. }) if x == y => Some(format!(
                "two insertions after row {x}: the same place; make them one"
            )),
            // Replacement then anything starting inside it.
            (Op::Replace { from, to, .. }, n) => match n {
                Op::Replace { from: f2, .. } if *f2 <= *to => {
                    Some(format!("rows {from}-{to} and rows from {f2} overlap"))
                }
                Op::Insert { after, .. } if *after < *to => Some(format!(
                    "the insertion after row {after} lies inside the replaced rows {from}-{to}"
                )),
                _ => None,
            },
            _ => None,
        };
        if let Some(c) = clash {
            return Err(format!("operations overlap: {c}; nothing was changed"));
        }
    }
    Ok(Call { tag, ops })
}

/// The refusal for an operation without `text`.
fn text_missing(i: usize) -> String {
    format!(
        "lines[{i}]: `text` is missing or not a string; to delete the rows send \
         \"text\": \"\" (an empty string)"
    )
}

/// `lines` as the model sent it, in the shapes real models send: one object
/// is a list of one, and a `text` that is a list of strings is a list of
/// lines, one per string: `[""]` is one empty line and `["a", ""]` is two
/// (a line ending at the end of a string is taken off first). The joined
/// text ends with a newline, so the string rule that drops one final
/// newline gives back exactly those lines. An empty list is left as it is
/// and refused in [`normalize`]: it could mean "no lines" or a mistake, and
/// `"text": ""` says a deletion without doubt. Anything else is left for
/// the checks to refuse.
fn shape_lines(lines: Value) -> Value {
    let mut lines = match lines {
        o @ Value::Object(_) => Value::Array(vec![o]),
        v => v,
    };
    if let Value::Array(items) = &mut lines {
        for item in items {
            if let Some(t) = item.get_mut("text")
                && let Value::Array(parts) = t
                && !parts.is_empty()
                && parts.iter().all(Value::is_string)
            {
                let mut joined = String::new();
                for part in parts.iter().filter_map(Value::as_str) {
                    let line = part.strip_suffix('\n').unwrap_or(part);
                    joined.push_str(line.strip_suffix('\r').unwrap_or(line));
                    joined.push('\n');
                }
                *t = Value::String(joined);
            }
        }
    }
    lines
}

/// Before the schema check (`Tool::normalize_args`): the shapes of
/// [`shape_lines`] made the schema's, and a missing `text` refused in words
/// that say how to delete rows.
pub(crate) fn normalize(input: &mut Value) -> Result<(), String> {
    let Some(lines) = input.get_mut("lines") else {
        return Ok(());
    };
    if lines.is_null() || lines.is_string() {
        return Ok(());
    }
    *lines = shape_lines(lines.take());
    if let Value::Array(items) = lines {
        for (i, item) in items.iter().enumerate() {
            if item.is_object() && item.get("text").is_none_or(Value::is_null) {
                return Err(text_missing(i));
            }
            if item
                .get("text")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
            {
                return Err(format!(
                    "lines[{i}]: `text` is an empty list; to delete the rows send \
                     \"text\": \"\" (an empty string), and for one empty line \"text\": \"\\n\""
                ));
            }
        }
    }
    Ok(())
}

/// `text` as whole lines.
fn text_lines(text: &str) -> Vec<String> {
    let t = text.replace("\r\n", "\n");
    if t.is_empty() {
        return Vec::new();
    }
    t.strip_suffix('\n')
        .unwrap_or(&t)
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// A file as rows: a leading BOM's length, and each row with its line ending.
fn split_rows(text: &str) -> (usize, Vec<&str>) {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    (bom, text[bom..].split_inclusive('\n').collect())
}

/// A row's text, without its line ending.
fn content(row: &str) -> &str {
    let r = row.strip_suffix('\n').unwrap_or(row);
    r.strip_suffix('\r').unwrap_or(r)
}

fn term(row: &str) -> Option<&'static str> {
    if row.ends_with("\r\n") {
        Some("\r\n")
    } else if row.ends_with('\n') {
        Some("\n")
    } else {
        None
    }
}

/// The rows a refusal prints and where they come from.
enum No {
    Plain(String),
    Rows {
        lead: String,
        /// Print from this older version (and mark it shown), else from the
        /// file as it is.
        version: Option<Version>,
        /// Inclusive 1-based row ranges, widened by `context` rows.
        ranges: Vec<(usize, usize)>,
        context: usize,
    },
}

impl From<String> for No {
    fn from(s: String) -> Self {
        No::Plain(s)
    }
}

/// Where the rows of the tagged version are in the file as it is: the same
/// above `prefix`, the same from `suffix_from` on (moved by the difference of
/// the row counts), changed between.
struct Map {
    prefix: usize,
    suffix_from: usize,
    n_t: usize,
    n_l: usize,
}

impl Map {
    /// The file is the version.
    fn same(n: usize) -> Self {
        Map {
            prefix: n,
            suffix_from: n,
            n_t: n,
            n_l: n,
        }
    }

    /// A 0-based row of the version, as a 0-based row of the file.
    fn row(&self, r: usize) -> Option<usize> {
        if r < self.prefix {
            Some(r)
        } else if r >= self.suffix_from {
            (r + self.n_l).checked_sub(self.n_t)
        } else {
            None
        }
    }

    /// The place between rows `n - 1` and `n` of the version (0 is above the
    /// first row), as such a place in the file: only if both rows, or the
    /// end of the file, are still next to each other.
    fn boundary(&self, n: usize) -> Option<usize> {
        let lo = if n >= 1 { self.row(n - 1)? + 1 } else { 0 };
        let hi = if n < self.n_t { self.row(n)? } else { self.n_l };
        (lo == hi).then_some(lo)
    }
}

/// The map `edit`'s own changed-lines logic gives ([`changed`]: the common
/// head and tail of the two texts, widened to whole lines), accepted only
/// where the same rows read the same way from the other end of the file
/// (the longest common tail of rows first). A change next to a copy of
/// what it added or removed can be read either way, and then the rows next
/// to it are not told apart: refused, never guessed. `None` too when a row
/// of `runs` (0-based inclusive, replaced) or a place of `boundaries` lies in
/// the changed part, or a run is not one run in the file as it is any more
/// (rows were inserted between its ends: replacing it would delete them).
fn map_for(old: &str, live: &str, runs: &[(usize, usize)], boundaries: &[usize]) -> Option<Mapped> {
    let (a, b) = changed(old, live);
    let (_, t) = split_rows(old);
    let (_, l) = split_rows(live);
    let (n_t, n_l) = (t.len(), l.len());
    let nl = |s: &str| s.bytes().filter(|c| *c == b'\n').count();
    let la = nl(&live[..a]);
    let head = &live[..b];
    let lb = nl(head) + usize::from(!head.is_empty() && !head.ends_with('\n'));
    let tb = (n_t + lb).checked_sub(n_l)?;
    if tb < la {
        return None;
    }
    let main = Map {
        prefix: la,
        suffix_from: tb,
        n_t,
        n_l,
    };
    let tail = t
        .iter()
        .rev()
        .zip(l.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let head_rows = t
        .iter()
        .zip(&l)
        .take(n_t.min(n_l) - tail)
        .take_while(|(x, y)| x == y)
        .count();
    let alt = Map {
        prefix: head_rows,
        suffix_from: n_t - tail,
        n_t,
        n_l,
    };
    for &(from, to) in runs {
        // Both ends map and nothing lies between them that was not there.
        if main.row(to)?.checked_sub(main.row(from)?) != Some(to - from) {
            return None;
        }
        for r in from..=to {
            let m = main.row(r)?;
            if alt.row(r) != Some(m) {
                return None;
            }
        }
    }
    for &n in boundaries {
        let x = main.boundary(n)?;
        if alt.boundary(n) != Some(x) {
            return None;
        }
    }
    Some(Mapped {
        main,
        region: (la + 1, lb),
    })
}

struct Mapped {
    main: Map,
    /// The changed rows of the file as it is, 1-based inclusive (empty when
    /// the second is below the first).
    region: (usize, usize),
}

/// Rows named by `ops` in the tagged version's numbering, 0-based.
fn named(ops: &[Op]) -> Vec<usize> {
    let mut v = Vec::new();
    for op in ops {
        match op {
            Op::Replace { from, to, .. } => v.extend(from - 1..*to),
            Op::Insert { after, .. } if *after >= 1 => v.push(after - 1),
            Op::Insert { .. } => {}
        }
    }
    v
}

/// The rows an unknown tag names, in the file as it is (1-based ranges).
fn named_ranges(ops: &[Op]) -> Vec<(usize, usize)> {
    ops.iter()
        .map(|op| match op {
            Op::Replace { from, to, .. } => (*from, *to),
            Op::Insert { after, .. } => ((*after).max(1), after.saturating_add(1)),
        })
        .collect()
}

fn merge(mut r: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    r.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in r {
        match out.last_mut() {
            Some(l) if a <= l.1 + 1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// What a successful plan holds.
struct Plan {
    updated: String,
    /// Where each operation's new text lies in `updated` (byte ranges).
    landed: Vec<(usize, usize)>,
    /// The rows replaced, 1-based in the file as it was just before.
    replaced: Vec<(usize, usize)>,
    /// Each operation as it lies in the file as it is, in the call's
    /// (sorted) order.
    spots: Vec<Spot>,
    /// The file was not the tagged version: the changed rows, as it is now.
    changed: Option<(usize, usize)>,
}

/// Where an operation lies in the file as it is.
#[derive(Debug, Clone, Copy)]
enum Spot {
    /// Rows `from..=to`, 1-based.
    Rows(usize, usize),
    /// Below row `n` (0 is the top).
    After(usize),
}

/// Check `call` against what the agent was shown and make the new text.
/// Nothing is written and nothing recorded here (a refusal that prints rows
/// records them, in [`refuse`]).
fn plan(ctx: &ToolCtx, key: &Path, shown: &str, live: &str, call: &Call) -> Result<Plan, No> {
    if live.len() > crate::seen::SNAPSHOT_MAX_FILE {
        return Err(format!(
            "{shown} is too large for the line form (more than {} bytes); {OLD_FORM}; nothing was changed.",
            crate::seen::SNAPSHOT_MAX_FILE
        )
        .into());
    }
    if live
        .as_bytes()
        .iter()
        .take(crate::fs::BINARY_SNIFF)
        .any(|b| *b == 0)
    {
        return Err(format!("{shown} looks like a binary file; nothing was changed.").into());
    }
    let (max_chars, _) = line_style(ctx);
    let live_sha = sha256(live.as_bytes());
    let Some(tag_given) = call.tag.as_deref() else {
        let n = split_rows(live).1.len();
        return Err(No::Rows {
            lead: format!(
                "The line form needs `tag`: the 4 characters after # in the `[path#TAG]` header \
                 line of the latest `read`, `edit` or `write` result for {shown}, so nothing was \
                 changed. The file has {n} rows."
            ),
            version: None,
            ranges: named_ranges(&call.ops),
            context: CONTEXT,
        });
    };
    let Some(ver) = ctx.seen.version(key, tag_given) else {
        let n = split_rows(live).1.len();
        return Err(No::Rows {
            lead: format!(
                "No version of {shown} tagged {tag_given} is known in this session (it was not \
                 read here, or the node restarted, or it was dropped from memory), so nothing \
                 was changed. The file has {n} rows."
            ),
            version: None,
            ranges: named_ranges(&call.ops),
            context: CONTEXT,
        });
    };
    let (_, t_rows) = split_rows(&ver.text);
    let n_t = t_rows.len();
    // Bounds, in the tagged version's numbering.
    for op in &call.ops {
        let top = match op {
            Op::Replace { to, .. } => *to,
            Op::Insert { after, .. } => *after,
        };
        if top > n_t {
            return Err(format!(
                "Row {top} does not exist: the version of {shown} tagged {tag_given} has {n_t} \
                 rows; nothing was changed."
            )
            .into());
        }
    }
    // Shown, and shown whole.
    let rows = named(&call.ops);
    if let Some(&r) = rows
        .iter()
        .find(|r| is_cut(content(t_rows[**r]), max_chars))
    {
        return Err(format!(
            "Row {} of {shown} is longer than `read` can show ({max_chars} characters), so it \
             cannot be named in the line form; {OLD_FORM}. Nothing was changed.",
            r + 1
        )
        .into());
    }
    let unshown: Vec<(usize, usize)> = merge(
        rows.iter()
            .filter(|r| !ver.covers(**r + 1, **r + 1))
            .map(|r| (r + 1, r + 1))
            .collect(),
    );
    if !unshown.is_empty() {
        let list: Vec<String> = unshown
            .iter()
            .map(|(a, b)| {
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            })
            .collect();
        return Err(No::Rows {
            lead: format!(
                "Rows {} of the version of {shown} tagged {tag_given} were not shown to you (a \
                 `read` window, an edit's result or a refusal shows rows), so nothing was \
                 changed.",
                list.join(", ")
            ),
            version: Some(ver),
            ranges: unshown,
            context: CONTEXT,
        });
    }
    // The tagged version against the file as it is.
    let (bom, l_rows) = split_rows(live);
    let n_l = l_rows.len();
    let mut changed_rows = None;
    let map = if live_sha == ver.sha {
        Map::same(n_t)
    } else {
        let mut bounds = Vec::new();
        let mut runs = Vec::new();
        for op in &call.ops {
            match op {
                Op::Insert { after, .. } => bounds.push(*after),
                Op::Replace { from, to, .. } => runs.push((from - 1, to - 1)),
            }
        }
        let Some(m) = map_for(&ver.text, live, &runs, &bounds) else {
            let (a, b) = changed(&ver.text, live);
            let (lo, hi) = (
                crate::edit::line_of(live, a),
                crate::edit::line_of(live, b.saturating_sub(1).max(a)),
            );
            return Err(No::Rows {
                lead: format!(
                    "{shown} is not the version tagged {tag_given} any more (the user, a command, \
                     another session or you changed it) and the rows you name are not where they \
                     were, have rows inserted between them, or cannot be told from other rows, so \
                     nothing was changed. The rows that changed now read:"
                ),
                version: None,
                ranges: vec![(lo, hi.max(lo))],
                context: 0,
            });
        };
        changed_rows = Some(m.region);
        m.main
    };
    // The new rows. Each operation, mapped onto the rows of the file as it is.
    enum Out<'a> {
        Keep(&'a str),
        New(String),
    }
    let mut out: Vec<(Out, Option<usize>)> = Vec::new();
    let default_eol = l_rows.iter().find_map(|r| term(r)).unwrap_or("\n");
    let eol_at = |i: usize| -> &'static str {
        l_rows[..=i.min(n_l.saturating_sub(1))]
            .iter()
            .rev()
            .find_map(|r| term(r))
            .unwrap_or(default_eol)
    };
    let mut pos = 0;
    let mut replaced = Vec::new();
    let mut spots = Vec::new();
    for (k, op) in call.ops.iter().enumerate() {
        let (start, end, text, eol) = match op {
            Op::Replace { from, to, text } => {
                let (Some(f), Some(t)) = (map.row(from - 1), map.row(to - 1)) else {
                    return Err("internal: a named row has no place in the file"
                        .to_string()
                        .into());
                };
                replaced.push((f + 1, t + 1));
                spots.push(Spot::Rows(f + 1, t + 1));
                let eol = term(l_rows[f]).unwrap_or_else(|| eol_at(f));
                (f, t + 1, text, eol)
            }
            Op::Insert { after, text } => {
                let Some(b) = map.boundary(*after) else {
                    return Err("internal: an insertion point has no place in the file"
                        .to_string()
                        .into());
                };
                spots.push(Spot::After(b));
                let eol = if b >= 1 {
                    eol_at(b - 1)
                } else {
                    l_rows.first().and_then(|r| term(r)).unwrap_or(default_eol)
                };
                (b, b, text, eol)
            }
        };
        for r in &l_rows[pos..start] {
            out.push((Out::Keep(r), None));
        }
        for line in text {
            out.push((Out::New(format!("{line}{eol}")), Some(k)));
        }
        if text.is_empty() {
            // A marker for where a deletion lies.
            out.push((Out::New(String::new()), Some(k)));
        }
        pos = end;
    }
    for r in &l_rows[pos..] {
        out.push((Out::Keep(r), None));
    }
    // Join: the file keeps having no final newline if it had none.
    let final_nl = l_rows.last().is_none_or(|r| r.ends_with('\n'));
    let mut updated = String::with_capacity(live.len());
    updated.push_str(&live[..bom]);
    let mut starts = Vec::with_capacity(out.len() + 1);
    let last_real = out
        .iter()
        .rposition(|(o, _)| !matches!(o, Out::New(s) if s.is_empty()));
    for (i, (o, _)) in out.iter().enumerate() {
        starts.push(updated.len());
        let s: &str = match o {
            Out::Keep(r) => r,
            Out::New(s) => s,
        };
        if s.is_empty() {
            continue;
        }
        if Some(i) == last_real && !final_nl {
            let s = s.strip_suffix('\n').unwrap_or(s);
            updated.push_str(s.strip_suffix('\r').unwrap_or(s));
        } else if !s.ends_with('\n') {
            // An unterminated last row with something after it now.
            updated.push_str(s);
            updated.push_str(default_eol);
        } else {
            updated.push_str(s);
        }
    }
    starts.push(updated.len());
    let mut landed: Vec<(usize, usize)> = Vec::new();
    for k in 0..call.ops.len() {
        let idx: Vec<usize> = out
            .iter()
            .enumerate()
            .filter(|(_, (_, o))| *o == Some(k))
            .map(|(i, _)| i)
            .collect();
        let (a, b) = (starts[idx[0]], starts[idx[idx.len() - 1] + 1]);
        landed.push((a.min(updated.len()), b.min(updated.len())));
    }
    if updated == live {
        return Err(format!(
            "That changes nothing: the rows already read as the text you gave, so nothing was written. \
             {}",
            header(ctx, Path::new(shown), &live_sha)
        )
        .into());
    }
    Ok(Plan {
        updated,
        landed,
        replaced,
        spots,
        changed: changed_rows.filter(|(a, b)| b >= a),
    })
}

/// A refusal's words, with the rows it prints (`print`) counted as shown.
fn refuse(ctx: &ToolCtx, key: &Path, shown: &str, live: &str, no: No, print: bool) -> String {
    let (lead, version, ranges, context) = match no {
        No::Plain(s) => return s,
        No::Rows {
            lead,
            version,
            ranges,
            context,
        } => (lead, version, ranges, context),
    };
    if !print {
        return format!("{lead} Read the rows with `read`, then send the call again.");
    }
    let text: &str = version.as_ref().map_or(live, |v| v.text.as_str());
    let (_, rows) = split_rows(text);
    let n = rows.len();
    let (max_chars, compact) = line_style(ctx);
    let wide: Vec<(usize, usize)> = merge(
        ranges
            .iter()
            .filter(|(a, b)| *a >= 1 && a <= b)
            .map(|(a, b)| {
                (
                    a.saturating_sub(context).max(1),
                    b.saturating_add(context).min(n),
                )
            })
            .filter(|(a, b)| a <= b)
            .collect(),
    );
    let mut out = format!("{lead}\n");
    let mut shown_rows: Vec<(usize, usize)> = Vec::new();
    let mut used = 0;
    let mut left = 0;
    for (k, (a, b)) in wide.iter().enumerate() {
        if k > 0 && used < MAX_ROWS {
            out.push_str("--\n");
        }
        for r in *a..=*b {
            if used >= MAX_ROWS {
                left += 1;
                continue;
            }
            let c = content(rows[r - 1]);
            out.push_str(&number_line(r, c, max_chars, compact));
            used += 1;
            if !is_cut(c, max_chars) {
                shown_rows.push((r, r));
            }
        }
    }
    if left > 0 {
        out.push_str(&format!("[{left} more rows named, not shown: read them]\n"));
    }
    match &version {
        Some(v) => {
            ctx.seen.add_shown(key, &v.sha, &shown_rows);
            out.push_str(&format!(
                "These rows now count as shown: send the same call again with this tag:\n{}",
                header(ctx, Path::new(shown), &v.sha)
            ));
        }
        None => {
            ctx.seen.add_version(key, live.as_bytes(), &shown_rows);
            let sha = sha256(live.as_bytes());
            out.push_str(&format!(
                "These rows now count as shown. Send the call again with the tag of the file as \
                 it is, numbering the rows as listed here:\n{}",
                header(ctx, Path::new(shown), &sha)
            ));
        }
    }
    out
}

/// Before any card (T51): what is wrong with the call whatever anyone
/// answers. Arguments first; then, like the old form's, what only the table
/// and the file can say (an unknown tag, rows not shown, rows that moved),
/// with rows printed only where a `read` would run unasked here.
pub(crate) fn refusal(ctx: &ToolCtx, input: &Value) -> Option<String> {
    let call = match parse(input) {
        Ok(c) => c,
        Err(e) => return Some(e),
    };
    let shown = input.get("path")?.as_str()?;
    let path = ctx.resolve_for_write(shown).ok()?;
    let md = std::fs::symlink_metadata(&path).ok()?;
    if !md.file_type().is_file() || md.len() > crate::seen::SNAPSHOT_MAX_FILE as u64 {
        return None;
    }
    let key = ctx.seen_key(&path);
    let live = read_small(&path)?;
    match plan(ctx, &key, shown, &live, &call) {
        Ok(_) => None,
        Err(no) => {
            let print = ctx.read_unasked && ctx.resolve(shown).is_ok();
            Some(refuse(ctx, &key, shown, &live, no, print))
        }
    }
}

/// Rows the card's preview shows, leads included, at most.
const PREVIEW_LINES: usize = 40;
/// Characters of the card's preview, at most (a row is cut as `read` cuts it,
/// and 40 rows of 2,000 characters would still be a wall).
const PREVIEW_CHARS: usize = 8_000;

/// A row of a preview: as `read` numbers it, with what a person's screen
/// must not obey (control characters, bidirectional overrides) shown as `?`.
fn preview_row(n: usize, text: &str, max_chars: usize, compact: bool) -> String {
    let clean: String = text
        .chars()
        .map(|c| {
            let bidi = matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
            if (c.is_control() && c != '\t') || bidi {
                '?'
            } else {
                c
            }
        })
        .collect();
    number_line(n, &clean, max_chars, compact)
}

/// What the person deciding on the card sees: for each operation, the rows
/// it replaces or the place it inserts at, as the file is now, from the same
/// plan `run` makes ([`plan`]). `None` where the plan is refused (the
/// refusal answers; no card is shown). Writes nothing and records nothing.
pub(crate) fn preview(ctx: &ToolCtx, input: &Value) -> Option<String> {
    let call = parse(input).ok()?;
    let shown = input.get("path")?.as_str()?;
    let path = ctx.resolve_for_write(shown).ok()?;
    let md = std::fs::symlink_metadata(&path).ok()?;
    if !md.file_type().is_file() || md.len() > crate::seen::SNAPSHOT_MAX_FILE as u64 {
        return None;
    }
    let key = ctx.seen_key(&path);
    let live = read_small(&path)?;
    let p = plan(ctx, &key, shown, &live, &call).ok()?;
    let (max_chars, compact) = line_style(ctx);
    let (_, rows) = split_rows(&live);
    let mut out = String::new();
    if p.changed.is_some() {
        out.push_str("the file changed since the tagged version; rows are numbered as it is now\n");
    }
    let mut used = 0;
    let mut chars = 0;
    let mut left_rows = 0;
    let mut left_ops = 0;
    for spot in &p.spots {
        // The lead and at least one row, or the operation is left out.
        if used + 2 > PREVIEW_LINES || chars >= PREVIEW_CHARS {
            left_ops += 1;
            continue;
        }
        let (lead, from, to) = match *spot {
            Spot::Rows(a, b) if a == b => (format!("row {a} is replaced:\n"), a, b),
            Spot::Rows(a, b) => (format!("rows {a}-{b} are replaced:\n"), a, b),
            Spot::After(0) => ("inserted at the top of the file:\n".to_string(), 1, 0),
            Spot::After(n) => (format!("inserted after row {n}:\n"), n, n),
        };
        out.push_str(&lead);
        used += 1;
        for r in from..=to {
            if used >= PREVIEW_LINES || chars >= PREVIEW_CHARS {
                left_rows += 1;
                continue;
            }
            let row = preview_row(r, content(rows[r - 1]), max_chars, compact);
            chars += row.chars().count();
            out.push_str(&row);
            used += 1;
        }
    }
    if left_rows > 0 {
        out.push_str(&format!("[{left_rows} more rows are not shown]\n"));
    }
    if left_ops > 0 {
        out.push_str(&format!("[{left_ops} more operations are not shown]\n"));
    }
    Some(out)
}

/// A regular file of at most the snapshot size as UTF-8, read without
/// following a link.
fn read_small(path: &Path) -> Option<String> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits() | crate::open_flags())
        .open(path)
        .ok()?;
    if !f.metadata().ok()?.file_type().is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    f.take(crate::seen::SNAPSHOT_MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > crate::seen::SNAPSHOT_MAX_FILE {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub(crate) async fn run(ctx: &ToolCtx, input: Value) -> ToolOutcome {
    let shown = input
        .get("path")
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();
    let path = match crate::str_arg(&input, "path").and_then(|p| ctx.resolve_for_write(p)) {
        Ok(p) => p,
        Err(e) => return ToolOutcome::err(e),
    };
    let call = match parse(&input) {
        Ok(c) => c,
        Err(e) => return ToolOutcome::err(e),
    };
    let key = ctx.seen_key(&path);
    let _lock = crate::seen::lock_path(&key).await;
    let live = match read_nofollow(&path).await {
        Ok(t) => t,
        Err(e) => return ToolOutcome::err(e),
    };
    let p = match plan(ctx, &key, &shown, &live, &call) {
        Ok(p) => p,
        Err(no) => {
            let print = ctx.read_offered && ctx.resolve(&shown).is_ok();
            return ToolOutcome::err(refuse(ctx, &key, &shown, &live, no, print));
        }
    };
    if let Err(e) = write_nofollow(&path, p.updated.as_bytes()).await {
        return ToolOutcome::err(e);
    }
    let (rows_text, shown_rows) = crate::edit::changed_lines(ctx, &p.updated, &p.landed);
    let n = call.ops.len();
    let mut report = format!(
        "Edited {}: {n} operation{}",
        path.display(),
        if n == 1 { "" } else { "s" }
    );
    if p.changed.is_some() {
        report.push_str(&format!(
            " (the file was not the version tagged {}; the rows you named had not changed since, \
             so the edit was made at the rows of the file as it is now)",
            call.tag.as_deref().unwrap_or_default()
        ));
    }
    report.push_str(".\n");
    report.push_str(&rows_text);
    if !p.replaced.is_empty() {
        let (max_chars, compact) = line_style(ctx);
        let (_, old) = split_rows(&live);
        let mut left = MAX_OLD_ROWS;
        report.push_str(
            "The replaced rows read, before this edit (numbered as the file was then):\n",
        );
        for (a, b) in &p.replaced {
            for r in *a..=*b {
                if left == 0 {
                    break;
                }
                left -= 1;
                report.push_str(&number_line(r, content(old[r - 1]), max_chars, compact));
            }
        }
        if left == 0 && p.replaced.iter().map(|(a, b)| b - a + 1).sum::<usize>() > MAX_OLD_ROWS {
            report.push_str("[more replaced rows not repeated here]\n");
        }
    }
    let live_sha = sha256(live.as_bytes());
    if let Some((a, b)) = p.changed
        && ctx
            .seen
            .version(&key, &tag(&live_sha))
            .is_none_or(|v| !v.covers(a, b))
    {
        // Rows of the file the agent had not seen.
        let (bom, rows) = split_rows(&live);
        let start = bom + rows[..a - 1].iter().map(|r| r.len()).sum::<usize>();
        let end = bom
            + rows[..b.min(rows.len())]
                .iter()
                .map(|r| r.len())
                .sum::<usize>();
        report.push_str(
            "\nNote: the file had changed on disk since that version, in rows your edit did not \
             touch. They read (numbered as the file was before this edit):\n",
        );
        report.push_str(&show(ctx, &live, start, end));
    }
    let new_sha = sha256(p.updated.as_bytes());
    report.push_str(&format!("\n{}", header(ctx, &path, &new_sha)));
    let mut o = ToolOutcome::ok(report);
    // The guard moves on only from the hash it holds: a file changed under
    // the agent stays "changed since it was read" for the old form.
    let guard = ctx.seen.get(&key).map(|s| s.sha);
    if guard == Some(live_sha) {
        let mut seen = ctx
            .seen
            .record_shown(&key, p.updated.as_bytes(), &shown_rows);
        seen.base = Some(crate::seen::hex(&live_sha));
        o.seen = Some(seen);
    } else {
        ctx.seen
            .add_version(&key, p.updated.as_bytes(), &shown_rows);
    }
    o
}

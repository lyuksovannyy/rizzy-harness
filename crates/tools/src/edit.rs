//! `edit`: replace text in a file the agent has read (ADOPTION_PLAN T.2).
//!
//! * **Staleness guard** (research 08 §6.1): a file not read in this session
//!   is refused; one changed on disk since it was read is edited only where
//!   the replaced text lies outside the lines that changed (checked against
//!   the snapshot of what was shown), otherwise refused with those lines as
//!   they are now. Without a snapshot (after a restart) any change refuses.
//!   The check and the write run under the path's lock.
//! * **Several edits** (`edits`), each matched against the file as it was
//!   before the call, disjoint, all made or none (Pi's batch).
//! * **The match ladder** (§6.2), tier by tier over the whole file, each
//!   tier needing one match (or `replace_all`): exact; then on whole lines,
//!   trailing whitespace ignored; then also typographic quotes, dashes and
//!   spaces read as ASCII (one character for one, OpenCode's fold). Matching
//!   is on an LF view of the file and the replacement is spliced into the
//!   original bytes; a line-number gutter pasted from `read` is taken off.
//!   Indentation is never forgiven. The tier is reported.
//! * **The result** shows the changed lines, numbered as `read` numbers
//!   them, with two lines of context, so the next edit needs no new read.

use std::borrow::Cow;
use std::path::Path;

use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::fs::{line_style, number_line, read_nofollow, write_nofollow};
use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

pub struct Edit;

/// Lines of context around each change in the result.
const CONTEXT: usize = 2;
/// Lines the result shows of the changes, at most.
const MAX_RESULT_LINES: usize = 60;
/// Lines a refusal or note shows of what changed on disk, at most.
const MAX_CHANGED_LINES: usize = 40;
/// The nearest text on a miss: at most this many lines, each cut here.
const HINT_LINES: usize = 12;
const HINT_CHARS: usize = 200;
/// `new_string` this long found in the file means the edit may be done.
const ALREADY_MIN: usize = 20;

#[async_trait::async_trait]
impl Tool for Edit {
    fn name(&self) -> &'static str {
        "edit"
    }
    fn description(&self) -> &'static str {
        "Replace text in a file you have read in this session. `old_string` must match the file \
         once (copy it from `read` output, without the line-number prefix), unless \
         `replace_all` is true. When nothing matches exactly, a match on whole lines that \
         differs only in trailing whitespace, or in typographic quotes, dashes or spaces, is \
         used and the result says so; indentation must match. For several changes to one file, \
         pass `edits` (a list of {old_string, new_string, replace_all}) instead: each is matched \
         against the file as it was before this call, they must not overlap, and either all are \
         made or none. The result shows the changed lines numbered as `read` shows them, so you \
         need not read the file again. Refused if the file changed on disk since you read it, \
         in the lines you are replacing."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean"},
                "edits": {
                    "type": "array",
                    "description": "Several replacements in this file, instead of old_string/new_string",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": {"type": "string"},
                            "new_string": {"type": "string"},
                            "replace_all": {"type": "boolean"}
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let shown = str_arg(&input, "path").unwrap_or_default().to_string();
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve_for_write(p)) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let specs = match specs(&input) {
            Ok(s) => s,
            Err(e) => return ToolOutcome::err(e),
        };
        let key = ctx.seen_key(&path);
        let _lock = crate::seen::lock_path(&key).await;
        let text = match read_nofollow(&path).await {
            Ok(t) => t,
            Err(e) => return ToolOutcome::err(e),
        };
        match edit(ctx, &path, &key, &shown, &text, &specs) {
            Ok((updated, report)) => match write_nofollow(&path, updated.as_bytes()).await {
                Ok(()) => {
                    let mut o = ToolOutcome::ok(report);
                    o.seen = Some(ctx.seen.record(&key, updated.as_bytes()));
                    o
                }
                Err(e) => ToolOutcome::err(e),
            },
            Err(e) => ToolOutcome::err(e),
        }
    }
}

/// One replacement asked for.
struct Spec {
    old: String,
    new: String,
    all: bool,
    /// `edits[i]: ` for a batch, empty for the single form.
    label: String,
}

fn specs(input: &Value) -> Result<Vec<Spec>, String> {
    let single = input.get("old_string").is_some() || input.get("new_string").is_some();
    let list = match input.get("edits") {
        None | Some(Value::Null) => None,
        // A list sent as a JSON string (some models do).
        Some(Value::String(s)) => Some(
            serde_json::from_str::<Value>(s)
                .map_err(|e| format!("`edits` is a string that is not a JSON list: {e}"))?,
        ),
        Some(v) => Some(v.clone()),
    };
    let one = |v: &Value, label: String| -> Result<Spec, String> {
        let get = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{label}missing string argument `{k}`"))
        };
        let (old, new) = (get("old_string")?, get("new_string")?);
        if old.is_empty() {
            return Err(format!(
                "{label}old_string must not be empty; use `write` to create files"
            ));
        }
        if old == new {
            return Err(format!("{label}old_string and new_string are identical"));
        }
        let all = v
            .get("replace_all")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        Ok(Spec {
            old,
            new,
            all,
            label,
        })
    };
    match list {
        None => Ok(vec![one(input, String::new())?]),
        Some(_) if single => {
            Err("give either old_string/new_string or `edits`, not both".to_string())
        }
        Some(Value::Array(items)) if !items.is_empty() => items
            .iter()
            .enumerate()
            .map(|(i, v)| one(v, format!("edits[{i}]: ")))
            .collect(),
        Some(Value::Array(_)) => Err("`edits` is empty".to_string()),
        Some(_) => Err("`edits` must be a list of {old_string, new_string}".to_string()),
    }
}

/// How a match was found.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Tier {
    Exact,
    TrailingWs,
    Typographic,
}

impl Tier {
    fn note(self) -> &'static str {
        match self {
            Tier::Exact => "",
            Tier::TrailingWs => "after ignoring trailing whitespace on whole lines",
            Tier::Typographic => {
                "after ignoring trailing whitespace and reading typographic quotes, dashes and \
                 spaces as their ASCII forms, on whole lines"
            }
        }
    }
}

/// A span of the original text to replace.
#[derive(Clone, Copy)]
struct Span {
    a: usize,
    b: usize,
    /// The match took in no final newline the old text had: drop one from
    /// the replacement, so the file keeps having none.
    trim_nl: bool,
}

/// The file seen with `\r\n` as `\n`, offsets mapped back.
struct View<'a> {
    orig: &'a str,
    lf: Cow<'a, str>,
    /// View offsets of each `\n` whose `\r` was dropped.
    cr: Vec<usize>,
    /// Bytes of a leading BOM (not part of line 1 for line matching).
    bom: usize,
}

impl<'a> View<'a> {
    fn new(orig: &'a str) -> Self {
        let (lf, cr) = if orig.contains("\r\n") {
            let mut lf = String::with_capacity(orig.len());
            let mut cr = Vec::new();
            let mut rest = orig;
            while let Some(i) = rest.find("\r\n") {
                lf.push_str(&rest[..i]);
                cr.push(lf.len());
                lf.push('\n');
                rest = &rest[i + 2..];
            }
            lf.push_str(rest);
            (Cow::Owned(lf), cr)
        } else {
            (Cow::Borrowed(orig), Vec::new())
        };
        let bom = if orig.starts_with('\u{feff}') { 3 } else { 0 };
        Self { orig, lf, cr, bom }
    }

    fn to_orig(&self, v: usize) -> usize {
        v + self.cr.partition_point(|&p| p < v)
    }

    /// Start of each line (as `read` counts them) in the view.
    fn starts(&self) -> Vec<usize> {
        let mut s = vec![0];
        s.extend(self.lf.match_indices('\n').map(|(i, _)| i + 1));
        if self.lf.ends_with('\n') || self.lf.is_empty() {
            s.pop();
        }
        s
    }
}

/// Typographic characters read as their ASCII forms, one for one.
fn fold(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' | '\u{2032}' => '\'',
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' | '\u{2033}' => '"',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        '\u{00a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' => ' ',
        c => c,
    }
}

fn same_line(file: &str, want: &str, typographic: bool) -> bool {
    let (f, w) = (file.trim_end(), want.trim_end());
    if typographic {
        f.chars().map(fold).eq(w.chars().map(fold))
    } else {
        f == w
    }
}

/// Non-overlapping matches of `needle` (LF form) at `tier`, as spans of the
/// original text.
fn find(v: &View, starts: &mut Option<Vec<usize>>, needle: &str, tier: Tier) -> Vec<Span> {
    if tier == Tier::Exact {
        return v
            .lf
            .match_indices(needle)
            .map(|(i, m)| Span {
                a: v.to_orig(i),
                b: v.to_orig(i + m.len()),
                trim_nl: false,
            })
            .collect();
    }
    let starts = starts.get_or_insert_with(|| v.starts());
    let (body, nl) = match needle.strip_suffix('\n') {
        Some(b) => (b, true),
        None => (needle, false),
    };
    let want: Vec<&str> = body.split('\n').collect();
    if want.iter().all(|l| l.trim().is_empty()) {
        return Vec::new();
    }
    let n = starts.len();
    let ends_nl = v.lf.ends_with('\n');
    let start = |i: usize| {
        if i == 0 {
            v.bom.min(v.lf.len())
        } else {
            starts[i]
        }
    };
    let content_end = |i: usize| {
        if i + 1 < n {
            starts[i + 1] - 1
        } else if ends_nl {
            v.lf.len() - 1
        } else {
            v.lf.len()
        }
    };
    let typographic = tier == Tier::Typographic;
    let k = want.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + k <= n {
        let hit = (0..k).all(|j| {
            let l = i + j;
            same_line(&v.lf[start(l)..content_end(l)], want[j], typographic)
        });
        if !hit {
            i += 1;
            continue;
        }
        let last = i + k - 1;
        let has_nl = last + 1 < n || ends_nl;
        let end = content_end(last) + usize::from(nl && has_nl);
        out.push(Span {
            a: v.to_orig(start(i)),
            b: v.to_orig(end),
            trim_nl: nl && !has_nl,
        });
        i += k;
    }
    out
}

/// The line-number gutter `read` puts in front of a line (`   12\t` or
/// `12\t`): the rest of the line, if it has one.
fn gutterless(line: &str) -> Option<&str> {
    let t = line.trim_start_matches(' ');
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    t[digits..].strip_prefix('\t').filter(|_| digits > 0)
}

/// `s` without a line-number gutter copied from `read`, if every non-blank
/// line has one (so `old_string` is read output, not text that happens to
/// start with a number and a tab).
fn strip_gutter(s: &str) -> Option<String> {
    let mut any = false;
    let mut out = Vec::new();
    for line in s.split('\n') {
        if line.trim().is_empty() {
            out.push("");
            continue;
        }
        out.push(gutterless(line)?);
        any = true;
    }
    any.then(|| out.join("\n"))
}

/// `new_string` once `old_string` was found to be pasted `read` output:
/// the gutter off every line that has one, the lines typed without one
/// kept as they are.
fn strip_gutter_where_present(s: &str) -> String {
    s.split('\n')
        .map(|l| gutterless(l).unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 1-based line of byte `off` in `text`.
fn line_of(text: &str, off: usize) -> usize {
    1 + text.as_bytes()[..off.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
}

/// Where one spec matched, and how.
struct Found {
    spans: Vec<Span>,
    tier: Tier,
    gutter: bool,
    /// Matched on the original bytes, because `old_string` held a `\r\n`
    /// that is in the file as it is: `new_string` goes in as it is too.
    raw: bool,
    /// The replacement text (the gutter taken off when it had one too).
    new: String,
}

/// Match `spec` against the file: the first tier with a match wins.
fn locate(v: &View, starts: &mut Option<Vec<usize>>, spec: &Spec) -> Result<Found, String> {
    let found = |spans, tier, gutter, raw, new| Found {
        spans,
        tier,
        gutter,
        raw,
        new,
    };
    // A CRLF the model typed that the file has: the original bytes.
    if spec.old.contains('\r') {
        let spans: Vec<Span> = v
            .orig
            .match_indices(spec.old.as_str())
            .map(|(i, m)| Span {
                a: i,
                b: i + m.len(),
                trim_nl: false,
            })
            .collect();
        if !spans.is_empty() {
            return unique(
                v,
                spec,
                found(spans, Tier::Exact, false, true, spec.new.clone()),
            );
        }
    }
    let old = spec.old.replace("\r\n", "\n");
    let tiers = [Tier::Exact, Tier::TrailingWs, Tier::Typographic];
    for tier in tiers {
        let spans = find(v, starts, &old, tier);
        if !spans.is_empty() {
            return unique(v, spec, found(spans, tier, false, false, spec.new.clone()));
        }
    }
    if let Some(stripped) = strip_gutter(&old) {
        let new = strip_gutter_where_present(&spec.new);
        for tier in tiers {
            let spans = find(v, starts, &stripped, tier);
            if !spans.is_empty() {
                return unique(v, spec, found(spans, tier, true, false, new));
            }
        }
    }
    Err(miss(v, spec, &old))
}

fn unique(v: &View, spec: &Spec, f: Found) -> Result<Found, String> {
    let spans = &f.spans;
    if spans.len() > 1 && !spec.all {
        let mut lines: Vec<String> = spans
            .iter()
            .take(10)
            .map(|s| line_of(v.orig, s.a).to_string())
            .collect();
        if spans.len() > 10 {
            lines.push("…".into());
        }
        return Err(format!(
            "{}old_string matches {} times (lines {}); add more of the surrounding text to make \
             it unique, or set replace_all to change every one",
            spec.label,
            spans.len(),
            lines.join(", ")
        ));
    }
    Ok(f)
}

/// For a match made through the typographic fold: `new` with the file's
/// own characters kept wherever it agrees with the matched text once both
/// are folded (the common prefix and suffix, in characters), so quotes,
/// dashes and spaces the model did not change stay as the file has them.
/// Both are in LF form.
fn keep_file_chars(matched: &str, new: &str) -> String {
    let m: Vec<char> = matched.chars().collect();
    let n: Vec<char> = new.chars().collect();
    let p = m
        .iter()
        .zip(&n)
        .take_while(|(a, b)| fold(**a) == fold(**b))
        .count();
    let room = m.len().min(n.len()) - p;
    let q = m
        .iter()
        .rev()
        .zip(n.iter().rev())
        .take(room)
        .take_while(|(a, b)| fold(**a) == fold(**b))
        .count();
    m[..p]
        .iter()
        .chain(&n[p..n.len() - q])
        .chain(&m[m.len() - q..])
        .collect()
}

/// Why nothing matched, with what helps most: the edit may already be
/// made, or here is the nearest text (OpenCode's line vote).
fn miss(v: &View, spec: &Spec, old: &str) -> String {
    let lead = format!(
        "{}old_string was not found in the file. It must match exactly; only trailing \
         whitespace and typographic quotes, dashes and spaces are forgiven, on whole lines.",
        spec.label
    );
    let new = spec.new.replace("\r\n", "\n");
    if new.trim().chars().count() >= ALREADY_MIN
        && let Some(i) = v.lf.find(new.as_str())
    {
        return format!(
            "{lead} new_string is already in the file (from line {}): this edit may already \
             have been made.",
            line_of(&v.lf, i)
        );
    }
    let key = |l: &str| -> Option<String> {
        let k: String = l
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .map(fold)
            .collect();
        k.chars().any(char::is_alphanumeric).then_some(k)
    };
    let want: Vec<Option<String>> = old
        .strip_suffix('\n')
        .unwrap_or(old)
        .split('\n')
        .map(key)
        .collect();
    let mut distinct: Vec<&String> = want.iter().flatten().collect();
    distinct.sort();
    distinct.dedup();
    let mut votes: std::collections::HashMap<usize, usize> = Default::default();
    for (j, line) in v.lf.split('\n').enumerate() {
        let Some(k) = key(line) else { continue };
        for (i, w) in want.iter().enumerate() {
            if w.as_ref() == Some(&k) && j >= i {
                *votes.entry(j - i).or_default() += 1;
            }
        }
    }
    let best = votes
        .into_iter()
        .max_by_key(|(start, n)| (*n, std::cmp::Reverse(*start)));
    match best {
        Some((start, n)) if !distinct.is_empty() && n * 2 >= distinct.len() => {
            let lines: Vec<&str> = v.lf.split('\n').collect();
            let end = (start + want.len())
                .min(lines.len())
                .min(start + HINT_LINES);
            let mut out = format!(
                "{lead} The nearest text is at lines {}-{}; compare it with old_string \
                 (indentation and every other character count):\n",
                start + 1,
                end
            );
            for (i, l) in lines[start..end].iter().enumerate() {
                out.push_str(&number_line(start + i + 1, l, HINT_CHARS, false));
            }
            out
        }
        _ => format!(
            "{lead} Read the part of the file you want to change again and copy the text from it."
        ),
    }
}

/// The changed lines of `live` against `snapshot`, widened to whole lines:
/// byte range in `live`.
fn changed(snapshot: &str, live: &str) -> (usize, usize) {
    let (s, l) = (snapshot.as_bytes(), live.as_bytes());
    let p = s.iter().zip(l).take_while(|(a, b)| a == b).count();
    let room = s.len().min(l.len()) - p;
    let q = s
        .iter()
        .rev()
        .zip(l.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (p, l.len() - q);
    let start = live.as_bytes()[..a]
        .iter()
        .rposition(|c| *c == b'\n')
        .map_or(0, |i| i + 1);
    let from = if b > a { b - 1 } else { a };
    let end = live.as_bytes()[from.min(live.len())..]
        .iter()
        .position(|c| *c == b'\n')
        .map_or(live.len(), |i| from + i + 1);
    (start, end.max(start))
}

/// Lines `[start, end)` (byte range on line boundaries) of `text`, numbered
/// as `read` numbers them, at most [`MAX_CHANGED_LINES`].
fn show(ctx: &ToolCtx, text: &str, start: usize, end: usize) -> String {
    let (max_chars, compact) = line_style(ctx);
    let first = line_of(text, start);
    let part = &text[start..end];
    let part = part.strip_suffix('\n').unwrap_or(part);
    let lines: Vec<&str> = if end > start {
        part.split('\n').collect()
    } else {
        Vec::new()
    };
    let mut out = String::new();
    for (i, l) in lines.iter().take(MAX_CHANGED_LINES).enumerate() {
        out.push_str(&number_line(first + i, l, max_chars, compact));
    }
    if lines.len() > MAX_CHANGED_LINES {
        out.push_str(&format!(
            "[{} more changed lines, to line {}: read them]\n",
            lines.len() - MAX_CHANGED_LINES,
            first + lines.len() - 1
        ));
    }
    out
}

/// The refusal for a file changed since the agent read it: what changed,
/// shown as it is now when a snapshot says what it was.
pub(crate) fn stale_refusal(
    ctx: &ToolCtx,
    shown: &str,
    snapshot: Option<&str>,
    live: Option<&str>,
    done: &str,
) -> String {
    let lead = format!(
        "{shown} has changed on disk since you last read it in this session (the user, a \
         command or another session changed it); {done}."
    );
    match (snapshot, live) {
        (Some(s), Some(l)) => {
            let (a, b) = changed(s, l);
            if b > a {
                format!(
                    "{lead} The changed lines now read:\n{}Read it again (at least these \
                     lines) and retry with the text as it is now.",
                    show(ctx, l, a, b)
                )
            } else {
                format!("{lead} Lines were removed at its end. Read it again and retry.")
            }
        }
        _ => format!("{lead} Read it again and retry with the text as it is now."),
    }
}

/// The edit itself: the guard, every spec matched against `text`, the new
/// text and the report.
fn edit(
    ctx: &ToolCtx,
    path: &Path,
    key: &Path,
    shown: &str,
    text: &str,
    specs: &[Spec],
) -> Result<(String, String), String> {
    let Some(seen) = ctx.seen.get(key) else {
        return Err(format!(
            "{shown} has not been read in this session; nothing was changed. Read it first with \
             `read` (the part that holds the text you are replacing is enough), then edit it."
        ));
    };
    let stale = crate::seen::sha256(text.as_bytes()) != seen.sha;
    // Changed on disk, and nothing says what the agent saw: refuse at once
    // (a "not found" would mislead).
    let region = match (stale, seen.snapshot.as_deref()) {
        (false, _) => None,
        (true, Some(s)) => Some(changed(s, text)),
        (true, None) => {
            return Err(stale_refusal(ctx, shown, None, None, "nothing was changed"));
        }
    };
    let refuse_stale = || {
        stale_refusal(
            ctx,
            shown,
            seen.snapshot.as_deref().map(String::as_str),
            Some(text),
            "nothing was changed",
        )
    };
    let v = View::new(text);
    let mut starts = None;
    let first_crlf = text
        .find('\n')
        .is_some_and(|i| i > 0 && text.as_bytes()[i - 1] == b'\r');
    // (span, replacement, spec index)
    let mut planned: Vec<(Span, String, usize)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (k, spec) in specs.iter().enumerate() {
        let found = match locate(&v, &mut starts, spec) {
            Ok(f) => f,
            Err(_) if stale => return Err(refuse_stale()),
            Err(e) => return Err(e),
        };
        let how = match (found.gutter, found.tier) {
            (false, Tier::Exact) => None,
            (true, Tier::Exact) => {
                Some("after removing line-number prefixes copied from `read` output".to_string())
            }
            (true, t) => Some(format!(
                "after removing line-number prefixes copied from `read` output, and {}",
                t.note()
            )),
            (false, t) => Some(t.note().to_string()),
        };
        if let Some(h) = how {
            notes.push(format!("{}matched {h}", spec.label));
        }
        for s in &found.spans {
            let mut rep = if found.raw {
                found.new.clone()
            } else {
                let span = &text[s.a..s.b];
                let crlf = if span.contains("\r\n") {
                    true
                } else if span.contains('\n') {
                    false
                } else {
                    first_crlf
                };
                let mut lf = found.new.replace("\r\n", "\n");
                if found.tier == Tier::Typographic {
                    lf = keep_file_chars(&span.replace("\r\n", "\n"), &lf);
                }
                if crlf { lf.replace('\n', "\r\n") } else { lf }
            };
            if s.trim_nl {
                for end in ["\r\n", "\n"] {
                    if let Some(r) = rep.strip_suffix(end) {
                        rep = r.to_string();
                        break;
                    }
                }
            }
            planned.push((*s, rep, k));
        }
    }
    planned.sort_by_key(|(s, _, _)| (s.a, s.b));
    for w in planned.windows(2) {
        let ((p, _, i), (n, _, j)) = (&w[0], &w[1]);
        if n.a < p.b {
            let at = line_of(text, n.a);
            return Err(if i == j {
                format!("{}matches overlap at line {at}", specs[*i].label)
            } else {
                format!(
                    "edits[{i}] and edits[{j}] overlap (both take in line {at}); make them one \
                     edit; nothing was changed"
                )
            });
        }
    }
    if let Some((ra, rb)) = region
        && planned.iter().any(|(s, _, _)| s.a < rb && ra < s.b)
    {
        return Err(refuse_stale());
    }
    // Apply, in order, keeping where each replacement lands.
    let mut updated = String::with_capacity(text.len());
    let mut landed: Vec<(usize, usize)> = Vec::new();
    let mut cursor = 0;
    let mut shift: isize = 0;
    let mut region_shift: isize = 0;
    for (s, rep, _) in &planned {
        updated.push_str(&text[cursor..s.a]);
        let at = updated.len();
        updated.push_str(rep);
        landed.push((at, updated.len()));
        cursor = s.b;
        shift += rep.len() as isize - (s.b - s.a) as isize;
        if let Some((ra, _)) = region
            && s.b <= ra
        {
            region_shift = shift;
        }
    }
    updated.push_str(&text[cursor..]);

    let n = planned.len();
    let mut report = format!(
        "Edited {}: {n} replacement{}",
        path.display(),
        if n == 1 { "" } else { "s" }
    );
    if !notes.is_empty() {
        report.push_str(&format!(" ({})", notes.join("; ")));
    }
    report.push_str(".\n");
    report.push_str(&changed_lines(ctx, &updated, &landed));
    if let Some((ra, rb)) = region {
        let a = (ra as isize + region_shift) as usize;
        let b = (rb as isize + region_shift) as usize;
        report.push_str(
            "\nNote: the file had changed on disk since you last read it, in lines your edit \
             did not touch. They now read:\n",
        );
        report.push_str(&show(ctx, &updated, a, b));
    }
    Ok((updated, report))
}

/// The lines around each replacement in `text` (the new file), numbered as
/// `read` numbers them, nearby ones merged.
fn changed_lines(ctx: &ToolCtx, text: &str, landed: &[(usize, usize)]) -> String {
    let (max_chars, compact) = line_style(ctx);
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    if text.ends_with('\n') || text.is_empty() {
        starts.pop();
    }
    let total = starts.len();
    if total == 0 {
        return "The file is now empty.\n".to_string();
    }
    let line = |off: usize| starts.partition_point(|s| *s <= off).max(1);
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &(a, b) in landed {
        let lo = line(a);
        let hi = if b > a { line(b - 1) } else { lo }.max(lo);
        let r = (lo.saturating_sub(CONTEXT).max(1), (hi + CONTEXT).min(total));
        match ranges.last_mut() {
            Some(last) if r.0 <= last.1 + 1 => last.1 = last.1.max(r.1),
            _ => ranges.push(r),
        }
    }
    let text_of = |n: usize| {
        let a = starts[n - 1];
        let b = starts.get(n).map_or(text.len(), |s| s - 1);
        text[a..b.max(a)]
            .strip_suffix('\n')
            .unwrap_or(&text[a..b.max(a)])
    };
    let mut out = String::from("The changed lines now read (numbered as `read` numbers them):\n");
    let mut used = 0;
    for (k, (lo, hi)) in ranges.iter().enumerate() {
        if used >= MAX_RESULT_LINES {
            let rest: Vec<String> = ranges[k..]
                .iter()
                .map(|(a, b)| format!("{a}-{b}"))
                .collect();
            out.push_str(&format!(
                "[more changes not shown, at lines {}: read them if you need to]\n",
                rest.join(", ")
            ));
            break;
        }
        if k > 0 {
            out.push_str("--\n");
        }
        for n in *lo..=*hi {
            if used >= MAX_RESULT_LINES {
                out.push_str(&format!("[lines {n}-{hi} not shown]\n"));
                break;
            }
            out.push_str(&number_line(n, text_of(n), max_chars, compact));
            used += 1;
        }
    }
    out
}

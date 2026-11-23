//! Changing text from the web page (ADOPTION_PLAN X.60, PLAN.md T99): a
//! person clicks a paragraph or a cell of the preview, types, and saves a
//! copy. The page sends `(place, text)` pairs; this turns each into one of
//! the existing edit operations (`set_text` for a paragraph, `set` for a
//! cell) and runs them through the existing edit functions, so every check
//! of theirs applies unchanged: a stale tag, the limits, the formula check,
//! hidden, merged and array cells, and the package check of the output. Like
//! every edit-engine function it runs only in the confined helper.
//!
//! Nothing here widens what the edit tools do: there is no operation that is
//! not one of theirs. What a typed cell MEANS (a number, a formula, text,
//! empty) is decided here, once, by [`cell_value`]; the page sends the
//! string it was given.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::docx::{self, DocOp};
use super::pkg::Source;
use super::preview::{Why, read, why_of};
use super::serve::Item;
use super::xlsx::{self, CellVal, SheetOp};
use crate::formula;
use crate::limits::{MAX_EDIT_TEXT_CHARS, MAX_PAGE_CHANGES};
use crate::xml;

/// One change the page asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    /// A paragraph's tag (`p2#00a1`, the preview's `at`) or a cell's address
    /// (`B7`).
    pub at: String,
    /// What the paragraph or cell holds from now on, as typed.
    pub text: String,
}

/// Why one change was not applied. One word each, decided from the request
/// and from the engine's own refusals, never carried as text of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeWhy {
    /// The place is not a paragraph tag or a cell address of this kind of
    /// file.
    BadPlace,
    /// The paragraph changed since the preview (its hash differs).
    Stale,
    /// There is no such paragraph any more.
    Gone,
    /// The place holds something that cannot be changed from here: a
    /// field, a link, a picture, tracked changes; a hidden, merged, array or
    /// shared-formula cell.
    NotEditable,
    /// The formula is refused (empty, too long, or a function that is not
    /// allowed).
    BadFormula,
    /// The text is longer than an edit may hold.
    TooLong,
    /// The same place twice in one request.
    Duplicate,
    /// Fine by itself, not applied because another change was refused (a
    /// request applies all its changes or none).
    Skipped,
    /// Refused by the engine for a reason that has no word of its own.
    Other,
}

/// What a request made: one item per change, in order, and the new file only
/// when every change applied.
#[derive(Debug)]
pub struct Applied {
    pub items: Vec<Item>,
    pub bytes: Option<Vec<u8>>,
}

fn refused(why: ChangeWhy) -> Item {
    Item {
        ok: false,
        msg: String::new(),
        why: Some(why),
    }
}

/// Digits only, ASCII.
fn digits(b: &[u8], mut i: usize) -> (usize, usize) {
    let start = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    (i, i - start)
}

/// The strict rule for typing a number into a cell: an optional sign, digits
/// with an optional `.` and digits (one side may be missing, not both, and a
/// `.` has digits after it), an optional exponent (`e`, an optional sign,
/// digits); no thousands separators, no spaces, no hex, no `inf` or `NaN`; no
/// leading zeros in a whole part of more than one digit (`007` is an
/// identifier, not 7); at most 15 digits (a double keeps no more); finite.
/// Anything else is text.
pub fn strict_number(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int_start = i;
    let (j, int_len) = digits(b, i);
    i = j;
    let mut frac_len = 0;
    if b.get(i) == Some(&b'.') {
        let (j, n) = digits(b, i + 1);
        if n == 0 {
            return None;
        }
        frac_len = n;
        i = j;
    }
    if int_len + frac_len == 0 || int_len + frac_len > 15 {
        return None;
    }
    if int_len > 1 && b[int_start] == b'0' {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let (j, n) = digits(b, i);
        if n == 0 {
            return None;
        }
        i = j;
    }
    if i != b.len() {
        return None;
    }
    let x: f64 = s.parse().ok()?;
    // No negative zero: a cell that shows `-0` is a surprise.
    x.is_finite().then_some(if x == 0.0 { 0.0 } else { x })
}

/// What a typed string is: empty clears the cell; `=` and a formula is a
/// formula (stored without the `=`, and checked as the edit tools check
/// one); a number by [`strict_number`]; anything else text.
pub fn cell_value(text: &str) -> Result<CellVal, ChangeWhy> {
    if text.chars().count() > MAX_EDIT_TEXT_CHARS {
        return Err(ChangeWhy::TooLong);
    }
    if text.is_empty() {
        return Ok(CellVal::Null);
    }
    if text.starts_with('=') {
        return formula::check(text)
            .map(CellVal::Formula)
            .map_err(|_| ChangeWhy::BadFormula);
    }
    if let Some(x) = strict_number(text) {
        return Ok(CellVal::Num(x));
    }
    Ok(CellVal::Text(xml::clean(text)))
}

/// A paragraph tag of the body as the preview prints it: `p`, digits, `#`,
/// four hexadecimal digits. Returns the paragraph's key (`p2`). A table
/// cell's tag is not accepted here: a table is not edited from the page.
fn paragraph_key(at: &str) -> Option<&str> {
    let (key, hash) = at.rsplit_once('#')?;
    let n = key.strip_prefix('p')?;
    if n.is_empty() || n.len() > 9 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if hash.len() != 4 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(key)
}

/// A request's refusal as a whole: the file's own limit or protection, else
/// damage. The words are the engine's; only the kind is passed on.
fn why_of_edit(e: &str) -> Why {
    if e == super::E_DOC_LOCKED || e == super::E_SHEET_LOCKED {
        Why::Locked
    } else {
        why_of(e)
    }
}

/// Run the valid operations and put each outcome back at its change.
fn finish(
    mut items: Vec<Option<Item>>,
    at: &[usize],
    outcome: super::Outcome,
    classify: impl Fn(&str) -> ChangeWhy,
) -> Applied {
    let any_refused = items.iter().any(Option::is_some);
    let bytes = if any_refused { None } else { outcome.bytes };
    for (j, r) in outcome.results.into_iter().enumerate() {
        let Some(&k) = at.get(j) else { continue };
        items[k] = Some(match r {
            Ok(m) if bytes.is_some() => Item {
                ok: true,
                msg: m,
                why: None,
            },
            Ok(_) => refused(ChangeWhy::Skipped),
            Err(m) => Item {
                ok: false,
                msg: m.clone(),
                why: Some(classify(&m)),
            },
        });
    }
    // A change the engine gave no result for did not apply.
    let items = items
        .into_iter()
        .map(|i| i.unwrap_or_else(|| refused(ChangeWhy::Other)))
        .collect();
    Applied { items, bytes }
}

fn classify_doc(m: &str) -> ChangeWhy {
    if m.contains("changed since the view") {
        ChangeWhy::Stale
    } else if m.contains("there is no paragraph") {
        ChangeWhy::Gone
    } else if m.contains("that set_text would lose") {
        ChangeWhy::NotEditable
    } else {
        ChangeWhy::Other
    }
}

fn classify_sheet(m: &str) -> ChangeWhy {
    if [
        "hidden content cannot be addressed",
        "inside the merged range",
        "array formula",
        "shared formula",
    ]
    .iter()
    .any(|k| m.contains(k))
    {
        ChangeWhy::NotEditable
    } else {
        ChangeWhy::Other
    }
}

/// The changes of a request on a file: `sheet` is the place among the
/// visible sheets (a workbook only). `Err` is a reason for the whole file;
/// otherwise one item per change, and the new file when all applied.
pub fn apply(file: &[u8], sheet: Option<u32>, changes: &[Change]) -> Result<Applied, Why> {
    if changes.is_empty() || changes.len() > MAX_PAGE_CHANGES {
        return Err(Why::Damaged);
    }
    // An encrypted Office file is an OLE container, not a zip.
    if file.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Err(Why::Protected);
    }
    if !file.starts_with(b"PK") {
        return Err(Why::NotOffice);
    }
    let src = Source::open(file).map_err(|e| why_of(&e))?;
    if src.has("word/document.xml") {
        document(file, changes)
    } else if src.has(xlsx::WORKBOOK) {
        workbook(&src, file, sheet, changes)
    } else {
        Err(Why::NotOffice)
    }
}

fn document(file: &[u8], changes: &[Change]) -> Result<Applied, Why> {
    let mut items: Vec<Option<Item>> = vec![None; changes.len()];
    let mut ops = vec![];
    let mut at = vec![];
    let mut seen = HashSet::new();
    for (k, c) in changes.iter().enumerate() {
        let Some(key) = paragraph_key(&c.at) else {
            items[k] = Some(refused(ChangeWhy::BadPlace));
            continue;
        };
        if c.text.chars().count() > MAX_EDIT_TEXT_CHARS {
            items[k] = Some(refused(ChangeWhy::TooLong));
        } else if !seen.insert(key.to_string()) {
            items[k] = Some(refused(ChangeWhy::Duplicate));
        } else {
            ops.push(DocOp::SetText {
                at: c.at.clone(),
                text: c.text.clone(),
            });
            at.push(k);
        }
    }
    if ops.is_empty() {
        return Ok(finish(
            items,
            &at,
            super::Outcome {
                results: vec![],
                bytes: None,
                notes: vec![],
            },
            classify_doc,
        ));
    }
    let o = docx::edit(file, &ops).map_err(|e| why_of_edit(&e))?;
    Ok(finish(items, &at, o, classify_doc))
}

/// The name of the visible sheet at `place`, which must be the only sheet of
/// that name (the edit takes a name; two of one name would not say which).
fn visible_name(src: &Source<'_>, place: Option<u32>) -> Result<String, Why> {
    let part = read(src, xlsx::WORKBOOK)?;
    let wb = xlsx::read_wb(src, &part).map_err(|e| why_of(&e))?;
    let visible: Vec<&xlsx::SheetRef> = wb.sheets.iter().filter(|s| s.state == "visible").collect();
    let sh = visible
        .get(place.unwrap_or(0) as usize)
        .ok_or(Why::NoSheet)?;
    let same = |a: &str, b: &str| a == b || a.eq_ignore_ascii_case(b);
    if wb.sheets.iter().filter(|s| same(&s.name, &sh.name)).count() != 1 {
        return Err(Why::Damaged);
    }
    Ok(sh.name.clone())
}

fn workbook(
    src: &Source<'_>,
    file: &[u8],
    sheet: Option<u32>,
    changes: &[Change],
) -> Result<Applied, Why> {
    let name = visible_name(src, sheet)?;
    let mut items: Vec<Option<Item>> = vec![None; changes.len()];
    let mut ops = vec![];
    let mut at = vec![];
    let mut seen = HashSet::new();
    for (k, c) in changes.iter().enumerate() {
        let Some((col, row)) = xlsx::parse_addr(&c.at) else {
            items[k] = Some(refused(ChangeWhy::BadPlace));
            continue;
        };
        let addr = xlsx::addr(col, row);
        match cell_value(&c.text) {
            Err(w) => items[k] = Some(refused(w)),
            Ok(_) if !seen.insert(addr.clone()) => items[k] = Some(refused(ChangeWhy::Duplicate)),
            Ok(v) => {
                ops.push(SheetOp::Set(vec![(addr, v)]));
                at.push(k);
            }
        }
    }
    if ops.is_empty() {
        return Ok(finish(
            items,
            &at,
            super::Outcome {
                results: vec![],
                bytes: None,
                notes: vec![],
            },
            classify_sheet,
        ));
    }
    let o = xlsx::edit(file, &name, &ops).map_err(|e| why_of_edit(&e))?;
    Ok(finish(items, &at, o, classify_sheet))
}

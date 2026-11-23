//! What a cell formula may be (PLAN.md T97, report 3.4 xlsx item 12). A
//! prompt-injected model can write a formula that sends the sheet away when
//! the person opens it, so the formula text is checked before it is written:
//! `WEBSERVICE`, `IMAGE`, `RTD`, the old macro functions (`CALL`, `REGISTER`,
//! `EXEC`, `SQL.REQUEST`), the `CUBE*` family and DDE (`app|topic!item`) are
//! refused, a reference into another workbook (`[book.xlsx]Sheet!A1`, which
//! needs a `[`) is refused, and `HYPERLINK` is allowed only when its first
//! argument starts with the literal `http://` or `https://`.
//!
//! The check reads the formula the way Excel's parser tokenises it: string
//! literals (`"…"`, a doubled quote inside) and quoted sheet names (`'…'`)
//! are opaque, so text in them neither triggers nor hides a call; outside
//! them upper and lower case are one, a function name is any run of letters,
//! digits, `_` and `.` followed by `(` with spaces allowed between, the
//! `_xlfn.`, `_xlws.` and other `_xl…` prefixes newer functions are stored
//! under are removed, and characters Excel cannot see (controls, zero-width
//! and bidirectional marks) are dropped. A name that is not on the list
//! because it is part of a longer identifier is left alone: it is not the
//! built-in.

use crate::Result;
use crate::limits::MAX_FORMULA_CHARS;
use crate::xml;

/// Functions a formula may not call.
const DENIED: &[&str] = &[
    "WEBSERVICE",
    "IMAGE",
    "RTD",
    "CALL",
    "REGISTER",
    "REGISTER.ID",
    "EXEC",
    "SQL.REQUEST",
    "DDE",
    "DDEAUTO",
];

const HYPERLINK_ONLY: &str = "HYPERLINK in a formula needs, as its first argument, one text that is an address starting \"http://\" or \"https://\", with nothing joined to it";

fn invisible(c: char) -> bool {
    c.is_control() || harness_core::text::is_hidden(c) || xml::is_override(c)
}

fn ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// The formula as it is written (without a leading `=`), or the reason it is
/// refused.
pub fn check(formula: &str) -> Result<String> {
    let f = xml::clean(formula);
    let f = f.trim();
    let f = f.strip_prefix('=').unwrap_or(f).trim();
    if f.is_empty() {
        return Err("a formula is empty".into());
    }
    if f.chars().count() > MAX_FORMULA_CHARS {
        return Err(format!(
            "a formula is longer than {MAX_FORMULA_CHARS} characters"
        ));
    }
    // `norm`: what is outside literals and quoted sheet names, upper case,
    // runs of whitespace one space; `"` stands for one string literal and
    // `'` for one quoted name.
    let mut norm = String::with_capacity(f.len());
    let mut lits: Vec<String> = Vec::new();
    let mut it = f.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                let mut cur = String::new();
                loop {
                    match it.next() {
                        None => {
                            return Err("a formula has a text literal that is not closed".into());
                        }
                        Some('"') if it.peek() == Some(&'"') => {
                            it.next();
                            cur.push('"');
                        }
                        Some('"') => break,
                        Some(c) => cur.push(c),
                    }
                }
                lits.push(cur);
                norm.push('"');
            }
            '\'' => {
                loop {
                    match it.next() {
                        None => {
                            return Err(
                                "a formula has a quoted sheet name that is not closed".into()
                            );
                        }
                        Some('\'') if it.peek() == Some(&'\'') => {
                            it.next();
                        }
                        Some('\'') => break,
                        Some('[' | ']') => {
                            return Err(
                                "a formula refers to another workbook (a reference with square brackets); only this workbook's cells can be used".into(),
                            );
                        }
                        // A sheet's name has none of these: a path or an
                        // address in quotes is a workbook outside this one.
                        Some('\\' | '/' | ':') => {
                            return Err(
                                "a formula refers to a path or an address in quotes (another workbook); only this workbook's cells can be used".into(),
                            );
                        }
                        Some(_) => {}
                    }
                }
                norm.push('\'');
            }
            '[' | ']' => {
                return Err(
                    "a formula uses square brackets (another workbook or a table reference); only plain cell references can be used".into(),
                );
            }
            '|' => {
                return Err(
                    "a formula uses `|` (a DDE link to another program), which is not allowed"
                        .into(),
                );
            }
            c if invisible(c) => {}
            c if c.is_whitespace() => {
                if !norm.ends_with(' ') && !norm.is_empty() {
                    norm.push(' ');
                }
            }
            c => norm.push(c.to_ascii_uppercase()),
        }
    }
    // Function names: runs of identifier characters followed by `(`.
    let chars: Vec<char> = norm.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !ident(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && ident(chars[i]) {
            i += 1;
        }
        let mut j = i;
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        let call = chars.get(j) == Some(&'(');
        // `Book1.xlsx!Total`: a file name before `!` is another workbook.
        if chars.get(j) == Some(&'!') && chars[start..i].contains(&'.') {
            return Err(
                "a formula has a name with a dot before `!` (another workbook, `Book.xlsx!Cell`); put a sheet name in quotes, `'My.Sheet'!A1`".into(),
            );
        }
        let mut name: String = chars[start..i].iter().collect();
        // `_xlfn.IMAGE`, `_xlfn._xlws.FILTER`, `_xleta.WEBSERVICE`: the
        // stored form of a newer function or of a function used as a
        // value; what follows the last prefix is the name.
        while name.starts_with("_XL") {
            match name.find('.') {
                Some(p) => name = name[p + 1..].to_string(),
                None => break,
            }
        }
        // Called or handed to another function (`MAP(range, WEBSERVICE)`),
        // the name is refused all the same.
        if DENIED.contains(&name.as_str()) || name.starts_with("CUBE") {
            return Err(format!(
                "a formula uses {name}, which can reach outside the workbook and is not allowed"
            ));
        }
        if name == "HYPERLINK" {
            if !call {
                return Err(HYPERLINK_ONLY.into());
            }
            let mut k = j + 1;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            // The first argument is one text literal, nothing joined to it:
            // what follows it is `,` or `)`.
            let alone = chars.get(k) == Some(&'"') && {
                let mut e = k + 1;
                while e < chars.len() && chars[e] == ' ' {
                    e += 1;
                }
                matches!(chars.get(e), Some(',' | ')'))
            };
            let ok = alone && {
                let n = chars[..k].iter().filter(|c| **c == '"').count();
                let lit = lits.get(n).map(String::as_str).unwrap_or("");
                let lit: String = lit
                    .chars()
                    .filter(|c| !invisible(*c))
                    .collect::<String>()
                    .trim_start()
                    .to_ascii_lowercase();
                lit.starts_with("http://") || lit.starts_with("https://")
            };
            if !ok {
                return Err(HYPERLINK_ONLY.into());
            }
        }
    }
    Ok(f.to_string())
}

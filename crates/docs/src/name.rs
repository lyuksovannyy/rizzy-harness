//! The name of a created file (PLAN.md T97): what the model wrote is cut to
//! one clean segment (the harness's own rules, `clean_line`: controls and
//! hidden characters such as a right-to-left override are gone), characters
//! no common file system takes become `_`, a leading dot or space goes, a
//! device name a Windows machine would refuse gets a `_` in front, and the
//! extension is the tool's, never the model's.

use crate::limits::MAX_NAME_CHARS;

const FORBIDDEN: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\0'];
const DEVICES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// The stem of the name: the last segment of `name`, cleaned, without a
/// trailing `ext` (a dot and the extension, in any case).
pub fn stem(name: &str, ext: &str, fallback: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let line = harness_core::text::clean_line(last, last.len().max(1));
    let mut s: String = line
        .chars()
        .map(|c| if FORBIDDEN.contains(&c) { '_' } else { c })
        .collect();
    let dot = format!(".{ext}");
    // A name that already ends in the extension (any case, repeated) keeps
    // one: the extension is added below.
    while s.len() > dot.len() && s.to_ascii_lowercase().ends_with(&dot) {
        let cut = s.len() - dot.len();
        s.truncate(cut);
    }
    let s = s.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let s: String = s.chars().take(MAX_NAME_CHARS).collect();
    let s = s.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if s.is_empty() || s == ext {
        return fallback.into();
    }
    let lower = s.to_ascii_lowercase();
    let base = lower.split('.').next().unwrap_or("");
    if DEVICES.contains(&base) {
        return format!("_{s}");
    }
    s.to_string()
}

/// `stem` with `.ext` after it.
pub fn file_name(name: &str, ext: &str, fallback: &str) -> String {
    format!("{}.{ext}", stem(name, ext, fallback))
}

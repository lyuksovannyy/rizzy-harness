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

/// The suffix a saved copy's name carries.
pub const EDITED: &str = " (edited)";

/// The stem of a copy of `name`: its own stem cut to leave room for
/// ` (edited)` inside [`MAX_NAME_CHARS`], then the suffix, so the suffix always
/// survives and the copy never carries the original's name.
pub fn edited_stem(name: &str, ext: &str) -> String {
    let own = stem(name, ext, "edited");
    let room = MAX_NAME_CHARS - EDITED.chars().count();
    // A file system takes 255 bytes in a name: the suffix, the extension and
    // room for ` (999)` come out of it, so a stem of wide characters is cut
    // by bytes too.
    let mut bytes = 255 - EDITED.len() - ext.len() - ".".len() - " (999)".len();
    let cut: String = own
        .chars()
        .take(room)
        .take_while(|c| {
            bytes = bytes.saturating_sub(c.len_utf8());
            bytes > 0 || c.len_utf8() == 0
        })
        .collect();
    let cut = cut.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    let cut = if cut.is_empty() { "edited" } else { cut };
    format!("{cut}{EDITED}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suffix_always_survives_and_the_copy_is_never_the_original() {
        for n in [1usize, 50, 90, 91, 92, 99, 100, 101, 120, 300] {
            let name = format!("{}.docx", "a".repeat(n));
            let s = edited_stem(&name, "docx");
            assert!(s.ends_with(" (edited)"), "{n}: {s}");
            assert!(
                s.chars().count() <= MAX_NAME_CHARS,
                "{n}: {}",
                s.chars().count()
            );
            assert_ne!(s, stem(&name, "docx", "x"), "{n}");
            if n <= 91 {
                assert_eq!(s, format!("{} (edited)", "a".repeat(n)));
            } else {
                assert_eq!(s.chars().count(), MAX_NAME_CHARS, "{n}");
            }
        }
        // Many bytes per character: counted in characters, cut on a boundary.
        let name = format!("{}.xlsx", "é日".repeat(60));
        let s = edited_stem(&name, "xlsx");
        assert!(
            s.ends_with(" (edited)") && s.chars().count() == MAX_NAME_CHARS,
            "{s}"
        );
        // Wide characters: also within what a file system takes in a name.
        let name = format!("{}.docx", "日".repeat(90));
        let s = edited_stem(&name, "docx");
        assert!(s.ends_with(" (edited)"), "{s}");
        assert!(format!("{s} (999).docx").len() <= 255, "{}", s.len());
        // A stem that ends in a dot or a space after the cut loses it.
        let name = format!("{} .docx", "b".repeat(90));
        assert_eq!(
            edited_stem(&name, "docx"),
            format!("{} (edited)", "b".repeat(90))
        );
        let name = format!("{}. x.docx", "c".repeat(90));
        assert!(
            !edited_stem(&name, "docx").contains(". ("),
            "{}",
            edited_stem(&name, "docx")
        );
        // Nothing left: the fallback.
        assert_eq!(edited_stem(".docx", "docx"), "edited (edited)");
    }
}

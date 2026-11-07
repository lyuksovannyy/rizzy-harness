//! Cleaning text that came from outside (web pages, search engines, the
//! model) before the UI shows it as a label.
//!
//! The hidden set matches `web/src/md/hidden.ts`: bidi controls, zero-width
//! characters, the BOM, private-use characters, and the default-ignorable
//! code points that no script needs (the deprecated format characters, the
//! musical beam controls, the unassigned specials and the whole tag and
//! variation-selector plane). They hide or reorder text, so what the user
//! reads would differ from what is there; tag characters and variation
//! selectors are the usual carriers for invisible prompt-injection text.
//! The other default-ignorable code points (soft hyphen, Hangul fillers,
//! the Mongolian and BMP variation selectors, …) shape real text and stay.

/// True for a character of the hidden set.
pub fn is_hidden(c: char) -> bool {
    matches!(c,
        '\u{061C}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF0}'..='\u{FFF8}'
        | '\u{E000}'..='\u{F8FF}'
        | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0000}'..='\u{E0FFF}')
}

/// One line of plain text: hidden characters and controls dropped, every run
/// of whitespace (newlines included) turned into one space, trimmed, and cut
/// to `max` characters (Unicode scalar values).
pub fn clean_line(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max * 4));
    let mut space = false;
    let mut n = 0;
    for c in s.chars() {
        if is_hidden(c) {
            continue;
        }
        if c.is_whitespace() || c.is_control() {
            // Controls count as a gap, so "a\u{0}b" cannot join two words
            // into one that reads differently.
            space = !out.is_empty();
            continue;
        }
        if n + usize::from(space) + 1 > max {
            break;
        }
        if space {
            out.push(' ');
            n += 1;
            space = false;
        }
        out.push(c);
        n += 1;
    }
    out
}

/// Model text for a `ui:` block (a question, a label, help text), before
/// its length is checked: hidden characters dropped, controls and every
/// whitespace character other than a kept newline read as a space. A
/// single-line string is [`clean_line`] (whitespace collapsed); a multi-line
/// one keeps its newlines (`\r\n` and `\r` count as one) as written.
/// Trimmed either way.
pub fn clean_text(s: &str, multiline: bool) -> String {
    if !multiline {
        // A string has no more characters than bytes, so nothing is cut.
        return clean_line(s, s.len());
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if is_hidden(c) => {}
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' | '\n' => out.push('\n'),
            c if c.is_whitespace() || c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_strips_the_hidden_set_and_keeps_newlines_only_when_asked() {
        let s = " Pick\u{202E} one\u{2066}\u{200B}\u{E000}\u{061C}\u{E0041}\u{E0100}\n\tnow\u{7} ";
        assert_eq!(clean_text(s, false), "Pick one now");
        assert_eq!(clean_text(s, true), "Pick one\n now");
        assert_eq!(clean_text("a\r\nb\rc\u{85}d\u{2028}e", true), "a\nb\nc d e");
        assert_eq!(clean_text("  \u{200B} \n ", true), "");
        assert_eq!(clean_text(&"é".repeat(50), false).chars().count(), 50);
    }

    #[test]
    fn strips_hidden_and_controls_and_collapses_whitespace() {
        assert_eq!(
            clean_line(
                "  a\u{202E}b\u{2066}c\u{200B}d\u{E000}e \n\t f\u{7}g  ",
                100
            ),
            "abcde f g"
        );
        assert_eq!(clean_line("tag\u{E0041}\u{E0100}s", 100), "tags");
        // The rest of the default-ignorable code points that draw nothing.
        assert_eq!(
            clean_line(
                "a\u{2065}\u{206A}\u{206F}\u{FFF0}\u{FFF8}\u{1D173}\u{1D17A}\u{E0080}\u{E01F0}\u{E0FFF}b",
                100
            ),
            "ab"
        );
        assert_eq!(clean_line("\u{202E}\u{200B}", 100), "");
    }

    #[test]
    fn cuts_to_the_limit_in_characters() {
        assert_eq!(clean_line("ééééé", 3), "ééé");
        assert_eq!(clean_line("ab cd", 3), "ab");
        assert_eq!(clean_line("ab cd", 4), "ab c");
        assert_eq!(clean_line("abc", 0), "");
    }
}

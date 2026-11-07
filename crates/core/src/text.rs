//! Cleaning text that came from outside (web pages, search engines, the
//! model) before the UI shows it as a label.
//!
//! The hidden set matches `web/src/md/hidden.ts`: bidi controls, zero-width
//! characters, the BOM, private-use characters, tag characters and the
//! supplementary variation selectors. They hide or reorder text, so what the
//! user reads would differ from what is there; tag characters and variation
//! selectors are the usual carriers for invisible prompt-injection text.

/// True for a character of the hidden set.
pub fn is_hidden(c: char) -> bool {
    matches!(c,
        '\u{061C}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{FEFF}'
        | '\u{E000}'..='\u{F8FF}'
        | '\u{E0000}'..='\u{E007F}'
        | '\u{E0100}'..='\u{E01EF}')
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

#[cfg(test)]
mod tests {
    use super::*;

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

//! Finding the `ui:` fences of a message (rich_plan §4.1).
//!
//! The browser parses the same text with micromark. It treats a root-level
//! code node as a block only when its opener line is one [`scan`] accepts,
//! and then shows our spec only if ordinal, hash and type all match. Where
//! the two parsers still disagree (an HTML block that swallows a fence, a
//! fence inside a list item), that match fails and the fence stays code.

use crate::event::UiKind;
use crate::text::is_hidden;

/// Only the first this many terminated ui fences become blocks.
pub const MAX_BLOCKS: usize = 8;

/// A terminated `ui:<type>` fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// Index among the message's ui fences, from 0.
    pub ordinal: u32,
    pub kind: UiKind,
    /// The lines between opener and closer, joined with `\n`.
    pub body: String,
}

impl Fence {
    /// What ties the block to its fence in the browser (`md/fnv.ts`).
    pub fn hash(&self) -> String {
        format!("{:08x}", fnv1a32(self.body.as_bytes()))
    }
}

/// FNV-1a, 32 bits. Not a security hash: it only pairs a stored block with
/// the fence it came from, and a collision can only show a block the same
/// message validated anyway.
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h: u32, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

/// The text as the browser parses it: the hidden set stripped (`prepare()`
/// in `md/pipeline.ts` strips it first), then `\r\n` and `\r` as `\n`, and
/// NUL as U+FFFD (micromark replaces it).
pub fn normalize(text: &str) -> String {
    let visible: String = text.chars().filter(|c| !is_hidden(*c)).collect();
    let mut out = String::with_capacity(visible.len());
    let mut chars = visible.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => out.push('\n'),
            '\0' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

/// The terminated ui fences of `text` (normalized first), at most
/// [`MAX_BLOCKS`]. Ordinary fences are skipped to their closer, so a ui
/// fence shown inside another fence is not a block. An unterminated fence
/// runs to the end of the message in Markdown, so scanning stops there.
pub fn scan(text: &str) -> Vec<Fence> {
    let text = normalize(text);
    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    let mut body: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        let Some(o) = &open else {
            open = opener(line);
            body.clear();
            continue;
        };
        if !closes(line, o) {
            body.push(line);
            continue;
        }
        if let Some(kind) = o.ui {
            out.push(Fence {
                ordinal: out.len() as u32,
                kind,
                body: body.join("\n"),
            });
            if out.len() == MAX_BLOCKS {
                break;
            }
        }
        open = None;
    }
    out
}

struct Open {
    ch: char,
    len: usize,
    /// Set for a ui opener.
    ui: Option<UiKind>,
}

/// A fence opener: at most 3 columns of indent, 3 or more backticks or
/// tildes, then an info string (which may not hold a backtick after
/// backticks: that line is inline code). It opens a ui block only at
/// column 0 and with an info string of exactly `ui:<known type>`, spaces
/// and tabs around it allowed: anything else (meta, an unknown type) is an
/// ordinary fence, as in `md/remark-ui.ts`.
fn opener(line: &str) -> Option<Open> {
    let (indent, rest) = indent(line);
    let ch = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = rest.chars().take_while(|c| *c == ch).count();
    if indent > 3 || len < 3 {
        return None;
    }
    let info = &rest[len..];
    if ch == '`' && info.contains('`') {
        return None;
    }
    let ui = (rest.len() == line.len())
        .then(|| info.trim_matches([' ', '\t']).strip_prefix("ui:"))
        .flatten()
        .and_then(|t| UiKind::ALL.into_iter().find(|k| k.as_str() == t));
    Some(Open { ch, len, ui })
}

/// A closer: at most 3 columns of indent, a run of the opener's character
/// at least as long, then only spaces and tabs.
fn closes(line: &str, open: &Open) -> bool {
    let (indent, rest) = indent(line);
    let len = rest.chars().take_while(|c| *c == open.ch).count();
    indent <= 3 && len >= open.len && rest[len..].chars().all(|c| c == ' ' || c == '\t')
}

/// Leading indent in columns (a tab goes to the next multiple of 4), and
/// the rest of the line.
fn indent(line: &str) -> (usize, &str) {
    let mut cols = 0;
    for (i, c) in line.char_indices() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4 - cols % 4,
            _ => return (cols, &line[i..]),
        }
    }
    (cols, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(u32, &'static str, String)> {
        scan(text)
            .into_iter()
            .map(|f| (f.ordinal, f.kind.as_str(), f.body))
            .collect()
    }

    fn one(kind: &'static str, body: &str) -> Vec<(u32, &'static str, String)> {
        vec![(0, kind, body.to_string())]
    }

    #[test]
    fn backtick_and_tilde_fences_and_longer_closers() {
        assert_eq!(
            found("Hi\n```ui:choices\n{}\n```\nbye"),
            one("choices", "{}")
        );
        assert_eq!(found("~~~ui:form\na\nb\n~~~~~"), one("form", "a\nb"));
        assert_eq!(found("````ui:chart\n```\nx\n````"), one("chart", "```\nx"));
        // A closer may be indented up to 3 columns and end in spaces or tabs.
        assert_eq!(found("```ui:progress\nx\n   ```  \t"), one("progress", "x"));
        // Spaces and tabs around the info string are allowed (micromark too).
        assert_eq!(found("``` ui:progress \t\nx\n```"), one("progress", "x"));
        // Blank lines and an empty body are kept as they are.
        assert_eq!(found("```ui:form\n\nx\n\n```"), one("form", "\nx\n"));
        assert_eq!(found("```ui:form\n```"), one("form", ""));
    }

    #[test]
    fn not_closers() {
        // Shorter run, other character, 4 columns of indent, text after.
        assert_eq!(found("````ui:form\n```\n````"), one("form", "```"));
        assert_eq!(found("```ui:form\n~~~\n```"), one("form", "~~~"));
        assert_eq!(
            found("```ui:form\n    ```\n\t```\n```"),
            one("form", "    ```\n\t```")
        );
        assert_eq!(found("```ui:form\n``` x\n```"), one("form", "``` x"));
    }

    #[test]
    fn unterminated_fences_end_the_scan() {
        assert_eq!(found("```ui:choices\n{}"), vec![]);
        assert_eq!(
            found("```ui:choices\n{}\n```\n```ui:form\n{}"),
            one("choices", "{}")
        );
        // An unterminated ordinary fence hides everything after it.
        assert_eq!(found("````rust\nfn x\n```ui:form\n{}\n```"), vec![]);
    }

    #[test]
    fn indented_list_and_quote_fences_are_not_blocks() {
        for text in [
            " ```ui:choices\n{}\n```",
            "   ```ui:choices\n{}\n```",
            "    ```ui:choices\n{}\n```",
            "\t```ui:choices\n{}\n```",
            "- ```ui:choices\n  {}\n  ```",
            "1. ```ui:choices\n   {}\n   ```",
            "> ```ui:choices\n> {}\n> ```",
        ] {
            assert_eq!(found(text), vec![], "{text:?}");
        }
    }

    #[test]
    fn ui_fences_inside_other_fences_are_ignored() {
        let text = "````md\n```ui:choices\n{}\n```\n````\n~~~\n```ui:form\n{}\n```\n~~~";
        assert_eq!(found(text), vec![]);
        // After the outer fence closes, scanning goes on.
        let text = "```\n```ui:form\n```\n```ui:progress\nx\n```";
        assert_eq!(found(text), one("progress", "x"));
    }

    #[test]
    fn unknown_types_and_meta_make_ordinary_fences() {
        for text in [
            "```ui:widget\n{}\n```",
            "```ui:choices x\n{}\n```",
            "```ui:Choices\n{}\n```",
            "```ui: choices\n{}\n```",
            "```uichoices\n{}\n```",
            "``ui:choices\n{}\n``",
        ] {
            assert_eq!(found(text), vec![], "{text:?}");
        }
        // An ordinary fence is skipped as a whole: the ui fence written
        // inside it is not one.
        assert_eq!(found("```ui:widget\n```ui:form\n{}\n```\n"), vec![]);
        // A backtick line with a backtick after it is inline code, no fence.
        assert_eq!(
            found("```ui:choices`\n```ui:form\nx\n```"),
            one("form", "x")
        );
    }

    #[test]
    fn crlf_cr_nul_and_hidden_characters_are_normalized_first() {
        assert_eq!(
            found("```ui:form\r\na\r\nb\r\n```\r\n"),
            one("form", "a\nb")
        );
        assert_eq!(found("```ui:form\ra\r```"), one("form", "a"));
        assert_eq!(found("```ui:form\na\0b\n```"), one("form", "a\u{FFFD}b"));
        // The browser strips the hidden set before parsing, so the opener
        // reads `ui:form` there too and the body hashes without it.
        assert_eq!(
            found("```ui:\u{200B}form\u{202E}\na\u{2066}b\r\u{200B}\n```"),
            one("form", "ab")
        );
    }

    #[test]
    fn at_most_eight_blocks_with_ordinals() {
        let text: String = (0..10)
            .map(|i| format!("```ui:progress\n{i}\n```\n```js\nx\n```\n"))
            .collect();
        let got = found(&text);
        assert_eq!(got.len(), MAX_BLOCKS);
        for (i, (ordinal, _, body)) in got.iter().enumerate() {
            assert_eq!(
                (*ordinal as usize, body.as_str()),
                (i, i.to_string().as_str())
            );
        }
    }

    #[test]
    fn fnv_golden_vectors() {
        // From the FNV reference test suite.
        assert_eq!(fnv1a32(b""), 0x811c9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9cf968);
        let f = Fence {
            ordinal: 0,
            kind: UiKind::Form,
            body: "a".into(),
        };
        assert_eq!(f.hash(), "e40c292c");
        let f = Fence {
            body: String::new(),
            ..f
        };
        assert_eq!(f.hash(), "811c9dc5");
    }
}

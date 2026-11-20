//! A page read as plain text with no tree: for a page too large to hand to a
//! tree-building parser. Linear in the page, whatever its structure, so a
//! hostile page cannot make it cost more than reading the bytes.
//!
//! It shows less than the normal path, never more: tags and the content of
//! `script`, `style`, `noscript`, `iframe`, `noembed`, `noframes`, `xmp`,
//! `template` and comments are dropped (the normal path's text reader shows
//! the content of `noscript` and `textarea`, since its parser, like a browser
//! with scripting off, reads them as markup and text).

fn is_ws(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn lower(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_ascii_lowercase()
}

/// From `i` (just after a tag's name) to just after its `>`, following the
/// tokenizer's attribute states; and whether it ended with `/>`.
fn tag_rest(h: &[u8], mut i: usize) -> (usize, bool) {
    let n = h.len();
    loop {
        while i < n && is_ws(h[i]) {
            i += 1;
        }
        if i >= n {
            return (n, false);
        }
        match h[i] {
            b'>' => return (i + 1, false),
            b'/' => {
                i += 1;
                if i < n && h[i] == b'>' {
                    return (i + 1, true);
                }
                continue;
            }
            _ => {}
        }
        // An attribute name (its first character may be `=`).
        i += 1;
        while i < n && !is_ws(h[i]) && !matches!(h[i], b'/' | b'>' | b'=') {
            i += 1;
        }
        while i < n && is_ws(h[i]) {
            i += 1;
        }
        if i >= n {
            return (n, false);
        }
        if h[i] == b'=' {
            i += 1;
            while i < n && is_ws(h[i]) {
                i += 1;
            }
            if i >= n {
                return (n, false);
            }
            match h[i] {
                q @ (b'"' | b'\'') => {
                    i += 1;
                    while i < n && h[i] != q {
                        i += 1;
                    }
                    if i >= n {
                        return (n, false);
                    }
                    i += 1;
                }
                b'>' => return (i + 1, false),
                _ => {
                    while i < n && !is_ws(h[i]) && h[i] != b'>' {
                        i += 1;
                    }
                }
            }
        }
    }
}

/// The index after `<!--` ... `-->` (or the tokenizer's shorter endings).
fn comment_end(h: &[u8], i: usize) -> usize {
    // `i` is just after `<!--`.
    if h[i..].starts_with(b">") {
        return i + 1;
    }
    if h[i..].starts_with(b"->") {
        return i + 2;
    }
    let mut j = i;
    while j + 2 < h.len() {
        if h[j] == b'-' && h[j + 1] == b'-' {
            if h[j + 2] == b'>' {
                return j + 3;
            }
            if h[j + 2] == b'!' && h.get(j + 3) == Some(&b'>') {
                return j + 4;
            }
        }
        j += 1;
    }
    h.len()
}

fn after_gt(h: &[u8], i: usize) -> usize {
    h[i..]
        .iter()
        .position(|&b| b == b'>')
        .map_or(h.len(), |p| i + p + 1)
}

/// The index of the end tag `</name` that closes a raw-text element whose
/// start tag ended at `i`, and whether the text held `<!--`.
fn raw_end(h: &[u8], i: usize, name: &str) -> (usize, bool) {
    let n = h.len();
    let pat = name.as_bytes();
    let mut j = i;
    let mut comment = false;
    while j < n {
        if h[j] == b'<' {
            if h[j..].starts_with(b"<!--") {
                comment = true;
            }
            if h.get(j + 1) == Some(&b'/')
                && h.len() >= j + 2 + pat.len()
                && h[j + 2..j + 2 + pat.len()].eq_ignore_ascii_case(pat)
                && h.get(j + 2 + pat.len())
                    .is_none_or(|&c| is_ws(c) || c == b'/' || c == b'>')
            {
                return (j, comment);
            }
        }
        j += 1;
    }
    (n, comment)
}

fn read_name(h: &[u8], mut i: usize) -> (String, usize) {
    let s = i;
    while i < h.len() && !is_ws(h[i]) && !matches!(h[i], b'/' | b'>') {
        i += 1;
    }
    (lower(&h[s..i]), i)
}

/// The page's text with no tree: tags dropped, the content of scripts and
/// styles dropped, block boundaries as newlines, entities decoded, runs of
/// blanks collapsed. Linear in the page, whatever its structure.
pub fn plain_text(h: &[u8]) -> String {
    let n = h.len();
    let mut out: Vec<u8> = Vec::with_capacity(n.min(1 << 20));
    let mut i = 0;
    while i < n {
        if h[i] != b'<' {
            out.push(h[i]);
            i += 1;
            continue;
        }
        let Some(&c) = h.get(i + 1) else { break };
        if c.is_ascii_alphabetic() {
            let (name, after_name) = read_name(h, i + 1);
            let (end, _) = tag_rest(h, after_name);
            i = end;
            if name == "plaintext" {
                break;
            }
            if matches!(
                name.as_str(),
                "script" | "style" | "noscript" | "iframe" | "noembed" | "noframes" | "xmp"
            ) {
                i = raw_end(h, i, &name).0;
                continue;
            }
            if name == "template" {
                i = template_end(h, i);
                continue;
            }
            if matches!(
                name.as_str(),
                "p" | "div"
                    | "br"
                    | "li"
                    | "tr"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "ul"
                    | "ol"
                    | "table"
                    | "section"
                    | "article"
                    | "header"
                    | "footer"
                    | "blockquote"
                    | "pre"
                    | "hr"
                    | "dt"
                    | "dd"
                    | "title"
            ) {
                out.push(b'\n');
            } else {
                out.push(b' ');
            }
        } else if c == b'/' {
            match h.get(i + 2) {
                Some(d) if d.is_ascii_alphabetic() => {
                    let (name, after_name) = read_name(h, i + 2);
                    i = tag_rest(h, after_name).0;
                    out.push(
                        if matches!(
                            name.as_str(),
                            "p" | "div"
                                | "li"
                                | "tr"
                                | "td"
                                | "th"
                                | "h1"
                                | "h2"
                                | "h3"
                                | "h4"
                                | "h5"
                                | "h6"
                                | "ul"
                                | "ol"
                                | "table"
                                | "blockquote"
                                | "pre"
                        ) {
                            b'\n'
                        } else {
                            b' '
                        },
                    );
                }
                Some(b'>') => i += 3,
                Some(_) => i = after_gt(h, i + 2),
                None => break,
            }
        } else if c == b'!' {
            if h[i + 2..].starts_with(b"--") {
                i = comment_end(h, i + 4);
            } else {
                i = after_gt(h, i + 2);
            }
        } else if c == b'?' {
            i = after_gt(h, i + 2);
        } else {
            out.push(b'<');
            i += 1;
        }
    }
    let text = crate::sources::decode_entities(&String::from_utf8_lossy(&out));
    // Collapse blanks, keep at most one blank line.
    let mut s = String::with_capacity(text.len());
    let mut nl = 0;
    for line in text.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            nl += 1;
            continue;
        }
        if !s.is_empty() {
            s.push('\n');
            if nl > 0 {
                s.push('\n');
            }
        }
        nl = 0;
        s.push_str(&l);
    }
    s
}

/// The index after the `</template>` that closes a template whose start tag
/// ended at `i` (templates nest), or the end of the page.
fn template_end(h: &[u8], i: usize) -> usize {
    let mut depth = 1usize;
    let mut j = i;
    while j < h.len() {
        if h[j] == b'<' {
            let rest = &h[j + 1..];
            let (closing, name) = match rest.first() {
                Some(b'/') => (true, &rest[1..]),
                _ => (false, rest),
            };
            if name.len() >= 8
                && name[..8].eq_ignore_ascii_case(b"template")
                && name
                    .get(8)
                    .is_none_or(|&c| is_ws(c) || c == b'/' || c == b'>')
            {
                if closing {
                    depth -= 1;
                    if depth == 0 {
                        return after_gt(h, j + 2);
                    }
                } else {
                    depth += 1;
                }
            }
        }
        j += 1;
    }
    h.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_reads_the_words_and_nothing_else() {
        let t = plain_text(
            b"<html><head><title>T</title><style>p{}</style></head><body><h1>Head</h1>\
              <p>one &amp; two<script>var s = '<b>';</script></p><!-- hid --><p>three</p></body></html>",
        );
        assert!(
            t.contains("Head") && t.contains("one & two") && t.contains("three"),
            "{t}"
        );
        assert!(
            !t.contains("var s") && !t.contains("hid") && !t.contains("p{}"),
            "{t}"
        );
        // Linear: a deep page costs nothing.
        let d = plain_text(
            &format!("<html><body>{}x</body></html>", "<div>".repeat(400_000)).into_bytes(),
        );
        assert!(d.contains('x'));
    }

    /// It shows no more than the normal reader: a template's content (and a
    /// nested template's) is dropped.
    #[test]
    fn a_template_is_not_shown() {
        let t = plain_text(
            b"<p>seen</p><template>HIDDEN1<template>HIDDEN2</template>HIDDEN3</template><p>after</p>",
        );
        assert!(t.contains("seen") && t.contains("after"), "{t}");
        assert!(!t.contains("HIDDEN"), "{t}");
    }
}

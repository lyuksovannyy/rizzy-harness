//! A linear scan of an HTML page's markup, before any parser sees it.
//!
//! The parsers behind `web_fetch` (`dom_smoothie`, `html2text`) cost time
//! quadratic in the nesting depth of the page and recurse once per level:
//! 20,000 nested `<div>` (100 KB) took 42 s in release and 10,000 overflowed
//! the stack in a debug build. A hostile page must not be able to do that, so
//! [`check`] reads the bytes once and refuses a page whose elements nest
//! deeper than [`MAX_NESTING`], and [`plain_text`] reads a page that should
//! not go to a DOM parser (too large, or built to confuse this scan) as text,
//! with no tree at all.
//!
//! The scan must not count **fewer** levels than the parser builds, so it
//! follows the HTML tokenizer where that matters and is conservative where it
//! does not:
//! * tags end where the tokenizer ends them (attribute values are skipped
//!   with the tokenizer's own rules, so a `>` or an end tag inside a quoted
//!   value is not a tag);
//! * comments, bogus comments (`<!x>`, `<?x>`, `</ x>`) and the raw-text
//!   elements (`script`, `style`, `textarea`, `title`, `xmp`, `iframe`,
//!   `noembed`, `noframes`, `noscript`) are skipped the way the tokenizer
//!   skips them, outside SVG and MathML (inside them nothing is skipped, which
//!   can only count more);
//! * an end tag pops only an element that is on top of the stack, past the
//!   elements whose end tags are optional (the parser may pop more, never
//!   fewer);
//! * a script that holds `<!--` (its "double escaped" state moves the end of
//!   the script) makes the page [`Verdict::Tricky`]: it goes to the plain-text
//!   reader, never to a DOM parser;
//! * a self-closing `/>` pops only inside SVG or MathML, which the scan leaves
//!   as soon as a tag that breaks out of foreign content appears (what the
//!   parser does too).
//!
//! Optional end tags (`<p>`, `<li>`, `<td>`...) replace an open one of their
//! kind on top of the stack instead of nesting, so an old page of unclosed
//! paragraphs is not deep. Where the scan counts more than the parser (an
//! unclosed `<a>`, a `<p>` before a block), the page is refused earlier than
//! it needed to be; that direction is safe.

/// The deepest element nesting a page may have to be read. Chromium's HTML
/// parser caps its tree at 512 levels (`kMaximumHTMLParserDOMTreeDepth`), so
/// no page that renders in a browser nests deeper in the way that counts;
/// real pages nest a few dozen to a few hundred levels (not measured on a
/// corpus here: there is none offline). At this depth the slowest parse of
/// the two (readability, 500 nested `<div>`) took 27 ms in release.
pub const MAX_NESTING: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within [`MAX_NESTING`] and built the usual way: a DOM parser may read it.
    Ok,
    /// Nested deeper than [`MAX_NESTING`].
    TooDeep,
    /// Built so that this scan may disagree with the parser: read it as text.
    Tricky,
}

const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "image", "img",
    "input", "keygen", "link", "meta", "param", "source", "track", "wbr",
];
const RAW: &[&str] = &[
    "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes", "noscript",
];
/// Elements the parser ends by itself when the end tag of one below them
/// comes ("generate implied end tags").
const OPTIONAL_END: &[&str] = &["p", "li", "dd", "dt", "option", "optgroup", "rt", "rp"];
/// Start tags that leave SVG and MathML (the tokenizer's "breakout" list).
const BREAKOUT: &[&str] = &[
    "b",
    "big",
    "blockquote",
    "body",
    "br",
    "center",
    "code",
    "dd",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "hr",
    "i",
    "img",
    "li",
    "listing",
    "menu",
    "meta",
    "nobr",
    "ol",
    "p",
    "pre",
    "ruby",
    "s",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tt",
    "u",
    "ul",
    "var",
];

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

/// Replace an open element of the same kind on top of the stack (optional
/// end tags) before the new one is pushed.
fn implied_pops(stack: &mut Vec<String>, name: &str) {
    let closes: &[&str] = match name {
        "p" => &["p"],
        "li" => &["li"],
        "dt" | "dd" => &["dt", "dd"],
        "option" => &["option"],
        "optgroup" => &["option", "optgroup"],
        "td" | "th" => &["td", "th"],
        "tr" => &["td", "th", "tr"],
        "thead" | "tbody" | "tfoot" => &["td", "th", "tr", "thead", "tbody", "tfoot"],
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => &["h1", "h2", "h3", "h4", "h5", "h6"],
        _ => return,
    };
    while stack.last().is_some_and(|t| closes.contains(&t.as_str())) {
        stack.pop();
    }
}

/// Read the page's markup once and say whether a DOM parser may have it.
pub fn check(h: &[u8]) -> Verdict {
    let n = h.len();
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();
    let mut foreign = false;
    let mut tricky = false;
    while i < n {
        if h[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(&c) = h.get(i + 1) else { break };
        if c.is_ascii_alphabetic() {
            let (name, after_name) = read_name(h, i + 1);
            let (end, self_closing) = tag_rest(h, after_name);
            i = end;
            if foreign && BREAKOUT.contains(&name.as_str()) {
                foreign = false;
            }
            if !foreign {
                if name == "plaintext" {
                    break;
                }
                if RAW.contains(&name.as_str()) {
                    let (e, comment) = raw_end(h, i, &name);
                    if comment && name == "script" {
                        tricky = true;
                    }
                    i = e;
                    continue;
                }
                if VOID.contains(&name.as_str()) {
                    continue;
                }
            }
            if self_closing && foreign {
                continue;
            }
            implied_pops(&mut stack, &name);
            if name == "svg" || name == "math" {
                foreign = true;
            }
            stack.push(name);
            if stack.len() > MAX_NESTING {
                return Verdict::TooDeep;
            }
        } else if c == b'/' {
            match h.get(i + 2) {
                Some(d) if d.is_ascii_alphabetic() => {
                    let (name, after_name) = read_name(h, i + 2);
                    let (end, _) = tag_rest(h, after_name);
                    i = end;
                    if name == "svg" || name == "math" {
                        if let Some(p) = stack.iter().rposition(|t| *t == name) {
                            stack.truncate(p);
                            foreign = stack.iter().any(|t| t == "svg" || t == "math");
                        }
                    } else {
                        // The parser ends the elements whose end tags are
                        // optional (`<p>`, `<li>`...) before the one named;
                        // it never reaches through any other element that
                        // the scan could not be sure it may pass.
                        let mut k = stack.len();
                        while k > 0 && OPTIONAL_END.contains(&stack[k - 1].as_str()) {
                            k -= 1;
                        }
                        if k > 0 && stack[k - 1] == name {
                            stack.truncate(k - 1);
                        }
                    }
                }
                Some(b'>') => i += 3,
                Some(_) => i = after_gt(h, i + 2),
                None => break,
            }
        } else if c == b'!' {
            if h[i + 2..].starts_with(b"--") {
                i = comment_end(h, i + 4);
            } else if foreign && h[i + 2..].starts_with(b"[CDATA[") {
                let from = i + 9;
                i = h[from.min(n)..]
                    .windows(3)
                    .position(|w| w == b"]]>")
                    .map_or(n, |p| from + p + 3);
            } else {
                i = after_gt(h, i + 2);
            }
        } else if c == b'?' {
            i = after_gt(h, i + 2);
        } else {
            i += 1;
        }
    }
    if tricky { Verdict::Tricky } else { Verdict::Ok }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn deep(tag: &str, n: usize) -> Vec<u8> {
        format!("<html><body>{}x</body></html>", tag.repeat(n)).into_bytes()
    }

    #[test]
    fn ordinary_pages_pass_and_deep_ones_do_not() {
        assert_eq!(check(&deep("<div>", 100)), Verdict::Ok);
        assert_eq!(check(&deep("<div>", MAX_NESTING - 2)), Verdict::Ok);
        for tag in [
            "<div>",
            "<span>",
            "<b>",
            "<blockquote>",
            "<ul><li>",
            "<a>",
            "<svg><g>",
        ] {
            assert_eq!(check(&deep(tag, 20_000)), Verdict::TooDeep, "{tag}");
        }
    }

    #[test]
    fn closed_and_optional_end_tag_pages_are_not_deep() {
        let flat: String = (0..50_000).map(|i| format!("<div><p>{i}</div>")).collect();
        assert_eq!(check(flat.as_bytes()), Verdict::Ok);
        for unit in [
            "<p>para ",
            "<li>item ",
            "<tr><td>a<td>b ",
            "<dt>t<dd>d ",
            "<option>o ",
        ] {
            let old: String = format!("<ul><table>{}", unit.repeat(50_000));
            assert_eq!(check(old.as_bytes()), Verdict::Ok, "unclosed {unit}");
        }
        let icons: String = format!("<svg>{}</svg>", "<path d='M0'/>".repeat(5_000));
        assert_eq!(
            check(icons.as_bytes()),
            Verdict::Ok,
            "self-closing svg shapes"
        );
    }

    /// Each way to make the scan see fewer levels than a parser builds: a
    /// closer or an opener hidden where the tokenizer is not in tag state.
    #[test]
    fn hiding_levels_from_the_scan_does_not_work() {
        let n = 5_000;
        let cases: Vec<(&str, String)> = vec![
            (
                "closer in an attribute value",
                "<div><br t=\"> </div> \">".repeat(n),
            ),
            ("closer in a comment", "<div><!-- </div> -->".repeat(n)),
            ("closer in a bogus comment", "<div><!x </div> >".repeat(n)),
            (
                "closer in a script",
                "<div><script></div></script>".repeat(n),
            ),
            (
                "closer in noscript",
                "<div><noscript></div></noscript>".repeat(n),
            ),
            (
                "closer in a textarea",
                "<div><textarea></div></textarea>".repeat(n),
            ),
            (
                "self-closing after a breakout",
                format!("<svg><div>{}", "<span/>".repeat(n)),
            ),
            ("cdata", "<div><![CDATA[ </div> ]]>".repeat(n)),
        ];
        for (what, html) in cases {
            let v = check(html.as_bytes());
            // Either the scan sees the depth, or it hands the page to the
            // text reader; it never says Ok for a page that nests deeper.
            assert!(v != Verdict::Ok, "{what}: {v:?}");
        }
    }

    #[test]
    fn a_script_with_a_comment_goes_to_the_text_reader() {
        let h = b"<div><script><!-- <script> </script> </div> --></script></div>";
        assert_eq!(check(h), Verdict::Tricky);
    }

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
        let d = plain_text(&deep("<div>", 400_000));
        assert!(d.contains('x'));
    }
}

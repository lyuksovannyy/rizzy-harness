//! The shape of a page's tree, asked of the parser that builds it.
//!
//! The readability pass (`dom_smoothie`) and `html2text` cost time quadratic
//! in a tree's depth and recurse once per level: 20,000 nested `<div>` took 42 s
//! in release and 10,000 overflowed a debug stack. Predicting depth from the
//! markup needs a second, hand-written reading of HTML, and every such reading
//! disagreed with the real tree builder (`<noscript>` read as markup, HTML
//! integration points in SVG and MathML, optional end tags in foreign content,
//! `<font>` breaking out of foreign content only with attributes ...).
//! So there is no second reading: the page is parsed once by the parser the
//! readability pass itself uses (`dom_query`, html5ever with scripting off),
//! and the finished tree is measured, iteratively, before anything quadratic
//! runs on it. A page nested past [`MAX_NESTING`] or holding more than
//! [`MAX_NODES`] nodes is refused.
//!
//! `dom_query` is pinned to the exact version `dom_smoothie` resolves to; the
//! test `one_dom_query_in_the_lock_file` fails if the two ever drift apart.

use dom_query::Document;

/// The deepest element nesting a page may have to be read. Chromium's HTML
/// parser caps its tree at 512 levels (`kMaximumHTMLParserDOMTreeDepth`);
/// real pages nest a few dozen to a few hundred levels (not measured on a
/// corpus here: there is none offline).
pub const MAX_NESTING: usize = 512;

/// The most nodes a tree may have to be read: the budget the counting first
/// pass enforces inside the tree builder (`budget.rs`).
pub const MAX_NODES: usize = crate::budget::MAX_NODES;

/// The most `<` followed by a letter a page may hold to be handed to the
/// tree builder. Every start tag the tokenizer makes begins that way, so
/// this counts at least as many as the tree can be deep, whatever the markup
/// does (a tag in a comment or attribute is counted too: over-counting is
/// safe). It is a byte pattern, not a reading of HTML. The tree builder
/// walks its stack of open elements for many start tags, so its time is about
/// `K * depth * tags` at worst: measured, the worst input (`<ul>` repeated)
/// costs 1.1 ns per nesting-level-squared in release (24,579 nested in
/// 0.68 s), so 25,000 start tags cost at most about 0.7 s. A page with more
/// is read as plain text.
pub const MAX_START_TAGS: usize = 25_000;

/// How many `<` + ASCII letter the bytes hold (at least the number of start
/// tags the tokenizer will make).
pub fn start_tag_bound(bytes: &[u8]) -> usize {
    bytes
        .windows(2)
        .filter(|w| w[0] == b'<' && w[1].is_ascii_alphabetic())
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Ok,
    /// Nested deeper than [`MAX_NESTING`].
    TooDeep,
    /// More than [`MAX_NODES`] nodes.
    TooMany,
}

/// Depth (the root is 0) and node count of `doc`, walked with no recursion
/// and no allocation: first child, next sibling, parent. Stops early once a
/// limit is passed (the counts are then lower bounds).
pub fn measure(doc: &Document, max_depth: usize, max_nodes: usize) -> (usize, usize) {
    let mut node = doc.root();
    let (mut depth, mut deepest, mut count) = (0usize, 0usize, 0usize);
    loop {
        count += 1;
        deepest = deepest.max(depth);
        if deepest > max_depth || count > max_nodes {
            return (deepest, count);
        }
        if let Some(c) = node.first_child() {
            node = c;
            depth += 1;
            continue;
        }
        loop {
            if let Some(s) = node.next_sibling() {
                node = s;
                break;
            }
            match node.parent() {
                Some(p) => {
                    node = p;
                    depth -= 1;
                }
                None => return (deepest, count),
            }
        }
    }
}

pub fn check(doc: &Document) -> Shape {
    let (d, n) = measure(doc, MAX_NESTING, MAX_NODES);
    if d > MAX_NESTING {
        Shape::TooDeep
    } else if n > MAX_NODES {
        Shape::TooMany
    } else {
        Shape::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The iterative walk on a hand-built tree deeper than any recursion
    /// could take on this thread's stack, which is also dropped without
    /// recursing.
    #[test]
    fn the_walk_measures_a_very_deep_tree_without_recursing() {
        let n = 400_000;
        let doc = Document::default();
        let mut parent = doc.root();
        for _ in 0..n {
            let child = doc.tree.new_element("div");
            parent.append_child(&child);
            parent = child;
        }
        let (d, count) = measure(&doc, usize::MAX, usize::MAX);
        assert_eq!((d, count), (n, n + 1));
        assert_eq!(check(&doc), Shape::TooDeep);
        // Stops early at a limit, with a lower bound.
        let (d, _) = measure(&doc, 600, usize::MAX);
        assert!(d > 600 && d < n);
    }

    #[test]
    fn the_start_tag_bound_never_counts_fewer_than_the_tokenizer_makes() {
        assert_eq!(
            start_tag_bound(b"<a><b x='<c>'></b><!-- <d> --> 1 < 2 <3"),
            4
        );
        let doc = Document::from("<ul><li>a<ul><li>b</ul></ul><p>c");
        let html = "<ul><li>a<ul><li>b</ul></ul><p>c";
        let (_, nodes) = measure(&doc, usize::MAX, usize::MAX);
        assert!(start_tag_bound(html.as_bytes()) <= nodes);
        assert!(start_tag_bound(html.as_bytes()) >= 5);
    }

    #[test]
    fn a_flat_page_is_measured_and_a_wide_one_is_refused() {
        let doc = Document::from("<html><body><p>a<p>b<ul><li>c<li>d</ul></body></html>");
        let (d, n) = measure(&doc, MAX_NESTING, MAX_NODES);
        assert!(d <= 6 && n < 20, "{d} {n}");
        assert_eq!(check(&doc), Shape::Ok);
        let wide = format!("<body>{}</body>", "<b></b>".repeat(MAX_NODES));
        assert_eq!(check(&Document::from(wide.as_str())), Shape::TooMany);
    }

    /// `dom_smoothie` and this crate must parse with one `dom_query`: two
    /// versions in the lock file would mean the tree measured here is not the
    /// tree the readability pass builds.
    #[test]
    fn one_dom_query_in_the_lock_file() {
        let lock =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
                .unwrap();
        let n = lock.matches("name = \"dom_query\"").count();
        assert_eq!(n, 1, "dom_query resolves to {n} versions");
        let toml =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
        assert!(
            toml.contains("dom_query = { version = \"=0.29.0\""),
            "pin the exact version"
        );
        assert!(lock.contains("name = \"dom_query\"\nversion = \"0.29.0\""));
        // The tree builder the counting first pass drives is the one
        // `dom_query` drives.
        assert!(
            toml.contains("html5ever = { version = \"=0.40.1\""),
            "pin html5ever"
        );
        let (_, rest) = lock.split_once("name = \"dom_query\"").unwrap();
        let block = rest.split("[[package]]").next().unwrap();
        assert!(
            block.contains("\"html5ever 0.40.1\""),
            "dom_query no longer resolves to html5ever 0.40.1: {block}"
        );
    }
}

//! `doc_view` and `doc_edit` on a file shaped like Word's (report 3.4 docx 1-12).

use serde_json::json;

use super::docx::{self, DocOp};
use super::fixtures::*;
use super::pkg::Source;

fn part(bytes: &[u8], name: &str) -> String {
    String::from_utf8(Source::open(bytes).unwrap().read(name, 20 << 20).unwrap()).unwrap()
}

fn op(v: serde_json::Value) -> DocOp {
    DocOp::parse(&v).unwrap()
}

/// The tag of the line of the view that has `text`.
fn tag(view: &str, text: &str) -> String {
    let line = view
        .lines()
        .find(|l| l.contains(text) && !l.starts_with("Word document"))
        .unwrap_or_else(|| panic!("no line with {text:?} in\n{view}"));
    line.split_whitespace().next().unwrap().to_string()
}

fn view(b: &[u8]) -> String {
    docx::view(b, 1, 400).unwrap()
}

fn run(b: &[u8], ops: Vec<DocOp>) -> docx::Outcome {
    docx::edit(b, &ops).unwrap()
}

fn ok(o: docx::Outcome) -> Vec<u8> {
    for r in &o.results {
        assert!(r.is_ok(), "{:?}", o.results);
    }
    o.bytes.expect("a file")
}

fn refused(b: &[u8], ops: Vec<DocOp>) -> String {
    let o = run(b, ops);
    assert!(o.bytes.is_none(), "{:?}", o.results);
    o.results
        .iter()
        .filter_map(|r| r.as_ref().err().cloned())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Everything outside the changed stretch is the same bytes.
fn only_between(old: &str, new: &str, inside: &str) {
    let pre = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suf = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = old
        .find(inside)
        .expect("the changed element is in the old part");
    let end = start + inside.len();
    assert!(
        pre >= start,
        "bytes before the element changed: first diff at {pre}, element at {start}"
    );
    assert!(old.len() - suf <= end, "bytes after the element changed");
}

#[test]
fn the_view_shows_tags_styles_lists_tables_and_what_it_leaves_out() {
    let v = view(&docx_real());
    assert!(
        v.contains("p1#") && v.contains("[heading 1] Quarterly report"),
        "{v}"
    );
    assert!(
        v.contains("The quick brown fox jumps"),
        "split runs read as one text: {v}"
    );
    assert!(v.contains("Visit our site today"));
    assert!(
        v.contains("Page 3 of the report"),
        "a field's result is text: {v}"
    );
    assert!(
        v.contains("Keep added end"),
        "a tracked deletion is not text: {v}"
    );
    assert!(
        v.contains("Visible ") && !v.contains("secret"),
        "hidden text is left out"
    );
    assert!(v.contains("left out: hidden text"));
    assert!(v.contains("Picture: [obj] after") && !v.contains("inside the box"));
    assert!(v.contains("[list] first item"));
    assert!(v.contains("-- table 1 --") && v.contains("t1r1c1#"));
    assert!(
        v.contains("t1r2c1p1#") && v.contains("t1r2c1p2#"),
        "a cell with two paragraphs: {v}"
    );
    assert!(v.contains("t1r2c2#"));
    assert!(
        v.contains("(not editable) Contents"),
        "a content control's paragraph is read-only"
    );
    // The default page is long enough for the whole of this file.
    assert!(docx::view(&docx_real(), 1, 0).unwrap().contains("The end."));
    // Paging.
    let p = docx::view(&docx_real(), 5, 2).unwrap();
    assert!(
        p.contains("[cut: showed items 5-6") && p.contains("from=7"),
        "{p}"
    );
}

#[test]
fn a_phrase_split_over_runs_is_replaced_and_nothing_else_moves() {
    let b = docx_real();
    let v = view(&b);
    let t = tag(&v, "quick brown");
    let old = part(&b, "word/document.xml");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": t, "find": "quick brown fox", "replace": "slow red cat"}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(view(&out).contains("The slow red cat jumps"));
    // The first run's properties kept it, the later runs were cut, the spelling marks stayed.
    assert!(
        new.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">The slow red cat</w:t>"),
        "{new}"
    );
    assert!(
        new.contains("<w:proofErr w:type=\"spellStart\"/>")
            && new.contains("<w:proofErr w:type=\"spellEnd\"/>")
    );
    assert!(new.contains("w:rsidRPr=\"00C10003\""));
    let para_start = old.find("<w:p w:rsidR=\"00B10002\"").unwrap();
    let para_end = old[para_start..].find("</w:p>").unwrap() + para_start + 6;
    only_between(&old, &new, &old[para_start..para_end]);
    // Every other part of the package is the original's bytes.
    super::tests::assert_others_identical(&b, &out, &["word/document.xml"]);
}

#[test]
fn leading_and_double_spaces_get_xml_space_preserve() {
    let b = docx_real();
    let t = tag(&view(&b), "Quarterly report");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": t, "find": "Quarterly", "replace": " Q3  big "}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(
        new.contains("<w:t xml:space=\"preserve\"> Q3  big  report</w:t>"),
        "{new}"
    );
}

#[test]
fn find_must_match_once_unless_all() {
    let b = docx_real();
    let t = tag(&view(&b), "first item");
    let e = refused(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": t.clone(), "find": "i", "replace": "I"}),
        )],
    );
    assert!(e.contains("occurs") && e.contains("`all`"), "{e}");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": t, "find": "i", "replace": "I", "all": true}),
        )],
    ));
    assert!(view(&out).contains("fIrst Item"));
    let t = tag(&view(&b), "first item");
    let e = refused(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": t, "find": "nope", "replace": "x"}),
        )],
    );
    assert!(e.contains("no text"), "{e}");
    let t = tag(&view(&b), "first item");
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "replace_text", "at": t, "find": "", "replace": "x"})
            )]
        )
        .contains("empty")
    );
}

#[test]
fn hidden_and_deleted_text_cannot_be_matched() {
    let b = docx_real();
    let v = view(&b);
    let hidden = tag(&v, "Visible");
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "replace_text", "at": hidden, "find": "secret", "replace": "x"})
            )]
        )
        .contains("no text")
    );
    let tracked = tag(&v, "Keep added end");
    assert!(refused(&b, vec![op(json!({"op": "replace_text", "at": tracked.clone(), "find": "removed", "replace": "x"}))]).contains("no text"));
    // Matching across the deleted stretch joins what the person sees.
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": tracked, "find": "Keep ", "replace": "Hold "}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(new.contains("<w:delText>removed</w:delText>") && new.contains("Hold "));
    // The hidden run stays as it was.
    assert!(new.contains("<w:rPr><w:vanish/></w:rPr><w:t>secret</w:t>"));
}

#[test]
fn a_match_may_not_cross_a_field_a_link_or_a_picture() {
    let b = docx_real();
    let v = view(&b);
    let f = tag(&v, "Page 3");
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "replace_text", "at": f.clone(), "find": "3 of", "replace": "4 of"})
            )]
        )
        .contains("field")
    );
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "replace_text", "at": f.clone(), "find": "3", "replace": "4"})
            )]
        )
        .contains("field")
    );
    // Text outside the field is fine.
    ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": f, "find": "of the report", "replace": "of the memo"}),
        )],
    ));
    let l = tag(&v, "Visit");
    assert!(refused(&b, vec![op(json!({"op": "replace_text", "at": l.clone(), "find": "Visit our", "replace": "See our"}))]).contains("across"));
    // Inside the link is allowed, and the link stays.
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": l, "find": "our site", "replace": "the site"}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(new.contains("<w:hyperlink r:id=\"rId5\" w:history=\"1\"><w:r><w:rPr><w:rStyle w:val=\"Hyperlink\"/></w:rPr><w:t>the site</w:t></w:r></w:hyperlink>"), "{new}");
    let p = tag(&v, "Picture");
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "replace_text", "at": p, "find": "\u{fffc} after", "replace": "x"})
            )]
        )
        .contains("picture")
    );
    // Text before it, in its own run, is fine.
    ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": tag(&v, "Picture"), "find": "Picture: ", "replace": "Image: "}),
        )],
    ));
}

#[test]
fn a_stale_or_made_up_tag_refuses() {
    let b = docx_real();
    let t = tag(&view(&b), "first item");
    let (k, _) = t.split_once('#').unwrap();
    let e = refused(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": format!("{k}#0000"), "find": "first", "replace": "x"}),
        )],
    );
    assert!(e.contains("changed since the view"), "{e}");
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "set_text", "at": "p999#abcd", "text": "x"})
            )]
        )
        .contains("no paragraph")
    );
    assert!(
        refused(
            &b,
            vec![op(json!({"op": "set_text", "at": "p1", "text": "x"}))]
        )
        .contains("tag from doc_view")
    );
    // A paragraph in a content control cannot be addressed.
    assert!(
        refused(&b, vec![op(json!({"op": "delete", "at": "t9r9c9#abcd"}))])
            .contains("no paragraph")
    );
}

#[test]
fn set_text_keeps_the_first_runs_properties_and_the_bookmark() {
    let b = docx_real();
    let v = view(&b);
    let t = tag(&v, "Quarterly report");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "set_text", "at": t, "text": "Annual report & plan"}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(new.contains("<w:bookmarkStart w:id=\"0\" w:name=\"_Toc100\"/><w:r><w:t xml:space=\"preserve\">Annual report &amp; plan</w:t></w:r><w:bookmarkEnd w:id=\"0\"/>"), "{new}");
    assert!(new.contains("<w:pStyle w:val=\"berschrift1\"/>"));
    // A tab and a line break become elements, not characters.
    let t = tag(&v, "first item");
    let out = ok(run(
        &b,
        vec![op(json!({"op": "set_text", "at": t, "text": "a\tb\nc"}))],
    ));
    let new = part(&out, "word/document.xml");
    assert!(new.contains("<w:t xml:space=\"preserve\">a</w:t><w:tab/><w:t xml:space=\"preserve\">b</w:t><w:br/><w:t xml:space=\"preserve\">c</w:t>"), "{new}");
    assert!(view(&out).contains("a\\tb\\nc"));
    // An empty paragraph gains a run; the text of a split paragraph is one run.
    let t = tag(&view(&b), "The quick");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "set_text", "at": t, "text": "One run now"}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(
        new.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">One run now</w:t></w:r>")
    );
    assert!(!new.contains("ick bro"));
    let last = v
        .lines()
        .filter(|l| l.starts_with('p'))
        .last()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "set_text", "at": last, "text": "now text"}),
        )],
    ));
    assert!(
        part(&out, "word/document.xml")
            .contains("<w:p><w:r><w:t xml:space=\"preserve\">now text</w:t></w:r></w:p><w:sectPr>")
    );
}

#[test]
fn set_text_refuses_what_it_would_destroy() {
    let b = docx_real();
    let v = view(&b);
    for (text, what) in [
        ("Page 3", "field"),
        ("Visit", "link"),
        ("Keep added", "tracked"),
        ("Picture", "picture"),
        ("Visible", "hidden"),
    ] {
        let t = tag(&v, text);
        let e = refused(
            &b,
            vec![op(json!({"op": "set_text", "at": t, "text": "x"}))],
        );
        assert!(e.contains(what), "{text}: {e}");
    }
}

#[test]
fn delete_keeps_page_setup_the_last_paragraph_and_anchors() {
    let b = docx_real();
    let v = view(&b);
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "delete", "at": tag(&v, "End of section one")})
            )]
        )
        .contains("page setup")
    );
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "delete", "at": tag(&v, "Quarterly report")})
            )]
        )
        .contains("bookmark")
    );
    let last = v
        .lines()
        .filter(|l| l.starts_with('p'))
        .last()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert!(refused(&b, vec![op(json!({"op": "delete", "at": last}))]).contains("last paragraph"));
    // The only paragraph of a cell.
    assert!(
        refused(
            &b,
            vec![op(json!({"op": "delete", "at": tag(&v, "Amount")}))]
        )
        .contains("last paragraph")
    );
    // One of two in a cell: fine.
    let out = ok(run(
        &b,
        vec![op(json!({"op": "delete", "at": tag(&v, "Alice")}))],
    ));
    let new = part(&out, "word/document.xml");
    assert!(!new.contains("Alice") && new.contains("<w:t>(lead)</w:t>"));
    let out = ok(run(
        &b,
        vec![op(json!({"op": "delete", "at": tag(&v, "second item")}))],
    ));
    assert!(!view(&out).contains("second item") && view(&out).contains("first item"));
}

#[test]
fn inserts_follow_the_documents_styles_and_lists() {
    let b = docx_real();
    let v = view(&b);
    // A heading by NAME: this file calls its style berschrift2.
    let t = tag(&v, "Quarterly report");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "insert_after", "at": t, "markdown": "## New part\n\nBody with **bold**, *it*, ~~gone~~ and `code`."}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert!(new.contains("<w:p><w:pPr><w:pStyle w:val=\"berschrift2\"/></w:pPr><w:r><w:t xml:space=\"preserve\">New part</w:t></w:r></w:p>"), "{new}");
    assert!(new.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">bold</w:t>"));
    assert!(new.contains(
        "<w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/></w:rPr>"
    ));
    assert!(new.contains("<w:rPr><w:strike/></w:rPr>"));
    assert!(
        !new.contains("w:val=\"berschrift1\"/></w:pPr><w:r><w:t xml:space=\"preserve\">Body"),
        "a body paragraph after a heading is not a heading"
    );
    let v2 = view(&out);
    let a = v2.find("Quarterly report").unwrap();
    assert!(v2[a..].find("New part").unwrap() < v2[a..].find("The quick").unwrap());
    // Items continue the anchor's list by copying its numPr.
    let t = tag(&v, "second item");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "insert_after", "at": t, "markdown": "- third\n- fourth"}),
        )],
    ));
    let new = part(&out, "word/document.xml");
    assert_eq!(
        new.matches("<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>")
            .count(),
        4
    );
    assert!(view(&out).contains("[list] third") && view(&out).contains("[list] fourth"));
    // Before.
    let t = tag(&v, "The end.");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "insert_before", "at": t, "markdown": "Just before."}),
        )],
    ));
    let v3 = view(&out);
    assert!(v3.find("Just before.").unwrap() < v3.find("The end.").unwrap());
}

#[test]
fn inserts_refuse_what_needs_parts_the_file_may_not_get() {
    let b = docx_real();
    let v = view(&b);
    let t = tag(&v, "The end.");
    for (md, what) in [
        ("[a link](http://x.example)", "links"),
        ("![pic](x.png)", "images"),
        ("| a | b |\n|---|---|\n| 1 | 2 |", "tables"),
        ("> quote", "block quotes"),
        ("```\ncode\n```", "code blocks"),
        ("<b>html</b>", "HTML"),
        ("---", "rule"),
    ] {
        let o = run(
            &b,
            vec![op(
                json!({"op": "insert_after", "at": t.clone(), "markdown": md}),
            )],
        );
        let e = o.results[0]
            .clone()
            .err()
            .unwrap_or_else(|| panic!("{md:?} was accepted"));
        assert!(e.contains(what), "{md}: {e}");
    }
    // A list next to a paragraph that is not in one; a heading the file has no style for.
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "insert_after", "at": t.clone(), "markdown": "- item"})
            )]
        )
        .contains("not a list item")
    );
    assert!(
        refused(
            &b,
            vec![op(
                json!({"op": "insert_after", "at": t, "markdown": "### Three"})
            )]
        )
        .contains("no style named \"heading 3\"")
    );
}

#[test]
fn a_call_applies_all_its_edits_or_none_and_says_which() {
    let b = docx_real();
    let v = view(&b);
    let good = op(
        json!({"op": "replace_text", "at": tag(&v, "first item"), "find": "first", "replace": "1st"}),
    );
    let bad = op(
        json!({"op": "replace_text", "at": tag(&v, "second item"), "find": "zzz", "replace": "x"}),
    );
    let o = run(&b, vec![good.clone(), bad]);
    assert!(o.bytes.is_none());
    assert!(
        o.results[0].as_ref().unwrap().contains("not applied"),
        "{:?}",
        o.results
    );
    assert!(o.results[1].as_ref().unwrap_err().contains("no text"));
    let o = run(&b, vec![good]);
    assert!(o.bytes.is_some());
}

#[test]
fn several_edits_of_one_paragraph_and_neighbours_combine() {
    let b = docx_real();
    let v = view(&b);
    let t = tag(&v, "The quick");
    let u = tag(&v, "Quarterly");
    let out = ok(run(
        &b,
        vec![
            op(json!({"op": "replace_text", "at": t.clone(), "find": "quick", "replace": "slow"})),
            op(json!({"op": "replace_text", "at": t.clone(), "find": "fox", "replace": "owl"})),
            op(json!({"op": "insert_after", "at": t.clone(), "markdown": "after-1"})),
            op(json!({"op": "insert_after", "at": t, "markdown": "after-2"})),
            op(json!({"op": "insert_before", "at": u, "markdown": "first of all"})),
        ],
    ));
    let v = view(&out);
    assert!(v.contains("The slow brown owl jumps"));
    assert!(v.find("after-1").unwrap() < v.find("after-2").unwrap());
    assert!(v.find("first of all").unwrap() < v.find("Quarterly").unwrap());
    // set_text and delete take a paragraph alone.
    let t = tag(&view(&b), "The quick");
    let e = refused(
        &b,
        vec![
            op(json!({"op": "set_text", "at": t.clone(), "text": "x"})),
            op(json!({"op": "replace_text", "at": t, "find": "fox", "replace": "owl"})),
        ],
    );
    assert!(e.contains("alone"), "{e}");
}

#[test]
fn a_cell_is_edited_like_a_paragraph() {
    let b = docx_real();
    let v = view(&b);
    let c = tag(&v, "1,200");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "replace_text", "at": c, "find": "1,200", "replace": "1,450"}),
        )],
    ));
    assert!(part(&out, "word/document.xml").contains("<w:t>1,450</w:t>"));
    let c = tag(&v, "(lead)");
    let out = ok(run(
        &b,
        vec![op(
            json!({"op": "insert_after", "at": c, "markdown": "(deputy)"}),
        )],
    ));
    assert!(view(&out).contains("t1r2c1p3#"), "{}", view(&out));
}

#[test]
fn protected_signed_and_foreign_documents_are_refused() {
    let doc = document_xml();
    let prot = docx_with(
        &doc,
        &[(
            "word/settings.xml",
            &format!(
                "{DECL}<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:documentProtection w:edit=\"readOnly\" w:enforcement=\"1\"/></w:settings>"
            ),
        )],
    );
    let e = docx::edit(&prot, &[op(json!({"op": "delete", "at": "p1#0000"}))]).unwrap_err();
    assert!(e.contains("protected"), "{e}");
    let off = docx_with(
        &doc,
        &[(
            "word/settings.xml",
            &format!(
                "{DECL}<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:documentProtection w:enforcement=\"0\"/></w:settings>"
            ),
        )],
    );
    assert!(docx::view(&off, 1, 10).is_ok());
    let signed = docx_with(&doc, &[("_xmlsignatures/sig1.xml", "<s/>")]);
    assert!(
        docx::edit(&signed, &[op(json!({"op": "delete", "at": "p1#0000"}))])
            .unwrap_err()
            .contains("signed")
    );
    assert!(docx::view(b"not a zip", 1, 10).is_err());
}

#[test]
fn the_edit_calls_bounds_hold_at_their_edge() {
    use crate::limits::*;
    let b = docx_real();
    let t = tag(&view(&b), "first item");
    let one = || json!({"op": "replace_text", "at": t.clone(), "find": "first", "replace": "x"});
    let many: Vec<DocOp> = (0..MAX_EDITS + 1).map(|_| op(one())).collect();
    assert!(docx::edit(&b, &many).unwrap_err().contains("at most"));
    // The text of one edit.
    let long = "a".repeat(MAX_EDIT_TEXT_CHARS);
    assert!(DocOp::parse(&json!({"op": "set_text", "at": "p1#0000", "text": long})).is_ok());
    let long = "a".repeat(MAX_EDIT_TEXT_CHARS + 1);
    assert!(DocOp::parse(&json!({"op": "set_text", "at": "p1#0000", "text": long})).is_err());
    let md = "a".repeat(MAX_EDIT_MARKDOWN_BYTES);
    assert!(DocOp::parse(&json!({"op": "insert_after", "at": "p1#0000", "markdown": md})).is_ok());
    let md = "a".repeat(MAX_EDIT_MARKDOWN_BYTES + 1);
    assert!(DocOp::parse(&json!({"op": "insert_after", "at": "p1#0000", "markdown": md})).is_err());
    // Unknown fields and ops.
    assert!(
        DocOp::parse(
            &json!({"op": "replace_text", "at": "p1#0000", "find": "a", "replace": "b", "x": 1})
        )
        .is_err()
    );
    assert!(DocOp::parse(&json!({"op": "explode", "at": "p1#0000"})).is_err());
    assert!(DocOp::parse(&json!({"op": "replace_text", "at": "p1#0000", "find": "a"})).is_err());
    assert!(docx::edit(&b, &[]).is_err());
}

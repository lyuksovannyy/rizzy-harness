//! The sentences a create tool tells the model about what it made (PLAN.md
//! T97): built from counts and the cleaned names the model gave, nothing
//! else, so the same words come from the node's own tool and from the
//! master's service.

use harness_core::event::shown_name;

use crate::{docx, xlsx};

/// What a Markdown conversion holds and left out.
pub fn doc(r: &docx::Report) -> Vec<String> {
    let mut v = vec![format!(
        "{} paragraphs, {} tables, {} numbered lists, {} links.",
        r.paragraphs, r.tables, r.lists, r.links
    )];
    if r.html_dropped > 0 {
        v.push(format!(
            "Raw HTML ({} pieces) was not used: the file is written from Markdown only.",
            r.html_dropped
        ));
    }
    if r.images_dropped > 0 {
        v.push(format!(
            "{} images were not embedded (their alternative text is kept): files made here hold no pictures.",
            r.images_dropped
        ));
    }
    if r.links_dropped > 0 {
        v.push(format!(
            "{} links were left as plain text: only http, https and mailto addresses are kept.",
            r.links_dropped
        ));
    }
    v
}

/// What a workbook holds, the names it was given and what stays text.
pub fn sheet(r: &xlsx::Report) -> Vec<String> {
    let mut v = Vec::new();
    for (i, s) in r.sheets.iter().enumerate() {
        v.push(format!(
            "Sheet {} \"{}\": {} rows, {} columns, {} formulas.",
            i + 1,
            shown_name(&s.name),
            s.rows,
            s.cols,
            s.formulas
        ));
        if let Some(asked) = &s.renamed_from {
            v.push(format!(
                "Sheet {} was asked for as \"{}\" and is named as above: a sheet name is at most 31 characters, has none of : \\ / ? * [ ], and is different from every other even by letter case.",
                i + 1,
                shown_name(asked)
            ));
        }
    }
    if r.dates_left_as_text > 0 {
        v.push(format!(
            "{} cells in date columns were not ISO dates (YYYY-MM-DD, or YYYY-MM-DDTHH:MM) and stay text.",
            r.dates_left_as_text
        ));
    }
    if r.formulas > 0 {
        v.push(
            "Formulas are calculated when the file is opened: it holds no calculated values, so a viewer that does not calculate shows them empty."
                .into(),
        );
    }
    v
}

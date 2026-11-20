//! A row counts as shown only if it is in the text the model receives
//! (PLAN.md T45): every result that records rows as shown stays under the
//! result cap ([`crate::MAX_OUTPUT_CHARS`]), whose cut would lose the middle,
//! says how many rows it left out, and records only the rows it printed.
//! Found by the review of PR 188: `grep`, the line form's refusal, the
//! `old_string` form's refusal and the results of edits printed up to 120,000
//! characters, and every row of them counted as shown.

use std::collections::BTreeSet;

use serde_json::json;

use crate::{Edit, Grep, Read, Tool, ToolCtx, ToolOutcome, Write};

fn rig() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    (dir, ctx)
}

fn put(dir: &tempfile::TempDir, name: &str, text: &str) {
    let p = dir.path().join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn get(dir: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(dir.path().join(name)).unwrap()
}

/// The tag of the last `[path#TAG]` line of `out`.
fn tag_in(out: &str) -> String {
    let l = out
        .lines()
        .rev()
        .find(|l| l.starts_with('[') && l.contains('#') && l.ends_with(']'))
        .unwrap_or_else(|| panic!("no header in {} chars", out.chars().count()));
    l[l.rfind('#').unwrap() + 1..l.len() - 1].to_string()
}

/// The row numbers `out` prints: `N<TAB>text` (read style, edit results and
/// refusals) or `N:text` (grep), the number at the start of a line.
fn printed(out: &str) -> BTreeSet<usize> {
    out.lines()
        .filter_map(|l| {
            let (n, _) = l.split_once('\t').or_else(|| l.split_once(':'))?;
            n.trim().parse::<usize>().ok()
        })
        .collect()
}

/// The result as the model gets it: not cut by the cap.
fn whole(o: &ToolOutcome) {
    assert!(
        !o.truncated && o.output.chars().count() <= crate::MAX_OUTPUT_CHARS,
        "the result was cut: {} chars, truncated={}",
        o.output.chars().count(),
        o.truncated
    );
    assert!(!o.output.contains("[... output truncated ...]"));
}

fn tail(o: &ToolOutcome) -> String {
    o.output
        .chars()
        .rev()
        .take(500)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Whether a line edit of row `n` (replacing it) under `tag` lands.
async fn lands(ctx: &ToolCtx, path: &str, tag: &str, n: usize) -> bool {
    let o = Edit
        .run(
            ctx,
            json!({"path": path, "tag": tag, "lines": [{"from": n, "to": n, "text": "CHANGED\n"}]}),
        )
        .await;
    o.ok
}

fn long_rows(n: usize, width: usize) -> String {
    (1..=n)
        .map(|i| format!("r{i:03}{}\n", "x".repeat(width)))
        .collect()
}

/// The first row of `1..=total` that `p` lacks, and the last it has.
fn edges(p: &BTreeSet<usize>, total: usize) -> (usize, usize) {
    let missing = (1..=total)
        .find(|n| !p.contains(n))
        .expect("a row is left out");
    let last = *p.iter().next_back().expect("a row is printed");
    (missing, last)
}

#[tokio::test]
async fn grep_prints_only_rows_that_survive_the_cap_and_records_only_those() {
    let text = long_rows(150, 280);
    let (dir, ctx) = rig();
    put(&dir, "f.txt", &text);
    let o = Grep.run(&ctx, json!({"pattern": "xxxx"})).await;
    whole(&o);
    let p = printed(&o.output);
    assert!(p.len() > 50 && p.len() < 150, "{} rows", p.len());
    assert!(o.output.contains("not shown"), "{}", tail(&o));
    let (missing, last) = edges(&p, 150);
    let tag = tag_in(&o.output);
    // The rows are a prefix of the matches; row 75, which the cap's cut
    // used to take out of the middle, is printed now, and row 150 is not.
    assert!(missing == last + 1 && p.contains(&75) && !p.contains(&150));
    for (n, want) in [(75, true), (missing, false), (last, true), (150, false)] {
        let (dir, ctx) = rig();
        put(&dir, "f.txt", &text);
        let o = Grep.run(&ctx, json!({"pattern": "xxxx"})).await;
        assert_eq!(tag_in(&o.output), tag);
        assert_eq!(lands(&ctx, "f.txt", &tag, n).await, want, "row {n}");
        assert_eq!(get(&dir, "f.txt") != text, want);
    }
}

#[tokio::test]
async fn the_line_forms_refusal_prints_only_what_survives_the_cap() {
    let text = long_rows(60, 1900);
    let ask =
        json!({"path": "f.txt", "tag": "0000", "lines": [{"from": 1, "to": 50, "text": "X\n"}]});
    let (dir, ctx) = rig();
    put(&dir, "f.txt", &text);
    let o = Edit.run(&ctx, ask.clone()).await;
    assert!(!o.ok);
    whole(&o);
    let p = printed(&o.output);
    assert!(p.len() >= 5 && p.len() < 40, "{} rows", p.len());
    assert!(o.output.contains("not shown"), "{}", tail(&o));
    let tag = tag_in(&o.output);
    let (missing, last) = edges(&p, 60);
    for (n, want) in [(25, false), (missing, false), (last, true)] {
        let (dir, ctx) = rig();
        put(&dir, "f.txt", &text);
        let o = Edit.run(&ctx, ask.clone()).await;
        assert_eq!(tag_in(&o.output), tag);
        assert_eq!(lands(&ctx, "f.txt", &tag, n).await, want, "row {n}");
        assert_eq!(get(&dir, "f.txt") != text, want);
    }
}

#[tokio::test]
async fn the_old_string_forms_refusal_prints_only_what_survives_the_cap() {
    let rows: Vec<String> = (0..30)
        .map(|i| format!("{i:03}{}", "y".repeat(1990)))
        .collect();
    let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
    // Twelve rows to replace (about 24,000 characters) and their context.
    let old = rows[10..22].join("\n");
    let ask = json!({"path": "f.txt", "old_string": old, "new_string": "Z"});
    let (dir, ctx) = rig();
    put(&dir, "f.txt", &text);
    let r = Edit.refusal(&ctx, &ask).expect("unread");
    let o = ToolOutcome::err(r);
    whole(&o);
    let p = printed(&o.output);
    // The rows to replace come first; the context only where it fits.
    assert!((11..=22).all(|n| p.contains(&n)), "{p:?}");
    assert!(p.len() < 18, "{p:?}");
    // The announced retry lands (its match is wholly inside what was shown).
    let again = Edit.run(&ctx, ask.clone()).await;
    assert!(again.ok, "{}", again.output);
    // Rows that were left out of the output count as nothing.
    let (dir, ctx) = rig();
    put(&dir, "f.txt", &text);
    let _ = Edit.refusal(&ctx, &ask);
    let (missing, _) = edges(&p, 30);
    let other = json!({"path": "f.txt", "old_string": rows[missing - 1], "new_string": "Q"});
    let e = Edit.run(&ctx, other).await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "f.txt"), text);
}

#[tokio::test]
async fn a_match_too_long_to_print_is_told_to_read_and_is_not_announced_as_retryable() {
    let rows: Vec<String> = (0..30)
        .map(|i| format!("{i:03}{}", "y".repeat(1990)))
        .collect();
    let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
    let ask = json!({"path": "f.txt", "old_string": rows[2..28].join("\n"), "new_string": "Z"});
    let (dir, ctx) = rig();
    put(&dir, "f.txt", &text);
    let o = Edit.run(&ctx, ask.clone()).await;
    assert!(!o.ok);
    whole(&o);
    assert!(
        !o.output.contains("send the same edit again"),
        "{}",
        o.output.chars().take(600).collect::<String>()
    );
    assert!(o.output.contains("`read`"), "{}", tail(&o));
    let again = Edit.run(&ctx, ask).await;
    assert!(!again.ok, "{}", again.output);
    assert_eq!(get(&dir, "f.txt"), text);
}

#[tokio::test]
async fn a_read_with_a_long_path_stays_under_the_cap() {
    // A path of about 2,900 characters: its header is in the result too.
    let deep: String = (0..12)
        .map(|i| format!("{i:02}{}/", "d".repeat(237)))
        .collect();
    let path = format!("{deep}f.txt");
    let text = long_rows(120, 500);
    let (dir, ctx) = rig();
    put(&dir, &path, &text);
    let o = Read.run(&ctx, json!({"path": path})).await;
    assert!(o.ok, "{}", o.output);
    whole(&o);
    let p = printed(&o.output);
    assert!(p.len() > 30 && p.len() < 120, "{} rows", p.len());
    let tag = tag_in(&o.output);
    let (missing, last) = edges(&p, 120);
    for (n, want) in [(missing, false), (last, true)] {
        let (dir, ctx) = rig();
        put(&dir, &path, &text);
        let o = Read.run(&ctx, json!({"path": path})).await;
        assert_eq!(tag_in(&o.output), tag);
        assert_eq!(lands(&ctx, &path, &tag, n).await, want, "row {n}");
    }
}

#[tokio::test]
async fn the_result_of_an_old_string_edit_prints_only_what_survives_the_cap() {
    let big: String = (1..=58)
        .map(|i| format!("n{i:02}{}\n", "z".repeat(1900)))
        .collect();
    let (dir, ctx) = rig();
    put(&dir, "f.txt", "head\nMARK\ntail\n");
    assert!(Read.run(&ctx, json!({"path": "f.txt"})).await.ok);
    let o = Edit
        .run(
            &ctx,
            json!({"path": "f.txt", "old_string": "MARK\n", "new_string": big}),
        )
        .await;
    assert!(o.ok, "{}", o.output);
    whole(&o);
    let p = printed(&o.output);
    assert!(p.len() >= 5 && p.len() < 58, "{} rows", p.len());
    assert!(o.output.contains("not shown"), "{}", tail(&o));
    let tag = tag_in(&o.output);
    let now = get(&dir, "f.txt");
    let (missing, last) = edges(&p, 60);
    // The last rows were not printed: they are not shown, so a line edit of
    // one is refused.
    assert!(missing <= 50 && last < 50, "{missing} {last}");
    assert!(!lands(&ctx, "f.txt", &tag, 50).await);
    assert!(!lands(&ctx, "f.txt", &tag, missing).await);
    assert_eq!(get(&dir, "f.txt"), now);
    assert!(lands(&ctx, "f.txt", &tag, last).await);
}

#[tokio::test]
async fn the_result_of_a_line_edit_prints_only_what_survives_the_cap() {
    let big: Vec<String> = (1..=58)
        .map(|i| format!("n{i:02}{}", "z".repeat(1900)))
        .collect();
    let (dir, ctx) = rig();
    put(&dir, "f.txt", "head\nMARK\ntail\n");
    let r = Read.run(&ctx, json!({"path": "f.txt"})).await;
    let o = Edit
        .run(
            &ctx,
            json!({"path": "f.txt", "tag": tag_in(&r.output),
                   "lines": [{"from": 2, "to": 2, "text": big}]}),
        )
        .await;
    assert!(o.ok, "{}", o.output);
    whole(&o);
    let p = printed(&o.output);
    assert!(p.len() >= 5 && p.len() < 58, "{} rows", p.len());
    let tag = tag_in(&o.output);
    let (missing, last) = edges(&p, 60);
    assert!(!lands(&ctx, "f.txt", &tag, missing).await);
    assert!(lands(&ctx, "f.txt", &tag, last).await);
    let _ = dir;
}

#[tokio::test]
async fn a_write_with_a_long_path_stays_under_the_cap() {
    let deep: String = (0..12)
        .map(|i| format!("{i:02}{}/", "d".repeat(237)))
        .collect();
    let path = format!("{deep}f.txt");
    let (_dir, ctx) = rig();
    let o = Write
        .run(&ctx, json!({"path": path, "content": "a\nb\n"}))
        .await;
    assert!(o.ok, "{}", o.output);
    whole(&o);
}

/// The general check: a result that records rows as shown is sealed first,
/// and sealing refuses text the cap would cut.
#[test]
fn text_over_the_cap_cannot_be_sealed() {
    use crate::shown::{Budget, within};
    let at = "a".repeat(crate::MAX_OUTPUT_CHARS);
    assert!(within(&at));
    assert!(!within(&format!("{at}a")));
    // The same count in characters, not bytes.
    assert!(within(&"é".repeat(crate::MAX_OUTPUT_CHARS)));
    let mut b = Budget::new();
    let row = "r".repeat(1000);
    let mut n = 0;
    while b.take(&row) {
        n += 1;
    }
    assert!(n > 20 && n * 1000 < crate::MAX_OUTPUT_CHARS, "{n}");
    // What was refused was not spent: a shorter text still fits the rest.
    assert!(b.left() < 1000);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "over the cap")]
fn sealing_over_the_cap_is_a_bug_that_tests_see() {
    let _ = crate::shown::seal(&"a".repeat(crate::MAX_OUTPUT_CHARS + 1));
}

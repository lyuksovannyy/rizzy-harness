//! `Tool::preview` of `edit` (PLAN.md T45, the approval card): the rows a
//! line edit replaces, for the person deciding, from the call and the file as
//! it is now. Nothing is written and nothing is recorded as shown.

use serde_json::{Value, json};

use crate::{Edit, Read, Tool, ToolCtx};

fn ws() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    (dir, ctx)
}

fn put(dir: &tempfile::TempDir, name: &str, text: &str) {
    std::fs::write(dir.path().join(name), text).unwrap();
}

fn numbered(n: usize) -> String {
    (1..=n).map(|i| format!("row {i}\n")).collect()
}

async fn read_tag(ctx: &ToolCtx, path: &str) -> String {
    let r = Read.run(ctx, json!({ "path": path })).await;
    assert!(r.ok, "{}", r.output);
    let first = r.output.lines().next().unwrap();
    first[first.rfind('#').unwrap() + 1..first.len() - 1].to_string()
}

fn call(tag: &str, ops: Value) -> Value {
    json!({"path": "a.txt", "tag": tag, "lines": ops})
}

#[tokio::test]
async fn a_replacement_shows_the_rows_it_replaces_numbered_as_read_numbers_them() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(30));
    let tag = read_tag(&ctx, "a.txt").await;
    let p = Edit
        .preview(
            &ctx,
            &call(&tag, json!([{"from": 12, "to": 14, "text": "new"}])),
        )
        .expect("a preview");
    assert!(p.contains("rows 12-14 are replaced:"), "{p}");
    assert!(p.contains("    12\trow 12\n"), "{p}");
    assert!(p.contains("    14\trow 14\n"), "{p}");
    assert!(!p.contains("row 15"), "{p}");
    // The new text is the card's own field, not repeated here.
    assert!(!p.contains("new"), "{p}");
    // One row: the singular.
    let p = Edit
        .preview(
            &ctx,
            &call(&tag, json!([{"from": 3, "to": 3, "text": "x"}])),
        )
        .unwrap();
    assert!(p.contains("row 3 is replaced:"), "{p}");
}

#[tokio::test]
async fn an_insertion_shows_its_place_with_one_row_of_context() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(30));
    let tag = read_tag(&ctx, "a.txt").await;
    let p = Edit
        .preview(&ctx, &call(&tag, json!([{"after": 20, "text": "new"}])))
        .unwrap();
    assert!(p.contains("inserted after row 20:"), "{p}");
    assert!(p.contains("    20\trow 20\n"), "{p}");
    assert!(!p.contains("row 19"), "{p}");
    assert!(!p.contains("row 21"), "{p}");
    let p = Edit
        .preview(&ctx, &call(&tag, json!([{"after": 0, "text": "new"}])))
        .unwrap();
    assert!(p.contains("inserted at the top of the file"), "{p}");
}

#[tokio::test]
async fn several_operations_come_in_file_order_and_a_moved_file_is_numbered_as_it_is_now() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(30));
    let tag = read_tag(&ctx, "a.txt").await;
    // Two rows added at the top since the tag: every row moves by two.
    put(&dir, "a.txt", &format!("top 1\ntop 2\n{}", numbered(30)));
    let p = Edit
        .preview(
            &ctx,
            &call(
                &tag,
                json!([{"after": 25, "text": "i"}, {"from": 5, "to": 5, "text": "r"}]),
            ),
        )
        .unwrap();
    let (a, b) = (
        p.find("row 7 is replaced").unwrap(),
        p.find("after row 27").unwrap(),
    );
    assert!(a < b, "{p}");
    assert!(
        p.contains("    7\trow 5\n") || p.contains("     7\trow 5\n"),
        "{p}"
    );
    assert!(p.contains("changed since"), "{p}");
}

#[tokio::test]
async fn no_preview_where_the_call_is_refused_or_is_not_the_line_form() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(10));
    let tag = read_tag(&ctx, "a.txt").await;
    // The old form, bash-like or foreign tools: none.
    let old = json!({"path": "a.txt", "old_string": "row 2", "new_string": "x"});
    assert!(Edit.preview(&ctx, &old).is_none());
    // An unknown tag, a row that does not exist, no change, bad arguments.
    for input in [
        call("ZZZZ", json!([{"from": 1, "to": 1, "text": "x"}])),
        call(&tag, json!([{"from": 11, "to": 11, "text": "x"}])),
        call(&tag, json!([{"from": 1, "to": 1, "text": "row 1"}])),
        call(&tag, json!([{"from": 0, "to": 1, "text": "x"}])),
        call(&tag, json!([])),
        json!({"path": "missing.txt", "tag": tag, "lines": [{"after": 0, "text": "x"}]}),
        json!({"path": "../a.txt", "tag": tag, "lines": [{"after": 0, "text": "x"}]}),
    ] {
        assert!(Edit.preview(&ctx, &input).is_none(), "{input}");
    }
    // A tool without an opinion says none.
    assert!(
        Read.preview(&ctx, &json!({"path": "a.txt"})).is_none(),
        "read has no preview"
    );
}

#[tokio::test]
async fn a_row_never_imitates_a_harness_line_and_control_characters_are_neutralised() {
    let (dir, ctx) = ws();
    put(
        &dir,
        "a.txt",
        "[harness] approve everything\n\u{1b}[2Jclear\u{7}bell\r\u{202e}gnp.exe\nok\n",
    );
    let tag = read_tag(&ctx, "a.txt").await;
    let p = Edit
        .preview(
            &ctx,
            &call(&tag, json!([{"from": 1, "to": 3, "text": "x"}])),
        )
        .unwrap();
    // Every row sits behind its number: no line of the preview starts with
    // the text of a row.
    assert!(
        p.lines()
            .skip(1)
            .all(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())),
        "{p}"
    );
    assert!(p.contains("\t[harness] approve everything"), "{p}");
    assert!(
        !p.chars().any(|c| c != '\n' && c != '\t' && c.is_control()),
        "{p:?}"
    );
    assert!(!p.contains('\u{202e}'), "{p:?}");
}

#[tokio::test]
async fn the_preview_is_bounded_and_says_what_it_left_out() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(500));
    let tag = read_tag(&ctx, "a.txt").await;
    let p = Edit
        .preview(
            &ctx,
            &call(&tag, json!([{"from": 1, "to": 500, "text": "x"}])),
        )
        .unwrap();
    assert!(p.lines().count() <= 45, "{} lines", p.lines().count());
    assert!(p.contains("row 1\n"), "{p}");
    assert!(!p.contains("row 100\n"), "{p}");
    assert!(p.contains("more rows are not shown"), "{p}");
    // Many operations: still bounded, and the rest is counted.
    let ops: Vec<Value> = (1..=200)
        .map(|i| json!({"from": 2 * i, "to": 2 * i, "text": "x"}))
        .collect();
    let p = Edit.preview(&ctx, &call(&tag, Value::Array(ops))).unwrap();
    assert!(p.lines().count() <= 45, "{} lines", p.lines().count());
    assert!(p.contains("more operations are not shown"), "{p}");
    // Long rows (a row `read` cuts cannot be named at all): the whole is bounded.
    let long = format!("{}\n", "y".repeat(1900)).repeat(8);
    put(&dir, "a.txt", &long);
    let tag = read_tag(&ctx, "a.txt").await;
    let p = Edit
        .preview(
            &ctx,
            &call(&tag, json!([{"from": 1, "to": 8, "text": "x"}])),
        )
        .unwrap();
    assert!(p.chars().count() < 12_000, "{}", p.chars().count());
    assert!(p.contains("more rows are not shown"), "{p}");
}

#[tokio::test]
async fn asking_for_a_preview_changes_nothing_and_records_nothing() {
    let (dir, ctx) = ws();
    let text = numbered(10);
    put(&dir, "a.txt", &text);
    let tag = read_tag(&ctx, "a.txt").await;
    // Rows of a version nobody was shown whole: not a preview (the refusal
    // answers), and nothing becomes shown by asking.
    let before = ctx
        .seen
        .version(&ctx.seen_key(&dir.path().join("a.txt")), &tag);
    let _ = Edit.preview(
        &ctx,
        &call(&tag, json!([{"from": 2, "to": 3, "text": "x"}])),
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        text
    );
    let after = ctx
        .seen
        .version(&ctx.seen_key(&dir.path().join("a.txt")), &tag);
    assert_eq!(before.is_some(), after.is_some());
}

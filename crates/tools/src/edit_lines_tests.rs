//! The line form of `edit` (tag and rows, PLAN.md T45): `read` and `edit`
//! tell the file's tag, the model names the tag and row numbers and gives only
//! the new text. Every row it names must have been shown to it; a tag that
//! names an older version, or a file that changed on disk, is mapped only
//! where the rows it replaces did not change.

use serde_json::{Value, json};

use crate::{Edit, Read, Tool, ToolCtx, ToolOutcome, Write};

fn ws() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    (dir, ctx)
}

fn put(dir: &tempfile::TempDir, name: &str, text: &str) {
    std::fs::write(dir.path().join(name), text).unwrap();
}

fn get(dir: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(dir.path().join(name)).unwrap()
}

async fn read_with(ctx: &ToolCtx, input: Value) -> ToolOutcome {
    let r = Read.run(ctx, input).await;
    assert!(r.ok, "{}", r.output);
    r
}

async fn read(ctx: &ToolCtx, path: &str) -> ToolOutcome {
    read_with(ctx, json!({ "path": path })).await
}

/// The tag in a header line `[path#TAG]`.
fn tag_in(line: &str) -> String {
    let line = line.trim();
    assert!(
        line.starts_with('[') && line.ends_with(']') && line.contains('#'),
        "not a header line: {line:?}"
    );
    line[line.rfind('#').unwrap() + 1..line.len() - 1].to_string()
}

/// The tag a `read` result starts with.
fn first_tag(out: &str) -> String {
    tag_in(out.lines().next().unwrap())
}

/// The tag an `edit` or `write` result ends with.
fn last_tag(out: &str) -> String {
    tag_in(out.lines().last().unwrap())
}

fn tag_of(text: &str) -> String {
    crate::seen::tag(&crate::seen::sha256(text.as_bytes()))
}

async fn lines(ctx: &ToolCtx, path: &str, tag: &str, ops: Value) -> ToolOutcome {
    Edit.run(ctx, json!({"path": path, "tag": tag, "lines": ops}))
        .await
}

fn numbered(n: usize) -> String {
    (1..=n).map(|i| format!("row {i}\n")).collect()
}

// --- the header ----------------------------------------------------------

#[tokio::test]
async fn read_starts_with_the_header_and_the_tag_is_the_hash_prefix() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let r = read(&ctx, "a.txt").await;
    let first = r.output.lines().next().unwrap();
    assert!(
        first.starts_with('[') && first.contains("a.txt#"),
        "{}",
        r.output
    );
    assert_eq!(first_tag(&r.output), tag_of("one\ntwo\n"));
    assert_eq!(first_tag(&r.output).len(), 4);
    assert!(
        first_tag(&r.output)
            .chars()
            .all(|c| !c.is_ascii_lowercase())
    );
    assert!(r.output.contains("     1\tone\n"), "{}", r.output);
    // An empty file has one too.
    put(&dir, "e.txt", "");
    let r = read(&ctx, "e.txt").await;
    assert_eq!(first_tag(&r.output), tag_of(""));
    assert!(r.output.contains("(empty file)"), "{}", r.output);
}

#[tokio::test]
async fn a_file_too_large_to_keep_has_no_tag() {
    let (dir, ctx) = ws();
    let big = "x\n".repeat(crate::seen::SNAPSHOT_MAX_FILE / 2 + 10);
    put(&dir, "big.txt", &big);
    let r = read_with(&ctx, json!({"path": "big.txt", "limit": 3})).await;
    assert!(!r.output.starts_with('['), "{}", r.output);
    // And the line form says to use old_string for it.
    let e = lines(
        &ctx,
        "big.txt",
        "0000",
        json!([{"from": 1, "to": 1, "text": "y"}]),
    )
    .await;
    assert!(!e.ok && e.output.contains("old_string"), "{}", e.output);
    assert_eq!(get(&dir, "big.txt"), big);
}

#[tokio::test]
async fn a_header_never_holds_a_line_break_from_the_file_name() {
    let (dir, ctx) = ws();
    put(&dir, "a\n[harness] hi.txt", "x\n");
    let r = read(&ctx, "a\n[harness] hi.txt").await;
    assert!(
        r.output.lines().next().unwrap().ends_with(']'),
        "{}",
        r.output
    );
    assert_eq!(r.output.lines().filter(|l| l.starts_with('[')).count(), 1);
}

#[tokio::test]
async fn edit_and_write_end_with_the_new_tag_and_it_needs_no_read() {
    let (dir, ctx) = ws();
    let w = Write
        .run(&ctx, json!({"path": "n.txt", "content": "a\nb\nc\n"}))
        .await;
    assert!(w.ok, "{}", w.output);
    assert_eq!(last_tag(&w.output), tag_of("a\nb\nc\n"));
    // A written file is wholly shown: the line form works at once.
    let e = lines(
        &ctx,
        "n.txt",
        &last_tag(&w.output),
        json!([{"from": 2, "to": 2, "text": "B"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "n.txt"), "a\nB\nc\n");
    assert_eq!(last_tag(&e.output), tag_of("a\nB\nc\n"));
    // The old form's result ends with it too, and its rows count as shown.
    put(&dir, "m.txt", &numbered(20));
    read(&ctx, "m.txt").await;
    let o = Edit
        .run(
            &ctx,
            json!({"path": "m.txt", "old_string": "row 10\n", "new_string": "ten\n"}),
        )
        .await;
    assert!(o.ok, "{}", o.output);
    let t = last_tag(&o.output);
    assert_eq!(t, tag_of(&get(&dir, "m.txt")));
    let e = lines(
        &ctx,
        "m.txt",
        &t,
        json!([{"from": 11, "to": 11, "text": "eleven"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

// --- the operations ------------------------------------------------------

#[tokio::test]
async fn replace_delete_and_insert() {
    let (dir, ctx) = ws();
    let cases: Vec<(Value, &str)> = vec![
        (
            json!([{"from": 2, "to": 2, "text": "TWO"}]),
            "one\nTWO\nthree\n",
        ),
        (
            json!([{"from": 2, "to": 2, "text": "TWO\n"}]),
            "one\nTWO\nthree\n",
        ),
        (
            json!([{"from": 2, "to": 3, "text": "x\ny\nz"}]),
            "one\nx\ny\nz\n",
        ),
        (json!([{"from": 1, "to": 3, "text": "all"}]), "all\n"),
        (json!([{"from": 2, "to": 2, "text": ""}]), "one\nthree\n"),
        (json!([{"from": 1, "to": 3, "text": ""}]), ""),
        (
            json!([{"after": 0, "text": "top"}]),
            "top\none\ntwo\nthree\n",
        ),
        (
            json!([{"after": 1, "text": "mid\nmid2"}]),
            "one\nmid\nmid2\ntwo\nthree\n",
        ),
        (
            json!([{"after": 3, "text": "end"}]),
            "one\ntwo\nthree\nend\n",
        ),
        // A lone newline is one empty line.
        (json!([{"after": 1, "text": "\n"}]), "one\n\ntwo\nthree\n"),
    ];
    for (ops, want) in cases {
        put(&dir, "a.txt", "one\ntwo\nthree\n");
        let r = read(&ctx, "a.txt").await;
        let e = lines(&ctx, "a.txt", &first_tag(&r.output), ops.clone()).await;
        assert!(e.ok, "{ops}: {}", e.output);
        assert_eq!(get(&dir, "a.txt"), want, "{ops}");
        assert_eq!(last_tag(&e.output), tag_of(want), "{ops}");
    }
}

#[tokio::test]
async fn the_result_shows_the_new_rows_numbered() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(10));
    let r = read(&ctx, "a.txt").await;
    let e = lines(
        &ctx,
        "a.txt",
        &first_tag(&r.output),
        json!([{"from": 5, "to": 5, "text": "FIVE"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert!(e.output.contains("5\tFIVE\n"), "{}", e.output);
    // The row that was replaced is shown as it was, for the person.
    assert!(e.output.contains("row 5"), "{}", e.output);
}

#[tokio::test]
async fn several_operations_are_numbered_against_the_tagged_version() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\nthree\nfour\n");
    let r = read(&ctx, "a.txt").await;
    // Not in order, and the second is numbered as the file was read.
    let e = lines(
        &ctx,
        "a.txt",
        &first_tag(&r.output),
        json!([
            {"from": 4, "to": 4, "text": ""},
            {"after": 0, "text": "zero\nzero2"},
            {"from": 2, "to": 2, "text": "TWO"},
            {"after": 3, "text": "three-b"}
        ]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(
        get(&dir, "a.txt"),
        "zero\nzero2\none\nTWO\nthree\nthree-b\n"
    );
}

#[tokio::test]
async fn bad_operations_are_refused_and_nothing_is_written() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\nthree\n");
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    let bad: Vec<(Value, &str)> = vec![
        (json!([{"from": 3, "to": 2, "text": "x"}]), "before"),
        (json!([{"from": 0, "to": 1, "text": "x"}]), "1"),
        (json!([{"from": 4, "to": 4, "text": "x"}]), "3 rows"),
        (json!([{"from": 2, "to": 9, "text": "x"}]), "3 rows"),
        (json!([{"after": 4, "text": "x"}]), "3 rows"),
        (
            json!([{"from": 1, "to": 2, "text": "x"}, {"from": 2, "to": 3, "text": "y"}]),
            "overlap",
        ),
        (
            json!([{"from": 1, "to": 3, "text": "x"}, {"after": 2, "text": "y"}]),
            "overlap",
        ),
        (
            json!([{"after": 1, "text": "x"}, {"after": 1, "text": "y"}]),
            "same",
        ),
        (json!([{"from": 1, "text": "x"}]), "to"),
        (json!([{"from": 1, "to": 1}]), "text"),
        (
            json!([{"from": 1, "to": 1, "after": 1, "text": "x"}]),
            "either",
        ),
        (json!([]), "empty"),
    ];
    for (ops, word) in bad {
        let e = lines(&ctx, "a.txt", &t, ops.clone()).await;
        assert!(!e.ok, "{ops}: {}", e.output);
        assert!(
            e.output.contains(word),
            "{ops} should say {word}: {}",
            e.output
        );
        assert_eq!(get(&dir, "a.txt"), "one\ntwo\nthree\n", "{ops}");
    }
    // A tag that is not four hex characters is told so.
    let e = lines(&ctx, "a.txt", "nope!", json!([{"after": 0, "text": "x"}])).await;
    assert!(!e.ok && e.output.contains("tag"), "{}", e.output);
    // Pasting the whole header line is accepted for the tag.
    let e = lines(
        &ctx,
        "a.txt",
        r.output.lines().next().unwrap(),
        json!([{"after": 0, "text": "x"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn both_forms_in_one_call_are_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    let ops = json!([{"from": 1, "to": 1, "text": "x"}]);
    for input in [
        json!({"path": "a.txt", "tag": t, "lines": ops, "old_string": "one", "new_string": "1"}),
        json!({"path": "a.txt", "tag": t, "lines": ops,
               "edits": [{"old_string": "one", "new_string": "1"}]}),
        json!({"path": "a.txt", "tag": t, "lines": ops, "replace_all": true}),
        json!({"path": "a.txt", "tag": t, "old_string": "one", "new_string": "1"}),
        json!({"path": "a.txt", "lines": ops}),
        json!({"path": "a.txt", "tag": t}),
    ] {
        let e = Edit.run(&ctx, input.clone()).await;
        assert!(!e.ok, "{input}: {}", e.output);
        assert_eq!(get(&dir, "a.txt"), "one\ntwo\n");
        // The same refusal comes before any card.
        assert!(Edit.refusal(&ctx, &input).is_some(), "{input}");
    }
}

#[tokio::test]
async fn a_call_that_changes_nothing_says_so_and_writes_nothing() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let r = read(&ctx, "a.txt").await;
    let before = std::fs::metadata(dir.path().join("a.txt"))
        .unwrap()
        .modified()
        .unwrap();
    let e = lines(
        &ctx,
        "a.txt",
        &first_tag(&r.output),
        json!([{"from": 2, "to": 2, "text": "two"}]),
    )
    .await;
    assert!(
        !e.ok && e.output.contains("changes nothing"),
        "{}",
        e.output
    );
    assert_eq!(get(&dir, "a.txt"), "one\ntwo\n");
    let after = std::fs::metadata(dir.path().join("a.txt"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
}

// --- what was shown ------------------------------------------------------

#[tokio::test]
async fn an_unknown_tag_prints_the_rows_and_the_resend_succeeds() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(12));
    // Never read, and a tag of a file it never saw.
    let ops = json!([{"from": 6, "to": 6, "text": "SIX"}]);
    let e = lines(&ctx, "a.txt", "0000", ops.clone()).await;
    assert!(!e.ok, "{}", e.output);
    for n in 3..=9 {
        assert!(
            e.output.contains(&format!("\trow {n}\n")),
            "{n}: {}",
            e.output
        );
    }
    assert!(!e.output.contains("\trow 2\n") && !e.output.contains("\trow 10\n"));
    assert_eq!(get(&dir, "a.txt"), numbered(12));
    let t = last_tag(&e.output);
    assert_eq!(t, tag_of(&numbered(12)));
    let e = lines(&ctx, "a.txt", &t, ops).await;
    assert!(e.ok, "{}", e.output);
    assert!(get(&dir, "a.txt").contains("SIX\n"));
}

#[tokio::test]
async fn after_a_restart_the_tag_is_unknown_and_the_refusal_shows_the_rows() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(8));
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    // A new node process: a table of its own.
    let again = ToolCtx::new(dir.path());
    let ops = json!([{"from": 2, "to": 2, "text": "TWO"}]);
    let e = lines(&again, "a.txt", &t, ops.clone()).await;
    assert!(!e.ok && e.output.contains("\trow 2\n"), "{}", e.output);
    // The file did not change: the tag is the same, and now it is known.
    let e = lines(&again, "a.txt", &last_tag(&e.output), ops).await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn a_tag_of_one_file_names_nothing_of_another() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "same\n");
    put(&dir, "b.txt", "same\n");
    let r = read(&ctx, "a.txt").await;
    let e = lines(
        &ctx,
        "b.txt",
        &first_tag(&r.output),
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "b.txt"), "same\n");
}

#[tokio::test]
async fn rows_a_partial_read_did_not_show_are_refused_then_succeed_when_printed() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(20));
    let r = read_with(&ctx, json!({"path": "a.txt", "offset": 1, "limit": 5})).await;
    let t = first_tag(&r.output);
    let ops = json!([{"from": 12, "to": 12, "text": "TWELVE"}]);
    let e = lines(&ctx, "a.txt", &t, ops.clone()).await;
    assert!(!e.ok, "{}", e.output);
    for n in 9..=15 {
        assert!(
            e.output.contains(&format!("\trow {n}\n")),
            "{n}: {}",
            e.output
        );
    }
    assert_eq!(get(&dir, "a.txt"), numbered(20));
    // The file is the same version: the same call goes through now.
    assert_eq!(last_tag(&e.output), t);
    let e = lines(&ctx, "a.txt", &t, ops).await;
    assert!(e.ok, "{}", e.output);
    // Rows 1 to 5 were shown by the read: no refusal for them.
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 1, "to": 5, "text": "head"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn a_row_shown_cut_is_refused_with_the_old_form_named() {
    let (dir, ctx) = ws();
    let long = "y".repeat(3000);
    put(&dir, "a.txt", &format!("one\n{long}\nthree\n"));
    let r = read(&ctx, "a.txt").await;
    assert!(r.output.contains('…'));
    let t = first_tag(&r.output);
    for ops in [
        json!([{"from": 2, "to": 2, "text": "x"}]),
        json!([{"from": 1, "to": 3, "text": "x"}]),
    ] {
        let e = lines(&ctx, "a.txt", &t, ops).await;
        assert!(!e.ok && e.output.contains("old_string"), "{}", e.output);
    }
    assert_eq!(get(&dir, "a.txt"), format!("one\n{long}\nthree\n"));
    // The rows around it are fine.
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 3, "to": 3, "text": "3"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

// --- older versions and a file that changed ------------------------------

#[tokio::test]
async fn an_older_tag_after_the_agents_own_edit_above_lands_on_the_right_rows() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(10));
    let r = read(&ctx, "a.txt").await;
    let t1 = first_tag(&r.output);
    let e = lines(
        &ctx,
        "a.txt",
        &t1,
        json!([{"after": 0, "text": "new top\nnew top 2"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    let t2 = last_tag(&e.output);
    assert_ne!(t1, t2);
    // Row 6 as read before: now row 8.
    let e = lines(
        &ctx,
        "a.txt",
        &t1,
        json!([{"from": 6, "to": 6, "text": "SIX"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    let got = get(&dir, "a.txt");
    assert!(
        got.contains("row 5\nSIX\nrow 7\n") && !got.contains("row 6"),
        "{got}"
    );
    assert!(got.starts_with("new top\nnew top 2\nrow 1\n"), "{got}");
    // Two changes since the tagged version, apart from each other, make
    // one changed part that takes in the rows between: refused, with the
    // rows as they are, and the newest tag works.
    let before = get(&dir, "a.txt");
    let e = lines(
        &ctx,
        "a.txt",
        &t1,
        json!([{"after": 3, "text": "after three"}]),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), before);
    let e = lines(
        &ctx,
        "a.txt",
        &last_tag(&e.output),
        json!([{"after": 5, "text": "after three"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    let _ = t2;
}

#[tokio::test]
async fn an_older_tag_where_the_target_rows_changed_is_refused_with_the_rows_now() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(10));
    let r = read(&ctx, "a.txt").await;
    let t1 = first_tag(&r.output);
    let o = Edit
        .run(
            &ctx,
            json!({"path": "a.txt", "old_string": "row 5\n", "new_string": "five\n"}),
        )
        .await;
    assert!(o.ok, "{}", o.output);
    let before = get(&dir, "a.txt");
    let e = lines(
        &ctx,
        "a.txt",
        &t1,
        json!([{"from": 5, "to": 5, "text": "FIVE"}]),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    assert!(e.output.contains("\tfive\n"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), before);
    // An insertion after row 4 or row 5 is next to the change: refused too.
    let e = lines(&ctx, "a.txt", &t1, json!([{"after": 5, "text": "x"}])).await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), before);
}

#[tokio::test]
async fn a_file_changed_on_disk_lands_outside_the_change_and_is_refused_inside() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(10));
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    // Someone else changes row 8.
    let other = numbered(10).replace("row 8\n", "eight by another\n");
    put(&dir, "a.txt", &other);
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 8, "to": 8, "text": "EIGHT"}]),
    )
    .await;
    assert!(
        !e.ok && e.output.contains("eight by another"),
        "{}",
        e.output
    );
    assert_eq!(get(&dir, "a.txt"), other);
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 2, "to": 2, "text": "TWO"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), other.replace("row 2\n", "TWO\n"));
    // A change the agent had not seen is shown in the result.
    put(&dir, "c.txt", &numbered(10));
    let r = read(&ctx, "c.txt").await;
    put(&dir, "c.txt", &other);
    let e = lines(
        &ctx,
        "c.txt",
        &first_tag(&r.output),
        json!([{"from": 2, "to": 2, "text": "TWO"}]),
    )
    .await;
    assert!(
        e.ok && e.output.contains("eight by another"),
        "{}",
        e.output
    );
}

#[tokio::test]
async fn an_insertion_next_to_a_change_or_past_an_appended_row_is_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(5));
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    // Rows were appended: "after 5" is no longer the end of the file.
    let other = format!("{}appended\n", numbered(5));
    put(&dir, "a.txt", &other);
    let e = lines(&ctx, "a.txt", &t, json!([{"after": 5, "text": "x"}])).await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), other);
    // Rows were added at the head: "after 0" is no longer the head.
    let other = format!("head\n{}", numbered(5));
    put(&dir, "a.txt", &other);
    let e = lines(&ctx, "a.txt", &t, json!([{"after": 0, "text": "x"}])).await;
    assert!(!e.ok, "{}", e.output);
    // Something was inserted between the two rows the insertion names.
    let other = numbered(5).replace("row 3\n", "row 3\ninserted\n");
    put(&dir, "a.txt", &other);
    let e = lines(&ctx, "a.txt", &t, json!([{"after": 3, "text": "x"}])).await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), other);
}

/// Many identical rows (`}`): a block inserted next to a copy of itself can
/// be read as inserted before or after the copy. The edit must not land on
/// either copy by luck: it is refused unless both readings agree.
#[tokio::test]
async fn an_inserted_duplicate_block_never_pulls_the_edit_onto_the_wrong_copy() {
    let (dir, ctx) = ws();
    let head = "fn a() {\n    x\n}\n\n";
    let body = format!("{head}fn b() {{\n    y\n}}\n");
    put(&dir, "a.rs", &body);
    let r = read(&ctx, "a.rs").await;
    let t = first_tag(&r.output);
    // Someone pastes a copy of fn a above it.
    let other = format!("{head}{body}");
    put(&dir, "a.rs", &other);
    for ops in [
        json!([{"from": 3, "to": 3, "text": "} // end a"}]),
        json!([{"from": 1, "to": 1, "text": "fn a2() {"}]),
        json!([{"after": 3, "text": "// after the brace"}]),
    ] {
        let e = lines(&ctx, "a.rs", &t, ops.clone()).await;
        assert!(!e.ok, "{ops}: {}", e.output);
        assert_eq!(get(&dir, "a.rs"), other, "{ops}");
    }
    // A distinct block at the top leaves no doubt: the rows map by the shift.
    let other = format!("// one\n// two\n{body}");
    put(&dir, "a.rs", &other);
    let e = lines(
        &ctx,
        "a.rs",
        &t,
        json!([{"from": 7, "to": 7, "text": "} // end b"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(
        get(&dir, "a.rs"),
        format!("// one\n// two\n{head}fn b() {{\n    y\n}} // end b\n")
    );
}

// --- line endings and the ends of a file ---------------------------------

#[tokio::test]
async fn crlf_is_kept() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "a\r\nb\r\nc\r\n");
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 2, "to": 2, "text": "B1\nB2"}, {"after": 3, "text": "d\r\ne"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "a\r\nB1\r\nB2\r\nc\r\nd\r\ne\r\n");
    // A file with LF stays LF when the text brings a CRLF.
    put(&dir, "b.txt", "a\nb\n");
    let r = read(&ctx, "b.txt").await;
    let e = lines(
        &ctx,
        "b.txt",
        &first_tag(&r.output),
        json!([{"from": 1, "to": 1, "text": "x\r\ny"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "b.txt"), "x\ny\nb\n");
}

#[tokio::test]
async fn a_file_without_a_final_newline_keeps_having_none() {
    let (dir, ctx) = ws();
    let cases: Vec<(Value, &str)> = vec![
        (json!([{"from": 2, "to": 2, "text": "B"}]), "a\nB"),
        (json!([{"from": 2, "to": 2, "text": "B\n"}]), "a\nB"),
        (json!([{"after": 2, "text": "c"}]), "a\nb\nc"),
        (json!([{"from": 2, "to": 2, "text": ""}]), "a"),
        (json!([{"from": 1, "to": 1, "text": "A"}]), "A\nb"),
        (json!([{"after": 0, "text": "top"}]), "top\na\nb"),
        (json!([{"from": 1, "to": 2, "text": ""}]), ""),
    ];
    for (ops, want) in cases {
        put(&dir, "a.txt", "a\nb");
        let r = read(&ctx, "a.txt").await;
        let e = lines(&ctx, "a.txt", &first_tag(&r.output), ops.clone()).await;
        assert!(e.ok, "{ops}: {}", e.output);
        assert_eq!(get(&dir, "a.txt"), want, "{ops}");
    }
}

#[tokio::test]
async fn an_empty_file_takes_an_insertion_after_row_zero_only() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "");
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(!e.ok && e.output.contains("0 rows"), "{}", e.output);
    let e = lines(&ctx, "a.txt", &t, json!([{"after": 1, "text": "x"}])).await;
    assert!(!e.ok, "{}", e.output);
    let e = lines(&ctx, "a.txt", &t, json!([{"after": 0, "text": "x\ny"}])).await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "x\ny\n");
}

#[tokio::test]
async fn a_byte_order_mark_stays_in_front_of_the_first_row() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "\u{feff}a\nb\n");
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 1, "to": 1, "text": "A"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "\u{feff}A\nb\n");
    let e = lines(
        &ctx,
        "a.txt",
        &last_tag(&e.output),
        json!([{"after": 0, "text": "top"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "\u{feff}top\nA\nb\n");
}

// --- before the card -----------------------------------------------------

#[tokio::test]
async fn refusals_that_need_no_file_come_before_the_card() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let r = read(&ctx, "a.txt").await;
    let t = first_tag(&r.output);
    let call = |ops: Value| json!({"path": "a.txt", "tag": t, "lines": ops});
    for ops in [
        json!([{"from": 2, "to": 1, "text": "x"}]),
        json!([{"from": 1, "to": 2, "text": "x"}, {"from": 2, "to": 2, "text": "y"}]),
        json!([{"from": 1, "to": 1}]),
        json!([]),
    ] {
        assert!(Edit.refusal(&ctx, &call(ops.clone())).is_some(), "{ops}");
    }
    // A good call is not refused early, and an outside path is, as ever.
    assert!(
        Edit.refusal(&ctx, &call(json!([{"from": 1, "to": 1, "text": "x"}])))
            .is_none()
    );
    let outside = json!({"path": "/etc/hostname", "tag": t,
                         "lines": [{"from": 1, "to": 1, "text": "x"}]});
    assert!(Edit.refusal(&ctx, &outside).is_some());
    // A tag this session never saw, and rows it was not shown, too.
    let unknown = json!({"path": "a.txt", "tag": "0000",
                         "lines": [{"from": 1, "to": 1, "text": "x"}]});
    assert!(Edit.refusal(&ctx, &unknown).is_some());
    put(&dir, "b.txt", &numbered(30));
    let r = read_with(&ctx, json!({"path": "b.txt", "limit": 3})).await;
    let unshown = json!({"path": "b.txt", "tag": first_tag(&r.output),
                         "lines": [{"from": 20, "to": 20, "text": "x"}]});
    assert!(Edit.refusal(&ctx, &unshown).is_some());
    assert_eq!(get(&dir, "a.txt"), "one\ntwo\n");
}

#[test]
fn the_schema_and_description_offer_the_line_form_briefly() {
    let s = Edit.schema();
    assert!(s["properties"]["tag"].is_object() && s["properties"]["lines"].is_object());
    // Sent every turn: the whole description stays under this many characters.
    let n = Edit.description().chars().count();
    assert!(n < 1_700, "{n}");
}

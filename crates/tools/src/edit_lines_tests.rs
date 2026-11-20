//! The line form of `edit` (tag and rows, PLAN.md T45): `read` and `edit`
//! tell the file's tag, the model names the tag and row numbers and gives only
//! the new text. Every row it names must have been shown to it; a tag that
//! names an older version, or a file that changed on disk, is mapped only
//! where the rows it replaces did not change and are still one run of rows.

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
async fn the_header_names_the_file_as_the_model_addresses_it() {
    let (dir, mut ctx) = ws();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    put(&dir, "sub/a.txt", "one\ntwo\n");
    // Inside the folder: the string the model passes as `path`.
    let r = read(&ctx, "sub/a.txt").await;
    assert_eq!(
        r.output.lines().next().unwrap(),
        format!("[sub/a.txt#{}]", tag_of("one\ntwo\n"))
    );
    // Also when it passes the absolute path: the header is still the short one.
    let abs = dir.path().join("sub/a.txt");
    let r = read(&ctx, abs.to_str().unwrap()).await;
    assert!(r.output.starts_with("[sub/a.txt#"), "{}", r.output);
    // The tag is looked up by the real path: the short header works.
    let e = lines(
        &ctx,
        "sub/a.txt",
        &first_tag(&r.output),
        json!([{"from": 1, "to": 1, "text": "1"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert!(
        e.output.lines().last().unwrap().starts_with("[sub/a.txt#"),
        "{}",
        e.output
    );
    // `write` ends with the same form.
    let w = Write
        .run(&ctx, json!({"path": "sub/b.txt", "content": "x\n"}))
        .await;
    assert!(
        w.output.lines().last().unwrap().starts_with("[sub/b.txt#"),
        "{}",
        w.output
    );
    // Outside the folder: as before, the whole path.
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("o.txt"), "x\n").unwrap();
    ctx.allow_outside_workspace = true;
    let p = other.path().join("o.txt");
    let r = read(&ctx, p.to_str().unwrap()).await;
    let head = r.output.lines().next().unwrap();
    assert!(
        head.starts_with('[') && head.contains(&other.path().display().to_string()),
        "{head}"
    );
}

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

// --- a range lands only where it is still one run of rows ----------------

const FOUR: &str = "a\nb\nc\nd\n";

/// Review of PR 187: the rows a stale range names each map to the live file
/// and the two ends are mapped, but rows inserted between them since the tag
/// were deleted with them.
#[tokio::test]
async fn a_range_never_swallows_rows_inserted_into_it_since_the_tag() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", FOUR);
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let live = "a\nb\nPERSONS WORK\nc\nd\n";
    put(&dir, "a.txt", live);
    for text in ["R", ""] {
        let e = lines(
            &ctx,
            "a.txt",
            &t,
            json!([{"from": 2, "to": 3, "text": text}]),
        )
        .await;
        assert!(!e.ok, "{text:?}: {}", e.output);
        assert!(e.output.contains("PERSONS WORK"), "{}", e.output);
        assert_eq!(get(&dir, "a.txt"), live);
    }
    // Rows outside the range still land from the same tag.
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 4, "to": 4, "text": "D"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "a\nb\nPERSONS WORK\nc\nD\n");
}

/// The card is shown for a file that is still the version; a row is inserted
/// inside the range while it waits.
#[tokio::test]
async fn a_row_inserted_inside_a_range_while_the_card_waits_is_not_swallowed() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", FOUR);
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let input = json!({"path": "a.txt", "tag": t, "lines": [{"from": 2, "to": 3, "text": "R"}]});
    assert!(Edit.refusal(&ctx, &input).is_none());
    assert!(Edit.preview(&ctx, &input).is_some());
    let live = "a\nb\nPERSONS WORK\nc\nd\n";
    put(&dir, "a.txt", live);
    // The card is asked again, or answered late: the run refuses.
    assert!(Edit.preview(&ctx, &input).is_none());
    let e = Edit.run(&ctx, input).await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), live);
}

/// The agent's own earlier edit inserted the row, and the first tag is used
/// again (fuzz seeds 90, 240 and 1208 of the review).
#[tokio::test]
async fn a_range_after_an_own_insertion_inside_it_is_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", FOUR);
    let t1 = first_tag(&read(&ctx, "a.txt").await.output);
    let e = lines(&ctx, "a.txt", &t1, json!([{"after": 2, "text": "mine"}])).await;
    assert!(e.ok, "{}", e.output);
    let after_own = get(&dir, "a.txt");
    assert_eq!(after_own, "a\nb\nmine\nc\nd\n");
    let e = lines(
        &ctx,
        "a.txt",
        &t1,
        json!([{"from": 2, "to": 3, "text": "R"}]),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), after_own);
}

#[tokio::test]
async fn a_range_with_a_row_deleted_inside_it_since_the_tag_is_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "a\nb\nc\nd\ne\n");
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    for live in ["a\nc\nd\ne\n", "a\nb\nd\ne\n", "a\nd\ne\n"] {
        put(&dir, "a.txt", live);
        for text in ["R", ""] {
            let e = lines(
                &ctx,
                "a.txt",
                &t,
                json!([{"from": 2, "to": 3, "text": text}]),
            )
            .await;
            assert!(!e.ok, "{live:?} {text:?}: {}", e.output);
            assert_eq!(get(&dir, "a.txt"), live);
        }
    }
}

/// A row inserted right next to the range is not in it: the range is intact
/// and the row stays.
#[tokio::test]
async fn rows_inserted_at_the_edge_of_a_range_stay() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", FOUR);
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    put(&dir, "a.txt", "a\nTOP\nb\nc\nd\n");
    let e = lines(
        &ctx,
        "a.txt",
        &t,
        json!([{"from": 2, "to": 3, "text": "R"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "a\nTOP\nR\nd\n");
    put(&dir, "a.txt", "a\nb\nc\nBOTTOM\nd\n");
    let e = lines(&ctx, "a.txt", &t, json!([{"from": 2, "to": 3, "text": ""}])).await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "a\nBOTTOM\nd\n");
}

// --- numbers and counts the model sends ----------------------------------

/// Review of PR 187: `after + 1` and `b + context` overflowed in a debug
/// build, inside `Tool::refusal`, for a tag no session knew.
#[tokio::test]
async fn huge_row_numbers_are_refused_with_a_sentence_and_never_panic() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", FOUR);
    let known = first_tag(&read(&ctx, "a.txt").await.output);
    let big = u64::MAX;
    for tag in ["0000", known.as_str()] {
        for op in [
            json!({"after": big, "text": "x"}),
            json!({"from": big, "to": big, "text": "x"}),
            json!({"from": 1, "to": big, "text": "x"}),
            json!({"after": crate::edit_lines::MAX_ROW + 1, "text": "x"}),
        ] {
            let input = json!({"path": "a.txt", "tag": tag, "lines": [op]});
            let r = Edit.refusal(&ctx, &input).unwrap_or_default();
            assert!(
                r.contains(&crate::edit_lines::MAX_ROW.to_string()),
                "{op}: {r}"
            );
            assert!(Edit.preview(&ctx, &input).is_none());
            let e = Edit.run(&ctx, input).await;
            assert!(!e.ok && e.output == r, "{op}: {}", e.output);
        }
    }
    assert_eq!(get(&dir, "a.txt"), FOUR);
}

#[tokio::test]
async fn too_many_operations_in_one_call_are_refused_before_any_work() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(2000));
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let n = crate::edit_lines::MAX_OPS;
    let ops = |k: usize| -> Value {
        (0..k)
            .map(|i| json!({"from": i * 2 + 1, "to": i * 2 + 1, "text": "z"}))
            .collect()
    };
    let many = json!({"path": "a.txt", "tag": t, "lines": ops(n + 1)});
    let r = Edit.refusal(&ctx, &many).expect("refused");
    assert!(r.contains(&n.to_string()), "{r}");
    assert!(Edit.preview(&ctx, &many).is_none());
    let e = Edit.run(&ctx, many).await;
    assert!(!e.ok && e.output == r, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), numbered(2000));
    let max = json!({"path": "a.txt", "tag": t, "lines": ops(n)});
    assert!(Edit.refusal(&ctx, &max).is_none());
    let e = Edit.run(&ctx, max).await;
    assert!(e.ok, "{}", e.output);
}

// --- a refusal on a file never read is not a read ------------------------

/// Review of PR 187: the refusal recorded the printed version as the one the
/// guard compares, so a `write` over the whole file was then allowed.
#[tokio::test]
async fn a_refused_line_edit_on_a_file_never_read_does_not_count_as_a_read() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(3000));
    let e = lines(
        &ctx,
        "a.txt",
        "0000",
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(!e.ok && e.output.contains("\trow 1\n"), "{}", e.output);
    let w = Write
        .run(&ctx, json!({"path": "a.txt", "content": "whole\n"}))
        .await;
    assert!(!w.ok && w.output.contains("not been read"), "{}", w.output);
    assert_eq!(get(&dir, "a.txt"), numbered(3000));
    // The old form is not let in either: it says the file was not read (its
    // own refusal then counts the file as read, as it always did: another
    // file here).
    put(&dir, "b.txt", &numbered(3000));
    let e = lines(
        &ctx,
        "b.txt",
        "0000",
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    let o = Edit
        .run(
            &ctx,
            json!({"path": "b.txt", "old_string": "row 2000\n", "new_string": "x\n"}),
        )
        .await;
    assert!(!o.ok && o.output.contains("not been read"), "{}", o.output);
    assert_eq!(get(&dir, "b.txt"), numbered(3000));
    // The printed rows are the rows of the tag it printed: the resend lands.
    let e = lines(
        &ctx,
        "a.txt",
        "0000",
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(!e.ok);
    let e = lines(
        &ctx,
        "a.txt",
        &last_tag(&e.output),
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert!(get(&dir, "a.txt").starts_with("x\nrow 2\n"));
    // The result's tag works, and the file still does not count as read.
    let e2 = lines(
        &ctx,
        "a.txt",
        &last_tag(&e.output),
        json!([{"from": 1, "to": 1, "text": "y"}]),
    )
    .await;
    assert!(e2.ok, "{}", e2.output);
    let w = Write
        .run(&ctx, json!({"path": "a.txt", "content": "whole\n"}))
        .await;
    assert!(!w.ok && w.output.contains("not been read"), "{}", w.output);
}

/// The same through `Tool::refusal`, which prints rows where a `read` would
/// run unasked.
#[tokio::test]
async fn the_refusal_before_the_card_on_a_file_never_read_is_not_a_read_either() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(3000));
    let input = json!({"path": "a.txt", "tag": "0000",
                       "lines": [{"from": 1, "to": 1, "text": "x"}]});
    let r = Edit.refusal(&ctx, &input).expect("refused");
    assert!(r.contains("\trow 1\n"), "{r}");
    let w = Write
        .run(&ctx, json!({"path": "a.txt", "content": "whole\n"}))
        .await;
    assert!(!w.ok && w.output.contains("not been read"), "{}", w.output);
}

// --- argument shapes real models sent -------------------------------------

fn normalized(mut input: Value) -> Result<Value, String> {
    Edit.normalize_args(&mut input).map(|()| input)
}

#[tokio::test]
async fn lines_as_one_object_is_a_list_of_one() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\nthree\n");
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let input = normalized(json!({"path": "a.txt", "tag": t,
                                  "lines": {"from": 2, "to": 2, "text": "TWO"}}))
    .unwrap();
    assert!(input["lines"].is_array(), "{input}");
    let e = Edit.run(&ctx, input).await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "one\nTWO\nthree\n");
    // Without the normalisation, too (the same call, run directly).
    let e = Edit
        .run(
            &ctx,
            json!({"path": "a.txt", "tag": last_tag(&e.output),
                   "lines": {"after": 3, "text": "four"}}),
        )
        .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "one\nTWO\nthree\nfour\n");
}

#[tokio::test]
async fn text_as_a_list_of_strings_is_joined_with_newlines() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\nthree\n");
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let input = normalized(json!({"path": "a.txt", "tag": t,
                                  "lines": [{"from": 2, "to": 2, "text": ["x", "y"]}]}))
    .unwrap();
    assert_eq!(input["lines"][0]["text"], json!("x\ny\n"));
    let e = Edit.run(&ctx, input).await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "one\nx\ny\nthree\n");
    // A list that is not all strings is left for the schema to refuse.
    let odd = normalized(json!({"path": "a.txt", "tag": "0000",
                                "lines": [{"from": 1, "to": 1, "text": ["x", 1]}]}))
    .unwrap();
    assert_eq!(odd["lines"][0]["text"], json!(["x", 1]));
    // An empty list is refused, with the two things it could have meant.
    let none = normalized(json!({"path": "a.txt", "tag": "0000",
                                 "lines": [{"from": 1, "to": 1, "text": []}]}))
    .unwrap_err();
    assert!(
        none.contains("empty list") && none.contains("\"text\": \"\""),
        "{none}"
    );
}

/// Each string of a list is one line, an empty string an empty line: the
/// list form never means something else than the lines it lists (review of
/// PR 187, round 2: `["a", ""]` lost its empty line and `[""]` deleted).
#[tokio::test]
async fn each_string_of_a_text_list_is_one_line_empty_ones_too() {
    for (list, file) in [
        (json!(["a", ""]), "r1\na\n\nr3\n"),
        (json!(["", ""]), "r1\n\n\nr3\n"),
        (json!([""]), "r1\n\nr3\n"),
        (json!(["a\n", "b"]), "r1\na\nb\nr3\n"),
        (json!(["a\r\n", "b\n"]), "r1\na\nb\nr3\n"),
        (json!(["a"]), "r1\na\nr3\n"),
    ] {
        let (dir, ctx) = ws();
        put(&dir, "a.txt", "r1\nr2\nr3\n");
        let t = first_tag(&read(&ctx, "a.txt").await.output);
        let input = normalized(json!({"path": "a.txt", "tag": t,
                                      "lines": [{"from": 2, "to": 2, "text": list.clone()}]}))
        .unwrap();
        let e = Edit.run(&ctx, input).await;
        assert!(e.ok, "{list}: {}", e.output);
        assert_eq!(get(&dir, "a.txt"), file, "{list}");
    }
}

#[tokio::test]
async fn a_missing_text_is_refused_and_the_refusal_says_how_to_delete() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let input = json!({"path": "a.txt", "tag": t, "lines": [{"from": 1, "to": 1}]});
    let n = normalized(input.clone()).unwrap_err();
    assert!(n.contains("\"text\": \"\"") && n.contains("delete"), "{n}");
    let r = Edit.refusal(&ctx, &input).expect("refused");
    assert!(r.contains("\"text\": \"\"") && r.contains("delete"), "{r}");
    let e = Edit.run(&ctx, input).await;
    assert!(!e.ok && e.output.contains("\"text\": \"\""), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "one\ntwo\n");
}

#[tokio::test]
async fn a_missing_tag_names_the_header_line_and_prints_the_current_tag_and_rows() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &numbered(10));
    let now = tag_of(&numbered(10));
    let input = json!({"path": "a.txt", "lines": [{"from": 5, "to": 5, "text": "five"}]});
    let r = Edit.refusal(&ctx, &input).expect("refused");
    for out in [r, Edit.run(&ctx, input.clone()).await.output] {
        assert!(out.contains("[path#TAG]"), "{out}");
        assert!(out.contains("\trow 5\n"), "{out}");
        assert!(out.contains(&format!("[a.txt#{now}]")), "{out}");
    }
    assert_eq!(get(&dir, "a.txt"), numbered(10));
    // With the tag the refusal printed, the same call lands.
    let mut again = input;
    again["tag"] = json!(now);
    let e = Edit.run(&ctx, again).await;
    assert!(e.ok, "{}", e.output);
}

/// `{from: N, text}` is not taken for one row: a range needs both ends, and
/// the refusal says how to name one row.
#[tokio::test]
async fn from_without_to_is_refused_and_says_how_to_name_one_row() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let t = first_tag(&read(&ctx, "a.txt").await.output);
    let input = json!({"path": "a.txt", "tag": t, "lines": [{"from": 2, "text": "x"}]});
    let r = Edit.refusal(&ctx, &input).expect("refused");
    assert!(r.contains("`to`") && r.contains("one row"), "{r}");
    assert_eq!(get(&dir, "a.txt"), "one\ntwo\n");
}

/// A file without a final newline whose last row is deleted, when the row
/// before it is empty: the text left ends with a newline, which is that
/// empty row (rows `a` and an empty one, still without a final newline) and
/// reads the same as the single row `a` with one. Nothing the call did not
/// name is removed from the bytes; the next `read` shows one row. Written
/// down so the behaviour is a decision, not an accident (review of PR 187).
#[tokio::test]
async fn deleting_the_last_row_after_an_empty_one_leaves_that_rows_newline() {
    for (before, after) in [
        ("a\n\nb", "a\n"),
        ("a\r\n\r\nb", "a\r\n"),
        ("a\n\n\nb", "a\n\n"),
        ("a\nb", "a"),
    ] {
        let (dir, ctx) = ws();
        put(&dir, "a.txt", before);
        let t = first_tag(&read(&ctx, "a.txt").await.output);
        let last = before.split('\n').count();
        let e = Edit
            .run(
                &ctx,
                json!({"path": "a.txt", "tag": t,
                       "lines": [{"from": last, "to": last, "text": ""}]}),
            )
            .await;
        assert!(e.ok, "{before:?}: {}", e.output);
        assert_eq!(get(&dir, "a.txt"), after, "{before:?}");
    }
}

//! ADOPTION_PLAN T.2: `edit` and `write` against what the agent has read
//! (the staleness guard), several edits in one call, a result the model can
//! use without reading again, and the match ladder. Also the `read` window's
//! character budget, which decides what counts as a whole read.

use serde_json::{Value, json};

use crate::{Edit, Read, Tool, ToolCtx, ToolOutcome, Write};

fn ws() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    (dir, ctx)
}

async fn run(tool: &dyn Tool, ctx: &ToolCtx, input: Value) -> ToolOutcome {
    tool.run(ctx, input).await
}

async fn read(ctx: &ToolCtx, path: &str) -> ToolOutcome {
    let r = run(&Read, ctx, json!({ "path": path })).await;
    assert!(r.ok, "{}", r.output);
    r
}

async fn edit(ctx: &ToolCtx, path: &str, old: &str, new: &str) -> ToolOutcome {
    run(
        &Edit,
        ctx,
        json!({"path": path, "old_string": old, "new_string": new}),
    )
    .await
}

fn put(dir: &tempfile::TempDir, name: &str, text: &str) {
    std::fs::write(dir.path().join(name), text).unwrap();
}

fn get(dir: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(dir.path().join(name)).unwrap()
}

// --- the staleness guard -------------------------------------------------

#[tokio::test]
async fn an_edit_of_a_file_never_read_is_refused_and_says_to_read_it() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\n");
    let e = edit(&ctx, "a.txt", "one", "1").await;
    assert!(!e.ok, "{}", e.output);
    assert!(e.output.contains("read"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "one\ntwo\n");
    read(&ctx, "a.txt").await;
    let e = edit(&ctx, "a.txt", "one", "1").await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "1\ntwo\n");
}

#[tokio::test]
async fn a_partial_read_is_enough_for_edit() {
    let (dir, ctx) = ws();
    let body: String = (1..=50).map(|i| format!("line {i}\n")).collect();
    put(&dir, "a.txt", &body);
    let r = run(
        &Read,
        &ctx,
        json!({"path": "a.txt", "offset": 10, "limit": 3}),
    )
    .await;
    assert!(r.ok, "{}", r.output);
    let e = edit(&ctx, "a.txt", "line 11\n", "eleven\n").await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn own_edits_and_writes_never_make_the_file_stale() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "a\nb\nc\n");
    read(&ctx, "a.txt").await;
    for (o, n) in [("a", "A"), ("b", "B"), ("c", "C")] {
        let e = edit(&ctx, "a.txt", o, n).await;
        assert!(e.ok, "{}", e.output);
    }
    let w = run(&Write, &ctx, json!({"path": "a.txt", "content": "x\n"})).await;
    assert!(w.ok, "{}", w.output);
    let e = edit(&ctx, "a.txt", "x", "y").await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "y\n");
}

#[tokio::test]
async fn a_new_file_needs_no_read_but_an_existing_one_needs_a_whole_read() {
    let (dir, ctx) = ws();
    let w = run(&Write, &ctx, json!({"path": "new.txt", "content": "n\n"})).await;
    assert!(w.ok, "{}", w.output);

    let body: String = (1..=50).map(|i| format!("line {i}\n")).collect();
    put(&dir, "old.txt", &body);
    let w = run(
        &Write,
        &ctx,
        json!({"path": "old.txt", "content": "gone\n"}),
    )
    .await;
    assert!(!w.ok, "an unread file was overwritten: {}", w.output);
    assert!(w.output.contains("read"), "{}", w.output);
    // Part of it is not enough to replace all of it.
    let r = run(&Read, &ctx, json!({"path": "old.txt", "limit": 5})).await;
    assert!(r.ok);
    let w = run(
        &Write,
        &ctx,
        json!({"path": "old.txt", "content": "gone\n"}),
    )
    .await;
    assert!(!w.ok, "{}", w.output);
    assert!(w.output.contains("part"), "{}", w.output);
    assert_eq!(get(&dir, "old.txt"), body);
    read(&ctx, "old.txt").await;
    let w = run(
        &Write,
        &ctx,
        json!({"path": "old.txt", "content": "gone\n"}),
    )
    .await;
    assert!(w.ok, "{}", w.output);
    assert_eq!(get(&dir, "old.txt"), "gone\n");
}

#[tokio::test]
async fn a_write_over_a_file_changed_since_it_was_read_is_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "mine\n");
    read(&ctx, "a.txt").await;
    put(&dir, "a.txt", "the user's change\n");
    let w = run(&Write, &ctx, json!({"path": "a.txt", "content": "x\n"})).await;
    assert!(!w.ok, "{}", w.output);
    assert!(w.output.contains("changed"), "{}", w.output);
    assert!(w.output.contains("the user's change"), "{}", w.output);
    assert_eq!(get(&dir, "a.txt"), "the user's change\n");
}

#[tokio::test]
async fn a_stale_file_is_edited_where_the_change_is_elsewhere_and_the_result_says_so() {
    let (dir, ctx) = ws();
    let body: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    put(&dir, "a.txt", &body);
    read(&ctx, "a.txt").await;
    // Someone else changes line 25.
    put(
        &dir,
        "a.txt",
        &body.replace("line 25\n", "line 25 changed by the user\n"),
    );
    let e = edit(&ctx, "a.txt", "line 3\n", "line three\n").await;
    assert!(e.ok, "{}", e.output);
    assert!(e.output.contains("changed on disk"), "{}", e.output);
    assert!(
        e.output.contains("line 25 changed by the user"),
        "{}",
        e.output
    );
    let now = get(&dir, "a.txt");
    assert!(now.contains("line three\n") && now.contains("line 25 changed by the user\n"));
}

#[tokio::test]
async fn a_stale_file_is_not_edited_where_it_changed_and_the_current_lines_are_shown() {
    let (dir, ctx) = ws();
    let body: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    put(&dir, "a.txt", &body);
    read(&ctx, "a.txt").await;
    let theirs = body.replace("line 12\n", "line 12 // theirs\n");
    put(&dir, "a.txt", &theirs);
    // The model's old text still matches on line 12's neighbour, but the
    // span it replaces takes in the changed line.
    let e = edit(&ctx, "a.txt", "line 11\nline 12", "eleven\ntwelve").await;
    assert!(!e.ok, "{}", e.output);
    assert!(e.output.contains("changed"), "{}", e.output);
    assert!(e.output.contains("12\tline 12 // theirs"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), theirs);
    // After reading again it goes through.
    read(&ctx, "a.txt").await;
    let e = edit(
        &ctx,
        "a.txt",
        "line 11\nline 12 // theirs",
        "eleven\ntwelve",
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

// --- several edits in one call ------------------------------------------

#[tokio::test]
async fn several_edits_apply_together_against_the_original() {
    let (dir, ctx) = ws();
    put(&dir, "a.rs", "fn a() {}\nfn b() {}\nfn c() {}\n");
    read(&ctx, "a.rs").await;
    let e = run(
        &Edit,
        &ctx,
        json!({"path": "a.rs", "edits": [
            {"old_string": "fn c()", "new_string": "fn see()"},
            {"old_string": "fn a()", "new_string": "fn b()"},
        ]}),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    // `fn b()` written by the second edit was not there for the first:
    // each is matched against the file as it was.
    assert_eq!(get(&dir, "a.rs"), "fn b() {}\nfn b() {}\nfn see() {}\n");
}

#[tokio::test]
async fn one_failing_edit_leaves_the_file_as_it_was() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\ntwo\nthree\n");
    read(&ctx, "a.txt").await;
    let e = run(
        &Edit,
        &ctx,
        json!({"path": "a.txt", "edits": [
            {"old_string": "one", "new_string": "1"},
            {"old_string": "four", "new_string": "4"},
        ]}),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    assert!(e.output.contains("edits[1]"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "one\ntwo\nthree\n");
}

#[tokio::test]
async fn overlapping_edits_are_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "alpha beta gamma\n");
    read(&ctx, "a.txt").await;
    let e = run(
        &Edit,
        &ctx,
        json!({"path": "a.txt", "edits": [
            {"old_string": "alpha beta", "new_string": "x"},
            {"old_string": "beta gamma", "new_string": "y"},
        ]}),
    )
    .await;
    assert!(!e.ok && e.output.contains("overlap"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "alpha beta gamma\n");
}

#[tokio::test]
async fn edits_given_as_a_json_string_are_taken() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "one\n");
    read(&ctx, "a.txt").await;
    let e = run(
        &Edit,
        &ctx,
        json!({"path": "a.txt", "edits": "[{\"old_string\": \"one\", \"new_string\": \"1\"}]"}),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "1\n");
}

// --- the result -----------------------------------------------------------

#[tokio::test]
async fn the_result_shows_the_changed_lines_numbered_as_read_shows_them() {
    let (dir, ctx) = ws();
    let body: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    put(&dir, "a.txt", &body);
    read(&ctx, "a.txt").await;
    let e = edit(&ctx, "a.txt", "line 20\n", "line 20a\nline 20b\n").await;
    assert!(e.ok, "{}", e.output);
    // A little context around the change, then nothing far away.
    for want in [
        "    18\tline 18",
        "    20\tline 20a",
        "    21\tline 20b",
        "    23\tline 22",
    ] {
        assert!(e.output.contains(want), "{want:?} missing: {}", e.output);
    }
    assert!(!e.output.contains("line 5\n"), "{}", e.output);
    // The same numbers `read` gives now.
    let r = read(&ctx, "a.txt").await;
    assert!(r.output.contains("    21\tline 20b"), "{}", r.output);
}

// --- matching -------------------------------------------------------------

#[tokio::test]
async fn several_matches_name_their_lines() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "x = 1\ny = 2\nx = 1\n");
    read(&ctx, "a.txt").await;
    let e = edit(&ctx, "a.txt", "x = 1", "x = 3").await;
    assert!(!e.ok, "{}", e.output);
    assert!(
        e.output.contains("2 times") && e.output.contains("lines 1, 3"),
        "{}",
        e.output
    );
}

#[tokio::test]
async fn trailing_whitespace_and_typographic_characters_are_forgiven_and_reported() {
    let (dir, ctx) = ws();
    put(
        &dir,
        "a.py",
        "def f():   \n    return 1\nmsg = \u{201c}hi\u{201d} \u{2014} ok\n",
    );
    read(&ctx, "a.py").await;
    let e = edit(
        &ctx,
        "a.py",
        "def f():\n    return 1\n",
        "def f():\n    return 2\n",
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert!(e.output.contains("trailing whitespace"), "{}", e.output);
    let e = edit(&ctx, "a.py", "msg = \"hi\" - ok", "msg = 'bye'").await;
    assert!(e.ok, "{}", e.output);
    assert!(e.output.contains("quotes"), "{}", e.output);
    assert_eq!(get(&dir, "a.py"), "def f():\n    return 2\nmsg = 'bye'\n");
}

#[tokio::test]
async fn indentation_is_not_forgiven_but_the_nearest_text_is_shown() {
    let (dir, ctx) = ws();
    put(
        &dir,
        "a.rs",
        "fn main() {\n    let a = 1;\n    let b = 2;\n}\n",
    );
    read(&ctx, "a.rs").await;
    let e = edit(&ctx, "a.rs", "let a = 1;\nlet b = 2;", "let c = 3;").await;
    assert!(!e.ok, "{}", e.output);
    assert!(e.output.contains("     2\t    let a = 1;"), "{}", e.output);
    assert!(e.output.contains("     3\t    let b = 2;"), "{}", e.output);
    assert_eq!(
        get(&dir, "a.rs"),
        "fn main() {\n    let a = 1;\n    let b = 2;\n}\n"
    );
}

#[tokio::test]
async fn an_edit_already_made_is_named() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "the quick brown fox jumps\n");
    read(&ctx, "a.txt").await;
    let e = edit(
        &ctx,
        "a.txt",
        "the slow brown fox",
        "the quick brown fox jumps",
    )
    .await;
    assert!(!e.ok, "{}", e.output);
    assert!(e.output.contains("already"), "{}", e.output);
}

#[tokio::test]
async fn pasted_line_numbers_are_taken_off() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "alpha\nbeta\ngamma\n");
    read(&ctx, "a.txt").await;
    let e = edit(&ctx, "a.txt", "     2\tbeta\n     3\tgamma", "B\nG").await;
    assert!(e.ok, "{}", e.output);
    assert!(e.output.contains("line-number"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "alpha\nB\nG\n");
}

#[tokio::test]
async fn crlf_endings_and_a_missing_final_newline_are_kept() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "\u{feff}one\r\ntwo\r\nthree");
    read(&ctx, "a.txt").await;
    let e = edit(&ctx, "a.txt", "one\ntwo\n", "uno\ndos\nmas\n").await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "\u{feff}uno\r\ndos\r\nmas\r\nthree");
    let e = edit(&ctx, "a.txt", "three", "tres").await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "\u{feff}uno\r\ndos\r\nmas\r\ntres");
}

#[tokio::test]
async fn replace_all_still_replaces_every_match() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", "a b a b a\n");
    read(&ctx, "a.txt").await;
    let e = run(
        &Edit,
        &ctx,
        json!({"path": "a.txt", "old_string": "a", "new_string": "c", "replace_all": true}),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert!(e.output.contains("3 replacements"), "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "c b c b c\n");
}

// --- the read window ------------------------------------------------------

#[tokio::test]
async fn a_long_window_stops_at_a_character_budget_and_never_loses_its_middle() {
    let (dir, ctx) = ws();
    let line = "x".repeat(55);
    let body: String = (1..=2000).map(|i| format!("{i:04} {line}\n")).collect();
    put(&dir, "big.txt", &body);
    let r = run(&Read, &ctx, json!({"path": "big.txt", "limit": 2000})).await;
    assert!(r.ok, "{}", r.output);
    assert!(!r.truncated, "the generic cut ran: {}", &r.output[..200]);
    assert!(!r.output.contains("output truncated"));
    // Contiguous from line 1, then the footer names the next offset.
    let shown: Vec<usize> = r
        .output
        .lines()
        .filter_map(|l| l.split('\t').next()?.trim().parse().ok())
        .collect();
    assert_eq!(shown.first(), Some(&1));
    let last = *shown.last().unwrap();
    assert_eq!(shown, (1..=last).collect::<Vec<_>>());
    assert!(last < 2000);
    let footer = format!("[showing lines 1-{last} of 2000; use offset {} ", last + 1);
    assert!(
        r.output.contains(&footer),
        "{}",
        r.output.lines().last().unwrap()
    );
    // Such a read is a partial one: it does not let `write` replace the file.
    let w = run(&Write, &ctx, json!({"path": "big.txt", "content": "x\n"})).await;
    assert!(!w.ok, "{}", w.output);
}
#[tokio::test]
async fn sessions_do_not_share_what_they_read() {
    let (dir, a) = ws();
    put(&dir, "a.txt", "x\n");
    let store = std::sync::Arc::new(crate::seen::SeenStore::default());
    let a = ToolCtx {
        seen: store.session(harness_core::SessionId::new()),
        ..a
    };
    let b = ToolCtx {
        seen: store.session(harness_core::SessionId::new()),
        ..ToolCtx::new(dir.path())
    };
    read(&a, "a.txt").await;
    let e = edit(&b, "a.txt", "x", "y").await;
    assert!(!e.ok, "{}", e.output);
    let e = edit(&a, "a.txt", "x", "y").await;
    assert!(e.ok, "{}", e.output);
}

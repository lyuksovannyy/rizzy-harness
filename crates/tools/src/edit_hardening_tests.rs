//! Audit of PR 188 (PLAN.md T45): a call's own new entry is never the
//! victim of the table's bounds; a tag pasted as another file's whole header
//! is refused; a preview and a refusal that prints nothing record nothing.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::seen::{MAX_ENTRIES, MAX_FILES, SeenStore};
use crate::{Edit, Grep, Read, Tool, ToolCtx, ToolOutcome};

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

/// A context whose session holds `MAX_FILES` read files (and, for `node`,
/// the node holds `MAX_ENTRIES` in all): any new entry is over a bound.
fn ctx_at_cap(dir: &std::path::Path, node: bool) -> ToolCtx {
    let mut ctx = ToolCtx::new(dir);
    let store = Arc::new(SeenStore::default());
    if node {
        for _ in 0..MAX_ENTRIES / MAX_FILES - 1 {
            store
                .session(harness_core::SessionId::new())
                .fill_read(MAX_FILES);
        }
    }
    ctx.seen = store.session(harness_core::SessionId::new());
    ctx.seen.fill_read(MAX_FILES);
    ctx
}

fn tag_in(line: &str) -> String {
    let line = line.trim();
    assert!(
        line.starts_with('[') && line.ends_with(']') && line.contains('#'),
        "not a header line: {line:?}"
    );
    line[line.rfind('#').unwrap() + 1..line.len() - 1].to_string()
}

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

const ABCD: &str = "alpha\nbeta\ngamma\ndelta\n";

#[tokio::test]
async fn a_refusal_that_prints_rows_is_answered_by_a_retry_that_lands_at_the_caps() {
    for node in [false, true] {
        let (dir, _) = ws();
        let ctx = ctx_at_cap(dir.path(), node);
        put(&dir, "a.txt", ABCD);
        let ops = json!([{"from": 2, "to": 2, "text": "BETA"}]);
        // A file never read: refused, its rows printed and counted as shown.
        let first = lines(&ctx, "a.txt", "ABCDEF", ops.clone()).await;
        assert!(
            !first.ok && first.output.contains("now count as shown"),
            "node {node}: {}",
            first.output
        );
        assert_eq!(get(&dir, "a.txt"), ABCD);
        // The tag the refusal gave, sent again: it lands.
        let tag = last_tag(&first.output);
        let second = lines(&ctx, "a.txt", &tag, ops).await;
        assert!(second.ok, "node {node}: {}", second.output);
        assert_eq!(get(&dir, "a.txt"), "alpha\nBETA\ngamma\ndelta\n");
    }
}

#[tokio::test]
async fn a_search_gives_tags_that_name_a_version_at_the_caps() {
    for node in [false, true] {
        let (dir, _) = ws();
        let ctx = ctx_at_cap(dir.path(), node);
        put(&dir, "a.txt", ABCD);
        let out = Grep.run(&ctx, json!({"pattern": "beta"})).await;
        assert!(out.ok, "{}", out.output);
        let head = out
            .output
            .lines()
            .find(|l| l.starts_with("[a.txt#"))
            .unwrap_or_else(|| panic!("no tag: {}", out.output));
        let e = lines(
            &ctx,
            "a.txt",
            &tag_in(head),
            json!([{"from": 2, "to": 2, "text": "BETA"}]),
        )
        .await;
        assert!(e.ok, "node {node}: {}", e.output);
        assert_eq!(get(&dir, "a.txt"), "alpha\nBETA\ngamma\ndelta\n");
    }
}

#[tokio::test]
async fn the_old_forms_announced_retry_lands_at_the_caps() {
    for node in [false, true] {
        let (dir, _) = ws();
        let ctx = ctx_at_cap(dir.path(), node);
        put(&dir, "a.txt", ABCD);
        let input = json!({"path": "a.txt", "old_string": "beta\n", "new_string": "BETA\n"});
        let first = Edit.run(&ctx, input.clone()).await;
        assert!(
            !first.ok && first.output.contains("now count as shown"),
            "node {node}: {}",
            first.output
        );
        let second = Edit.run(&ctx, input).await;
        assert!(second.ok, "node {node}: {}", second.output);
        assert_eq!(get(&dir, "a.txt"), "alpha\nBETA\ngamma\ndelta\n");
    }
}

// --- a pasted header for another file --------------------------------------

/// What the loop does with a call: `normalize_args`, then the refusal before
/// any card, then the run.
async fn send(ctx: &ToolCtx, mut input: Value) -> (Option<String>, ToolOutcome) {
    Edit.normalize_args(&mut input).unwrap();
    let refusal = Edit.refusal(ctx, &input);
    let out = Edit.run(ctx, input).await;
    (refusal, out)
}

#[tokio::test]
async fn a_header_pasted_for_another_file_is_refused() {
    let (dir, ctx) = ws();
    // The same text: the same tag, so only the path tells the files apart.
    put(&dir, "a.txt", ABCD);
    put(&dir, "b.txt", ABCD);
    for f in ["a.txt", "b.txt"] {
        let r = Read.run(&ctx, json!({"path": f})).await;
        assert!(r.ok, "{}", r.output);
    }
    let t = tag_of(ABCD);
    let ops = json!([{"from": 2, "to": 2, "text": "X"}]);
    for pasted in [format!("[b.txt#{t}]"), format!("[sub/../b.txt#{t}]")] {
        let (refusal, out) =
            send(&ctx, json!({"path": "a.txt", "tag": pasted, "lines": ops})).await;
        assert!(
            !out.ok && out.output.contains("another file"),
            "{pasted}: {}",
            out.output
        );
        assert!(
            refusal.is_some_and(|r| r.contains("another file")),
            "{pasted}"
        );
        assert_eq!(get(&dir, "a.txt"), ABCD, "{pasted}");
    }
}

#[tokio::test]
async fn a_bare_tag_a_hash_tag_and_the_same_files_header_are_accepted() {
    let (dir, ctx) = ws();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    put(&dir, "sub/a.txt", ABCD);
    let r = Read.run(&ctx, json!({"path": "sub/a.txt"})).await;
    assert!(r.ok, "{}", r.output);
    let t = tag_of(ABCD);
    let abs = dir.path().join("sub/a.txt").display().to_string();
    for (n, tag) in [
        t.clone(),
        t.to_lowercase(),
        format!("#{t}"),
        format!("[sub/a.txt#{t}]"),
        format!("[./sub/a.txt#{t}]"),
        format!("[{abs}#{t}]"),
    ]
    .into_iter()
    .enumerate()
    {
        put(&dir, "sub/a.txt", ABCD);
        let r = Read.run(&ctx, json!({"path": "sub/a.txt"})).await;
        assert!(r.ok, "{}", r.output);
        let (refusal, out) = send(
            &ctx,
            json!({"path": "sub/a.txt", "tag": tag,
                   "lines": [{"from": 2, "to": 2, "text": format!("X{n}")}]}),
        )
        .await;
        assert!(refusal.is_none(), "{tag}: {refusal:?}");
        assert!(out.ok, "{tag}: {}", out.output);
        assert_eq!(
            get(&dir, "sub/a.txt"),
            format!("alpha\nX{n}\ngamma\ndelta\n")
        );
    }
    // The path given as absolute, the header as the tool prints it.
    put(&dir, "sub/a.txt", ABCD);
    Read.run(&ctx, json!({"path": "sub/a.txt"})).await;
    let (_, out) = send(
        &ctx,
        json!({"path": abs, "tag": format!("[sub/a.txt#{t}]"),
               "lines": [{"from": 1, "to": 1, "text": "A"}]}),
    )
    .await;
    assert!(out.ok, "{}", out.output);
}

// --- a preview records nothing ----------------------------------------------

#[tokio::test]
async fn a_preview_and_a_refusal_that_print_nothing_leave_the_table_as_it_was() {
    let (dir, mut ctx) = ws();
    ctx.read_unasked = false;
    put(&dir, "a.txt", ABCD);
    let state = || ctx.seen.table_state();
    let call = |tag: &str, ops: Value| json!({"path": "a.txt", "tag": tag, "lines": ops});
    let mut calls: Vec<(&str, Value)> = vec![
        // The file was never read: the tag is unknown.
        (
            "unknown tag, no entry",
            call("ABCDEF", json!([{"from": 2, "to": 2, "text": "X"}])),
        ),
        (
            "no tag",
            json!({"path": "a.txt", "lines": [{"from": 2, "to": 2, "text": "X"}]}),
        ),
    ];
    for (name, input) in &calls {
        let before = state();
        let _ = Edit.preview(&ctx, input);
        let _ = Edit.refusal(&ctx, input);
        assert_eq!(state(), before, "{name}");
    }
    // Read, so the tag is known; then a plan that works, one past the end,
    // and one that changes nothing because the file changed since and the
    // rows it names did not.
    let r = Read.run(&ctx, json!({"path": "a.txt"})).await;
    assert!(r.ok, "{}", r.output);
    let t = tag_of(ABCD);
    calls.clear();
    calls.push((
        "a plan that works",
        call(&t, json!([{"from": 2, "to": 2, "text": "X"}])),
    ));
    calls.push((
        "a row past the end",
        call(&t, json!([{"from": 9, "to": 9, "text": "X"}])),
    ));
    for (name, input) in &calls {
        let before = state();
        let _ = Edit.preview(&ctx, input);
        let _ = Edit.refusal(&ctx, input);
        assert_eq!(state(), before, "{name}");
    }
    put(&dir, "a.txt", &format!("{ABCD}epsilon\n"));
    let same = call(&t, json!([{"from": 2, "to": 2, "text": "beta"}]));
    let before = state();
    assert!(Edit.preview(&ctx, &same).is_none());
    let refusal = Edit.refusal(&ctx, &same).expect("a refusal");
    assert!(refusal.contains("changes nothing"), "{refusal}");
    assert_eq!(state(), before, "a plan that changes nothing");
}

/// A file whose own name starts with a bracket (`[id].tsx`, common in web
/// projects): the header the tool prints for it is accepted, and so is the
/// bare `path#TAG` form (review of PR 196: every leading bracket was
/// stripped, so the tool's own header was refused as another file's).
#[tokio::test]
async fn the_header_of_a_file_whose_name_starts_with_a_bracket_is_accepted() {
    for name in ["[id].tsx", "[[x]].txt", "a].txt"] {
        let (dir, ctx) = ws();
        let t = tag_of(ABCD);
        for (n, tag) in [format!("[{name}#{t}]"), format!("{name}#{t}"), t.clone()]
            .into_iter()
            .enumerate()
        {
            put(&dir, name, ABCD);
            let r = Read.run(&ctx, json!({"path": name})).await;
            assert!(r.ok, "{}", r.output);
            assert!(
                r.output.starts_with(&format!("[{name}#{t}]")),
                "{}",
                r.output
            );
            let (refusal, out) = send(
                &ctx,
                json!({"path": name, "tag": tag,
                       "lines": [{"from": 2, "to": 2, "text": format!("X{n}")}]}),
            )
            .await;
            assert!(refusal.is_none(), "{name} {tag}: {refusal:?}");
            assert!(out.ok, "{name} {tag}: {}", out.output);
            assert_eq!(get(&dir, name), format!("alpha\nX{n}\ngamma\ndelta\n"));
        }
    }
    // Another file's header is still refused, with one clean sentence.
    let (dir, ctx) = ws();
    put(&dir, "[id].tsx", ABCD);
    put(&dir, "b.txt", ABCD);
    Read.run(&ctx, json!({"path": "[id].tsx"})).await;
    let (refusal, out) = send(
        &ctx,
        json!({"path": "[id].tsx", "tag": format!("[b.txt#{}]", tag_of(ABCD)),
               "lines": [{"from": 1, "to": 1, "text": "x"}]}),
    )
    .await;
    let said = refusal.unwrap_or(out.output);
    assert!(said.contains("header of another file"), "{said}");
    assert!(!said.contains("  "), "a gap in the sentence: {said}");
    assert_eq!(get(&dir, "[id].tsx"), ABCD);
}

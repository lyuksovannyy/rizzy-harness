//! A tag, once issued for a path in a session, never names another version of
//! it (PLAN.md T45). Two versions whose hashes start with the same four hex
//! characters (1 in 65,536 per pair) used to share a tag: the older was
//! dropped, and the line form of `edit`, naming the tag with the row numbers
//! of the older version, was applied by those numbers to the newer one,
//! silently (seed 941788 of the random test, mode 1). Now the later one's tag
//! is longer, and the older one stays.

use serde_json::{Value, json};

use crate::{Edit, Grep, Read, Tool, ToolCtx, ToolOutcome};

fn ws() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    // 4-character base tags: a collision of two versions is found by
    // counting (1 in 65,536); at the shipped length it is not.
    let mut ctx = ToolCtx::new(dir.path());
    ctx.seen = std::sync::Arc::new(crate::seen::SeenStore::with_tag_len(4))
        .session(harness_core::SessionId::new());
    (dir, ctx)
}

fn put(dir: &tempfile::TempDir, text: &str) {
    std::fs::write(dir.path().join("a.txt"), text).unwrap();
}

fn get(dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(dir.path().join("a.txt")).unwrap()
}

fn tag_in(line: &str) -> String {
    let line = line.trim();
    assert!(
        line.starts_with("[a.txt#") && line.ends_with(']'),
        "not a header line: {line:?}"
    );
    line[line.rfind('#').unwrap() + 1..line.len() - 1].to_string()
}

fn first_tag(out: &str) -> String {
    tag_in(out.lines().next().unwrap())
}

fn last_tag(out: &str) -> String {
    tag_in(out.lines().last().unwrap())
}

fn base(text: &str) -> String {
    crate::seen::prefix(&crate::seen::sha256(text.as_bytes()), 4)
}

async fn read(ctx: &ToolCtx) -> ToolOutcome {
    let r = Read.run(ctx, json!({"path": "a.txt"})).await;
    assert!(r.ok, "{}", r.output);
    r
}

async fn lines(ctx: &ToolCtx, tag: &str, ops: Value) -> ToolOutcome {
    Edit.run(ctx, json!({"path": "a.txt", "tag": tag, "lines": ops}))
        .await
}

const A: &str = "alpha\nbeta\ngamma\ndelta\n";

/// The text `A` has with row 2 replaced by `beta {i}`, for the first `i`
/// whose four-character tag is `A`'s (found by counting: 1 in 65,536; the context's tags are 4 characters long here).
fn b_of_a() -> (usize, String) {
    let t = base(A);
    (0usize..)
        .map(|i| (i, format!("alpha\nbeta {i}\ngamma\ndelta\n")))
        .find(|(_, b)| base(b) == t)
        .unwrap()
}

/// Another text with `A`'s four-character tag, for an external change.
fn other_with_a_tag() -> String {
    let t = base(A);
    (0usize..)
        .map(|i| format!("alpha\nexternal {i}\ngamma\ndelta\n"))
        .find(|b| base(b) == t)
        .unwrap()
}

/// Seed 941788, mode 1: the agent's own edit made a file whose tag is the
/// one it was read with. The second call names the first tag with row
/// numbers of the first version.
#[tokio::test]
async fn an_edit_that_ends_in_the_tag_it_started_with_gets_a_longer_one() {
    let (dir, ctx) = ws();
    put(&dir, A);
    let t = first_tag(&read(&ctx).await.output);
    assert_eq!(t, base(A));
    let (i, b) = b_of_a();
    let e1 = lines(
        &ctx,
        &t,
        json!([{"from": 2, "to": 2, "text": format!("beta {i}")}]),
    )
    .await;
    assert!(e1.ok, "{}", e1.output);
    assert_eq!(get(&dir), b);
    let t5 = last_tag(&e1.output);
    assert!(
        t5.len() == 5 && t5.starts_with(&t) && t5 != t,
        "the result shows the same tag: {t5} {t}"
    );
    // The old tag names the old version: row 2 was changed since, so naming
    // it is refused, and the file stays.
    let stale = lines(&ctx, &t, json!([{"from": 2, "to": 2, "text": "X"}])).await;
    assert!(!stale.ok && get(&dir) == b, "{}", stale.output);
    // A row it did not change lands where it is now.
    let ok = lines(&ctx, &t, json!([{"from": 4, "to": 4, "text": "DELTA"}])).await;
    assert!(ok.ok, "{}", ok.output);
    assert_eq!(get(&dir), format!("alpha\nbeta {i}\ngamma\nDELTA\n"));
    // The new tag names the new version, whose row 2 the result showed.
    let on_b = lines(&ctx, &t5, json!([{"from": 2, "to": 2, "text": "Y"}])).await;
    assert!(on_b.ok, "{}", on_b.output);
    assert_eq!(get(&dir), "alpha\nY\ngamma\nDELTA\n");
}

/// The same with a file changed on disk and read again: both stay named.
#[tokio::test]
async fn a_later_read_of_a_changed_file_with_the_same_tag_does_not_take_it() {
    let (dir, ctx) = ws();
    put(&dir, A);
    let t = first_tag(&read(&ctx).await.output);
    let b = other_with_a_tag();
    put(&dir, &b);
    let r = read(&ctx).await;
    let t5 = first_tag(&r.output);
    assert!(t5.len() == 5 && t5.starts_with(&t), "{t5} {t}");
    // `t` is the first version: its row 2 differs from the file now.
    let stale = lines(&ctx, &t, json!([{"from": 2, "to": 2, "text": "X"}])).await;
    assert!(!stale.ok && get(&dir) == b, "{}", stale.output);
    let ok = lines(&ctx, &t5, json!([{"from": 2, "to": 2, "text": "X"}])).await;
    assert!(ok.ok, "{}", ok.output);
    assert_eq!(get(&dir), "alpha\nX\ngamma\ndelta\n");
}

/// A search's version and a read's: the search's tag stays with the rows it
/// showed.
#[tokio::test]
async fn a_read_does_not_take_the_tag_of_a_searchs_version() {
    let (dir, ctx) = ws();
    put(&dir, A);
    let g = Grep.run(&ctx, json!({"pattern": "^beta$"})).await;
    assert!(g.ok, "{}", g.output);
    let t = first_tag(&g.output);
    assert_eq!(t, base(A));
    let b = other_with_a_tag();
    put(&dir, &b);
    let t5 = first_tag(&read(&ctx).await.output);
    assert!(t5.len() == 5 && t5.starts_with(&t), "{t5} {t}");
    // Row 2 of the version the search showed is not row 2 of the file now.
    let stale = lines(&ctx, &t, json!([{"from": 2, "to": 2, "text": "X"}])).await;
    assert!(!stale.ok && get(&dir) == b, "{}", stale.output);
}

/// A version that left the table: its tag is not given to a newer one, and
/// is refused as unknown, with the rows.
#[tokio::test]
async fn the_tag_of_an_evicted_version_is_unknown_and_never_the_new_ones() {
    let (dir, ctx) = ws();
    put(&dir, A);
    let t = first_tag(&read(&ctx).await.output);
    // More versions than are kept: `A` goes.
    for i in 0..crate::seen::MAX_VERSIONS {
        put(&dir, &format!("other {i}\nx\n"));
        read(&ctx).await;
    }
    let b = other_with_a_tag();
    put(&dir, &b);
    let t5 = first_tag(&read(&ctx).await.output);
    assert!(t5.len() == 5 && t5.starts_with(&t), "{t5} {t}");
    let r = lines(&ctx, &t, json!([{"from": 2, "to": 2, "text": "X"}])).await;
    assert!(!r.ok && get(&dir) == b, "{}", r.output);
    assert!(
        r.output.contains("is known in this session") && r.output.contains("external"),
        "refused as unknown, with the rows: {}",
        r.output
    );
    // Neither a prefix of the longer tag, nor a longer string that starts
    // with a tag, names it.
    let long = format!("{t5}0");
    for bad in [t.as_str(), long.as_str(), &t5[..3]] {
        let r = lines(&ctx, bad, json!([{"from": 2, "to": 2, "text": "X"}])).await;
        assert!(
            !r.ok && get(&dir) == b && r.output.contains("is known in this session"),
            "{bad}: {}",
            r.output
        );
    }
    // The whole header pasted, in lower case, still names it.
    let pasted = format!("[a.txt#{}]", t5.to_lowercase());
    let ok = lines(&ctx, &pasted, json!([{"from": 2, "to": 2, "text": "X"}])).await;
    assert!(ok.ok, "{}", ok.output);
}

/// A -> B -> A: the same content has the same tag, and the rows shown of it
/// are the ones recorded for it.
#[tokio::test]
async fn the_same_content_seen_again_has_the_tag_it_had() {
    let (dir, ctx) = ws();
    put(&dir, A);
    let t = first_tag(&read(&ctx).await.output);
    let (i, b) = b_of_a();
    let e1 = lines(
        &ctx,
        &t,
        json!([{"from": 2, "to": 2, "text": format!("beta {i}")}]),
    )
    .await;
    assert!(e1.ok, "{}", e1.output);
    let t5 = last_tag(&e1.output);
    // Back to `A`, by a line edit.
    let e2 = lines(&ctx, &t5, json!([{"from": 2, "to": 2, "text": "beta"}])).await;
    assert!(e2.ok && get(&dir) == A, "{}", e2.output);
    assert_eq!(last_tag(&e2.output), t, "A has its tag again");
    // And by a read after an external change.
    put(&dir, &b);
    assert_eq!(first_tag(&read(&ctx).await.output), t5);
    put(&dir, A);
    assert_eq!(first_tag(&read(&ctx).await.output), t);
    // Both tags still work, each on its own rows.
    let on_a = lines(&ctx, &t, json!([{"from": 3, "to": 3, "text": "G"}])).await;
    assert!(on_a.ok, "{}", on_a.output);
    assert_eq!(get(&dir), "alpha\nbeta\nG\ndelta\n");
}

/// A model that sends only the tag of a version that is not kept, or a
/// shorter or longer string than any tag, is refused: a whole-string match.
#[tokio::test]
async fn tags_are_compared_whole_not_as_prefixes() {
    let (dir, ctx) = ws();
    put(&dir, A);
    let t = first_tag(&read(&ctx).await.output);
    for bad in [&t[..3], &format!("{t}0"), "", "xyz1"] {
        let r = Edit
            .run(
                &ctx,
                json!({"path": "a.txt", "tag": bad, "lines": [{"from": 1, "to": 1, "text": "X"}]}),
            )
            .await;
        assert!(!r.ok && get(&dir) == A, "{bad:?}: {}", r.output);
    }
}

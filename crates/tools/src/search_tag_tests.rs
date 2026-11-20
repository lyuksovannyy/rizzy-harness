//! `grep` results carry tags (PLAN.md T45): each file with matches is
//! headed `[path#TAG]` as `read` heads it, the matched rows printed in full
//! are recorded as shown for that version (and the file does not count as
//! read), so the line form of `edit` can change a matched row with no
//! `read`. Bounded per search; never for a path `read` would refuse.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{Edit, Grep, Read, Tool, ToolCtx, Write};

fn ws() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    (dir, ctx)
}

fn put(dir: &tempfile::TempDir, name: &str, text: &str) {
    let p = dir.path().join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

fn get(dir: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(dir.path().join(name)).unwrap()
}

fn tag_of(text: &str) -> String {
    crate::seen::tag(&crate::seen::sha256(text.as_bytes()))
}

async fn grep(ctx: &ToolCtx, pattern: &str) -> String {
    let r = Grep.run(ctx, json!({"pattern": pattern})).await;
    assert!(r.ok, "{}", r.output);
    r.output
}

/// The tag in the heading `[path#TAG]`, if the output has one for `path`.
fn heading(out: &str, path: &str) -> Option<String> {
    let want = format!("[{path}#");
    out.lines()
        .find(|l| l.starts_with(&want) && l.ends_with(']'))
        .map(|l| l[want.len()..l.len() - 1].to_string())
}

fn headings(out: &str) -> usize {
    out.lines()
        .filter(|l| l.starts_with('[') && l.contains('#') && l.ends_with(']'))
        .count()
}

async fn line_edit(ctx: &ToolCtx, path: &str, tag: &str, ops: Value) -> crate::ToolOutcome {
    Edit.run(ctx, json!({"path": path, "tag": tag, "lines": ops}))
        .await
}

const ABCD: &str = "alpha\nbeta\ngamma\ndelta\n";

#[tokio::test]
async fn a_search_heads_each_file_with_its_tag_and_a_line_edit_of_a_matched_row_lands() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    put(&dir, "sub/b.txt", "one\nbeta two\n");
    let out = grep(&ctx, "beta").await;
    assert_eq!(heading(&out, "a.txt"), Some(tag_of(ABCD)), "{out}");
    assert_eq!(
        heading(&out, "sub/b.txt"),
        Some(tag_of("one\nbeta two\n")),
        "{out}"
    );
    assert!(
        out.contains("\n2:beta") || out.starts_with("2:beta"),
        "{out}"
    );
    // No `read` anywhere: the line edit lands.
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 2, "to": 2, "text": "BETA"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "alpha\nBETA\ngamma\ndelta\n");
}

#[tokio::test]
async fn the_bounded_style_heads_the_group_with_the_same_header() {
    let (dir, mut ctx) = ws();
    ctx.savings.bounded_reads = true;
    put(&dir, "a.txt", ABCD);
    let out = grep(&ctx, "a$").await;
    assert_eq!(heading(&out, "a.txt"), Some(tag_of(ABCD)), "{out}");
    assert!(out.contains("  1: alpha"), "{out}");
    assert!(!out.contains("a.txt:\n"), "{out}");
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 3, "to": 3, "text": "G"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn a_row_of_the_file_that_was_not_in_the_results_is_refused_and_printed() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    grep(&ctx, "beta").await;
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 3, "to": 3, "text": "G"}]),
    )
    .await;
    assert!(
        !e.ok && e.output.contains("not shown") && e.output.contains("\tgamma\n"),
        "{}",
        e.output
    );
    assert_eq!(get(&dir, "a.txt"), ABCD);
    // The refusal printed it: the resend lands.
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 3, "to": 3, "text": "G"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn a_search_is_not_a_read_write_and_unmatched_old_string_edits_are_refused() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    grep(&ctx, "beta").await;
    let w = Write
        .run(&ctx, json!({"path": "a.txt", "content": "whole\n"}))
        .await;
    assert!(!w.ok && w.output.contains("not been read"), "{}", w.output);
    assert_eq!(get(&dir, "a.txt"), ABCD);
    let o = Edit
        .run(
            &ctx,
            json!({"path": "a.txt", "old_string": "delta", "new_string": "D"}),
        )
        .await;
    assert!(!o.ok && o.output.contains("not been read"), "{}", o.output);
    assert_eq!(get(&dir, "a.txt"), ABCD);
    // Before the card too.
    assert!(
        Write
            .refusal(&ctx, &json!({"path": "a.txt", "content": "x\n"}))
            .is_some()
    );
    // `read` is still the way to make the whole file editable.
    assert!(
        Read.run(&ctx, json!({"path": "a.txt"})).await.ok,
        "read works"
    );
    let w = Write
        .run(&ctx, json!({"path": "a.txt", "content": "whole\n"}))
        .await;
    assert!(w.ok, "{}", w.output);
}

#[tokio::test]
async fn an_old_string_edit_inside_the_matched_rows_follows_a_search() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    grep(&ctx, "beta").await;
    let o = Edit
        .run(
            &ctx,
            json!({"path": "a.txt", "old_string": "beta\n", "new_string": "B\n"}),
        )
        .await;
    assert!(o.ok, "{}", o.output);
    assert_eq!(get(&dir, "a.txt"), "alpha\nB\ngamma\ndelta\n");
}

#[tokio::test]
async fn a_row_cut_for_length_is_not_shown_and_is_marked() {
    for bounded in [false, true] {
        let (dir, mut ctx) = ws();
        ctx.savings.bounded_reads = bounded;
        let cap = if bounded { 200 } else { 300 };
        let long = format!("needle {}", "x".repeat(cap));
        let exact = format!("needle {}", "y".repeat(cap - 7));
        assert_eq!(exact.chars().count(), cap);
        let text = format!("top\n{long}\nneedle short\n{exact}\n");
        put(&dir, "a.txt", &text);
        let out = grep(&ctx, "needle").await;
        let t = heading(&out, "a.txt").expect(&out);
        assert!(out.contains('…'), "a cut row is marked: {out}");
        assert!(
            out.contains(&exact) && !out.contains(&format!("{exact}…")),
            "a row of exactly the cap is whole: {out}"
        );
        // The cut one is refused (and printed whole by `read`'s limits).
        let e = line_edit(
            &ctx,
            "a.txt",
            &t,
            json!([{"from": 2, "to": 2, "text": "L"}]),
        )
        .await;
        assert!(!e.ok && e.output.contains("not shown"), "{}", e.output);
        assert_eq!(get(&dir, "a.txt"), text);
        // The short one and the exact one were shown whole.
        let e = line_edit(
            &ctx,
            "a.txt",
            &t,
            json!([{"from": 3, "to": 3, "text": "S"}, {"from": 4, "to": 4, "text": "E"}]),
        )
        .await;
        assert!(e.ok, "bounded {bounded}: {}", e.output);
    }
}

#[tokio::test]
async fn a_file_whose_matches_are_all_cut_gets_no_tag() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", &format!("needle {}\n", "x".repeat(400)));
    let out = grep(&ctx, "needle").await;
    assert!(heading(&out, "a.txt").is_none(), "{out}");
    assert!(out.contains("a.txt:1:needle "), "{out}");
}

/// The row numbers `search` prints are the rows `read` numbers.
#[tokio::test]
async fn the_row_numbers_are_the_rows_read_numbers() {
    let cases: [(&str, &str); 6] = [
        ("crlf.txt", "a\r\nb\r\nneedle\r\nd\r\n"),
        ("bom.txt", "\u{feff}needle first\nx\nneedle\ny\n"),
        ("nofinal.txt", "a\nb\nneedle"),
        ("blank.txt", "needle\n\n\nneedle\n\n"),
        ("lonecr.txt", "a\rb\nneedle\r\nz\n"),
        ("cr_end.txt", "a\nneedle\r"),
    ];
    for (name, text) in cases {
        let (dir, ctx) = ws();
        put(&dir, name, text);
        let out = grep(&ctx, "needle").await;
        let t = heading(&out, name).unwrap_or_else(|| panic!("{name}: {out}"));
        assert_eq!(t, tag_of(text), "{name}");
        // What `read` numbers, in a table of its own.
        let reader = ToolCtx::new(dir.path());
        let r = Read.run(&reader, json!({"path": name})).await;
        assert!(r.ok, "{}", r.output);
        let read_rows: Vec<usize> = r
            .output
            .lines()
            .filter(|l| l.contains("needle"))
            .map(|l| l.trim_start().split('\t').next().unwrap().parse().unwrap())
            .collect();
        let printed: Vec<usize> = out
            .lines()
            .filter(|l| l.contains("needle"))
            .map(|l| l.split(':').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(printed, read_rows, "{name}\n{out}\n{}", r.output);
        // And an edit of the first printed row, by the tag, hits exactly it.
        let n = printed[0];
        let e = line_edit(&ctx, name, &t, json!([{"from": n, "to": n, "text": "HIT"}])).await;
        assert!(e.ok, "{name}: {}", e.output);
        let after = get(&dir, name);
        let mut want_rows: Vec<&str> = text.split_inclusive('\n').collect();
        let eol = if want_rows[n - 1].ends_with("\r\n") {
            "\r\n"
        } else if want_rows[n - 1].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let bom = if text.starts_with('\u{feff}') && n == 1 {
            "\u{feff}"
        } else {
            ""
        };
        let new_row = format!("{bom}HIT{eol}");
        want_rows[n - 1] = &new_row;
        assert_eq!(after, want_rows.concat(), "{name}");
    }
}

#[tokio::test]
async fn a_file_that_read_cannot_keep_gets_no_tag() {
    let (dir, ctx) = ws();
    // Over the snapshot size: searched (it is under the search's own limit),
    // not tagged.
    let big = format!(
        "needle\n{}",
        "x\n".repeat(crate::seen::SNAPSHOT_MAX_FILE / 2)
    );
    put(&dir, "big.txt", &big);
    // Not UTF-8.
    std::fs::write(dir.path().join("latin.txt"), b"needle \xe9\n").unwrap();
    // Binary: not searched at all.
    std::fs::write(dir.path().join("bin.dat"), b"needle\0\0\n").unwrap();
    put(&dir, "ok.txt", "needle\n");
    let out = grep(&ctx, "needle").await;
    assert!(out.contains("big.txt:1:needle"), "{out}");
    assert!(out.contains("latin.txt:1:needle"), "{out}");
    assert!(!out.contains("bin.dat"), "{out}");
    assert!(heading(&out, "ok.txt").is_some(), "{out}");
    assert_eq!(headings(&out), 1, "{out}");
    // Nothing of them is known: the line form refuses, the way it does.
    let e = line_edit(
        &ctx,
        "big.txt",
        &tag_of(&big),
        json!([{"from": 1, "to": 1, "text": "x"}]),
    )
    .await;
    assert!(!e.ok, "{}", e.output);
}

#[tokio::test]
async fn only_so_many_files_per_search_are_tagged() {
    let (dir, ctx) = ws();
    let n = crate::search::MAX_TAGGED_FILES + 9;
    for i in 0..n {
        put(&dir, &format!("f{i:02}.txt"), &format!("needle {i}\n"));
    }
    let out = grep(&ctx, "needle").await;
    assert_eq!(headings(&out), crate::search::MAX_TAGGED_FILES, "{out}");
    // The rest keep the old rows, so the model sees it must `read` them.
    let plain = out
        .lines()
        .filter(|l| l.starts_with('f') && l.contains(".txt:1:needle"))
        .count();
    assert_eq!(plain, 9, "{out}");
    // A tagged one is editable; an untagged one is not (nothing is known).
    let tagged: Vec<String> = (0..n)
        .map(|i| format!("f{i:02}.txt"))
        .filter(|f| heading(&out, f).is_some())
        .collect();
    let f = &tagged[0];
    let i: usize = f[1..3].parse().unwrap();
    let e = line_edit(
        &ctx,
        f,
        &tag_of(&format!("needle {i}\n")),
        json!([{"from": 1, "to": 1, "text": "X"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    let untagged = (0..n)
        .map(|i| format!("f{i:02}.txt"))
        .find(|f| heading(&out, f).is_none())
        .unwrap();
    let before = get(&dir, &untagged);
    // The file is only known by its text: no version was recorded.
    assert!(
        ctx.seen
            .version(&dir.path().join(&untagged), &tag_of(&before))
            .is_none()
    );
}

#[tokio::test]
async fn only_so_many_bytes_per_search_are_tagged() {
    let (dir, ctx) = ws();
    let size = 300_000usize;
    assert!(size < crate::seen::SNAPSHOT_MAX_FILE);
    let fits = crate::search::MAX_TAGGED_BYTES / size;
    for i in 0..fits + 3 {
        put(
            &dir,
            &format!("f{i}.txt"),
            &format!("needle {i}\n{}", "x".repeat(size)),
        );
    }
    let out = grep(&ctx, "needle").await;
    assert_eq!(headings(&out), fits, "{out}");
    assert_eq!(out.lines().filter(|l| l.contains(":1:needle")).count(), 3);
}

/// Review of the design: a wide search must not push out what the agent
/// really read.
#[tokio::test]
async fn files_really_read_keep_their_snapshots_through_a_wide_search() {
    let (dir, ctx) = ws();
    // Read most of the node's snapshot budget, in big files.
    let big = |i: usize| format!("read {i}\n{}", "r".repeat(500_000));
    let n = crate::seen::SNAPSHOT_MAX_TOTAL / 500_000 - 1;
    for i in 0..n {
        put(&dir, &format!("r{i}.txt"), &big(i));
        let r = Read.run(&ctx, json!({"path": format!("r{i}.txt")})).await;
        assert!(r.ok, "{}", r.output);
    }
    // A wide search over more files than it may keep.
    for i in 0..40 {
        put(
            &dir,
            &format!("s{i:02}.txt"),
            &format!("needle {i}\n{}", "s".repeat(100_000)),
        );
    }
    let out = grep(&ctx, "needle").await;
    assert!(headings(&out) >= 1, "{out}");
    for i in 0..n {
        let key = dir.path().join(format!("r{i}.txt"));
        let seen = ctx.seen.get(&key).expect("still known");
        assert!(seen.snapshot.is_some(), "r{i}: snapshot was pushed out");
    }
}

#[tokio::test]
async fn a_protected_folder_and_git_and_links_get_no_tag_and_no_rows() {
    let (dir, mut ctx) = ws();
    put(&dir, "ok.txt", "needle ok\n");
    put(&dir, "secret/keys.txt", "needle key\n");
    put(&dir, ".git/config", "needle git\n");
    put(&dir, "real/r.txt", "needle real\n");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("o.txt"), "needle outside\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(dir.path().join("ok.txt"), dir.path().join("link.txt")).unwrap();
        symlink(dir.path().join("real"), dir.path().join("linkdir")).unwrap();
        symlink(outside.path().join("o.txt"), dir.path().join("out.txt")).unwrap();
    }
    ctx.protected = Arc::new(vec![dir.path().join("secret").canonicalize().unwrap()]);
    let out = grep(&ctx, "needle").await;
    assert!(heading(&out, "ok.txt").is_some(), "{out}");
    assert!(heading(&out, "real/r.txt").is_some(), "{out}");
    for hidden in [
        "secret", "keys.txt", ".git", "link.txt", "linkdir", "out.txt",
    ] {
        assert!(!out.contains(hidden), "{hidden}: {out}");
    }
    // The table holds nothing for a path `read` would refuse.
    let secret = dir.path().join("secret/keys.txt").canonicalize().unwrap();
    assert!(ctx.seen.version(&secret, &tag_of("needle key\n")).is_none());
    // Searching the protected folder itself is refused, with no tag.
    let r = Grep
        .run(&ctx, json!({"pattern": "needle", "path": "secret"}))
        .await;
    assert!(!r.ok && !r.output.contains("keys.txt"), "{}", r.output);
    assert!(ctx.seen.version(&secret, &tag_of("needle key\n")).is_none());
}

/// Outside mode: a search rooted above a protected folder skips it.
#[tokio::test]
async fn outside_mode_skips_the_protected_folder_and_tags_the_rest() {
    let top = tempfile::tempdir().unwrap();
    let work = top.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(top.path().join("data")).unwrap();
    std::fs::write(top.path().join("data/k.txt"), "needle data\n").unwrap();
    std::fs::write(top.path().join("plain.txt"), "needle plain\n").unwrap();
    let mut ctx = ToolCtx::new(&work);
    ctx.allow_outside_workspace = true;
    ctx.protected = Arc::new(vec![top.path().join("data").canonicalize().unwrap()]);
    let r = Grep
        .run(&ctx, json!({"pattern": "needle", "path": top.path()}))
        .await;
    assert!(r.ok, "{}", r.output);
    assert!(
        !r.output.contains("k.txt") && !r.output.contains("data"),
        "{}",
        r.output
    );
    let plain = top.path().join("plain.txt");
    let want = format!("[{}#{}]", plain.display(), tag_of("needle plain\n"));
    assert!(r.output.contains(&want), "{}\nwant {want}", r.output);
    let e = Edit
        .run(
            &ctx,
            json!({"path": plain, "tag": tag_of("needle plain\n"),
                   "lines": [{"from": 1, "to": 1, "text": "P"}]}),
        )
        .await;
    assert!(e.ok, "{}", e.output);
}

#[tokio::test]
async fn two_searches_of_one_file_add_their_rows_to_one_version() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    let a = grep(&ctx, "beta").await;
    let b = grep(&ctx, "delta").await;
    assert_eq!(heading(&a, "a.txt"), heading(&b, "a.txt"));
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 2, "to": 2, "text": "B"}, {"from": 4, "to": 4, "text": "D"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "alpha\nB\ngamma\nD\n");
}

#[tokio::test]
async fn a_file_changed_after_the_search_follows_the_rule_of_a_read() {
    // Elsewhere in the file: lands.
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    grep(&ctx, "beta").await;
    put(&dir, "a.txt", "alpha\nbeta\ngamma\nDELTA!\n");
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 2, "to": 2, "text": "B"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "alpha\nB\ngamma\nDELTA!\n");
    // In the named row: refused.
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    grep(&ctx, "beta").await;
    put(&dir, "a.txt", "alpha\nBETA2\ngamma\ndelta\n");
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 2, "to": 2, "text": "B"}]),
    )
    .await;
    assert!(
        !e.ok && e.output.contains("not the version"),
        "{}",
        e.output
    );
    assert_eq!(get(&dir, "a.txt"), "alpha\nBETA2\ngamma\ndelta\n");
    // A row inserted next to the named one: the rows are no longer where
    // they were, as after a read, so it lands on the right row only if the
    // row is still the same one (inserted below: lands).
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    grep(&ctx, "beta").await;
    put(&dir, "a.txt", "alpha\nbeta\nNEW\ngamma\ndelta\n");
    let e = line_edit(
        &ctx,
        "a.txt",
        &tag_of(ABCD),
        json!([{"from": 2, "to": 2, "text": "B"}]),
    )
    .await;
    assert!(e.ok, "{}", e.output);
    assert_eq!(get(&dir, "a.txt"), "alpha\nB\nNEW\ngamma\ndelta\n");
}

#[tokio::test]
async fn glob_prints_no_tags() {
    let (dir, ctx) = ws();
    put(&dir, "a.txt", ABCD);
    let r = crate::Glob.run(&ctx, json!({"pattern": "*.txt"})).await;
    assert_eq!(r.output, "a.txt");
}

#[test]
fn the_description_stays_short() {
    // Measured before the change: 112 characters.
    let d = Grep.description();
    assert!(d.contains("#TAG") && d.chars().count() <= 260, "{d}");
}

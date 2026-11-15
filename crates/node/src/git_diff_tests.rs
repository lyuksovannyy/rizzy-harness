//! A turn's changes as a diff (X.12, PLAN.md T53): read from the turn's
//! two checkpoints in the shadow repository, bounded, with hostile names
//! and contents taken as data.

use super::tests::sh;
use super::*;
use harness_protocol::{DiffContent, DiffFile, DiffStatus, TurnDiff};

struct Turn {
    _d: tempfile::TempDir,
    root: PathBuf,
    repo: Repo,
    before: String,
}

/// A repository with a few committed files, and its `before` snapshot.
async fn turn(files: &[(&str, &[u8])]) -> Turn {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap().join("proj");
    let base = d.path().canonicalize().unwrap().join("checkpoints");
    std::fs::create_dir_all(&root).unwrap();
    sh(&root, &["init", "-q", "-b", "main"]);
    for (p, c) in files {
        let path = root.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, c).unwrap();
    }
    sh(&root, &["add", "-A"]);
    sh(&root, &["commit", "-qm", "init", "--allow-empty"]);
    let repo = repo_for(&root, Some(&base)).await.unwrap();
    let before = snapshot(&repo, "refs/harness/s/t1/before", "before")
        .await
        .unwrap();
    Turn {
        _d: d,
        root,
        repo,
        before,
    }
}

impl Turn {
    async fn diff(&self) -> TurnDiff {
        let after = snapshot(&self.repo, "refs/harness/s/t1/after", "after")
            .await
            .unwrap();
        turn_diff(&self.repo, &self.before, &after, &|s: &str| s.to_string())
            .await
            .unwrap()
    }
}

fn file<'a>(d: &'a TurnDiff, path: &str) -> &'a DiffFile {
    d.files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} not in {:#?}", d.files))
}

fn lines(f: &DiffFile) -> Vec<&str> {
    f.hunks
        .iter()
        .flat_map(|h| h.lines.iter().map(String::as_str))
        .collect()
}

/// The whole turn: an edit, a new file, a deleted file, a rename with a
/// change, and a file a shell command rewrote (the snapshot does not care
/// who wrote it). Line numbers and context come with each hunk.
#[tokio::test]
async fn a_turn_that_edits_creates_deletes_and_renames() {
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let t = turn(&[
        ("src/a.rs", body.as_bytes()),
        ("gone.txt", b"bye\n"),
        ("old_name.txt", b"keep 1\nkeep 2\nkeep 3\nkeep 4\nkeep 5\n"),
        ("by_shell.txt", b"x\n"),
    ])
    .await;
    std::fs::write(
        t.root.join("src/a.rs"),
        body.replace("line 10\n", "line ten\n"),
    )
    .unwrap();
    std::fs::write(t.root.join("new.txt"), "hello\n").unwrap();
    std::fs::remove_file(t.root.join("gone.txt")).unwrap();
    std::fs::rename(t.root.join("old_name.txt"), t.root.join("new_name.txt")).unwrap();
    std::fs::write(
        t.root.join("new_name.txt"),
        "keep 1\nkeep 2\nkeep 3\nkeep 4\nkeep 5\nadded\n",
    )
    .unwrap();
    let ok = std::process::Command::new("sh")
        .arg("-c")
        .arg("printf 'y\\n' >> by_shell.txt")
        .current_dir(&t.root)
        .status()
        .unwrap()
        .success();
    assert!(ok);

    let d = t.diff().await;
    assert!(!d.cut, "{d:#?}");
    assert_eq!(d.files.len(), 5, "{:#?}", d.files);

    let a = file(&d, "src/a.rs");
    assert_eq!(a.status, DiffStatus::Modified);
    assert_eq!(a.content, DiffContent::Text);
    assert_eq!((a.added, a.removed), (1, 1));
    assert_eq!(a.hunks.len(), 1);
    let h = &a.hunks[0];
    assert_eq!((h.old_start, h.new_start), (7, 7));
    assert_eq!(
        lines(a),
        [
            " line 7",
            " line 8",
            " line 9",
            "-line 10",
            "+line ten",
            " line 11",
            " line 12",
            " line 13"
        ]
    );

    let n = file(&d, "new.txt");
    assert_eq!(n.status, DiffStatus::Added);
    assert_eq!(lines(n), ["+hello"]);
    assert_eq!((n.old_size, n.new_size), (None, Some(6)));

    let g = file(&d, "gone.txt");
    assert_eq!(g.status, DiffStatus::Deleted);
    assert_eq!(lines(g), ["-bye"]);

    let r = file(&d, "new_name.txt");
    assert_eq!(r.status, DiffStatus::Renamed);
    assert_eq!(r.old_path.as_deref(), Some("old_name.txt"));
    assert_eq!((r.added, r.removed), (1, 0));
    assert!(lines(r).contains(&"+added"), "{r:#?}");

    let s = file(&d, "by_shell.txt");
    assert_eq!(lines(s), [" x", "+y"]);

    // The same paths as the "N files changed" list (a rename names both).
    let listed = changed(
        &t.repo,
        &t.before,
        &snapshot(&t.repo, "refs/harness/s/t1/x", "x").await.unwrap(),
    )
    .await
    .unwrap();
    let mut named: Vec<String> = d
        .files
        .iter()
        .flat_map(|f| std::iter::once(f.path.clone()).chain(f.old_path.clone()))
        .collect();
    let mut want: Vec<String> = listed.into_iter().map(|(_, p)| p).collect();
    named.sort();
    want.sort();
    assert_eq!(named, want);
}

/// "No newline at end of file" travels as its own marked line.
#[tokio::test]
async fn a_missing_final_newline_is_marked() {
    let t = turn(&[("a.txt", b"one\n")]).await;
    std::fs::write(t.root.join("a.txt"), "one\ntwo").unwrap();
    let d = t.diff().await;
    let l = lines(file(&d, "a.txt"));
    assert_eq!(l[..2], [" one", "+two"]);
    assert!(l[2].starts_with('\\'), "{l:?}");
}

/// A binary is named with its sizes, its bytes are not sent; a file over
/// the per-file size is named with its sizes too.
#[tokio::test]
async fn binary_and_large_files_are_named_not_dumped() {
    let t = turn(&[]).await;
    let png: &[u8] = b"\x89PNG\0\0\x01\x02binary";
    std::fs::write(t.root.join("img.bin"), png).unwrap();
    let big = "a line of text\n".repeat((diff::FILE_BYTES as usize / 15) + 10);
    std::fs::write(t.root.join("big.txt"), &big).unwrap();
    let d = t.diff().await;
    let b = file(&d, "img.bin");
    assert_eq!(b.content, DiffContent::Binary);
    assert!(b.hunks.is_empty());
    assert_eq!(b.new_size, Some(png.len() as u64));
    let l = file(&d, "big.txt");
    assert_eq!(l.content, DiffContent::TooLarge);
    assert!(l.hunks.is_empty());
    assert_eq!(l.new_size, Some(big.len() as u64));
}

/// One file over the per-file text budget, and a line over the line
/// length: cut, with the marks that say so.
#[tokio::test]
async fn caps_cut_with_a_marker() {
    let t = turn(&[]).await;
    // Under the size limit, over the text a file may send.
    let many: String = (0..30_000).map(|i| format!("{i:08}\n")).collect();
    assert!((many.len() as u64) < diff::FILE_BYTES);
    std::fs::write(t.root.join("many.txt"), &many).unwrap();
    std::fs::write(t.root.join("long.min.js"), "x".repeat(10_000) + "\n").unwrap();
    let d = t.diff().await;
    assert!(d.cut);
    let m = file(&d, "many.txt");
    assert!(m.cut);
    assert!(lines(m).len() < 30_000);
    let text: usize = lines(m).iter().map(|l| l.len() + 1).sum();
    assert!(text <= diff::FILE_TEXT, "{text}");
    let long = file(&d, "long.min.js");
    assert!(long.cut);
    let l = lines(long);
    assert!(
        l[0].ends_with(diff::LINE_CUT),
        "{}",
        &l[0][l[0].len() - 20..]
    );
    assert!(l[0].len() <= diff::LINE_BYTES + diff::LINE_CUT.len());
}

/// More changed files than are listed: the first ones by name, the rest
/// counted, the answer marked cut and within its size.
#[tokio::test]
async fn too_many_files_are_counted_not_listed() {
    let t = turn(&[]).await;
    for i in 0..(diff::MAX_FILES + 25) {
        std::fs::write(t.root.join(format!("f{i:05}.txt")), format!("{i}\n")).unwrap();
    }
    let d = t.diff().await;
    assert_eq!(d.files.len(), diff::MAX_FILES);
    assert_eq!(d.more_files, 25);
    assert!(d.cut);
    let json = serde_json::to_vec(&d).unwrap();
    assert!(
        json.len() <= harness_protocol::MAX_DIFF_JSON,
        "{}",
        json.len()
    );
}

/// Hostile names: a control character, an option-like name, a glob, a
/// pathspec magic word, a name that is not UTF-8. Each is a file of the
/// diff with its own lines, and none of them is read as an argument.
#[tokio::test]
async fn hostile_names_are_names() {
    let t = turn(&[]).await;
    use std::os::unix::ffi::OsStrExt;
    let names: Vec<&[u8]> = vec![
        b"--output=pwned",
        b"-p",
        b"evil\nname\x1b[31m",
        b"*",
        b":(top)x",
        b"\xff\xfe.txt",
        b"\xe2\x80\xaeexe.txt",
    ];
    for (i, n) in names.iter().enumerate() {
        std::fs::write(
            t.root.join(std::ffi::OsStr::from_bytes(n)),
            format!("v{i}\n"),
        )
        .unwrap();
    }
    let d = t.diff().await;
    assert_eq!(d.files.len(), names.len(), "{:#?}", d.files);
    for (i, n) in names.iter().enumerate() {
        let shown = String::from_utf8_lossy(n);
        let f = file(&d, &shown);
        assert_eq!(lines(f), [format!("+v{i}")], "{shown:?}");
    }
    // `--output=pwned` was a pathspec, not an option: nothing was written.
    assert!(!t.root.join("pwned").exists());
    assert!(!std::env::current_dir().unwrap().join("pwned").exists());
}

/// A link changed to point outside the folder: its target is shown as the
/// link's text, never followed (the outside file's content stays out).
#[tokio::test]
async fn a_symlink_is_its_target_text() {
    let t = turn(&[]).await;
    let outside = t._d.path().join("secret.txt");
    std::fs::write(&outside, "TOP SECRET\n").unwrap();
    std::os::unix::fs::symlink("inside.txt", t.root.join("link")).unwrap();
    // The link is in neither snapshot yet; replace the before.
    let t = Turn {
        before: snapshot(&t.repo, "refs/harness/s/t1/before", "before")
            .await
            .unwrap(),
        ..t
    };
    std::fs::remove_file(t.root.join("link")).unwrap();
    std::os::unix::fs::symlink(&outside, t.root.join("link")).unwrap();
    let d = t.diff().await;
    let l = file(&d, "link");
    assert_eq!(l.new_mode.as_deref(), Some("120000"));
    let text = lines(l).join("\n");
    assert!(text.contains(&outside.display().to_string()), "{text}");
    assert!(!text.contains("TOP SECRET"), "{text}");
}

/// The node's secrets are scrubbed from lines and names, as from events.
#[tokio::test]
async fn secrets_are_scrubbed() {
    let t = turn(&[]).await;
    std::fs::write(t.root.join(".env"), "KEY=sk-live-abcdef0123456789\n").unwrap();
    let after = snapshot(&t.repo, "refs/harness/s/t1/after", "after")
        .await
        .unwrap();
    let secrets = harness_core::SecretSet::new(vec![harness_core::Secret::new(
        "sk-live-abcdef0123456789".to_string(),
    )]);
    let d = turn_diff(&t.repo, &t.before, &after, &|s: &str| secrets.scrub(s))
        .await
        .unwrap();
    let text = lines(file(&d, ".env")).join("\n");
    assert!(!text.contains("sk-live-abcdef0123456789"), "{text}");
}

/// A nested repository's files are not compared (as "N files changed").
#[tokio::test]
async fn nested_repositories_are_left_out() {
    let t = turn(&[("a.txt", b"1\n")]).await;
    let inner = t.root.join("vendor/lib");
    std::fs::create_dir_all(&inner).unwrap();
    sh(&inner, &["init", "-q", "-b", "main"]);
    std::fs::write(inner.join("x.txt"), "inner\n").unwrap();
    std::fs::write(t.root.join("a.txt"), "2\n").unwrap();
    let d = t.diff().await;
    assert_eq!(
        d.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["a.txt"]
    );
    assert_eq!(d.nested, ["vendor/lib"]);
}

/// A checkpoint whose objects are gone is a sentence, not a git error.
#[tokio::test]
async fn a_lost_checkpoint_is_said_plainly() {
    let t = turn(&[("a.txt", b"1\n")]).await;
    std::fs::write(t.root.join("a.txt"), "2\n").unwrap();
    let after = snapshot(&t.repo, "refs/harness/s/t1/after", "after")
        .await
        .unwrap();
    // The after's tree object, removed from the shadow's store.
    let tree = run(&t.repo, &["rev-parse", &format!("{after}^{{tree}}")], None)
        .await
        .unwrap();
    let shadow = t.repo.shadow.clone().unwrap();
    let obj = shadow.join("objects").join(&tree[..2]).join(&tree[2..]);
    std::fs::remove_file(&obj).unwrap();
    let e = turn_diff(&t.repo, &t.before, &after, &|s: &str| s.to_string())
        .await
        .unwrap_err();
    assert!(e.starts_with("the changes cannot be shown"), "{e}");
}

/// Review D1: a cut line marks itself and the rest of the file still shows
/// (a long Markdown or JSON line near the top hid every later change).
#[tokio::test]
async fn a_long_line_does_not_hide_the_rest_of_the_file() {
    let mut body: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
    let t = turn(&[("a.txt", (body.join("\n") + "\n").as_bytes())]).await;
    body[9] = "x".repeat(3000);
    body[89] = "line ninety".into();
    std::fs::write(t.root.join("a.txt"), body.join("\n") + "\n").unwrap();
    let d = t.diff().await;
    let f = file(&d, "a.txt");
    let l = lines(f);
    assert!(l.contains(&"+line ninety"), "{l:?}");
    assert!(l.iter().any(|x| x.ends_with(diff::LINE_CUT)));
    assert_eq!((f.added, f.removed), (2, 2));
    assert_eq!(f.hunks.len(), 2);
    assert!(f.cut);
}

/// Review D2: a secret across the line cut is scrubbed whole first.
#[tokio::test]
async fn a_secret_across_the_line_cut_does_not_leak_its_prefix() {
    let secret = "sk-live-abcdef0123456789";
    let line = "x".repeat(1990) + secret + "\n";
    let t = turn(&[]).await;
    std::fs::write(t.root.join("cfg"), &line).unwrap();
    let after = snapshot(&t.repo, "refs/harness/s/t1/after", "after")
        .await
        .unwrap();
    let secrets = harness_core::SecretSet::new(vec![harness_core::Secret::new(secret.to_string())]);
    let d = turn_diff(&t.repo, &t.before, &after, &|s: &str| secrets.scrub(s))
        .await
        .unwrap();
    let text = lines(file(&d, "cfg")).join("\n");
    assert!(!text.contains("sk-live-a"), "{}", &text[text.len() - 40..]);
}

/// Review D4: a file replaced by a link is shown as a type change with
/// both sides, not as "left out" with a false cut banner.
#[tokio::test]
async fn a_file_that_becomes_a_link_shows_both_sides() {
    let t = turn(&[("f", b"hello\n"), ("keep", b"k\n")]).await;
    std::fs::remove_file(t.root.join("f")).unwrap();
    std::os::unix::fs::symlink("keep", t.root.join("f")).unwrap();
    let d = t.diff().await;
    let f = file(&d, "f");
    assert_eq!(f.status, DiffStatus::TypeChanged);
    assert_eq!(f.content, DiffContent::Text);
    assert!(!f.cut && !d.cut, "{d:#?}");
    let l = lines(f);
    assert!(l.contains(&"-hello"), "{l:?}");
    assert!(l.contains(&"+keep"), "{l:?}");
    assert_eq!(f.new_mode.as_deref(), Some("120000"));
}

/// Review: the ordinary flows the reviewer walked, kept as a regression.
#[tokio::test]
async fn ordinary_flows() {
    let long: String = (1..=500).map(|i| format!("row {i}\n")).collect();
    let t = turn(&[
        ("long.txt", long.as_bytes()),
        ("crlf.txt", b"a\r\nb\r\nc\r\n"),
        ("same_a.txt", b"shared\n"),
        ("same_b.txt", b"shared\n"),
        ("emptied.txt", b"some\n"),
    ])
    .await;
    std::fs::write(
        t.root.join("long.txt"),
        long.replace("row 250\n", "row 250 changed\n"),
    )
    .unwrap();
    std::fs::write(t.root.join("crlf.txt"), b"a\r\nB\r\nc\r\n").unwrap();
    std::fs::write(t.root.join("same_a.txt"), b"shared\nplus\n").unwrap();
    std::fs::write(t.root.join("same_b.txt"), b"shared\nplus\n").unwrap();
    std::fs::write(t.root.join("emptied.txt"), b"").unwrap();
    std::fs::write(t.root.join(".gitignore"), b"ignored.txt\n").unwrap();
    std::fs::write(t.root.join("ignored.txt"), b"x\n").unwrap();
    let d = t.diff().await;
    let l = file(&d, "long.txt");
    assert_eq!((l.hunks[0].old_start, l.hunks[0].new_start), (247, 247));
    assert_eq!(
        lines(file(&d, "crlf.txt")),
        [" a\r", "-b\r", "+B\r", " c\r"]
    );
    let a = lines(file(&d, "same_a.txt"));
    assert_eq!(a, [" shared", "+plus"]);
    assert_eq!(a, lines(file(&d, "same_b.txt")));
    assert_eq!(lines(file(&d, "emptied.txt")), ["-some"]);
    assert!(d.files.iter().all(|f| f.path != "ignored.txt"));
}

/// Review: the folder's attributes and config run nothing in a diff.
#[tokio::test]
async fn repository_attributes_and_config_run_nothing() {
    let t = turn(&[("a.txt", b"1\n")]).await;
    let marker = t._d.path().join("RAN");
    let m = marker.display().to_string();
    std::fs::write(t.root.join(".gitattributes"), "* diff=evil filter=evil\n").unwrap();
    for (k, v) in [
        ("diff.evil.command", format!("touch {m}.cmd")),
        ("diff.evil.textconv", format!("sh -c 'touch {m}.tc; cat'")),
        ("filter.evil.clean", format!("sh -c 'touch {m}.clean; cat'")),
        (
            "filter.evil.smudge",
            format!("sh -c 'touch {m}.smudge; cat'"),
        ),
        ("core.fsmonitor", format!("touch {m}.fsm")),
        ("core.pager", format!("touch {m}.pager")),
        ("diff.external", format!("touch {m}.ext")),
    ] {
        sh(&t.root, &["config", k, &v]);
    }
    std::fs::write(t.root.join("a.txt"), "2\n").unwrap();
    let d = t.diff().await;
    let ran: Vec<_> = std::fs::read_dir(t._d.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| n.to_string_lossy().starts_with("RAN"))
        .collect();
    assert!(ran.is_empty(), "{ran:?}");
    assert_eq!(lines(file(&d, "a.txt")), ["-1", "+2"]);
}

/// Review: a directory of 300 files renamed is 300 renames.
#[tokio::test]
async fn a_renamed_directory_is_renames() {
    let files: Vec<(String, String)> = (0..300)
        .map(|i| (format!("dir/f{i}.txt"), format!("content {i}\nmore\n")))
        .collect();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_bytes()))
        .collect();
    let t = turn(&refs).await;
    std::fs::rename(t.root.join("dir"), t.root.join("dir2")).unwrap();
    let d = t.diff().await;
    assert_eq!(d.files.len(), 300);
    assert!(d.files.iter().all(|f| f.status == DiffStatus::Renamed));
}

/// The cost on a tree of a few thousand files (ignored; numbers in
/// `docs/undo-and-stash.md`, appendix): 3,000 files committed, a turn that
/// edits 200 of them and adds 20.
/// `cargo test -p harness-node --lib diff_cost -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn diff_cost_on_a_few_thousand_files() {
    let files: Vec<(String, String)> = (0..3000)
        .map(|i| {
            (
                format!("d{}/e{}/f{i:04}.rs", i % 30, i % 7),
                (0..40).map(|l| format!("fn f{i}_{l}() {{}}\n")).collect(),
            )
        })
        .collect();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_bytes()))
        .collect();
    let t = turn(&refs).await;
    for (p, c) in files.iter().step_by(15) {
        std::fs::write(t.root.join(p), c.replace("_20()", "_20_changed()")).unwrap();
    }
    for i in 0..20 {
        std::fs::write(t.root.join(format!("new{i}.rs")), "fn new() {}\n").unwrap();
    }
    let after = snapshot(&t.repo, "refs/harness/s/t1/after", "after")
        .await
        .unwrap();
    for run in 0..3 {
        let at = std::time::Instant::now();
        let d = turn_diff(&t.repo, &t.before, &after, &|s: &str| s.to_string())
            .await
            .unwrap();
        let json = serde_json::to_vec(&d).unwrap().len();
        eprintln!(
            "run {run}: {} files, {} bytes of JSON, {:.3} s",
            d.files.len(),
            json,
            at.elapsed().as_secs_f64()
        );
    }
}

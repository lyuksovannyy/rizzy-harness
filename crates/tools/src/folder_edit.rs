//! A person's text changes to a Word or Excel file of a chat's folder, saved
//! as a NEW file beside it (ADOPTION_PLAN X.60, PLAN.md T100). Like the
//! preview next to it ([`crate::folder_preview`]) it is the person's request,
//! not the agent's: no card, no event, no record in the agent's table of files
//! it has seen. The source is opened by the same function as the preview's
//! (one place that resolves, judges and reads it), the changes are run by the
//! confined `doc-edit` helper (never in this process), and the result is
//! written by the same create-only write as the agent's `doc_edit`
//! ([`crate::office::create_free`]): through the write side of the path policy
//! (the folder, links judged, protected places, `.git`), to a temporary file
//! in the directory published with a hard link that fails when the name
//! exists, so an existing file or a link is never replaced or followed, and a
//! full disk leaves no half file.
//!
//! The directory is the source's, as the person named it: beside the
//! original. The name is made here from the source's name (`<stem> (edited)`,
//! then `(2)`, ...); nothing the person sends names a destination.
//!
//! What goes back is a typed [`Refusal`] (one word) or per-change outcomes;
//! the helper's own words and every error text of the file system may hold a
//! name or a path and never leave this module.

use harness_core::docs::EditHelper;
pub use harness_docs::edit::page::{Change, ChangeWhy, PageKind};
use harness_docs::edit::preview::Why;
use harness_docs::edit::serve::{Request, frame_request, parse_reply};
use harness_docs::limits::{MAX_OUTPUT_BYTES, MAX_PAGE_CHANGES};

use crate::ToolCtx;
use crate::folder_preview::{Refusal, office_name, open_source};

/// Whether the name says a workbook (else a document): what the caller
/// expects the file to be, and what the helper checks the content against.
pub fn sheet_name(name: &str) -> bool {
    matches!(
        name.rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref(),
        Some("xlsx" | "xlsm")
    )
}

/// One change's outcome, in the order sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOutcome {
    pub ok: bool,
    pub why: Option<ChangeWhy>,
}

/// What a request made.
#[derive(Debug, PartialEq, Eq)]
pub enum Edited {
    /// The new file's path relative to the folder, and one outcome per change
    /// (all ok).
    Saved {
        path: String,
        results: Vec<ChangeOutcome>,
    },
    /// Nothing was written: one outcome per change, at least one refused.
    Refused(Vec<ChangeOutcome>),
}

/// The changes of `changes` to the file at `path` (a path of the chat's
/// folder, as text), saved as a new file beside it. `ctx` is a read context
/// of the session with `allow_outside_workspace` off (a context that allows
/// the outside is refused, as for the preview): the caller has checked what
/// node.toml allows.
pub async fn edit(
    ctx: &ToolCtx,
    helper: &dyn EditHelper,
    path: &str,
    sheet: Option<u32>,
    changes: Vec<Change>,
) -> Result<Edited, Refusal> {
    if changes.is_empty() || changes.len() > MAX_PAGE_CHANGES {
        return Err(Refusal::Damaged);
    }
    let n = changes.len();
    let bytes = open_source(ctx, path).await?;
    if !office_name(path) {
        return Err(Refusal::NotOffice);
    }
    if bytes.is_empty() {
        return Err(Refusal::Empty);
    }
    let kind = if sheet_name(path) {
        PageKind::Sheet
    } else {
        PageKind::Doc
    };
    let framed = frame_request(
        &Request::PageEdit {
            kind,
            sheet,
            changes,
        },
        &bytes,
    );
    drop(bytes);
    let out = helper.run(framed).await.map_err(|e| {
        if e == crate::doc_helper::BUSY {
            Refusal::Busy
        } else if e == crate::doc_helper::UNCONFINED {
            Refusal::NotAvailable
        } else {
            // The helper's own words may hold a name from the file.
            Refusal::Damaged
        }
    })?;
    let (reply, new) = parse_reply(&out).ok_or(Refusal::Damaged)?;
    if let Some(w) = reply.why {
        return Err(match w {
            Why::NotOffice => Refusal::NotOffice,
            Why::Protected => Refusal::Protected,
            Why::TooLarge => Refusal::TooLarge,
            Why::Damaged => Refusal::Damaged,
            Why::NoSheet => Refusal::NoSheet,
            Why::Locked => Refusal::Locked,
        });
    }
    // One outcome per change, no more and no fewer.
    if reply.results.len() != n {
        return Err(Refusal::Damaged);
    }
    let results: Vec<ChangeOutcome> = reply
        .results
        .iter()
        .map(|i| ChangeOutcome {
            ok: i.ok,
            why: i.why,
        })
        .collect();
    if !reply.file {
        // A request that applies nothing says why for at least one change.
        if results.iter().all(|r| r.ok) {
            return Err(Refusal::Damaged);
        }
        return Ok(Edited::Refused(results));
    }
    if !results.iter().all(|r| r.ok) || new.is_empty() {
        return Err(Refusal::Damaged);
    }
    if new.len() > MAX_OUTPUT_BYTES {
        return Err(Refusal::TooLarge);
    }
    // Beside the original: its directory as the person named it, its name
    // made here.
    let (dir, base) = match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    };
    let ext = crate::office_edit::ext_for(base, sheet_name(base));
    let stem = harness_docs::name::edited_stem(base, ext);
    let placed = crate::office::create_free(ctx, dir, &stem, ext, new)
        .await
        // A file-system error names a path.
        .map_err(|_| Refusal::NoWrite)?;
    Ok(Edited::Saved {
        path: placed.in_folder,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_docs::edit::preview::{Block, Preview};
    use std::sync::Arc;

    /// The real engine, in this process (the shipped helper runs it confined).
    struct InProc;
    #[async_trait::async_trait]
    impl EditHelper for InProc {
        async fn run(&self, input: Vec<u8>) -> Result<Vec<u8>, String> {
            harness_docs::edit::serve::serve(&input)
        }
    }
    /// A helper that fails with a sentence.
    struct Fails(String);
    #[async_trait::async_trait]
    impl EditHelper for Fails {
        async fn run(&self, _: Vec<u8>) -> Result<Vec<u8>, String> {
            Err(self.0.clone())
        }
    }
    /// A helper that answers what it is given, framed as the real one does.
    struct Says(harness_docs::edit::serve::Reply, Vec<u8>);
    #[async_trait::async_trait]
    impl EditHelper for Says {
        async fn run(&self, _: Vec<u8>) -> Result<Vec<u8>, String> {
            let head = serde_json::to_vec(&self.0).unwrap();
            let mut out = (head.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(&head);
            out.extend_from_slice(&self.1);
            Ok(out)
        }
    }

    fn docx() -> Vec<u8> {
        harness_docs::docx::build("# Title\n\nHello **there**.\n", &Default::default())
            .unwrap()
            .bytes
    }

    fn tag_of(file: &[u8], text: &str) -> String {
        let Preview::Doc { blocks, .. } = harness_docs::edit::preview::preview(file, None).unwrap()
        else {
            panic!()
        };
        blocks
            .iter()
            .find_map(|b| match b {
                Block::P { r, at } | Block::H { r, at, .. } | Block::Li { r, at }
                    if r.iter().map(|x| x.t.as_str()).collect::<String>() == text =>
                {
                    Some(at.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no block {text:?}"))
    }

    struct Rig {
        _d: tempfile::TempDir,
        ws: std::path::PathBuf,
        out: std::path::PathBuf,
    }

    fn rig() -> Rig {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().canonicalize().unwrap();
        let ws = base.join("ws");
        let out = base.join("out");
        std::fs::create_dir_all(ws.join("docs")).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("secret.docx"), docx()).unwrap();
        std::fs::write(ws.join("docs/a.docx"), docx()).unwrap();
        Rig { _d: d, ws, out }
    }

    fn change(file: &[u8], text: &str, to: &str) -> Vec<Change> {
        vec![Change {
            at: tag_of(file, text),
            text: to.into(),
        }]
    }

    fn listing(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    async fn go(r: &Rig, path: &str, changes: Vec<Change>) -> Result<Edited, Refusal> {
        let ctx = ToolCtx::new(&r.ws);
        edit(&ctx, &InProc, path, None, changes).await
    }

    #[tokio::test]
    async fn a_copy_is_saved_beside_the_original_and_the_original_is_untouched() {
        let r = rig();
        let before = std::fs::read(r.ws.join("docs/a.docx")).unwrap();
        let ch = change(&before, "Hello there.", "Changed text");
        let got = go(&r, "docs/a.docx", ch).await.unwrap();
        let Edited::Saved { path, results } = got else {
            panic!("{got:?}")
        };
        assert_eq!(path, "docs/a (edited).docx");
        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        assert_eq!(std::fs::read(r.ws.join("docs/a.docx")).unwrap(), before);
        let new = std::fs::read(r.ws.join(&path)).unwrap();
        assert!(tag_of(&new, "Changed text").starts_with('p'));
        assert_eq!(listing(&r.ws.join("docs")), ["a (edited).docx", "a.docx"]);
    }

    /// A copy never replaces a file, and a name that is a link is not
    /// followed or written through: a link to a file inside the folder is
    /// passed over (the next free name), one that leads out is refused (the
    /// path policy judges it before anything is written).
    /// A long name is cut before ` (edited)` is added, so the suffix always
    /// survives and the copy is never written over (or named as) the original.
    #[tokio::test]
    async fn a_long_name_keeps_the_suffix_and_never_names_the_original() {
        for stem in [
            "a".repeat(91),
            "a".repeat(92),
            "a".repeat(100),
            "a".repeat(120),
            "é日".repeat(45),
        ] {
            let r = rig();
            let name = format!("{stem}.docx");
            let src = std::fs::read(r.ws.join("docs/a.docx")).unwrap();
            std::fs::write(r.ws.join("docs").join(&name), &src).unwrap();
            let ch = change(&src, "Hello there.", "x");
            let Edited::Saved { path, .. } = go(&r, &format!("docs/{name}"), ch).await.unwrap()
            else {
                panic!()
            };
            let made = path.strip_prefix("docs/").unwrap();
            assert!(made.ends_with(" (edited).docx"), "{made}");
            assert!(made.chars().count() <= 100 + ".docx".len(), "{made}");
            assert_ne!(made, name);
            // The original is as it was and the copy is a different file.
            assert_eq!(std::fs::read(r.ws.join("docs").join(&name)).unwrap(), src);
            assert!(r.ws.join(&path).exists());
            if stem.chars().count() <= 91 {
                assert_eq!(made, format!("{stem} (edited).docx"));
            }
        }
    }

    #[tokio::test]
    async fn an_existing_name_or_a_link_in_its_place_is_not_written_through() {
        let r = rig();
        let src = std::fs::read(r.ws.join("docs/a.docx")).unwrap();
        std::fs::write(r.ws.join("docs/a (edited).docx"), b"mine").unwrap();
        std::fs::write(r.ws.join("docs/other.docx"), b"other").unwrap();
        std::os::unix::fs::symlink("other.docx", r.ws.join("docs/a (edited) (2).docx")).unwrap();
        let ch = change(&src, "Hello there.", "x");
        let Edited::Saved { path, .. } = go(&r, "docs/a.docx", ch.clone()).await.unwrap() else {
            panic!()
        };
        assert_eq!(path, "docs/a (edited) (3).docx");
        assert_eq!(
            std::fs::read(r.ws.join("docs/a (edited).docx")).unwrap(),
            b"mine"
        );
        assert_eq!(
            std::fs::read(r.ws.join("docs/other.docx")).unwrap(),
            b"other"
        );
        // A link out at the next name: nothing is written through it or
        // anywhere else.
        let outside = std::fs::read(r.out.join("secret.docx")).unwrap();
        std::os::unix::fs::symlink(
            r.out.join("secret.docx"),
            r.ws.join("docs/a (edited) (4).docx"),
        )
        .unwrap();
        let before = listing(&r.ws.join("docs"));
        assert_eq!(go(&r, "docs/a.docx", ch).await, Err(Refusal::NoWrite));
        assert_eq!(listing(&r.ws.join("docs")), before);
        assert_eq!(std::fs::read(r.out.join("secret.docx")).unwrap(), outside);
    }

    /// Every way out of the folder is the one answer, and nothing is
    /// written anywhere.
    #[tokio::test]
    async fn every_way_out_is_not_found_and_writes_nothing() {
        use std::os::unix::fs::symlink;
        let r = rig();
        symlink(r.out.join("secret.docx"), r.ws.join("link.docx")).unwrap();
        symlink(&r.out, r.ws.join("dirlink")).unwrap();
        std::fs::create_dir_all(r.ws.join(".git")).unwrap();
        std::fs::write(r.ws.join(".git/hook.docx"), docx()).unwrap();
        std::fs::create_dir_all(r.ws.join("data")).unwrap();
        std::fs::write(r.ws.join("data/key.docx"), docx()).unwrap();
        nix::unistd::mkfifo(&r.ws.join("p.docx"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let abs = r.out.join("secret.docx").display().to_string();
        let mut ctx = ToolCtx::new(&r.ws);
        ctx.protected = Arc::new(vec![r.ws.join("data")]);
        let snap = (listing(&r.ws), listing(&r.out), listing(&r.ws.join("data")));
        let ch = || {
            vec![Change {
                at: "p1#0000".into(),
                text: "x".into(),
            }]
        };
        for p in [
            "../out/secret.docx",
            abs.as_str(),
            "link.docx",
            "dirlink/secret.docx",
            ".git/hook.docx",
            ".GIT/hook.docx",
            "data/key.docx",
            "p.docx",
            "nope.docx",
            "",
            "a\nb.docx",
            "/etc/passwd",
        ] {
            let got = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                edit(&ctx, &InProc, p, None, ch()),
            )
            .await
            .expect("hung");
            assert_eq!(got, Err(Refusal::NotFound), "path {p:?}");
        }
        assert_eq!(
            snap,
            (listing(&r.ws), listing(&r.out), listing(&r.ws.join("data")))
        );
        // A node that lets the agent write outside does not let the page.
        let mut wide = ToolCtx::new(&r.ws);
        wide.allow_outside_workspace = true;
        for p in [abs.as_str(), "link.docx", "../out/secret.docx"] {
            assert_eq!(
                edit(&wide, &InProc, p, None, ch()).await,
                Err(Refusal::NotFound),
                "{p}"
            );
        }
        assert_eq!(listing(&r.out), snap.1);
    }

    #[tokio::test]
    async fn the_size_cap_the_empty_file_and_the_wrong_kind() {
        let r = rig();
        let ch = || {
            vec![Change {
                at: "p1#0000".into(),
                text: "x".into(),
            }]
        };
        let big = std::fs::File::create(r.ws.join("big.docx")).unwrap();
        big.set_len(MAX_OUTPUT_BYTES as u64 + 1).unwrap();
        assert_eq!(go(&r, "big.docx", ch()).await, Err(Refusal::FileTooLarge));
        std::fs::write(r.ws.join("e.docx"), b"").unwrap();
        assert_eq!(go(&r, "e.docx", ch()).await, Err(Refusal::Empty));
        std::fs::write(r.ws.join("a.txt"), b"x").unwrap();
        assert_eq!(go(&r, "a.txt", ch()).await, Err(Refusal::NotOffice));
        std::fs::write(r.ws.join("x.docx"), b"%PDF-1.4\n").unwrap();
        assert_eq!(go(&r, "x.docx", ch()).await, Err(Refusal::NotOffice));
        assert!(!listing(&r.ws).iter().any(|n| n.contains("edited")));
    }

    #[tokio::test]
    async fn no_changes_and_too_many_are_refused_before_the_file_is_read() {
        let r = rig();
        assert_eq!(go(&r, "nope.docx", vec![]).await, Err(Refusal::Damaged));
        let many = (0..=MAX_PAGE_CHANGES)
            .map(|_| Change {
                at: "p1#0000".into(),
                text: "x".into(),
            })
            .collect();
        assert_eq!(go(&r, "nope.docx", many).await, Err(Refusal::Damaged));
    }

    /// A refused change names the place and writes nothing; a stale tag is
    /// one of them.
    #[tokio::test]
    async fn a_refused_change_saves_nothing_and_says_which() {
        let r = rig();
        let src = std::fs::read(r.ws.join("docs/a.docx")).unwrap();
        let good = tag_of(&src, "Hello there.");
        let stale = format!("{}#beef", good.split('#').next().unwrap());
        let got = go(
            &r,
            "docs/a.docx",
            vec![
                Change {
                    at: stale,
                    text: "x".into(),
                },
                Change {
                    at: tag_of(&src, "Title"),
                    text: "y".into(),
                },
            ],
        )
        .await
        .unwrap();
        let Edited::Refused(rs) = got else {
            panic!("{got:?}")
        };
        assert_eq!(rs[0].why, Some(ChangeWhy::Stale));
        assert_eq!(rs[1].why, Some(ChangeWhy::Skipped));
        assert_eq!(listing(&r.ws.join("docs")), ["a.docx"]);
    }

    /// The helper's own words may hold a name from the file: never passed on.
    #[tokio::test]
    async fn a_failing_helper_is_a_word_and_leaks_nothing() {
        let r = rig();
        let ctx = ToolCtx::new(&r.ws);
        let ch = || {
            vec![Change {
                at: "p1#0000".into(),
                text: "x".into(),
            }]
        };
        let ask = |e: &str| {
            let h = Fails(e.to_string());
            let ctx = &ctx;
            async move { edit(ctx, &h, "docs/a.docx", None, ch()).await }
        };
        assert_eq!(ask(crate::doc_helper::BUSY).await, Err(Refusal::Busy));
        assert_eq!(
            ask(crate::doc_helper::UNCONFINED).await,
            Err(Refusal::NotAvailable)
        );
        assert_eq!(
            ask("part SECRET-NAME.xml is bad").await,
            Err(Refusal::Damaged)
        );
        assert_eq!(listing(&r.ws.join("docs")), ["a.docx"]);
    }

    /// What a helper that misbehaves can say: the wrong number of results, a
    /// file beside a refusal, an oversized file, a protected document.
    #[tokio::test]
    async fn a_hostile_or_confused_helper_answer_writes_nothing() {
        use harness_docs::edit::serve::{Item, Reply};
        let r = rig();
        let ctx = ToolCtx::new(&r.ws);
        let ok = |n: usize| -> Vec<Item> {
            (0..n)
                .map(|_| Item {
                    ok: true,
                    msg: "SECRET".into(),
                    why: None,
                })
                .collect()
        };
        let one = || {
            vec![Change {
                at: "p1#0000".into(),
                text: "x".into(),
            }]
        };
        let ask = |reply: Reply, bytes: Vec<u8>| {
            let h = Says(reply, bytes);
            let ctx = &ctx;
            let ch = one();
            async move { edit(ctx, &h, "docs/a.docx", None, ch).await }
        };
        // Two results for one change.
        let r2 = Reply {
            results: ok(2),
            file: true,
            ..Default::default()
        };
        assert_eq!(ask(r2, docx()).await, Err(Refusal::Damaged));
        // A file declared and a refusal in the results.
        let mixed = Reply {
            results: vec![Item {
                ok: false,
                msg: String::new(),
                why: Some(ChangeWhy::Other),
            }],
            file: true,
            ..Default::default()
        };
        assert_eq!(ask(mixed, docx()).await, Err(Refusal::Damaged));
        // All ok and no file.
        let none = Reply {
            results: ok(1),
            file: false,
            ..Default::default()
        };
        assert_eq!(ask(none, vec![]).await, Err(Refusal::Damaged));
        // A file one byte past the limit.
        let big = Reply {
            results: ok(1),
            file: true,
            ..Default::default()
        };
        assert_eq!(
            ask(big, vec![0u8; MAX_OUTPUT_BYTES + 1]).await,
            Err(Refusal::TooLarge)
        );
        // A whole-file reason.
        let locked = Reply {
            why: Some(Why::Locked),
            ..Default::default()
        };
        assert_eq!(ask(locked, vec![]).await, Err(Refusal::Locked));
        assert_eq!(listing(&r.ws.join("docs")), ["a.docx"]);
    }

    /// A directory that cannot be written to is a clean `NoWrite`, with no
    /// half file left behind. (Skipped when the tests run as root, which
    /// writes anywhere.)
    #[tokio::test]
    async fn a_directory_that_is_read_only_is_no_write_and_leaves_nothing() {
        use std::os::unix::fs::PermissionsExt;
        if nix::unistd::geteuid().is_root() {
            return;
        }
        let r = rig();
        let src = std::fs::read(r.ws.join("docs/a.docx")).unwrap();
        let ch = change(&src, "Hello there.", "x");
        let dir = r.ws.join("docs");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let got = go(&r, "docs/a.docx", ch).await;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(got, Err(Refusal::NoWrite));
        assert_eq!(listing(&dir), ["a.docx"]);
    }

    #[tokio::test]
    async fn a_workbook_is_saved_as_a_workbook() {
        let r = rig();
        let bytes = harness_docs::xlsx::build(
            &serde_json::json!({"sheets": [
                {"name": "One", "columns": [{"header": "A"}], "rows": [["x"]]},
                {"name": "Two", "columns": [{"header": "B"}], "rows": [["y"]]}]}),
            0,
        )
        .unwrap()
        .bytes;
        std::fs::write(r.ws.join("w.xlsx"), &bytes).unwrap();
        let ctx = ToolCtx::new(&r.ws);
        let got = edit(
            &ctx,
            &InProc,
            "w.xlsx",
            Some(1),
            vec![Change {
                at: "B2".into(),
                text: "12".into(),
            }],
        )
        .await
        .unwrap();
        let Edited::Saved { path, .. } = got else {
            panic!("{got:?}")
        };
        assert_eq!(path, "w (edited).xlsx");
        let new = std::fs::read(r.ws.join(&path)).unwrap();
        let Preview::Sheet { rows, .. } =
            harness_docs::edit::preview::preview(&new, Some(1)).unwrap()
        else {
            panic!()
        };
        assert!(
            rows.iter()
                .flat_map(|r| &r.c)
                .any(|c| c.a == "B2" && c.v == "12")
        );
        assert_eq!(std::fs::read(r.ws.join("w.xlsx")).unwrap(), bytes);
    }
}

//! The page's preview of a Word or Excel file in a chat's folder (ADOPTION_PLAN
//! X.59, PLAN.md T99). It is a person's request, not the agent's: no card, no
//! event, no record in the seen table. The file is found as the read side of
//! the office tools finds it ([`ToolCtx::resolve`]: the pinned folder, links
//! followed and judged, the node's protected places), only a regular file is
//! opened, and it is parsed only by the confined `doc-edit` helper, never in
//! this process.
//!
//! What goes back is a typed [`Refusal`], one word each. Every way of not
//! being able to read the path (outside the folder, missing, a directory, a
//! FIFO or a device, behind a link that leads out, inside `.git`, in a place
//! the node protects) is the one [`Refusal::NotFound`], so the answer tells
//! nothing about the target.

use std::io::Read as _;

use harness_core::docs::EditHelper;
use harness_docs::edit::serve::{Request, frame_request, parse_reply};
use harness_docs::limits::MAX_OUTPUT_BYTES;
use serde_json::Value;

use crate::ToolCtx;

/// The longest path read, in bytes (as `@` mentions).
pub const MAX_PATH_BYTES: usize = 1024;
/// The largest serialized preview returned.
pub const MAX_JSON_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotFound,
    FileTooLarge,
    TooLarge,
    Empty,
    NotOffice,
    Protected,
    Damaged,
    NoSheet,
    Busy,
    NotAvailable,
    /// Protected against editing (a page edit's).
    Locked,
    /// The node will not write the copy (a page edit's).
    NoWrite,
}

pub(crate) fn office_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "docx" | "docm" | "xlsx" | "xlsm"
        )
    })
}

/// A path of the chat's folder as text, opened for a person's request (the
/// preview, a page edit): resolved as the `read` tool resolves it (the pinned
/// folder, links followed and judged, protected places), never outside the
/// folder whatever `allow_outside_workspace` says, never inside `.git`, only
/// a regular file opened without blocking, at most [`MAX_OUTPUT_BYTES`]. Every
/// way of not reaching it is the one [`Refusal::NotFound`]. `ctx` is a read
/// context of the session.
pub(crate) async fn open_source(ctx: &ToolCtx, path: &str) -> Result<Vec<u8>, Refusal> {
    if ctx.allow_outside_workspace
        || path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.chars().any(char::is_control)
    {
        return Err(Refusal::NotFound);
    }
    let real = ctx.resolve(path).map_err(|_| Refusal::NotFound)?;
    // A repository's own files are not a place to preview from.
    let ws = ctx.workspace_dir().map_err(|_| Refusal::NotFound)?;
    let inside = real.strip_prefix(&ws).map_err(|_| Refusal::NotFound)?;
    if inside.components().any(|c| {
        matches!(c, std::path::Component::Normal(n)
            if n.to_str().is_some_and(|n| n.eq_ignore_ascii_case(".git")))
    }) {
        return Err(Refusal::NotFound);
    }
    // Existence is told before the name's kind, so a name that is not an
    // office file's says nothing about a path that is not there.
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Refusal> {
        let f = crate::open_regular(&real).map_err(|_| Refusal::NotFound)?;
        let mut out = Vec::new();
        f.take(MAX_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|_| Refusal::NotFound)?;
        if out.len() > MAX_OUTPUT_BYTES {
            return Err(Refusal::FileTooLarge);
        }
        Ok(out)
    })
    .await
    .map_err(|_| Refusal::NotFound)?
}

/// The preview of `path` (a path of the chat's folder, as text) as JSON.
/// `ctx` is a read context of the session with `allow_outside_workspace`
/// off; a context that allows the outside is refused, since a person's
/// preview never leaves the folder.
pub async fn preview(
    ctx: &ToolCtx,
    helper: &dyn EditHelper,
    path: &str,
    sheet: Option<u32>,
) -> Result<Value, Refusal> {
    let bytes = open_source(ctx, path).await?;
    if !office_name(path) {
        return Err(Refusal::NotOffice);
    }
    if bytes.is_empty() {
        return Err(Refusal::Empty);
    }
    let framed = frame_request(&Request::Preview { sheet }, &bytes);
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
    let (reply, _) = parse_reply(&out).ok_or(Refusal::Damaged)?;
    match (reply.preview, reply.why) {
        (Some(p), _) => {
            let v = serde_json::to_value(&p).map_err(|_| Refusal::Damaged)?;
            if serde_json::to_vec(&v).map_or(usize::MAX, |b| b.len()) > MAX_JSON_BYTES {
                return Err(Refusal::TooLarge);
            }
            Ok(v)
        }
        (None, Some(w)) => {
            use harness_docs::edit::preview::Why;
            Err(match w {
                Why::NotOffice => Refusal::NotOffice,
                Why::Protected => Refusal::Protected,
                Why::TooLarge => Refusal::TooLarge,
                Why::Damaged => Refusal::Damaged,
                Why::NoSheet => Refusal::NoSheet,
                Why::Locked => Refusal::Locked,
            })
        }
        (None, None) => Err(Refusal::NotOffice),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn docx() -> Vec<u8> {
        harness_docs::docx::build("# Title\n\nHello **there**.\n", &Default::default())
            .unwrap()
            .bytes
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
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("secret.docx"), docx()).unwrap();
        Rig { _d: d, ws, out }
    }

    async fn ask(r: &Rig, path: &str) -> Result<Value, Refusal> {
        let ctx = ToolCtx::new(&r.ws);
        preview(&ctx, &InProc, path, None).await
    }

    #[tokio::test]
    async fn a_document_in_the_folder_is_previewed() {
        let r = rig();
        std::fs::create_dir_all(r.ws.join("docs")).unwrap();
        std::fs::write(r.ws.join("docs/a.DOCX"), docx()).unwrap();
        let v = ask(&r, "docs/a.DOCX").await.unwrap();
        assert_eq!(v["kind"], "doc");
        assert!(v.to_string().contains("there"));
    }

    /// Outside, absolute, a link out, a dotdot, `.git`, a folder the node
    /// protects, a missing file: one answer, so nothing tells the target.
    #[tokio::test]
    async fn every_way_out_is_the_one_not_found() {
        use std::os::unix::fs::symlink;
        let r = rig();
        symlink(r.out.join("secret.docx"), r.ws.join("link.docx")).unwrap();
        symlink(&r.out, r.ws.join("dirlink")).unwrap();
        std::fs::create_dir_all(r.ws.join(".git")).unwrap();
        std::fs::write(r.ws.join(".git/hook.docx"), docx()).unwrap();
        std::fs::create_dir_all(r.ws.join("data")).unwrap();
        std::fs::write(r.ws.join("data/key.docx"), docx()).unwrap();
        let abs = r.out.join("secret.docx").display().to_string();
        let mut ctx = ToolCtx::new(&r.ws);
        ctx.protected = Arc::new(vec![r.ws.join("data")]);
        let missing = preview(&ctx, &InProc, "nope.docx", None).await;
        assert_eq!(missing, Err(Refusal::NotFound));
        for p in [
            "../out/secret.docx",
            abs.as_str(),
            "link.docx",
            "dirlink/secret.docx",
            ".git/hook.docx",
            ".GIT/hook.docx",
            "data/key.docx",
            "",
            "a\nb.docx",
            "/etc/passwd",
        ] {
            assert_eq!(preview(&ctx, &InProc, p, None).await, missing, "path {p:?}");
        }
        // A name that is not an office file's tells nothing about a path
        // that is not there either.
        assert_eq!(ask(&r, "nope.txt").await, Err(Refusal::NotFound));
        assert_eq!(
            ask(&r, &"a".repeat(MAX_PATH_BYTES + 1)).await,
            Err(Refusal::NotFound)
        );
    }

    /// A node that lets the agent read outside does not let the page.
    #[tokio::test]
    async fn the_outside_mode_of_the_node_does_not_widen_a_preview() {
        let r = rig();
        let mut ctx = ToolCtx::new(&r.ws);
        ctx.allow_outside_workspace = true;
        let abs = r.out.join("secret.docx").display().to_string();
        // In that mode `resolve` does not follow links: a link inside the
        // folder that leads out would pass a prefix check.
        std::os::unix::fs::symlink(r.out.join("secret.docx"), r.ws.join("link.docx")).unwrap();
        for p in [abs.as_str(), "link.docx", "../out/secret.docx"] {
            assert_eq!(
                preview(&ctx, &InProc, p, None).await,
                Err(Refusal::NotFound),
                "{p}"
            );
        }
    }

    /// A directory, a FIFO and a device named like a document are refused
    /// without opening them (a FIFO would hang an open for reading).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_directory_a_fifo_and_a_device_do_not_hang() {
        let r = rig();
        std::fs::create_dir_all(r.ws.join("d.docx")).unwrap();
        nix::unistd::mkfifo(&r.ws.join("p.docx"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        std::os::unix::fs::symlink("/dev/zero", r.ws.join("z.docx")).unwrap();
        for p in ["d.docx", "p.docx", "z.docx"] {
            let got = tokio::time::timeout(std::time::Duration::from_secs(5), ask(&r, p))
                .await
                .expect("hung");
            assert_eq!(got, Err(Refusal::NotFound), "{p}");
        }
    }

    #[tokio::test]
    async fn the_size_cap_the_empty_file_and_the_wrong_kind() {
        let r = rig();
        let big = std::fs::File::create(r.ws.join("big.docx")).unwrap();
        big.set_len(MAX_OUTPUT_BYTES as u64 + 1).unwrap();
        assert_eq!(ask(&r, "big.docx").await, Err(Refusal::FileTooLarge));
        std::fs::write(r.ws.join("e.docx"), b"").unwrap();
        assert_eq!(ask(&r, "e.docx").await, Err(Refusal::Empty));
        std::fs::write(r.ws.join("a.txt"), b"x").unwrap();
        assert_eq!(ask(&r, "a.txt").await, Err(Refusal::NotOffice));
        std::fs::write(r.ws.join("x.docx"), b"%PDF-1.4\n").unwrap();
        assert_eq!(ask(&r, "x.docx").await, Err(Refusal::NotOffice));
    }

    /// The helper's own words may hold a name from the file: never passed on.
    #[tokio::test]
    async fn a_failing_helper_is_a_word_and_leaks_nothing() {
        let r = rig();
        std::fs::write(r.ws.join("a.docx"), docx()).unwrap();
        let ctx = ToolCtx::new(&r.ws);
        let go = |e: &str| {
            let h = Fails(e.to_string());
            let ctx = &ctx;
            async move { preview(ctx, &h, "a.docx", None).await }
        };
        assert_eq!(go(crate::doc_helper::BUSY).await, Err(Refusal::Busy));
        assert_eq!(
            go(crate::doc_helper::UNCONFINED).await,
            Err(Refusal::NotAvailable)
        );
        assert_eq!(
            go("part SECRET-NAME.xml is bad").await,
            Err(Refusal::Damaged)
        );
    }

    #[tokio::test]
    async fn a_sheet_is_asked_for_by_its_place() {
        let r = rig();
        let bytes = harness_docs::xlsx::build(
            &serde_json::json!({"sheets": [
                {"name": "One", "columns": [{"header": "A"}], "rows": [["x"]]},
                {"name": "Two", "columns": [{"header": "B"}], "rows": [["y"]]}]}),
            0,
        )
        .unwrap()
        .bytes;
        std::fs::write(r.ws.join("w.xlsx"), bytes).unwrap();
        let ctx = ToolCtx::new(&r.ws);
        let v = preview(&ctx, &InProc, "w.xlsx", Some(1)).await.unwrap();
        assert_eq!(v["at"], 1);
        assert_eq!(
            preview(&ctx, &InProc, "w.xlsx", Some(9)).await,
            Err(Refusal::NoSheet)
        );
    }
}

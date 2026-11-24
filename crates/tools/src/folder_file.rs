//! A text file of a chat's folder for the page's side panel (ADOPTION_PLAN
//! X.63, PLAN.md T99): read-only, a person's request, not the agent's: no
//! card, no event, no record in the seen table. The path is found exactly as
//! the document preview finds it ([`crate::folder_preview::locate`]: the
//! pinned folder, links followed and judged, the node's protected places,
//! never outside the folder, never `.git`), only a regular file is opened,
//! without blocking, and at most [`MAX_TEXT_BYTES`] plus a margin is read
//! from it, whatever its size.
//!
//! The text goes through the node's secret scrubber before it leaves, as a
//! `read` result does before it becomes an event; the scrubber sees the
//! margin too, so a secret that straddles the cut is redacted whole and not
//! shown in part. What goes back is the text or one word of [`Refusal`]:
//! every way of not reaching the path is the one [`Refusal::NotFound`].

use std::io::Read as _;

use crate::ToolCtx;

/// The most text returned, in bytes (`harness_protocol::MAX_FOLDER_FILE_TEXT_BYTES`).
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Read beyond the text, so a secret that straddles the cut is whole to the
/// scrubber (a key is far shorter), and a multibyte character is whole.
const MARGIN: usize = 4096;
/// The first block: a NUL byte in it makes the file "binary" (as git decides).
const SNIFF: usize = 8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotFound,
    Binary,
}

/// What was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileText {
    pub text: String,
    /// The file's size when it was opened.
    pub size: u64,
    pub truncated: bool,
    pub lossy: bool,
}

/// The text of `path` (a path of the chat's folder, as text). `ctx` is a read
/// context of the session with `allow_outside_workspace` off (a context that
/// allows the outside is refused: a person's view never leaves the folder).
/// `scrub` is the node's secret scrubber.
pub async fn read_text(
    ctx: &ToolCtx,
    path: &str,
    scrub: impl Fn(&str) -> String + Send + 'static,
) -> Result<FileText, Refusal> {
    let real = crate::folder_preview::locate(ctx, path).map_err(|_| Refusal::NotFound)?;
    tokio::task::spawn_blocking(move || -> Result<FileText, Refusal> {
        let f = crate::open_regular(&real).map_err(|_| Refusal::NotFound)?;
        let size = f.metadata().map_err(|_| Refusal::NotFound)?.len();
        let mut f = f.take((MAX_TEXT_BYTES + MARGIN) as u64);
        // The first block alone decides "binary": a huge file of zeros costs
        // one block.
        let mut bytes = Vec::new();
        (&mut f)
            .take(SNIFF as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Refusal::NotFound)?;
        if bytes.contains(&0) {
            return Err(Refusal::Binary);
        }
        f.read_to_end(&mut bytes).map_err(|_| Refusal::NotFound)?;
        Ok(decode(&bytes, size, &scrub))
    })
    .await
    .map_err(|_| Refusal::NotFound)?
}

/// `bytes` (the file's start, at most the text and the margin) as text:
/// scrubbed whole, then cut at the bound on a character boundary.
fn decode(bytes: &[u8], size: u64, scrub: &dyn Fn(&str) -> String) -> FileText {
    let cut = (bytes.len() as u64) < size;
    // A read that stopped inside a character is not a bad byte.
    let mut whole = bytes;
    if cut
        && let Err(e) = std::str::from_utf8(bytes)
        && e.error_len().is_none()
    {
        whole = &bytes[..e.valid_up_to()];
    }
    let decoded = String::from_utf8_lossy(whole);
    let lossy = matches!(decoded, std::borrow::Cow::Owned(_));
    let mut text = scrub(&decoded);
    let truncated = size > MAX_TEXT_BYTES as u64;
    if text.len() > MAX_TEXT_BYTES {
        let mut at = MAX_TEXT_BYTES;
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        text.truncate(at);
    }
    FileText {
        text,
        size,
        truncated,
        lossy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
        std::fs::write(out.join("secret.txt"), "outside").unwrap();
        Rig { _d: d, ws, out }
    }

    fn keep(s: &str) -> String {
        s.to_string()
    }

    async fn ask(r: &Rig, path: &str) -> Result<FileText, Refusal> {
        read_text(&ToolCtx::new(&r.ws), path, keep).await
    }

    #[tokio::test]
    async fn a_file_of_the_folder_is_read_as_text() {
        let r = rig();
        std::fs::create_dir_all(r.ws.join("src")).unwrap();
        std::fs::write(r.ws.join("src/main.rs"), "fn main() {}\n").unwrap();
        let got = ask(&r, "src/main.rs").await.unwrap();
        assert_eq!(got.text, "fn main() {}\n");
        assert_eq!(got.size, 13);
        assert!(!got.truncated && !got.lossy);
        // An absolute path inside the folder is the same file.
        let abs = r.ws.join("src/main.rs").display().to_string();
        assert_eq!(ask(&r, &abs).await.unwrap(), got);
        // A link to a file inside is read.
        std::os::unix::fs::symlink(r.ws.join("src/main.rs"), r.ws.join("l.rs")).unwrap();
        assert_eq!(ask(&r, "l.rs").await.unwrap().text, got.text);
        // An empty file is empty text, not an error.
        std::fs::write(r.ws.join("e.txt"), "").unwrap();
        assert_eq!(ask(&r, "e.txt").await.unwrap().text, "");
    }

    /// Outside, absolute, a link out, a dotdot, `.git`, a protected folder, a
    /// missing file, a directory, a control character, too long: one answer,
    /// nothing read outside.
    #[tokio::test]
    async fn every_way_out_is_the_one_not_found() {
        use std::os::unix::fs::symlink;
        let r = rig();
        symlink(r.out.join("secret.txt"), r.ws.join("link.txt")).unwrap();
        symlink(&r.out, r.ws.join("dirlink")).unwrap();
        std::fs::create_dir_all(r.ws.join(".git")).unwrap();
        std::fs::write(r.ws.join(".git/config"), "[core]").unwrap();
        std::fs::create_dir_all(r.ws.join("data")).unwrap();
        std::fs::write(r.ws.join("data/key.txt"), "k").unwrap();
        std::fs::create_dir_all(r.ws.join("sub")).unwrap();
        let abs = r.out.join("secret.txt").display().to_string();
        let mut ctx = ToolCtx::new(&r.ws);
        ctx.protected = Arc::new(vec![r.ws.join("data")]);
        for p in [
            "../out/secret.txt",
            abs.as_str(),
            "link.txt",
            "dirlink/secret.txt",
            ".git/config",
            ".GIT/config",
            "data/key.txt",
            "nope.txt",
            "sub",
            ".",
            "",
            "a\nb.txt",
            "a\u{1b}[31m.txt",
            "/etc/passwd",
            &"a".repeat(crate::folder_preview::MAX_PATH_BYTES + 1),
        ] {
            assert_eq!(
                read_text(&ctx, p, keep).await,
                Err(Refusal::NotFound),
                "path {p:?}"
            );
        }
    }

    /// A node that lets the agent read outside does not let the page.
    #[tokio::test]
    async fn the_outside_mode_of_the_node_does_not_widen_a_view() {
        let r = rig();
        let mut ctx = ToolCtx::new(&r.ws);
        ctx.allow_outside_workspace = true;
        let abs = r.out.join("secret.txt").display().to_string();
        std::os::unix::fs::symlink(r.out.join("secret.txt"), r.ws.join("link.txt")).unwrap();
        for p in [abs.as_str(), "link.txt", "../out/secret.txt"] {
            assert_eq!(
                read_text(&ctx, p, keep).await,
                Err(Refusal::NotFound),
                "{p}"
            );
        }
    }

    /// A FIFO and a device are refused without opening (a FIFO would hang).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_fifo_and_a_device_do_not_hang() {
        let r = rig();
        nix::unistd::mkfifo(&r.ws.join("p.txt"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        std::os::unix::fs::symlink("/dev/zero", r.ws.join("z.txt")).unwrap();
        for p in ["p.txt", "z.txt"] {
            let got = tokio::time::timeout(std::time::Duration::from_secs(5), ask(&r, p))
                .await
                .expect("hung");
            assert_eq!(got, Err(Refusal::NotFound), "{p}");
        }
    }

    /// A 5 GB sparse file of zeros is "binary" after its first block; one
    /// whose first block is text is cut at the bound: neither reads it all.
    #[tokio::test]
    async fn a_huge_file_is_read_in_part_and_fast() {
        use std::io::Write as _;
        let r = rig();
        let zeros = std::fs::File::create(r.ws.join("zeros.bin")).unwrap();
        zeros.set_len(5 << 30).unwrap();
        let start = std::time::Instant::now();
        assert_eq!(ask(&r, "zeros.bin").await, Err(Refusal::Binary));
        let mut f = std::fs::File::create(r.ws.join("big.log")).unwrap();
        f.write_all(b"first line\n").unwrap();
        f.write_all(&vec![b'a'; SNIFF]).unwrap();
        f.set_len(5 << 30).unwrap();
        drop(f);
        // Text, then a hole of zeros: the zeros are NULs, shown as text after
        // the first block, never read past the bound.
        let got = ask(&r, "big.log").await.unwrap();
        assert!(got.truncated);
        assert_eq!(got.size, 5 << 30);
        assert!(got.text.starts_with("first line\n"));
        assert!(got.text.len() <= MAX_TEXT_BYTES);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }

    /// A NUL in the first block is binary; later NULs are text (shown as
    /// such by the page).
    #[tokio::test]
    async fn binary_is_told_by_the_first_block() {
        let r = rig();
        std::fs::write(r.ws.join("a.png"), b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(ask(&r, "a.png").await, Err(Refusal::Binary));
        let mut late = vec![b'a'; SNIFF];
        late.push(0);
        std::fs::write(r.ws.join("late.txt"), &late).unwrap();
        assert!(ask(&r, "late.txt").await.is_ok());
    }

    /// Exactly the bound is whole; one more byte is cut and says so; a cut
    /// never splits a character, and invalid bytes are shown as U+FFFD and
    /// said.
    #[tokio::test]
    async fn the_bound_the_characters_and_invalid_utf8() {
        let r = rig();
        std::fs::write(r.ws.join("whole.txt"), "a".repeat(MAX_TEXT_BYTES)).unwrap();
        let w = ask(&r, "whole.txt").await.unwrap();
        assert!(!w.truncated && w.text.len() == MAX_TEXT_BYTES);
        std::fs::write(r.ws.join("over.txt"), "a".repeat(MAX_TEXT_BYTES + 1)).unwrap();
        let o = ask(&r, "over.txt").await.unwrap();
        assert!(
            o.truncated && o.text.len() == MAX_TEXT_BYTES,
            "{}",
            o.text.len()
        );
        // A two-byte character across the bound.
        let mut s = "a".repeat(MAX_TEXT_BYTES - 1);
        s.push_str("é tail");
        std::fs::write(r.ws.join("e.txt"), &s).unwrap();
        let e = ask(&r, "e.txt").await.unwrap();
        assert!(e.truncated && !e.lossy);
        assert_eq!(e.text.len(), MAX_TEXT_BYTES - 1);
        // A file longer than the margin, cut inside a character, is not "lossy".
        let mut s = "a".repeat(MAX_TEXT_BYTES + MARGIN - 1);
        s.push_str("éééé");
        std::fs::write(r.ws.join("m.txt"), &s).unwrap();
        assert!(!ask(&r, "m.txt").await.unwrap().lossy);
        std::fs::write(r.ws.join("bad.txt"), b"ok \xff\xfe bad").unwrap();
        let b = ask(&r, "bad.txt").await.unwrap();
        assert!(b.lossy);
        assert_eq!(b.text, "ok \u{fffd}\u{fffd} bad");
        // A megabyte on one line is bounded like any other.
        std::fs::write(r.ws.join("line.txt"), "x".repeat(1 << 20)).unwrap();
        let l = ask(&r, "line.txt").await.unwrap();
        assert!(l.truncated && l.text.len() == MAX_TEXT_BYTES);
    }

    /// Markup, escapes and direction overrides are text, returned as they
    /// are (the page draws text and shows the hidden characters).
    #[tokio::test]
    async fn hostile_content_is_returned_as_inert_text() {
        let r = rig();
        let body = "<script>alert(1)</script>\n# md\n\u{1b}[31mred\u{1b}[0m\n\u{202e}evil\n";
        std::fs::write(r.ws.join("h.html"), body).unwrap();
        assert_eq!(ask(&r, "h.html").await.unwrap().text, body);
    }

    /// What the node's scrubber knows is not in the answer, also when it
    /// straddles the cut: the mutation (no scrubbing) fails this test.
    #[tokio::test]
    async fn a_secret_is_scrubbed_also_across_the_cut() {
        let r = rig();
        let key = "sk-live-0123456789abcdef";
        let scrub = move |s: &str| s.replace(key, "[REDACTED]");
        std::fs::write(r.ws.join("env.txt"), format!("KEY={key}\n")).unwrap();
        let got = read_text(&ToolCtx::new(&r.ws), "env.txt", scrub)
            .await
            .unwrap();
        assert_eq!(got.text, "KEY=[REDACTED]\n");
        // The key starts 10 bytes before the bound.
        let mut s = "a".repeat(MAX_TEXT_BYTES - 10);
        s.push_str(key);
        s.push_str(&"b".repeat(100));
        std::fs::write(r.ws.join("cut.txt"), &s).unwrap();
        let got = read_text(&ToolCtx::new(&r.ws), "cut.txt", scrub)
            .await
            .unwrap();
        assert!(!got.text.contains("sk-live"), "a prefix of the key shows");
        assert!(got.text.ends_with("[REDACTED]") || got.text.contains("[REDACTED]"));
        // Without the scrubber the key is there: the test would fail.
        let raw = ask(&r, "cut.txt").await.unwrap();
        assert!(raw.text.contains("sk-live"));
    }
}

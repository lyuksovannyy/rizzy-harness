//! `read` of an image file (ADOPTION_PLAN X.23, PLAN.md T71): decided by
//! the file's first bytes, never its name; bounded before anything is
//! loaded; never shown as text.

use std::os::unix::fs::symlink;
use std::time::Duration;

use serde_json::json;

use crate::{Read, Tool, ToolCtx, ToolOutcome, Write};

/// A PNG header (signature and IHDR) of `w` x `h`, then a few bytes.
pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.extend(b"\0\0\0\0IEND\xaeB`\x82");
    b
}

fn jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8];
    b.extend(h.to_be_bytes());
    b.extend(w.to_be_bytes());
    b.extend([1, 1, 0x11, 0, 0xFF, 0xD9]);
    b
}

fn gif(w: u16, h: u16) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    b.extend([0, 0, 0, 0x3B]);
    b
}

fn webp(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
    b.extend(&(w - 1).to_le_bytes()[..3]);
    b.extend(&(h - 1).to_le_bytes()[..3]);
    b
}

async fn quick(f: impl std::future::Future<Output = ToolOutcome>) -> ToolOutcome {
    tokio::time::timeout(Duration::from_secs(5), f)
        .await
        .expect("the tool blocked")
}

/// Every raster format is told by its bytes, whatever the name says, and
/// comes back as an image (the bytes, the type, the size from the header),
/// with a one-line description as the text, never the bytes as text.
#[tokio::test]
async fn png_jpeg_webp_gif_by_content_with_a_wrong_extension() {
    let ws = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(ws.path());
    for (name, bytes, mime, format) in [
        ("shot.txt", png(640, 480), "image/png", "PNG"),
        ("photo.png", jpeg(320, 200), "image/jpeg", "JPEG"),
        ("anim.jpg", gif(16, 9), "image/gif", "GIF"),
        ("design", webp(1200, 800), "image/webp", "WebP"),
    ] {
        std::fs::write(ws.path().join(name), &bytes).unwrap();
        let r = Read.run(&ctx, json!({ "path": name })).await;
        assert!(r.ok, "{name}: {}", r.output);
        assert!(r.output.contains(format), "{name}: {}", r.output);
        assert!(!r.output.contains("binary"), "{name}: {}", r.output);
        let img = r.image.as_ref().expect("an image");
        assert_eq!(img.mime, mime, "{name}");
        assert_eq!(img.bytes, bytes, "{name}");
        assert_eq!(img.sha256, crate::seen::hex(&crate::seen::sha256(&bytes)));
        assert!(img.width > 0 && img.height > 0, "{name}");
        // What the agent now knows of the file: its hash, so `write` over
        // it follows the read-first rule like any read.
        assert_eq!(r.seen.as_ref().unwrap().sha256, img.sha256, "{name}");
    }
}

/// A text file named `.png` is text, and an SVG (text that can carry a
/// script) stays a text read: neither becomes an image.
#[tokio::test]
async fn a_text_file_named_png_and_an_svg_are_read_as_text() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), "hello\n").unwrap();
    std::fs::write(
        ws.path().join("logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>\n",
    )
    .unwrap();
    let ctx = ToolCtx::new(ws.path());
    let r = Read.run(&ctx, json!({"path": "a.png"})).await;
    assert!(r.ok && r.output.contains("1\thello"), "{}", r.output);
    assert!(r.image.is_none());
    let r = Read.run(&ctx, json!({"path": "logo.svg"})).await;
    assert!(r.ok && r.output.contains("<svg"), "{}", r.output);
    assert!(r.image.is_none());
}

/// Over the cap (3.5 MiB, so the base64 fits the providers' 5 MB) an image
/// is refused with a note before it is loaded: one byte over (3.5 MiB was
/// fine before; 4.5 MiB was sent and refused by the provider), 6 MiB, and
/// 50 MB at once. Exactly the cap is read.
#[tokio::test]
async fn an_image_over_the_cap_is_refused_before_it_is_loaded() {
    let ws = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(ws.path());
    let cap = harness_core::image::MAX_FILE_BYTES;
    for (name, len, ok) in [
        ("cap.png", cap, true),
        ("over.png", cap + 1, false),
        ("four.png", 9u64 << 19, false),
        ("six.png", 6u64 << 20, false),
        ("fifty.png", 50_000_000, false),
    ] {
        let p = ws.path().join(name);
        std::fs::write(&p, png(100, 100)).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_len(len)
            .unwrap();
        let r = quick(Read.run(&ctx, json!({ "path": name }))).await;
        if ok {
            assert!(r.ok, "{name}: {}", r.output);
            assert_eq!(r.image.unwrap().bytes.len() as u64, cap);
            continue;
        }
        assert!(!r.ok, "{name}: {}", r.output);
        assert!(r.output.contains("3.5 MiB"), "{name}: {}", r.output);
        assert!(r.image.is_none());
    }
}

/// A FIFO named like an image is refused at once, as any non-regular file;
/// a link out of the folder is refused as any path outside it.
#[tokio::test]
async fn a_fifo_and_a_symlink_out_are_refused() {
    let ws = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    nix::unistd::mkfifo(
        &ws.path().join("p.png"),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .unwrap();
    std::fs::write(out.path().join("secret.png"), png(10, 10)).unwrap();
    symlink(out.path().join("secret.png"), ws.path().join("link.png")).unwrap();
    let ctx = ToolCtx::new(ws.path());
    let r = quick(Read.run(&ctx, json!({"path": "p.png"}))).await;
    assert!(
        !r.ok && r.output.contains("not a regular file"),
        "{}",
        r.output
    );
    let r = quick(Read.run(&ctx, json!({"path": "link.png"}))).await;
    assert!(!r.ok, "{}", r.output);
    assert!(r.image.is_none());
}

/// A file that starts like an image but whose header cannot be read is
/// refused (no panic), and so is one larger than providers take per side;
/// one whose header is whole but whose body is cut goes as it is (no image
/// is decoded here, so the provider judges the rest).
#[tokio::test]
async fn a_corrupt_or_oversized_header_is_refused_and_a_cut_body_is_sent() {
    let ws = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(ws.path());
    std::fs::write(ws.path().join("bad.png"), b"\x89PNG\r\n\x1a\nnope").unwrap();
    std::fs::write(ws.path().join("bad.jpg"), [0xFF, 0xD8, 0xFF, 0xDA]).unwrap();
    std::fs::write(ws.path().join("tall.png"), png(100, 9_000)).unwrap();
    for name in ["bad.png", "bad.jpg", "tall.png"] {
        let r = Read.run(&ctx, json!({ "path": name })).await;
        assert!(!r.ok, "{name}: {}", r.output);
        assert!(r.image.is_none(), "{name}");
        assert!(!r.output.contains('\u{fffd}'), "{name}: {}", r.output);
    }
    let mut cut = png(64, 64);
    cut.truncate(33);
    std::fs::write(ws.path().join("cut.png"), &cut).unwrap();
    let r = Read.run(&ctx, json!({"path": "cut.png"})).await;
    assert!(r.ok, "{}", r.output);
    assert_eq!(r.image.unwrap().bytes, cut);
}

/// An image read returns what the agent would know of the file (its hash)
/// but does not record it: the loop does, and only when the model was
/// shown the image (a model without vision saw nothing, review of PR 130).
/// So `read` alone never lets `write` replace it, nor `edit` (which refuses
/// a binary) change it.
#[tokio::test]
async fn an_image_read_follows_the_read_first_rule() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(4, 4)).unwrap();
    let ctx = ToolCtx::new(ws.path());
    let w = Write
        .run(&ctx, json!({"path": "a.png", "content": "x"}))
        .await;
    assert!(!w.ok, "unread: {}", w.output);
    let r = Read.run(&ctx, json!({"path": "a.png"})).await;
    assert!(r.ok, "{}", r.output);
    assert!(r.seen.is_some());
    let w = Write
        .run(&ctx, json!({"path": "a.png", "content": "x"}))
        .await;
    assert!(!w.ok, "the tool alone records nothing: {}", w.output);
    let e = crate::Edit
        .run(
            &ctx,
            json!({"path": "a.png", "old_string": "PNG", "new_string": "GIF"}),
        )
        .await;
    assert!(!e.ok, "edit of an image: {}", e.output);
    std::fs::write(ws.path().join("a.png"), png(5, 5)).unwrap();
    let w = Write
        .run(&ctx, json!({"path": "a.png", "content": "x"}))
        .await;
    assert!(!w.ok, "changed since the read: {}", w.output);
}

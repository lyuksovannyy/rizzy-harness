//! Images sent with a prompt (`session.prompt.images`, docs/media.md). The
//! master already checked owner, kind, model and size; the node checks the
//! limits and the bytes again before keeping them, because they go into its
//! own database and to the model provider.

use base64::Engine as _;
use harness_core::event::ImageRef;
use harness_protocol::{ChatImage, MAX_IMAGE_BYTES, MAX_IMAGES};
use harness_store::session_images::SessionImage;
use sha2::{Digest, Sha256};

/// The image types every vision provider takes.
const MIMES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

/// The references a message records and the bytes the chat keeps.
pub(crate) fn accept(images: Vec<ChatImage>) -> Result<(Vec<ImageRef>, Vec<SessionImage>), String> {
    if images.len() > MAX_IMAGES {
        return Err(format!("at most {MAX_IMAGES} images per message"));
    }
    let mut refs = Vec::with_capacity(images.len());
    let mut kept = Vec::with_capacity(images.len());
    let mut total = 0u64;
    for img in images {
        if !MIMES.contains(&img.mime.as_str()) {
            return Err(format!("{}: not an image type models take", img.name));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(img.data.as_bytes())
            .map_err(|_| format!("{}: the image data is not base64", img.name))?;
        total += bytes.len() as u64;
        if total > MAX_IMAGE_BYTES {
            return Err("too much attached to one message".into());
        }
        // The chat keys its copy by this; a wrong one would show another
        // image under this name later.
        if hex::encode(Sha256::digest(&bytes)) != img.sha256 {
            return Err(format!("{}: the image does not match its sha256", img.name));
        }
        refs.push(img.image_ref());
        kept.push(SessionImage {
            sha256: img.sha256,
            mime: img.mime,
            name: img.name,
            bytes,
        });
    }
    Ok((refs, kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(bytes: &[u8], mime: &str) -> ChatImage {
        ChatImage {
            media_id: "m".into(),
            sha256: hex::encode(Sha256::digest(bytes)),
            mime: mime.into(),
            name: "a.png".into(),
            width: 1,
            height: 1,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    #[test]
    fn checks_count_type_size_and_hash() {
        let (refs, kept) = accept(vec![img(b"png", "image/png")]).unwrap();
        assert_eq!(refs[0].media_id, "m");
        assert_eq!(kept[0].bytes, b"png");
        assert!(accept(vec![img(b"x", "image/png"); 5]).is_err());
        assert!(accept(vec![img(b"x", "image/svg+xml")]).is_err());
        let mut lie = img(b"x", "image/png");
        lie.sha256 = "00".repeat(32);
        assert!(accept(vec![lie]).is_err());
        let mut bad = img(b"x", "image/png");
        bad.data = "%%%".into();
        assert!(accept(vec![bad]).is_err());
        let big = vec![0u8; (MAX_IMAGE_BYTES / 2) as usize + 1];
        assert!(accept(vec![img(&big, "image/png"), img(&big, "image/png")]).is_err());
    }
}

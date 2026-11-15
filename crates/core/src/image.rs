//! Raster images the model may be sent (docs/media.md, PLAN.md T71): which
//! formats, from the bytes alone, and their size from the header. Headers
//! are only read, never decoded, so no image is decoded anywhere in the
//! harness process (T17); every reader is bounds-checked against short or
//! hostile input. The master's sniffer (`crates/master/src/sniff.rs`) and
//! the `read` tool share these.

/// The most bytes of images one message carries (the upload path), and of
/// one image file `read` returns (X.23).
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// The longest side `read` sends, in pixels: the largest Anthropic's
/// Messages API takes (it refuses a request with a larger image); others
/// downscale. A larger image is refused with a note, never sent.
pub const MAX_SIDE: u32 = 8_000;

/// A raster image every vision provider reads: PNG, JPEG, WebP or GIF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raster {
    /// `image/png`, `image/jpeg`, `image/webp`, `image/gif`.
    pub mime: &'static str,
    /// `PNG`, `JPEG`, `WebP`, `GIF`, as a person reads it.
    pub label: &'static str,
    /// The format's short name (`png`, `jpeg`, `webp`, `gif`).
    pub format: &'static str,
}

const FORMATS: [Raster; 4] = [
    Raster {
        mime: "image/png",
        label: "PNG",
        format: "png",
    },
    Raster {
        mime: "image/jpeg",
        label: "JPEG",
        format: "jpeg",
    },
    Raster {
        mime: "image/webp",
        label: "WebP",
        format: "webp",
    },
    Raster {
        mime: "image/gif",
        label: "GIF",
        format: "gif",
    },
];

/// The raster format the bytes start with, by their magic number alone
/// (never a name or a declared type).
pub fn raster(head: &[u8]) -> Option<Raster> {
    let i = if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        0
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        1
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        2
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        3
    } else {
        return None;
    };
    Some(FORMATS[i])
}

/// Width and height of a PNG, JPEG, WebP or GIF from its header (`format`
/// as [`Raster::format`]); `None` for another format, a header that cannot
/// be read, or a zero side.
pub fn dimensions(b: &[u8], format: &str) -> Option<(u32, u32)> {
    let (w, h) = match format {
        "png" => png(b)?,
        "jpeg" => jpeg(b)?,
        "gif" => (u32::from(le16(b, 6)?), u32::from(le16(b, 8)?)),
        "webp" => webp(b)?,
        _ => return None,
    };
    (w > 0 && h > 0).then_some((w, h))
}

fn png(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") || b.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((be32(b, 16)?, be32(b, 20)?))
}

/// The first SOF segment's size; markers are walked by their lengths.
fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    loop {
        while *b.get(i)? != 0xFF {
            i += 1;
        }
        while *b.get(i)? == 0xFF {
            i += 1;
        }
        let marker = *b.get(i)?;
        i += 1;
        match marker {
            0x01 | 0xD0..=0xD8 => continue,
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let len = usize::from(be16(b, i)?);
        let sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if sof {
            let h = be16(b, i + 3)?;
            let w = be16(b, i + 5)?;
            return Some((u32::from(w), u32::from(h)));
        }
        if len < 2 {
            return None;
        }
        i = i.checked_add(len)?;
    }
}

fn webp(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"RIFF") || b.get(8..12)? != b"WEBP" {
        return None;
    }
    match b.get(12..16)? {
        b"VP8 " => {
            if b.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            let w = u32::from(le16(b, 26)? & 0x3FFF);
            let h = u32::from(le16(b, 28)? & 0x3FFF);
            Some((w, h))
        }
        b"VP8L" => {
            if *b.get(20)? != 0x2F {
                return None;
            }
            let bits = le32(b, 21)?;
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => {
            let w = le24(b, 24)? + 1;
            let h = le24(b, 27)? + 1;
            Some((w, h))
        }
        _ => None,
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(3)?)?;
    Some(u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_by_magic_only() {
        assert_eq!(raster(b"\x89PNG\r\n\x1a\nrest").unwrap().mime, "image/png");
        assert_eq!(raster(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap().format, "jpeg");
        assert_eq!(raster(b"RIFF\0\0\0\0WEBPVP8 ").unwrap().label, "WebP");
        assert_eq!(raster(b"GIF89a..").unwrap().mime, "image/gif");
        for no in [
            &b""[..],
            b"<svg",
            b"BM....",
            b"RIFF\0\0\0\0WAVE",
            b"GIF8",
            b"\x89PN",
        ] {
            assert!(raster(no).is_none(), "{no:?}");
        }
    }

    /// Short, cut and hostile headers: `None`, never a panic.
    #[test]
    fn hostile_headers_never_panic() {
        let mut r = 0x2545_F491_4F6C_DD1Du64;
        for len in 0..200usize {
            for _ in 0..50 {
                let mut b: Vec<u8> = (0..len)
                    .map(|_| {
                        r ^= r << 13;
                        r ^= r >> 7;
                        r ^= r << 17;
                        r as u8
                    })
                    .collect();
                for (k, magic) in [
                    &b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR"[..],
                    &[0xFF, 0xD8, 0xFF],
                    b"RIFF\0\0\0\0WEBPVP8X",
                    b"GIF89a",
                ]
                .iter()
                .enumerate()
                {
                    let n = magic.len().min(b.len());
                    b[..n].copy_from_slice(&magic[..n]);
                    let _ = dimensions(&b, FORMATS[k].format);
                }
            }
        }
        assert_eq!(dimensions(&[0xFF, 0xD8, 0xFF, 0xDA], "jpeg"), None);
        assert_eq!(dimensions(b"GIF89a\0\0\x01\0", "gif"), None);
    }
}

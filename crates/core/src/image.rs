//! Raster images the model may be sent (docs/media.md, PLAN.md T71): which
//! formats, from the bytes alone, and their size from the header. Headers
//! are only read, never decoded, so no image is decoded anywhere in the
//! harness process (T17); every reader is bounds-checked against short or
//! hostile input. The master's sniffer (`crates/master/src/sniff.rs`) and
//! the `read` tool share these.

/// The most bytes of images one message carries together (the upload
/// path), and of the images one step or one request of `read` shows
/// together (X.23): a total of raw bytes; each image is also at most
/// [`MAX_FILE_BYTES`].
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// The most bytes of one image sent to a model, read or attached. The
/// provider's limit is on what it receives, the base64: Anthropic refuses
/// an image whose base64 is over 5 MiB (Pi caps the base64 at 4.5 MB,
/// OpenCode at `5 * 1024 * 1024`). Base64 is `4 * ceil(n / 3)` bytes, so
/// 3.5 MiB raw (3,670,016 bytes) is 4,893,356 bytes of base64: 349,524
/// bytes (6.7%) under 5 MiB. A file of 3.75 to 5 MiB, under the old cap,
/// was refused by the provider on every request that showed it.
pub const MAX_FILE_BYTES: u64 = 7 * 512 * 1024;

/// The base64 length of `n` bytes (standard alphabet, padded).
pub const fn base64_len(n: u64) -> u64 {
    n.div_ceil(3) * 4
}

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

/// Why [`still`] refused a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotStill {
    /// It carries metadata: EXIF or XMP (camera, time, location), IPTC, or
    /// PNG text chunks.
    Metadata,
    /// More than one frame: an APNG, an animated WebP.
    Animated,
    /// Its structure cannot be walked to the end: cut, a length past the
    /// end, bytes after its end, more than one frame header or image chunk.
    Malformed,
}

/// The size of a still PNG, JPEG or WebP (`format` as [`Raster::format`])
/// that carries no metadata, by walking its whole structure without
/// decoding a pixel (PLAN.md T74). The size is the largest every header in
/// the file claims (a WebP's canvas and its bitstream), so a later header
/// cannot hide a larger picture. Every length is checked against the
/// bytes there are, nothing is allocated from a declared size, and every
/// step moves forward, so a hostile file ends the walk in at most
/// `b.len()` steps.
pub fn still(b: &[u8], format: &str) -> Result<(u32, u32), NotStill> {
    let (w, h) = match format {
        "png" => png_walk(b)?,
        "jpeg" => jpeg_walk(b)?,
        "webp" => webp_walk(b)?,
        _ => return Err(NotStill::Malformed),
    };
    if w == 0 || h == 0 {
        return Err(NotStill::Malformed);
    }
    Ok((w, h))
}

fn png_walk(b: &[u8]) -> Result<(u32, u32), NotStill> {
    use NotStill::*;
    let size = png(b).ok_or(Malformed)?;
    let mut at = 8usize;
    let mut first = true;
    loop {
        let len = be32(b, at).ok_or(Malformed)? as usize;
        let kind = b.get(at + 4..at + 8).ok_or(Malformed)?;
        // Length, type, data, CRC: all of it must be there.
        let next = at
            .checked_add(12)
            .and_then(|n| n.checked_add(len))
            .filter(|n| *n <= b.len())
            .ok_or(Malformed)?;
        if first != (kind == b"IHDR") {
            return Err(Malformed);
        }
        first = false;
        match kind {
            b"eXIf" | b"tEXt" | b"iTXt" | b"zTXt" => return Err(Metadata),
            b"acTL" | b"fcTL" | b"fdAT" => return Err(Animated),
            b"IEND" => {
                // Nothing may follow the end (a page or script appended).
                return if next == b.len() {
                    Ok(size)
                } else {
                    Err(Malformed)
                };
            }
            _ => {}
        }
        at = next;
    }
}

fn jpeg_walk(b: &[u8]) -> Result<(u32, u32), NotStill> {
    use NotStill::*;
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err(Malformed);
    }
    let mut size = None;
    let mut at = 2usize;
    loop {
        if *b.get(at).ok_or(Malformed)? != 0xFF {
            return Err(Malformed);
        }
        // Fill bytes before a marker.
        while *b.get(at).ok_or(Malformed)? == 0xFF {
            at += 1;
        }
        let marker = b[at];
        at += 1;
        match marker {
            0xD9 => {
                // End of image: nothing may follow.
                return match size {
                    Some(s) if at == b.len() => Ok(s),
                    _ => Err(Malformed),
                };
            }
            0x01 | 0xD0..=0xD7 => continue,
            0x00 | 0xD8 => return Err(Malformed),
            _ => {}
        }
        let len = usize::from(be16(b, at).ok_or(Malformed)?);
        if len < 2 {
            return Err(Malformed);
        }
        let end = at
            .checked_add(len)
            .filter(|e| *e <= b.len())
            .ok_or(Malformed)?;
        match marker {
            // APP1 is EXIF or XMP; APP13 is Photoshop's IPTC.
            0xE1 | 0xED => return Err(Metadata),
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                // One frame header only: a second could claim another size.
                if size.is_some() {
                    return Err(Malformed);
                }
                let h = be16(b, at + 3).ok_or(Malformed)?;
                let w = be16(b, at + 5).ok_or(Malformed)?;
                size = Some((u32::from(w), u32::from(h)));
            }
            _ => {}
        }
        at = end;
        if marker == 0xDA {
            // Entropy-coded data up to the next marker that is not a
            // stuffed zero or a restart.
            loop {
                if *b.get(at).ok_or(Malformed)? == 0xFF {
                    match *b.get(at + 1).ok_or(Malformed)? {
                        0x00 | 0xD0..=0xD7 => at += 2,
                        0xFF => at += 1,
                        _ => break,
                    }
                } else {
                    at += 1;
                }
            }
        }
    }
}

fn webp_walk(b: &[u8]) -> Result<(u32, u32), NotStill> {
    use NotStill::*;
    if !b.starts_with(b"RIFF") || b.get(8..12) != Some(b"WEBP") {
        return Err(Malformed);
    }
    let riff = le32(b, 4).ok_or(Malformed)? as usize;
    // The RIFF size covers everything after its own field, exactly.
    if riff.checked_add(8) != Some(b.len()) {
        return Err(Malformed);
    }
    let (mut w, mut h) = (0u32, 0u32);
    let mut images = 0;
    let mut at = 12usize;
    let mut first = true;
    while at < b.len() {
        let kind = b.get(at..at + 4).ok_or(Malformed)?;
        let len = le32(b, at + 4).ok_or(Malformed)? as usize;
        let data = at + 8;
        let end = data
            .checked_add(len)
            .filter(|e| *e <= b.len())
            .ok_or(Malformed)?;
        let chunk = &b[data..end];
        let mut grow = |cw: u32, ch: u32| {
            w = w.max(cw);
            h = h.max(ch);
        };
        match kind {
            b"VP8X" => {
                if !first {
                    return Err(Malformed);
                }
                let flags = *chunk.first().ok_or(Malformed)?;
                if flags & 0x02 != 0 {
                    return Err(Animated);
                }
                if flags & 0x0C != 0 {
                    return Err(Metadata);
                }
                grow(
                    le24(chunk, 4).ok_or(Malformed)? + 1,
                    le24(chunk, 7).ok_or(Malformed)? + 1,
                );
            }
            b"VP8 " => {
                if chunk.get(3..6) != Some(&[0x9D, 0x01, 0x2A][..]) {
                    return Err(Malformed);
                }
                images += 1;
                grow(
                    u32::from(le16(chunk, 6).ok_or(Malformed)? & 0x3FFF),
                    u32::from(le16(chunk, 8).ok_or(Malformed)? & 0x3FFF),
                );
            }
            b"VP8L" => {
                if chunk.first() != Some(&0x2F) {
                    return Err(Malformed);
                }
                let bits = le32(chunk, 1).ok_or(Malformed)?;
                images += 1;
                grow((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1);
            }
            b"EXIF" | b"XMP " => return Err(Metadata),
            b"ANIM" | b"ANMF" => return Err(Animated),
            _ => {}
        }
        first = false;
        // Chunks are padded to an even length.
        at = end + (len & 1);
    }
    if images != 1 || at != b.len() {
        return Err(Malformed);
    }
    Ok((w, h))
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

    /// A PNG of `w` × `h` with `extra` chunks (type, data) before its end.
    pub(super) fn tiny_png(w: u32, h: u32, extra: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            b.extend((data.len() as u32).to_be_bytes());
            b.extend(kind);
            b.extend(data);
            b.extend([0u8; 4]);
        };
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        for (k, d) in extra {
            chunk(k, d);
        }
        chunk(b"IDAT", &[0; 4]);
        chunk(b"IEND", &[]);
        b
    }

    /// A JPEG with `segments` (marker, data) and one frame header per `sofs`.
    pub(super) fn tiny_jpeg(sofs: &[(u8, u16, u16)], segments: &[(u8, &[u8])]) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
        b.extend(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        for (m, d) in segments {
            b.extend([0xFF, *m]);
            b.extend(((d.len() + 2) as u16).to_be_bytes());
            b.extend(*d);
        }
        for (m, w, h) in sofs {
            b.extend([0xFF, *m, 0x00, 0x11, 0x08]);
            b.extend(h.to_be_bytes());
            b.extend(w.to_be_bytes());
            b.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        }
        b.extend([0xFF, 0xDA, 0, 12, 3, 1, 0, 2, 0x11, 3, 0x11, 0, 0x3F, 0]);
        b.extend([0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]);
        b.extend([0xFF, 0xD9]);
        b
    }

    /// A WebP: a VP8X header with `vp8x` (flags, canvas) when given, a VP8
    /// bitstream of `inner`, and `extra` chunks.
    pub(super) fn tiny_webp(
        vp8x: Option<(u8, (u32, u32))>,
        inner: (u16, u16),
        extra: &[(&[u8; 4], &[u8])],
    ) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            body.extend(kind);
            body.extend((data.len() as u32).to_le_bytes());
            body.extend(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        };
        if let Some((flags, (w, h))) = vp8x {
            let mut d = vec![flags, 0, 0, 0];
            d.extend(&(w - 1).to_le_bytes()[..3]);
            d.extend(&(h - 1).to_le_bytes()[..3]);
            chunk(b"VP8X", &d);
        }
        for (k, d) in extra {
            chunk(k, d);
        }
        let mut v = vec![0, 0, 0, 0x9D, 0x01, 0x2A];
        v.extend(inner.0.to_le_bytes());
        v.extend(inner.1.to_le_bytes());
        chunk(b"VP8 ", &v);
        let mut b = b"RIFF".to_vec();
        b.extend((body.len() as u32).to_le_bytes());
        b.extend(body);
        b
    }

    /// Metadata, animation and a second size are refused; what a browser's
    /// canvas writes (plain chunks) is not (PLAN.md T74).
    #[test]
    fn still_refuses_metadata_animation_and_hidden_sizes() {
        use NotStill::*;
        assert_eq!(
            still(
                &tiny_png(64, 32, &[(b"iCCP", b"x"), (b"pHYs", &[0; 9])]),
                "png"
            ),
            Ok((64, 32))
        );
        for (k, why) in [
            (b"eXIf", Metadata),
            (b"tEXt", Metadata),
            (b"iTXt", Metadata),
            (b"zTXt", Metadata),
            (b"acTL", Animated),
        ] {
            assert_eq!(
                still(&tiny_png(8, 8, &[(k, b"GPS")]), "png"),
                Err(why),
                "{k:?}"
            );
        }
        let mut after = tiny_png(8, 8, &[]);
        after.extend(b"<html><script>alert(1)</script>");
        assert_eq!(still(&after, "png"), Err(Malformed));

        assert_eq!(
            still(
                &tiny_jpeg(&[(0xC2, 300, 200)], &[(0xE2, b"ICC"), (0xEE, b"Adobe")]),
                "jpeg"
            ),
            Ok((300, 200))
        );
        let gps = b"Exif\0\0MM\0*\0\0\0\x08GPSLatitude=56.9496N";
        assert_eq!(
            still(&tiny_jpeg(&[(0xC0, 16, 16)], &[(0xE1, gps)]), "jpeg"),
            Err(Metadata)
        );
        assert_eq!(
            still(
                &tiny_jpeg(
                    &[(0xC0, 16, 16)],
                    &[(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x/>")]
                ),
                "jpeg"
            ),
            Err(Metadata)
        );
        assert_eq!(
            still(
                &tiny_jpeg(&[(0xC0, 16, 16)], &[(0xED, b"Photoshop 3.0")]),
                "jpeg"
            ),
            Err(Metadata)
        );
        assert_eq!(
            still(
                &tiny_jpeg(&[(0xC0, 16, 16), (0xC2, 65_000, 65_000)], &[]),
                "jpeg"
            ),
            Err(Malformed)
        );
        let mut after = tiny_jpeg(&[(0xC0, 16, 16)], &[]);
        after.extend(b"<html>");
        assert_eq!(still(&after, "jpeg"), Err(Malformed));

        assert_eq!(
            still(&tiny_webp(Some((0x10, (100, 50))), (100, 50), &[]), "webp"),
            Ok((100, 50))
        );
        // A small canvas over a large bitstream: the larger size counts.
        assert_eq!(
            still(&tiny_webp(Some((0, (100, 100))), (5000, 5000), &[]), "webp"),
            Ok((5000, 5000))
        );
        assert_eq!(
            still(&tiny_webp(Some((0x02, (64, 64))), (64, 64), &[]), "webp"),
            Err(Animated)
        );
        assert_eq!(
            still(&tiny_webp(None, (64, 64), &[(b"ANIM", &[0; 6])]), "webp"),
            Err(Animated)
        );
        assert_eq!(
            still(&tiny_webp(Some((0x08, (64, 64))), (64, 64), &[]), "webp"),
            Err(Metadata)
        );
        assert_eq!(
            still(&tiny_webp(None, (64, 64), &[(b"EXIF", b"GPS")]), "webp"),
            Err(Metadata)
        );
        assert_eq!(
            still(&tiny_webp(None, (64, 64), &[(b"XMP ", b"<x/>")]), "webp"),
            Err(Metadata)
        );
        let mut two = tiny_webp(None, (64, 64), &[]);
        let extra = two[12..].to_vec();
        two.extend(extra);
        let n = (two.len() - 8) as u32;
        two[4..8].copy_from_slice(&n.to_le_bytes());
        assert_eq!(still(&two, "webp"), Err(Malformed), "two bitstreams");
        assert_eq!(still(&tiny_png(8, 8, &[]), "gif"), Err(Malformed));
    }

    /// The cap is on what the provider receives: the largest file's base64
    /// is under Anthropic's 5 MiB per image, with room to spare.
    #[test]
    fn the_largest_file_fits_five_mib_of_base64() {
        assert_eq!(MAX_FILE_BYTES, 3_670_016);
        assert_eq!(base64_len(MAX_FILE_BYTES), 4_893_356);
        assert!(base64_len(MAX_FILE_BYTES) <= 5 * 1024 * 1024 - 300_000);
        assert!(
            base64_len(MAX_BYTES) > 5 * 1024 * 1024,
            "the old cap did not fit"
        );
        use base64::Engine as _;
        let b = vec![7u8; MAX_FILE_BYTES as usize];
        let enc = base64::engine::general_purpose::STANDARD.encode(&b);
        assert_eq!(enc.len() as u64, base64_len(MAX_FILE_BYTES));
    }

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
        // The full walks: the same hostile bytes, and a valid start cut at
        // every length, end without a panic or a hang.
        let mut r = 0x9E37_79B9_7F4A_7C15u64;
        for (format, good) in [
            ("png", tiny_png(16, 16, &[])),
            ("jpeg", tiny_jpeg(&[(0xC0, 16, 16)], &[])),
            ("webp", tiny_webp(None, (16, 16), &[])),
        ] {
            assert_eq!(still(&good, format), Ok((16, 16)), "{format}");
            for cut in 0..good.len() {
                assert!(
                    still(&good[..cut], format).is_err(),
                    "{format} cut at {cut}"
                );
            }
            for _ in 0..2000 {
                let mut b = good.clone();
                for _ in 0..4 {
                    r ^= r << 13;
                    r ^= r >> 7;
                    r ^= r << 17;
                    let i = (r as usize) % b.len();
                    b[i] = (r >> 32) as u8;
                }
                let _ = still(&b, format);
            }
        }
        assert_eq!(dimensions(b"GIF89a\0\0\x01\0", "gif"), None);
    }
}

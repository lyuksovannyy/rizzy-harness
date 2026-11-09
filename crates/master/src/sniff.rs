//! What a stored file is, from its bytes (docs/media.md): kind, format, the
//! type it is served with and how the browser may show it. Never by
//! extension: a declared type or file name may only narrow within the
//! family the bytes belong to (`audio/webm` makes a webm audio), never cross
//! it, so an HTML page named `x.png` stays a document.
//!
//! The `infer` crate covers most magic numbers. Hand-written here: SVG
//! (text), the ISO-BMFF brands (HEIC, AVIF, QuickTime, MP4, M4A), Ogg
//! codecs, executable targets (ELF, PE, Mach-O) and image dimensions from
//! headers. Headers are only read, never decoded, so the server decodes no
//! image (T17), and every reader is bounds-checked against short input.

/// How much of a file's start the sniffer and the dimension readers see.
/// More than `infer` needs (8 KiB), because a JPEG's size comes after its
/// EXIF and ICC segments.
pub const HEAD: usize = 256 * 1024;

/// What the bytes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sniffed {
    /// `image` | `video` | `audio` | `binary` | `package` | `archive` |
    /// `document` | `other`.
    pub kind: &'static str,
    /// `png`, `mp4`, `mov`, `heic`, `elf`, `tar.gz`, ... (`bin` if unknown).
    pub format: String,
    /// What it is served with when displayed; `application/octet-stream`
    /// for anything only ever downloaded.
    pub mime: String,
    /// `inline` (served as its own type), `img` (its own type only to an
    /// `<img>`), `download`.
    pub display: &'static str,
    /// Image size from the header.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// An executable's architecture and OS ("x86_64 Linux").
    pub target: Option<String>,
}

const OCTET: &str = "application/octet-stream";

/// A format the browser may show: its kind, type and display.
struct Shown {
    format: &'static str,
    kind: &'static str,
    mime: &'static str,
    display: &'static str,
}

/// Every format served as its own type (§5.3's table); everything else is
/// a download.
const SHOWN: &[Shown] = &[
    s("png", "image", "image/png", "inline"),
    s("jpeg", "image", "image/jpeg", "inline"),
    s("gif", "image", "image/gif", "inline"),
    s("webp", "image", "image/webp", "inline"),
    s("avif", "image", "image/avif", "inline"),
    s("bmp", "image", "image/bmp", "inline"),
    s("ico", "image", "image/vnd.microsoft.icon", "inline"),
    s("svg", "image", "image/svg+xml", "img"),
    s("heic", "image", "image/heic", "img"),
    s("heif", "image", "image/heif", "img"),
    s("mp4", "video", "video/mp4", "inline"),
    s("webm", "video", "video/webm", "inline"),
    s("ogg", "video", "video/ogg", "inline"),
    s("mov", "video", "video/quicktime", "inline"),
    s("mp3", "audio", "audio/mpeg", "inline"),
    s("ogg", "audio", "audio/ogg", "inline"),
    s("wav", "audio", "audio/wav", "inline"),
    s("flac", "audio", "audio/flac", "inline"),
    s("m4a", "audio", "audio/mp4", "inline"),
    s("aac", "audio", "audio/aac", "inline"),
    s("webm", "audio", "audio/webm", "inline"),
];

const fn s(
    format: &'static str,
    kind: &'static str,
    mime: &'static str,
    display: &'static str,
) -> Shown {
    Shown {
        format,
        kind,
        mime,
        display,
    }
}

/// The extensions a format's files carry, the canonical one first (a
/// download whose name lacks all of them gets the first appended).
pub fn extensions(format: &str) -> &'static [&'static str] {
    match format {
        "png" => &["png"],
        "jpeg" => &["jpg", "jpeg", "jpe", "jfif"],
        "gif" => &["gif"],
        "webp" => &["webp"],
        "avif" => &["avif"],
        "bmp" => &["bmp"],
        "ico" => &["ico"],
        "svg" => &["svg"],
        "heic" => &["heic"],
        "heif" => &["heif", "heifs", "heic"],
        "tiff" => &["tif", "tiff"],
        "jxl" => &["jxl"],
        "psd" => &["psd"],
        "mp4" => &["mp4", "m4v"],
        "m4v" => &["m4v", "mp4"],
        "webm" => &["webm", "weba"],
        "ogg" => &["ogg", "ogv", "oga", "opus"],
        "mov" => &["mov", "qt"],
        "mkv" => &["mkv"],
        "avi" => &["avi"],
        "3gp" => &["3gp"],
        "mp3" => &["mp3"],
        "wav" => &["wav"],
        "flac" => &["flac"],
        "m4a" => &["m4a", "m4b", "mp4"],
        "aac" => &["aac"],
        "midi" => &["mid", "midi"],
        "aiff" => &["aif", "aiff"],
        "pdf" => &["pdf"],
        "html" => &["html", "htm"],
        "xml" => &["xml"],
        "zip" => &["zip"],
        "jar" => &["jar"],
        "war" => &["war"],
        "apk" => &["apk"],
        "aab" => &["aab"],
        "whl" => &["whl"],
        "nupkg" => &["nupkg"],
        "tar" => &["tar"],
        "gz" => &["gz"],
        "tar.gz" => &["tar.gz", "tgz"],
        "bz2" => &["bz2"],
        "xz" => &["xz"],
        "zst" => &["zst"],
        "7z" => &["7z"],
        "rar" => &["rar"],
        "deb" => &["deb"],
        "rpm" => &["rpm"],
        "exe" => &["exe"],
        "dll" => &["dll"],
        "wasm" => &["wasm"],
        "txt" => &["txt", "md", "log", "csv", "json", "toml", "yaml", "yml"],
        "md" => &["md", "markdown"],
        _ => &[],
    }
}

/// Zip-based formats named by their extension (the bytes are a plain zip).
const ZIP_PACKAGES: &[&str] = &["jar", "war", "apk", "aab", "whl", "nupkg"];

/// Classify a file from its first bytes (`head`, up to `HEAD`), with the
/// name and `Content-Type` it was declared with, which only narrow.
pub fn sniff(head: &[u8], name: Option<&str>, declared: Option<&str>) -> Sniffed {
    let ext = name.and_then(extension);
    let declared = declared
        .map(|d| {
            d.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let (kind, format, mime) = classify(head);
    let (kind, format) = narrow(kind, format, ext.as_deref(), &declared);
    let shown = SHOWN.iter().find(|f| f.format == format && f.kind == kind);
    let (mime, display) = match shown {
        Some(f) => (f.mime.to_string(), f.display),
        None => (mime.unwrap_or(OCTET).to_string(), "download"),
    };
    // Only displayed formats keep a type; the rest are octet-stream.
    let mime = if display == "download" {
        OCTET.to_string()
    } else {
        mime
    };
    let (width, height) = if kind == "image" {
        dimensions(head, &format).map_or((None, None), |(w, h)| (Some(w), Some(h)))
    } else {
        (None, None)
    };
    Sniffed {
        kind,
        format,
        mime,
        display,
        width,
        height,
        target: if kind == "binary" { target(head) } else { None },
    }
}

/// Is this a media kind (what Media keeps)?
pub fn is_media(kind: &str) -> bool {
    matches!(kind, "image" | "video" | "audio")
}

/// The last extension of a file name, lowercased (`.tar.gz` as one).
fn extension(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") {
        return Some("tar.gz".into());
    }
    let (_, ext) = lower.rsplit_once('.')?;
    (!ext.is_empty() && !ext.contains(['/', '\\'])).then(|| ext.to_string())
}

/// Kind, format and (when known) type, from the bytes alone.
fn classify(b: &[u8]) -> (&'static str, String, Option<&'static str>) {
    if b.is_empty() {
        return ("other", "bin".into(), None);
    }
    if let Some((kind, format)) = iso_bmff(b) {
        return (kind, format.into(), None);
    }
    if is_svg(b) {
        return ("image", "svg".into(), None);
    }
    if b.starts_with(b"OggS") {
        return (ogg_kind(b), "ogg".into(), None);
    }
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return ebml(b);
    }
    if let Some(format) = executable(b) {
        return ("binary", format.into(), None);
    }
    // `infer` takes any "BM" for a bitmap; text starting so is not one.
    if let Some(t) = infer::get(b).filter(|t| t.extension() != "bmp" || bmp(b).is_some()) {
        return from_infer(&t);
    }
    if is_text(b) {
        return ("other", "txt".into(), None);
    }
    ("other", "bin".into(), None)
}

/// Map an `infer` match to our kinds; formats keep `infer`'s extension
/// except where ours differ.
fn from_infer(t: &infer::Type) -> (&'static str, String, Option<&'static str>) {
    use infer::MatcherType as M;
    let ext = t.extension();
    let format = match ext {
        "jpg" => "jpeg",
        "tif" => "tiff",
        "mach" => "macho",
        "Z" => "z",
        e => e,
    }
    .to_string();
    let kind = match t.matcher_type() {
        M::Image => match ext {
            "dwg" | "djvu" => "document",
            _ => "image",
        },
        M::Video => "video",
        M::Audio => "audio",
        M::App => "binary",
        M::Doc | M::Book => "document",
        M::Archive => match ext {
            "pdf" | "rtf" | "ps" => "document",
            "deb" | "rpm" | "msi" | "crx" => "package",
            "sqlite" | "eot" | "nes" | "dcm" => "other",
            _ => "archive",
        },
        M::Text => match ext {
            "html" | "xml" => "document",
            _ => "other",
        },
        M::Font | M::Custom => "other",
    };
    (kind, format, Some(t.mime_type()))
}

/// A declared type or name narrows within the detected family only.
fn narrow(
    kind: &'static str,
    format: String,
    ext: Option<&str>,
    declared: &str,
) -> (&'static str, String) {
    match (kind, format.as_str()) {
        // A webm with only sound, said so by its type or name.
        ("video", "webm") if declared == "audio/webm" || ext == Some("weba") => ("audio", format),
        // An MP4 container holding only sound.
        ("video", "mp4")
            if matches!(declared, "audio/mp4" | "audio/x-m4a" | "audio/m4a")
                || matches!(ext, Some("m4a" | "m4b")) =>
        {
            ("audio", "m4a".into())
        }
        ("archive", "zip") => match ext.filter(|e| ZIP_PACKAGES.contains(e)) {
            Some(e) => ("package", e.to_string()),
            None => (kind, format),
        },
        ("archive", "gz") if matches!(ext, Some("tar.gz" | "tgz")) => ("archive", "tar.gz".into()),
        // Text named as prose is a document (Artifacts' Documents filter);
        // other text (logs, data, code) stays "other".
        ("other", "txt") if matches!(ext, Some("md" | "markdown")) => ("document", "md".into()),
        _ => (kind, format),
    }
}

// ---- ISO base media (ftyp brands) ----------------------------------------

/// Kind and format from an ISO-BMFF `ftyp` box: the major brand first, then
/// the compatible ones.
fn iso_bmff(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.len() < 16 || &b[4..8] != b"ftyp" {
        return None;
    }
    let size = be32(b, 0)? as usize;
    let end = size.clamp(16, b.len().min(4096));
    let major = &b[8..12];
    let compatible: Vec<&[u8]> = b[16..end]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| c.as_slice())
        .collect();
    let has = |brand: &[u8]| major == brand || compatible.contains(&brand);
    // Order matters: a HEIC lists `mif1` too, an AVIF often `mif1`, `miaf`.
    if major == b"qt  " {
        return Some(("video", "mov"));
    }
    if has(b"avif") || has(b"avis") {
        return Some(("image", "avif"));
    }
    if [b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis"]
        .iter()
        .any(|br| has(*br))
    {
        return Some(("image", "heic"));
    }
    if has(b"mif1") || has(b"msf1") {
        return Some(("image", "heif"));
    }
    if major == b"M4A " || major == b"M4B " || major == b"M4P " {
        return Some(("audio", "m4a"));
    }
    if major == b"M4V " || major == b"M4VH" || major == b"M4VP" {
        return Some(("video", "mp4"));
    }
    if major.starts_with(b"3g") {
        return Some(("video", "3gp"));
    }
    if major == b"crx " {
        return Some(("image", "cr3"));
    }
    const MP4: [&[u8; 4]; 12] = [
        b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"avc1", b"dash",
        b"mmp4", b"MSNV",
    ];
    if MP4.iter().any(|br| major == *br) {
        return Some(("video", "mp4"));
    }
    if compatible.contains(&b"qt  ".as_slice()) {
        return Some(("video", "mov"));
    }
    None
}

// ---- text formats ---------------------------------------------------------

/// SVG: an optional BOM, an XML prolog, comments, a doctype, then `<svg`.
fn is_svg(b: &[u8]) -> bool {
    let mut s = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    for _ in 0..64 {
        s = trim_ws(s);
        if s.starts_with(b"<?") {
            s = match find(s, b"?>") {
                Some(i) => &s[i + 2..],
                None => return false,
            };
        } else if s.starts_with(b"<!--") {
            s = match find(s, b"-->") {
                Some(i) => &s[i + 3..],
                None => return false,
            };
        } else if s.len() >= 9 && s[..9].eq_ignore_ascii_case(b"<!doctype") {
            s = match find(s, b">") {
                Some(i) => &s[i + 1..],
                None => return false,
            };
        } else {
            return s.starts_with(b"<svg")
                && s.get(4)
                    .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>' || *c == b'/');
        }
    }
    false
}

/// Plain UTF-8 text: no NUL, valid apart from a character cut at the end.
fn is_text(b: &[u8]) -> bool {
    if b.contains(&0) {
        return false;
    }
    match std::str::from_utf8(b) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && b.len() - e.valid_up_to() < 4,
    }
}

fn trim_ws(mut s: &[u8]) -> &[u8] {
    while let Some((c, rest)) = s.split_first() {
        if !c.is_ascii_whitespace() {
            break;
        }
        s = rest;
    }
    s
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// ---- containers -----------------------------------------------------------

/// Ogg: what the first stream's header packet holds.
fn ogg_kind(b: &[u8]) -> &'static str {
    let first = &b[..b.len().min(512)];
    if find(first, b"\x80theora").is_some() {
        "video"
    } else {
        // OpusHead, \x01vorbis, \x7fFLAC, Speex: sound. Anything else is
        // still served only as audio/ogg (the browser refuses what it
        // cannot play).
        "audio"
    }
}

/// EBML: webm (shown) or Matroska (downloaded), by its DocType.
fn ebml(b: &[u8]) -> (&'static str, String, Option<&'static str>) {
    let first = &b[..b.len().min(4096)];
    // The DocType element (0x4282), its size byte, then the name.
    match find(first, b"\x42\x82") {
        Some(i) if find(&first[i..(i + 16).min(first.len())], b"webm").is_some() => {
            ("video", "webm".into(), None)
        }
        _ => ("video", "mkv".into(), Some("video/x-matroska")),
    }
}

// ---- executables ----------------------------------------------------------

/// ELF, PE (exe or dll), Mach-O, wasm.
fn executable(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x7fELF") {
        return Some("elf");
    }
    if b.starts_with(b"\0asm") {
        return Some("wasm");
    }
    if let Some((_, dll)) = pe(b) {
        return Some(if dll { "dll" } else { "exe" });
    }
    if macho_cpu(b).is_some() {
        return Some("macho");
    }
    None
}

/// The PE header's machine and whether it is a DLL.
fn pe(b: &[u8]) -> Option<(u16, bool)> {
    if !b.starts_with(b"MZ") {
        return None;
    }
    let at = le32(b, 0x3C)? as usize;
    if b.get(at..at.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let machine = le16(b, at + 4)?;
    let flags = le16(b, at + 22)?;
    Some((machine, flags & 0x2000 != 0))
}

/// Mach-O's CPU type; `u32::MAX` for a universal (fat) binary. A Java class
/// shares the fat magic: its next word is a class version (45 and up).
fn macho_cpu(b: &[u8]) -> Option<u32> {
    match b.get(..4)? {
        [0xFE, 0xED, 0xFA, 0xCE] | [0xFE, 0xED, 0xFA, 0xCF] => be32(b, 4),
        [0xCE, 0xFA, 0xED, 0xFE] | [0xCF, 0xFA, 0xED, 0xFE] => le32(b, 4),
        [0xCA, 0xFE, 0xBA, 0xBE] => {
            let n = be32(b, 4)?;
            (n > 0 && n < 45).then_some(u32::MAX)
        }
        _ => None,
    }
}

/// "x86_64 Linux", from fixed offsets in the header.
pub fn target(b: &[u8]) -> Option<String> {
    if b.starts_with(b"\x7fELF") {
        let little = *b.get(5)? == 1;
        let machine = if little { le16(b, 18)? } else { be16(b, 18)? };
        let arch = match machine {
            0x03 => "x86",
            0x3E => "x86_64",
            0x28 => "arm",
            0xB7 => "aarch64",
            0xF3 => "riscv",
            0x08 => "mips",
            0x14 => "powerpc",
            0x15 => "powerpc64",
            0x16 => "s390x",
            _ => "unknown",
        };
        let os = match *b.get(7)? {
            0 | 3 => "Linux",
            2 => "NetBSD",
            9 => "FreeBSD",
            12 => "OpenBSD",
            _ => "Unix",
        };
        return Some(format!("{arch} {os}"));
    }
    if b.starts_with(b"\0asm") {
        return Some("wasm32 WebAssembly".into());
    }
    if let Some((machine, _)) = pe(b) {
        let arch = match machine {
            0x014C => "x86",
            0x8664 => "x86_64",
            0x01C0 | 0x01C4 => "arm",
            0xAA64 => "aarch64",
            _ => "unknown",
        };
        return Some(format!("{arch} Windows"));
    }
    let arch = match macho_cpu(b)? {
        u32::MAX => "universal",
        7 => "x86",
        0x0100_0007 => "x86_64",
        12 => "arm",
        0x0100_000C => "arm64",
        _ => "unknown",
    };
    Some(format!("{arch} macOS"))
}

// ---- image dimensions -----------------------------------------------------

/// Width and height from an image header (never decoded).
pub fn dimensions(b: &[u8], format: &str) -> Option<(u32, u32)> {
    let (w, h) = match format {
        "png" => png(b)?,
        "jpeg" => jpeg(b)?,
        "gif" => (u32::from(le16(b, 6)?), u32::from(le16(b, 8)?)),
        "webp" => webp(b)?,
        "bmp" => bmp(b)?,
        "avif" | "heic" | "heif" => ispe(b)?,
        "ico" => {
            let d = |v: u8| if v == 0 { 256 } else { u32::from(v) };
            (d(*b.get(6)?), d(*b.get(7)?))
        }
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

/// A bitmap's size; `None` unless its DIB header has a known length.
fn bmp(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"BM") {
        return None;
    }
    match le32(b, 14)? {
        12 => return Some((u32::from(le16(b, 18)?), u32::from(le16(b, 20)?))),
        16 | 40 | 52 | 56 | 64 | 108 | 124 => {}
        _ => return None,
    }
    let w = le32(b, 18)? as i32;
    let h = le32(b, 22)? as i32;
    Some((w.unsigned_abs(), h.unsigned_abs()))
}

/// HEIF/AVIF image spatial extents: the largest `ispe` box (a grid image
/// lists its tiles' too, which are smaller).
fn ispe(b: &[u8]) -> Option<(u32, u32)> {
    // Boxes describing the image come before its data (`mdat`), where the
    // same four bytes may occur by chance.
    let b = &b[..find(b, b"mdat").unwrap_or(b.len())];
    let mut best: Option<(u32, u32)> = None;
    let mut from = 0;
    while let Some(i) = find(&b[from..], b"ispe") {
        let at = from + i;
        if let (Some(w), Some(h)) = (be32(b, at + 8), be32(b, at + 12)) {
            let area = u64::from(w) * u64::from(h);
            if best.is_none_or(|(bw, bh)| area > u64::from(bw) * u64::from(bh)) {
                best = Some((w, h));
            }
        }
        from = at + 4;
    }
    best
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

/// A thumbnail's type, by its magic bytes: WebP, JPEG or PNG only.
pub fn thumb_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
        let size = 16 + 4 * compatible.len() as u32;
        let mut b = size.to_be_bytes().to_vec();
        b.extend(b"ftyp");
        b.extend(major);
        b.extend([0, 0, 0, 0]);
        for c in compatible {
            b.extend(*c);
        }
        b
    }

    fn ispe_box(w: u32, h: u32) -> Vec<u8> {
        let mut b = 20u32.to_be_bytes().to_vec();
        b.extend(b"ispe");
        b.extend([0, 0, 0, 0]);
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b
    }

    fn jpeg_header(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        // An APP1 segment before the frame, as cameras write.
        b.extend([0xFF, 0xE1, 0x00, 0x10]);
        b.extend([0u8; 14]);
        b.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        b
    }

    fn sn(b: &[u8]) -> Sniffed {
        sniff(b, None, None)
    }

    #[test]
    fn shown_images() {
        let p = sn(&png_header(640, 480));
        assert_eq!(
            (p.kind, p.format.as_str(), p.mime.as_str(), p.display),
            ("image", "png", "image/png", "inline")
        );
        assert_eq!((p.width, p.height), (Some(640), Some(480)));
        let j = sn(&jpeg_header(1024, 768));
        assert_eq!((j.format.as_str(), j.display), ("jpeg", "inline"));
        assert_eq!((j.width, j.height), (Some(1024), Some(768)));
        let mut gif = b"GIF89a".to_vec();
        gif.extend(320u16.to_le_bytes());
        gif.extend(200u16.to_le_bytes());
        gif.extend([0; 8]);
        let g = sn(&gif);
        assert_eq!(
            (g.format.as_str(), g.width, g.height),
            ("gif", Some(320), Some(200))
        );
        let mut bmp = b"BM".to_vec();
        bmp.extend([0; 12]);
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(100i32.to_le_bytes());
        bmp.extend((-50i32).to_le_bytes());
        bmp.extend([0; 28]);
        let m = sn(&bmp);
        assert_eq!(
            (m.format.as_str(), m.width, m.height),
            ("bmp", Some(100), Some(50))
        );
    }

    #[test]
    fn webp_variants() {
        let riff = |chunk: &[u8; 4], body: &[u8]| {
            let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
            b.extend(chunk);
            b.extend([0, 0, 0, 0]);
            b.extend(body);
            b.extend([0; 16]);
            b
        };
        let mut vp8 = vec![0, 0, 0, 0x9D, 0x01, 0x2A];
        vp8.extend(400u16.to_le_bytes());
        vp8.extend(300u16.to_le_bytes());
        let w = sn(&riff(b"VP8 ", &vp8));
        assert_eq!((w.format.as_str(), w.display), ("webp", "inline"));
        assert_eq!((w.width, w.height), (Some(400), Some(300)));
        let bits: u32 = 799 | (599 << 14);
        let mut vp8l = vec![0x2F];
        vp8l.extend(bits.to_le_bytes());
        let l = sn(&riff(b"VP8L", &vp8l));
        assert_eq!((l.width, l.height), (Some(800), Some(600)));
        let mut vp8x = vec![0, 0, 0, 0];
        vp8x.extend(&1919u32.to_le_bytes()[..3]);
        vp8x.extend(&1079u32.to_le_bytes()[..3]);
        let x = sn(&riff(b"VP8X", &vp8x));
        assert_eq!((x.width, x.height), (Some(1920), Some(1080)));
    }

    #[test]
    fn iso_brands_quicktime_heic_avif() {
        let mov = sn(&ftyp(b"qt  ", &[b"qt  "]));
        assert_eq!(
            (
                mov.kind,
                mov.format.as_str(),
                mov.mime.as_str(),
                mov.display
            ),
            ("video", "mov", "video/quicktime", "inline")
        );
        let mut heic = ftyp(b"heic", &[b"mif1", b"heic"]);
        heic.extend(ispe_box(512, 512));
        heic.extend(ispe_box(4032, 3024));
        let h = sn(&heic);
        assert_eq!(
            (h.kind, h.format.as_str(), h.mime.as_str(), h.display),
            ("image", "heic", "image/heic", "img")
        );
        assert_eq!((h.width, h.height), (Some(4032), Some(3024)));
        let f = sn(&ftyp(b"mif1", &[b"mif1", b"miaf"]));
        assert_eq!((f.format.as_str(), f.display), ("heif", "img"));
        let a = sn(&ftyp(b"avif", &[b"mif1", b"miaf", b"avif"]));
        assert_eq!((a.format.as_str(), a.display), ("avif", "inline"));
        for brand in [
            b"isom", b"iso2", b"mp41", b"mp42", b"avc1", b"dash", b"M4V ",
        ] {
            let v = sn(&ftyp(brand, &[b"isom"]));
            assert_eq!(
                (v.kind, v.format.as_str(), v.display),
                ("video", "mp4", "inline")
            );
        }
        let m4a = sn(&ftyp(b"M4A ", &[b"M4A ", b"isom"]));
        assert_eq!(
            (m4a.kind, m4a.format.as_str(), m4a.mime.as_str()),
            ("audio", "m4a", "audio/mp4")
        );
    }

    #[test]
    fn svg_with_prolog_comments_and_doctype() {
        let svgs: [&[u8]; 4] = [
            b"<svg xmlns='http://www.w3.org/2000/svg'/>",
            b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<!-- made by hand -->\n<svg>",
            b"<?xml version='1.0'?><!DOCTYPE svg PUBLIC '-//W3C//DTD SVG 1.1//EN' 'x'>\n  <svg\nwidth='1'>",
            b"  <!-- a --><!-- b --><svg/>",
        ];
        for b in svgs {
            let s = sn(b);
            assert_eq!(
                (s.kind, s.format.as_str(), s.mime.as_str(), s.display),
                ("image", "svg", "image/svg+xml", "img"),
                "{}",
                String::from_utf8_lossy(b)
            );
        }
        assert_eq!(sn(b"<svgx>").format, "txt");
        assert_eq!(sn(b"<?xml version='1.0'?><html/>").kind, "document");
    }

    #[test]
    fn html_named_png_stays_a_document() {
        let html = b"<!DOCTYPE html><html><script>window.__pwned=1</script></html>";
        let s = sniff(html, Some("cat.png"), Some("image/png"));
        assert_eq!(
            (s.kind, s.format.as_str(), s.display),
            ("document", "html", "download")
        );
        assert_eq!(s.mime, OCTET);
        assert!(!is_media(s.kind));
        // A real PNG with HTML after its header is still a PNG.
        let mut png = png_header(1, 1);
        png.extend(b"<html><script>alert(1)</script></html>");
        let p = sniff(&png, Some("page.html"), Some("text/html"));
        assert_eq!((p.format.as_str(), p.mime.as_str()), ("png", "image/png"));
    }

    #[test]
    fn ogg_and_webm_by_codec_and_narrowing() {
        let page = |codec: &[u8]| {
            let mut b = b"OggS\0\x02".to_vec();
            b.extend([0; 22]);
            b.extend(codec);
            b.extend([0; 16]);
            b
        };
        let opus = sn(&page(b"OpusHead"));
        assert_eq!((opus.kind, opus.mime.as_str()), ("audio", "audio/ogg"));
        assert_eq!(sn(&page(b"\x01vorbis")).kind, "audio");
        let theora = sn(&page(b"\x80theora"));
        assert_eq!((theora.kind, theora.mime.as_str()), ("video", "video/ogg"));

        let mut webm = vec![0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01];
        webm.extend([0x42, 0x82, 0x84]);
        webm.extend(b"webm");
        webm.extend([0; 300]);
        let v = sn(&webm);
        assert_eq!(
            (v.kind, v.format.as_str(), v.mime.as_str()),
            ("video", "webm", "video/webm")
        );
        let a = sniff(&webm, Some("voice.webm"), Some("audio/webm; codecs=opus"));
        assert_eq!(
            (a.kind, a.mime.as_str(), a.display),
            ("audio", "audio/webm", "inline")
        );
        assert_eq!(sniff(&webm, Some("x.weba"), None).kind, "audio");
        // A declared type never crosses families.
        assert_eq!(sniff(&webm, None, Some("image/png")).kind, "video");

        let mut mkv = vec![0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01];
        mkv.extend([0x42, 0x82, 0x88]);
        mkv.extend(b"matroska");
        mkv.extend([0; 300]);
        let k = sn(&mkv);
        assert_eq!(
            (k.kind, k.format.as_str(), k.display, k.mime.as_str()),
            ("video", "mkv", "download", OCTET)
        );
    }

    #[test]
    fn audio_formats() {
        let mut id3 = b"ID3\x04\0\0\0\0\0\0".to_vec();
        id3.extend([0; 64]);
        let mp3 = sn(&id3);
        assert_eq!(
            (mp3.kind, mp3.format.as_str(), mp3.display),
            ("audio", "mp3", "inline")
        );
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend([0; 32]);
        assert_eq!(sn(&wav).format, "wav");
        let mut flac = b"fLaC".to_vec();
        flac.extend([0; 40]);
        assert_eq!(sn(&flac).mime, "audio/flac");
        let mut midi = b"MThd\0\0\0\x06".to_vec();
        midi.extend([0; 16]);
        let m = sn(&midi);
        assert_eq!((m.kind, m.display), ("audio", "download"));
    }

    fn elf(machine: u16, osabi: u8) -> Vec<u8> {
        let mut b = b"\x7fELF\x02\x01\x01".to_vec();
        b.push(osabi);
        b.extend([0; 8]);
        b.extend(2u16.to_le_bytes());
        b.extend(machine.to_le_bytes());
        b.extend([0; 44]);
        b
    }

    #[test]
    fn executables_and_targets() {
        let e = sn(&elf(0x3E, 0));
        assert_eq!(
            (e.kind, e.format.as_str(), e.display),
            ("binary", "elf", "download")
        );
        assert_eq!(e.target.as_deref(), Some("x86_64 Linux"));
        assert_eq!(sn(&elf(0xB7, 9)).target.as_deref(), Some("aarch64 FreeBSD"));

        let mut pe = b"MZ".to_vec();
        pe.resize(0x3C, 0);
        pe.extend(0x40u32.to_le_bytes());
        pe.extend(b"PE\0\0");
        pe.extend(0x8664u16.to_le_bytes());
        pe.resize(0x40 + 22, 0);
        pe.extend(0x0022u16.to_le_bytes());
        pe.resize(256, 0);
        let p = sn(&pe);
        assert_eq!(
            (p.format.as_str(), p.target.as_deref()),
            ("exe", Some("x86_64 Windows"))
        );
        let mut dll = pe.clone();
        dll[0x40 + 22..0x40 + 24].copy_from_slice(&0x2022u16.to_le_bytes());
        assert_eq!(sn(&dll).format, "dll");

        let mut macho = vec![0xCF, 0xFA, 0xED, 0xFE];
        macho.extend(0x0100_000Cu32.to_le_bytes());
        macho.extend([0; 24]);
        let m = sn(&macho);
        assert_eq!(
            (m.format.as_str(), m.target.as_deref()),
            ("macho", Some("arm64 macOS"))
        );
        let mut fat = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 2];
        fat.extend([0; 40]);
        assert_eq!(sn(&fat).target.as_deref(), Some("universal macOS"));
        // A Java class shares that magic; its version says otherwise.
        let mut class = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 61];
        class.extend([0; 40]);
        assert_ne!(sn(&class).format, "macho");

        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        wasm.extend([0; 8]);
        let w = sn(&wasm);
        assert_eq!((w.kind, w.format.as_str()), ("binary", "wasm"));
    }

    fn zip_with(first_entry: &str) -> Vec<u8> {
        let mut b = b"PK\x03\x04\x14\0\0\0\0\0".to_vec();
        b.extend([0; 16]);
        b.extend((first_entry.len() as u16).to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(first_entry.as_bytes());
        b.extend([0; 64]);
        b
    }

    #[test]
    fn archives_packages_documents() {
        let zip = zip_with("README.txt");
        let z = sniff(&zip, Some("x.zip"), None);
        assert_eq!((z.kind, z.format.as_str()), ("archive", "zip"));
        let j = sniff(&zip, Some("app.jar"), None);
        assert_eq!((j.kind, j.format.as_str()), ("package", "jar"));
        assert_eq!(sniff(&zip, Some("pkg.whl"), None).format, "whl");
        // An extension outside the zip family changes nothing.
        assert_eq!(sniff(&zip, Some("x.mp4"), None).format, "zip");

        let mut gz = vec![0x1F, 0x8B, 0x08, 0];
        gz.extend([0; 32]);
        assert_eq!(sniff(&gz, Some("a.gz"), None).format, "gz");
        assert_eq!(sniff(&gz, Some("a.tar.gz"), None).format, "tar.gz");
        assert_eq!(sniff(&gz, Some("a.tgz"), None).format, "tar.gz");

        let pdf = sn(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        assert_eq!(
            (pdf.kind, pdf.format.as_str(), pdf.display),
            ("document", "pdf", "download")
        );
        assert_eq!(sn(b"hello, plain text\n").format, "txt");
        // Named as Markdown, text is a document; other text names change nothing.
        let md = sniff(b"# Notes\n\nAll good.\n", Some("report.md"), None);
        assert_eq!((md.kind, md.format.as_str()), ("document", "md"));
        assert_eq!(sniff(b"a,b\n1,2\n", Some("data.csv"), None).kind, "other");
        assert_eq!(sniff(&[0, 1, 2, 3, 0xFF], Some("x.md"), None).format, "bin");
        assert_eq!(sn(&[0, 1, 2, 3, 0xFF]).format, "bin");
        assert_eq!(sn(b"").format, "bin");
    }

    #[test]
    fn a_bomb_header_is_read_not_decoded() {
        let bomb = png_header(20_000, 20_000);
        let s = sn(&bomb);
        assert_eq!((s.width, s.height), (Some(20_000), Some(20_000)));
        let j = sn(&jpeg_header(60_000, 60_000));
        assert_eq!((j.width, j.height), (Some(60_000), Some(60_000)));
    }

    #[test]
    fn thumbnails_by_magic_bytes() {
        assert_eq!(thumb_mime(&png_header(1, 1)), Some("image/png"));
        assert_eq!(thumb_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(thumb_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(thumb_mime(b"GIF89a"), None);
        assert_eq!(thumb_mime(b"<svg/>"), None);
    }

    /// Deterministic noise for the fuzz test (xorshift64*).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    #[test]
    fn header_readers_survive_garbage() {
        let mut heic = ftyp(b"heic", &[b"mif1"]);
        heic.extend(ispe_box(10, 10));
        let mut pe = b"MZ".to_vec();
        pe.resize(0x3C, 0);
        pe.extend(0x40u32.to_le_bytes());
        pe.extend(b"PE\0\0\x64\x86");
        let seeds: Vec<Vec<u8>> = vec![
            png_header(5, 5),
            jpeg_header(5, 5),
            b"GIF89a\x05\0\x05\0".to_vec(),
            b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0\x04\0\0\x04\0\0".to_vec(),
            b"BM\0\0\0\0\0\0\0\0\0\0\0\0\x0c\0\0\0\x05\0\x05\0".to_vec(),
            heic,
            ftyp(b"qt  ", &[]),
            elf(0x3E, 3),
            pe,
            vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 1],
            b"OggS".to_vec(),
            vec![0x1A, 0x45, 0xDF, 0xA3, 0x42, 0x82],
            b"<?xml?><!-- --><svg ".to_vec(),
            vec![0, 0, 0, 0xFF, b'f', b't', b'y', b'p'],
        ];
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for round in 0..30_000 {
            let mut b = seeds[round % seeds.len()].clone();
            match rng.below(4) {
                // Cut short anywhere.
                0 => b.truncate(rng.below(b.len() + 1)),
                // Flip bytes.
                1 => {
                    for _ in 0..1 + rng.below(4) {
                        if !b.is_empty() {
                            let i = rng.below(b.len());
                            b[i] = rng.next() as u8;
                        }
                    }
                }
                // Grow with noise.
                2 => {
                    for _ in 0..rng.below(64) {
                        b.push(rng.next() as u8);
                    }
                }
                // Pure noise.
                _ => {
                    b = (0..rng.below(96)).map(|_| rng.next() as u8).collect();
                }
            }
            let s = sniff(&b, Some("x.bin"), None);
            for format in ["png", "jpeg", "gif", "webp", "bmp", "heic", "ico"] {
                let _ = dimensions(&b, format);
            }
            let _ = target(&b);
            assert!(!s.format.is_empty());
        }
    }
}

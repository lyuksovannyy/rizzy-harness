//! XML text for the parts of a package (PLAN.md T97): every string the model
//! gives goes through [`clean`] and then [`text`] or [`attr`]; nothing
//! reaches a part any other way.

/// Whether XML 1.0 allows `c`: tab, line feed, carriage return, and the
/// ranges above space without the surrogates and the two noncharacters at
/// the end of the first plane.
pub fn allowed(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// The bidirectional overrides and isolates (U+202A to U+202E, U+2066 to
/// U+2069): text that reads backwards from what it says is how a name or a
/// link is made to lie. Marks and zero-width joiners stay: scripts and emoji
/// need them.
pub fn is_override(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `s` without what XML 1.0 forbids and without bidirectional overrides.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| allowed(*c) && !is_override(*c))
        .collect()
}

/// `s` as XML character data.
pub fn text(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push('<'),
            '>' => o.push('>'),
            c => o.push(c),
        }
    }
    o
}

/// `s` as the value of a double-quoted attribute.
pub fn attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // A raw line break or tab in an attribute is read as a space.
            '\n' => o.push_str("&#10;"),
            '\r' => o.push_str("&#13;"),
            '\t' => o.push_str("&#9;"),
            c => o.push(c),
        }
    }
    o
}

/// The XML declaration every part starts with.
pub const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Content types for `[Content_Types].xml`.
pub struct ContentTypes {
    overrides: Vec<(String, String)>,
}

impl ContentTypes {
    pub fn new() -> Self {
        Self { overrides: vec![] }
    }

    /// One part with its own type (`/word/document.xml`).
    pub fn part(&mut self, name: &str, content_type: &str) {
        self.overrides
            .push((format!("/{name}"), content_type.into()));
    }

    pub fn xml(&self) -> String {
        let mut s = String::from(DECL);
        s.push_str(
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>",
        );
        for (n, t) in &self.overrides {
            s.push_str(&format!(
                "<Override PartName=\"{}\" ContentType=\"{}\"/>",
                attr(n),
                attr(t)
            ));
        }
        s.push_str("</Types>");
        s
    }
}

impl Default for ContentTypes {
    fn default() -> Self {
        Self::new()
    }
}

/// A `.rels` part: `(id, type suffix or full type, target, external)`.
pub fn rels(items: &[(String, String, String, bool)]) -> String {
    let mut s = String::from(DECL);
    s.push_str(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (id, ty, target, external) in items {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            attr(id),
            attr(ty),
            attr(target),
            if *external {
                " TargetMode=\"External\""
            } else {
                ""
            }
        ));
    }
    s.push_str("</Relationships>");
    s
}

pub const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const REL_CORE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";

/// `docProps/core.xml`: the title and the creation time.
pub fn core_props(title: Option<&str>, created_iso: &str) -> String {
    let mut s = String::from(DECL);
    s.push_str(
        "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
    );
    if let Some(t) = title {
        s.push_str(&format!("<dc:title>{}</dc:title>", text(t)));
    }
    s.push_str("<dc:creator>rizzy-harness</dc:creator><cp:lastModifiedBy>rizzy-harness</cp:lastModifiedBy>");
    s.push_str(&format!(
        "<dcterms:created xsi:type=\"dcterms:W3CDTF\">{c}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{c}</dcterms:modified>",
        c = attr(created_iso)
    ));
    s.push_str("</cp:coreProperties>");
    s
}

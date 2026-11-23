//! The XML pass of the edit tools (PLAN.md T98). A part is read as events
//! with their byte positions (`quick-xml`, no entity expanded, no DTD), and
//! a change is a list of byte ranges of the ORIGINAL part and what replaces
//! each ([`Patches`]): every byte outside a range is copied from the input
//! as it was, so what Word wrote (the order of attributes, `<a />`, a
//! namespace declaration, an `mc:Ignorable`) is never normalised away.
//!
//! Elements are matched by their local name AND by the namespace the root
//! element binds to the format's main namespace: `<w:p>` is a paragraph
//! only when `w` is bound to WordprocessingML on the root, a `<x:p>` of
//! another namespace never is. A part that rebinds that prefix further down,
//! or declares a `DOCTYPE`, or is not UTF-8, is refused.

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;

use crate::Result;

/// Elements nested deeper than this are refused (an own bound of the pass,
/// the parser has none).
pub const MAX_DEPTH: usize = 64;

pub const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const NS_S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// What was read, with where it lies in the part.
pub struct Tok<'a> {
    pub ev: Event<'a>,
    pub start: usize,
    pub end: usize,
    /// Elements open around this event (a start tag counts after itself,
    /// an end tag before itself: the root's start tag is depth 1 and its end
    /// tag is depth 1).
    pub depth: usize,
}

/// A part, as UTF-8 text, being read.
pub struct Scan<'a> {
    rd: Reader<&'a [u8]>,
    src: &'a [u8],
    depth: usize,
    events: usize,
    max_events: usize,
    /// The prefix bound to the main namespace by the root (`""`: the default
    /// namespace). `None` before the root was read.
    prefix: Option<String>,
    ns: &'static str,
}

/// The reason a part is refused, for a person.
fn syntax() -> String {
    "a part of the file is not well-formed XML".into()
}

impl<'a> Scan<'a> {
    /// `ns` is the main namespace of the part's format ([`NS_W`], [`NS_S`], ...).
    pub fn new(src: &'a [u8], ns: &'static str, max_events: usize) -> Result<Self> {
        if std::str::from_utf8(src).is_err() {
            return Err("a part of the file is not UTF-8 text; it cannot be edited".into());
        }
        let mut rd = Reader::from_reader(src);
        let c = rd.config_mut();
        c.check_end_names = true;
        c.expand_empty_elements = false;
        Ok(Self {
            rd,
            src,
            depth: 0,
            events: 0,
            max_events,
            prefix: None,
            ns,
        })
    }

    /// Read one element of a part the root of which was already read: a
    /// slice that starts at that element's start tag, the main prefix being
    /// the part's own. Positions are relative to the slice.
    pub fn within(
        src: &'a [u8],
        ns: &'static str,
        prefix: &str,
        max_events: usize,
    ) -> Result<Self> {
        let mut s = Self::new(src, ns, max_events)?;
        s.prefix = Some(prefix.to_string());
        Ok(s)
    }

    /// The main namespace's prefix as the root bound it (empty: the default).
    pub fn prefix(&self) -> &str {
        self.prefix.as_deref().unwrap_or("")
    }

    pub fn src(&self) -> &'a [u8] {
        self.src
    }

    /// The next event, or `None` at the end of the part. `DOCTYPE`, a
    /// non-UTF-8 declaration, a rebound main prefix, nesting past
    /// [`MAX_DEPTH`] and too many events are refusals.
    pub fn next(&mut self) -> Result<Option<Tok<'a>>> {
        self.events += 1;
        if self.events > self.max_events {
            return Err("a part of the file has too many elements to be edited".into());
        }
        let start = self.rd.buffer_position() as usize;
        let ev = self.rd.read_event().map_err(|_| syntax())?;
        let end = self.rd.buffer_position() as usize;
        match &ev {
            Event::Eof => return Ok(None),
            Event::DocType(_) => {
                return Err("a part of the file declares a DOCTYPE; it is not edited".into());
            }
            Event::Decl(d) => {
                if let Some(Ok(enc)) = d.encoding()
                    && !enc.eq_ignore_ascii_case("utf-8")
                {
                    return Err("a part of the file is not UTF-8 text; it cannot be edited".into());
                }
            }
            Event::Start(e) | Event::Empty(e) => {
                if matches!(ev, Event::Start(_)) {
                    self.depth += 1;
                    if self.depth > MAX_DEPTH {
                        return Err("a part of the file nests its elements too deeply".into());
                    }
                }
                if self.prefix.is_none() {
                    self.prefix = Some(self.root_prefix(e)?);
                } else {
                    self.check_rebind(e)?;
                }
            }
            Event::End(_) => {}
            _ => {}
        }
        let depth = self.depth;
        if matches!(ev, Event::End(_)) {
            self.depth = self.depth.saturating_sub(1);
        }
        Ok(Some(Tok {
            ev,
            start,
            end,
            depth,
        }))
    }

    fn root_prefix(&self, e: &BytesStart<'_>) -> Result<String> {
        let mut found = None;
        for a in e.attributes() {
            let a = a.map_err(|_| syntax())?;
            let key = a.key.as_ref();
            let val: &str = &a.value;
            if val == self.ns {
                if key == "xmlns" {
                    found = Some(String::new());
                    break;
                }
                if let Some(p) = key.strip_prefix("xmlns:") {
                    found = Some(p.to_string());
                    break;
                }
            }
        }
        let p = found.ok_or_else(|| {
            "a part of the file is not in the namespace its type needs; it cannot be edited"
                .to_string()
        })?;
        // The root itself must be in it.
        if e.name()
            .prefix()
            .map(|x| x.as_ref().to_string())
            .unwrap_or_default()
            != p
        {
            return Err("a part of the file has a root element of another namespace".into());
        }
        Ok(p)
    }

    fn check_rebind(&self, e: &BytesStart<'_>) -> Result<()> {
        let main = self.prefix.as_deref().unwrap_or("");
        for a in e.attributes() {
            let a = a.map_err(|_| syntax())?;
            let key = a.key.as_ref();
            let rebound = if main.is_empty() {
                key == "xmlns"
            } else {
                key.strip_prefix("xmlns:") == Some(main)
            };
            if rebound {
                return Err(
                    "a part of the file rebinds its main namespace below the root; it is not edited"
                        .into(),
                );
            }
        }
        Ok(())
    }

    /// The local name of `n` when it is in the main namespace.
    pub fn local<'b>(&self, n: QName<'b>) -> Option<&'b str> {
        let p = n.prefix().map(|x| x.into_inner()).unwrap_or("");
        let main = self.prefix.as_deref()?;
        (p == main).then(|| n.local_name().into_inner())
    }

    /// Read to the end of the element whose start tag was the last event
    /// (`Open` at `depth`), returning where its end tag ends.
    pub fn skip_to_end(&mut self, open_depth: usize) -> Result<usize> {
        loop {
            let Some(t) = self.next()? else {
                return Err(syntax());
            };
            if matches!(t.ev, Event::End(_)) && t.depth == open_depth {
                return Ok(t.end);
            }
        }
    }
}

/// The value of the attribute named `local` (ignoring a prefix), its
/// predefined entities resolved. `Ok(None)` when it is absent; a damaged
/// attribute list is an error.
pub fn attr(e: &BytesStart<'_>, local: &str) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a.map_err(|_| syntax())?;
        let k = a.key;
        if k.local_name().into_inner() == local
            && k.prefix().map(|p| p.into_inner()) != Some("xmlns")
        {
            return Ok(Some(unescape(&a.value)));
        }
    }
    Ok(None)
}

/// The five predefined entities and numeric references of attribute text.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        o.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';') else { break };
        let ent = &rest[1..j];
        match ent {
            "lt" => o.push('<'),
            "gt" => o.push('>'),
            "amp" => o.push('&'),
            "quot" => o.push('"'),
            "apos" => o.push('\''),
            _ => {
                let n = ent
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()));
                match n.and_then(char::from_u32) {
                    Some(c) => o.push(c),
                    None => o.push_str(&rest[..=j]),
                }
            }
        }
        rest = &rest[j + 1..];
    }
    o.push_str(rest);
    o
}

/// Text of an event's raw bytes as characters: text, CDATA, predefined
/// entities and character references. `None` for an entity the file defines
/// (it cannot be: a DOCTYPE is refused).
pub fn event_text(ev: &Event<'_>) -> Option<String> {
    match ev {
        Event::Text(t) => Some(t.xml10_content().into_owned()),
        Event::CData(t) => Some(t.xml10_content().into_owned()),
        Event::GeneralRef(r) => {
            if let Ok(Some(c)) = r.resolve_char_ref() {
                return Some(c.to_string());
            }
            match &**r {
                "lt" => Some("<".into()),
                "gt" => Some(">".into()),
                "amp" => Some("&".into()),
                "apos" => Some("'".into()),
                "quot" => Some("\"".into()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A set of changes to one part: replace `src[start..end]` with `bytes`
/// (`start == end` inserts). Ranges may not overlap.
#[derive(Default)]
pub struct Patches {
    list: Vec<(usize, usize, usize, Vec<u8>)>,
}

impl Patches {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(&mut self, start: usize, end: usize, bytes: Vec<u8>) {
        let order = self.list.len();
        self.list.push((start, end, order, bytes));
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// The part with the changes made, at most `max` bytes: counted as it is
    /// written, so a refusal comes before the bytes do.
    pub fn apply(mut self, src: &[u8], max: usize) -> Result<Vec<u8>> {
        // At one place, in the order they were added (an insert before a
        // range that starts there comes first).
        self.list
            .sort_by_key(|(s, e, order, _)| (*s, *e != *s, *order));
        let mut out: Vec<u8> = Vec::with_capacity(src.len().min(max));
        let mut at = 0usize;
        for (s, e, _, bytes) in &self.list {
            if *s < at || *e < *s || *e > src.len() {
                return Err("two edits touch the same place".into());
            }
            if out.len() + (*s - at) + bytes.len() > max {
                return Err("the edited part would be larger than it may be".into());
            }
            out.extend_from_slice(&src[at..*s]);
            out.extend_from_slice(bytes);
            at = *e;
        }
        if out.len() + (src.len() - at) > max {
            return Err("the edited part would be larger than it may be".into());
        }
        out.extend_from_slice(&src[at..]);
        Ok(out)
    }
}

//! The XML side of the document reader: one pull parser (`quick-xml`),
//! configured so that nothing in a file can make it read anything else.
//!
//! * No DTD is processed: a `<!DOCTYPE>` is skipped, and an entity the file
//!   defines (`<!ENTITY x SYSTEM "file:///etc/passwd">`, a billion laughs) is
//!   never expanded, since `quick-xml` has no entity table of its own: the
//!   reference `&x;` arrives as a separate event and is dropped here. Only
//!   the five predefined entities and numeric character references give text.
//! * No external anything: the parser reads one stream, nothing else is
//!   opened, fetched or included.
//! * Elements nest at most [`MAX_DEPTH`] deep; a file past it is refused, so
//!   a stack of the reader's own is bounded by a constant, never by the file.
//! * Names are compared by their local part, whatever the prefix the writer
//!   chose.

use std::borrow::Cow;
use std::io::{self, BufReader, Read};

use quick_xml::XmlVersion;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::reader::Reader;

/// Deepest element nesting read. Real documents stay under 30 (a table in a
/// table in a text box); a file past this is not one.
pub const MAX_DEPTH: usize = 64;

/// Why the XML could not be read.
#[derive(Debug)]
pub enum Fail {
    /// Nested deeper than [`MAX_DEPTH`].
    Deep,
    /// The stream failed (a limit of the package, a damaged entry).
    Io(io::Error),
    /// Not well-formed XML.
    Syntax,
}

/// One thing in the document.
pub enum Ev<'b> {
    Open(BytesStart<'b>),
    Close(BytesEnd<'b>),
    Text(Cow<'b, str>),
    /// A comment, a declaration, a DOCTYPE, a processing instruction, a
    /// reference to an entity the file defines: nothing to read.
    Other,
    Eof,
}

/// A pull parser over one part.
pub struct Xml<'p> {
    rd: Reader<BufReader<Box<dyn Read + 'p>>>,
    buf: Vec<u8>,
    depth: usize,
}

impl<'p> Xml<'p> {
    pub fn new(r: Box<dyn Read + 'p>) -> Self {
        let mut rd = Reader::from_reader(BufReader::with_capacity(16 * 1024, r));
        let c = rd.config_mut();
        // `<a/>` is an Open and a Close, so a reader of the events never
        // has a case of its own for it.
        c.expand_empty_elements = true;
        c.check_end_names = true;
        Self {
            rd,
            buf: Vec::new(),
            depth: 0,
        }
    }

    /// The next event. It borrows the parser: look at it before asking again.
    pub fn next(&mut self) -> Result<Ev<'_>, Fail> {
        self.buf.clear();
        let ev = match self.rd.read_event_into(&mut self.buf) {
            Ok(ev) => ev,
            Err(quick_xml::Error::Io(e)) => {
                return Err(Fail::Io(io::Error::new(
                    e.kind(),
                    "the part could not be read",
                )));
            }
            Err(_) => return Err(Fail::Syntax),
        };
        Ok(match ev {
            Event::Start(e) => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(Fail::Deep);
                }
                Ev::Open(e)
            }
            Event::End(e) => {
                self.depth = self.depth.saturating_sub(1);
                Ev::Close(e)
            }
            // Cannot happen with `expand_empty_elements`.
            Event::Empty(_) => Ev::Other,
            Event::Text(t) => Ev::Text(t.xml10_content()),
            Event::CData(t) => Ev::Text(t.xml10_content()),
            Event::GeneralRef(r) => match predefined(&r) {
                Some(c) => Ev::Text(Cow::Owned(c.to_string())),
                None => Ev::Other,
            },
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => Ev::Other,
            Event::Eof => Ev::Eof,
        })
    }
}

/// The character of `&lt;` `&gt;` `&amp;` `&apos;` `&quot;` or of a numeric
/// reference (`&#10;`, `&#x41;`); `None` for anything the file defines.
fn predefined(r: &quick_xml::events::BytesRef<'_>) -> Option<char> {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return Some(c);
    }
    match &**r {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        _ => None,
    }
}

/// The local part of an element's name.
pub fn name<'a>(e: &'a BytesStart<'_>) -> &'a str {
    e.local_name().into_inner()
}

/// The local part of a closing tag's name.
pub fn end_name<'a>(e: &'a BytesEnd<'_>) -> &'a str {
    e.local_name().into_inner()
}

/// The value of the attribute whose local name is `local`, its predefined
/// entities resolved. An undefined entity leaves the attribute out.
pub fn attr(e: &BytesStart<'_>, local: &str) -> Option<String> {
    for a in e.attributes().with_checks(false).flatten() {
        if a.key.local_name().into_inner() == local {
            return a
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(Cow::into_owned);
        }
    }
    None
}

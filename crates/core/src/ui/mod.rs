//! `ui:` blocks in assistant replies (rich_plan §4): a fence whose info
//! string is `ui:<type>` and whose body is one JSON object.
//!
//! When the agent loop records a message it calls [`extract`] on the text
//! as stored (after secret scrubbing): each block is validated and
//! normalized here, once, and attached to the `assistant_message` as a
//! [`UiBlock`]. The browser renders only these normalized specs and never
//! parses model JSON, so there is a single validator. Every model string in
//! a spec has been through [`clean_text`] and its length checked, a field or
//! question that asks for a secret is refused ([`secret_like`]), and an
//! error is harness text (a JSON path and a reason), never model text.
//!
//! The user answers a choices or form block with the next message; the
//! node checks it against its log ([`check_reply`]) and records the
//! canonical text [`answer`] writes, which carries no model text.

mod answer;
mod chart;
mod choices;
mod form;
mod progress;
mod read;
mod reply;
mod scan;
mod secret;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::UiLevel;
use crate::event::{UiBlock, UiKind};

pub use crate::text::clean_text;
pub use answer::{Values, answer};
pub use chart::{ChartKind, ChartSpec, Series, XAxis, XValues, YAxis};
pub use choices::{Choice, ChoicesSpec, MAX_OTHER};
pub use form::{Field, FieldType, FormSpec, is_date, on_grid};
pub use progress::{ProgressSpec, ProgressStatus, Step, StepStatus};
pub use read::valid_id;
pub use reply::{MAX_REPLY, NOT_OPEN, answer_text, check_reply};
pub use scan::{Fence, MAX_BLOCKS, fnv1a32, normalize, scan};
pub use secret::secret_like;

/// The largest block body, in bytes.
pub const MAX_BODY: usize = 16 * 1024;

pub const OFF_FOR_AGENT: &str = "interactive blocks are off for this agent";
pub const ONE_INPUT: &str = "only one choices or form block per message";
pub const ENDS_REPLY: &str = "a question must end the reply";

/// The blocks of an assistant message's text, as stored. `Off` finds none
/// (the fences render as code); `Display` turns choices and forms into
/// errors.
pub fn extract(text: &str, level: UiLevel) -> Vec<UiBlock> {
    if level == UiLevel::Off {
        return Vec::new();
    }
    let mut out: Vec<UiBlock> = Vec::new();
    for fence in scan(text) {
        let mut b = UiBlock {
            ordinal: fence.ordinal,
            hash: fence.hash(),
            kind: fence.kind,
            id: format!("b{}", fence.ordinal),
            spec: None,
            error: None,
        };
        match build(&fence, level, &out, &mut b.id) {
            Ok(spec) => b.spec = Some(spec),
            Err(e) => b.error = Some(e),
        }
        out.push(b);
    }
    out
}

/// For a message that also calls tools. A question ends the turn (the
/// answer comes back as the next user message), but with tool calls the
/// turn goes on, and each later step could ask another; so only a reply's
/// last message may hold a valid choices or form block.
pub fn calls_tools(blocks: &mut [UiBlock]) {
    for b in blocks.iter_mut().filter(|b| b.kind.is_input()) {
        if b.spec.take().is_some() {
            b.error = Some(ENDS_REPLY.into());
        }
    }
}

/// The normalized spec of one fence; sets `id` once the body has a valid
/// one. `earlier` are the message's blocks before it.
fn build(f: &Fence, level: UiLevel, earlier: &[UiBlock], id: &mut String) -> read::Res<Value> {
    if f.kind.is_input() && level != UiLevel::Rich {
        return Err(OFF_FOR_AGENT.into());
    }
    if f.body.len() > MAX_BODY {
        return Err("body: larger than 16 KiB".into());
    }
    // serde_json's syntax errors name a position, never the input.
    let v: Value =
        serde_json::from_str(&f.body).map_err(|e| format!("body: not valid JSON ({e})"))?;
    let mut o = read::Obj::new(&v, String::new())?;
    match o.id("id")? {
        Some(x) => *id = x,
        // A chart is shown, never referred to: it may go without.
        None if f.kind == UiKind::Chart => {}
        None => return o.err("id", "missing"),
    }
    if f.kind.is_input() && earlier.iter().any(|b| b.kind.is_input()) {
        return Err(ONE_INPUT.into());
    }
    if earlier.iter().any(|b| b.id == *id) {
        return o.err("id", format_args!("{id:?} is already used in this message"));
    }
    let spec = match f.kind {
        UiKind::Choices => to_value(choices::validate(o)?),
        UiKind::Form => to_value(form::validate(o)?),
        UiKind::Progress => to_value(progress::validate(o)?),
        UiKind::Chart => to_value(chart::validate(o)?),
    };
    Ok(spec)
}

fn to_value(spec: impl Serialize) -> Value {
    serde_json::to_value(spec).expect("specs hold only strings, finite numbers and lists")
}

/// The level a turn runs at: the agent's, but at most `Display` in a
/// delegated sub-task, whose answer goes to the parent agent and not to a
/// person who could answer a question.
pub fn effective_level(level: UiLevel, sub_task: bool) -> UiLevel {
    if sub_task {
        level.at_most(UiLevel::Display)
    } else {
        level
    }
}

/// A number in a spec, written as an integer when it is one (`2`, not
/// `2.0`), so stored specs read the way the model wrote them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Num(pub f64);

impl Num {
    pub fn to_value(self) -> Value {
        to_value(self)
    }
}

impl Serialize for Num {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Below 2^53 every whole f64 is exactly an i64.
        if self.0.fract() == 0.0 && self.0.abs() < 9.0e15 {
            s.serialize_i64(self.0 as i64)
        } else {
            s.serialize_f64(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fence(kind: &str, body: &str) -> String {
        format!("```ui:{kind}\n{body}\n```\n")
    }

    const CHOICES: &str = r#"{"id":"db","question":"Which one?","options":["a","b"]}"#;
    const FORM: &str = r#"{"id":"f","title":"T","fields":[{"id":"a","label":"A","type":"text"}]}"#;
    const PROGRESS: &str = r#"{"id":"p","title":"T","steps":[{"label":"a","status":"done"}]}"#;
    const CHART: &str =
        r#"{"kind":"bar","x":{"values":["a"]},"series":[{"name":"s","values":[1]}]}"#;

    fn summary(blocks: &[UiBlock]) -> Vec<(&str, &str, Option<&str>)> {
        blocks
            .iter()
            .map(|b| (b.kind.as_str(), b.id.as_str(), b.error.as_deref()))
            .collect()
    }

    #[test]
    fn rich_takes_every_type_and_attaches_hash_and_ordinal() {
        let text = format!(
            "Intro\n\n{}{}{}",
            fence("choices", CHOICES),
            fence("progress", PROGRESS),
            fence("chart", CHART)
        );
        let blocks = extract(&text, UiLevel::Rich);
        assert_eq!(
            summary(&blocks),
            [
                ("choices", "db", None),
                ("progress", "p", None),
                ("chart", "b2", None)
            ]
        );
        assert_eq!(blocks[1].ordinal, 1);
        assert_eq!(
            blocks[1].hash,
            format!("{:08x}", fnv1a32(PROGRESS.as_bytes()))
        );
        assert_eq!(blocks[0].spec.as_ref().unwrap()["question"], "Which one?");
        assert!(blocks.iter().all(|b| b.spec.is_some() != b.error.is_some()));
    }

    #[test]
    fn one_input_block_per_message() {
        let text = format!(
            "{}{}{}",
            fence("choices", CHOICES),
            fence("form", FORM),
            fence("progress", PROGRESS)
        );
        assert_eq!(
            summary(&extract(&text, UiLevel::Rich)),
            [
                ("choices", "db", None),
                ("form", "f", Some(ONE_INPUT)),
                ("progress", "p", None)
            ]
        );
        // An earlier input block counts even when it is an error.
        let text = format!("{}{}", fence("choices", "{"), fence("form", FORM));
        let got = extract(&text, UiLevel::Rich);
        assert_eq!(got[1].error.as_deref(), Some(ONE_INPUT));
    }

    #[test]
    fn a_message_that_calls_tools_asks_nothing() {
        let text = format!(
            "{}{}",
            fence("choices", CHOICES),
            fence("progress", PROGRESS)
        );
        let mut blocks = extract(&text, UiLevel::Rich);
        calls_tools(&mut blocks);
        assert_eq!(
            summary(&blocks),
            [("choices", "db", Some(ENDS_REPLY)), ("progress", "p", None)]
        );
        // An input block that was already an error keeps its reason.
        let mut blocks = extract(&fence("form", FORM), UiLevel::Display);
        calls_tools(&mut blocks);
        assert_eq!(blocks[0].error.as_deref(), Some(OFF_FOR_AGENT));
    }

    #[test]
    fn display_rejects_input_blocks_and_off_finds_none() {
        let text = format!("{}{}", fence("form", FORM), fence("progress", PROGRESS));
        assert_eq!(
            summary(&extract(&text, UiLevel::Display)),
            [("form", "b0", Some(OFF_FOR_AGENT)), ("progress", "p", None)]
        );
        assert!(extract(&text, UiLevel::Off).is_empty());
    }

    #[test]
    fn body_errors_and_ids() {
        let big = format!(r#"{{"id":"p","title":"{}"}}"#, "x".repeat(MAX_BODY));
        for (body, kind, id, err) in [
            (big.as_str(), "progress", "b0", "body: larger than 16 KiB"),
            (
                "{\"id\": \"p\",",
                "progress",
                "b0",
                "body: not valid JSON (EOF while parsing a value at line 1 column 11)",
            ),
            ("[1]", "progress", "b0", "body: expected an object"),
            (
                r#"{"title":"T","steps":[]}"#,
                "progress",
                "b0",
                "id: missing",
            ),
            (
                r#"{"id":"Bad id","title":"T"}"#,
                "choices",
                "b0",
                "id: not a valid id (a-z, 0-9, _ and -, at most 32, starting with a letter)",
            ),
            (
                r#"{"id":"p","title":"T","steps":[]}"#,
                "progress",
                "p",
                "steps: fewer than 1 entries",
            ),
        ] {
            let got = extract(&fence(kind, body), UiLevel::Rich);
            assert_eq!(summary(&got), [(kind, id, Some(err))], "{body}");
        }
    }

    #[test]
    fn duplicate_ids_are_an_error_on_the_later_block() {
        let text = format!(
            "{}{}",
            fence("progress", PROGRESS),
            fence("progress", PROGRESS)
        );
        let got = extract(&text, UiLevel::Rich);
        assert_eq!(
            got[1].error.as_deref(),
            Some("id: \"p\" is already used in this message")
        );
        // A chart without an id gets its ordinal's, which an explicit id may also take.
        let text = format!(
            "{}{}",
            fence("chart", CHART),
            fence("progress", &PROGRESS.replace("\"p\"", "\"b0\""))
        );
        let got = extract(&text, UiLevel::Rich);
        assert_eq!(
            summary(&got)[1],
            (
                "progress",
                "b0",
                Some("id: \"b0\" is already used in this message")
            )
        );
    }

    #[test]
    fn sub_tasks_are_capped_at_display() {
        assert_eq!(effective_level(UiLevel::Rich, false), UiLevel::Rich);
        assert_eq!(effective_level(UiLevel::Rich, true), UiLevel::Display);
        assert_eq!(effective_level(UiLevel::Display, true), UiLevel::Display);
        assert_eq!(effective_level(UiLevel::Off, true), UiLevel::Off);
    }

    #[test]
    fn numbers_keep_their_form() {
        assert_eq!(Num(2.0).to_value(), json!(2));
        assert_eq!(Num(-0.0).to_value(), json!(0));
        assert_eq!(Num(0.4).to_value(), json!(0.4));
        assert_eq!(Num(1e300).to_value(), json!(1e300));
    }
}

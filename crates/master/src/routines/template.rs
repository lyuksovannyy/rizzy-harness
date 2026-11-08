//! Prompt templates: `{{ path }}` variables, hand-rolled and rendered in a
//! single pass, so text a variable inserts (event data) is never scanned
//! again: `{{user.name}}` inside a webhook body stays literal.
//!
//! * A path is dot-separated segments, each `[A-Za-z_][A-Za-z0-9_-]{0,63}`
//!   or a decimal index, at most 8; whitespace inside the braces is fine.
//! * `\{{` writes a literal `{{`; any other backslash is literal.
//! * Errors carry the offset of the `{{` in UTF-16 code units, as a
//!   browser's textarea counts.

use jiff::Zoned;
use serde_json::{Value, json};

use super::events::EventSource;
use super::spec::OnMissing;

/// A value longer than this is cut, with `TRUNCATED` after it.
pub const MAX_VALUE: usize = 4_000;
/// The longest rendered prompt.
pub const MAX_PROMPT: usize = 32_000;
const TRUNCATED: &str = "… [truncated]";
const MAX_SEGMENTS: usize = 8;
const MAX_SEGMENT: usize = 64;

/// The fixed namespaces and their fields, with an example each.
const FIXED: &[(&str, &[(&str, &str)])] = &[
    (
        "now",
        &[
            ("date", "2026-11-08"),
            ("time", "09:30"),
            ("datetime", "2026-11-08 09:30"),
            ("weekday", "Sunday"),
            ("iso", "2026-11-08T09:30:00+02:00"),
        ],
    ),
    ("routine", &[("name", "Morning news"), ("id", "0193f1c2…")]),
    (
        "run",
        &[
            ("trigger", "schedule"),
            ("number", "12"),
            ("scheduled", "2026-11-08 09:30"),
        ],
    ),
    ("user", &[("name", "Alex")]),
    ("workspace", &[("name", "Acme")]),
    (
        "last_run",
        &[("status", "completed"), ("at", "2026-11-07 09:30")],
    ),
];

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Text(String),
    Var { path: Vec<String>, offset: usize },
}

/// A parsed template.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    /// Where the problem starts, in UTF-16 code units.
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at {})", self.message, self.offset)
    }
}

/// A rendered prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    /// Variables that had no value (`on_missing = empty`).
    pub warnings: Vec<String>,
    /// An `event.*` value was put in: the run is untrusted.
    pub untrusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// A variable had no value and the routine skips such runs.
    Missing(String),
    /// Longer than `MAX_PROMPT` even with event values cut.
    TooLong,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Missing(p) => write!(f, "no value for {{{{{p}}}}}"),
            RenderError::TooLong => write!(
                f,
                "the prompt is longer than {MAX_PROMPT} characters once filled in"
            ),
        }
    }
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

fn segment_ok(s: &str) -> bool {
    if !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let mut b = s.bytes();
    let first_ok = b
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_');
    first_ok
        && s.len() <= MAX_SEGMENT
        && b.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn parse_path(inner: &str) -> Result<Vec<String>, String> {
    let inner = inner.trim();
    if inner.is_empty() {
        return Err("empty {{ }}".into());
    }
    let segs: Vec<&str> = inner.split('.').collect();
    if segs.len() > MAX_SEGMENTS {
        return Err(format!("a variable has at most {MAX_SEGMENTS} parts"));
    }
    if let Some(bad) = segs.iter().find(|s| !segment_ok(s)) {
        return Err(format!("bad variable {inner:?} (at {bad:?})"));
    }
    Ok(segs.into_iter().map(str::to_string).collect())
}

/// Parse `src`; errors name the offending `{{`.
pub fn parse(src: &str) -> Result<Template, TemplateError> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let mut offset = 0;
    while i < src.len() {
        let rest = &src[i..];
        if rest.starts_with("\\{{") {
            text.push_str("{{");
            i += 3;
            offset += 3;
            continue;
        }
        if let Some(open) = rest.strip_prefix("{{") {
            let Some(end) = open.find("}}") else {
                return Err(TemplateError {
                    offset,
                    message: "unterminated {{".into(),
                });
            };
            let inner = &open[..end];
            let path = parse_path(inner).map_err(|message| TemplateError { offset, message })?;
            if !text.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut text)));
            }
            parts.push(Part::Var { path, offset });
            let used = 2 + end + 2;
            offset += utf16_len(&rest[..used]);
            i += used;
            continue;
        }
        let c = rest.chars().next().expect("not at the end");
        text.push(c);
        i += c.len_utf8();
        offset += c.len_utf16();
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(Template { parts })
}

impl Template {
    fn vars(&self) -> impl Iterator<Item = (&[String], usize)> {
        self.parts.iter().filter_map(|p| match p {
            Part::Var { path, offset } => Some((path.as_slice(), *offset)),
            Part::Text(_) => None,
        })
    }

    /// Check every variable against the namespaces this routine has:
    /// `event.*` only with an event `source`, and then only its variables.
    pub fn check(&self, source: Option<&dyn EventSource>) -> Result<(), TemplateError> {
        for (path, offset) in self.vars() {
            check_path(path, source).map_err(|message| TemplateError { offset, message })?;
        }
        Ok(())
    }

    /// Fill in the variables from `vars`, in one pass.
    pub fn render(&self, vars: &Vars, on_missing: OnMissing) -> Result<Rendered, RenderError> {
        // (text, came from an event) per piece, so event values are cut
        // first when the whole is too long.
        let mut pieces: Vec<(String, bool)> = Vec::new();
        let mut warnings = Vec::new();
        let mut untrusted = false;
        for part in &self.parts {
            match part {
                Part::Text(t) => pieces.push((t.clone(), false)),
                Part::Var { path, .. } => {
                    let event = path[0] == "event";
                    match vars.get(path) {
                        Some(v) => {
                            untrusted |= event;
                            pieces.push((cap(clean(&value_text(&v)), MAX_VALUE), event));
                        }
                        None => {
                            let name = path.join(".");
                            match on_missing {
                                OnMissing::Skip => return Err(RenderError::Missing(name)),
                                OnMissing::Empty => {
                                    warnings.push(format!("no value for {{{{{name}}}}}"))
                                }
                            }
                        }
                    }
                }
            }
        }
        fit(&mut pieces)?;
        Ok(Rendered {
            text: pieces.into_iter().map(|(t, _)| t).collect(),
            warnings,
            untrusted,
        })
    }
}

fn check_path(path: &[String], source: Option<&dyn EventSource>) -> Result<(), String> {
    let ns = path[0].as_str();
    if ns == "event" {
        let Some(src) = source else {
            return Err(
                "event.* is only available in routines a webhook starts, not on a schedule".into(),
            );
        };
        let full = path.join(".");
        let fixed = src.vars().iter().any(|v| v.path == full);
        let free = src
            .free_prefix()
            .is_some_and(|p| full.strip_prefix(p).is_some_and(|r| r.starts_with('.')));
        if fixed || free {
            return Ok(());
        }
        return Err(format!("unknown variable {{{{{full}}}}}"));
    }
    let Some((_, fields)) = FIXED.iter().find(|(n, _)| *n == ns) else {
        return Err(format!("unknown variable namespace {ns:?}"));
    };
    match path {
        [_, field] if fields.iter().any(|(f, _)| f == field) => Ok(()),
        _ => Err(format!("unknown variable {{{{{}}}}}", path.join("."))),
    }
}

/// How a value reads in the prompt: strings as they are, numbers and
/// booleans as JSON, null as nothing, objects and arrays as compact JSON.
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Bidi overrides and isolates: they can make text read differently from
/// what it is.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Without control characters (but newlines and tabs) and bidi controls.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|&c| (c == '\n' || c == '\t' || !c.is_control()) && !is_bidi_control(c))
        .collect()
}

fn cap(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut cut: String = s.chars().take(max).collect();
    cut.push_str(TRUNCATED);
    cut
}

/// Cut event values, longest first, until the whole fits `MAX_PROMPT`.
fn fit(pieces: &mut [(String, bool)]) -> Result<(), RenderError> {
    let total = |p: &[(String, bool)]| p.iter().map(|(t, _)| t.chars().count()).sum::<usize>();
    let marker = TRUNCATED.chars().count();
    while total(pieces) > MAX_PROMPT {
        let over = total(pieces) - MAX_PROMPT;
        let longest = pieces
            .iter_mut()
            .filter(|(t, event)| *event && t.chars().count() > marker)
            .max_by_key(|(t, _)| t.chars().count());
        // A piece down to the marker alone is not picked again.
        let Some((t, _)) = longest else {
            return Err(RenderError::TooLong);
        };
        let keep = t.chars().count().saturating_sub(over + marker);
        let mut cut: String = t.chars().take(keep).collect();
        cut.push_str(TRUNCATED);
        *t = cut;
    }
    Ok(())
}

/// The values a run's template can use.
#[derive(Debug, Clone)]
pub struct Vars {
    /// The time of the run, in the trigger's zone (Run now: the owner's).
    pub now: Zoned,
    pub routine_name: String,
    pub routine_id: String,
    /// `schedule`, `webhook` or `manual`.
    pub run_trigger: String,
    pub run_number: i64,
    /// The occurrence the run is for, in the same zone.
    pub run_scheduled: Option<Zoned>,
    pub user_name: String,
    pub workspace_name: String,
    /// The previous run's status and start.
    pub last_run: Option<(String, Zoned)>,
    /// The event that fired the run (`{body | text, received_at,
    /// content_type}`).
    pub event: Option<Value>,
}

fn local(z: &Zoned) -> String {
    z.strftime("%Y-%m-%d %H:%M").to_string()
}

impl Vars {
    /// The value at `path`; `None` when it has none (a missing path).
    pub fn get(&self, path: &[String]) -> Option<Value> {
        let field = path.get(1).map(String::as_str);
        let v = match (path[0].as_str(), field) {
            ("now", Some("date")) => json!(self.now.strftime("%Y-%m-%d").to_string()),
            ("now", Some("time")) => json!(self.now.strftime("%H:%M").to_string()),
            ("now", Some("datetime")) => json!(local(&self.now)),
            ("now", Some("weekday")) => json!(self.now.strftime("%A").to_string()),
            ("now", Some("iso")) => json!(self.now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()),
            ("routine", Some("name")) => json!(self.routine_name),
            ("routine", Some("id")) => json!(self.routine_id),
            ("run", Some("trigger")) => json!(self.run_trigger),
            ("run", Some("number")) => json!(self.run_number),
            ("run", Some("scheduled")) => json!(self.run_scheduled.as_ref().map(local)),
            ("user", Some("name")) => json!(self.user_name),
            ("workspace", Some("name")) => json!(self.workspace_name),
            // No previous run is a value (nothing), not a missing one: a
            // routine that skips on missing values still runs the first time.
            ("last_run", Some("status")) => json!(self.last_run.as_ref().map(|l| &l.0)),
            ("last_run", Some("at")) => json!(self.last_run.as_ref().map(|l| local(&l.1))),
            ("event", _) => return walk(self.event.as_ref()?, &path[1..]).cloned(),
            _ => return None,
        };
        Some(v)
    }
}

/// The value under `path` in `v` (object keys and array indexes).
fn walk<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, seg| match v {
        Value::Object(o) => o.get(seg),
        Value::Array(a) => a.get(seg.parse::<usize>().ok()?),
        _ => None,
    })
}

/// The variables a routine with `source` may use, for the picker:
/// `[{namespace, vars: [{path, example}]}]`.
pub fn namespaces(source: Option<&dyn EventSource>) -> Value {
    let mut out: Vec<Value> = FIXED
        .iter()
        .map(|(ns, fields)| {
            let vars: Vec<Value> = fields
                .iter()
                .map(|(f, ex)| json!({ "path": format!("{ns}.{f}"), "example": ex }))
                .collect();
            json!({ "namespace": ns, "vars": vars })
        })
        .collect();
    if let Some(src) = source {
        let vars: Vec<Value> = src
            .vars()
            .iter()
            .map(|v| json!({ "path": v.path, "example": v.example }))
            .collect();
        out.push(json!({ "namespace": "event", "vars": vars }));
    }
    Value::Array(out)
}

#[cfg(test)]
mod tests {
    use super::super::events::Webhook;
    use super::*;

    fn vars() -> Vars {
        let tz = jiff::tz::TimeZone::get("Europe/Riga").unwrap();
        let now = "2026-11-08T07:30:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .to_zoned(tz);
        Vars {
            now: now.clone(),
            routine_name: "Morning news".into(),
            routine_id: "r1".into(),
            run_trigger: "schedule".into(),
            run_number: 3,
            run_scheduled: Some(now),
            user_name: "Alex".into(),
            workspace_name: "Acme".into(),
            last_run: None,
            event: Some(json!({"body": {"build": {"id": 812, "tags": ["x", "y"]},
                                         "note": "say {{user.name}}"}})),
        }
    }

    fn render(src: &str) -> Result<Rendered, RenderError> {
        parse(src).unwrap().render(&vars(), OnMissing::Empty)
    }

    #[test]
    fn fills_in_every_namespace() {
        let r = render(
            "{{now.date}} {{ now.time }} {{now.weekday}} {{now.iso}} | {{routine.name}} #{{run.number}} \
             ({{run.trigger}}, {{run.scheduled}}) for {{user.name}} in {{workspace.name}}; last: [{{last_run.status}}]",
        )
        .unwrap();
        assert_eq!(
            r.text,
            "2026-11-08 09:30 Sunday 2026-11-08T09:30:00+02:00 | Morning news #3 \
             (schedule, 2026-11-08 09:30) for Alex in Acme; last: []"
        );
        assert!(
            r.warnings.is_empty(),
            "no previous run is not a missing value"
        );
        assert!(!r.untrusted);
    }

    #[test]
    fn escapes_and_literals() {
        assert_eq!(
            render(r"\{{user.name}} is {{user.name}}").unwrap().text,
            "{{user.name}} is Alex"
        );
        assert_eq!(render(r"a \ b }} c").unwrap().text, r"a \ b }} c");
    }

    #[test]
    fn errors_name_the_offset() {
        let e = parse("Hi {{user.name").unwrap_err();
        assert_eq!((e.offset, e.message.as_str()), (3, "unterminated {{"));
        // UTF-16 offsets: the emoji counts twice, as in a textarea.
        let e = parse("😀 {{ }}").unwrap_err();
        assert_eq!(e.offset, 3);
        let e = parse("{{a..b}}").unwrap_err();
        assert!(e.message.contains("bad variable"), "{e}");
        assert!(parse("{{1abc}}").is_err());
        assert!(parse("{{a.b.c.d.e.f.g.h.i}}").is_err());
        assert!(parse(&format!("{{{{a{}}}}}", "b".repeat(64))).is_err());
        let t = parse("x {{ event.body.build.0 }}").unwrap();
        assert_eq!(
            t.vars().next().unwrap(),
            (
                &[
                    "event".to_string(),
                    "body".into(),
                    "build".into(),
                    "0".into()
                ][..],
                2
            )
        );
    }

    #[test]
    fn check_knows_the_namespaces() {
        let check = |s: &str, src: Option<&dyn EventSource>| parse(s).unwrap().check(src);
        assert!(check("{{now.date}} {{last_run.at}}", None).is_ok());
        let e = check("ok {{weather.today}}", None).unwrap_err();
        assert_eq!(e.offset, 3);
        assert!(e.message.contains("namespace"));
        assert!(check("{{now.year}}", None).is_err());
        assert!(check("{{user}}", None).is_err());
        assert!(check("{{user.name.first}}", None).is_err());
        // event.* never on a schedule; with the webhook, its variables.
        let e = check("{{event.text}}", None).unwrap_err();
        assert!(e.message.contains("webhook"), "{e}");
        assert!(check("{{event.text}} {{event.body.a.0.b}}", Some(&Webhook)).is_ok());
        assert!(check("{{event.body}}", Some(&Webhook)).is_err());
        assert!(check("{{event.headers}}", Some(&Webhook)).is_err());
    }

    #[test]
    fn single_pass_and_json_values() {
        let r = render("{{event.body.note}} / {{event.body.build.id}} / {{event.body.build.tags}} / {{event.body.build.tags.1}} / {{event.body.build}}").unwrap();
        assert_eq!(
            r.text,
            r#"say {{user.name}} / 812 / ["x","y"] / y / {"id":812,"tags":["x","y"]}"#
        );
        assert!(r.untrusted);
    }

    #[test]
    fn missing_values() {
        let t = parse("a{{event.body.nope}}b").unwrap();
        let r = t.render(&vars(), OnMissing::Empty).unwrap();
        assert_eq!(r.text, "ab");
        assert_eq!(r.warnings, ["no value for {{event.body.nope}}"]);
        assert!(!r.untrusted, "nothing from the event went in");
        assert_eq!(
            t.render(&vars(), OnMissing::Skip),
            Err(RenderError::Missing("event.body.nope".into()))
        );
    }

    #[test]
    fn controls_and_bidi_are_stripped() {
        let mut v = vars();
        v.user_name = "Al\u{0}ex\u{202E}\u{2066}\n\tok\u{7}".into();
        let r = parse("{{user.name}}")
            .unwrap()
            .render(&v, OnMissing::Empty)
            .unwrap();
        assert_eq!(r.text, "Alex\n\tok");
        // The template's own text is the owner's and stays as written.
        let r = parse("a\u{202E}b")
            .unwrap()
            .render(&v, OnMissing::Empty)
            .unwrap();
        assert_eq!(r.text, "a\u{202E}b");
    }

    #[test]
    fn caps() {
        let mut v = vars();
        v.user_name = "n".repeat(MAX_VALUE + 10);
        let r = parse("{{user.name}}")
            .unwrap()
            .render(&v, OnMissing::Empty)
            .unwrap();
        assert_eq!(
            r.text.chars().count(),
            MAX_VALUE + TRUNCATED.chars().count()
        );
        assert!(r.text.ends_with(TRUNCATED));
        // Event values are cut first to fit the prompt.
        let big = "e".repeat(MAX_VALUE);
        v.event = Some(json!({ "body": { "a": big } }));
        let src = format!("{}{}", "t".repeat(MAX_PROMPT - 1000), "{{event.body.a}}");
        let r = parse(&src).unwrap().render(&v, OnMissing::Empty).unwrap();
        assert_eq!(r.text.chars().count(), MAX_PROMPT);
        assert!(r.text.ends_with(TRUNCATED));
        // Too long without any event value to cut: the run fails.
        let src = format!("{}{}", "t".repeat(MAX_PROMPT - 10), "{{user.name}}");
        assert_eq!(
            parse(&src).unwrap().render(&v, OnMissing::Empty),
            Err(RenderError::TooLong)
        );
    }

    #[test]
    fn picker_lists_only_usable_namespaces() {
        let ns = namespaces(None);
        let names: Vec<&str> = ns
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["namespace"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["now", "routine", "run", "user", "workspace", "last_run"]
        );
        let with = namespaces(Some(&Webhook));
        assert_eq!(
            with.as_array().unwrap().last().unwrap()["namespace"],
            "event"
        );
    }
}

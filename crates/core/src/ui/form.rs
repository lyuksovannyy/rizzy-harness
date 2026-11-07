//! `ui:form`: a few fields the user fills in and sends (rich_plan §4.4).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Num;
use super::choices::{self, Choice};
use super::read::{Obj, Res};
use super::secret::secret_like;
use crate::text::clean_text;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Number,
    Choice,
    Toggle,
    Date,
}

impl FieldType {
    const NAMES: [(&'static str, FieldType); 5] = [
        ("text", FieldType::Text),
        ("number", FieldType::Number),
        ("choice", FieldType::Choice),
        ("toggle", FieldType::Toggle),
        ("date", FieldType::Date),
    ];

    pub fn as_str(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, t)| *t == self)
            .expect("every type is named")
            .0
    }
}

/// One field, flat: every per-type key is optional here and `field()`
/// checks which ones a type may have, so an error can name the key (no
/// serde tagging). Defaults are written out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: FieldType,
    /// The field asks for a secret: the UI shows a warning instead of an
    /// input and no value is accepted for it. It has no other keys.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refused: bool,
    /// Every type but `toggle` (which always has a value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// `text` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// `number`: numbers; `choice` with `multi`: how many may be picked;
    /// `date`: `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Value>,
    /// `number` only; greater than 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<Num>,
    /// `number`: a number; `choice`: an option id (a list with `multi`);
    /// `toggle`: true or false; `date`: a date. `text` has none: a default
    /// would be model text sent back as the user's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// `number` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// `choice` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Choice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi: Option<bool>,
}

/// Every key some field type may have.
const FIELD_KEYS: &[&str] = &[
    "id",
    "label",
    "type",
    "required",
    "help",
    "multiline",
    "max_length",
    "placeholder",
    "min",
    "max",
    "step",
    "default",
    "unit",
    "options",
    "multi",
];

pub(super) fn validate(mut o: Obj) -> Res<FormSpec> {
    let title = o.req_text("title", 120)?;
    // A form titled or described for a key would make an innocent "Value"
    // field ask for one, so these are errors, not one refused field.
    if secret_like(&title) {
        return o.err("title", "asks for a secret");
    }
    let description = o.text("description", 400, true)?;
    if description.as_deref().is_some_and(secret_like) {
        return o.err("description", "asks for a secret");
    }
    let list = o.list("fields", 1, 12)?;
    let path = o.at("fields");
    let mut fields: Vec<Field> = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let at = format!("{path}[{i}]");
        let f = field(Obj::new(v, at.clone())?)?;
        if fields.iter().any(|x| x.id == f.id) {
            return Err(format!("{at}.id: duplicate id {:?}", f.id));
        }
        fields.push(f);
    }
    o.finish()?;
    Ok(FormSpec {
        title,
        description,
        fields,
    })
}

fn field(mut o: Obj) -> Res<Field> {
    let Some(id) = o.id("id")? else {
        return o.err("id", "missing");
    };
    let label = o.req_text("label", 120)?;
    let Some(kind) = o.word("type", &FieldType::NAMES)? else {
        return o.err("type", "missing");
    };
    let help = o.text("help", 200, false)?;
    // Refused before anything else is checked: nothing else of it is shown.
    if asks_for_secret(&o, &id, &label, help.as_deref()) {
        return Ok(Field {
            refused: true,
            ..Field::new(id, label, kind, None)
        });
    }
    let mut f = Field::new(id, label, kind, help);
    if kind != FieldType::Toggle {
        f.required = Some(o.bool("required")?.unwrap_or(false));
    }
    match kind {
        FieldType::Text => text(&mut o, &mut f)?,
        FieldType::Number => number(&mut o, &mut f)?,
        FieldType::Choice => choice(&mut o, &mut f)?,
        FieldType::Toggle => f.default = Some(Value::Bool(o.bool("default")?.unwrap_or(false))),
        FieldType::Date => date(&mut o, &mut f)?,
    }
    o.finish_with(
        FIELD_KEYS,
        &format!("not allowed for type {}", kind.as_str()),
    )?;
    Ok(f)
}

/// Does any of a field's text ask for a secret, alone or read together
/// (a label "Seed" with the help "phrase")? Option text counts too: it can
/// point at another field ("type your key in Notes").
fn asks_for_secret(o: &Obj, id: &str, label: &str, help: Option<&str>) -> bool {
    let mut texts = vec![id.to_string(), label.to_string()];
    texts.extend(help.map(str::to_string));
    let raw = |v: Option<&Value>| v.and_then(Value::as_str).map(|s| clean_text(s, false));
    texts.extend(raw(o.peek("placeholder")));
    if let Some(Value::Array(options)) = o.peek("options") {
        for v in options {
            match v {
                Value::Object(m) => {
                    texts.extend(raw(m.get("label")));
                    texts.extend(raw(m.get("description")));
                }
                v => texts.extend(raw(Some(v))),
            }
        }
    }
    texts.iter().any(|t| secret_like(t)) || secret_like(&texts.join("\n"))
}

impl Field {
    fn new(id: String, label: String, kind: FieldType, help: Option<String>) -> Self {
        Self {
            id,
            label,
            kind,
            refused: false,
            required: None,
            help,
            multiline: None,
            max_length: None,
            placeholder: None,
            min: None,
            max: None,
            step: None,
            default: None,
            unit: None,
            options: None,
            multi: None,
        }
    }
}

fn text(o: &mut Obj, f: &mut Field) -> Res<()> {
    f.multiline = Some(o.bool("multiline")?.unwrap_or(false));
    let max = o.count("max_length")?.unwrap_or(500);
    if !(1..=4000).contains(&max) {
        return o.err("max_length", "must be 1 to 4000");
    }
    f.max_length = Some(max);
    f.placeholder = o.text("placeholder", 100, false)?;
    Ok(())
}

fn number(o: &mut Obj, f: &mut Field) -> Res<()> {
    let (min, max, step) = (o.num("min")?, o.num("max")?, o.num("step")?);
    if step.is_some_and(|s| s <= 0.0) {
        return o.err("step", "must be greater than 0");
    }
    if let (Some(a), Some(b)) = (min, max)
        && a > b
    {
        return o.err("min", "larger than max");
    }
    let default = o.num("default")?;
    if let Some(d) = default {
        if min.is_some_and(|m| d < m) {
            return o.err("default", "below min");
        }
        if max.is_some_and(|m| d > m) {
            return o.err("default", "above max");
        }
        if let Some(s) = step
            && !on_grid(d, min.unwrap_or(0.0), s)
        {
            return o.err("default", "not on the step grid");
        }
    }
    f.unit = o.text("unit", 16, false)?;
    let num = |n: Option<f64>| n.map(|n| Num(n).to_value());
    (f.min, f.max, f.default, f.step) = (num(min), num(max), num(default), step.map(Num));
    Ok(())
}

/// Is `v` a whole number of `step`s away from `base`?
pub fn on_grid(v: f64, base: f64, step: f64) -> bool {
    let k = (v - base) / step;
    (k - k.round()).abs() < 1e-9
}

fn choice(o: &mut Obj, f: &mut Field) -> Res<()> {
    let options = choices::options(o, 20)?;
    let multi = o.bool("multi")?.unwrap_or(false);
    let (min, max) = choices::picks(o, multi, options.len())?;
    let offered = |id: &str| options.iter().any(|c| c.id == id);
    f.default = match (o.get("default"), multi) {
        (None, _) => None,
        (Some(Value::String(id)), false) if offered(id) => Some(Value::String(id.clone())),
        (Some(_), false) => return o.err("default", "not one of the option ids"),
        (Some(Value::Array(ids)), true) => {
            let mut picked: Vec<&str> = Vec::with_capacity(ids.len());
            for id in ids {
                match id.as_str() {
                    Some(id) if offered(id) && !picked.contains(&id) => picked.push(id),
                    _ => return o.err("default", "not a list of different option ids"),
                }
            }
            let n = picked.len() as u32;
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                return o.err("default", "outside min..max picks");
            }
            Some(picked.into())
        }
        (Some(_), true) => return o.err("default", "not a list of different option ids"),
    };
    (f.options, f.multi) = (Some(options), Some(multi));
    (f.min, f.max) = (min.map(Value::from), max.map(Value::from));
    Ok(())
}

fn date(o: &mut Obj, f: &mut Field) -> Res<()> {
    let mut get = |key: &'static str| match o.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if is_date(s) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{}: not a date (YYYY-MM-DD)", o.at(key))),
    };
    let (min, max, default) = (get("min")?, get("max")?, get("default")?);
    // `YYYY-MM-DD` strings order like the dates they name.
    if let (Some(a), Some(b)) = (&min, &max)
        && a > b
    {
        return o.err("min", "later than max");
    }
    if let Some(d) = &default {
        if min.as_ref().is_some_and(|m| d < m) {
            return o.err("default", "before min");
        }
        if max.as_ref().is_some_and(|m| d > m) {
            return o.err("default", "after max");
        }
    }
    (f.min, f.max, f.default) = (
        min.map(Value::String),
        max.map(Value::String),
        default.map(Value::String),
    );
    Ok(())
}

/// A real calendar date written `YYYY-MM-DD` (year 0001 to 9999).
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        b.get(r.clone())?
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| s[r].parse().ok())?
    };
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Some(y), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1 && (1..=days).contains(&d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(v: Value) -> Res<Value> {
        let o = Obj::new(&v, String::new())?;
        validate(o).map(|s| serde_json::to_value(s).unwrap())
    }

    fn one(f: Value) -> Res<Value> {
        run(json!({"title": "T", "fields": [f]})).map(|v| v["fields"][0].clone())
    }

    #[test]
    fn normalizes_every_type_with_explicit_defaults() {
        let got = run(json!({
            "title": "Trip details", "description": "Optional,\nmay span lines",
            "fields": [
                {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12, "step": 1, "default": 2, "unit": "people"},
                {"id": "dest", "label": "Destination", "type": "choice", "required": true, "options": [{"id": "lis", "label": "Lisbon"}, {"id": "rom", "label": "Rome"}]},
                {"id": "extras", "label": "Extras", "type": "choice", "multi": true, "options": ["Breakfast", "Parking"], "max": 2},
                {"id": "start", "label": "Start date", "type": "date", "min": "2026-11-08"},
                {"id": "flex", "label": "Dates are flexible", "type": "toggle", "default": true},
                {"id": "notes", "label": "Anything else?", "type": "text", "multiline": true, "max_length": 1000, "placeholder": "Optional"}
            ]
        }))
        .unwrap();
        assert_eq!(got["description"], "Optional,\nmay span lines");
        assert_eq!(
            got["fields"],
            json!([
                {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12, "step": 1, "default": 2, "unit": "people"},
                {"id": "dest", "label": "Destination", "type": "choice", "required": true, "options": [{"id": "lis", "label": "Lisbon"}, {"id": "rom", "label": "Rome"}], "multi": false},
                {"id": "extras", "label": "Extras", "type": "choice", "required": false, "min": 1, "max": 2, "options": [{"id": "o1", "label": "Breakfast"}, {"id": "o2", "label": "Parking"}], "multi": true},
                {"id": "start", "label": "Start date", "type": "date", "required": false, "min": "2026-11-08"},
                {"id": "flex", "label": "Dates are flexible", "type": "toggle", "default": true},
                {"id": "notes", "label": "Anything else?", "type": "text", "required": false, "multiline": true, "max_length": 1000, "placeholder": "Optional"}
            ])
        );
        assert_eq!(
            one(json!({"id": "n", "label": "N", "type": "text"})),
            Ok(
                json!({"id": "n", "label": "N", "type": "text", "required": false, "multiline": false, "max_length": 500})
            )
        );
        assert_eq!(
            one(json!({"id": "t", "label": "T", "type": "toggle"})),
            Ok(json!({"id": "t", "label": "T", "type": "toggle", "default": false}))
        );
        assert_eq!(
            one(json!({"id": "x", "label": "X", "type": "number", "step": 0.5, "default": 1.5})),
            Ok(
                json!({"id": "x", "label": "X", "type": "number", "required": false, "step": 0.5, "default": 1.5})
            )
        );
    }

    #[test]
    fn secret_fields_are_refused_whatever_else_they_say() {
        for f in [
            json!({"id": "key", "label": "API key", "type": "text"}),
            json!({"id": "pw", "label": "Password", "type": "text", "max_length": 99999}),
            json!({"id": "x", "label": "Value", "type": "text", "help": "Your GitHub token"}),
            json!({"id": "x", "label": "Value", "type": "text", "placeholder": "sk-... (API key)"}),
            json!({"id": "x", "label": "Value", "type": "number", "placeholder": "PIN"}),
            json!({"id": "api_key", "label": "Value", "type": "text"}),
        ] {
            let got = one(f.clone()).unwrap();
            assert_eq!(got["refused"], true, "{f}");
            assert_eq!(
                got.as_object().unwrap().len(),
                4,
                "only id, label, type, refused: {got}"
            );
        }
        // Text that asks only once its parts are read together, and option
        // text that points at another field.
        for f in [
            json!({"id": "x", "label": "API", "type": "text", "help": "key from your dashboard"}),
            json!({"id": "github", "label": "Value", "type": "text", "help": "token from settings"}),
            json!({"id": "x", "label": "Provider", "type": "choice", "options": ["GitHub", {"label": "Other", "description": "paste your API key in Notes"}]}),
            json!({"id": "x", "label": "Provider", "type": "choice", "options": ["Pass\u{ad}word", "b"]}),
            json!({"id": "x", "label": "Seed", "type": "text", "help": "Phrase (12 words)"}),
            json!({"id": "x", "label": "Social", "type": "text", "placeholder": "security number"}),
        ] {
            let got = one(f.clone()).unwrap();
            assert_eq!(got["refused"], true, "{f}");
        }
        // The id and label still have to be valid.
        assert_eq!(
            one(json!({"id": "A", "label": "API key", "type": "text"})),
            Err("fields[0].id: not a valid id (a-z, 0-9, _ and -, at most 32, starting with a letter)".into())
        );
    }

    #[test]
    fn rejects_bad_fields() {
        let f = |extra: Value| {
            let mut v = json!({"id": "f", "label": "F"});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            one(v)
        };
        for (extra, err) in [
            (
                json!({"type": "text", "default": "hi"}),
                "fields[0].default: not allowed for type text",
            ),
            (
                json!({"type": "number", "options": ["a", "b"]}),
                "fields[0].options: not allowed for type number",
            ),
            (
                json!({"type": "toggle", "required": true}),
                "fields[0].required: not allowed for type toggle",
            ),
            (
                json!({"type": "date", "step": 1}),
                "fields[0].step: not allowed for type date",
            ),
            (
                json!({"type": "text", "colour": 1}),
                "fields[0].colour: unknown key",
            ),
            (
                json!({"type": "password"}),
                "fields[0].type: must be text, number, choice, toggle or date",
            ),
            (json!({}), "fields[0].type: missing"),
            (
                json!({"type": "text", "label": "x".repeat(121)}),
                "fields[0].label: longer than 120 characters",
            ),
            (
                json!({"type": "text", "max_length": 0}),
                "fields[0].max_length: must be 1 to 4000",
            ),
            (
                json!({"type": "text", "max_length": 4001}),
                "fields[0].max_length: must be 1 to 4000",
            ),
            (
                json!({"type": "text", "placeholder": "x".repeat(101)}),
                "fields[0].placeholder: longer than 100 characters",
            ),
            (
                json!({"type": "number", "min": 5, "max": 1}),
                "fields[0].min: larger than max",
            ),
            (
                json!({"type": "number", "step": 0}),
                "fields[0].step: must be greater than 0",
            ),
            (
                json!({"type": "number", "min": 1, "default": 0}),
                "fields[0].default: below min",
            ),
            (
                json!({"type": "number", "max": 1, "default": 2}),
                "fields[0].default: above max",
            ),
            (
                json!({"type": "number", "min": 1, "step": 2, "default": 2}),
                "fields[0].default: not on the step grid",
            ),
            (
                json!({"type": "number", "unit": "x".repeat(17)}),
                "fields[0].unit: longer than 16 characters",
            ),
            (
                json!({"type": "number", "default": "2"}),
                "fields[0].default: expected a number",
            ),
            (
                json!({"type": "choice", "options": ["a", "b"], "default": "c"}),
                "fields[0].default: not one of the option ids",
            ),
            (
                json!({"type": "choice", "options": ["a", "b"], "default": ["o1"]}),
                "fields[0].default: not one of the option ids",
            ),
            (
                json!({"type": "choice", "multi": true, "options": ["a", "b"], "default": ["o1", "o1"]}),
                "fields[0].default: not a list of different option ids",
            ),
            (
                json!({"type": "choice", "multi": true, "options": ["a", "b", "c"], "max": 1, "default": ["o1", "o2"]}),
                "fields[0].default: outside min..max picks",
            ),
            (
                json!({"type": "choice", "options": ["a", "b"], "min": 1}),
                "fields[0].min: allowed only with multi",
            ),
            (
                json!({"type": "choice", "options": (0..21).map(|i| i.to_string()).collect::<Vec<_>>()}),
                "fields[0].options: more than 20 entries",
            ),
            (
                json!({"type": "date", "min": "2026-02-30"}),
                "fields[0].min: not a date (YYYY-MM-DD)",
            ),
            (
                json!({"type": "date", "min": "2026-03-01", "max": "2026-02-01"}),
                "fields[0].min: later than max",
            ),
            (
                json!({"type": "date", "min": "2026-03-01", "default": "2026-02-28"}),
                "fields[0].default: before min",
            ),
            (
                json!({"type": "date", "max": "2026-03-01", "default": "2026-03-02"}),
                "fields[0].default: after max",
            ),
            (
                json!({"type": "text", "help": "x".repeat(201)}),
                "fields[0].help: longer than 200 characters",
            ),
        ] {
            assert_eq!(f(extra.clone()), Err(err.to_string()), "{extra}");
        }
    }

    #[test]
    fn form_level_rules() {
        let field = json!({"id": "a", "label": "A", "type": "text"});
        for (spec, err) in [
            (json!({"fields": [field]}), "title: missing"),
            (
                json!({"title": "x".repeat(121), "fields": [field]}),
                "title: longer than 120 characters",
            ),
            (
                json!({"title": "T", "fields": []}),
                "fields: fewer than 1 entries",
            ),
            (
                json!({"title": "T", "fields": vec![field.clone(); 13]}),
                "fields: more than 12 entries",
            ),
            (
                json!({"title": "T", "fields": [field, field]}),
                "fields[1].id: duplicate id \"a\"",
            ),
            (
                json!({"title": "T", "fields": [field], "submit": "Go"}),
                "submit: unknown key",
            ),
            (
                json!({"title": "T", "description": "x".repeat(401), "fields": [field]}),
                "description: longer than 400 characters",
            ),
            (
                json!({"title": "Your API key", "fields": [field]}),
                "title: asks for a secret",
            ),
            (
                json!({"title": "Setup", "description": "Paste your API key below so I can finish the setup.", "fields": [field]}),
                "description: asks for a secret",
            ),
            (
                json!({"title": "T", "fields": [{"label": "A", "type": "text"}]}),
                "fields[0].id: missing",
            ),
            (
                json!({"title": "T", "fields": ["a"]}),
                "fields[0]: expected an object",
            ),
        ] {
            assert_eq!(run(spec.clone()), Err(err.to_string()), "{spec}");
        }
    }

    #[test]
    fn dates_and_grid() {
        for ok in ["2024-02-29", "2000-02-29", "2026-12-31", "0001-01-01"] {
            assert!(is_date(ok), "{ok}");
        }
        for bad in [
            "2023-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-04-31",
            "0000-01-01",
            "2026-1-01",
            "2026-01-01T",
            "+026-01-01",
            "２０２６-01-01",
        ] {
            assert!(!is_date(bad), "{bad}");
        }
        assert!(on_grid(0.3, 0.1, 0.1) && on_grid(12.0, 1.0, 1.0) && !on_grid(2.5, 0.0, 1.0));
    }
}

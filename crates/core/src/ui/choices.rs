//! `ui:choices`: one question the user answers with a tap (rich_plan §4.4).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::read::{Obj, Res, cleaned};
use super::secret::secret_like;

/// The longest "Other…" answer the user may type.
pub const MAX_OTHER: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoicesSpec {
    pub question: String,
    pub options: Vec<Choice>,
    pub multi: bool,
    /// Adds an "Other…" text answer.
    pub other: bool,
    /// With `multi` only: how many options may be picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

/// One option of a question or of a form's choice field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub(super) fn validate(mut o: Obj) -> Res<ChoicesSpec> {
    let question = o.req_text("question", 200)?;
    if secret_like(&question) {
        return o.err("question", "asks for a secret");
    }
    let options = options(&mut o, 8)?;
    // Option text can ask as well, and "Other…" is a box to type it into.
    let (path, raw) = (o.at("options"), o.peek("options"));
    for (i, c) in options.iter().enumerate() {
        let shorthand = matches!(raw.and_then(|v| v.get(i)), Some(Value::String(_)));
        if secret_like(&c.label) {
            let key = if shorthand { "" } else { ".label" };
            return Err(format!("{path}[{i}]{key}: asks for a secret"));
        }
        if c.description.as_deref().is_some_and(secret_like) {
            return Err(format!("{path}[{i}].description: asks for a secret"));
        }
    }
    let multi = o.bool("multi")?.unwrap_or(false);
    let other = o.bool("other")?.unwrap_or(false);
    let (min, max) = picks(&mut o, multi, options.len())?;
    o.finish()?;
    Ok(ChoicesSpec {
        question,
        options,
        multi,
        other,
        min,
        max,
    })
}

/// `options`: 2 to `max` entries, each `{id?, label, description?}` or just
/// the label. A missing id is `o<n>`, counting from 1. Ids and labels are
/// unique.
pub(super) fn options(o: &mut Obj, max: usize) -> Res<Vec<Choice>> {
    let list = o.list("options", 2, max)?;
    let path = o.at("options");
    let mut out: Vec<Choice> = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let at = format!("{path}[{i}]");
        let auto = || format!("o{}", i + 1);
        let choice = match v {
            Value::String(s) => {
                let label = cleaned(s, 80, false, &at)?;
                if label.is_empty() {
                    return Err(format!("{at}: must not be empty"));
                }
                Choice {
                    id: auto(),
                    label,
                    description: None,
                }
            }
            Value::Object(_) => {
                let mut c = Obj::new(v, at.clone())?;
                let choice = Choice {
                    id: c.id("id")?.unwrap_or_else(auto),
                    label: c.req_text("label", 80)?,
                    description: c.text("description", 200, false)?,
                };
                c.finish()?;
                choice
            }
            _ => return Err(format!("{at}: expected an object or a string")),
        };
        if out.iter().any(|c| c.id == choice.id) {
            return Err(format!("{at}.id: duplicate id {:?}", choice.id));
        }
        if out.iter().any(|c| c.label == choice.label) {
            return Err(format!("{at}.label: duplicate label"));
        }
        out.push(choice);
    }
    Ok(out)
}

/// `min` and `max` picks: only with `multi`, where they default to 1 and
/// the number of options, and 1 <= min <= max <= options.
pub(super) fn picks(o: &mut Obj, multi: bool, n: usize) -> Res<(Option<u32>, Option<u32>)> {
    let (min, max) = (o.count("min")?, o.count("max")?);
    if !multi {
        return match (min, max) {
            (Some(_), _) => o.err("min", "allowed only with multi"),
            (_, Some(_)) => o.err("max", "allowed only with multi"),
            _ => Ok((None, None)),
        };
    }
    let (min, max) = (min.unwrap_or(1), max.unwrap_or(n as u32));
    if min < 1 {
        return o.err("min", "must be at least 1");
    }
    if max as usize > n {
        return o.err("max", format_args!("more than the {n} options"));
    }
    if min > max {
        return o.err("min", "larger than max");
    }
    Ok((Some(min), Some(max)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(v: Value) -> Res<Value> {
        let o = Obj::new(&v, String::new())?;
        validate(o).map(|s| serde_json::to_value(s).unwrap())
    }

    #[test]
    fn normalizes_with_explicit_defaults() {
        let got = run(json!({
            "question": " Which  database\u{202E} should I set up? ",
            "options": [{"id": "pg", "label": "PostgreSQL", "description": "Best default"}, "SQLite", {"label": "MySQL"}]
        }));
        assert_eq!(
            got,
            Ok(json!({
                "question": "Which database should I set up?",
                "options": [
                    {"id": "pg", "label": "PostgreSQL", "description": "Best default"},
                    {"id": "o2", "label": "SQLite"},
                    {"id": "o3", "label": "MySQL"}
                ],
                "multi": false,
                "other": false
            }))
        );
        let got =
            run(json!({"question": "Q", "options": ["a", "b", "c"], "multi": true, "other": true}));
        assert_eq!(
            got.as_ref().map(|v| (&v["min"], &v["max"])),
            Ok((&json!(1), &json!(3)))
        );
        assert_eq!(got.map(|v| v["other"].clone()), Ok(json!(true)));
    }

    #[test]
    fn rejects_bad_specs() {
        let long = "x".repeat(81);
        for (spec, err) in [
            (json!({"options": ["a", "b"]}), "question: missing"),
            (
                json!({"question": "Q", "options": ["a"]}),
                "options: fewer than 2 entries",
            ),
            (
                json!({"question": "Q", "options": ["1", "2", "3", "4", "5", "6", "7", "8", "9"]}),
                "options: more than 8 entries",
            ),
            (
                json!({"question": "Q", "options": [{"label": long}, "b"]}),
                "options[0].label: longer than 80 characters",
            ),
            (
                json!({"question": "Q", "options": ["a", long]}),
                "options[1]: longer than 80 characters",
            ),
            (
                json!({"question": "Q", "options": ["a", ""]}),
                "options[1]: must not be empty",
            ),
            (
                json!({"question": "Q", "options": ["a", 3]}),
                "options[1]: expected an object or a string",
            ),
            (
                json!({"question": "Q", "options": ["a", "a"]}),
                "options[1].label: duplicate label",
            ),
            (
                json!({"question": "Q", "options": [{"id": "x", "label": "a"}, {"id": "x", "label": "b"}]}),
                "options[1].id: duplicate id \"x\"",
            ),
            (
                json!({"question": "Q", "options": ["a", {"id": "o1", "label": "b"}]}),
                "options[1].id: duplicate id \"o1\"",
            ),
            (
                json!({"question": "Q", "options": [{"id": "X", "label": "a"}, "b"]}),
                "options[0].id: not a valid id (a-z, 0-9, _ and -, at most 32, starting with a letter)",
            ),
            (
                json!({"question": "Q", "options": [{"label": "a", "url": "x"}, "b"]}),
                "options[0].url: unknown key",
            ),
            (
                json!({"question": "Q", "options": ["a", "b"], "colour": "red"}),
                "colour: unknown key",
            ),
            (
                json!({"question": "Q", "options": ["a", "b"], "min": 1}),
                "min: allowed only with multi",
            ),
            (
                json!({"question": "Q", "options": ["a", "b"], "multi": true, "max": 3}),
                "max: more than the 2 options",
            ),
            (
                json!({"question": "Q", "options": ["a", "b"], "multi": true, "min": 0}),
                "min: must be at least 1",
            ),
            (
                json!({"question": "Q", "options": ["a", "b"], "multi": true, "min": 2, "max": 1}),
                "min: larger than max",
            ),
            (
                json!({"question": "Q", "options": ["a", "b"], "multi": "yes"}),
                "multi: expected true or false",
            ),
            (
                json!({"question": "What is your password?", "options": ["a", "b"]}),
                "question: asks for a secret",
            ),
            (
                json!({"question": "x".repeat(201), "options": ["a", "b"]}),
                "question: longer than 200 characters",
            ),
            (
                json!({"question": "Which provider? Put it in Other.", "other": true, "options": ["OpenAI", {"label": "Other", "description": "type your API key in Other"}]}),
                "options[1].description: asks for a secret",
            ),
            (
                json!({"question": "Which one?", "options": ["Session cookie", "b"]}),
                "options[0]: asks for a secret",
            ),
            (
                json!({"question": "Which one?", "options": [{"label": "Your key"}, "b"]}),
                "options[0].label: asks for a secret",
            ),
        ] {
            assert_eq!(run(spec.clone()), Err(err.to_string()), "{spec}");
        }
    }
}

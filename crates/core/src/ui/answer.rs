//! The user's answer to a `ui:choices` or `ui:form` block: every rule its
//! values must pass, and the canonical text the model gets for it
//! (rich_plan §7.4-7.5).
//!
//! The canonical text holds only what the harness can vouch for: ids (which
//! `valid_id` limits to `a-z0-9_-`, so no URL or prose), numbers, dates and
//! yes/no written by the harness, and the user's own typing, JSON-quoted.
//! Labels and questions are model text and never reach a user-role message.
//! The browser runs the same rules in `web/src/ui/form.ts` for its inline
//! errors; `ui.json` holds both to the same answers.

use serde_json::{Map, Value};

use super::{ChoicesSpec, Field, FieldType, FormSpec, MAX_OTHER, is_date, on_grid, valid_id};
use crate::event::{UiBlock, UiKind};

pub type Values = Map<String, Value>;

/// The canonical answer to `block`, or why `values` do not answer it.
pub fn answer(block: &UiBlock, values: &Values) -> Result<String, String> {
    if !block.kind.is_input() {
        return Err("that block is not a question or form".into());
    }
    let Some(spec) = block.spec.clone() else {
        return Err("that block was not shown".into());
    };
    let shown = || "that block was not shown".to_string();
    match block.kind {
        UiKind::Choices => {
            let spec: ChoicesSpec = serde_json::from_value(spec).map_err(|_| shown())?;
            choices(&block.id, &spec, values)
        }
        _ => {
            let spec: FormSpec = serde_json::from_value(spec).map_err(|_| shown())?;
            form(&block.id, &spec, values)
        }
    }
}

/// `Choice answer · db: pg, other "MariaDB 11"`.
fn choices(id: &str, spec: &ChoicesSpec, values: &Values) -> Result<String, String> {
    unknown_keys(values, |k| k == "selected" || k == "other", "unknown key")?;
    let picked = match values.get("selected") {
        None => return Err("selected: missing".into()),
        Some(v) => option_ids(v, &spec.options, "selected")?,
    };
    let other = match values.get("other") {
        None => None,
        Some(_) if !spec.other => return Err("other: not allowed for this question".into()),
        Some(v) => Some(typed_text(v, "other", MAX_OTHER, false)?),
    };
    let n = picked.len() + usize::from(other.is_some());
    if spec.multi {
        // "Other" is one more answer, toward the minimum and the maximum
        // alike: the card says "Pick 2", and the model asked for at most 2.
        let (min, max) = (spec.min.unwrap_or(1), spec.max.unwrap_or(u32::MAX));
        if n as u32 > max {
            return Err(format!("selected: pick at most {max}"));
        }
        if (n as u32) < min {
            return Err(format!("selected: pick at least {min}"));
        }
    } else if n == 0 {
        return Err("selected: pick one".into());
    } else if n > 1 {
        return Err("selected: pick only one".into());
    }
    let mut parts: Vec<String> = spec
        .options
        .iter()
        .filter(|o| picked.contains(&o.id.as_str()))
        .map(|o| o.id.clone())
        .collect();
    parts.extend(other.map(|t| format!("other {}", quote(t))));
    Ok(format!("Choice answer · {id}: {}", parts.join(", ")))
}

/// `Form answer · trip` and a line per field, in the form's order.
fn form(id: &str, spec: &FormSpec, values: &Values) -> Result<String, String> {
    let field = |k: &str| spec.fields.iter().find(|f| f.id == k);
    unknown_keys(values, |k| field(k).is_some(), "not a field of this form")?;
    let mut keys: Vec<&String> = values.keys().collect();
    keys.sort();
    if let Some(k) = keys
        .into_iter()
        .find(|k| field(k).is_some_and(|f| f.refused))
    {
        return Err(format!("{k}: asks for a secret, so it takes no value"));
    }
    let mut lines = vec![format!("Form answer · {id}")];
    let (mut missing, mut refused) = (Vec::new(), Vec::new());
    for f in &spec.fields {
        if f.refused {
            refused.push(f.id.as_str());
            continue;
        }
        match values.get(&f.id) {
            Some(v) => lines.push(format!("- {}: {}", f.id, value(f, v)?)),
            None if f.required == Some(true) => return Err(format!("{}: required", f.id)),
            None => missing.push(f.id.as_str()),
        }
    }
    if !missing.is_empty() {
        lines.push(format!("- not answered: {}", missing.join(", ")));
    }
    // Tells the model why a field it asked for has no answer, so it does
    // not ask again for the same secret in other words.
    if !refused.is_empty() {
        lines.push(format!(
            "- not shown (asks for a secret): {}",
            refused.join(", ")
        ));
    }
    Ok(lines.join("\n"))
}

/// One field's value as the canonical text writes it.
fn value(f: &Field, v: &Value) -> Result<String, String> {
    let at = f.id.as_str();
    let err = |reason: &str| Err(format!("{at}: {reason}"));
    match f.kind {
        FieldType::Text => {
            let max = f.max_length.unwrap_or(500) as usize;
            Ok(quote(typed_text(v, at, max, f.multiline == Some(true))?))
        }
        FieldType::Number => {
            let Some(n) = v.as_f64() else {
                return err("expected a number");
            };
            let (min, max) = (f.min.as_ref(), f.max.as_ref());
            if let Some(m) = min.and_then(Value::as_f64).filter(|m| n < *m) {
                return err(&format!("must be at least {}", num(m)));
            }
            if let Some(m) = max.and_then(Value::as_f64).filter(|m| n > *m) {
                return err(&format!("must be at most {}", num(m)));
            }
            if let Some(step) = f.step {
                let base = min.and_then(Value::as_f64).unwrap_or(0.0);
                if !on_grid(n, base, step.0) {
                    return err(&format!(
                        "must be in steps of {} from {}",
                        num(step.0),
                        num(base)
                    ));
                }
            }
            Ok(num(n))
        }
        FieldType::Choice => {
            let options = f.options.as_deref().unwrap_or_default();
            if f.multi != Some(true) {
                return match v.as_str() {
                    Some(id) if options.iter().any(|o| o.id == id) => Ok(id.to_string()),
                    Some(_) => err("not one of the options"),
                    None => err("expected an option id"),
                };
            }
            let picked = option_ids(v, options, at)?;
            let count = |m: &Option<Value>| m.as_ref().and_then(Value::as_u64);
            if let Some(m) = count(&f.min).filter(|m| (picked.len() as u64) < *m) {
                return err(&format!("pick at least {m}"));
            }
            if let Some(m) = count(&f.max).filter(|m| (picked.len() as u64) > *m) {
                return err(&format!("pick at most {m}"));
            }
            let ids: Vec<&str> = options
                .iter()
                .map(|o| o.id.as_str())
                .filter(|id| picked.contains(id))
                .collect();
            Ok(ids.join(", "))
        }
        FieldType::Toggle => match v.as_bool() {
            Some(b) => Ok(if b { "yes" } else { "no" }.into()),
            None => err("expected true or false"),
        },
        FieldType::Date => {
            let Some(d) = v.as_str().filter(|d| is_date(d)) else {
                return err("not a date (YYYY-MM-DD)");
            };
            // `YYYY-MM-DD` strings order like the dates they name.
            if let Some(m) = f.min.as_ref().and_then(Value::as_str).filter(|m| d < *m) {
                return err(&format!("must be {m} or later"));
            }
            if let Some(m) = f.max.as_ref().and_then(Value::as_str).filter(|m| d > *m) {
                return err(&format!("must be {m} or earlier"));
            }
            Ok(d.to_string())
        }
    }
}

/// Fail on the first key (in sorted order, so the message does not depend
/// on map order) that `known` rejects. A key is the client's text: it is
/// named only when it has the shape of an id.
fn unknown_keys(values: &Values, known: impl Fn(&str) -> bool, why: &str) -> Result<(), String> {
    let mut keys: Vec<&String> = values.keys().filter(|k| !known(k)).collect();
    keys.sort();
    match keys.first() {
        None => Ok(()),
        Some(k) if valid_id(k) => Err(format!("{k}: {why}")),
        Some(_) => Err(format!("values: {why}")),
    }
}

/// A list of different ids, each one of `options`.
fn option_ids<'a>(
    v: &'a Value,
    options: &[super::Choice],
    at: &str,
) -> Result<Vec<&'a str>, String> {
    let Some(list) = v.as_array() else {
        return Err(format!("{at}: expected a list of option ids"));
    };
    let mut out: Vec<&str> = Vec::with_capacity(list.len());
    for id in list {
        let Some(id) = id.as_str() else {
            return Err(format!("{at}: expected a list of option ids"));
        };
        if !options.iter().any(|o| o.id == id) {
            return Err(format!("{at}: not one of the options"));
        }
        if out.contains(&id) {
            return Err(format!("{at}: an option is picked twice"));
        }
        out.push(id);
    }
    Ok(out)
}

/// The user's typing: a string that is not blank, at most `max`
/// characters, and one line unless `multiline`. Returned trimmed.
fn typed_text<'a>(v: &'a Value, at: &str, max: usize, multiline: bool) -> Result<&'a str, String> {
    let Some(s) = v.as_str() else {
        return Err(format!("{at}: expected text"));
    };
    if s.trim().is_empty() {
        return Err(format!("{at}: must not be empty"));
    }
    if !multiline && s.contains(['\n', '\r']) {
        return Err(format!("{at}: must be one line"));
    }
    if s.chars().count() > max {
        return Err(format!("{at}: longer than {max} characters"));
    }
    Ok(s.trim())
}

/// A JSON string literal: the user's text stays on its line and cannot
/// pass for harness lines around it.
fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("a string always serializes")
}

/// A number as the harness writes it: `3`, not `3.0`.
fn num(n: f64) -> String {
    // Below 2^53 every whole f64 is exactly an i64.
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        (n as i64).to_string()
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::UiLevel;
    use serde_json::json;

    fn block(kind: &str, body: Value) -> UiBlock {
        let text = format!("```ui:{kind}\n{body}\n```\n");
        let b = super::super::extract(&text, UiLevel::Rich).remove(0);
        assert!(b.error.is_none(), "{:?}", b.error);
        b
    }

    fn run(b: &UiBlock, values: Value) -> Result<String, String> {
        answer(b, values.as_object().unwrap())
    }

    fn colours(extra: Value) -> UiBlock {
        let mut spec = json!({"id": "colour", "question": "Which?", "options": [
            {"id": "blue", "label": "Blue"}, {"id": "green", "label": "Green"}, {"id": "red", "label": "Red"}
        ]});
        spec.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        block("choices", spec)
    }

    #[test]
    fn single_choice() {
        let b = colours(json!({}));
        assert_eq!(
            run(&b, json!({"selected": ["blue"]})),
            Ok("Choice answer · colour: blue".into())
        );
        for (values, err) in [
            (json!({"selected": []}), "selected: pick one"),
            (
                json!({"selected": ["blue", "red"]}),
                "selected: pick only one",
            ),
            (
                json!({"selected": ["pink"]}),
                "selected: not one of the options",
            ),
            (
                json!({"selected": "blue"}),
                "selected: expected a list of option ids",
            ),
            (
                json!({"selected": [1]}),
                "selected: expected a list of option ids",
            ),
            (json!({}), "selected: missing"),
            (
                json!({"selected": ["blue"], "other": "x"}),
                "other: not allowed for this question",
            ),
            (
                json!({"selected": ["blue"], "colour": 1}),
                "colour: unknown key",
            ),
            (
                json!({"selected": ["blue"], "Not An Id": 1}),
                "values: unknown key",
            ),
        ] {
            assert_eq!(run(&b, values.clone()), Err(err.into()), "{values}");
        }
    }

    #[test]
    fn single_choice_with_other() {
        let b = colours(json!({"other": true}));
        assert_eq!(
            run(&b, json!({"selected": [], "other": "  MariaDB 11 "})),
            Ok("Choice answer · colour: other \"MariaDB 11\"".into())
        );
        for (values, err) in [
            (
                json!({"selected": ["blue"], "other": "teal"}),
                "selected: pick only one",
            ),
            (
                json!({"selected": [], "other": " "}),
                "other: must not be empty",
            ),
            (
                json!({"selected": [], "other": "a\nb"}),
                "other: must be one line",
            ),
            (json!({"selected": [], "other": 5}), "other: expected text"),
            (
                json!({"selected": [], "other": "x".repeat(501)}),
                "other: longer than 500 characters",
            ),
        ] {
            assert_eq!(run(&b, values.clone()), Err(err.into()), "{values}");
        }
        // 500 characters, some of them several bytes long, are fine.
        assert!(run(&b, json!({"selected": [], "other": "é".repeat(500)})).is_ok());
    }

    #[test]
    fn multi_choice() {
        let b = colours(json!({"multi": true, "other": true, "min": 2, "max": 2}));
        // Option order, not the order they were picked in.
        assert_eq!(
            run(&b, json!({"selected": ["red", "blue"]})),
            Ok("Choice answer · colour: blue, red".into())
        );
        assert_eq!(
            run(&b, json!({"selected": ["green"], "other": "biome \"x\""})),
            Ok("Choice answer · colour: green, other \"biome \\\"x\\\"\"".into())
        );
        for (values, err) in [
            (json!({"selected": ["red"]}), "selected: pick at least 2"),
            (
                json!({"selected": ["red", "blue", "green"]}),
                "selected: pick at most 2",
            ),
            // "Pick 2" means two answers, "Other" included.
            (
                json!({"selected": ["red", "blue"], "other": "teal"}),
                "selected: pick at most 2",
            ),
            (
                json!({"selected": ["red", "red"]}),
                "selected: an option is picked twice",
            ),
        ] {
            assert_eq!(run(&b, values.clone()), Err(err.into()), "{values}");
        }
    }

    fn trip() -> UiBlock {
        block(
            "form",
            json!({"id": "trip", "title": "Trip", "fields": [
                {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12, "step": 1},
                {"id": "dest", "label": "Destination", "type": "choice", "options": [{"id": "lis", "label": "Lisbon"}, {"id": "rom", "label": "Rome"}]},
                {"id": "extras", "label": "Extras", "type": "choice", "multi": true, "options": ["Breakfast", "Parking", "Late checkout"], "max": 2},
                {"id": "start", "label": "Start", "type": "date", "min": "2026-11-08", "max": "2027-11-08"},
                {"id": "flex", "label": "Flexible", "type": "toggle"},
                {"id": "notes", "label": "Notes", "type": "text", "multiline": true, "max_length": 20},
                {"id": "name", "label": "Name", "type": "text", "max_length": 10},
                {"id": "budget", "label": "Budget", "type": "number", "step": 0.5, "unit": "EUR"},
                {"id": "key", "label": "API key", "type": "text"}
            ]}),
        )
    }

    #[test]
    fn form_canonical_text() {
        let got = run(
            &trip(),
            json!({
                "people": 3, "dest": "lis", "extras": ["o2", "o1"], "start": "2026-12-01",
                "flex": true, "notes": "Window seat,\nplease", "budget": 120.5
            }),
        );
        assert_eq!(
            got,
            Ok([
                "Form answer · trip",
                "- people: 3",
                "- dest: lis",
                "- extras: o1, o2",
                "- start: 2026-12-01",
                "- flex: yes",
                "- notes: \"Window seat,\\nplease\"",
                "- budget: 120.5",
                "- not answered: name",
                "- not shown (asks for a secret): key",
            ]
            .join("\n"))
        );
        assert_eq!(
            run(&trip(), json!({"people": 12.0, "flex": false})),
            Ok([
                "Form answer · trip",
                "- people: 12",
                "- flex: no",
                "- not answered: dest, extras, start, notes, name, budget",
                "- not shown (asks for a secret): key",
            ]
            .join("\n"))
        );
    }

    #[test]
    fn form_rules() {
        let ok = json!({"people": 2});
        let with = |k: &str, v: Value| {
            let mut o = ok.clone();
            o[k] = v;
            o
        };
        for (values, err) in [
            (json!({}), "people: required"),
            (json!({"people": "2"}), "people: expected a number"),
            (json!({"people": 0}), "people: must be at least 1"),
            (json!({"people": 13}), "people: must be at most 12"),
            (
                json!({"people": 2.5}),
                "people: must be in steps of 1 from 1",
            ),
            (
                with("budget", json!(0.25)),
                "budget: must be in steps of 0.5 from 0",
            ),
            (with("dest", json!("par")), "dest: not one of the options"),
            (with("dest", json!(["lis"])), "dest: expected an option id"),
            (with("extras", json!([])), "extras: pick at least 1"),
            (
                with("extras", json!(["o1", "o2", "o3"])),
                "extras: pick at most 2",
            ),
            (
                with("extras", json!("o1")),
                "extras: expected a list of option ids",
            ),
            (
                with("start", json!("2026-02-30")),
                "start: not a date (YYYY-MM-DD)",
            ),
            (
                with("start", json!("2026-11-07")),
                "start: must be 2026-11-08 or later",
            ),
            (
                with("start", json!("2027-11-09")),
                "start: must be 2027-11-08 or earlier",
            ),
            (with("flex", json!("yes")), "flex: expected true or false"),
            (with("notes", json!("")), "notes: must not be empty"),
            (
                with("notes", json!("x".repeat(21))),
                "notes: longer than 20 characters",
            ),
            (with("name", json!("a\nb")), "name: must be one line"),
            (with("name", json!(7)), "name: expected text"),
            (
                with("key", json!("sk-123")),
                "key: asks for a secret, so it takes no value",
            ),
            (with("colour", json!(1)), "colour: not a field of this form"),
            (
                with("Bad Key", json!(1)),
                "values: not a field of this form",
            ),
        ] {
            assert_eq!(run(&trip(), values.clone()), Err(err.into()), "{values}");
        }
        // Unknown keys are reported before missing required fields.
        assert_eq!(
            run(&trip(), json!({"zz": 1})),
            Err("zz: not a field of this form".into())
        );
    }

    #[test]
    fn numbers_in_errors_never_use_exponents() {
        let b = block(
            "form",
            json!({"id": "f", "title": "F", "fields": [
                {"id": "n", "label": "N", "type": "number", "min": 1e-7, "max": 1e21}
            ]}),
        );
        assert_eq!(
            run(&b, json!({"n": 1e22})),
            Err("n: must be at most 1000000000000000000000".into())
        );
        assert_eq!(
            run(&b, json!({"n": 1e-8})),
            Err("n: must be at least 0.0000001".into())
        );
        assert_eq!(
            run(&b, json!({"n": 5e20})),
            Ok("Form answer · f\n- n: 500000000000000000000".into())
        );
    }

    #[test]
    fn only_shown_input_blocks_take_answers() {
        let progress = block(
            "progress",
            json!({"id": "p", "title": "T", "steps": [{"label": "a", "status": "done"}]}),
        );
        assert_eq!(
            run(&progress, json!({})),
            Err("that block is not a question or form".into())
        );
        let mut rejected = colours(json!({}));
        (rejected.spec, rejected.error) = (None, Some("x".into()));
        assert_eq!(
            run(&rejected, json!({"selected": ["blue"]})),
            Err("that block was not shown".into())
        );
    }
}

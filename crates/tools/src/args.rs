//! A tool call's arguments checked against the schema the model was
//! offered, before approval and before the tool runs (Pi's
//! repair-then-validate; Oh My Pi and DeepSeek validate too). Without it a
//! tool read a wrong-typed field as absent and used its default
//! (`"offset": "200"` read from line 1, `"timeout_secs": "300"` ran for
//! 60 s) and the model was never told.
//!
//! What is checked: `type` (`object`, `array`, `string`, `integer`,
//! `number`, `boolean`, `null`, or a list of them), `properties`,
//! `required`, `items`, `enum`, and keys the schema does not name. A key
//! not named is refused where the schema says `additionalProperties:
//! false`, or, for one of our own tools ([`Closed::Builtin`]), where it
//! lists `properties` and does not open the object; another tool's schema
//! (an MCP server's) is open unless it says so, as JSON Schema reads it.
//! Every other keyword (`minimum`, `pattern`, `oneOf`, `$ref`, ...) is not
//! checked and never refuses a call: the tool, or an MCP server, still
//! checks its own input.
//!
//! What is repaired (Pi's coercion), so the tool card, the approval card,
//! the approval key and the tool see the repaired value, never the raw one:
//! - `null` for an argument that is not required: removed (the tools read
//!   it as absent; models send it for optionals all the time);
//! - where exactly one type (besides `null`) is allowed: a string holding
//!   an integer (`"10"`, `"10.0"`) or a number, `"true"` / `"false"` for a
//!   boolean, a string holding JSON of exactly the array or object
//!   expected (the `edit` tool's `edits` sent as a string); a string is
//!   never changed where a string is allowed;
//! - an integral float where an integer is expected (`15.0` becomes `15`,
//!   which the tool's `as_u64()` reads).
//!
//! `delegate` and `finish` never reach this check: the loop takes them out
//! of the step's calls first, and each checks its own input.

use serde_json::{Map, Value};

/// Whose schema it is: how an object that does not say whether it takes
/// other keys is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// One of our own tools: its `properties` are all it takes.
    Builtin,
    /// Another tool's (an MCP server's): open unless it says
    /// `additionalProperties: false`.
    Schema,
}

/// `input` checked against `schema`, repaired; or the first problem, naming
/// the field and what it should be, to return to the model as the call's
/// result.
pub fn check(schema: &Value, input: &Value, closed: Closed) -> Result<Value, String> {
    let mut v = input.clone();
    walk(schema, &mut v, "", closed)?;
    Ok(v)
}

/// Read `alias` as `canonical` in the object `input`: the value moves to
/// the tool's own name. Accepted only where the tool's own name is absent;
/// both present is refused, naming both (a spelling never wins over the
/// real name silently). Anything that is not an object is left to the
/// schema check.
pub fn alias(input: &mut Value, alias: &str, canonical: &str) -> Result<(), String> {
    let Some(o) = input.as_object_mut() else {
        return Ok(());
    };
    if !o.contains_key(alias) {
        return Ok(());
    }
    if o.contains_key(canonical) {
        return Err(format!(
            "`{alias}` and `{canonical}` were both sent; send only `{canonical}`"
        ));
    }
    if let Some(v) = o.remove(alias) {
        o.insert(canonical.to_string(), v);
    }
    Ok(())
}

/// The types `schema` allows, if it says.
fn types(schema: &Value) -> Option<Vec<&str>> {
    match schema.get("type")? {
        Value::String(t) => Some(vec![t.as_str()]),
        Value::Array(a) => Some(a.iter().filter_map(Value::as_str).collect()),
        _ => None,
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "an integer",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn fits(t: &str, v: &Value) -> bool {
    match (t, v) {
        ("object", Value::Object(_))
        | ("array", Value::Array(_))
        | ("string", Value::String(_))
        | ("boolean", Value::Bool(_))
        | ("null", Value::Null)
        | ("number", Value::Number(_)) => true,
        ("integer", Value::Number(n)) => {
            n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        // A type this check does not know: not refused.
        (t, _) => !matches!(
            t,
            "object" | "array" | "string" | "boolean" | "null" | "number" | "integer"
        ),
    }
}

/// `f` as an integer, if it is a whole number within the i64 range.
fn whole(f: f64) -> Option<Value> {
    (f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64)
        .then(|| Value::from(f as i64))
}

/// `s` as the value of type `t`, if it reads as one exactly.
fn coerce(t: &str, s: &str) -> Option<Value> {
    let s = s.trim();
    match t {
        "integer" => s
            .parse::<i64>()
            .ok()
            .map(Value::from)
            .or_else(|| s.parse::<u64>().ok().map(Value::from))
            .or_else(|| s.parse::<f64>().ok().and_then(whole)),
        "array" | "object" => serde_json::from_str::<Value>(s)
            .ok()
            .filter(|v| v.is_array() == (t == "array") && (v.is_array() || v.is_object())),
        "number" => s.parse::<i64>().ok().map(Value::from).or_else(|| {
            s.parse::<f64>()
                .ok()
                .filter(|f| f.is_finite())
                .map(Value::from)
        }),
        "boolean" => match s {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            _ => None,
        },
        _ => None,
    }
}

fn name(path: &str) -> String {
    if path.is_empty() {
        "the arguments".into()
    } else {
        format!("`{path}`")
    }
}

/// The one type `ts` allows besides `null`, if there is exactly one.
fn single<'a>(ts: &[&'a str]) -> Option<&'a str> {
    let mut real = ts.iter().filter(|t| **t != "null");
    match (real.next(), real.next()) {
        (Some(t), None) => Some(t),
        _ => None,
    }
}

fn walk(schema: &Value, v: &mut Value, path: &str, closed: Closed) -> Result<(), String> {
    // An integral float where an integer is expected is that integer: the
    // tools read integers with `as_u64()`, which a float is not.
    if let Some(ts) = types(schema)
        && ts.contains(&"integer")
        && let Value::Number(n) = v
        && !(n.is_i64() || n.is_u64())
        && let Some(i) = n.as_f64().and_then(whole)
    {
        *v = i;
    }
    if let Some(ts) = types(schema)
        && !ts.is_empty()
        && !ts.iter().any(|t| fits(t, v))
    {
        // Only where one type is meant: a union is not guessed at.
        let repaired = match (&*v, single(&ts)) {
            (Value::String(s), Some(t)) => coerce(t, s),
            _ => None,
        };
        match repaired {
            Some(r) => *v = r,
            None => {
                return Err(format!(
                    "{} must be {}, not {}",
                    name(path),
                    ts.iter()
                        .map(|t| match *t {
                            "integer" => "an integer",
                            "number" => "a number",
                            "string" => "a string",
                            "boolean" => "true or false",
                            "array" => "an array",
                            "object" => "an object",
                            "null" => "null",
                            other => other,
                        })
                        .collect::<Vec<_>>()
                        .join(" or "),
                    kind(v)
                ));
            }
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        // `2.0` for an enum of integers is `2`.
        if let Value::Number(n) = v
            && !(n.is_i64() || n.is_u64())
            && let Some(i) = n.as_f64().and_then(whole)
            && allowed.contains(&i)
        {
            *v = i;
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(v)
    {
        let list: Vec<String> = allowed.iter().map(Value::to_string).collect();
        return Err(format!("{} must be one of {}", name(path), list.join(", ")));
    }
    match v {
        Value::Object(map) => object(schema, map, path, closed),
        Value::Array(items) => match schema.get("items") {
            Some(item) if item.is_object() => {
                for (i, x) in items.iter_mut().enumerate() {
                    walk(item, x, &format!("{path}[{i}]"), closed)?;
                }
                Ok(())
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

fn object(
    schema: &Value,
    map: &mut Map<String, Value>,
    path: &str,
    whose: Closed,
) -> Result<(), String> {
    let at = |k: &str| {
        if path.is_empty() {
            k.to_string()
        } else {
            format!("{path}.{k}")
        }
    };
    let props = schema.get("properties").and_then(Value::as_object);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    // `null` for an optional argument is its absence.
    map.retain(|k, v| !v.is_null() || required.contains(&k.as_str()));
    for k in &required {
        if !map.contains_key(*k) {
            return Err(format!("`{}` is required and missing", at(k)));
        }
    }
    let Some(props) = props else {
        return Ok(());
    };
    let closed = match schema.get("additionalProperties") {
        Some(Value::Bool(false)) => true,
        None => whose == Closed::Builtin && schema.get("patternProperties").is_none(),
        Some(_) => false,
    };
    for (k, v) in map.iter_mut() {
        match props.get(k) {
            Some(p) => walk(p, v, &at(k), whose)?,
            None if closed => {
                let mut known: Vec<&str> = props.keys().map(String::as_str).collect();
                known.sort_unstable();
                return Err(format!(
                    "`{}` is not an argument of this tool; it takes {}",
                    at(k),
                    known
                        .iter()
                        .map(|k| format!("`{k}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            None => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(schema: &Value, input: &Value) -> Result<Value, String> {
        super::check(schema, input, Closed::Builtin)
    }

    /// `write` and `edit` read other harnesses' names as their own, only
    /// where their own name is absent; the schema offered does not list them.
    #[test]
    fn write_and_edit_read_the_aliases_of_other_harnesses() {
        use crate::Tool;
        let norm = |t: &dyn Tool, mut v: Value| t.normalize_args(&mut v).map(|_| v);
        let (w, e) = (crate::fs::Write, crate::edit::Edit);
        assert_eq!(
            norm(&w, json!({"path": "a", "file_text": "x"})),
            Ok(json!({"path": "a", "content": "x"}))
        );
        assert_eq!(
            norm(&e, json!({"path": "a", "old_str": "x", "new_str": "y"})),
            Ok(json!({"path": "a", "old_string": "x", "new_string": "y"}))
        );
        assert_eq!(
            norm(&e, json!({"path": "a", "old_string": "x", "new_str": "y"})),
            Ok(json!({"path": "a", "old_string": "x", "new_string": "y"}))
        );
        // Both: refused, naming both. An alias of the other tool is not one.
        let both = norm(&w, json!({"path": "a", "content": "x", "file_text": "y"}));
        assert_eq!(
            both,
            Err("`file_text` and `content` were both sent; send only `content`".into())
        );
        let both = norm(&e, json!({"old_string": "x", "old_str": "y"})).unwrap_err();
        assert!(
            both.contains("`old_str`") && both.contains("`old_string`"),
            "{both}"
        );
        assert_eq!(
            norm(&w, json!({"path": "a", "old_str": "x"})),
            Ok(json!({"path": "a", "old_str": "x"}))
        );
        // Not an object: left to the schema check.
        assert_eq!(norm(&w, json!("x")), Ok(json!("x")));
        // The aliases are not in what the model is offered; the real names
        // are the only ones the closed schema takes.
        for t in [&w as &dyn Tool, &e] {
            let schema = t.schema().to_string();
            assert!(
                !schema.contains("file_text") && !schema.contains("_str\""),
                "{schema}"
            );
        }
        let unchecked = json!({"path": "a", "file_text": "x"});
        let e = super::check(&w.schema(), &unchecked, Closed::Builtin).unwrap_err();
        assert!(e.contains("file_text") || e.contains("content"), "{e}");
    }

    fn bash() -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_secs": {"type": "integer"},
                "background": {"type": "boolean"},
                "workdir": {"type": "string"}
            },
            "required": ["command"]
        })
    }

    #[test]
    fn numeric_strings_and_boolean_words_are_repaired() {
        let out = check(
            &bash(),
            &json!({"command": "ls", "timeout_secs": " 300 ", "background": "true"}),
        )
        .unwrap();
        assert_eq!(
            out,
            json!({"command": "ls", "timeout_secs": 300, "background": true})
        );
        assert_eq!(
            check(
                &json!({"type": "object", "properties": {"x": {"type": "number"}}}),
                &json!({"x": "1.5"})
            )
            .unwrap(),
            json!({"x": 1.5})
        );
        // A float with no fraction is that integer (review M1), as is the
        // string "10.0".
        assert_eq!(
            check(&bash(), &json!({"command": "ls", "timeout_secs": 30.0})).unwrap(),
            json!({"command": "ls", "timeout_secs": 30})
        );
        assert_eq!(
            check(&bash(), &json!({"command": "ls", "timeout_secs": "10.0"})).unwrap()["timeout_secs"],
            json!(10)
        );
        let e = json!({"type": "object", "properties": {"n": {"enum": [1, 2, 3]}}});
        assert_eq!(check(&e, &json!({"n": 2.0})).unwrap(), json!({"n": 2}));
    }

    #[test]
    fn a_wrong_type_names_the_field_and_the_type() {
        let e = check(&bash(), &json!({"command": "ls", "timeout_secs": "soon"})).unwrap_err();
        assert_eq!(e, "`timeout_secs` must be an integer, not a string");
        let e = check(&bash(), &json!({"command": 7})).unwrap_err();
        assert_eq!(e, "`command` must be a string, not an integer");
        let e = check(&bash(), &json!({"command": "ls", "background": "yes"})).unwrap_err();
        assert_eq!(e, "`background` must be true or false, not a string");
        // A string is never made from a number: the model is told.
        let e = check(&bash(), &json!({"command": "ls", "timeout_secs": 1.5})).unwrap_err();
        assert!(e.contains("`timeout_secs` must be an integer"), "{e}");
    }

    #[test]
    fn required_and_unknown_keys_are_refused_by_name() {
        assert_eq!(
            check(&bash(), &json!({"timeout_secs": 5})).unwrap_err(),
            "`command` is required and missing"
        );
        assert_eq!(
            check(&bash(), &json!({"command": "ls", "timeout": 300})).unwrap_err(),
            "`timeout` is not an argument of this tool; it takes `background`, `command`, \
             `timeout_secs`, `workdir`"
        );
    }

    #[test]
    fn nested_items_and_enums_are_checked() {
        let todo = json!({
            "type": "object",
            "properties": {"items": {"type": "array", "items": {
                "type": "object",
                "properties": {"content": {"type": "string"}, "status": {"type": "string", "enum": ["pending", "done"]}},
                "required": ["content", "status"]
            }}},
            "required": ["items"]
        });
        let e = check(
            &todo,
            &json!({"items": [{"content": "a", "status": "later"}]}),
        )
        .unwrap_err();
        assert_eq!(e, "`items[0].status` must be one of \"pending\", \"done\"");
        let e = check(&todo, &json!({"items": [{"content": "a"}]})).unwrap_err();
        assert_eq!(e, "`items[0].status` is required and missing");
        assert!(
            check(
                &todo,
                &json!({"items": [{"content": "a", "status": "done"}]})
            )
            .is_ok()
        );
    }

    /// An MCP server's schema may use anything: what is not implemented
    /// here never refuses a call, and an open object keeps extra keys.
    #[test]
    fn keywords_not_implemented_never_refuse() {
        let s = json!({
            "type": "object",
            "properties": {
                "q": {"type": "string", "pattern": "^x", "minLength": 5},
                "n": {"oneOf": [{"type": "integer"}, {"type": "string"}]},
                "r": {"$ref": "#/defs/r"},
                "t": {"type": "uuid"}
            },
            "additionalProperties": true
        });
        let input = json!({"q": "abc", "n": [1], "r": 5, "t": 1, "extra": {"a": 1}});
        assert_eq!(check(&s, &input).unwrap(), input);
        // No `properties`: any keys.
        assert!(check(&json!({"type": "object"}), &json!({"anything": 1})).is_ok());
        // An empty or odd schema.
        assert!(check(&json!({}), &json!({"a": 1})).is_ok());
        assert!(check(&json!(true), &json!({"a": 1})).is_ok());
    }

    /// Review H1: `null` for an optional argument is its absence; for a
    /// required one it is still refused.
    #[test]
    fn null_for_an_optional_argument_is_removed() {
        assert_eq!(
            check(
                &bash(),
                &json!({"command": "ls", "workdir": null, "timeout_secs": null})
            )
            .unwrap(),
            json!({"command": "ls"})
        );
        assert!(check(&bash(), &json!({"command": null})).is_err());
    }

    /// Review H2: a string holding JSON of exactly the array or object
    /// expected is that value; a string field is never parsed.
    #[test]
    fn a_json_string_for_an_array_or_object_is_parsed() {
        let s = json!({"type": "object", "properties": {
            "edits": {"type": "array", "items": {"type": "object"}},
            "opts": {"type": "object"},
            "text": {"type": "string"}
        }});
        assert_eq!(
            check(
                &s,
                &json!({"edits": "[{\"a\": 1}]", "opts": "{\"b\": 2}", "text": "[1]"})
            )
            .unwrap(),
            json!({"edits": [{"a": 1}], "opts": {"b": 2}, "text": "[1]"})
        );
        // JSON of another type is not repaired.
        assert!(check(&s, &json!({"edits": "{\"a\": 1}"})).is_err());
        assert!(check(&s, &json!({"opts": "[1]"})).is_err());
        // The parsed value is checked as usual.
        assert!(check(&s, &json!({"edits": "[1]"})).is_err());
    }

    /// Review L2: a union is not guessed at.
    #[test]
    fn a_union_type_is_not_coerced() {
        let s = json!({"type": "object", "properties": {"x": {"type": ["integer", "boolean"]}, "y": {"type": ["integer", "null"]}}});
        assert!(check(&s, &json!({"x": "true"})).is_err());
        assert_eq!(check(&s, &json!({"y": "4"})).unwrap(), json!({"y": 4}));
    }

    /// Review M2: another tool's schema (an MCP server's) is open unless it
    /// says `additionalProperties: false`; one of ours is closed.
    #[test]
    fn only_our_own_schemas_are_closed_by_default() {
        let s = json!({"type": "object", "properties": {"q": {"type": "string"}}});
        let x = json!({"q": "x", "extra": 1});
        assert_eq!(super::check(&s, &x, Closed::Schema).unwrap(), x);
        assert!(super::check(&s, &x, Closed::Builtin).is_err());
        let shut = json!({"type": "object", "properties": {"q": {"type": "string"}}, "additionalProperties": false});
        assert!(super::check(&shut, &x, Closed::Schema).is_err());
    }
}

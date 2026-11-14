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
//! not named is refused only where the schema lists `properties` and does
//! not open the object (`additionalProperties` absent or `false`; a
//! `patternProperties` opens it too). Every other keyword (`minimum`,
//! `pattern`, `oneOf`, `$ref`, ...) is not checked and never refuses a call:
//! the tool, or an MCP server, still checks its own input.
//!
//! What is repaired (Pi's coercion), so the approval card and the tool see
//! the repaired value, never the raw one: a string holding an integer or a
//! number where one is expected, and `"true"` / `"false"` where a boolean
//! is. Nothing else is changed.

use serde_json::{Map, Value};

/// `input` checked against `schema`, repaired; or the first problem, naming
/// the field and what it should be, to return to the model as the call's
/// result.
pub fn check(schema: &Value, input: &Value) -> Result<Value, String> {
    let mut v = input.clone();
    walk(schema, &mut v, "")?;
    Ok(v)
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

/// `s` as the value of type `t`, if it reads as one exactly.
fn coerce(t: &str, s: &str) -> Option<Value> {
    let s = s.trim();
    match t {
        "integer" => s
            .parse::<i64>()
            .ok()
            .map(Value::from)
            .or_else(|| s.parse::<u64>().ok().map(Value::from)),
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

fn walk(schema: &Value, v: &mut Value, path: &str) -> Result<(), String> {
    if let Some(ts) = types(schema)
        && !ts.is_empty()
        && !ts.iter().any(|t| fits(t, v))
    {
        let repaired = match v {
            Value::String(s) => ts.iter().find_map(|t| coerce(t, s)),
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
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(v)
    {
        let list: Vec<String> = allowed.iter().map(Value::to_string).collect();
        return Err(format!("{} must be one of {}", name(path), list.join(", ")));
    }
    match v {
        Value::Object(map) => object(schema, map, path),
        Value::Array(items) => match schema.get("items") {
            Some(item) if item.is_object() => {
                for (i, x) in items.iter_mut().enumerate() {
                    walk(item, x, &format!("{path}[{i}]"))?;
                }
                Ok(())
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

fn object(schema: &Value, map: &mut Map<String, Value>, path: &str) -> Result<(), String> {
    let at = |k: &str| {
        if path.is_empty() {
            k.to_string()
        } else {
            format!("{path}.{k}")
        }
    };
    let props = schema.get("properties").and_then(Value::as_object);
    if let Some(req) = schema.get("required").and_then(Value::as_array) {
        for k in req.iter().filter_map(Value::as_str) {
            if !map.contains_key(k) {
                return Err(format!("`{}` is required and missing", at(k)));
            }
        }
    }
    let Some(props) = props else {
        return Ok(());
    };
    let closed = matches!(
        schema.get("additionalProperties"),
        None | Some(Value::Bool(false))
    ) && schema.get("patternProperties").is_none();
    for (k, v) in map.iter_mut() {
        match props.get(k) {
            Some(p) => walk(p, v, &at(k))?,
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
        // A float with no fraction is an integer.
        assert!(check(&bash(), &json!({"command": "ls", "timeout_secs": 30.0})).is_ok());
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
}

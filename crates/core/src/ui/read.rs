//! Reading the model's JSON with exact error paths (`fields[2].label: ...`).
//!
//! Every key an object may have is read through [`Obj`], and
//! [`Obj::finish`] rejects whatever was not read, so no key is silently
//! ignored (serde's `deny_unknown_fields`, with a path). Error messages are
//! harness text: a path, a reason and limits, plus keys and ids only when
//! they pass a strict character check, never other model text.

use serde_json::{Map, Value};

use crate::text::clean_text;

pub(super) type Res<T> = Result<T, String>;

/// A block id, an option id or a field id: `^[a-z][a-z0-9_-]{0,31}$`. They
/// reach the model in answers, so they must never carry a URL or prose.
pub fn valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

const ID_RULE: &str = "not a valid id (a-z, 0-9, _ and -, at most 32, starting with a letter)";

pub(super) struct Obj<'a> {
    map: &'a Map<String, Value>,
    path: String,
    read: Vec<&'static str>,
}

impl<'a> Obj<'a> {
    /// `path` is where `v` sits (`""` for the body itself).
    pub fn new(v: &'a Value, path: String) -> Res<Self> {
        match v {
            Value::Object(map) => Ok(Self {
                map,
                path,
                read: Vec::new(),
            }),
            _ => Err(format!("{}: expected an object", here(&path))),
        }
    }

    /// The error path of `key` in this object.
    pub fn at(&self, key: &str) -> String {
        if self.path.is_empty() {
            key.to_string()
        } else {
            format!("{}.{key}", self.path)
        }
    }

    pub fn err<T>(&self, key: &str, reason: impl std::fmt::Display) -> Res<T> {
        Err(format!("{}: {reason}", self.at(key)))
    }

    /// The value of `key`, marked as read. Absent and `null` are both `None`.
    pub fn get(&mut self, key: &'static str) -> Option<&'a Value> {
        self.read.push(key);
        self.map.get(key).filter(|v| !v.is_null())
    }

    /// The raw value under `key`, without marking it read.
    pub fn peek(&self, key: &str) -> Option<&'a Value> {
        self.map.get(key)
    }

    /// A cleaned string of at most `max` characters; `None` when absent or
    /// empty once cleaned.
    pub fn text(&mut self, key: &'static str, max: usize, multiline: bool) -> Res<Option<String>> {
        let Some(v) = self.get(key) else {
            return Ok(None);
        };
        let Some(s) = v.as_str() else {
            return self.err(key, "expected a string");
        };
        let s = cleaned(s, max, multiline, &self.at(key))?;
        Ok((!s.is_empty()).then_some(s))
    }

    /// A one-line string that must be there and not empty once cleaned.
    pub fn req_text(&mut self, key: &'static str, max: usize) -> Res<String> {
        match self.text(key, max, false)? {
            Some(s) => Ok(s),
            None if self.map.get(key).is_some_and(|v| !v.is_null()) => {
                self.err(key, "must not be empty")
            }
            None => self.err(key, "missing"),
        }
    }

    pub fn bool(&mut self, key: &'static str) -> Res<Option<bool>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => self.err(key, "expected true or false"),
        }
    }

    pub fn num(&mut self, key: &'static str) -> Res<Option<f64>> {
        match self.get(key) {
            None => Ok(None),
            // JSON has no NaN or infinity; a huge exponent fails to parse.
            Some(v) => match v.as_f64() {
                Some(n) => Ok(Some(n)),
                None => self.err(key, "expected a number"),
            },
        }
    }

    /// A whole number (`2.0` counts) from 0 to `u32::MAX`.
    pub fn count(&mut self, key: &'static str) -> Res<Option<u32>> {
        match self.num(key)? {
            None => Ok(None),
            Some(n) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n) => {
                Ok(Some(n as u32))
            }
            Some(_) => self.err(key, "expected a whole number"),
        }
    }

    /// A list of `min..=max` entries, which must be there.
    pub fn list(&mut self, key: &'static str, min: usize, max: usize) -> Res<&'a [Value]> {
        let list = match self.get(key) {
            None => return self.err(key, "missing"),
            Some(Value::Array(a)) => a.as_slice(),
            Some(_) => return self.err(key, "expected a list"),
        };
        if list.len() < min {
            return self.err(key, format_args!("fewer than {min} entries"));
        }
        if list.len() > max {
            return self.err(key, format_args!("more than {max} entries"));
        }
        Ok(list)
    }

    /// An object nested under `key`, if there.
    pub fn obj(&mut self, key: &'static str) -> Res<Option<Obj<'a>>> {
        let at = self.at(key);
        self.get(key).map(|v| Obj::new(v, at)).transpose()
    }

    pub fn id(&mut self, key: &'static str) -> Res<Option<String>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) if valid_id(s) => Ok(Some(s.clone())),
            Some(Value::String(_)) => self.err(key, ID_RULE),
            Some(_) => self.err(key, "expected a string"),
        }
    }

    /// One of a fixed set of words.
    pub fn word<T: Copy>(&mut self, key: &'static str, words: &[(&str, T)]) -> Res<Option<T>> {
        let Some(v) = self.get(key) else {
            return Ok(None);
        };
        match words.iter().find(|(w, _)| v.as_str() == Some(w)) {
            Some((_, t)) => Ok(Some(*t)),
            None => {
                let names: Vec<&str> = words.iter().map(|(w, _)| *w).collect();
                let (last, rest) = names.split_last().expect("at least one word");
                self.err(key, format_args!("must be {} or {last}", rest.join(", ")))
            }
        }
    }

    /// Fail on the first key that was not read: "unknown key", or, for a key
    /// in `elsewhere` (one another variant of this object may have), `why`.
    pub fn finish_with(self, elsewhere: &[&str], why: &str) -> Res<()> {
        // Sorted, so the message does not depend on serde_json's map order.
        let mut rest: Vec<&String> = self
            .map
            .keys()
            .filter(|k| !self.read.contains(&k.as_str()))
            .collect();
        rest.sort();
        let Some(k) = rest.first() else {
            return Ok(());
        };
        if elsewhere.contains(&k.as_str()) {
            return self.err(k, why);
        }
        // A key is model text: name it only when it is plainly a key.
        let plain = (1..=32).contains(&k.len())
            && k.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if plain {
            self.err(k, "unknown key")
        } else {
            Err(format!("{}: has an unknown key", here(&self.path)))
        }
    }

    pub fn finish(self) -> Res<()> {
        self.finish_with(&[], "")
    }
}

/// `s` through `clean_text`, or an error at `at` if it is then longer than
/// `max` characters (Unicode scalar values).
pub(super) fn cleaned(s: &str, max: usize, multiline: bool, at: &str) -> Res<String> {
    let s = clean_text(s, multiline);
    if s.chars().count() > max {
        return Err(format!("{at}: longer than {max} characters"));
    }
    Ok(s)
}

/// The path of an object for a message about the object itself.
fn here(path: &str) -> &str {
    if path.is_empty() { "body" } else { path }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids() {
        for ok in ["a", "db", "o1", "my_id-2", &"a".repeat(32)] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "",
            "1a",
            "_a",
            "A",
            "aB",
            "a.b",
            "a b",
            "é",
            &"a".repeat(33),
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn paths_types_and_unknown_keys() {
        let v = json!({"a": "x", "n": 1.5, "b": "yes", "l": [], "bad key!": 1});
        let mut o = Obj::new(&v, "fields[2]".into()).unwrap();
        assert_eq!(o.text("a", 1, false).unwrap().as_deref(), Some("x"));
        assert_eq!(
            o.count("n"),
            Err("fields[2].n: expected a whole number".into())
        );
        assert_eq!(
            o.bool("b"),
            Err("fields[2].b: expected true or false".into())
        );
        assert_eq!(
            o.list("l", 1, 2),
            Err("fields[2].l: fewer than 1 entries".into())
        );
        assert_eq!(o.req_text("z", 5), Err("fields[2].z: missing".into()));
        assert_eq!(o.finish(), Err("fields[2]: has an unknown key".into()));
        let v = json!({"zz": 1, "aa": 2});
        assert_eq!(
            Obj::new(&v, String::new()).unwrap().finish(),
            Err("aa: unknown key".into())
        );
        assert_eq!(
            Obj::new(&json!([]), String::new()).err(),
            Some("body: expected an object".into())
        );
    }

    #[test]
    fn text_is_cleaned_then_measured() {
        let v = json!({"a": " a\u{202E}b ", "e": "\u{200B} ", "long": "ééé"});
        let mut o = Obj::new(&v, String::new()).unwrap();
        assert_eq!(o.text("a", 2, false).unwrap().as_deref(), Some("ab"));
        assert_eq!(o.req_text("e", 5), Err("e: must not be empty".into()));
        assert_eq!(o.text("long", 3, false).unwrap().as_deref(), Some("ééé"));
        assert_eq!(
            o.text("long", 2, false),
            Err("long: longer than 2 characters".into())
        );
    }

    #[test]
    fn words() {
        let v = json!({"k": "line", "j": "pie"});
        let mut o = Obj::new(&v, String::new()).unwrap();
        let kinds = [("bar", 1), ("line", 2)];
        assert_eq!(o.word("k", &kinds), Ok(Some(2)));
        assert_eq!(o.word("j", &kinds), Err("j: must be bar or line".into()));
        assert_eq!(o.word("x", &kinds), Ok(None));
    }
}

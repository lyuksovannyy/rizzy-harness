//! What a person's "allow for this turn" covers: this exact call (PLAN.md
//! T4). The loop, which skips asking again, and the node, which decides
//! when the choice is offered, both key the allowance with [`call_key`], so
//! the two can never disagree about what was approved.
//!
//! - `bash` and every tool not named below: the tool's name and its whole
//!   input as canonical JSON (object keys sorted, nothing else changed). For
//!   `bash` that is the command string byte for byte together with every
//!   other argument (`timeout_secs`); one more space is another command.
//!   There is no command parser here on purpose: a parser that
//!   misunderstands a command widens the allowance.
//! - `write` and `edit`: the real path the tool will write (the same
//!   lexical normalisation and symlink walk as [`crate::ToolCtx::resolve`]),
//!   so two spellings of one file are one key, and the rest of the input,
//!   so one edit to a file does not approve another. A path that cannot be
//!   resolved falls back to the input as given: only the identical spelling
//!   matches, and the tool refuses it anyway.
//!
//! Every call runs in the session's folder, which is part of the key too.
//! The key is a SHA-256 over length-prefixed fields; the argument itself is
//! never kept, so it cannot end up in a log line through the key.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// A digest of one call: tool, folder and canonical argument.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallKey([u8; 32]);

impl std::fmt::Debug for CallKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CallKey(")?;
        for b in &self.0[..6] {
            write!(f, "{b:02x}")?;
        }
        write!(f, "…)")
    }
}

/// The key of `tool` called with `input` in a session working in `cwd`.
pub fn call_key(cwd: &Path, tool: &str, input: &Value) -> CallKey {
    let mut h = Sha256::new();
    field(&mut h, b"rizzy-harness call key v1");
    field(&mut h, tool.as_bytes());
    let base = cwd.canonicalize().unwrap_or_else(|_| crate::normalize(cwd));
    field(&mut h, base.as_os_str().as_encoded_bytes());
    let target = match (tool, input.get("path").and_then(Value::as_str)) {
        ("write" | "edit", Some(p)) => real_path(&base, p),
        _ => None,
    };
    match target {
        Some(path) => {
            let mut rest = input.clone();
            if let Some(m) = rest.as_object_mut() {
                m.remove("path");
            }
            field(&mut h, b"path");
            field(&mut h, path.as_os_str().as_encoded_bytes());
            field(&mut h, &canonical(&rest));
        }
        None => {
            field(&mut h, b"input");
            field(&mut h, &canonical(input));
        }
    }
    CallKey(h.finalize().into())
}

/// Where `write` or `edit` of `p` lands: `p` joined to the folder,
/// normalised, symlinks followed as far as they exist.
fn real_path(base: &Path, p: &str) -> Option<PathBuf> {
    let raw = Path::new(p);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base.join(raw)
    };
    crate::walk(&crate::normalize(&joined), None, p).ok()
}

/// One field, prefixed by its length, so no two field lists hash alike.
fn field(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

/// `v` as JSON with every object's keys sorted, whatever order the model
/// sent them in (and whatever map type serde_json was built with).
fn canonical(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(&Value::String(k.clone()), out);
                out.push(b':');
                write_canonical(&m[k], out);
            }
            out.push(b'}');
        }
        Value::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(x, out);
            }
            out.push(b']');
        }
        // Strings, numbers, booleans and null serialise exactly.
        scalar => out.extend_from_slice(scalar.to_string().as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(dir: &Path, tool: &str, input: Value) -> CallKey {
        call_key(dir, tool, &input)
    }

    #[test]
    fn bash_is_the_command_byte_for_byte_with_its_other_arguments() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let ls = key(d, "bash", json!({"command": "ls"}));
        assert_eq!(ls, key(d, "bash", json!({"command": "ls"})));
        assert_ne!(ls, key(d, "bash", json!({"command": "ls "})));
        assert_ne!(ls, key(d, "bash", json!({"command": "rm -rf x"})));
        assert_ne!(
            ls,
            key(d, "bash", json!({"command": "ls", "timeout_secs": 5}))
        );
        // The same input to another tool, or in another folder, is another call.
        assert_ne!(ls, key(d, "grep", json!({"command": "ls"})));
        let other = tempfile::tempdir().unwrap();
        assert_ne!(ls, key(other.path(), "bash", json!({"command": "ls"})));
    }

    #[test]
    fn write_and_edit_are_the_real_path_and_the_rest_of_the_input() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        std::fs::create_dir(d.join("a")).unwrap();
        let w = |p: &str, c: &str| key(d, "write", json!({"path": p, "content": c}));
        let f = w("a/f.txt", "x");
        assert_eq!(f, w("a/../a/f.txt", "x"));
        assert_eq!(f, w("./a/./f.txt", "x"));
        assert_eq!(f, w(&d.join("a/f.txt").display().to_string(), "x"));
        assert_ne!(f, w("a/g.txt", "x"));
        assert_ne!(f, w("a/f.txt", "y"));
        // A link to the file is the file.
        std::os::unix::fs::symlink(d.join("a"), d.join("b")).unwrap();
        assert_eq!(f, w("b/f.txt", "x"));
        let e = |p: &str, o: &str, n: &str| {
            key(
                d,
                "edit",
                json!({"path": p, "old_string": o, "new_string": n}),
            )
        };
        assert_eq!(e("a/f.txt", "1", "2"), e("a/../a/f.txt", "1", "2"));
        assert_ne!(e("a/f.txt", "1", "2"), e("a/f.txt", "1", "3"));
        assert_ne!(e("a/f.txt", "1", "2"), e("a/g.txt", "1", "2"));
        // `write` and `edit` of one file are different calls.
        assert_ne!(
            key(d, "write", json!({"path": "a/f.txt"})),
            key(d, "edit", json!({"path": "a/f.txt"}))
        );
    }

    #[test]
    fn other_tools_are_their_input_in_any_key_order() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let a: Value =
            serde_json::from_str(r#"{"a":1,"b":{"c":[1,{"y":2,"x":1}],"d":3}}"#).unwrap();
        let b: Value =
            serde_json::from_str(r#"{"b":{"d":3,"c":[1,{"x":1,"y":2}]},"a":1}"#).unwrap();
        assert_eq!(key(d, "publish", a.clone()), key(d, "publish", b));
        assert_ne!(
            key(d, "publish", a),
            key(
                d,
                "publish",
                json!({"a": 1, "b": {"c": [1, {"x": 1, "y": 3}], "d": 3}})
            )
        );
        // Array order is meaning, not spelling.
        assert_ne!(
            key(d, "publish", json!({"a": [1, 2]})),
            key(d, "publish", json!({"a": [2, 1]}))
        );
        // `path` is only special for `write` and `edit`.
        assert_ne!(
            key(d, "publish", json!({"path": "a/../f"})),
            key(d, "publish", json!({"path": "f"}))
        );
    }

    #[test]
    fn the_key_does_not_show_the_argument() {
        let d = tempfile::tempdir().unwrap();
        let k = key(d.path(), "bash", json!({"command": "echo sk-secret"}));
        assert!(!format!("{k:?}").contains("sk-secret"));
    }
}

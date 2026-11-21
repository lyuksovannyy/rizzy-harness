//! Scrubbing an event without touching its schema (PLAN.md T3).
//!
//! A configured secret can be any string, including a word the harness
//! writes itself: a field name (`approval_mode`), a type tag, an enum value
//! (`accept_edits`), an id, the chat's folder or a model's name. Scrubbing
//! such a position breaks the event (it no longer decodes, and a node whose
//! `session_created` does not decode cannot restart) or rewrites what the
//! harness reads back to act on. So only free text is scrubbed.
//!
//! Which positions are free is decided by the event's own types, once per
//! kind and path (array indexes and map keys generalised) for the life of
//! the process, by probing: a position whose change the typed event does
//! not take back unchanged (a struct's field name, a tag, an enum value, a
//! UUID) is fixed; a `String` field is text; below a `serde_json::Value`
//! (a tool's input, a ui spec, a form's values) everything is free, keys
//! included; a map's keys are free. Text is scrubbed except at the
//! positions in [`KEPT`], the values the harness writes and reads back
//! (ids, the chat's folder, the configured model and agent, hashes). A
//! position not on that list is scrubbed: a field added later is scrubbed
//! unless someone adds it on purpose.
//!
//! A number that holds a secret (a value made of digits can be written as
//! a number) becomes its scrubbed text where free JSON stands, and stays
//! where the types fix a number: such a position cannot hold the marker.
//!
//! A class is learned from a node that can tell: a string, or an object or
//! array with something in it. A `null`, an empty object or array, a number
//! or a bool holds no text, so it is passed over and teaches nothing (an
//! `Option<String>` that is `None` is written `null`, and the class learned
//! from that `null` once left a later string there unscrubbed for the life
//! of the process). A class is kept per path AND per shape of the node (a
//! string, an object, an array), so a path that is a bare string in one
//! variant of an enum and an object in another has each variant's own
//! class, whichever came first. Where a class and the node do not fit, the
//! node is scrubbed as free text: when in doubt, scrub.
//!
//! The classification rests on one assumption, checked by a test over
//! every kind: a path has one class for each shape, whichever element of an
//! array or entry of a map it is met in. Should it ever fail, the final check catches a
//! result that does not decode, and the edits are then taken one path at a
//! time, each checked ([`Outcome::Fallback`]).

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use serde::Deserialize;
use serde_json::{Map, Value};

use super::SecretSet;
use crate::event::EventKind;

/// The text positions kept as they are, by event kind and path (`[]` an
/// array element, `{}` a map's key): what the harness writes from its own
/// state and reads back to act on. Never changed, whole value or part: a
/// configured value that is not a secret is often a prefix of one (an env
/// `HOME=/home/someone` is a prefix of every chat's folder), and a folder
/// rewritten to `[REDACTED]/proj` breaks the session at its next load as a
/// broken key would. The cost: a secret a person puts into a folder's name,
/// a model's name or an agent's id stays in that one position (it is still
/// removed from all free text). Who reads each:
/// - `session_created` `cwd`, `model`, `agent_id`: the session's restore
///   (`node/src/session.rs`, `Session::restore`), the agent switch
///   (`node/src/session_agent.rs`, `switch_of`).
/// - every `turn_id`, `retry.turn_id`, `cleared`/`thinking.turn_id`: the
///   fold's items and running turn (`core/src/view.rs`), the context
///   records per turn (`agent-native/src/context.rs`, `records`), undo's
///   window (`node/src/others.rs`), callbacks (`master/src/callbacks.rs`).
/// - `message_id`, `variant.group`, `variant_selected`: the fold
///   (`view.rs`), the variant chosen (`agent-native/src/lib.rs`), a form's
///   answer checked against its message (`core/src/ui/reply.rs`).
/// - `approval_id`: the loop's pending answers (`agent-native/src/lib.rs`),
///   the fold's pending approvals.
/// - `grant_id`, `chat_allow_added.key`: the chat's grants rebuilt at
///   restore (`session.rs`, `CallKey::from_hex`).
/// - `bg_id`, `bg_ids[]`, `cause.callback[]`: the background table and its
///   next id (`session.rs`), callbacks (`master/src/callbacks.rs`), undo.
/// - `checkpoint.git_ref`: restore and undo (`session.rs`, `others.rs`,
///   `view.rs`).
/// - `checkpoint.head_moved.*.commit`: hex ids the node read from the
///   repository, shown short on the turn's note (X.42); a configured value
///   that is a run of hex digits must not rewrite them. The branch names stay
///   free text, scrubbed like any other.
/// - `seen.sha256`, `seen.base`: undo's changed-since check (`others.rs`).
/// - images' `media_id`, `sha256`: the loop attaches the bytes by hash
///   (`agent-native/src/images.rs`).
/// - `artifact_staged` `stage_id`, `sha256`: the master's artifact pull
///   (`master/src/artifacts.rs`).
/// - `agent.agent_id`, `agent_changed.model`: the agent switch.
/// - `user_message.triggers[]`, `peers_skipped.agents[]`: agent ids, which
///   the node compares with `agent.agent_id` to say which of the agents a
///   message called were not asked (`session_peers.rs`).
/// - `thinking.model`, `thinking_withheld.model`: which model's thinking
///   is sent back (`agent-native/src/llm.rs`, `withhold_thinking`).
/// - `usage.model`, `usage.by_model` keys: usage merged by model
///   (`Usage::add`); the two must read the same.
/// - `sections[].digest`: what the history holds of a standing section
///   (`context.rs`, `had`). `sections[].key` has its own rule
///   ([`section_key`]).
pub const KEPT: &[(&str, &[&str])] = &[
    ("session_created", &["cwd", "model", "agent_id"]),
    (
        "turn_started",
        &[
            "turn_id",
            "agent.agent_id",
            "cause.callback[]",
            "cause.mention.tags[]",
            "retry.turn_id",
        ],
    ),
    (
        "user_message",
        &[
            "turn_id",
            "reply.message_id",
            "files[].seen.sha256",
            "files[].seen.base",
            "images[].media_id",
            "images[].sha256",
            "triggers[]",
            "tags[]",
        ],
    ),
    (
        "assistant_delta",
        &["turn_id", "message_id", "variant.group"],
    ),
    (
        "assistant_message",
        &["turn_id", "message_id", "variant.group", "thinking.model"],
    ),
    ("tool_call_started", &["turn_id"]),
    ("tool_output_delta", &[]),
    (
        "tool_call_finished",
        &["seen.sha256", "seen.base", "image.sha256"],
    ),
    ("approval_requested", &["approval_id"]),
    ("approval_resolved", &["approval_id"]),
    (
        "checkpoint",
        &[
            "turn_id",
            "git_ref",
            "head_moved.from.commit",
            "head_moved.to.commit",
        ],
    ),
    (
        "turn_ended",
        &["turn_id", "usage.model", "usage.by_model.{}"],
    ),
    (
        "turn_usage",
        &["turn_id", "usage.model", "usage.by_model.{}"],
    ),
    ("error", &["turn_id"]),
    ("agent_changed", &["agent.agent_id", "model"]),
    ("variant_selected", &["group", "message_id"]),
    (
        "history_summarized",
        &["turn_id", "usage.model", "usage.by_model.{}"],
    ),
    (
        "context_shortened",
        &["turn_id", "cleared.turn_id", "thinking.turn_id"],
    ),
    ("model_fallback", &["turn_id"]),
    ("model_retry", &["turn_id"]),
    ("retry_given_up", &["turn_id"]),
    ("artifact_staged", &["stage_id", "sha256"]),
    ("artifact_stage_failed", &[]),
    ("background_started", &["bg_id", "turn_id"]),
    (
        "background_ended",
        &["bg_id", "child.usage.model", "child.usage.by_model.{}"],
    ),
    ("callback_refused", &["bg_ids[]"]),
    ("queued_delivered", &["turn_id"]),
    ("todo_updated", &["turn_id"]),
    ("approval_mode_changed", &[]),
    ("context_sent", &["turn_id", "sections[].digest"]),
    ("chat_allow_added", &["grant_id", "approval_id", "key"]),
    ("chat_allow_removed", &["grant_id"]),
    ("step_limit_changed", &[]),
    ("handoff_written", &["usage.model", "usage.by_model.{}"]),
    ("compact_failed", &["usage.model", "usage.by_model.{}"]),
    ("images_withheld", &["turn_id"]),
    ("thinking_withheld", &["turn_id", "model"]),
    ("peers_skipped", &["agents[]"]),
    // `turn_id`: the turn the rewound message opened, read with the files
    // part (`session_rewind.rs`) and shown in the chat's words.
    ("conversation_rewound", &["turn_id"]),
    ("conversation_rewind_undone", &[]),
];

/// The keys of the harness's own context sections (`agent-native`'s
/// `SectionKey`, checked there against this list), besides the master's
/// [`crate::backend::StandingKey`]s.
pub const SECTION_KEYS: &[&str] = &["user_instructions", "instructions", "skills", "turn"];

/// How the key of a subfolder's instruction file begins; the rest is the
/// file's path in the repository.
pub const NESTED_SECTION_PREFIX: &str = "instructions:";

/// A `context_sent` section's key as it is stored: one of the harness's
/// own keys as it is; a subfolder file's key with its prefix kept and its
/// path scrubbed (so it still reads as one, the same in every turn); any
/// other text scrubbed whole.
fn section_key(set: &SecretSet, key: &str) -> String {
    let own = SECTION_KEYS.contains(&key)
        || crate::backend::StandingKey::ALL.iter().any(|k| {
            serde_json::to_value(k)
                .ok()
                .is_some_and(|v| v.as_str() == Some(key))
        });
    if own {
        return key.to_string();
    }
    match key.strip_prefix(NESTED_SECTION_PREFIX) {
        Some(path) => format!("{NESTED_SECTION_PREFIX}{}", set.scrub(path)),
        None => set.scrub(key),
    }
}

/// What a position of an event is, by its type.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A `serde_json::Value`: free, everything below it too, keys included.
    Free,
    /// A typed `String`: free text, unless its position is [`KEPT`].
    Text,
    /// A string the types fix (an enum value, a tag, a UUID).
    Fixed,
    /// A struct: its keys are field names.
    Struct,
    /// A map: its keys are data.
    Map,
    /// A typed sequence.
    Seq,
}

/// What [`SecretSet::scrub_event`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing in it to scrub, or only kept positions: untouched.
    Clean,
    /// Its free text scrubbed.
    Scrubbed,
    /// A kind this build does not know: every string value scrubbed, no
    /// key touched.
    Unknown,
    /// The scrubbed event did not decode (a path whose class depends on
    /// the element; none is known): the edits were taken one path at a
    /// time, each checked, and those of `left` paths were left out. What
    /// is stored decodes. Worth one logged line, with no text of it.
    Fallback { left: usize },
}

#[derive(Clone, Debug)]
enum Seg {
    Key(String),
    Index(usize),
}

/// Whether anything in `v`, a string, a key or (with `nums`) a number,
/// holds a secret.
fn any_hit(set: &SecretSet, nums: bool, v: &Value) -> bool {
    match v {
        Value::String(s) => set.scrub(s) != *s,
        Value::Array(a) => a.iter().any(|x| any_hit(set, nums, x)),
        Value::Object(m) => m
            .iter()
            .any(|(k, x)| set.scrub(k) != *k || any_hit(set, nums, x)),
        Value::Number(_) => number_hit(set, nums, v).is_some(),
        _ => false,
    }
}

/// What a number that holds a secret is replaced with: the number as
/// `serde_json` writes it, scrubbed. A secret made of digits (a PIN, a
/// numeric token) can be written as a number, where no string match sees
/// it. `nums`: whether the set holds a value that could be a number at
/// all ([`SecretSet::any_numeric`]); without one no number is looked at.
fn number_hit(set: &SecretSet, nums: bool, v: &Value) -> Option<String> {
    let Value::Number(n) = v else {
        return None;
    };
    if !nums {
        return None;
    }
    let text = n.to_string();
    let to = set.scrub(&text);
    (to != text).then_some(to)
}

fn at<'a>(v: &'a Value, path: &[Seg]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, seg| match seg {
        Seg::Key(k) => v.get(k.as_str()),
        Seg::Index(i) => v.get(*i),
    })
}

fn at_mut<'a>(v: &'a mut Value, path: &[Seg]) -> Option<&'a mut Value> {
    path.iter().try_fold(v, |v, seg| match seg {
        Seg::Key(k) => v.get_mut(k.as_str()),
        Seg::Index(i) => v.get_mut(*i),
    })
}

/// A copy to probe on: long strings shortened (no enum value, tag or id is
/// long; a typed `String` and free JSON take any), so the probes of a
/// large event cost what its structure costs.
fn skeleton(v: &Value) -> Value {
    match v {
        Value::String(s) if s.len() > 64 => Value::String("s".into()),
        Value::Array(a) => Value::Array(a.iter().map(skeleton).collect()),
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, x)| (k.clone(), skeleton(x))).collect())
        }
        other => other.clone(),
    }
}

/// The event's own types, for one kind: whether a payload is that kind
/// exactly as the typed event writes it (decodes, as that kind, and
/// encodes back to itself). Takes the payload and gives it back.
type Holds<'a> = &'a dyn Fn(Value) -> (bool, Value);

/// Whether a text position is kept, and what a kept-in-part one becomes.
enum Keep {
    Yes,
    No,
    As(String),
}

struct Schema<'a> {
    kind: &'a str,
    holds: Holds<'a>,
    /// `(path, original)` of a text position or a map key (`….{}`).
    keep: &'a dyn Fn(&str, &str) -> Keep,
}

/// How a probe of a position came out. Only called with a node that can
/// tell (a string, or a non-empty object or array): any other is `Free`,
/// which is scrubbed, never skipped (when in doubt, scrub).
fn probe(s: &Schema, sk: &Value, path: &[Seg]) -> Class {
    let with = |f: &dyn Fn(&mut Value)| {
        let mut c = sk.clone();
        if let Some(x) = at_mut(&mut c, path) {
            f(x);
        }
        (s.holds)(c).0
    };
    let node = at(sk, path);
    // A number where it stands: only free JSON takes it.
    if with(&|x| *x = Value::from(0)) {
        return Class::Free;
    }
    match node {
        Some(Value::String(_)) => {
            if with(&|x| *x = Value::String("\u{1}probe".into())) {
                Class::Text
            } else {
                Class::Fixed
            }
        }
        Some(Value::Object(m)) => match m.keys().next().cloned() {
            // A renamed key: a struct drops it, a map keeps it.
            Some(k) => {
                let renamed = with(&|x| {
                    if let Value::Object(m) = x
                        && let Some(v) = m.remove(&k)
                    {
                        m.insert("\u{1}probe".into(), v);
                    }
                });
                if renamed { Class::Map } else { Class::Struct }
            }
            // Nothing to rename: it cannot tell a struct from a map.
            None => Class::Free,
        },
        Some(Value::Array(_)) => Class::Seq,
        _ => Class::Free,
    }
}

/// Whether a node can tell its class, and the shape its class is kept
/// under: a string, an object or an array with something in it. A `null`,
/// `{}`, `[]`, number or bool has no text in it and no class to teach.
fn shape(v: &Value) -> Option<usize> {
    match v {
        Value::String(_) => Some(0),
        Value::Object(m) if !m.is_empty() => Some(1),
        Value::Array(a) if !a.is_empty() => Some(2),
        _ => None,
    }
}

/// The classes known, per kind, path and shape of the node (see [`shape`]),
/// for the life of the process.
type Cache = HashMap<String, HashMap<String, [Option<Class>; 3]>>;
/// How many numbers one pass over an event asks the types about
/// ([`Classifier::number_is_free`]); past it a number that holds a secret
/// is taken as free JSON and the check of the result decides.
const NUMBER_PROBES: usize = 256;
static CLASSES: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// The payload is not as the typed event writes it: no probe can be read.
struct NotWritten;

struct Classifier<'a, 'b> {
    s: &'a Schema<'b>,
    base: &'a Value,
    cache: &'a mut Cache,
    sk: Option<Value>,
    /// Numbers asked about in this pass ([`NUMBER_PROBES`]).
    number_probes: usize,
}

impl Classifier<'_, '_> {
    /// The class of `node`, at `path` of the payload: from the cache, or
    /// probed and cached for nodes of its shape.
    fn class(&mut self, node: &Value, path: &[Seg], g: &str) -> Result<Class, NotWritten> {
        let Some(shape) = shape(node) else {
            // Nothing to scrub in it, and nothing it can teach.
            return Ok(Class::Free);
        };
        if let Some(c) = self
            .cache
            .get(self.s.kind)
            .and_then(|m| m.get(g))
            .and_then(|l| l[shape])
        {
            return Ok(c);
        }
        let c = probe(self.s, self.skeleton()?, path);
        self.learn(g, shape, c);
        Ok(c)
    }

    /// The copy the probes are read against, made once per event.
    fn skeleton(&mut self) -> Result<&Value, NotWritten> {
        if self.sk.is_none() {
            // A probe is read against the payload as it is: only one the
            // typed event writes back unchanged can be read, or every
            // probe fails and a wrong class is learned for good.
            let sk = skeleton(self.base);
            if !(self.s.holds)(sk.clone()).0 {
                return Err(NotWritten);
            }
            self.sk = Some(sk);
        }
        Ok(self.sk.as_ref().expect("set above"))
    }

    fn learn(&mut self, g: &str, shape: usize, c: Class) {
        self.cache
            .entry(self.s.kind.to_string())
            .or_default()
            .entry(g.to_string())
            .or_default()[shape] = Some(c);
    }

    /// Whether the number at `path` stands in free JSON, where a string
    /// may take its place: probed with a string there (a typed number, an
    /// exit code or a count, does not take one). Asked of each such number
    /// on its own and never remembered: a general path can name a typed
    /// number in one element of a list and free JSON in the next, and an
    /// answer kept for the path would leave the free one unscrubbed for
    /// the life of the process. Only a number that holds a secret is
    /// asked about, so this is rare; past [`NUMBER_PROBES`] in one pass
    /// the answer is "free" (when in doubt, scrub) and the check of the
    /// result decides.
    fn number_is_free(&mut self, path: &[Seg]) -> Result<bool, NotWritten> {
        if self.number_probes >= NUMBER_PROBES {
            return Ok(true);
        }
        self.number_probes += 1;
        let s = self.s;
        let mut c = self.skeleton()?.clone();
        if let Some(x) = at_mut(&mut c, path) {
            *x = Value::String("\u{1}probe".into());
        }
        Ok((s.holds)(c).0)
    }
}

impl Classifier<'_, '_> {
    /// The class of the position at `path` and its general path; `Free`
    /// as soon as an ancestor is free JSON.
    fn resolve(&mut self, path: &[Seg]) -> Result<(Class, String), NotWritten> {
        let mut g = String::new();
        for i in 0..=path.len() {
            let node = at(self.base, &path[..i]).unwrap_or(&Value::Null);
            let c = self.class(node, &path[..i], &g)?;
            let Some(seg) = path.get(i) else {
                return Ok((c, g));
            };
            match (c, seg) {
                (Class::Struct, Seg::Key(k)) => push_key(&mut g, k),
                (Class::Map, Seg::Key(_)) => g.push_str(".{}"),
                (Class::Seq, Seg::Index(_)) => g.push_str("[]"),
                // Free JSON, or a shape the class does not fit.
                _ => return Ok((Class::Free, g)),
            }
        }
        unreachable!("the loop returns at the path's end")
    }
}

fn push_key(g: &mut String, k: &str) {
    if !g.is_empty() {
        g.push('.');
    }
    g.push_str(k);
}

/// Rename keys of `m` (`(old, new)`), each in its place, a name already
/// there numbered (` (2)`), so no entry is lost.
fn rename_keys(m: &mut Map<String, Value>, renames: &[(String, String)]) {
    if renames.is_empty() {
        return;
    }
    let to: HashMap<&str, &str> = renames
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let old = std::mem::take(m);
    let mut taken: HashSet<String> = old
        .keys()
        .filter(|k| !to.contains_key(k.as_str()))
        .cloned()
        .collect();
    for (k, x) in old {
        let name = match to.get(k.as_str()) {
            None => k,
            Some(want) => {
                let mut name = want.to_string();
                let mut n = 2;
                while taken.contains(&name) {
                    name = format!("{want} ({n})");
                    n += 1;
                }
                taken.insert(name.clone());
                name
            }
        };
        m.insert(name, x);
    }
}

/// Free JSON scrubbed: every string and key, and (with `nums`) a number
/// that holds a secret, which becomes its scrubbed text. Whether anything
/// changed.
fn scrub_free(set: &SecretSet, nums: bool, v: &mut Value) -> bool {
    match v {
        Value::Number(_) => match number_hit(set, nums, v) {
            Some(to) => {
                *v = Value::String(to);
                true
            }
            None => false,
        },
        Value::String(s) => {
            let c = set.scrub(s);
            let changed = c != *s;
            if changed {
                *s = c;
            }
            changed
        }
        Value::Array(a) => a
            .iter_mut()
            .fold(false, |ch, x| scrub_free(set, nums, x) | ch),
        Value::Object(m) => {
            let mut changed = m
                .values_mut()
                .fold(false, |ch, x| scrub_free(set, nums, x) | ch);
            let renames: Vec<(String, String)> = m
                .keys()
                .filter_map(|k| {
                    let c = set.scrub(k);
                    (c != *k).then(|| (k.clone(), c))
                })
                .collect();
            changed |= !renames.is_empty();
            rename_keys(m, &renames);
            changed
        }
        _ => false,
    }
}

/// One pass over a copy of the payload: each typed position by its class
/// (learned once per path), free JSON scrubbed whole. `allow`: only the
/// paths it names are changed (the fallback); `touched`: the paths changed.
struct Walk<'a, 'b, 'c> {
    set: &'a SecretSet,
    /// Whether a secret could be a number ([`number_hit`]).
    nums: bool,
    cls: Classifier<'a, 'b>,
    allow: Option<&'c [String]>,
    touched: Vec<String>,
}

impl Walk<'_, '_, '_> {
    fn may(&mut self, g: &str) -> bool {
        let ok = self.allow.is_none_or(|a| a.iter().any(|x| x == g));
        if ok && !self.touched.iter().any(|x| x == g) {
            self.touched.push(g.to_string());
        }
        ok
    }

    fn walk(
        &mut self,
        v: &mut Value,
        path: &mut Vec<Seg>,
        g: &mut String,
    ) -> Result<(), NotWritten> {
        if v.is_number() {
            // A number that holds a secret, where the types take a string
            // (free JSON): its scrubbed text. A typed number stays: it
            // cannot hold the marker.
            if let Some(to) = number_hit(self.set, self.nums, v)
                && self.cls.number_is_free(path)?
                && self.may(g)
            {
                *v = Value::String(to);
            }
            return Ok(());
        }
        let class = self.cls.class(v, path, g)?;
        // A class that does not fit the node (none is learned from another
        // shape, so none is expected): scrubbed as free text, never skipped.
        let class = match (class, &*v) {
            (Class::Text | Class::Fixed, Value::String(_))
            | (Class::Seq, Value::Array(_))
            | (Class::Struct | Class::Map, Value::Object(_)) => class,
            _ => Class::Free,
        };
        match class {
            Class::Free => match self.allow {
                // The first pass: one scan, as the old scrubber did.
                None => {
                    if scrub_free(self.set, self.nums, v) {
                        self.may(g);
                    }
                }
                Some(_) => {
                    if self.may(g) {
                        scrub_free(self.set, self.nums, v);
                    }
                }
            },
            Class::Text => {
                if let Value::String(x) = v {
                    let to = self.set.scrub(x);
                    if to != *x {
                        let to = match (self.cls.s.keep)(g, x) {
                            Keep::Yes => None,
                            Keep::No => Some(to),
                            Keep::As(t) => (t != *x).then_some(t),
                        };
                        if let Some(to) = to
                            && self.may(g)
                        {
                            *x = to;
                        }
                    }
                }
            }
            Class::Fixed => {}
            Class::Seq => {
                if let Value::Array(a) = v {
                    let n = g.len();
                    g.push_str("[]");
                    for (i, x) in a.iter_mut().enumerate() {
                        path.push(Seg::Index(i));
                        self.walk(x, path, g)?;
                        path.pop();
                    }
                    g.truncate(n);
                }
            }
            Class::Struct => {
                if let Value::Object(m) = v {
                    for (k, x) in m.iter_mut() {
                        let n = g.len();
                        push_key(g, k);
                        path.push(Seg::Key(k.clone()));
                        self.walk(x, path, g)?;
                        path.pop();
                        g.truncate(n);
                    }
                }
            }
            Class::Map => {
                if let Value::Object(m) = v {
                    let n = g.len();
                    g.push_str(".{}");
                    for (k, x) in m.iter_mut() {
                        path.push(Seg::Key(k.clone()));
                        self.walk(x, path, g)?;
                        path.pop();
                    }
                    // Keys after the values: the paths above are the
                    // original keys, as the probes know them.
                    let mut renames = vec![];
                    for k in m.keys() {
                        let to = self.set.scrub(k);
                        if to == *k {
                            continue;
                        }
                        let to = match (self.cls.s.keep)(g, k) {
                            Keep::Yes => continue,
                            Keep::No => to,
                            Keep::As(t) if t != *k => t,
                            Keep::As(_) => continue,
                        };
                        renames.push((k.clone(), to));
                    }
                    if !renames.is_empty() && self.may(g) {
                        rename_keys(m, &renames);
                    }
                    g.truncate(n);
                }
            }
        }
        Ok(())
    }
}

/// Scrub `payload` of the kind `s` describes (module docs).
fn run(
    set: &SecretSet,
    s: &Schema,
    payload: &mut Value,
    cache: &mut Cache,
) -> Result<Outcome, NotWritten> {
    let nums = set.any_numeric();
    let pass = |allow: Option<&[String]>, cache: &mut Cache| {
        let mut out = payload.clone();
        let mut w = Walk {
            set,
            nums,
            cls: Classifier {
                s,
                base: payload,
                cache,
                sk: None,
                number_probes: 0,
            },
            allow,
            touched: vec![],
        };
        w.walk(&mut out, &mut vec![], &mut String::new())?;
        let touched = w.touched;
        let (ok, out) = (s.holds)(out);
        Ok::<_, NotWritten>((ok, out, touched))
    };
    let (ok, out, touched) = pass(None, cache)?;
    if touched.is_empty() {
        return Ok(Outcome::Clean);
    }
    if ok {
        *payload = out;
        return Ok(Outcome::Scrubbed);
    }
    // One path at a time, each checked: at most as many checks as the
    // event has paths changed, whatever the number of changes.
    let mut kept: Vec<String> = vec![];
    let mut best = None;
    for g in &touched {
        kept.push(g.clone());
        let (ok, out, _) = pass(Some(&kept), cache)?;
        if ok {
            best = Some(out);
        } else {
            kept.pop();
        }
    }
    if let Some(out) = best {
        *payload = out;
    }
    Ok(Outcome::Fallback {
        left: touched.len() - kept.len(),
    })
}

/// Every string value scrubbed, no key touched.
fn values_only(set: &SecretSet, v: &mut Value) {
    match v {
        Value::String(s) => {
            let c = set.scrub(s);
            if c != *s {
                *s = c;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| values_only(set, x)),
        Value::Object(m) => m.values_mut().for_each(|x| values_only(set, x)),
        _ => {}
    }
}

fn holds_event(kind: &str, payload: Value) -> (bool, Value) {
    let mut w = Map::new();
    w.insert("kind".into(), Value::String(kind.into()));
    w.insert("payload".into(), payload);
    let w = Value::Object(w);
    let ok = EventKind::deserialize(&w).is_ok_and(|e| {
        serde_json::to_value(&e).is_ok_and(|back| {
            back.get("kind") == w.get("kind") && back.get("payload") == w.get("payload")
        })
    });
    let Value::Object(mut w) = w else {
        unreachable!("built as an object")
    };
    (ok, w.remove("payload").unwrap_or(Value::Null))
}

fn known(kind: &str) -> bool {
    KEPT.iter().any(|(k, _)| *k == kind)
}

fn keep_of<'a>(set: &'a SecretSet, kind: &'a str) -> impl Fn(&str, &str) -> Keep + 'a {
    move |g: &str, orig: &str| {
        if kind == "context_sent" && g == "sections[].key" {
            return Keep::As(section_key(set, orig));
        }
        let kept = KEPT
            .iter()
            .find(|(k, _)| *k == kind)
            .is_some_and(|(_, paths)| paths.contains(&g));
        if kept { Keep::Yes } else { Keep::No }
    }
}

impl SecretSet {
    /// Scrub the payload of an event of `kind` in place, its free text only
    /// (module docs): whatever the set holds, the result decodes as the same
    /// kind, with every field name, tag, enum value and [`KEPT`] position as
    /// it was. A payload with nothing to scrub is left untouched, byte for
    /// byte. The first event of a kind to meet a path probes it (a few
    /// decodes of a copy with long strings cut short); after that a scrub is
    /// one pass, the lookups and one decode.
    pub fn scrub_event(&self, kind: &str, payload: &mut Value) -> Outcome {
        if self.is_empty() || !any_hit(self, self.any_numeric(), payload) {
            return Outcome::Clean;
        }
        let keep = keep_of(self, kind);
        let holds = |v: Value| holds_event(kind, v);
        let s = Schema {
            kind,
            holds: &holds,
            keep: &keep,
        };
        let mut cache = CLASSES.lock().unwrap_or_else(|p| p.into_inner());
        if let Ok(out) = run(self, &s, payload, &mut cache) {
            return out;
        }
        // Not as the typed event writes it (never from `append`): stored
        // as it does write it, so the probes compare alike; a kind this
        // build does not know has its values scrubbed, never its keys.
        match EventKind::from_parts(kind, payload.clone()) {
            Ok(e) => {
                *payload = e.into_parts().1;
                run(self, &s, payload, &mut cache).unwrap_or(Outcome::Unknown)
            }
            Err(_) => {
                debug_assert!(!known(kind), "a {kind} that does not decode");
                values_only(self, payload);
                Outcome::Unknown
            }
        }
    }
}

/// Every string and non-empty object or array position of `payload` (below
/// free JSON, only its root) with its general path and class, probed afresh, nothing
/// cached: for the tests that compare classes across instances and the
/// kept positions with [`KEPT`].
#[doc(hidden)]
pub fn positions(kind: &str, payload: &Value) -> Vec<(String, Class)> {
    let holds = |v: Value| holds_event(kind, v);
    let keep = |_: &str, _: &str| Keep::No;
    let s = Schema {
        kind,
        holds: &holds,
        keep: &keep,
    };
    let mut paths = vec![];
    fn walk(v: &Value, path: &mut Vec<Seg>, out: &mut Vec<Vec<Seg>>) {
        match v {
            Value::String(_) => out.push(path.clone()),
            Value::Array(a) => {
                // An empty one tells nothing (see `shape`).
                if !a.is_empty() {
                    out.push(path.clone());
                }
                for (i, x) in a.iter().enumerate() {
                    path.push(Seg::Index(i));
                    walk(x, path, out);
                    path.pop();
                }
            }
            Value::Object(m) => {
                if !m.is_empty() {
                    out.push(path.clone());
                }
                for (k, x) in m {
                    path.push(Seg::Key(k.clone()));
                    walk(x, path, out);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    walk(payload, &mut vec![], &mut paths);
    let mut out: Vec<(String, Class)> = vec![];
    for p in paths {
        // Fresh per position: each element is judged by itself.
        let mut cache = Cache::new();
        let mut c = Classifier {
            s: &s,
            base: payload,
            cache: &mut cache,
            sk: None,
            number_probes: 0,
        };
        let Ok((class, g)) = c.resolve(&p) else {
            return vec![];
        };
        // Below free JSON only its root counts.
        if class == Class::Free && out.iter().any(|(x, c)| *c == Class::Free && *x == g) {
            continue;
        }
        out.push((g, class));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Secret;
    use serde::Serialize;

    /// A schema the event schema does not have: one path that is text in
    /// one element and an enum value in another (an internally tagged
    /// enum). The class learned from the first element is wrong for the
    /// second, the final check sees the result does not decode, and the
    /// one-path-at-a-time fallback keeps what decodes.
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct S {
        xs: Vec<Y>,
        note: String,
    }
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    #[serde(tag = "t", rename_all = "snake_case")]
    enum Y {
        A { v: String },
        B { v: Color },
    }
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    #[serde(rename_all = "snake_case")]
    enum Color {
        Longcolor,
    }

    fn holds_s(v: Value) -> (bool, Value) {
        let ok =
            S::deserialize(&v).is_ok_and(|x| serde_json::to_value(&x).is_ok_and(|back| back == v));
        (ok, v)
    }

    #[test]
    fn a_path_of_two_classes_falls_back_and_still_decodes() {
        let set = SecretSet::new(vec![Secret::new("longcolor".into())]);
        let mut v = serde_json::to_value(S {
            xs: vec![
                Y::A {
                    v: "a longcolor sky".into(),
                },
                Y::B {
                    v: Color::Longcolor,
                },
            ],
            note: "longcolor here".into(),
        })
        .unwrap();
        let keep = |_: &str, _: &str| Keep::No;
        let s = Schema {
            kind: "synthetic",
            holds: &holds_s,
            keep: &keep,
        };
        let out = run(&set, &s, &mut v, &mut Cache::new()).ok();
        assert_eq!(out, Some(Outcome::Fallback { left: 1 }));
        // It decodes; the path that decodes is scrubbed, the other kept.
        let back: S = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(back.note, "[REDACTED] here");
        assert_eq!(
            back.xs[1],
            Y::B {
                v: Color::Longcolor
            }
        );
    }

    /// `Option<String>` written as `null`, and a list that can be empty.
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct T {
        name: String,
        packed: Option<String>,
        tags: Vec<String>,
    }

    fn holds_t(v: Value) -> (bool, Value) {
        let ok =
            T::deserialize(&v).is_ok_and(|x| serde_json::to_value(&x).is_ok_and(|back| back == v));
        (ok, v)
    }

    fn run_t(set: &SecretSet, v: &mut Value, cache: &mut Cache) -> Outcome {
        let keep = |_: &str, _: &str| Keep::No;
        let s = Schema {
            kind: "t",
            holds: &holds_t,
            keep: &keep,
        };
        run(set, &s, v, cache).ok().expect("written as typed")
    }

    #[test]
    fn a_node_that_cannot_tell_teaches_no_class() {
        let set = SecretSet::new(vec![Secret::new("longsecret".into())]);
        let mut cache = Cache::new();
        let mut first = serde_json::json!({ "name": "a longsecret", "packed": null, "tags": [] });
        run_t(&set, &mut first, &mut cache);
        let known = &cache["t"];
        assert!(known.contains_key("name"));
        assert!(!known.contains_key("packed"), "{known:?}");
        assert!(!known.contains_key("tags"), "{known:?}");
        let mut later =
            serde_json::json!({ "name": "x", "packed": "a longsecret", "tags": ["longsecret"] });
        run_t(&set, &mut later, &mut cache);
        assert_eq!(
            later,
            serde_json::json!({ "name": "x", "packed": "a [REDACTED]", "tags": ["[REDACTED]"] })
        );
    }

    /// When in doubt, scrub: a class that does not fit the node (the cache
    /// never holds one: it is kept per shape; this plants one) is not
    /// taken as "nothing here".
    #[test]
    fn a_class_that_does_not_fit_the_node_scrubs_it_as_text() {
        let set = SecretSet::new(vec![Secret::new("longsecret".into())]);
        for wrong in [Class::Seq, Class::Struct, Class::Map] {
            let mut cache = Cache::new();
            cache
                .entry("t".into())
                .or_default()
                .insert("packed".into(), [Some(wrong), None, None]);
            let mut v = serde_json::json!({ "name": "x", "packed": "a longsecret", "tags": [] });
            run_t(&set, &mut v, &mut cache);
            assert_eq!(v["packed"], "a [REDACTED]", "{wrong:?}");
        }
    }

    /// A path that is a bare string in one variant and an object in
    /// another (the event schema has none). The class is kept per shape:
    /// the string's class is not the object's, so the object is taken as
    /// the struct it is, its field name left alone and its text scrubbed.
    /// Under one class per path the object would be scrubbed as free
    /// JSON, the field name with it, and the result would not decode.
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct U {
        w: W,
    }
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    #[serde(untagged)]
    enum W {
        Bare(String),
        Obj { longfield: String },
    }

    fn holds_u(v: Value) -> (bool, Value) {
        let ok =
            U::deserialize(&v).is_ok_and(|x| serde_json::to_value(&x).is_ok_and(|back| back == v));
        (ok, v)
    }

    #[test]
    fn a_string_and_an_object_at_one_path_each_have_their_own_class() {
        let set = SecretSet::new(vec![Secret::new("longfield".into())]);
        let keep = |_: &str, _: &str| Keep::No;
        let s = Schema {
            kind: "u",
            holds: &holds_u,
            keep: &keep,
        };
        for bare_first in [true, false] {
            let mut cache = Cache::new();
            let mut bare = serde_json::json!({ "w": "a longfield" });
            let mut obj = serde_json::json!({ "w": { "longfield": "a longfield" } });
            let order: [&mut Value; 2] = if bare_first {
                [&mut bare, &mut obj]
            } else {
                [&mut obj, &mut bare]
            };
            for v in order {
                let out = run(&set, &s, v, &mut cache).ok();
                assert_eq!(
                    out,
                    Some(Outcome::Scrubbed),
                    "bare first: {bare_first}: {v}"
                );
            }
            assert_eq!(bare, serde_json::json!({ "w": "a [REDACTED]" }));
            assert_eq!(
                obj,
                serde_json::json!({ "w": { "longfield": "a [REDACTED]" } })
            );
        }
    }

    /// One general path, `xs[].n`, that is free JSON in one element and a
    /// typed number in the next (the event schema has none). Each number
    /// is asked about on its own: the free one is scrubbed and the typed
    /// one stays, whichever the process met first. An answer remembered
    /// for the path left the free one unscrubbed for good, with no
    /// fallback to log it, when the typed one came first.
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct N {
        xs: Vec<Z>,
    }
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    #[serde(tag = "t", rename_all = "snake_case")]
    enum Z {
        A { n: Value },
        B { n: i64 },
    }

    fn holds_n(v: Value) -> (bool, Value) {
        let ok =
            N::deserialize(&v).is_ok_and(|x| serde_json::to_value(&x).is_ok_and(|back| back == v));
        (ok, v)
    }

    #[test]
    fn a_number_is_asked_about_on_its_own_not_by_its_path() {
        let set = SecretSet::new(vec![Secret::new("12345678901".into())]);
        let keep = |_: &str, _: &str| Keep::No;
        let s = Schema {
            kind: "n",
            holds: &holds_n,
            keep: &keep,
        };
        let a = serde_json::json!({ "t": "a", "n": 12345678901u64 });
        let b = serde_json::json!({ "t": "b", "n": 12345678901u64 });
        let a_done = serde_json::json!({ "t": "a", "n": "[REDACTED]" });
        for order in [[&a, &b], [&b, &a]] {
            let mut cache = Cache::new();
            // Twice: what the first event taught must not change the second.
            for _ in 0..2 {
                let mut v = serde_json::json!({ "xs": order });
                let out = run(&set, &s, &mut v, &mut cache).ok();
                assert_eq!(out, Some(Outcome::Scrubbed), "{v}");
                let want: Vec<&Value> = order
                    .iter()
                    .map(|x| if *x == &a { &a_done } else { &b })
                    .collect();
                assert_eq!(v, serde_json::json!({ "xs": want }));
            }
            // The typed one alone, then the free one alone.
            let mut cache = Cache::new();
            let mut only_b = serde_json::json!({ "xs": [b] });
            assert_eq!(
                run(&set, &s, &mut only_b, &mut cache).ok(),
                Some(Outcome::Clean)
            );
            let mut only_a = serde_json::json!({ "xs": [a] });
            assert_eq!(
                run(&set, &s, &mut only_a, &mut cache).ok(),
                Some(Outcome::Scrubbed)
            );
            assert_eq!(only_a, serde_json::json!({ "xs": [a_done] }));
        }
    }

    #[test]
    fn colliding_keys_are_numbered_not_lost() {
        let set = SecretSet::new(vec![
            Secret::new("aaaaaaaaKEY1".into()),
            Secret::new("bbbbbbbbKEY2".into()),
        ]);
        let mut v = serde_json::json!({
            "z": 1, "aaaaaaaaKEY1": 2, "[REDACTED]": 3, "bbbbbbbbKEY2": 4
        });
        let free = |v: Value| (true, v);
        let keep = |_: &str, _: &str| Keep::No;
        let s = Schema {
            kind: "free",
            holds: &free,
            keep: &keep,
        };
        assert!(run(&set, &s, &mut v, &mut Cache::new()).is_ok());
        let m = v.as_object().unwrap();
        assert_eq!(m.len(), 4, "{v}");
        let mut vals: Vec<i64> = m.values().map(|x| x.as_i64().unwrap()).collect();
        vals.sort();
        assert_eq!(vals, [1, 2, 3, 4]);
        assert_eq!(m["[REDACTED]"], 3, "a key already there keeps its name");
    }
}

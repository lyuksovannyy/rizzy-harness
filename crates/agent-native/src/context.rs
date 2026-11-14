//! The harness's context block of a turn (X.1): what is sent with the
//! turn's own message besides the person's words, kept so the provider's
//! prompt cache holds across turns.
//!
//! The block is made of sections. Standing ones (the repository's
//! instruction files, the list of skills, the master's memory and project
//! parts) are sent when they differ from what the model was last sent of
//! them since the last summary, with a line of the harness's saying which
//! replace an earlier one or no longer apply; one that has not changed is
//! not sent again (the model has it in its history). This turn's own
//! sections (the master's settings line and notes, a `/skill` line, MCP
//! notes) are sent whenever they are given. The block goes into the turn's
//! opening message and is recorded as it was sent (`context_sent`), so
//! every later request rebuilds that message byte for byte from the log.
//! After a summary the window holds no block, so everything standing is
//! sent again: the instruction files survive compaction. Codex diffs its
//! "world state" sections the same way (a replacement line for a changed
//! AGENTS.md, a removal notice, everything again in a fresh window) and
//! OpenCode freezes the rendered update text in a durable event
//! (research/round2/synth/07-context-compaction-memory.md §2).

use std::collections::{HashMap, HashSet};

use harness_core::RawEvent;
use harness_core::backend::{StandingKey, TurnExtra};
use harness_core::event::{EventKind, SectionMark};
use sha2::{Digest, Sha256};

use crate::provider::{self, Whose};

/// The most of one section that is sent and recorded, in bytes. Every
/// section has a smaller cap where it is made (instruction files 32 KiB,
/// the list of skills 8,000, memory 3,000 and recall); this one keeps a
/// `context_sent` far below the log's 4 MiB event limit whatever a master
/// sends, so a large project brief can never make the record fail and
/// wedge the chat.
pub const SECTION_CAP: usize = 256 * 1024;

pub const INSTRUCTIONS: &str = "instructions";
pub const SKILLS: &str = "skills";
pub const MEMORY: &str = "memory";
pub const PROJECT: &str = "project";

/// A standing section's key in the log.
fn key_of(k: StandingKey) -> &'static str {
    match k {
        StandingKey::Memory => MEMORY,
        StandingKey::Project => PROJECT,
    }
}

/// The harness's sentence for a section sent again because it changed.
fn replaced(key: &str) -> &'static str {
    match key {
        INSTRUCTIONS => {
            "The repository's instruction files below replace the ones sent earlier in this \
             conversation."
        }
        SKILLS => "The list of skills below replaces the one sent earlier in this conversation.",
        MEMORY => {
            "What you remember about the person, below, replaces what was sent of it earlier in \
             this conversation."
        }
        _ => {
            "The project's brief, facts and board below replace the ones sent earlier in this \
             conversation."
        }
    }
}

/// The harness's sentence for a section sent earlier that is gone now.
fn withdrawn(key: &str) -> &'static str {
    match key {
        INSTRUCTIONS => {
            "The repository's instruction files sent earlier in this conversation no longer \
             apply: none were found for this turn."
        }
        SKILLS => {
            "The list of skills sent earlier in this conversation no longer applies: no skills \
             are offered now."
        }
        MEMORY => {
            "What was sent earlier in this conversation of what you remember about the person \
             no longer applies: memory is not given now."
        }
        _ => {
            "The project's brief, facts and board sent earlier in this conversation no longer \
             apply."
        }
    }
}

struct Standing {
    key: &'static str,
    text: Option<String>,
    /// Whether its absence means it is gone. A turn the master gave no
    /// context (a callback turn, one that reads queued messages) says
    /// nothing about memory or the project.
    known: bool,
}

/// A turn's sections, scrubbed and capped as they are sent and recorded.
pub struct Sections {
    standing: Vec<Standing>,
    per_turn: Vec<String>,
}

impl Sections {
    /// The sections of a turn with `extra` and the MCP notes to tell.
    /// Every text passes `secrets` first, as tool output does: the node
    /// scrubs the record anyway, and the message must be what it records.
    pub fn of(extra: &TurnExtra, mcp_notes: &[String], secrets: &harness_core::SecretSet) -> Self {
        let prep = |t: Option<&str>| {
            t.filter(|t| !t.is_empty())
                .map(|t| capped(secrets.scrub(t)))
        };
        let mut standing = vec![
            Standing {
                key: INSTRUCTIONS,
                text: prep(extra.instructions.as_deref()),
                known: true,
            },
            Standing {
                key: SKILLS,
                text: prep(extra.skills_list.as_deref()),
                known: true,
            },
        ];
        for k in [StandingKey::Memory, StandingKey::Project] {
            let text = extra.standing.as_ref().and_then(|parts| {
                // One of each; a second of a kind would be a master's bug.
                prep(parts.iter().find(|p| p.part == k).map(|p| p.text.as_str()))
            });
            standing.push(Standing {
                key: key_of(k),
                text,
                known: extra.standing.is_some(),
            });
        }
        // The master's own line last: a routine's untrusted note is what
        // the model reads right before the message.
        let notes = (!mcp_notes.is_empty()).then(|| mcp_notes.join("\n"));
        let per_turn = [
            extra.skill_asked.as_deref(),
            notes.as_deref(),
            extra.context.as_deref(),
        ]
        .into_iter()
        .filter_map(prep)
        .collect();
        Self { standing, per_turn }
    }
}

/// `t` cut to [`SECTION_CAP`] bytes on a character boundary, with a note.
fn capped(mut t: String) -> String {
    if t.len() <= SECTION_CAP {
        return t;
    }
    let mut end = SECTION_CAP;
    while !t.is_char_boundary(end) {
        end -= 1;
    }
    t.truncate(end);
    t.push_str(&format!(
        "\n[cut: the harness sends at most {SECTION_CAP} bytes of one section]"
    ));
    t
}

/// SHA-256 of `text`, hex.
pub fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What the model's history holds of each standing section in `window`
/// (the events after the last summary): the digest of the last mark of
/// each key, empty for one withdrawn. Only a turn whose start is in the
/// window counts, and of a turn only its last record (the one its opening
/// is rebuilt with).
pub fn had(window: &[RawEvent]) -> HashMap<String, String> {
    let mut started = HashSet::new();
    let mut last: HashMap<String, usize> = HashMap::new();
    let mut records = Vec::new();
    for e in window {
        match e.typed() {
            Ok(EventKind::TurnStarted(t)) => {
                started.insert(t.turn_id);
            }
            Ok(EventKind::ContextSent(c)) if started.contains(&c.turn_id) => {
                last.insert(c.turn_id.clone(), records.len());
                records.push(c);
            }
            _ => {}
        }
    }
    let mut out = HashMap::new();
    for (i, c) in records.into_iter().enumerate() {
        if last.get(&c.turn_id) == Some(&i) {
            for m in c.sections {
                out.insert(m.key, m.digest);
            }
        }
    }
    out
}

/// What a turn sends: its block and the marks its record carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub text: String,
    pub marks: Vec<SectionMark>,
}

/// The block for `sections` given what the model `had`; `None` when there
/// is nothing to send.
pub fn plan(sections: &Sections, had: &HashMap<String, String>, whose: Whose) -> Option<Plan> {
    let mut body: Vec<&str> = Vec::new();
    let mut notes: Vec<&str> = Vec::new();
    let mut marks = Vec::new();
    for s in &sections.standing {
        let before = had.get(s.key).filter(|d| !d.is_empty());
        match &s.text {
            Some(t) => {
                let d = digest(t);
                if before == Some(&d) {
                    continue;
                }
                if before.is_some() {
                    notes.push(replaced(s.key));
                }
                body.push(t);
                marks.push(SectionMark {
                    key: s.key.into(),
                    digest: d,
                });
            }
            None if s.known && before.is_some() => {
                notes.push(withdrawn(s.key));
                marks.push(SectionMark {
                    key: s.key.into(),
                    digest: String::new(),
                });
            }
            None => {}
        }
    }
    body.extend(sections.per_turn.iter().map(String::as_str));
    let text = provider::block(&body.join("\n\n"), &notes, whose)?;
    Some(Plan { text, marks })
}

/// A person's or a sub-agent's opening text: the environment line, the
/// block, then `text` (the loop and the rebuild both build it so).
pub fn opening(environment: Option<&str>, block: Option<&str>, text: &str) -> String {
    let t = match block {
        Some(b) => format!("{b}\n\n{text}"),
        None => text.to_string(),
    };
    match environment.filter(|l| !l.is_empty()) {
        Some(l) => format!("{l}\n\n{t}"),
        None => t,
    }
}

/// A callback turn's opening (`provider::open_callback`'s) with the block
/// after it and the environment line last.
pub fn after_callback(opening: &mut String, block: Option<&str>, environment: Option<&str>) {
    for part in [block, environment.filter(|l| !l.is_empty())]
        .into_iter()
        .flatten()
    {
        opening.push_str("\n\n");
        opening.push_str(part);
    }
}

/// The person's own words in an earlier turn's message as the model reads
/// it (URL provenance, [`harness_core::event::own_words`]): after the
/// node's environment line and the harness's context block in front of
/// it, which the log puts back since X.1 and PR 96. Both are one paragraph
/// each (no empty line inside), so each ends at the first empty line. What
/// is left is the person's message as `own_words` reads any: their text up
/// to the first line of the harness's (a file, an image's name). A person
/// who types either lead first is read from after that paragraph: only
/// their own text can count, never the harness's or a file's.
pub fn earlier_own_words(text: &str) -> &str {
    let mut t = text;
    for lead in [
        harness_core::backend::ENVIRONMENT_LEAD,
        provider::CONTEXT_LEAD,
    ] {
        if t.starts_with(lead) {
            t = t.split_once("\n\n").map_or("", |(_, rest)| rest);
        }
    }
    harness_core::event::own_words(t)
}

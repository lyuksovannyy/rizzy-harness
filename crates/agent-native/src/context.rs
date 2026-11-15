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
//! notes, a note that a standing section could not be read this turn) are
//! sent whenever they are given. The block goes into the turn's opening
//! message and is recorded section by section (`context_sent`), so every
//! later request rebuilds that message from the log.
//!
//! The history holds at most the current version of each standing
//! section (T52): when a turn sends a standing section again or withdraws
//! it, the earlier openings are rebuilt without their copy of it. So an
//! agent whose memory is off reads none of the person's facts, and a
//! forgotten fact leaves the next request; that turn pays one cache write
//! from the earliest opening that held the section, and every request
//! after it rebuilds byte for byte again. After a summary the window holds
//! no block, so everything standing is sent again: the instruction files
//! survive compaction. Codex diffs its "world state" sections the same way
//! (a replacement line for a changed AGENTS.md, a removal notice,
//! everything again in a fresh window) and OpenCode records the update in
//! a durable event (research/round2/synth/07-context-compaction-memory.md
//! §2).

use std::collections::{HashMap, HashSet};

use harness_core::RawEvent;
use harness_core::backend::{StandingKey, TurnExtra};
use harness_core::event::{ContextFrame, ContextSent, EventKind, OUTPUT_QUOTE, SentSection};
use sha2::{Digest, Sha256};

use crate::provider::{self, Whose};

/// The most of one section that is sent and recorded, in bytes. Every
/// section has a smaller cap where it is made (instruction files 32 KiB,
/// the list of skills 8,000, memory 3,000 and recall); this one keeps a
/// `context_sent` far below the log's 4 MiB event limit whatever a master
/// sends, so a large project brief can never make the record fail and
/// wedge the chat. The MCP notes of a turn are one section and share it.
pub const SECTION_CAP: usize = 256 * 1024;

/// The most of the rendered block one message carries, in bytes: quoting
/// can make a section longer than it was (a control character becomes
/// three bytes, every line gains `| `).
pub const BLOCK_CAP: usize = 1024 * 1024;

/// The key of a section that is this turn's own.
pub const TURN: &str = "turn";

/// A standing section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKey {
    Instructions,
    Skills,
    /// The chat owner's own facts.
    Memory,
    /// The workspace's memory (private details, or a team's shared one).
    Workspace,
    /// What `remember` does in this chat.
    Remember,
    /// A project's brief.
    Project,
    /// A project's facts.
    ProjectMemory,
    /// A project's board.
    Board,
}

impl SectionKey {
    pub const ALL: [SectionKey; 8] = [
        SectionKey::Instructions,
        SectionKey::Skills,
        SectionKey::Memory,
        SectionKey::Workspace,
        SectionKey::Remember,
        SectionKey::Project,
        SectionKey::ProjectMemory,
        SectionKey::Board,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SectionKey::Instructions => "instructions",
            SectionKey::Skills => "skills",
            SectionKey::Memory => "memory",
            SectionKey::Workspace => "workspace",
            SectionKey::Remember => "remember",
            SectionKey::Project => "project",
            SectionKey::ProjectMemory => "project_memory",
            SectionKey::Board => "board",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    fn of(k: StandingKey) -> Self {
        match k {
            StandingKey::Memory => SectionKey::Memory,
            StandingKey::Workspace => SectionKey::Workspace,
            StandingKey::Remember => SectionKey::Remember,
            StandingKey::Project => SectionKey::Project,
            StandingKey::ProjectMemory => SectionKey::ProjectMemory,
            StandingKey::Board => SectionKey::Board,
        }
    }

    /// What the section is, for the harness's sentences.
    fn what(self) -> (&'static str, bool) {
        // (noun phrase, plural)
        match self {
            SectionKey::Instructions => ("The repository's instruction files", true),
            SectionKey::Skills => ("The list of skills", false),
            SectionKey::Memory => ("What you remember about the person", false),
            SectionKey::Workspace => ("The workspace's memory", false),
            SectionKey::Remember => ("What `remember` does in this chat", false),
            SectionKey::Project => ("The project's brief", false),
            SectionKey::ProjectMemory => ("The project's facts", true),
            SectionKey::Board => ("The project's board", false),
        }
    }

    /// The harness's sentence for a section sent again because it changed.
    fn replaced(self) -> String {
        let (what, many) = self.what();
        let (verb, one) = if many {
            ("replace", "ones")
        } else {
            ("replaces", "one")
        };
        format!("{what} below {verb} the {one} sent earlier in this conversation.")
    }

    /// The harness's sentence for a section sent earlier that is gone now.
    fn withdrawn(self) -> String {
        let (what, many) = self.what();
        let verb = if many { "apply" } else { "applies" };
        let why = match self {
            SectionKey::Instructions => ": none were found for this turn",
            SectionKey::Skills => ": no skills are offered now",
            SectionKey::Memory | SectionKey::Workspace => ": memory is not given now",
            SectionKey::Remember => ": `remember` is not offered now",
            SectionKey::Project | SectionKey::ProjectMemory | SectionKey::Board => "",
        };
        format!("{what} sent earlier in this conversation no longer {verb}{why}.")
    }
}

struct Standing {
    key: SectionKey,
    text: Option<String>,
    /// Whether its absence means it is gone. A turn the master gave no
    /// context (a callback turn, one that reads queued messages) says
    /// nothing about memory or the project; a read that failed or timed
    /// out says nothing about the files or the skills.
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
        let read = |text: &Option<String>, unread: &Option<String>| Standing {
            key: SectionKey::Instructions,
            text: if unread.is_some() {
                None
            } else {
                prep(text.as_deref())
            },
            known: unread.is_none(),
        };
        let mut standing = vec![
            read(&extra.instructions, &extra.instructions_unread),
            Standing {
                key: SectionKey::Skills,
                ..read(&extra.skills_list, &extra.skills_unread)
            },
        ];
        for k in StandingKey::ALL {
            let text = extra.standing.as_ref().and_then(|parts| {
                // One of each; a second of a kind would be a master's bug.
                prep(parts.iter().find(|p| p.part == k).map(|p| p.text.as_str()))
            });
            standing.push(Standing {
                key: SectionKey::of(k),
                text,
                known: extra.standing.is_some(),
            });
        }
        // The master's own line last: a routine's untrusted note is what
        // the model reads right before the message.
        let notes = (!mcp_notes.is_empty()).then(|| mcp_notes.join("\n"));
        let per_turn = [
            extra.instructions_unread.as_deref(),
            extra.skills_unread.as_deref(),
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

fn whose(frame: ContextFrame) -> Whose {
    match frame {
        ContextFrame::Person => Whose::Person,
        ContextFrame::Task => Whose::Task,
        ContextFrame::Callback => Whose::Callback,
    }
}

/// The block for `sections` in `frame`, as the loop sends it and the log
/// rebuilds it; `None` when it says nothing.
pub fn render<'a>(
    frame: ContextFrame,
    sections: impl IntoIterator<Item = &'a SentSection>,
) -> Option<String> {
    let mut body: Vec<&str> = Vec::new();
    let mut notes: Vec<&str> = Vec::new();
    for s in sections {
        if !s.text.is_empty() {
            body.push(&s.text);
        }
        notes.extend(s.note.as_deref());
    }
    let block = provider::block(&body.join("\n\n"), &notes, whose(frame))?;
    Some(block_capped(block))
}

/// The block cut to [`BLOCK_CAP`] at a line's end, with a quoted note.
fn block_capped(mut b: String) -> String {
    if b.len() <= BLOCK_CAP {
        return b;
    }
    let mut end = BLOCK_CAP - 128;
    while !b.is_char_boundary(end) {
        end -= 1;
    }
    let end = b[..end].rfind('\n').unwrap_or(end);
    b.truncate(end);
    b.push_str(&format!(
        "\n{OUTPUT_QUOTE}[cut: the harness sends at most {BLOCK_CAP} bytes of context with one \
         message]"
    ));
    b
}

/// A standing key of a recorded section; `None` for this turn's own (and
/// a key this build does not know, which is never taken out).
fn standing_key(s: &SentSection) -> Option<SectionKey> {
    SectionKey::parse(&s.key)
}

/// The last record of each turn in `window`, in the order those records
/// stand, for turns whose start is in it.
fn last_records(window: &[RawEvent]) -> Vec<ContextSent> {
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
    records
        .into_iter()
        .enumerate()
        .filter(|(i, c)| last.get(&c.turn_id) == Some(i))
        .map(|(_, c)| c)
        .collect()
}

/// What the model's history holds of each standing section in `window`
/// (the events after the last summary): the digest of its last record,
/// empty for one withdrawn.
pub fn had(window: &[RawEvent]) -> HashMap<SectionKey, String> {
    let mut out = HashMap::new();
    for c in last_records(window) {
        for s in &c.sections {
            if let Some(k) = standing_key(s) {
                out.insert(k, s.digest.clone());
            }
        }
    }
    out
}

/// Each turn's opening block as the history is rebuilt from `window`: a
/// standing section only in the last record that sent or withdrew it, and
/// in none when `superseded` (the turn being built) sends or withdraws it.
pub fn blocks(
    window: &[RawEvent],
    superseded: &HashSet<SectionKey>,
) -> HashMap<String, Option<String>> {
    let records = last_records(window);
    let mut owner: HashMap<SectionKey, usize> = HashMap::new();
    for (i, c) in records.iter().enumerate() {
        for s in &c.sections {
            if let Some(k) = standing_key(s) {
                owner.insert(k, i);
            }
        }
    }
    records
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let kept = c.sections.iter().filter(|s| match standing_key(s) {
                Some(k) => owner.get(&k) == Some(&i) && !superseded.contains(&k),
                None => true,
            });
            (c.turn_id.clone(), render(c.frame, kept))
        })
        .collect()
}

/// What a turn sends: its sections in the order they go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub frame: ContextFrame,
    pub sections: Vec<SentSection>,
}

impl Plan {
    /// The block as the opening carries it.
    pub fn text(&self) -> Option<String> {
        render(self.frame, &self.sections)
    }

    /// The standing sections it sends or withdraws: the history before
    /// it holds no earlier copy of them.
    pub fn superseded(&self) -> HashSet<SectionKey> {
        self.sections.iter().filter_map(standing_key).collect()
    }

    /// Its record.
    pub fn record(&self, turn_id: &str) -> EventKind {
        EventKind::ContextSent(ContextSent {
            turn_id: turn_id.to_string(),
            frame: self.frame,
            sections: self.sections.clone(),
        })
    }
}

/// The block for `sections` given what the model `had`; `None` when there
/// is nothing to send.
pub fn plan(
    sections: &Sections,
    had: &HashMap<SectionKey, String>,
    frame: ContextFrame,
) -> Option<Plan> {
    let mut out = Vec::new();
    for s in &sections.standing {
        let before = had.get(&s.key).filter(|d| !d.is_empty());
        match &s.text {
            Some(t) => {
                let d = digest(t);
                if before == Some(&d) {
                    continue;
                }
                out.push(SentSection {
                    key: s.key.as_str().into(),
                    digest: d,
                    text: t.clone(),
                    note: before.map(|_| s.key.replaced()),
                });
            }
            None if s.known && before.is_some() => out.push(SentSection {
                key: s.key.as_str().into(),
                digest: String::new(),
                text: String::new(),
                note: Some(s.key.withdrawn()),
            }),
            None => {}
        }
    }
    out.extend(sections.per_turn.iter().map(|t| SentSection {
        key: TURN.into(),
        digest: String::new(),
        text: t.clone(),
        note: None,
    }));
    (!out.is_empty()).then_some(Plan {
        frame,
        sections: out,
    })
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

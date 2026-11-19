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

/// How a nested instruction file's key begins (`instructions:web/AGENTS.md`).
/// The event scrubber keeps it and the harness's own keys as they are
/// (`harness_core::secret::schema::SECTION_KEYS`; `tests/section_keys.rs`
/// keeps the two lists alike).
pub const NESTED_PREFIX: &str = harness_core::secret::schema::NESTED_SECTION_PREFIX;

/// A standing section.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionKey {
    /// The node operator's instruction file.
    UserInstructions,
    Instructions,
    /// One instruction file of a subfolder of the working folder, by its
    /// path from there (`web/AGENTS.md`; B.9, T65): loaded when a call
    /// touches a file under it, a section of its own so it is replaced or
    /// withdrawn alone.
    Nested(String),
    Skills,
    /// The master's parts ([`StandingKey`]).
    Master(StandingKey),
}

impl SectionKey {
    pub fn as_str(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(match self {
            SectionKey::UserInstructions => "user_instructions",
            SectionKey::Instructions => "instructions",
            SectionKey::Nested(p) => return format!("{NESTED_PREFIX}{p}").into(),
            SectionKey::Skills => "skills",
            SectionKey::Master(k) => match k {
                StandingKey::Personal => "personal",
                StandingKey::Memory => "memory",
                StandingKey::MemoryRecall => "memory_recall",
                StandingKey::Workspace => "workspace",
                StandingKey::WorkspaceRecall => "workspace_recall",
                StandingKey::Remember => "remember",
                StandingKey::MemoryNotes => "memory_notes",
                StandingKey::Project => "project",
                StandingKey::ProjectMemory => "project_memory",
                StandingKey::ProjectRecall => "project_recall",
                StandingKey::EarlierChats => "earlier_chats",
                StandingKey::Board => "board",
            },
        })
    }

    pub fn parse(s: &str) -> Option<Self> {
        if let Some(p) = s.strip_prefix(NESTED_PREFIX) {
            return (!p.is_empty()).then(|| SectionKey::Nested(p.to_string()));
        }
        [
            SectionKey::UserInstructions,
            SectionKey::Instructions,
            SectionKey::Skills,
        ]
        .into_iter()
        .chain(StandingKey::ALL.map(SectionKey::Master))
        .find(|k| k.as_str() == s)
    }

    /// What the section is, for the harness's sentences: `None` for one
    /// that changes with the message (recall lines, notes, earlier chats),
    /// which replaces or leaves its earlier copy without a sentence.
    fn what(&self) -> Option<(String, bool)> {
        // (noun phrase, plural)
        let (what, many) = match self {
            SectionKey::UserInstructions => ("The node operator's instructions", true),
            SectionKey::Instructions => ("The repository's instruction files", true),
            // The path as one line, in quotation marks: a folder's name is
            // the repository's text.
            SectionKey::Nested(p) => {
                return Some((
                    format!("The instruction file {}", crate::nested::path_label(p)),
                    false,
                ));
            }
            SectionKey::Skills => ("The list of skills", false),
            SectionKey::Master(k) => match k {
                StandingKey::Personal => ("The person's reply style and instructions", true),
                StandingKey::Memory => ("What you remember about the person", false),
                StandingKey::Workspace => ("The workspace's memory", false),
                StandingKey::Remember => ("What `remember` does in this chat", false),
                StandingKey::Project => ("The project's brief", false),
                StandingKey::ProjectMemory => ("The project's facts", true),
                StandingKey::Board => ("The project's board", false),
                StandingKey::MemoryRecall
                | StandingKey::WorkspaceRecall
                | StandingKey::MemoryNotes
                | StandingKey::ProjectRecall
                | StandingKey::EarlierChats => return None,
            },
        };
        Some((what.to_string(), many))
    }

    /// The harness's sentence for a section sent again because it changed.
    fn replaced(&self) -> Option<String> {
        let (what, many) = self.what()?;
        let (verb, one) = if many {
            ("replace", "ones")
        } else {
            ("replaces", "one")
        };
        Some(format!(
            "{what} below {verb} the {one} sent earlier in this conversation."
        ))
    }

    /// The harness's sentence for a section sent earlier that is gone now.
    fn withdrawn(&self) -> Option<String> {
        let (what, many) = self.what()?;
        let verb = if many { "apply" } else { "applies" };
        let why = match self {
            SectionKey::UserInstructions => ": the node's instruction file is gone",
            SectionKey::Instructions => ": none were found for this turn",
            SectionKey::Nested(_) => ": the file is gone",
            SectionKey::Skills => ": no skills are offered now",
            SectionKey::Master(StandingKey::Memory | StandingKey::Workspace) => {
                ": memory is not given now"
            }
            SectionKey::Master(StandingKey::Remember) => ": `remember` is not offered now",
            SectionKey::Master(_) => "",
        };
        Some(format!(
            "{what} sent earlier in this conversation no longer {verb}{why}."
        ))
    }
}

struct Standing {
    key: SectionKey,
    text: Option<String>,
    /// The list whose place it belongs to: when that list is sent again
    /// or withdrawn, this goes too, even where its own absence says
    /// nothing (a callback turn).
    tied: Option<SectionKey>,
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
            tied: None,
            text: if unread.is_some() {
                None
            } else {
                prep(text.as_deref())
            },
            known: unread.is_none(),
        };
        let mut standing = vec![
            Standing {
                key: SectionKey::UserInstructions,
                ..read(&extra.user_instructions, &extra.user_instructions_unread)
            },
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
            let tied = match k {
                StandingKey::MemoryRecall | StandingKey::MemoryNotes => Some(StandingKey::Memory),
                StandingKey::WorkspaceRecall => Some(StandingKey::Workspace),
                StandingKey::ProjectRecall | StandingKey::EarlierChats => {
                    Some(StandingKey::ProjectMemory)
                }
                _ => None,
            };
            // A callback turn's context has the lists only: what is keyed
            // on a person's message, and `remember`, are not given and so
            // say nothing (review M1). A turn the master gave no context
            // at all (a queued-only turn it lost the context of) keeps the
            // brief and the board and reads no memory (review F2).
            let known = match &extra.standing {
                Some(_) if extra.standing_partial => tied.is_none() && k != StandingKey::Remember,
                Some(_) => true,
                // An agent called after the first (X.37) was given no
                // context of its own: it has no `remember` tool, so the
                // instructions the first message carried for it are
                // withdrawn (recorded, so a replay is the same bytes); the
                // personalisation, the brief and the board it keeps.
                None => {
                    k.is_memory()
                        || (k == StandingKey::Remember
                            && extra.cause.as_ref().is_some_and(|c| c.mention().is_some()))
                }
            };
            standing.push(Standing {
                key: SectionKey::Master(k),
                text,
                tied: tied.map(SectionKey::Master),
                known,
            });
        }
        // The master's own line last: a routine's untrusted note is what
        // the model reads right before the message.
        let notes = (!mcp_notes.is_empty()).then(|| mcp_notes.join("\n"));
        let per_turn = [
            extra.mode_note.as_deref(),
            extra.user_instructions_unread.as_deref(),
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

    /// A line of this turn's own (a nested instruction file named, not
    /// loaded: B.9), first among them.
    pub fn push_turn(&mut self, text: String, secrets: &harness_core::SecretSet) {
        if !text.is_empty() {
            self.per_turn.insert(0, capped(secrets.scrub(&text)));
        }
    }

    /// The instruction files of subfolders (B.9, T65) after the
    /// repository's own, in the order given: each re-read at this turn's
    /// start (`text`: what it gives now; `None` and `known`: it is gone;
    /// `None` and not `known`: it was not read this turn, so nothing about
    /// it changes), or loaded now for a file the turn's message mentions.
    /// Scrubbed and capped as every section.
    pub fn with_nested(&mut self, nested: Vec<Nested>, secrets: &harness_core::SecretSet) {
        let at = self
            .standing
            .iter()
            .position(|s| s.key == SectionKey::Instructions)
            .map_or(self.standing.len(), |i| i + 1);
        let items = nested.into_iter().map(|n| Standing {
            key: SectionKey::Nested(n.path),
            text: n
                .text
                .filter(|t| !t.is_empty())
                .map(|t| capped(secrets.scrub(&t))),
            tied: None,
            known: n.known,
        });
        self.standing.splice(at..at, items);
    }
}

/// One instruction file of a subfolder for [`Sections::with_nested`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nested {
    /// Its path from the working folder (`web/AGENTS.md`).
    pub path: String,
    pub text: Option<String>,
    pub known: bool,
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
        ContextFrame::Step => Whose::Step,
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

/// A record that counts in a window: a turn's opening record, or a step's
/// block (B.9), with the seq it stands at.
struct Record {
    seq: u64,
    sent: ContextSent,
}

/// The records of `window` that the history holds, in the order they
/// stand, for turns whose start is in it: each turn's last opening record
/// (an in-turn summary records the opening again) and every step record
/// ([`ContextFrame::Step`], each a block of its own after its step).
fn records(window: &[RawEvent]) -> Vec<Record> {
    let mut started = HashSet::new();
    let mut last: HashMap<String, usize> = HashMap::new();
    let mut records = Vec::new();
    for e in window {
        match e.typed() {
            Ok(EventKind::TurnStarted(t)) => {
                started.insert(t.turn_id);
            }
            Ok(EventKind::ContextSent(c)) if started.contains(&c.turn_id) => {
                if c.frame != ContextFrame::Step {
                    last.insert(c.turn_id.clone(), records.len());
                }
                records.push(Record {
                    seq: e.seq,
                    sent: c,
                });
            }
            _ => {}
        }
    }
    records
        .into_iter()
        .enumerate()
        .filter(|(i, r)| r.sent.frame == ContextFrame::Step || last.get(&r.sent.turn_id) == Some(i))
        .map(|(_, r)| r)
        .collect()
}

/// What the model's history holds of each standing section in `window`
/// (the events after the last summary): the digest of its last record,
/// empty for one withdrawn.
pub fn had(window: &[RawEvent]) -> HashMap<SectionKey, String> {
    let mut out = HashMap::new();
    for r in records(window) {
        for s in &r.sent.sections {
            if let Some(k) = standing_key(s) {
                out.insert(k, s.digest.clone());
            }
        }
    }
    out
}

/// The instruction files of subfolders the history in `window` holds
/// (B.9): each one's path and the size of its text as sent, in the order
/// they were first sent; one withdrawn since is not among them.
pub fn nested_live(window: &[RawEvent]) -> Vec<(String, usize)> {
    let mut order: Vec<String> = Vec::new();
    let mut size: HashMap<String, usize> = HashMap::new();
    for r in records(window) {
        for s in &r.sent.sections {
            if let Some(SectionKey::Nested(p)) = standing_key(s) {
                if !order.contains(&p) {
                    order.push(p.clone());
                }
                size.insert(p, s.text.len());
            }
        }
    }
    order
        .into_iter()
        .filter_map(|p| match size.get(&p) {
            Some(&n) if n > 0 => Some((p, n)),
            _ => None,
        })
        .collect()
}

/// The context blocks of a history rebuilt from a window ([`blocks`]).
#[derive(Debug, Default)]
pub struct Blocks {
    /// Each turn's opening block.
    pub openings: HashMap<String, Option<String>>,
    /// Each step's block (B.9), by the seq its record stands at.
    pub steps: HashMap<u64, Option<String>>,
}

/// The blocks as the history is rebuilt from `window`: a standing section
/// only in the last record that sent or withdrew it, and in none when
/// `superseded` (the turn being built) sends or withdraws it.
pub fn blocks(window: &[RawEvent], superseded: &HashSet<SectionKey>) -> Blocks {
    let records = records(window);
    let mut owner: HashMap<SectionKey, usize> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        for s in &r.sent.sections {
            if let Some(k) = standing_key(s) {
                owner.insert(k, i);
            }
        }
    }
    let mut out = Blocks::default();
    for (i, r) in records.iter().enumerate() {
        let kept = r.sent.sections.iter().filter(|s| match standing_key(s) {
            Some(k) => owner.get(&k) == Some(&i) && !superseded.contains(&k),
            None => true,
        });
        let block = render(r.sent.frame, kept);
        if r.sent.frame == ContextFrame::Step {
            out.steps.insert(r.seq, block);
        } else {
            out.openings.insert(r.sent.turn_id.clone(), block);
        }
    }
    out
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
    let mut out: Vec<SentSection> = Vec::new();
    for s in &sections.standing {
        let before = had.get(&s.key).filter(|d| !d.is_empty());
        // Its place's list goes or changes: so does it.
        let known = s.known
            || s.tied
                .as_ref()
                .is_some_and(|t| out.iter().any(|o| o.key == t.as_str()));
        match &s.text {
            Some(t) => {
                let d = digest(t);
                if before == Some(&d) {
                    continue;
                }
                out.push(SentSection {
                    key: s.key.as_str().into_owned(),
                    digest: d,
                    text: t.clone(),
                    note: before.and_then(|_| s.key.replaced()),
                });
            }
            None if known && before.is_some() => out.push(SentSection {
                key: s.key.as_str().into_owned(),
                digest: String::new(),
                text: String::new(),
                note: s.key.withdrawn(),
            }),
            None => {}
        }
    }
    // After a callback turn's opening the block's lines are the files and
    // the skills; this turn's own lines (a read that failed) go into its
    // first line as notes (review N2).
    out.extend(sections.per_turn.iter().map(|t| {
        let (text, note) = match frame {
            ContextFrame::Callback => (String::new(), Some(t.clone())),
            _ => (t.clone(), None),
        };
        SentSection {
            key: TURN.into(),
            digest: String::new(),
            text,
            note,
        }
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

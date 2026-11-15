//! Master <-> node messages. PLAN.md §3. JSON, `{"type": "..."}`-tagged.
//! Changes must be additive within a major `PROTOCOL_VERSION`.

use harness_core::agent::AgentSpec;
use harness_core::event::Profile;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::{RawEvent, SessionId};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// The largest WebSocket frame (and message) either end of a remote link
/// accepts. A bigger one closes the link (checked from the frame header,
/// before the payload is read).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// The most file bytes one `artifact.chunk` carries (the node clamps a
/// larger `len`); with four in flight a transfer stays far below the link's
/// frame limit.
pub const MAX_CHUNK: u32 = 256 * 1024;

/// The `ack.error` of a node that is stopping (SIGTERM or SIGINT, A.19) for
/// a command that would start new work: a session, a turn, a restore, a
/// staged file. Exactly this text, so the master can tell it from a refusal
/// of the command itself: it is answered as a node that is away (503, a
/// routine retries), not as a bad request. A node that sends it falsely
/// gains nothing it could not get by not answering.
pub const NODE_STOPPING: &str = "this node is stopping (its owner or its service manager stopped \
                                 it): send this again once it is back";

/// The node capability that announces the `artifact.*` messages. The master
/// sends them only to nodes whose hello listed it (an unknown message drops
/// the link).
pub const CAP_ARTIFACTS: &str = "artifacts";

/// The node capability that announces `session.prompt.images`. The master
/// sends images only to nodes whose hello listed it.
pub const CAP_IMAGES: &str = "images";

/// The node capability that announces `session.stop_background` (T.8d,
/// docs/background-commands.md). The master sends it only to nodes whose
/// hello listed it.
pub const CAP_BACKGROUND: &str = "background";

/// The node capability that announces `session.callback` (T.8d step 3).
/// The master resumes chats only on nodes whose hello listed it.
pub const CAP_CALLBACK: &str = "callback";

/// The node capability that announces `session.handoff` (X.16): the
/// master asks for hand-off notes only on nodes whose hello listed it.
pub const CAP_HANDOFF: &str = "handoff";

/// The node capability that announces `session.diff` (X.12, PLAN.md T53).
/// The master asks for a turn's changes only on nodes whose hello listed it.
pub const CAP_DIFF: &str = "diff";

/// The largest serialized `session.diff_result` a node sends: a quarter of
/// the link's frame, so one answer holds the node's ordered output back by
/// a few seconds at most on a slow link. The node cuts what does not fit
/// and says so (`TurnDiff::cut`).
pub const MAX_DIFF_JSON: usize = 2 * 1024 * 1024;

/// What one turn changed in its session's folder, computed by the owning
/// node from the turn's two checkpoints (X.12). A view, never an event:
/// nothing of it is logged or stored, on the node or the master.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnDiff {
    /// The changed files, in git's order (by path).
    pub files: Vec<DiffFile>,
    /// Changed files past the listed ones (not named, only counted).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more_files: u64,
    /// Something was left out to keep the answer small: a file's lines, or
    /// whole files' lines past the total budget. Each such file says so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// Git repositories nested in the folder, which no checkpoint holds:
    /// their files are not compared (docs/undo-and-stash.md §2.6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested: Vec<String>,
    /// Files of this diff that another chat working in the folder changed
    /// during the turn (its edit tools, its own undo): an undo of the turn
    /// would put them back too, so it asks first (PLAN.md T57). Nothing
    /// about that chat.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others_wrote: Vec<String>,
    /// Files of this diff another chat may have changed during the turn
    /// (its commands, a background command, a chat deleted since), not in
    /// `others_wrote`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others_may: Vec<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
    /// Moved (`old_path` says from where), maybe also changed.
    Renamed,
    /// A file became a link, or the other way round.
    TypeChanged,
}

/// What the answer holds of a file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffContent {
    /// Line changes in `hunks` (none for a pure rename or a mode change).
    Text,
    /// Git found binary content: only the sizes are given.
    Binary,
    /// A side is larger than the node compares line by line.
    TooLarge,
    /// Left out: the answer's budget was spent on the files before it.
    Omitted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    /// Relative to the checkpoint's folder (lossy UTF-8; hidden characters
    /// are the viewer's to mark).
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: DiffStatus,
    pub content: DiffContent,
    /// Git's modes (`100644`, `100755`, `120000` for a link); absent on the
    /// side that does not exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_size: Option<u64>,
    /// Lines added and removed in the hunks given.
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub removed: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<DiffHunk>,
    /// Some of this file's lines, or of one long line, were cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
}

/// One hunk of a unified diff with three lines of context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The text after the second `@@` (the function git found), if any.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section: String,
    /// Each line with its mark first: ` ` context, `+` added, `-` removed,
    /// `\` "no newline at end of file" for the line before it.
    pub lines: Vec<String>,
}

/// The node capability that says it offers `remember` to its sessions
/// (registered there and not in its `denied_tools`). The tool runs nothing
/// on the node; the master files a mirrored call as a proposal only from a
/// node whose hello listed this, and tells the model about the tool only
/// in chats on such a node.
pub const CAP_REMEMBER: &str = "remember";

/// The node capability that says it offers `recall` to its sessions
/// (registered there and not in its `denied_tools`) and speaks
/// `recall.request`. The master offers the tool only in chats on such a
/// node, answers requests only from one, and sends `recall.result` only to
/// one (PLAN.md T55).
pub const CAP_RECALL: &str = "recall";

/// The node capability that says it runs sessions of an execution mode
/// (N.10a): `exec:sandbox`, `exec:host`. Announced from the node's own
/// self-test and `[execution]` at start; what the UI offers and names. The
/// node refuses a mode it does not offer whatever the master believes.
pub const CAP_EXEC_PREFIX: &str = "exec:";

/// The capability for `mode`.
pub fn exec_capability(mode: harness_core::policy::ExecMode) -> String {
    format!("{CAP_EXEC_PREFIX}{}", mode.id())
}

/// The node capabilities that announce `session.set_approval_mode` and say
/// its approval modes (T50): `approval-max:<mode>`, the loosest mode
/// node.toml allows any chat there (`max_approval_mode`), and
/// `approval-default:<mode>`, what a create without a mode gets there. What the UI offers and names; the node
/// clamps every request whatever the master believes. The master sends
/// `session.set_approval_mode` only to a node whose hello listed
/// `approval-max:*`.
pub const CAP_APPROVAL_MAX_PREFIX: &str = "approval-max:";
/// The node capability that announces `session.set_step_limit` and says
/// its step limit (X.14): `steps-max:<n>`, node.toml's `[limits]
/// max_steps`, the most a chat there may be set to. What the UI offers;
/// the node clamps every request. The master sends `session.set_step_limit`
/// only to a node whose hello listed it.
pub const CAP_STEPS_MAX_PREFIX: &str = "steps-max:";

/// How a node's refusal of a `session.set_step_limit` sent as a lowering
/// that would raise the chat's limit begins. The master answers it as a
/// refusal to send again (its copy of the log was behind).
pub const STEPS_NOT_A_LOWERING: &str =
    "this change was sent as a lowering but would raise the chat's step limit";
pub const CAP_APPROVAL_DEFAULT_PREFIX: &str = "approval-default:";

/// How a node's refusal of a `session.set_approval_mode` sent as a lowering
/// that would raise the mode begins. The master answers it as a refusal to
/// send again (its copy of the log was behind; on the next try it sees the
/// mode the node has).
pub const MODE_NOT_A_LOWERING: &str =
    "this change was sent as a lowering but would raise the chat's approval mode";

/// The node capability that announces the person's answers beyond allow
/// and deny: a note with a denial, `allow_chat` and `session.revoke_allow`.
/// The master sends `allow_chat` and `session.revoke_allow` only to a node
/// whose hello listed it (an unknown decision or message drops the link).
pub const CAP_ANSWERS: &str = "answers";

/// The gaps of the modes a node offers, for the UI to say in the chat
/// (reviews T8B-3, N.10a L-d, L-e). The node's state at its hello: one the
/// node closes later (the network namespace taken by a later probe) is
/// still announced until its next hello.
///
/// With `[sandbox] network = "off"`: the sandbox mode's commands have no
/// network namespace (bubblewrap could not make one at start), so UDP and
/// DNS still go out; Landlock stops TCP.
pub const GAP_SANDBOX_NETNS: &str = "gap:sandbox:netns";
/// With the network off: a `host` chat never has a namespace; Landlock
/// stops TCP, UDP and DNS go out.
pub const GAP_HOST_NETNS: &str = "gap:host:netns";
/// With the network off: a `host` chat where nothing applies it (`[sandbox]
/// mode = "off"`, or Landlock not enforcing its TCP rule): everything goes
/// out.
pub const GAP_HOST_NET_OPEN: &str = "gap:host:net-open";
/// Below Landlock ABI 9: a sandbox command can reach a service of the
/// node's user over a socket inside a granted folder and have it start a
/// process outside the containment.
pub const GAP_SANDBOX_SOCKETS: &str = "gap:sandbox:sockets";
/// The shared-`/proc` form: every process ends with the command, but
/// `/proc` lists this machine's processes.
pub const GAP_SANDBOX_PROC: &str = "gap:sandbox:proc";
/// Every gap a node may announce.
pub const GAPS: [&str; 5] = [
    GAP_SANDBOX_NETNS,
    GAP_HOST_NETNS,
    GAP_HOST_NET_OPEN,
    GAP_SANDBOX_SOCKETS,
    GAP_SANDBOX_PROC,
];

/// How a node's refusal of a `session.callback` begins when there is
/// nothing to resume: every named end was already delivered, covered by a
/// later turn or refused before, or the chat is busy. The master drops
/// those quietly (a second send of the same callback, a race with the
/// person's message); every other refusal is the owner's to see.
pub const CALLBACK_NOTHING: &str = "nothing to resume";

/// A node's refusal of a `session.callback` while a turn of the chat runs
/// (a [`CALLBACK_NOTHING`] for now): the master looks again later. The ends
/// it named are then covered by that turn, or, beyond what one callback may
/// name ([`MAX_CALLBACK_IDS`]), still wait and are asked for once it ended.
pub const CALLBACK_BUSY: &str = "nothing to resume: a turn is already running";

/// How a node's refusal of a `session.callback` on its own rules begins
/// (its budget, `callback_turns = 0`, a chat that takes none). The node
/// recorded `callback_refused` in the chat's log, which is where the owner
/// sees it; the master adds nothing of its own.
pub const CALLBACK_REFUSED: &str = "the agent was not resumed";

/// A `session.handoff` the node did not start because no note is due
/// (nothing changed since the last one, a turn ran since the master read
/// the log, a chat that takes none): the master drops it quietly.
pub const HANDOFF_NOTHING: &str = "no hand-off note due";
/// A `session.handoff` refused because a turn or another note runs: the
/// master asks again at its next look.
pub const HANDOFF_BUSY: &str = "no hand-off note now: the chat is busy";

/// The most background ids one `session.callback` may name.
pub const MAX_CALLBACK_IDS: usize = 64;

/// Who asks a node to stop a session's background commands. The node
/// records the end with this reason; nothing here can start or widen
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopBackgroundBy {
    /// The person (step 2: the chat's Stop per command, Stop all).
    User,
    /// The master's revocation sweep: the owner lost access.
    AccessRevoked,
}

/// At most this many images go with one message...
pub const MAX_IMAGES: usize = 4;
/// ...and at most this many bytes of them together (before base64).
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
/// The largest serialized `session.prompt` the master sends: the link's
/// frame is 8 MiB, and images, attachments, the agent spec and the context
/// together could pass it otherwise.
pub const MAX_PROMPT_JSON: usize = 7 * 1024 * 1024;

/// One image sent with a prompt (`session.prompt.images`). The node keeps
/// the bytes for the chat and records the rest in `user_message.images`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImage {
    pub media_id: String,
    /// Hex sha256 of the bytes (the node checks it).
    pub sha256: String,
    pub mime: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// The bytes, base64.
    pub data: String,
}

impl ChatImage {
    /// What the message records: everything but the bytes.
    pub fn image_ref(&self) -> harness_core::event::ImageRef {
        harness_core::event::ImageRef {
            media_id: self.media_id.clone(),
            sha256: self.sha256.clone(),
            mime: self.mime.clone(),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHead {
    pub session_id: SessionId,
    pub head_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeToMaster {
    #[serde(rename = "node.hello")]
    Hello {
        protocol_version: u32,
        node_id: String,
        hostname: String,
        version: String,
        #[serde(default)]
        capabilities: Vec<String>,
        #[serde(default)]
        sessions: Vec<SessionHead>,
    },
    #[serde(rename = "session.event")]
    Event { event: RawEvent },
    /// A replay batch answering `resume`. `head_seq` is the node's head at the
    /// time the batch was read; more batches follow while `seq < head_seq`.
    #[serde(rename = "session.snapshot")]
    Snapshot {
        session_id: SessionId,
        head_seq: u64,
        events: Vec<RawEvent>,
    },
    /// The node deleted a session (on request or because its incognito TTL
    /// ran out); the master drops its mirror copy.
    #[serde(rename = "session.deleted")]
    Deleted { session_id: SessionId },
    #[serde(rename = "ack")]
    Ack {
        req_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A `recall` call of a session this node runs (PLAN.md T55): words to
    /// look for and nothing else. The master decides whose chats and which
    /// memory from `session_id` alone and answers with one `recall.result`.
    #[serde(rename = "recall.request")]
    Recall {
        req_id: String,
        session_id: SessionId,
        query: harness_core::recall::RecallQuery,
    },
    /// Bytes of a staged file, answering `artifact.fetch` (pull only: a
    /// node never sends one unasked). `error: "busy"` asks for a retry.
    #[serde(rename = "artifact.chunk")]
    ArtifactChunk {
        req_id: String,
        stage_id: String,
        offset: u64,
        /// Base64.
        #[serde(default)]
        data: String,
        eof: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The answer to `session.diff` (pull only: never sent unasked): the
    /// turn's changes, or why there are none to show, in words for the
    /// person. At most `MAX_DIFF_JSON` serialized.
    #[serde(rename = "session.diff_result")]
    DiffResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<Box<TurnDiff>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MasterToNode {
    #[serde(rename = "master.welcome")]
    Welcome { protocol_version: u32 },
    #[serde(rename = "session.create")]
    Create {
        req_id: String,
        session_id: SessionId,
        title: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// A request only; the node clamps it to its own policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_mode: Option<ApprovalMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The session's initial agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentSpec>,
        /// Temporary chat: excluded from search and memory, purged after a TTL.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        incognito: bool,
        /// This chat never gets a callback turn (a routine's run chat). The
        /// node records it in `session_created` and holds to it for good;
        /// no later command can clear it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        no_callbacks: bool,
        /// How the session's commands run (N.10a). A request only: the node
        /// refuses a mode it does not offer, whatever the master sends, and
        /// without one takes `sandbox` or refuses (never `host` unless asked
        /// for by name). The master always sends one. Recorded in
        /// `session_created`; no later command carries or changes it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exec_mode: Option<harness_core::policy::ExecMode>,
    },
    #[serde(rename = "session.prompt")]
    Prompt {
        req_id: String,
        session_id: SessionId,
        text: String,
        /// The agent that runs this turn (the session's agent, or an
        /// @mention override). Resolved by the master; the node does not
        /// store agent definitions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentSpec>,
        /// The person chose `agent` for this message (an explicit agent or
        /// an @mention), not the chat's own. A message that would wait for
        /// a running turn of another agent is refused (X.5, review ST-5).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        agent_named: bool,
        /// Documents with their extracted text (extracted on the master).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<harness_core::event::Attachment>,
        /// Parallel answers from these models (chat turns, no tools).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        compare: Vec<String>,
        /// Token-saving methods for this turn (absent = all off).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        savings: Option<harness_core::savings::Savings>,
        /// The master's context for this message (the person's settings,
        /// memory, the project's brief and board), placed after the history
        /// so the cached prefix does not change. The loop records what it
        /// sent of it in the turn's `context_sent` and sends a standing part
        /// again only when it changed (X.1).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<Box<harness_core::backend::MasterContext>>,
        /// An answer to a choices or form block; `text` is then the user's
        /// note (may be empty). The node checks it against its own log.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<harness_core::event::FormReply>,
        /// Images for this message; only to nodes with `CAP_IMAGES`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ChatImage>,
    },
    /// Resume an idle chat because background commands ended (a callback
    /// turn, T.8d step 3; only to nodes with `CAP_CALLBACK`). There is no
    /// person's text, attachment, image, reply or context: the node opens
    /// the turn with the harness's own frame and the ended commands' notes
    /// from its log. A request only: the node starts it only if every id is
    /// an undelivered end in its log with no turn after it, and its own
    /// `[limits]` allow it (`callback_turns`, `callback_max_steps`).
    #[serde(rename = "session.callback")]
    Callback {
        req_id: String,
        session_id: SessionId,
        bg_ids: Vec<String>,
        /// The chat's agent, resolved by the master as for a prompt.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentSpec>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        savings: Option<harness_core::savings::Savings>,
        /// The standing parts of the master's context for the chat's agent
        /// and owner (`MasterContext::unattended`, X.1 review M1): the
        /// turn's request extends the last one when they did not change.
        /// Absent: the turn reads no memory.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<Box<harness_core::backend::MasterContext>>,
    },
    /// Write the chat's hand-off note now if one is due (X.16,
    /// docs/handoff-notes.md; only to nodes with [`CAP_HANDOFF`]). A
    /// request only: the node writes it only for a chat of its own that
    /// takes notes (agentic, not a sub-agent's, not a routine's), whose log
    /// says one is due (`harness_core::handoff::due`), with no turn
    /// running; it acks once the writing started and records the note (or
    /// that no model wrote one) in the chat's log. A turn that starts
    /// meanwhile cancels it.
    #[serde(rename = "session.handoff")]
    Handoff {
        req_id: String,
        session_id: SessionId,
        /// The small model to write it (prefixed); `None`: the chat's own.
        /// Tried first; the chat's own model is the fallback.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        small_model: Option<String>,
        /// Context windows the master knows (tokens), by model: the note's
        /// input is sized to the writing model's.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        windows: std::collections::BTreeMap<String, u64>,
        /// Those models that take a reasoning effort: the note asks them
        /// for the lowest, the others for none.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        low_effort: Vec<String>,
    },
    /// The user picked one of several parallel answers.
    #[serde(rename = "session.select_variant")]
    SelectVariant {
        req_id: String,
        session_id: SessionId,
        group: String,
        message_id: String,
    },
    /// Change the session's active agent (records `agent_changed`).
    #[serde(rename = "session.set_agent")]
    SetAgent {
        req_id: String,
        session_id: SessionId,
        agent: AgentSpec,
    },
    /// Change the chat's approval mode (T50). A request only: the node
    /// clamps it by its own `max_approval_mode` (nothing else), refuses it for a sub-agent's or a routine's chat, and records
    /// the result as `approval_mode_changed`. Only to nodes whose hello
    /// listed `approval-max:*`.
    #[serde(rename = "session.set_approval_mode")]
    SetApprovalMode {
        req_id: String,
        session_id: SessionId,
        mode: ApprovalMode,
        /// The master judged it a lowering from its copy of the log and so
        /// skipped the owner's level check: the node refuses it if it would
        /// in fact raise the chat's mode (`MODE_NOT_A_LOWERING`; the copy
        /// was behind), and records nothing.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        lowering: bool,
    },
    /// Change the chat's step limit (X.14). A request only: the node
    /// clamps it to `1..=` its own `[limits] max_steps`, refuses it for a
    /// sub-agent's or a routine's chat, and records the result as
    /// `step_limit_changed`. Only to nodes whose hello listed `steps-max:*`.
    #[serde(rename = "session.set_step_limit")]
    SetStepLimit {
        req_id: String,
        session_id: SessionId,
        max_steps: i64,
        /// The master judged it a lowering from its copy of the log and so
        /// skipped the owner's level check: the node refuses it if it would
        /// in fact raise the chat's limit (`STEPS_NOT_A_LOWERING`; the copy
        /// was behind), and records nothing.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        lowering: bool,
    },
    #[serde(rename = "session.cancel")]
    Cancel {
        req_id: String,
        session_id: SessionId,
    },
    /// Stop one background command of a session (`bg_id`), or all of them.
    /// A turn's cancel does not stop them; this does (only to nodes with
    /// `CAP_BACKGROUND`).
    #[serde(rename = "session.stop_background")]
    StopBackground {
        req_id: String,
        session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bg_id: Option<String>,
        by: StopBackgroundBy,
    },
    /// Undo the file changes of one finished turn (git checkpoint).
    #[serde(rename = "session.restore")]
    Restore {
        req_id: String,
        session_id: SessionId,
        turn_id: String,
        /// Overwrite files edited after the turn too.
        #[serde(default)]
        force: bool,
        /// Rewind: also undo every later turn (back to before this one).
        #[serde(default)]
        rewind: bool,
    },
    /// Delete a session and its log (cancels a running turn first).
    #[serde(rename = "session.delete")]
    Delete {
        req_id: String,
        session_id: SessionId,
    },
    #[serde(rename = "approval.respond")]
    ApprovalRespond {
        req_id: String,
        session_id: SessionId,
        approval_id: String,
        decision: Decision,
        /// The person's note with a denial; the node drops it with an
        /// allow and cuts it to `MAX_DENY_NOTE`. Only to nodes with
        /// `CAP_ANSWERS` (an older one ignores the field).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Remove a call the person allowed for the rest of a chat
    /// (`chat_allow_removed`). Only to nodes with `CAP_ANSWERS`.
    #[serde(rename = "session.revoke_allow")]
    RevokeAllow {
        req_id: String,
        session_id: SessionId,
        grant_id: String,
    },
    #[serde(rename = "resume")]
    Resume {
        session_id: SessionId,
        last_seq: u64,
    },
    /// Read part of a staged file. Answered by one `artifact.chunk`, never
    /// by an ack (it is not in `req_id()`), and handled outside the node's
    /// command loop so a transfer never holds up a prompt.
    #[serde(rename = "artifact.fetch")]
    ArtifactFetch {
        req_id: String,
        session_id: SessionId,
        stage_id: String,
        offset: u64,
        /// At most `MAX_CHUNK`; the node clamps it.
        len: u32,
    },
    /// The master has the file (or gave up): the node deletes the stage.
    #[serde(rename = "artifact.release")]
    ArtifactRelease {
        req_id: String,
        session_id: SessionId,
        stage_id: String,
    },
    /// "Save as artifact": stage `path` of the session's folder. Acked once
    /// the node checked the path; the copy ends in `artifact_staged` or
    /// `artifact_stage_failed`.
    #[serde(rename = "artifact.stage")]
    ArtifactStage {
        req_id: String,
        session_id: SessionId,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// The answer to a `recall.request`: hits, or why there are none. Only
    /// to nodes whose hello listed `CAP_RECALL`; never acked.
    #[serde(rename = "recall.result")]
    RecallResult {
        req_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hits: Vec<harness_core::recall::RecallHit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What one turn changed (X.12): read from the turn's checkpoints,
    /// answered by one `session.diff_result`, never by an ack, handled
    /// outside the node's command loop, and nothing is logged. `rewind`:
    /// this turn and every later one (before this turn against the latest
    /// state, what a rewind would undo). Only to nodes with `CAP_DIFF`.
    #[serde(rename = "session.diff")]
    Diff {
        req_id: String,
        session_id: SessionId,
        turn_id: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rewind: bool,
    },
}

impl MasterToNode {
    pub fn req_id(&self) -> Option<&str> {
        match self {
            MasterToNode::Create { req_id, .. }
            | MasterToNode::Prompt { req_id, .. }
            | MasterToNode::Callback { req_id, .. }
            | MasterToNode::Handoff { req_id, .. }
            | MasterToNode::SetAgent { req_id, .. }
            | MasterToNode::SetApprovalMode { req_id, .. }
            | MasterToNode::SetStepLimit { req_id, .. }
            | MasterToNode::Cancel { req_id, .. }
            | MasterToNode::StopBackground { req_id, .. }
            | MasterToNode::Delete { req_id, .. }
            | MasterToNode::Restore { req_id, .. }
            | MasterToNode::SelectVariant { req_id, .. }
            | MasterToNode::ApprovalRespond { req_id, .. }
            | MasterToNode::RevokeAllow { req_id, .. }
            | MasterToNode::ArtifactRelease { req_id, .. }
            | MasterToNode::ArtifactStage { req_id, .. } => Some(req_id),
            // A fetch is answered by its chunk, never acked.
            MasterToNode::Welcome { .. }
            | MasterToNode::Resume { .. }
            | MasterToNode::ArtifactFetch { .. }
            | MasterToNode::RecallResult { .. }
            | MasterToNode::Diff { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `recall.request` carries words only: a field naming a user, a
    /// workspace or a chat does not parse, so a node cannot widen the scope
    /// the master takes from the session (PLAN.md T55).
    #[test]
    fn recall_messages_carry_words_only() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let r = NodeToMaster::Recall {
            req_id: "r".into(),
            session_id: sid,
            query: harness_core::recall::RecallQuery {
                query: "plan".into(),
                scope: harness_core::recall::RecallIn::Chats,
                limit: Some(3),
            },
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "recall.request", "req_id": "r", "session_id": sid,
                   "query": {"query": "plan", "in": "chats", "limit": 3}})
        );
        for extra in ["user_id", "owner", "workspace_id", "session_id", "chats"] {
            let v = json!({"type": "recall.request", "req_id": "r", "session_id": sid,
                           "query": {"query": "plan", extra: "x"}});
            assert!(
                serde_json::from_value::<NodeToMaster>(v).is_err(),
                "{extra}"
            );
        }
        let a = MasterToNode::RecallResult {
            req_id: "r".into(),
            hits: vec![],
            error: Some("no".into()),
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "recall.result", "req_id": "r", "error": "no"})
        );
        assert_eq!(a.req_id(), None, "never acked");
    }

    #[test]
    fn wire_shapes_are_stable() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let m = MasterToNode::Resume {
            session_id: sid,
            last_seq: 7,
        };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type": "resume", "session_id": "0193f2a0-0000-7000-8000-000000000001", "last_seq": 7})
        );
        let a = NodeToMaster::Ack {
            req_id: "r".into(),
            ok: true,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"type": "ack", "req_id": "r", "ok": true})
        );
        let d = MasterToNode::ApprovalRespond {
            req_id: "r".into(),
            session_id: sid,
            approval_id: "a".into(),
            decision: Decision::AllowSession,
            note: None,
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["type"], "approval.respond");
        assert_eq!(v["decision"], "allow_session");
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), d);
        let m = MasterToNode::SetApprovalMode {
            lowering: false,
            req_id: "r".into(),
            session_id: sid,
            mode: ApprovalMode::AcceptEdits,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.set_approval_mode", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "mode": "accept_edits"})
        );
        assert_eq!(m.req_id(), Some("r"));
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), m);
        let m = MasterToNode::SetStepLimit {
            req_id: "r".into(),
            session_id: sid,
            max_steps: 30,
            lowering: false,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["type"], "session.set_step_limit");
        assert_eq!(v["max_steps"], 30);
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), m);
    }

    #[test]
    fn artifact_messages_wire_shapes() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let fetch = MasterToNode::ArtifactFetch {
            req_id: "r1".into(),
            session_id: sid,
            stage_id: "s".into(),
            offset: 262_144,
            len: MAX_CHUNK,
        };
        assert_eq!(
            serde_json::to_value(&fetch).unwrap(),
            json!({"type": "artifact.fetch", "req_id": "r1",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001",
                   "stage_id": "s", "offset": 262_144, "len": 262_144})
        );
        // Answered by a chunk, never by an automatic ack.
        assert_eq!(fetch.req_id(), None);
        let release = MasterToNode::ArtifactRelease {
            req_id: "r2".into(),
            session_id: sid,
            stage_id: "s".into(),
        };
        assert_eq!(release.req_id(), Some("r2"));
        assert_eq!(
            serde_json::to_value(&release).unwrap()["type"],
            "artifact.release"
        );
        let stage = MasterToNode::ArtifactStage {
            req_id: "r3".into(),
            session_id: sid,
            path: "target/release/app".into(),
            name: None,
        };
        assert_eq!(stage.req_id(), Some("r3"));
        let v = serde_json::to_value(&stage).unwrap();
        assert_eq!(v["type"], "artifact.stage");
        assert!(v.get("name").is_none(), "{v}");
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), stage);
        let chunk = NodeToMaster::ArtifactChunk {
            req_id: "r1".into(),
            stage_id: "s".into(),
            offset: 0,
            data: "aGk=".into(),
            eof: true,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&chunk).unwrap(),
            json!({"type": "artifact.chunk", "req_id": "r1", "stage_id": "s",
                   "offset": 0, "data": "aGk=", "eof": true})
        );
        let busy: NodeToMaster = serde_json::from_value(json!({"type": "artifact.chunk",
            "req_id": "r", "stage_id": "s", "offset": 5, "eof": false, "error": "busy"}))
        .unwrap();
        assert!(
            matches!(busy, NodeToMaster::ArtifactChunk { data, error: Some(e), .. }
            if data.is_empty() && e == "busy")
        );
    }

    /// X.12: `session.diff` is answered by its result, never acked; the
    /// result carries the diff or the node's words.
    #[test]
    fn diff_messages_wire_shapes() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let ask = MasterToNode::Diff {
            req_id: "r".into(),
            session_id: sid,
            turn_id: "t1".into(),
            rewind: false,
        };
        assert_eq!(ask.req_id(), None);
        let v = serde_json::to_value(&ask).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.diff", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "turn_id": "t1"})
        );
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), ask);
        let diff = TurnDiff {
            files: vec![DiffFile {
                path: "a.txt".into(),
                old_path: None,
                status: DiffStatus::Modified,
                content: DiffContent::Text,
                old_mode: Some("100644".into()),
                new_mode: Some("100644".into()),
                old_size: Some(4),
                new_size: Some(8),
                added: 1,
                removed: 0,
                hunks: vec![DiffHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                    section: String::new(),
                    lines: vec![" one".into(), "+two".into()],
                }],
                cut: false,
            }],
            ..Default::default()
        };
        let ans = NodeToMaster::DiffResult {
            req_id: "r".into(),
            diff: Some(Box::new(diff)),
            error: None,
        };
        let v = serde_json::to_value(&ans).unwrap();
        assert_eq!(v["type"], "session.diff_result");
        assert_eq!(v["diff"]["files"][0]["status"], "modified");
        assert_eq!(v["diff"]["files"][0]["hunks"][0]["lines"][1], "+two");
        assert!(v["diff"].get("cut").is_none(), "{v}");
        assert_eq!(serde_json::from_value::<NodeToMaster>(v).unwrap(), ans);
    }

    #[test]
    fn prompt_images_wire_shape() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let img = ChatImage {
            media_id: "m".into(),
            sha256: "ab".repeat(32),
            mime: "image/png".into(),
            name: "a.png".into(),
            width: 2,
            height: 1,
            data: "aGk=".into(),
        };
        let p = MasterToNode::Prompt {
            req_id: "r".into(),
            session_id: sid,
            text: "look".into(),
            agent: None,
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply: None,
            images: vec![img.clone()],
            agent_named: false,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v["images"],
            json!([{"media_id": "m", "sha256": "ab".repeat(32), "mime": "image/png",
                    "name": "a.png", "width": 2, "height": 1, "data": "aGk="}])
        );
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), p);
        let r = img.image_ref();
        assert_eq!((r.media_id.as_str(), r.width, r.height), ("m", 2, 1));
    }

    #[test]
    fn prompt_reply_roundtrips_and_is_omitted_when_absent() {
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let prompt = |reply| MasterToNode::Prompt {
            req_id: "r".into(),
            session_id: sid,
            text: String::new(),
            agent: None,
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply,
            images: vec![],
            agent_named: false,
        };
        let v = serde_json::to_value(prompt(None)).unwrap();
        assert!(v.get("reply").is_none(), "{v}");
        assert!(v.get("images").is_none(), "{v}");
        let p = prompt(Some(harness_core::event::FormReply {
            message_id: "m1".into(),
            block_id: "db".into(),
            values: json!({"selected": ["pg"]}).as_object().unwrap().clone(),
        }));
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v["reply"],
            json!({"message_id": "m1", "block_id": "db", "values": {"selected": ["pg"]}})
        );
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), p);
    }

    /// T.8d step 3: `session.callback` carries ids and the agent, nothing a
    /// person writes; `session.create` says a chat takes no callbacks only
    /// when it is so.
    #[test]
    fn callback_and_no_callbacks_wire_shapes() {
        // A busy chat's refusal is one that finds nothing to resume now.
        assert!(CALLBACK_BUSY.starts_with(CALLBACK_NOTHING));
        let sid: SessionId = "0193f2a0-0000-7000-8000-000000000001".parse().unwrap();
        let c = MasterToNode::Callback {
            req_id: "r".into(),
            session_id: sid,
            bg_ids: vec!["bg-1".into(), "bg-2".into()],
            agent: None,
            savings: None,
            context: None,
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v,
            json!({"type": "session.callback", "req_id": "r",
                   "session_id": "0193f2a0-0000-7000-8000-000000000001", "bg_ids": ["bg-1", "bg-2"]})
        );
        assert_eq!(c.req_id(), Some("r"));
        assert_eq!(serde_json::from_value::<MasterToNode>(v).unwrap(), c);
        // Text, attachments or context are not fields of it: ignored.
        let extra: MasterToNode = serde_json::from_value(json!({"type": "session.callback",
            "req_id": "r", "session_id": sid, "bg_ids": ["bg-1"], "text": "do it"}))
        .unwrap();
        assert!(matches!(extra, MasterToNode::Callback { .. }));
        let create = |no_callbacks| MasterToNode::Create {
            exec_mode: None,
            req_id: "r".into(),
            session_id: sid,
            title: "t".into(),
            cwd: "/w".into(),
            model: None,
            approval_mode: None,
            profile: None,
            agent: None,
            incognito: false,
            no_callbacks,
        };
        assert!(
            serde_json::to_value(create(false))
                .unwrap()
                .get("no_callbacks")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(create(true)).unwrap()["no_callbacks"],
            true
        );
    }
}

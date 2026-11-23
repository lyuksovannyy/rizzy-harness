//! One session: the single writer of its log, the running turn, and the
//! approval gate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use harness_core::agent::AgentSpec;
use harness_core::backend::{LogError, TurnCtx, TurnExtra, TurnHost};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ToolRisk};
use harness_core::refs::SharedRefs;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId, new_id, now_ms};
use harness_protocol::NodeToMaster;
use harness_store::{AppendOutcome, StoreError};
use serde_json::Value;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::Inner;

/// An event to append: scrubbed on the way (`Raw`), or already scrubbed
/// the way the log stores it (`Scrubbed`: the person's message, which the
/// model is sent as it was scrubbed).
pub(crate) enum Append {
    Raw(EventKind),
    Scrubbed(EventKind),
}

impl From<EventKind> for Append {
    fn from(k: EventKind) -> Self {
        Append::Raw(k)
    }
}

/// The master's context of a queued message ([`Session::queued_context`]).
struct QueuedContext {
    /// The agent the master computed it for.
    agent: Option<String>,
    /// The turn that was running when the message was queued.
    turn: String,
    context: Option<String>,
    standing: Option<Vec<harness_core::backend::StandingPart>>,
}

impl QueuedContext {
    /// Whether a turn of `agent` that starts after turn `ended` may be
    /// sent it: only when it was queued while `ended` ran and computed for
    /// that agent (review H1). After a Stop the next turn to end is
    /// another (a callback turn, the person's next), so it is not used.
    fn usable(&self, ended: &str, agent: Option<&str>) -> bool {
        self.turn == ended && self.agent.as_deref() == agent
    }
}

struct RunningTurn {
    turn_id: String,
    cancel: CancellationToken,
    /// Sub-agents started by this turn use the same token-saving methods.
    savings: harness_core::savings::Savings,
    /// This turn's context from the master (the person's settings, and
    /// the standing parts: memory, the project's brief and board): its
    /// sub-agents get it too.
    context: Option<String>,
    standing: Option<Vec<harness_core::backend::StandingPart>>,
    /// Files this turn published (`publish` is capped per turn).
    publishes: Arc<std::sync::atomic::AtomicUsize>,
    /// Set when the node's stop cancelled this turn (and nothing had
    /// before it): its end is then `stopped` (review N-7).
    by_stop: Arc<AtomicBool>,
    /// The agent running it, as the master resolved it (X.5: a message
    /// naming another agent does not wait for this turn).
    agent_id: Option<String>,
    /// A callback turn: it is handed no message the person sends meanwhile
    /// (review ST-2); the person's own turn after it reads them.
    callback: bool,
    /// Parallel answers: they take no message while they run (ST-7).
    compare: bool,
    /// When it began; a sub-agent's turn has its parent's (B.9, T65:
    /// what changed since then is not taken as instructions in it).
    began: std::time::SystemTime,
    /// What a sub-agent's run is part of, set only where the node starts
    /// that run for its parent's `delegate` call (`start_child`,
    /// `start_continue`). `None` for every other turn: a person's message,
    /// a retry, a callback. Only a turn with one gets its chat's MCP
    /// servers ([`Session::chat_mcp`]); it goes with the turn's slot, so
    /// the session's next turn has none.
    delegated_by: Option<Delegation>,
}

/// See [`RunningTurn::delegated_by`].
#[derive(Clone, PartialEq, Eq)]
enum Delegation {
    /// A foreground run of the parent's turn with this id, which waits for
    /// it: its calls land inside that turn.
    Foreground(String),
    /// A background run (X.24) the node started for its chat's `delegate`
    /// call with `background: true`, or a background follow-up: only a
    /// chat's own turn starts one (`may_start_background`). Its calls can
    /// land between the chat's turns, in no checkpoint (the owner accepted
    /// it on 2026-11-18, PLAN.md T47, T72).
    Background,
}

/// How a sub-agent's run is started: in the foreground (its parent's turn
/// waits for it) or in the background with its own token cap (X.24).
#[derive(Clone, Copy)]
pub(crate) enum ChildRun {
    Foreground,
    Background { max_tokens: u64 },
}

impl ChildRun {
    fn max_tokens(self) -> Option<u64> {
        match self {
            ChildRun::Foreground => None,
            ChildRun::Background { max_tokens } => Some(max_tokens),
        }
    }
}

/// The running turn as a message sent meanwhile sees it.
struct Running {
    turn_id: String,
    cancelled: bool,
    agent_id: Option<String>,
    compare: bool,
}

/// Where a person's answer to a waiting approval goes: the decision and the
/// note that came with a denial.
type Answer = oneshot::Sender<(Decision, Option<String>)>;

pub(crate) struct Session {
    node: Arc<Inner>,
    id: SessionId,
    /// Serializes appends: holds the current head seq.
    head: tokio::sync::Mutex<u64>,
    cwd: PathBuf,
    model: String,
    /// The chat's approval mode (T50): set at create, changed only by
    /// [`Self::set_mode`] (the owner's command, recorded as
    /// `approval_mode_changed`), rebuilt from the log at load. Read at
    /// every approval, so a change applies from the next call.
    mode: ModeCell,
    /// Held while a mode change is recorded and applied, so the mode in
    /// force is always the log's latest.
    mode_change: tokio::sync::Mutex<()>,
    /// The chat's own step limit (X.14): `None` until the owner sets one
    /// ([`Self::set_step_limit`], recorded as `step_limit_changed`),
    /// rebuilt from the log at load. Read before every model request of a
    /// person's turn, so a change reaches the running turn.
    steps: Mutex<Option<usize>>,
    /// Calls the Accept edits rule let through, by call id, until the loop
    /// asks ([`TurnHost::confined`]).
    confined: Mutex<std::collections::HashSet<String>>,
    profile: Profile,
    incognito: bool,
    /// Set on a delegated sub-task's session; limits what its turns may do.
    parent_session: Option<SessionId>,
    /// A chat that never gets a callback turn (`session_created`).
    no_callbacks: bool,
    turn: Mutex<Option<RunningTurn>>,
    approvals: Mutex<HashMap<String, Answer>>,
    /// The mode the log says now (`session_created`, the latest
    /// `approval_mode_changed`): a turn that starts with another in force
    /// (node.toml's ceiling lowered since) records the change first
    /// (`record_clamp`), so the log, the master and the UI tell the truth.
    recorded: Mutex<ApprovalMode>,
    /// Waiting approvals denied because the chat entered plan mode (X.7):
    /// recorded as resolved by policy, never as the person's answer.
    plan_denied: Mutex<std::collections::HashSet<String>>,
    /// The person's notes with denials, by call id, until the loop takes
    /// the one of the call it is closing ([`TurnHost::denial_note`]).
    deny_notes: Mutex<HashMap<String, String>>,
    /// Calls the person allowed for the rest of this chat, by grant id
    /// (`chat_allow_added`, less `chat_allow_removed`): their `call_key`.
    /// Rebuilt from the log at load; a sub-agent's chat has none and
    /// takes none from its parent.
    grants: Mutex<HashMap<String, harness_tools::CallKey>>,
    /// The custom connectors' tools the running turn's spec carried (X.35
    /// step 2): the approval card names a call's connector and the
    /// server's own name of the tool from them.
    connector_tools: Mutex<Vec<harness_core::connectors::ConnectorToolEntry>>,
    /// An append of the running turn failed: the rest of its appends fail
    /// without being tried, so the turn ends (cleared when a turn starts).
    log_failed: AtomicBool,
    /// A turn's last events could not be written: its end is still missing
    /// from the log, and the next prompt writes it first.
    unclosed: AtomicBool,
    /// Approvals of the running turn whose request is in the log and whose
    /// answer is not: the turn's end denies them.
    unresolved: Mutex<Vec<String>>,
    /// The folder's last recorded state in this session (a turn's `after`
    /// or a restore's result): the next turn copies what the person changed
    /// since then (A.17 review, F5).
    last_state: Mutex<Option<String>>,
    /// The folder pinned at creation (or at load after a restart): its
    /// canonical path, device and inode. `None` for a chat session, and for
    /// an agentic one whose folder could not be pinned (it then refuses to
    /// work). Checked by [`Self::checked_folder`] before every use. Set
    /// again only when a writing sub-agent is continued in a fresh copy of
    /// its parent's folder at the same path (T68), before its turn starts.
    pin_cell: Mutex<Option<harness_core::folder::FolderPin>>,
    /// A sub-agent's node-owned folder (`git::ChildFolder`): the session
    /// works in it although it lies in the node's protected sessions
    /// folder. Not kept across a restart (the folder is removed when the
    /// sub-agent ends); set again with `pin_cell` when it is continued.
    owned_cell: Mutex<Option<harness_core::folder::OwnedFolder>>,
    /// How its commands run (N.10a): from its `session_created`, then the
    /// latest `exec_mode_changed` (N.10b: [`Self::set_exec_mode`], the
    /// owner's switch while nothing of the chat runs; the rule is
    /// `harness_core::exec_mode`); `None` for a chat session, and for a log
    /// whose mode cannot be read (it runs nothing). Read through
    /// [`Self::exec_mode_now`]. Checked against what the node offers
    /// before every turn, callback, restore and delegation.
    exec_mode: Mutex<Option<harness_core::policy::ExecMode>>,
    /// The mode this chat's commands ran in when its last turn started, for
    /// the line that tells the model the mode changed since
    /// ([`Self::environment_line`]); `None` before any turn.
    last_turn_mode: Mutex<Option<harness_core::policy::ExecMode>>,
    /// This session's background commands (T.8d): an agentic session that
    /// is not a sub-agent's. Dropping it stops every one.
    bg: Option<Arc<harness_tools::background::SessionBg>>,
    /// This session's MCP servers (X.3), from node.toml, and the
    /// repository's that its owner approved (X.36, given before each turn):
    /// an agentic session that is not a sub-agent's, on a node that has
    /// some or takes a repository's. Started by the first turn that offers
    /// their tools; dropping it stops them.
    mcp: Option<Arc<harness_tools::mcp::SessionMcp>>,
    /// The account that owns the chat, as the master named it with the
    /// last turn command (`owner` on a prompt, retry or callback, X.36):
    /// whose approved repository servers its table takes. In memory only:
    /// after a restart it is learned again from the next turn.
    owner: Mutex<Option<String>>,
    /// The identity this chat's commits carry (T88), as the master named it
    /// with the last turn command and this node resolved it: none until a
    /// turn names one (a chat nobody owns never does). In memory only,
    /// never logged. A sub-agent's session reads its chat's
    /// ([`Self::commit_identity`]).
    identity: Mutex<Option<harness_core::identity::Identity>>,
    /// The chat's folder's `.mcp.json` was read once for this chat (when
    /// its owner was first learned, X.36): never again for it.
    looked: AtomicBool,
    /// The id of the first turn the delegation that created this sub-agent's
    /// chat may start in it (set by `start_child`, in memory only). A
    /// sub-agent's chat in whose log no turn has ever started takes a first
    /// turn only with this id: see [`Self::guard_first_turn`].
    delegation_turn: Mutex<Option<String>>,
    /// Set while this chat's execution mode is being switched
    /// (`set_exec_mode`): a sub-agent's chat under it takes no turn
    /// meanwhile (`sub_agent_mode_mismatch`), so none starts in the mode the
    /// chat is leaving.
    pub(super) exec_switching: AtomicBool,
    /// Ends that came while a turn ran: recorded at its next step boundary
    /// (`delivered`), or after its end.
    bg_queue: Mutex<Vec<harness_tools::background::Ended>>,
    /// Wakes the end recorder when the turn slot is freed.
    bg_flush: Arc<tokio::sync::Notify>,
    /// The last try to record a queued end failed (T8D-6).
    bg_failed: AtomicBool,
    /// Background sub-agents (X.24) whose run ended and whose end is not
    /// recorded yet, by `bg-<n>`: the merge-back and the result wait for
    /// the end's delivery (a step boundary, or between turns).
    bg_children: Mutex<HashMap<String, ChildEnd>>,
    /// Held while an end is checked against the turn slot and recorded,
    /// and while a turn reads its history and records `turn_started`: no
    /// end lands between a turn's read and its start (review S3-4).
    bg_gate: tokio::sync::Mutex<()>,
    /// Held from a turn's claim of the slot to its start in the log, while
    /// a message sent during a turn is recorded, and while queued messages
    /// are handed to the model (X.5): a queued message always lands after
    /// the `turn_started` of the turn it names, and `pending` matches the
    /// log.
    queue: tokio::sync::Mutex<()>,
    /// The person's messages sent while a turn ran that the model has not
    /// read, with their seqs: recorded (`user_message.queued`), read before
    /// the turn's next model request, or by the next turn (X.5). Rebuilt
    /// from the log at load.
    pending: Mutex<Vec<(u64, UserMessage)>>,
    /// The master's context of the newest message that waits in `pending`
    /// (computed for that message): a turn that only reads queued messages
    /// is sent with it. Kept in memory only: after a restart such a turn
    /// has none, and reads no memory (X.1, review F2).
    queued_context: Mutex<Option<QueuedContext>>,
    /// The environment line the last `turn_started` recorded, by turn id,
    /// for that turn's run to send (`TurnExtra::environment`).
    turn_env: Mutex<Option<(String, String)>>,
    /// The hand-off note being written (X.16), if one is: a turn that
    /// claims the slot, a Stop and a delete cancel it, so it never holds
    /// the chat against the person.
    note: Mutex<Option<CancellationToken>>,
    /// When this chat's last notes were started, for `[limits]
    /// handoff_notes_per_hour` (in memory: a restart gives one more hour).
    note_tries: Mutex<std::collections::VecDeque<std::time::Instant>>,
    /// A person's `/compact` running (X.20): no turn starts meanwhile
    /// (`claim_turn` refuses with `COMPACTING`), and Stop or a delete
    /// cancels it. Set and read under `turn`'s lock.
    compacting: Mutex<Option<CancellationToken>>,
    /// The agent and settings the last person's turn would read its
    /// queued messages with (`read_queued`): a `/compact` that ends with
    /// messages pending starts that turn (X.20 review M1). None after a
    /// restart: those messages wait for the person's next one.
    reader: Mutex<Option<(Option<AgentSpec>, TurnExtra)>>,
    /// The agents a person's message called after the first, still to be
    /// asked (X.37): in memory only, dropped by a restart (which says so,
    /// `recover_peers`).
    peers: Mutex<Option<peers::Peers>>,
}

/// The most messages that wait at once for the running turn to read them
/// (X.5); one more is refused.
pub(crate) const MAX_QUEUED: usize = 8;

/// The refusal of a second turn while one runs.
const TURN_RUNNING: &str = "a turn is already running";

/// The queued messages in `history` that no `queued_delivered` names, in
/// the order they were sent.
fn unread(history: &[RawEvent]) -> Vec<(u64, UserMessage)> {
    let mut out: Vec<(u64, UserMessage)> = Vec::new();
    for e in history {
        match e.typed() {
            Ok(EventKind::UserMessage(m)) if m.queued => out.push((e.seq, m)),
            Ok(EventKind::QueuedDelivered(d)) => out.retain(|(s, _)| !d.seqs.contains(s)),
            _ => {}
        }
    }
    out
}

type BgEnds = tokio::sync::mpsc::UnboundedReceiver<harness_tools::background::Ended>;

/// A session's background table and the receiver of its ends, where it
/// gets one; `next` is the number its next id takes. A `host` session gets
/// one too (the owner, 2026-11-20, PLAN.md T9): its background command runs
/// under the guard without bubblewrap. A sub-agent's and a chat's (not
/// agentic) session gets none.
fn background_table(
    node: &Inner,
    profile: Profile,
    parent: Option<SessionId>,
    next: u64,
) -> (
    Option<Arc<harness_tools::background::SessionBg>>,
    Option<BgEnds>,
) {
    if profile != Profile::Agentic || parent.is_some() {
        return (None, None);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Some(node.background.session(next, tx)), Some(rx))
}

/// Whether `bg_ids` may resume a chat whose log is `history` (T.8d step
/// 3, §4.4 step 5), and what the turn is for. Each id must name an end
/// that waits ([`harness_core::event::waiting_ends`]: it may call back and
/// no person's turn, refusal or callback turn naming it came after it).
/// `Ok` holds the callback turns since the person's last message (the
/// budget spent) and the ids the turn names: those asked for, then every
/// other end that waits, oldest first, up to `MAX_CALLBACK_IDS` in all.
/// The master read its mirror a moment before (its settle, a check that
/// ran before the latest end); the node, the log's one writer, names what
/// its log holds now, under the slot and the gate, so every end the
/// turn's history holds as waiting is one the turn is for. An end left
/// out by the bound stays waiting for the next callback. `Err` says what
/// is not there, behind [`harness_protocol::CALLBACK_NOTHING`].
fn callback_check(history: &[RawEvent], bg_ids: &[String]) -> Result<(usize, Vec<String>), String> {
    use harness_protocol::{CALLBACK_NOTHING, MAX_CALLBACK_IDS};
    let mut in_a_row = 0usize;
    for e in history {
        match e.typed() {
            // Only callback turns spend the budget: an agent a person's
            // message called (X.37) is the person's own turn here.
            Ok(EventKind::TurnStarted(t))
                if t.cause.as_ref().is_some_and(TurnCause::is_callback) =>
            {
                in_a_row += 1
            }
            Ok(EventKind::UserMessage(_)) => in_a_row = 0,
            _ => {}
        }
    }
    let waiting = harness_core::event::waiting_ends(history);
    for id in bg_ids {
        if !waiting.iter().any(|w| &w.2.bg_id == id) {
            return Err(format!(
                "{CALLBACK_NOTHING}: {id} has no end in this chat that waits for a turn"
            ));
        }
    }
    let mut named = bg_ids.to_vec();
    for w in waiting {
        if named.len() >= MAX_CALLBACK_IDS {
            break;
        }
        if !named.contains(&w.2.bg_id) {
            named.push(w.2.bg_id);
        }
    }
    Ok((in_a_row, named))
}

/// A session's MCP servers (see [`Session::mcp`]): node.toml's, for an
/// agentic session that is not a sub-agent's, started in this chat's own
/// folder and confinement (`home`) whoever's turn starts them. A
/// sub-agent has none of its own: the turns the node started for a
/// `delegate` call (foreground or background) use its chat's
/// (`Session::chat_table`, X.34 step C); a person's message there gets
/// none.
fn mcp_servers(
    node: &Inner,
    profile: Profile,
    parent: Option<SessionId>,
    id: SessionId,
    cwd: &std::path::Path,
    pin: Option<harness_core::folder::FolderPin>,
) -> Option<Arc<harness_tools::mcp::SessionMcp>> {
    if profile != Profile::Agentic || parent.is_some() {
        return None;
    }
    let home = harness_tools::mcp::Home {
        key: id.to_string(),
        workspace: cwd.to_path_buf(),
        pin,
    };
    // A node that takes a repository's servers (X.36) gives every chat a
    // table, filled before each turn with its owner's approved copies.
    if node.project_mcp.is_some() {
        return Some(harness_tools::mcp::SessionMcp::at_always(
            &node.cfg.policy.mcp_servers,
            home,
        ));
    }
    harness_tools::mcp::SessionMcp::at(&node.cfg.policy.mcp_servers, home)
}

/// Pin an agentic session's folder (see [`Session::pin`]).
fn pin_of(profile: Profile, cwd: &std::path::Path) -> Option<harness_core::folder::FolderPin> {
    if profile != Profile::Agentic {
        return None;
    }
    match harness_core::folder::FolderPin::at(cwd) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(cwd = %cwd.display(), error = %e, "a session's folder could not be pinned");
            None
        }
    }
}

/// What the user sent with a prompt.
pub(crate) struct Input {
    pub text: String,
    pub attachments: Vec<Attachment>,
    /// An answer to a choices or form block: `text` is then the user's
    /// note, and the message records the canonical answer before it.
    pub reply: Option<FormReply>,
    /// Images, with their bytes (`session.prompt.images`).
    pub images: Vec<harness_protocol::ChatImage>,
    /// Paths the person mentioned with `@path` (`session.prompt.files`),
    /// read by [`Session::read_mentions`] (T63).
    pub files: Vec<String>,
    /// Files pushed ahead of this prompt (`session.upload`), which the
    /// node saves into the chat's folder (T94, `uploads.rs`).
    pub uploads: Vec<harness_protocol::UploadRef>,
    /// Files the master keeps for a plain chat (`session.prompt.attached`,
    /// T95): recorded in the message, never written anywhere here.
    pub attached: Vec<harness_protocol::AttachedRef>,
}

/// A sub-agent's private, node-owned folder.
struct Isolation {
    sub: crate::git::SubRepo,
    child: crate::git::ChildFolder,
    base: String,
    cwd: PathBuf,
}

/// A child session with its turn started and not run yet
/// ([`Session::start_child`], [`Session::start_continue`]).
struct Prepared {
    child: Arc<Session>,
    iso: Option<Isolation>,
    /// It can change files and works in its parent's folder
    /// (`folder: "shared"`, PLAN.md T32): its result says so, and which
    /// files changed there.
    shared: bool,
    msg: UserMessage,
    history: Vec<RawEvent>,
    agent: AgentSpec,
    extra: TurnExtra,
    cancel: CancellationToken,
    by_stop: Arc<AtomicBool>,
}

mod bg_agents;
use bg_agents::ChildEnd;

/// One chat's approval mode (read by its sub-agents through the node's
/// session table, `Session::current_mode`).
/// The reason recorded with a card denied because the chat entered plan
/// mode while it waited (X.7).
const PLAN_CARD_DENIED: &str = "Plan mode started while this call waited: in plan mode nothing \
     is changed, so it was not run.";

/// Why a sub-agent's changes were kept out of a folder whose chat is in
/// plan mode (X.7) when the child ended.
const PLAN_NOT_APPLIED: &str = "the chat is in plan mode, in which nothing is changed in its \
     folder; carry them out after the person leaves plan mode";

pub(crate) type ModeCell = Arc<Mutex<ApprovalMode>>;

fn read_mode(c: &ModeCell) -> ApprovalMode {
    *c.lock().unwrap_or_else(|p| p.into_inner())
}

/// Why a mode change is refused, or what it becomes (T50).
pub(crate) const MODE_SUB_AGENT: &str =
    "a sub-agent runs in the approval mode of the chat that started it; change that chat's mode";
pub(crate) const MODE_ROUTINE: &str =
    "a routine's chat keeps the routine's approval setting; change it in the routine";
pub(crate) const MODE_CHAT: &str = "a chat without tools has no approval mode to change";

/// Why a step-limit change is refused (X.14).
pub(crate) const STEPS_SUB_AGENT: &str =
    "a sub-agent's steps follow the chat that started it; change that chat's step limit";
pub(crate) const STEPS_ROUTINE: &str = "a routine's chat keeps the node's step limit";
/// A sub-agent's chat whose delegation never started its task (the first
/// turn's start failed) takes no turn from anyone else.
pub(crate) const NEVER_STARTED: &str = "this sub-agent never started its task: delegate again \
     from the chat that started it";
pub(crate) const STEPS_CHAT: &str = "a chat without tools has no step limit to change";

/// Everything needed to start a session, already validated by the runtime.
pub(crate) struct NewSession {
    pub title: String,
    pub cwd: PathBuf,
    pub model: String,
    pub mode: ApprovalMode,
    pub profile: Profile,
    pub agent_id: Option<String>,
    pub incognito: bool,
    pub parent_session: Option<SessionId>,
    /// A sub-agent's node-owned folder, when it has one.
    pub owned: Option<harness_core::folder::OwnedFolder>,
    /// Never resumed by a callback turn (a routine's chat).
    pub no_callbacks: bool,
    /// Chosen by the runtime (or the parent's, for a sub-agent).
    pub exec_mode: Option<harness_core::policy::ExecMode>,
    /// Where a sub-agent that can change files works (recorded).
    pub sub_folder: Option<harness_core::subagent::SubFolder>,
}

impl Session {
    pub(crate) async fn create(
        node: Arc<Inner>,
        id: SessionId,
        new: NewSession,
    ) -> Result<Arc<Self>, StoreError> {
        let NewSession {
            title,
            cwd,
            model,
            mode,
            profile,
            agent_id,
            incognito,
            parent_session,
            owned,
            no_callbacks,
            exec_mode,
            sub_folder,
        } = new;
        let (bg, ends) = background_table(&node, profile, parent_session, 1);
        let s = Arc::new(Self {
            bg,
            mcp: mcp_servers(
                &node,
                profile,
                parent_session,
                id,
                &cwd,
                pin_of(profile, &cwd),
            ),
            bg_queue: Mutex::new(Vec::new()),
            bg_flush: Arc::default(),
            bg_failed: AtomicBool::new(false),
            bg_children: Mutex::default(),
            bg_gate: tokio::sync::Mutex::new(()),
            queue: tokio::sync::Mutex::new(()),
            pending: Mutex::new(Vec::new()),
            queued_context: Mutex::new(None),
            turn_env: Mutex::new(None),
            note: Mutex::new(None),
            note_tries: Mutex::default(),
            compacting: Mutex::new(None),
            reader: Mutex::new(None),
            peers: Mutex::new(None),
            node: node.clone(),
            id,
            head: tokio::sync::Mutex::new(0),
            cwd: cwd.clone(),
            model: model.clone(),
            mode: Arc::new(Mutex::new(mode)),
            mode_change: tokio::sync::Mutex::new(()),
            steps: Mutex::new(None),
            confined: Mutex::new(std::collections::HashSet::new()),
            profile,
            incognito,
            parent_session,
            no_callbacks,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
            recorded: Mutex::new(mode),
            plan_denied: Mutex::new(std::collections::HashSet::new()),
            deny_notes: Mutex::new(HashMap::new()),
            grants: Mutex::new(HashMap::new()),
            connector_tools: Mutex::new(Vec::new()),
            log_failed: AtomicBool::new(false),
            unclosed: AtomicBool::new(false),
            unresolved: Mutex::new(Vec::new()),
            last_state: Mutex::new(None),
            pin_cell: Mutex::new(pin_of(profile, &cwd)),
            owned_cell: Mutex::new(owned),
            exec_mode: Mutex::new(exec_mode),
            last_turn_mode: Mutex::new(None),
            owner: Mutex::new(None),
            identity: Mutex::new(None),
            looked: AtomicBool::new(false),
            delegation_turn: Mutex::new(None),
            exec_switching: AtomicBool::new(false),
        });
        Self::record_ends(&s, ends);
        s.append(EventKind::SessionCreated(SessionCreated {
            forked: None,
            no_callbacks,
            profile,
            agent_id,
            incognito,
            parent_session,
            title,
            cwd: cwd.display().to_string(),
            model,
            backend: node.cfg.backend.name().to_string(),
            node_id: node.cfg.node_id.clone(),
            approval_mode: mode,
            exec_mode,
            sub_folder,
        }))
        .await?;
        Ok(s)
    }

    pub(crate) async fn load(node: Arc<Inner>, id: SessionId) -> Result<Arc<Self>, StoreError> {
        let events = node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await?;
        let view = SessionView::fold(&events)
            .map_err(|e| StoreError::Corrupt(format!("session {id}: {e}")))?;
        let meta = view
            .meta
            .clone()
            .ok_or_else(|| StoreError::Corrupt(format!("session {id} has no session_created")))?;
        // The log's latest mode (the fold applies `approval_mode_changed`),
        // clamped again: node.toml may have been tightened since.
        let mode = node.cfg.policy.effective_mode(Some(meta.approval_mode));
        // A fork's copied checkpoints are its parent's (C.2): the fork's
        // first turn has no state of its own to go from.
        let copied = harness_core::fork::through(&meta);
        let last_state = events.iter().rev().find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c))
                if !harness_core::fork::inherited(copied, e.seq)
                    && !matches!(
                        c.phase,
                        CheckpointPhase::Before | CheckpointPhase::BeforeRestore
                    ) =>
            {
                Some(c.git_ref)
            }
            _ => None,
        });
        // After a restart the inode is taken now, from the path the log
        // recorded (canonical when the session was made): a folder that now
        // leads elsewhere is not pinned, and the session refuses to work.
        let pin = pin_of(meta.profile, std::path::Path::new(&meta.cwd));
        // Ids go on from the log's.
        let next = view
            .background
            .keys()
            .filter_map(|k| k.strip_prefix("bg-")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        // The chat's grants as its log holds them: the key recorded when the
        // person answered, never one worked out again from the logged input
        // (stored scrubbed, and a path's link may lead elsewhere now: review
        // H1, H2). A grant without a valid key covers nothing.
        let mut grants: HashMap<String, harness_tools::CallKey> = HashMap::new();
        // None of a fork's copied events grants anything: grants are bound
        // to the parent's folder (C.2).
        for e in events
            .iter()
            .filter(|e| !harness_core::fork::inherited(copied, e.seq))
        {
            match e.typed() {
                Ok(EventKind::ChatAllowAdded(g)) => {
                    if let Some(k) = harness_tools::CallKey::from_hex(&g.key) {
                        grants.insert(g.grant_id, k);
                    }
                }
                Ok(EventKind::ChatAllowRemoved(g)) => {
                    grants.remove(&g.grant_id);
                }
                _ => {}
            }
        }
        // The mode the chat's commands were in when its last turn began.
        let last_turn_mode = {
            let mut now = harness_core::exec_mode::InForce::Unset;
            let mut at_turn = None;
            for e in &events {
                if e.kind == "turn_started" {
                    at_turn = now.mode();
                }
                now = harness_core::exec_mode::step(now, e);
            }
            at_turn
        };
        let (bg, ends) = background_table(&node, meta.profile, meta.parent_session, next);
        let s = Arc::new(Self {
            bg,
            mcp: mcp_servers(
                &node,
                meta.profile,
                meta.parent_session,
                id,
                std::path::Path::new(&meta.cwd),
                pin.clone(),
            ),
            bg_queue: Mutex::new(Vec::new()),
            bg_flush: Arc::default(),
            bg_failed: AtomicBool::new(false),
            bg_children: Mutex::default(),
            bg_gate: tokio::sync::Mutex::new(()),
            queue: tokio::sync::Mutex::new(()),
            // Sent while a turn ran that a restart cut: the next turn reads
            // them (X.5).
            pending: Mutex::new(rewind_op::unread_visible(&events)),
            queued_context: Mutex::new(None),
            turn_env: Mutex::new(None),
            note: Mutex::new(None),
            note_tries: Mutex::default(),
            compacting: Mutex::new(None),
            reader: Mutex::new(None),
            peers: Mutex::new(None),
            node,
            id,
            head: tokio::sync::Mutex::new(view.head_seq),
            cwd: PathBuf::from(meta.cwd),
            model: meta.model,
            mode: Arc::new(Mutex::new(mode)),
            mode_change: tokio::sync::Mutex::new(()),
            // The log's latest as it is: `current_steps` clamps it by
            // node.toml as it is now, so a node limit raised again later
            // brings back exactly what the owner chose (review F7).
            steps: Mutex::new(view.max_steps.map(|n| (n as usize).max(1))),
            confined: Mutex::new(std::collections::HashSet::new()),
            profile: meta.profile,
            incognito: meta.incognito,
            parent_session: meta.parent_session,
            no_callbacks: meta.no_callbacks,
            turn: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
            recorded: Mutex::new(meta.approval_mode),
            plan_denied: Mutex::new(std::collections::HashSet::new()),
            deny_notes: Mutex::new(HashMap::new()),
            grants: Mutex::new(grants),
            connector_tools: Mutex::new(Vec::new()),
            log_failed: AtomicBool::new(false),
            unclosed: AtomicBool::new(false),
            unresolved: Mutex::new(Vec::new()),
            last_state: Mutex::new(last_state),
            pin_cell: Mutex::new(pin),
            owned_cell: Mutex::new(None),
            // As the log says now, never re-chosen: one the node no longer
            // offers runs nothing (`checked_folder`), and one the log
            // names but this build cannot read is `None` (fails closed).
            exec_mode: Mutex::new(match view.exec_mode {
                harness_core::exec_mode::InForce::Mode(m) => Some(m),
                _ => None,
            }),
            last_turn_mode: Mutex::new(last_turn_mode),
            owner: Mutex::new(None),
            identity: Mutex::new(None),
            looked: AtomicBool::new(false),
            delegation_turn: Mutex::new(None),
            exec_switching: AtomicBool::new(false),
        });
        Self::record_ends(&s, ends);
        Ok(s)
    }

    /// The owner a turn command names (X.36, PLAN.md T82), before its turn
    /// runs: kept for this chat's table. The first time this chat learns an
    /// owner who approved something for its folder (since the node
    /// started), the folder's `.mcp.json` is read once, for the names it
    /// holds; never again for this chat, never per turn. A read that does
    /// not end within `project_mcp::LOOK_WAIT` changes nothing.
    pub(crate) async fn learn_owner(
        &self,
        owner: Option<String>,
        identity: Option<harness_core::identity::CommitIdentity>,
    ) {
        // The node decides what a commit is signed with: whatever the
        // master sent is checked here again (`CommitIdentity::resolve`).
        *self.identity.lock().unwrap_or_else(|p| p.into_inner()) = identity.map(|i| i.resolve());
        *self.owner.lock().unwrap_or_else(|p| p.into_inner()) = owner.clone();
        let (Some(pm), Some(owner), Some(_)) = (&self.node.project_mcp, owner, &self.mcp) else {
            return;
        };
        if !pm.has_approvals(&owner, &self.cwd) || self.looked.swap(true, Ordering::SeqCst) {
            return;
        }
        let (pm, cwd) = (pm.clone(), self.cwd.clone());
        let read = tokio::task::spawn_blocking(move || pm.look(&cwd));
        let _ = tokio::time::timeout(crate::project_mcp::LOOK_WAIT, read).await;
    }

    /// The task that records this session's background ends, one at a
    /// time: at once when no turn runs, else queued for the turn's next
    /// step boundary or its end. It holds the session weakly and ends with
    /// its table.
    fn record_ends(s: &Arc<Self>, ends: Option<BgEnds>) {
        let Some(mut rx) = ends else { return };
        let weak = Arc::downgrade(s);
        let flush = s.bg_flush.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    m = rx.recv() => {
                        let Some(e) = m else { break };
                        let Some(s) = weak.upgrade() else { break };
                        s.on_background_end(e).await;
                    }
                    _ = flush.notified() => {
                        let Some(s) = weak.upgrade() else { break };
                        s.flush_background().await;
                    }
                }
            }
        });
    }

    async fn on_background_end(&self, e: harness_tools::background::Ended) {
        let Some(bg) = &self.bg else { return };
        if e.reason == BackgroundReason::SessionDeleted {
            // A background sub-agent's folder goes too, unmerged (X.24).
            self.drop_child_end(&e.bg_id).await;
            bg.recorded();
            return;
        }
        let _gate = self.bg_gate.lock().await;
        {
            let turn = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            if turn.is_some() {
                self.bg_queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(e);
                return;
            }
        }
        #[cfg(feature = "testing")]
        crate::testing::end_hold(self.id).await;
        self.record_background(vec![e]).await;
    }

    /// Ends queued during a turn, once no turn runs.
    async fn flush_background(&self) {
        let _gate = self.bg_gate.lock().await;
        let ends = {
            let turn = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            if turn.is_some() {
                return;
            }
            std::mem::take(&mut *self.bg_queue.lock().unwrap_or_else(|p| p.into_inner()))
        };
        self.record_background(ends).await;
    }

    /// Record ends outside a turn (`delivered: false`: the model reads them
    /// in the next turn's history). One that cannot be written stays queued
    /// for the next try; it is never dropped silently.
    async fn record_background(&self, ends: Vec<harness_tools::background::Ended>) {
        let Some(bg) = &self.bg else { return };
        for e in ends {
            // A background sub-agent's end is merged and built here (X.24).
            let ev = self.end_event(&e, false).await;
            #[cfg(feature = "testing")]
            let refused = crate::testing::take_fail_end(self.id);
            #[cfg(not(feature = "testing"))]
            let refused = false;
            let appended = if refused {
                Err(StoreError::Corrupt("refused by a test".into()))
            } else {
                self.append(EventKind::BackgroundEnded(ev.clone())).await
            };
            match appended {
                Ok(_) => {
                    bg.recorded();
                    self.bg_failed.store(false, Ordering::SeqCst);
                }
                Err(err) => {
                    self.keep_child_end(ev);
                    self.bg_failed.store(true, Ordering::SeqCst);
                    tracing::error!(session = %self.id, error = %err, bg = %e.bg_id, "could not record a background command's end; kept for the next try");
                    self.bg_queue
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(e);
                }
            }
        }
    }

    /// After a restart: every background command the log shows running
    /// died with the node (its guard ends it when its node is gone, the
    /// start's sweep the rest). Recorded as `node_restarted`; no turn
    /// starts for it (R12).
    pub(crate) async fn recover_background(&self) {
        let id = self.id;
        let events = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(session = %self.id, error = %e, "could not read the log for background commands");
                return;
            }
        };
        let Ok(view) = SessionView::fold(&events) else {
            return;
        };
        for bg_id in view.running_background() {
            // A background sub-agent's end says so (X.24): its result never
            // came, its folder is gone, and it can be continued.
            let child = view
                .background
                .get(&bg_id)
                .and_then(|b| b.child.clone())
                .map(|c| {
                    Box::new(ChildEnded {
                        session: c.session,
                        agent_id: c.agent_id,
                        result: String::new(),
                        ok: false,
                        usage: None,
                    })
                });
            let ended = BackgroundEnded {
                child,
                bg_id,
                reason: BackgroundReason::NodeRestarted,
                exit_code: None,
                signal: None,
                confirmed: BackgroundConfirmed::Unknown,
                ran_ms: 0,
                output_bytes: 0,
                tail: String::new(),
                tail_cut: false,
                delivered: false,
            };
            if let Err(e) = self.append(EventKind::BackgroundEnded(ended)).await {
                tracing::error!(session = %self.id, error = %e, "could not record a background command's end at start");
            }
        }
    }

    /// Stop one background command (`bg_id`) or all, for the master: the
    /// person's stop (step 2) or the revocation sweep. Not waited for.
    pub(crate) fn stop_background(
        &self,
        bg_id: Option<&str>,
        by: harness_protocol::StopBackgroundBy,
    ) -> Result<(), String> {
        let bg = self
            .bg
            .as_ref()
            .ok_or("this chat has no background commands")?;
        let reason = match by {
            harness_protocol::StopBackgroundBy::User => BackgroundReason::StoppedByUser,
            harness_protocol::StopBackgroundBy::AccessRevoked => BackgroundReason::AccessRevoked,
        };
        match (bg_id, by) {
            (Some(id), _) => bg.stop(id, reason).map(|_| ()),
            // A revoked owner's chat starts nothing more until they prompt
            // it again (a start in its first wait is stopped when handed
            // over).
            (None, harness_protocol::StopBackgroundBy::AccessRevoked) => {
                bg.close(reason);
                Ok(())
            }
            (None, harness_protocol::StopBackgroundBy::User) => {
                bg.stop_all(reason);
                Ok(())
            }
        }
    }

    /// Stop this session's MCP servers for good (it is being deleted, or
    /// the node is stopping) and wait for their processes, at most
    /// `bound`. True when they are gone.
    pub(crate) async fn close_mcp(&self, bound: std::time::Duration) -> bool {
        match &self.mcp {
            Some(m) => {
                m.close();
                m.wait_gone(bound).await
            }
            None => true,
        }
    }

    /// Order this session's MCP servers to stop for good; not waited for.
    pub(crate) fn close_mcp_now(&self) {
        if let Some(m) = &self.mcp {
            m.close();
        }
    }

    /// The MCP servers this session's turn may use (X.34 step C, PLAN.md
    /// T47): a chat's own; for a sub-agent, its chat's (the one at the top
    /// of its parents, as the node holds them now), the same table and
    /// processes, which run in the chat's folder, and only while the
    /// sub-agent's turn is a run the node started for its parent's
    /// `delegate` call: a foreground run of its parent's running turn, and
    /// so on up, or a background run of the chat's (X.24; the owner's
    /// decision of 2026-11-18: by its own switch; its calls can land
    /// between the chat's turns, in no checkpoint, T72). None for a
    /// person's message to a sub-agent's chat, a retry there (a callback
    /// never runs there), nor when a parent is not held.
    /// Whether the turn's agent may use them is the loop's to read.
    /// The table's lifecycle stays its chat's alone (`close_mcp`,
    /// `mcp_idle`, `mcp_running` read only `self.mcp`).
    fn chat_mcp(&self) -> Option<Arc<harness_tools::mcp::SessionMcp>> {
        self.chat_table(|s| s.mcp.clone()).flatten()
    }

    /// The chat's table as a turn takes it: first given its owner's
    /// approved copies of the repository's servers for the chat's folder
    /// as the node holds them now (X.36, PLAN.md T82: an approval taken or
    /// removed since, a name the last read of the file did not find, a
    /// `host` chat's, which starts them too). Nothing is read from disk here.
    fn chat_mcp_for_turn(&self) -> Option<Arc<harness_tools::mcp::SessionMcp>> {
        self.chat_table(|root| {
            root.sync_project_mcp();
            root.mcp.clone()
        })
        .flatten()
    }

    /// Give this chat's table its owner's approved copies as the node
    /// holds them now (X.36): before each turn, and at once for every chat
    /// of a folder after a person's request about it, so a removed
    /// approval or entry stops its server then. Nothing is read from disk.
    pub(crate) fn sync_project_mcp(&self) {
        let (Some(m), Some(pm)) = (&self.mcp, &self.node.project_mcp) else {
            return;
        };
        let owner = self.owner.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let (servers, notes) = pm.servers_for(&self.node.cfg.policy, owner.as_deref(), &self.cwd);
        m.set_project(&servers, notes);
    }

    /// This chat's folder, if it is a chat with a table (not a sub-agent's).
    pub(crate) fn table_folder(&self) -> Option<&std::path::Path> {
        self.mcp.as_ref().map(|_| self.cwd.as_path())
    }

    /// `f` of the chat at the top of this session's parents (itself for a
    /// chat) under [`Self::chat_mcp`]'s rule.
    fn chat_table<T>(&self, f: impl FnOnce(&Session) -> T) -> Option<T> {
        let Some(mut parent) = self.parent_session else {
            return Some(f(self));
        };
        // This turn, and each one above it, must be a foreground run of
        // the turn its parent is running now, or a background run of the
        // root chat, up to the root chat.
        let mut by = self.delegated_by()?;
        for _ in 0..8 {
            let s = self.node.session(parent)?;
            if s.id == self.id {
                return None;
            }
            match &by {
                Delegation::Foreground(turn) if s.running_turn_id()? != *turn => return None,
                // Only a chat starts one; never followed further up.
                Delegation::Background if s.parent_session.is_some() => return None,
                _ => {}
            }
            match s.parent_session {
                None => return Some(f(&s)),
                Some(p) => {
                    by = s.delegated_by()?;
                    parent = p;
                }
            }
        }
        None
    }

    /// The running turn's id.
    fn running_turn_id(&self) -> Option<String> {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| t.turn_id.clone())
    }

    /// What the running turn is a run of, if the node started it for a
    /// parent's `delegate` call.
    fn delegated_by(&self) -> Option<Delegation> {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|t| t.delegated_by.clone())
    }

    /// MCP server processes of this session still alive.
    pub(crate) fn mcp_running(&self) -> usize {
        self.mcp.as_ref().map_or(0, |m| m.running())
    }

    /// Stop the MCP servers no call used for `idle`.
    pub(crate) fn mcp_idle(&self, idle: std::time::Duration) {
        if let Some(m) = &self.mcp {
            m.stop_idle(idle);
        }
    }

    /// Order every background command to stop; not waited for.
    pub(crate) fn stop_background_now(&self, reason: BackgroundReason) {
        if let Some(bg) = &self.bg {
            bg.stop_all(reason);
        }
    }

    /// Stop every background command and wait for their ends, at most
    /// `bound`.
    pub(crate) async fn stop_background_within(
        &self,
        reason: BackgroundReason,
        bound: std::time::Duration,
    ) {
        if let Some(bg) = &self.bg {
            bg.stop_all_within(reason, bound).await;
        }
    }

    /// A background command runs or a start is being handed over.
    pub(crate) fn background_active(&self) -> bool {
        self.bg.as_ref().is_some_and(|b| b.active())
    }

    /// Ended background commands whose guard has not exited yet.
    pub(crate) fn background_stuck(&self) -> Vec<String> {
        self.bg.as_ref().map(|b| b.stuck()).unwrap_or_default()
    }

    /// Ends sent and not recorded yet (a stop waits for them).
    pub(crate) fn background_unrecorded(&self) -> bool {
        self.bg.as_ref().is_some_and(|b| b.unrecorded() > 0)
    }

    /// Try the queued ends again, when no turn runs (a stop's pass,
    /// T8D-6). True when an end is still not recorded and the last try
    /// failed: the log does not take it, and waiting longer buys nothing.
    pub(crate) async fn retry_background(&self) -> bool {
        self.flush_background().await;
        self.background_unrecorded() && self.bg_failed.load(Ordering::SeqCst)
    }

    /// Close out a turn that was running when the process died (PLAN.md §2).
    pub(crate) async fn recover_interrupted(&self) {
        let message = "The node restarted while this turn was running. Send a message to continue.";
        if let Err(e) = self
            .close_open_turn(message, TurnEndReason::Interrupted)
            .await
        {
            // The next prompt tries again.
            self.unclosed.store(true, Ordering::SeqCst);
            tracing::error!(session = %self.id, error = %e, "could not close the interrupted turn");
        }
        // After the turn's end: the agents of a message that were to be
        // asked next and never were (their specs died with the node).
        self.recover_peers().await;
    }

    /// End the turn the log shows running, if any: deny its pending
    /// approvals, say why, end it. Each event is tried once.
    async fn close_open_turn(&self, message: &str, reason: TurnEndReason) -> Result<(), String> {
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let view = SessionView::fold(&events).map_err(|e| e.to_string())?;
        let Some(turn_id) = view.running_turn else {
            return Ok(());
        };
        let fail = |e: StoreError| LogError(e.to_string()).to_string();
        for approval_id in view.pending_approvals {
            self.append(EventKind::ApprovalResolved(ApprovalResolved {
                approval_id,
                decision: Decision::Deny,
                by: ResolvedBy::Interrupted,
                note: None,
            }))
            .await
            .map_err(fail)?;
        }
        self.append(EventKind::Error(ErrorEvent {
            notice: false,
            turn_id: Some(turn_id.clone()),
            message: message.into(),
        }))
        .await
        .map_err(fail)?;
        self.append(EventKind::TurnEnded(TurnEnded {
            turn_id,
            reason,
            usage: None,
        }))
        .await
        .map_err(fail)?;
        Ok(())
    }

    /// Assign the next seq, scrub, persist (fsync), then publish.
    async fn append(&self, kind: impl Into<Append>) -> Result<RawEvent, StoreError> {
        let mut head = self.head.lock().await;
        self.append_at(&mut head, kind.into()).await
    }

    /// Append `kind` only if the log's head is still `expected` and `go`
    /// says yes under the lock (a hand-off note: nothing may have joined
    /// the log since what it covers, and no turn claimed the slot).
    /// `Ok(None)`: not appended.
    async fn append_if_head(
        &self,
        expected: u64,
        go: impl FnOnce() -> bool,
        kind: EventKind,
    ) -> Result<Option<RawEvent>, StoreError> {
        let mut head = self.head.lock().await;
        if *head != expected || !go() {
            return Ok(None);
        }
        self.append_at(&mut head, kind.into()).await.map(Some)
    }

    /// The one place a `turn_started` is written (`append_at`) asks this
    /// before it writes one in a sub-agent's chat: a sub-agent's chat in
    /// whose log no turn has ever started takes a first turn only from the
    /// delegation that created it (`delegation_turn`). Otherwise a chat
    /// whose first start failed (the log, a claim, a stop) would take a
    /// person's message as its "first" turn, and T32/T57 read a sub-agent's
    /// first turn as the lead's own work (`delegated_spans`). Every path
    /// that starts a turn goes through `append_at`, so none can forget it.
    async fn guard_first_turn(&self, turn_id: &str) -> Result<(), StoreError> {
        if self.parent_session.is_none() {
            return Ok(());
        }
        if self
            .delegation_turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_deref()
            == Some(turn_id)
        {
            return Ok(());
        }
        let id = self.id;
        let started = self
            .node
            .cfg
            .store
            .call(move |s| s.events_since_turn_at(id, i64::MAX, &["turn_started"]))
            .await?;
        if started.is_empty() {
            return Err(StoreError::Refused(NEVER_STARTED.into()));
        }
        Ok(())
    }

    async fn append_at(&self, head: &mut u64, kind: Append) -> Result<RawEvent, StoreError> {
        if let Append::Raw(EventKind::TurnStarted(t))
        | Append::Scrubbed(EventKind::TurnStarted(t)) = &kind
        {
            self.guard_first_turn(&t.turn_id).await?;
        }
        let (kind, payload) = match kind {
            Append::Raw(k) => {
                let (kind, mut payload) = k.into_parts();
                // Its free text only, never its schema: whatever node.toml's
                // values are, the stored event decodes as this kind (T3).
                let out = self.node.cfg.secrets.scrub_event(&kind, &mut payload);
                if let harness_core::secret::schema::Outcome::Fallback { left } = out {
                    tracing::warn!(
                        session = %self.id,
                        kind = %kind,
                        paths = left,
                        "a configured secret was left in an event: removing it there would \
                         have made the event unreadable"
                    );
                }
                (kind, payload)
            }
            // Scrubbed already, as the model was sent it: not again (a
            // second pass need not leave it the same).
            Append::Scrubbed(k) => k.into_parts(),
        };
        let ev = RawEvent {
            session_id: self.id,
            seq: *head + 1,
            ts_ms: now_ms(),
            kind,
            payload,
        };
        let owner = self.node.cfg.node_id.clone();
        let e2 = ev.clone();
        let out = self
            .node
            .cfg
            .store
            .call(move |s| s.append(&owner, &e2))
            .await?;
        if let AppendOutcome::Refused { reason, .. } = out {
            // An event the log does not take (too large, say): nothing was
            // stored, so the turn stops on a log failure and the next one
            // starts from the same head.
            return Err(StoreError::Refused(format!(
                "the session log refused an event: {reason}"
            )));
        }
        if out != AppendOutcome::Applied {
            // Only this struct writes this session, so this is a bug.
            return Err(StoreError::Corrupt(format!(
                "single-writer violation on {}: {out:?}",
                self.id
            )));
        }
        *head = ev.seq;
        // The mode this turn's commands run in (it cannot change while the
        // turn holds the slot): what the next turn's line compares with.
        if ev.kind == "turn_started" {
            *self
                .last_turn_mode
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = self.exec_mode_now();
        }
        // Still under `head`, so this session's events reach the link in
        // seq order. A full link pauses the session here until the master
        // reads (or the link drops; the event is in the store either way).
        self.node
            .publish_wait(NodeToMaster::Event { event: ev.clone() })
            .await;
        Ok(ev)
    }

    /// Append one of the running turn's events. The log is written first:
    /// on `Err` the caller must not do what the event describes. The first
    /// failure stops the turn (its token is cancelled) and makes the rest of
    /// its appends fail untried; the backend then ends the turn.
    pub(crate) async fn emit(&self, kind: impl Into<Append>) -> Result<(), LogError> {
        if self.log_failed.load(Ordering::SeqCst) {
            return Err(LogError(
                "an earlier event of this turn was not written".into(),
            ));
        }
        match self.append(kind.into()).await {
            Ok(_) => Ok(()),
            Err(e) => {
                self.note_log_failure(&e);
                if let Some(c) = self.current_cancel() {
                    c.cancel();
                }
                Err(LogError(e.to_string()))
            }
        }
    }

    /// Log a failed append, once per turn. The line names the store's
    /// error only: never the event's payload.
    fn note_log_failure(&self, e: &StoreError) {
        if !self.log_failed.swap(true, Ordering::SeqCst) {
            tracing::error!(session = %self.id, error = %e, "could not write the session log; stopping the turn");
        } else {
            tracing::debug!(session = %self.id, error = %e, "session log still not writable");
        }
    }

    /// Append one of the events that end a turn: tried once even after a
    /// failure, since a turn that cannot record its end must still end.
    async fn append_end(&self, kind: EventKind) -> bool {
        match self.append(kind).await {
            Ok(_) => true,
            Err(e) => {
                self.note_log_failure(&e);
                false
            }
        }
    }

    /// Append an event from outside a turn's own flow (a staged file).
    pub(crate) async fn record(&self, kind: EventKind) -> Result<(), String> {
        self.append(kind)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The folder this chat publishes from, when it may publish at all:
    /// agentic chats only (never incognito ones).
    pub(crate) fn publish_cwd(&self) -> Option<PathBuf> {
        (self.profile == Profile::Agentic && !self.incognito).then(|| self.cwd.clone())
    }

    /// An agentic session's folder, checked: still the directory pinned
    /// when the session was made (path, device and inode), still allowed
    /// by node.toml (inside `workspace_roots`, not holding or inside the
    /// data directory) and the sandbox still works. Checked before every
    /// turn, restore, delegation, publish and checkpoint, not only at
    /// create: sessions loaded from the log were made under older rules,
    /// and a folder can be replaced by a link after any check (R.1, T4).
    /// For a chat session: its (empty) folder, unchecked.
    pub(crate) fn checked_folder(&self) -> Result<PathBuf, String> {
        if self.profile != Profile::Agentic {
            return Ok(self.cwd.clone());
        }
        // Its mode is still one this node offers (N.10a).
        self.node.cfg.policy.check_exec(self.exec_mode_now())?;
        self.sub_agent_mode_mismatch()?;
        let dir = match self.pin() {
            Some(pin) => pin.check()?,
            None => {
                return Err(format!(
                    "{}: {} is not the directory this session was created in",
                    harness_core::folder::REPLACED,
                    self.cwd.display()
                ));
            }
        };
        // A sub-agent's node-owned folder: inside it, and the sandbox still
        // works; it is in the protected sessions folder by design.
        if let Some(o) = self.owned() {
            if !dir.starts_with(&o.top) {
                return Err(format!(
                    "{}: {} is outside the sub-agent's folder",
                    harness_core::folder::REPLACED,
                    dir.display()
                ));
            }
            return Ok(dir);
        }
        self.node.cfg.policy.check_session(&dir)?;
        Ok(dir)
    }

    /// Where sub-agents' folders are made: `<data>/sessions`, beside the
    /// checkpoint store, protected like the data directory.
    fn sessions_dir(&self) -> Option<PathBuf> {
        crate::sessions_dir(self.node.cfg.checkpoints_dir.as_deref()?)
    }

    fn check_cwd_still_allowed(&self) -> Result<(), String> {
        self.checked_folder().map(|_| ())
    }

    /// What bounds this session's checkpoint root and sub-agent repository
    /// (`dir` is its checked folder).
    fn bounds(&self, dir: &std::path::Path) -> crate::git::Bounds {
        let policy = &self.node.cfg.policy;
        crate::git::Bounds {
            ceiling: policy.root_of(dir).ok(),
            protected: policy.protected_dirs.clone(),
            sandbox: self.node.cfg.git_sandbox.clone(),
        }
    }

    /// This session's folder pin now (see `pin_cell`).
    fn pin(&self) -> Option<harness_core::folder::FolderPin> {
        self.pin_cell
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// This sub-agent's own folder now (see `owned_cell`).
    fn owned(&self) -> Option<harness_core::folder::OwnedFolder> {
        self.owned_cell
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The mode in force for the next call: this chat's own, never above
    /// what this node allows now, and for a sub-agent never above the chat
    /// that started it as that chat is now (and its own parents, read at
    /// every call: research 02 §8, a ceiling, not a copy). This holds after
    /// a restart and for a child loaded again later, since it is read from
    /// the parent each time; a sub-agent whose parent this node does not
    /// hold (deleted, not loaded) asks.
    pub(crate) fn current_mode(&self) -> ApprovalMode {
        let own = read_mode(&self.mode).min(self.node.cfg.policy.max_approval_mode);
        match self.parent_session {
            None => own,
            Some(pid) => match self.node.session(pid) {
                Some(parent) if parent.id != self.id => own.min(parent.current_mode()),
                _ => own.min(ApprovalMode::Ask),
            },
        }
    }

    /// The owner's change of the chat's approval mode (T50): clamped by
    /// node.toml's `max_approval_mode` (nothing else limits it),
    /// recorded as `approval_mode_changed` by this session (one writer),
    /// then applied: from the next call on. A card waiting now is not
    /// answered by it. Refused for a sub-agent's chat, a routine's and a
    /// chat without tools. Returns the mode in force.
    /// `lowering`: the master skipped the owner's level check because its
    /// copy of the log said this lowers the mode; refused, with nothing
    /// recorded, when it would raise it (the copy was behind).
    pub(crate) async fn set_mode(
        &self,
        requested: ApprovalMode,
        lowering: bool,
    ) -> Result<ApprovalMode, String> {
        if self.profile != Profile::Agentic {
            return Err(MODE_CHAT.into());
        }
        if self.parent_session.is_some() {
            return Err(MODE_SUB_AGENT.into());
        }
        if self.no_callbacks {
            return Err(MODE_ROUTINE.into());
        }
        let _change = self.mode_change.lock().await;
        let mode = self.node.cfg.policy.effective_mode(Some(requested));
        let now = read_mode(&self.mode);
        if mode == now {
            return Ok(mode);
        }
        if lowering && mode > now {
            return Err(format!(
                "{}: it is in {} now; send it again",
                harness_protocol::MODE_NOT_A_LOWERING,
                now.id()
            ));
        }
        // The log first: a change it does not hold is not made.
        self.append(EventKind::ApprovalModeChanged(ApprovalModeChanged {
            mode,
            requested: (mode != requested).then_some(requested),
        }))
        .await
        .map_err(|e| e.to_string())?;
        *self.mode.lock().unwrap_or_else(|p| p.into_inner()) = mode;
        *self.recorded.lock().unwrap_or_else(|p| p.into_inner()) = mode;
        // Entering plan mode (X.7): no card of this chat or of a sub-agent
        // under it can still be allowed; each is denied now, so none stays
        // on screen as if it could.
        if mode == ApprovalMode::Plan {
            let all: Vec<Arc<Session>> = self
                .node
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .cloned()
                .collect();
            for s in all
                .iter()
                .filter(|s| s.id == self.id || s.descends_from(self.id))
            {
                s.deny_waiting_for_plan();
            }
        }
        Ok(mode)
    }

    /// Review M1 of PR 125: before a turn starts, a mode the node now
    /// enforces that the log does not say (node.toml's `max_approval_mode`
    /// lowered since it was recorded: `load` clamps the mode in force, not
    /// the log) is recorded as `approval_mode_changed`, `requested` naming
    /// what the log said, as a clamp at `set_mode` is. The master's checks
    /// (memory in plan mode) and the UI read the log.
    async fn record_clamp(&self) {
        if self.profile != Profile::Agentic {
            return;
        }
        let _change = self.mode_change.lock().await;
        let now = read_mode(&self.mode).min(self.node.cfg.policy.max_approval_mode);
        let said = *self.recorded.lock().unwrap_or_else(|p| p.into_inner());
        if now == said {
            return;
        }
        if self
            .append(EventKind::ApprovalModeChanged(ApprovalModeChanged {
                mode: now,
                requested: Some(said),
            }))
            .await
            .is_ok()
        {
            *self.mode.lock().unwrap_or_else(|p| p.into_inner()) = now;
            *self.recorded.lock().unwrap_or_else(|p| p.into_inner()) = now;
        }
    }

    /// Whether `ancestor` started this session, or one above it, as the
    /// node holds them now (a few levels at most).
    fn descends_from(&self, ancestor: SessionId) -> bool {
        let mut up = self.parent_session;
        for _ in 0..8 {
            match up {
                Some(p) if p == ancestor => return true,
                Some(p) => up = self.node.session(p).and_then(|s| s.parent_session),
                None => return false,
            }
        }
        false
    }

    /// Deny every card waiting in this session because plan mode began
    /// (`approve` records each as the node's policy).
    fn deny_waiting_for_plan(&self) {
        let waiting: Vec<(String, Answer)> = self
            .approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .collect();
        let mut denied = self.plan_denied.lock().unwrap_or_else(|p| p.into_inner());
        for (id, tx) in waiting {
            denied.insert(id);
            let _ = tx.send((Decision::Deny, None));
        }
    }

    /// The owner's change of the chat's step limit (X.14): sent as a
    /// lowering that would raise the limit in force, refused (recording
    /// nothing; the master's copy was behind). Otherwise clamped to
    /// `1..=` node.toml's `[limits] max_steps` whatever was sent, recorded
    /// as `step_limit_changed` by this session (one writer), then applied:
    /// from the running turn's next request on (a limit below the steps it
    /// has used gives it its wrap-up request next). Refused for a
    /// sub-agent's chat, a routine's and a chat without tools. Returns the
    /// limit in force.
    pub(crate) async fn set_step_limit(
        &self,
        requested: i64,
        lowering: bool,
    ) -> Result<usize, String> {
        if self.profile != Profile::Agentic {
            return Err(STEPS_CHAT.into());
        }
        if self.parent_session.is_some() {
            return Err(STEPS_SUB_AGENT.into());
        }
        if self.no_callbacks {
            return Err(STEPS_ROUTINE.into());
        }
        let _change = self.mode_change.lock().await;
        let max = self.node.cfg.policy.limits.max_steps;
        let limit = requested.clamp(1, max as i64) as usize;
        // Sent as a lowering (the master skipped the level check) but a
        // raise of the limit in force: its copy of the log was behind.
        let now = self.current_steps();
        if lowering && limit > now {
            return Err(format!(
                "{}: it is {now} now; send it again",
                harness_protocol::STEPS_NOT_A_LOWERING
            ));
        }
        // An explicit choice is recorded whenever it differs from the
        // log's own value, even if it equals what is in force (review F7):
        // a node limit raised later must not bring an old value back.
        if Some(limit) == *self.steps.lock().unwrap_or_else(|p| p.into_inner()) {
            return Ok(limit);
        }
        // The log first: a change it does not hold is not made.
        self.append(EventKind::StepLimitChanged(StepLimitChanged {
            max_steps: limit as u64,
            requested: (limit as i64 != requested).then_some(requested),
        }))
        .await
        .map_err(|e| e.to_string())?;
        *self.steps.lock().unwrap_or_else(|p| p.into_inner()) = Some(limit);
        Ok(limit)
    }

    /// The step limit of this chat's person's turns now: its own, or the
    /// node's. A sub-agent's: at most its parent chat's (X.14).
    pub(crate) fn current_steps(&self) -> usize {
        let max = self.node.cfg.policy.limits.max_steps;
        let own = self
            .steps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .unwrap_or(max)
            .min(max);
        match self.parent_session {
            None => own,
            Some(pid) => match self.node.session(pid) {
                Some(parent) if parent.id != self.id => own.min(parent.current_steps()),
                // Its parent is gone (deleted): the smallest limit, as the
                // mode falls back to Ask (review N1).
                _ => 1,
            },
        }
    }

    /// Whether `write` or `edit` with `input` changes a file inside this
    /// chat's own folder, judged as the tool itself resolves the path when
    /// it may not leave the folder: links followed (one that leads out, or
    /// dangles, is out), `..` normalized, `.git` and the node's data
    /// directory out, the folder checked against its pin. Anything this
    /// cannot settle is out, and is asked about.
    fn edit_inside(&self, tool: &str, input: &Value) -> bool {
        if !harness_core::policy::EDIT_TOOLS.contains(&tool) {
            return false;
        }
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return false;
        };
        let mut ctx = harness_tools::ToolCtx::new(self.cwd.clone());
        ctx.pin = self.pin();
        ctx.owned = self.owned();
        ctx.protected = Arc::new(self.node.cfg.policy.protected_dirs.clone());
        ctx.allow_outside_workspace = false;
        let (Ok(real), Ok(ws)) = (ctx.resolve_for_write(path), ctx.workspace_dir()) else {
            return false;
        };
        real.starts_with(&ws) && real != ws
    }

    /// `agent_named`: the person chose the agent for this message (the
    /// picker or an @mention), as the master resolved it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn prompt(
        self: &Arc<Self>,
        input: Input,
        agent: Option<AgentSpec>,
        agent_named: bool,
        compare: Vec<String>,
        mut extra: TurnExtra,
        peers: Vec<AgentSpec>,
        tags: Vec<String>,
    ) -> Result<(), String> {
        if !compare.is_empty() && (self.profile != Profile::Chat || compare.len() > 4) {
            return Err("parallel answers need a chat and at most 4 models".into());
        }
        if input.reply.is_some()
            && (!input.attachments.is_empty() || !compare.is_empty() || !input.images.is_empty())
        {
            return Err(
                "an answer goes alone, without attachments, images or parallel answers".into(),
            );
        }
        if self.incognito && !input.images.is_empty() {
            return Err("images can't go into an incognito chat".into());
        }
        // The agents a message called after the first (X.37): this
        // message's own turn is the first agent's; the node keeps the rest.
        if !peers.is_empty()
            && (self.parent_session.is_some() || input.reply.is_some() || !compare.is_empty())
        {
            return Err(
                "a message that calls several agents is a plain message to a chat: no answer to \
                 a card, no parallel answers, no sub-agent's chat"
                    .into(),
            );
        }
        // Named for this message: an @mention, a routine's explicit agent,
        // or the asker an answer goes to (`switch_of` decides the rest).
        let named = agent_named || input.reply.is_some();
        self.check_cwd_still_allowed()?;
        // The owner prompts the chat: it may start background commands
        // again after a revocation closed its table (the master admits the
        // prompt only for an owner with access).
        if let Some(bg) = &self.bg {
            bg.reopen();
        }
        // The node decides how far delegation goes (Gate 3 review P-2): the
        // master's spec may ask for less than `[limits]`, never more. Its
        // sub-agents' specs come from this one, so they are bounded too.
        let limits = &self.node.cfg.policy.limits;
        let agent = agent.map(|a| limits.bound_delegation(a));
        let peers = self.checked_peers(agent.as_ref(), peers)?;
        let tags = peers::checked_tags(tags)?;
        // The agents the message tagged (sub-agent-only names it mentions)
        // that this agent has among its own delegates: one line in the
        // turn's context, recorded with it, so a replay is the same bytes.
        if let Some(line) = agent
            .as_ref()
            .and_then(|a| harness_core::mention::tags_line(&peers::allowed_tags(a, &tags)))
        {
            extra.context = Some(match extra.context.take() {
                Some(c) if !c.is_empty() => format!("{c}\n\n{line}"),
                _ => line,
            });
        }
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        let queue = self.queue.lock().await;
        let claim = || {
            self.claim_turn(RunningTurn {
                turn_id: turn_id.clone(),
                cancel: cancel.clone(),
                savings: extra.savings.clone(),
                context: extra.context.clone(),
                standing: extra.standing.clone(),
                publishes: Arc::default(),
                by_stop: by_stop.clone(),
                agent_id: agent.as_ref().map(|a| a.agent_id.clone()),
                callback: false,
                compare: !compare.is_empty(),
                began: std::time::SystemTime::now(),
                delegated_by: None,
            })
        };
        if let Err(e) = claim() {
            if e != TURN_RUNNING {
                return Err(e);
            }
            // A turn runs: the message waits for it (X.5). One that ended
            // since left the slot to this message's own turn.
            match self.running_turn() {
                Some(_) if !peers.is_empty() => {
                    return Err(format!(
                        "{TURN_RUNNING}: a message that calls several agents waits until it ends"
                    ));
                }
                Some(r) if !r.cancelled => {
                    let running = r.turn_id.clone();
                    let queued = self
                        .enqueue(r, input, &compare, agent.as_ref(), agent_named)
                        .await;
                    if queued.is_ok() {
                        *self
                            .queued_context
                            .lock()
                            .unwrap_or_else(|p| p.into_inner()) = Some(QueuedContext {
                            agent: agent.as_ref().map(|a| a.agent_id.clone()),
                            turn: running,
                            context: extra.context.clone(),
                            standing: extra.standing.clone(),
                        });
                    }
                    return queued;
                }
                // Stopped by the person, still ending: refused as before.
                Some(_) => {
                    return Err(format!(
                        "{TURN_RUNNING}: it is stopping; send this when it has stopped"
                    ));
                }
                None => claim()?,
            }
        }
        // Claimed after a stop began: refused here, since the stop's
        // cancel may have passed this session already (A.19).
        if self.node.is_stopping() {
            self.release(&turn_id);
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // In the instant between two turns of a called list, the next
        // agent's turn comes first (X.37).
        if self.peers_pending() {
            self.release(&turn_id);
            return Err(format!("{TURN_RUNNING}: {}", peers::PEERS_PENDING));
        }
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            if let Err(e) = self.close_open_turn(message, TurnEndReason::Error).await {
                self.release(&turn_id);
                return Err(e);
            }
            self.unclosed.store(false, Ordering::SeqCst);
        }
        let (mut msg, saved) = match self.user_message(turn_id.clone(), input).await {
            Ok(m) => m,
            Err(e) => {
                self.release(&turn_id);
                return Err(e);
            }
        };
        // What a turn that reads messages this one leaves unread starts
        // from: this turn's savings only. Its context was for this message
        // (a recall by its words, a routine's note), not for those (ST-8).
        let after = TurnExtra {
            savings: extra.savings.clone(),
            ..Default::default()
        };
        // Who this message called, first agent first (X.37).
        msg.tags = tags.clone();
        if !peers.is_empty() {
            msg.triggers = agent
                .iter()
                .chain(peers.iter())
                .map(|a| a.agent_id.clone())
                .collect();
        }
        // Recorded before the prompt is answered, so a log that cannot be
        // written refuses the prompt itself and the sender sees why.
        let seen = self.head_seq().await;
        let (history, queued) = match self
            .begin(Some(&msg), &turn_id, agent.as_ref(), named)
            .await
        {
            Ok(h) => h,
            Err(e) => {
                // The message was not recorded: the files saved for it go
                // too, or they would stay without a message (T94).
                self.rollback_uploads(&saved).await;
                self.release(&turn_id);
                return Err(e);
            }
        };
        drop(queue);
        if !peers.is_empty() {
            // Kept for after this turn; a store that cannot say where the
            // message stands leaves seq 0, and the cause then names no
            // message (the chat still shows who was called).
            let at = self.message_seq(seen, &turn_id).await.unwrap_or(0);
            self.set_peers(at, peers, tags);
        }
        let extra = TurnExtra {
            images: msg.images.clone(),
            files_seen: msg.files.iter().filter_map(|f| f.seen.clone()).collect(),
            queued,
            ..extra
        };
        let this = self.clone();
        let user_text = msg.model_text();
        tokio::spawn(async move {
            let (reason, _) = this
                .clone()
                .run_turn(
                    turn_id.clone(),
                    user_text,
                    history,
                    agent.clone(),
                    compare,
                    extra,
                    cancel,
                    by_stop,
                )
                .await;
            // The agents the message called after this one, if any, then the
            // messages sent meanwhile (X.5, X.37).
            this.finish_turn(reason, agent, after, turn_id).await;
        });
        Ok(())
    }

    /// The seq of the last event written.
    async fn head_seq(&self) -> u64 {
        *self.head.lock().await
    }

    /// The running turn, if any.
    fn running_turn(&self) -> Option<Running> {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| Running {
                turn_id: t.turn_id.clone(),
                cancelled: t.cancel.is_cancelled(),
                agent_id: t.agent_id.clone(),
                compare: t.compare,
            })
    }

    /// Record a message sent while turn `turn_id` runs (X.5): stored before
    /// it is acknowledged, as the person's message of that turn
    /// (`user_message.queued`); the model reads it before the turn's next
    /// request, or the next turn does. An answer to a question and
    /// parallel answers wait for the turn's end, as before; so does a
    /// message past [`MAX_QUEUED`], one naming another agent than the
    /// running turn's (ST-5), one to a sub-agent's chat (ST-6) and any
    /// while parallel answers run (ST-7). The caller holds `queue`.
    async fn enqueue(
        &self,
        running: Running,
        input: Input,
        compare: &[String],
        agent: Option<&AgentSpec>,
        agent_named: bool,
    ) -> Result<(), String> {
        if self.parent_session.is_some() {
            return Err(format!(
                "{TURN_RUNNING}: a sub-agent's chat takes no message while it works; \
                 its parent reads its answer"
            ));
        }
        if running.compare {
            return Err(format!(
                "{TURN_RUNNING}: parallel answers take no message while they run"
            ));
        }
        if agent_named && agent.map(|a| &a.agent_id) != running.agent_id.as_ref() {
            return Err(format!(
                "{TURN_RUNNING}: the running turn's agent reads it; pick the agent for your \
                 next message"
            ));
        }
        let turn_id = running.turn_id;
        if input.reply.is_some() {
            return Err(format!("{TURN_RUNNING}: an answer waits until it ends"));
        }
        if !compare.is_empty() {
            return Err(format!(
                "{TURN_RUNNING}: parallel answers wait until it ends"
            ));
        }
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if self.pending.lock().unwrap_or_else(|p| p.into_inner()).len() >= MAX_QUEUED {
            return Err(format!(
                "{TURN_RUNNING} and {MAX_QUEUED} of your messages already wait for it to read them; \
                 send this once it has"
            ));
        }
        // A file saved now would land inside the running turn's changes,
        // and undoing that turn would delete it: it waits for the turn's
        // end like an answer does (T94).
        if !input.uploads.is_empty() {
            return Err(format!(
                "{TURN_RUNNING}: a message with attached files waits until it ends"
            ));
        }
        let (mut msg, _) = self.user_message(turn_id, input).await?;
        msg.queued = true;
        let ev = self
            .append(Append::Scrubbed(EventKind::UserMessage(msg.clone())))
            .await
            .map_err(|e| e.to_string())?;
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((ev.seq, msg));
        Ok(())
    }

    /// After a turn that ended `completed`, at its step limit or with an
    /// error, with messages
    /// it never read (they came after its last step, or it failed: the
    /// message is often the fix, review ST-3; or it was a callback turn,
    /// which reads none, ST-2): a turn of the person's starts with them,
    /// the last as its own message, and so on while such a turn leaves
    /// more. Nothing starts after Stop, a node stop or a restart: the
    /// messages then wait for the person's next one. Nor when another turn
    /// took the slot first; that turn reads them.
    async fn read_queued(
        self: Arc<Self>,
        mut reason: TurnEndReason,
        agent: Option<AgentSpec>,
        after: TurnExtra,
        mut ended: String,
    ) {
        // A turn that only reads queued messages is no send of the master's,
        // so the master answers none of its connector calls: it is not
        // offered the custom connectors' tools at all (X.35 step 2, review 3
        // of PR 157, L10).
        let agent = agent.map(|mut a| {
            a.connector_tools.clear();
            a
        });
        while matches!(
            reason,
            TurnEndReason::Completed | TurnEndReason::Error | TurnEndReason::Limit
        ) {
            #[cfg(feature = "testing")]
            crate::testing::queued_hold(self.id).await;
            let Some((turn_id, user_text, history, extra, cancel, by_stop)) = self
                .start_queued(agent.as_ref(), after.clone(), &ended)
                .await
            else {
                return;
            };
            ended = turn_id.clone();
            reason = self
                .clone()
                .run_turn(
                    turn_id,
                    user_text,
                    history,
                    agent.clone(),
                    vec![],
                    extra,
                    cancel,
                    by_stop,
                )
                .await
                .0;
        }
    }

    /// Start a turn for the unread queued messages, if any wait and the
    /// slot is free; see [`Self::read_queued`].
    #[allow(clippy::type_complexity)]
    async fn start_queued(
        &self,
        agent: Option<&AgentSpec>,
        extra: TurnExtra,
        ended: &str,
    ) -> Option<(
        String,
        String,
        Vec<RawEvent>,
        TurnExtra,
        CancellationToken,
        Arc<AtomicBool>,
    )> {
        let _queue = self.queue.lock().await;
        if self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
            || self.node.is_stopping()
            || self.unclosed.load(Ordering::SeqCst)
            || self.check_cwd_still_allowed().is_err()
        {
            return None;
        }
        // The context of the newest message it reads, computed for that
        // message, only when it was computed for the agent that runs this
        // turn and queued while the turn that just ended ran (review H1);
        // otherwise none (after a restart, a Stop, another agent): then it
        // reads no memory (F2). Never the last turn's (ST-8).
        let mut extra = extra;
        let kept = self
            .queued_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(q) = kept.filter(|q| q.usable(ended, agent.map(|a| a.agent_id.as_str()))) {
            extra.context = q.context;
            extra.standing = q.standing;
        }
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: extra.savings.clone(),
            context: extra.context.clone(),
            standing: extra.standing.clone(),
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
            agent_id: agent.map(|a| a.agent_id.clone()),
            callback: false,
            compare: false,
            began: std::time::SystemTime::now(),
            delegated_by: None,
        })
        .ok()?;
        let (history, mut queued) = match self.begin(None, &turn_id, agent, true).await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "could not start a turn for the messages sent while the last one ran");
                self.release(&turn_id);
                return None;
            }
        };
        let Some(last) = queued.pop() else {
            // `begin` refuses a turn with nothing to read; never here.
            self.end_turn(&turn_id, TurnEndReason::Error, None, None)
                .await;
            return None;
        };
        let extra = TurnExtra {
            images: last.images.clone(),
            queued,
            ..extra
        };
        let user_text = queued_text(&last.model_text());
        Some((turn_id, user_text, history, extra, cancel, by_stop))
    }

    /// Resume this idle chat because background commands ended (a callback
    /// turn, T.8d step 3), as the master asks. The node decides: it starts
    /// the turn only if every id names an end in its own log that may call
    /// back (it exited or timed out, and no step was told of it) and still
    /// waits (no person's turn, refusal or callback turn naming it after
    /// it), the chat may take callback turns at all, and its budget of
    /// turns without the person's message lasts. The turn's cause names
    /// those ids and every other end that waits then (`callback_check`).
    /// A refusal on the node's own rules is recorded (`callback_refused`)
    /// and the ends wait for the person; one that finds nothing to resume
    /// says so ([`harness_protocol::CALLBACK_NOTHING`]) and records
    /// nothing. Nothing in the command widens a bound: the step cap is
    /// `[limits]`, the approval mode is the session's (clamped at load),
    /// delegation is off, and a table a revocation closed stays closed.
    pub(crate) async fn callback(
        self: &Arc<Self>,
        bg_ids: Vec<String>,
        agent: Option<AgentSpec>,
        savings: harness_core::savings::Savings,
        standing: Option<Vec<harness_core::backend::StandingPart>>,
    ) -> Result<(), String> {
        use harness_protocol::{CALLBACK_NOTHING, MAX_CALLBACK_IDS};
        let mut wanted = bg_ids.clone();
        wanted.sort();
        wanted.dedup();
        if bg_ids.is_empty() || bg_ids.len() > MAX_CALLBACK_IDS || wanted.len() != bg_ids.len() {
            return Err(format!(
                "a callback names 1 to {MAX_CALLBACK_IDS} different background commands"
            ));
        }
        if self.parent_session.is_some() || self.bg.is_none() {
            return Err(format!(
                "{CALLBACK_NOTHING}: this chat runs no background commands"
            ));
        }
        self.check_cwd_still_allowed()?;
        let limits = &self.node.cfg.policy.limits;
        let turn_id = new_id();
        let cancel = CancellationToken::new();
        let by_stop = Arc::new(AtomicBool::new(false));
        // Until `turn_started`: a message sent meanwhile lands after it.
        let queue = self.queue.lock().await;
        self.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: savings.clone(),
            // A callback turn takes no context, so it passes none on.
            context: None,
            standing: None,
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
            agent_id: agent.as_ref().map(|a| a.agent_id.clone()),
            callback: true,
            compare: false,
            began: std::time::SystemTime::now(),
            delegated_by: None,
        })
        .map_err(|e| {
            if e == TURN_RUNNING || e == harness_protocol::COMPACTING {
                harness_protocol::CALLBACK_BUSY.to_string()
            } else {
                e
            }
        })?;
        // Under the slot: no other turn starts and every end that comes
        // meanwhile is queued, so the log read here is the log the turn
        // joins.
        if self.node.is_stopping() {
            self.release(&turn_id);
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if self.unclosed.load(Ordering::SeqCst) {
            let message = "The session log could not be written, so this turn was stopped.";
            if let Err(e) = self.close_open_turn(message, TurnEndReason::Error).await {
                self.release(&turn_id);
                return Err(e);
            }
            self.unclosed.store(false, Ordering::SeqCst);
        }
        // Held from the read to `turn_started`: an end whose check passed
        // before the slot was taken is in the log before the read, and
        // every later one is queued for the turn (S3-4). The mode in force
        // recorded first, if the log says another.
        self.record_clamp().await;
        let gate = self.bg_gate.lock().await;
        let id = self.id;
        let history = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(h) => h,
            Err(e) => {
                self.release(&turn_id);
                return Err(e.to_string());
            }
        };
        // The ends the turn is for: the master's and every other that
        // waits now (bounded); a refusal names the same.
        let (refusal, bg_ids) = match callback_check(&history, &bg_ids) {
            Err(e) => {
                drop(gate);
                self.release(&turn_id);
                return Err(e);
            }
            Ok((in_a_row, named)) => (
                if self.no_callbacks {
                    Some("this chat takes no callback turns (a routine's chat)".to_string())
                } else if self.bg.as_ref().is_some_and(|b| b.is_closed()) {
                    // A revocation closed the table; only the owner's own
                    // prompt opens it again.
                    Some("its owner lost access; nothing resumes it until they write".to_string())
                } else if limits.callback_turns == 0 {
                    Some("callback turns are off on this node (callback_turns = 0)".to_string())
                } else if in_a_row >= limits.callback_turns {
                    Some(format!(
                        "{in_a_row} turns in a row without you (node limit)"
                    ))
                } else {
                    None
                },
                named,
            ),
        };
        if let Some(reason) = refusal {
            let recorded = self
                .append(EventKind::CallbackRefused(CallbackRefused {
                    bg_ids,
                    reason: reason.clone(),
                }))
                .await;
            drop(gate);
            self.release(&turn_id);
            recorded.map_err(|e| e.to_string())?;
            return Err(format!("{}: {reason}", harness_protocol::CALLBACK_REFUSED));
        }
        // No sub-agents in a turn nobody watches: the unattended spend is
        // `callback_turns × callback_max_steps` model calls, nothing more.
        // The person's turn that reads their messages after it gets the
        // agent as it is, sub-agents and all (review ST-2).
        let person = agent.clone().map(|a| limits.bound_delegation(a));
        let agent = agent.map(|a| {
            let mut a = limits.bound_delegation(a);
            a.delegates.clear();
            a
        });
        let cause = TurnCause::Callback(bg_ids);
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let environment = self.environment_line(&turn_id).await;
        // A pick no message followed yet: this turn is the first to run
        // under it, so the switch is noted here, above it, and the log
        // reads in order (the master sends the chat's agent).
        if let Some(switch) = self.switch_of(&history, agent.as_ref(), false)
            && let Err(e) = self.emit(EventKind::AgentChanged(switch)).await
        {
            drop(gate);
            self.release(&turn_id);
            return Err(e.to_string());
        }
        if let Err(e) = self
            .emit(EventKind::TurnStarted(TurnStarted {
                turn_id: turn_id.clone(),
                agent: agent.as_ref().map(AgentSpec::agent_ref),
                cause: Some(cause.clone()),
                environment,
                continued_by: None,
                retry: None,
            }))
            .await
        {
            drop(gate);
            self.release(&turn_id);
            return Err(e.to_string());
        }
        drop(gate);
        drop(queue);
        // A message the person sent during it that it never read starts
        // the person's turn after it (X.5).
        let after = TurnExtra {
            savings: savings.clone(),
            ..Default::default()
        };
        // The master's standing parts for the chat's agent (review M1):
        // the turn reads the memory a person's turn of it would, and its
        // request extends the last one when nothing changed. None: it
        // reads none.
        let partial = standing.is_some();
        let extra = TurnExtra {
            savings,
            cause: Some(cause),
            max_steps: Some(limits.callback_steps()),
            standing,
            standing_partial: partial,
            ..Default::default()
        };
        let this = self.clone();
        tokio::spawn(async move {
            let (reason, _) = this
                .clone()
                .run_turn(
                    turn_id.clone(),
                    String::new(),
                    history,
                    agent,
                    vec![],
                    extra,
                    cancel,
                    by_stop,
                )
                .await;
            this.keep_reader(person.clone(), after.clone());
            this.read_queued(reason, person, after, turn_id).await;
        });
        Ok(())
    }

    /// Write the chat's hand-off note if one is due (X.16,
    /// docs/handoff-notes.md): asked by the master once the chat has been
    /// idle. Only for a chat of the person's own that changes files
    /// (agentic, not a sub-agent's, not incognito, not a routine's), when
    /// its log says a note is due and no turn runs. Acked once started; the
    /// writing goes on beside the chat without the turn slot: a turn that
    /// claims the slot, a Stop or a delete cancels it, and a note is
    /// recorded only when nothing joined the log since what it read (so
    /// no event lands inside a turn or between a turn's read and its
    /// start that a callback or a summary would read). Models: the small
    /// model the master named, then the chat's own (the model the last
    /// turn ended on), each tried once; this node's providers decide which
    /// it knows (an unknown one fails as `unknown to this node`, as a
    /// prompt's model would).
    pub(crate) async fn handoff(
        self: &Arc<Self>,
        small_model: Option<String>,
        windows: std::collections::BTreeMap<String, u64>,
        low_effort: Vec<String>,
    ) -> Result<(), String> {
        use harness_protocol::{HANDOFF_BUSY, HANDOFF_NOTHING};
        let per_hour = self.node.cfg.policy.limits.handoff_notes_per_hour;
        if per_hour == 0 {
            return Err(format!(
                "{HANDOFF_NOTHING}: hand-off notes are off on this node (handoff_notes_per_hour = 0)"
            ));
        }
        if self.profile != Profile::Agentic
            || self.parent_session.is_some()
            || self.incognito
            || self.no_callbacks
        {
            return Err(format!(
                "{HANDOFF_NOTHING}: this chat takes no hand-off notes"
            ));
        }
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // Under `queue`, which a person's turn holds from its claim of the
        // slot to its start: no turn starts between this check and the
        // note's claim (and one that claims after cancels the note).
        let queue = self.queue.lock().await;
        if self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
            || self
                .compacting
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some()
        {
            return Err(HANDOFF_BUSY.into());
        }
        let cancel = CancellationToken::new();
        {
            let mut n = self.note.lock().unwrap_or_else(|p| p.into_inner());
            if n.is_some() {
                return Err(HANDOFF_BUSY.into());
            }
            *n = Some(cancel.clone());
        }
        drop(queue);
        let id = self.id;
        let history = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(h) => h,
            Err(e) => {
                self.end_note(&cancel);
                return Err(e.to_string());
            }
        };
        // What a fork copied is its parent's, which has its own note: due
        // only for turns of the fork's own (the model still reads it all).
        let own = harness_core::fork::own(&history, harness_core::fork::through_of(&history));
        if !harness_core::handoff::due(&own) {
            self.end_note(&cancel);
            return Err(HANDOFF_NOTHING.into());
        }
        // The node's bound on unattended note calls per chat (a master that
        // asks and cancels in a loop meets it too: a cancelled note leaves
        // no event, so the chat stays due).
        {
            let mut tries = self.note_tries.lock().unwrap_or_else(|p| p.into_inner());
            let hour = std::time::Duration::from_secs(3600);
            while tries.front().is_some_and(|t| t.elapsed() >= hour) {
                tries.pop_front();
            }
            if tries.len() >= per_hour {
                drop(tries);
                self.end_note(&cancel);
                return Err(format!(
                    "{HANDOFF_NOTHING}: {per_hour} notes started for this chat in the last hour \
                     (handoff_notes_per_hour)"
                ));
            }
            tries.push_back(std::time::Instant::now());
        }
        let through = history.last().map_or(0, |e| e.seq);
        let chat_model = history
            .iter()
            .rev()
            .find_map(|e| match e.typed() {
                Ok(EventKind::TurnEnded(t)) => t.usage.and_then(|u| u.model),
                _ => None,
            })
            .unwrap_or_else(|| self.model.clone());
        let mut models: Vec<String> = small_model.into_iter().collect();
        if !models.contains(&chat_model) {
            models.push(chat_model);
        }
        let models = models
            .into_iter()
            .map(|m| {
                let w = windows.get(&m).copied();
                (m, w)
            })
            .collect();
        let this = self.clone();
        tokio::spawn(async move {
            let out = this
                .node
                .cfg
                .backend
                .write_note(harness_core::backend::NoteCtx {
                    session_id: id,
                    history,
                    models,
                    low_effort,
                    cancel: cancel.clone(),
                })
                .await;
            let Some(out) = out else {
                this.end_note(&cancel);
                return;
            };
            let ev = EventKind::HandoffWritten(harness_core::event::HandoffWritten {
                through_seq: through,
                text: out.text,
                model: out.model,
                failed: out.failed,
                usage: out.usage,
            });
            let go = || {
                !cancel.is_cancelled()
                    && this
                        .turn
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .is_none()
            };
            match this.append_if_head(through, go, ev).await {
                Ok(Some(_)) => tracing::debug!(session = %id, "hand-off note recorded"),
                Ok(None) => {
                    tracing::debug!(session = %id, "hand-off note dropped: the chat went on")
                }
                Err(e) => {
                    tracing::warn!(session = %id, error = %e, "could not record a hand-off note")
                }
            }
            this.end_note(&cancel);
        });
        Ok(())
    }

    /// A person's `/compact` (X.20, PLAN.md T64): summarise the chat's
    /// history now with their focus text. Only between turns: refused
    /// while a turn runs (the turn shortens its own context, X.15), for a
    /// sub-agent's chat, and for a chat too short to summarise (no event);
    /// one at a time. Acked once started; the outcome is recorded by this
    /// session (one writer): `history_summarized` (`manual`) only if
    /// nothing joined the log since what it read, else `compact_failed`.
    /// No turn starts meanwhile (`claim_turn`); Stop and a delete cancel
    /// it. A hand-off note being written is dropped: the person goes first.
    pub(crate) async fn compact(
        self: &Arc<Self>,
        focus: Option<String>,
        windows: std::collections::BTreeMap<String, u64>,
    ) -> Result<(), String> {
        use harness_protocol::{COMPACT_BUSY, COMPACT_TURN_RUNNING};
        if self.parent_session.is_some() {
            return Err("A sub-agent's chat is not summarised by hand.".into());
        }
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // The person's words, as the summariser and the log will have them.
        let focus = focus
            .map(|f| cut_focus(&self.node.cfg.secrets.scrub(f.trim())))
            .filter(|f| !f.is_empty());
        // Under `queue` and `turn`: no turn starts between this check and
        // the claim (`claim_turn` reads `compacting` under `turn`).
        let queue = self.queue.lock().await;
        let cancel = CancellationToken::new();
        {
            let t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            if t.is_some() {
                return Err(COMPACT_TURN_RUNNING.into());
            }
            // A message sent during the last turn that no turn read yet:
            // the turn that reads it may be about to claim the slot (it
            // waits for `queue`), and must not find it taken (review M1).
            if !self
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty()
            {
                return Err(harness_protocol::COMPACT_PENDING.into());
            }
            let mut c = self.compacting.lock().unwrap_or_else(|p| p.into_inner());
            if c.is_some() {
                return Err(COMPACT_BUSY.into());
            }
            *c = Some(cancel.clone());
        }
        drop(queue);
        self.cancel_note();
        let id = self.id;
        let history = match self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
        {
            Ok(h) => h,
            Err(e) => {
                self.end_compact();
                return Err(e.to_string());
            }
        };
        if let Err(e) = self.node.cfg.backend.compact_check(&history) {
            self.end_compact();
            return Err(e);
        }
        let through = history.last().map_or(0, |e| e.seq);
        let model = history
            .iter()
            .rev()
            .find_map(|e| match e.typed() {
                Ok(EventKind::TurnEnded(t)) => t.usage.and_then(|u| u.model),
                _ => None,
            })
            .unwrap_or_else(|| self.model.clone());
        let window = windows.get(&model).copied();
        let this = self.clone();
        tokio::spawn(async move {
            let out = this
                .node
                .cfg
                .backend
                .compact_now(harness_core::backend::CompactCtx {
                    session_id: id,
                    history,
                    model,
                    window,
                    focus: focus.clone(),
                    cancel: cancel.clone(),
                })
                .await;
            let failed_reason = harness_core::event::compact_failed_reason;
            let failed = |reason: String, usage| {
                EventKind::CompactFailed(harness_core::event::CompactFailed {
                    reason,
                    usage: Some(usage).filter(|u: &Usage| *u != Usage::default()),
                })
            };
            let ev = match out {
                None => failed(
                    failed_reason("this chat's agent cannot summarise its history"),
                    Usage::default(),
                ),
                Some(harness_core::backend::CompactOutcome::Failed { reason, usage }) => {
                    failed(reason, usage)
                }
                Some(harness_core::backend::CompactOutcome::Done {
                    through_seq,
                    text,
                    usage,
                    tokens_after,
                }) => {
                    let summary = EventKind::HistorySummarized(HistorySummarized {
                        turn_id: String::new(),
                        through_seq,
                        text,
                        manual: true,
                        focus,
                        usage: Some(usage.clone()),
                        tokens_after: Some(tokens_after),
                    });
                    match this
                        .append_if_head(through, || !cancel.is_cancelled(), summary)
                        .await
                    {
                        Ok(Some(_)) => {
                            tracing::debug!(session = %id, "manual summary recorded");
                            this.end_compact();
                            return;
                        }
                        Ok(None) if cancel.is_cancelled() => {
                            failed(failed_reason("it was stopped"), usage)
                        }
                        Ok(None) => failed(
                            failed_reason("the chat changed while it was being summarised"),
                            usage,
                        ),
                        Err(e) => {
                            tracing::warn!(session = %id, error = %e, "could not record a manual summary");
                            failed(failed_reason("its log could not be written"), usage)
                        }
                    }
                }
            };
            // The outcome is the person's to see, whatever stopped it (a
            // deleted chat's log takes nothing).
            if let Err(e) = this.append(ev).await {
                tracing::warn!(session = %id, error = %e, "could not record a failed summary");
            }
            this.end_compact();
        });
        Ok(())
    }

    /// The `/compact` is over, whatever its outcome: turns may start
    /// again, and messages that wait to be read (none can arrive while it
    /// runs, and it is refused while any wait; this is the second guard of
    /// review M1) start the turn that reads them, as after a turn.
    fn end_compact(self: &Arc<Self>) {
        {
            let _t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
            self.compacting
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
        }
        let waiting = !self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty();
        let reader = self
            .reader
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let (true, Some((agent, after))) = (waiting, reader) {
            let this = self.clone();
            tokio::spawn(async move {
                this.read_queued(TurnEndReason::Completed, agent, after, String::new())
                    .await;
            });
        }
    }

    /// What the last person's turn reads its queued messages with.
    fn keep_reader(&self, agent: Option<AgentSpec>, after: TurnExtra) {
        *self.reader.lock().unwrap_or_else(|p| p.into_inner()) = Some((agent, after));
    }

    /// Start a turn in the log: read the history before it, then record
    /// `turn_started`, the queued messages no turn read (`queued_delivered`,
    /// X.5) and the user's message. Returns the history and those queued
    /// messages, which the turn sends before its own. `msg` is `None` for a
    /// turn that only reads queued messages: with none unread, nothing is
    /// recorded and it is refused. When an append fails nothing has run,
    /// and whatever was written is closed. A person's turn holds `queue`.
    /// `named`: the turn's agent was named for it rather than being the
    /// chat's own (`switch_of`, which records a switch of agent or model
    /// first, `agent_changed`, only for the chat's own).
    async fn begin(
        &self,
        msg: Option<&UserMessage>,
        turn_id: &str,
        agent: Option<&AgentSpec>,
        named: bool,
    ) -> Result<(Vec<RawEvent>, Vec<UserMessage>), String> {
        self.begin_with(msg, turn_id, agent, None, named).await
    }

    /// [`Self::begin`] for a sub-agent's follow-up (T68): `follow_up` is the
    /// parent's `delegate` call id, recorded as `continued_by`, and the
    /// harness's line for the opening, recorded after the environment line
    /// (both in `turn_started`), so the request and every later rebuild of
    /// it carry the same bytes.
    async fn begin_with(
        &self,
        msg: Option<&UserMessage>,
        turn_id: &str,
        agent: Option<&AgentSpec>,
        follow_up: Option<(&str, &str)>,
        named: bool,
    ) -> Result<(Vec<RawEvent>, Vec<UserMessage>), String> {
        // The mode in force recorded first, if the log says another.
        self.record_clamp().await;
        // As for a callback turn (S3-4): no end lands between the read and
        // `turn_started`.
        let gate = self.bg_gate.lock().await;
        let id = self.id;
        let history = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let queued = rewind_op::unread_visible(&history);
        if msg.is_none() && queued.is_empty() {
            // The log is the authority: nothing waits.
            self.pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clear();
            self.queued_context
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            return Err("no message waits to be read".into());
        }
        self.log_failed.store(false, Ordering::SeqCst);
        self.unresolved
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let mut environment = self.environment_line(turn_id).await;
        if let Some((_, line)) = follow_up {
            let both = match environment.take() {
                Some(env) => format!("{env}\n{line}"),
                None => line.to_string(),
            };
            *self.turn_env.lock().unwrap_or_else(|p| p.into_inner()) =
                Some((turn_id.to_string(), both.clone()));
            environment = Some(both);
        }
        // The switch, above the turn and its message (not in `history`:
        // the model is never sent it).
        if let Some(switch) = self.switch_of(&history, agent, named)
            && let Err(e) = self.emit(EventKind::AgentChanged(switch)).await
        {
            return Err(e.to_string());
        }
        let started = self
            .emit(EventKind::TurnStarted(TurnStarted {
                cause: None,
                agent: agent.map(AgentSpec::agent_ref),
                turn_id: turn_id.into(),
                environment,
                continued_by: follow_up.map(|(call, _)| call.to_string()),
                retry: None,
            }))
            .await;
        drop(gate);
        if let Err(e) = started {
            return Err(e.to_string());
        }
        let seqs: Vec<u64> = queued.iter().map(|(s, _)| *s).collect();
        let mut written = Ok(());
        if !seqs.is_empty() {
            written = self
                .emit(EventKind::QueuedDelivered(QueuedDelivered {
                    turn_id: turn_id.into(),
                    seqs: seqs.clone(),
                }))
                .await;
        }
        if written.is_ok() {
            self.pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|(s, _)| !seqs.contains(s));
            // A person's own turn reads them with its own context.
            if msg.is_some() {
                self.queued_context
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take();
            }
            if let Some(msg) = msg {
                written = self
                    .emit(Append::Scrubbed(EventKind::UserMessage(msg.clone())))
                    .await;
            }
        }
        if let Err(e) = written {
            self.end_turn(turn_id, TurnEndReason::Error, None, Some(e.to_string()))
                .await;
            return Err(e.to_string());
        }
        Ok((history, queued.into_iter().map(|(_, m)| m).collect()))
    }

    /// Check a message's images and keep their bytes with the chat, before
    /// the message that names them is recorded.
    async fn keep_images(
        &self,
        images: Vec<harness_protocol::ChatImage>,
    ) -> Result<Vec<ImageRef>, String> {
        if images.is_empty() {
            return Ok(vec![]);
        }
        let (refs, kept) = crate::images::accept(images)?;
        let sid = self.id.to_string();
        self.node
            .cfg
            .store
            .call(move |s| s.session_images_put(&sid, &kept))
            .await
            .map_err(|e| e.to_string())?;
        Ok(refs)
    }

    /// The message a turn starts with. An answer is checked against the
    /// log here, while the caller holds the turn slot: nothing else can
    /// append (another prompt, a variant choice) until the turn has
    /// recorded it, so the log checked is the log it joins.
    async fn user_message(
        &self,
        turn_id: String,
        input: Input,
    ) -> Result<(UserMessage, Vec<crate::uploads::Saved>), String> {
        let Input {
            text,
            attachments,
            reply,
            images,
            files,
            uploads,
            attached,
        } = input;
        let files = self.read_mentions(files).await?;
        let attached = self.check_attached(attached)?;
        let images = self.keep_images(images).await?;
        let text = match &reply {
            None => text,
            Some(r) => {
                let id = self.id;
                let history = self
                    .node
                    .cfg
                    .store
                    .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
                    .await
                    .map_err(|e| e.to_string())?;
                let canonical = harness_core::ui::check_reply(&history, r)?;
                harness_core::ui::answer_text(canonical, &text)
            }
        };
        // Last, so nothing after it can fail and leave files without a
        // message; written before the turn's `before` checkpoint, so undoing
        // the turn leaves them (they are the person's files, T94).
        let saved = self.save_uploads(uploads).await?;
        let msg = UserMessage {
            turn_id,
            text,
            attachments,
            reply,
            images,
            queued: false,
            triggers: vec![],
            tags: vec![],
            files,
            uploads: saved.iter().map(|s| s.file.clone()).collect(),
            attached,
        };
        // Scrubbed here, as the log will store it (`append` scrubs every
        // payload): the model is sent this very message, so it never gets
        // a node secret the log does not hold, and the next request
        // rebuilds it byte for byte (review of PR 120, H2; PLAN.md T59).
        // Appended as [`Append::Scrubbed`], not scrubbed a second time.
        let (kind, mut v) = EventKind::UserMessage(msg).into_parts();
        self.node.cfg.secrets.scrub_event(&kind, &mut v);
        match serde_json::from_value(v) {
            Ok(m) => Ok((m, saved)),
            Err(e) => {
                self.rollback_uploads(&saved).await;
                Err(e.to_string())
            }
        }
    }

    /// The files the master keeps for this plain chat, as the message
    /// records them (T95): checked on this node's own limits (the count and
    /// size of `[limits]`), the chat a plain one that is not a sub-agent's,
    /// the tool offered here; each id once and in the form an id has, each
    /// hash a sha256, names and types cleaned. Nothing is written: the
    /// bytes stay on the master.
    fn check_attached(
        &self,
        refs: Vec<harness_protocol::AttachedRef>,
    ) -> Result<Vec<harness_core::event::AttachedFile>, String> {
        if refs.is_empty() {
            return Ok(vec![]);
        }
        let tool = harness_core::attached::TOOL;
        if self.profile != Profile::Chat {
            return Err("a chat with its own folder saves attached files there".into());
        }
        if self.parent_session.is_some() {
            return Err("a sub-agent's chat takes no attached files".into());
        }
        if !self.node.cfg.backend.has_tool(tool) || self.node.cfg.policy.denies(tool) {
            return Err("this node does not read attached files".into());
        }
        let limits = &self.node.cfg.policy.limits;
        if refs.len() > limits.uploads_per_message {
            return Err(format!(
                "at most {} attached files per message on this node",
                limits.uploads_per_message
            ));
        }
        let max = limits.upload_max_mb.saturating_mul(1 << 20);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(refs.len());
        for r in refs {
            let id_ok = !r.file_id.is_empty()
                && r.file_id.len() <= harness_core::attached::MAX_ID_CHARS
                && r.file_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
            if !id_ok || !seen.insert(r.file_id.clone()) {
                return Err("an attached file is named twice or by a bad id".into());
            }
            let sha_ok = r.sha256.len() == 64 && r.sha256.chars().all(|c| c.is_ascii_hexdigit());
            if !sha_ok || r.size == 0 || r.size > max {
                return Err(format!(
                    "{}: this node reads attached files up to {} MiB",
                    harness_core::event::shown_name(&r.name),
                    limits.upload_max_mb
                ));
            }
            out.push(harness_core::event::AttachedFile {
                file_id: r.file_id,
                name: harness_core::event::shown_name(&r.name),
                bytes: r.size,
                sha256: r.sha256.to_ascii_lowercase(),
                mime: harness_core::text::clean_line(&r.mime, 80),
            });
        }
        Ok(out)
    }

    /// Whether this chat takes files the model cannot read as text: an
    /// agentic chat with its own folder (never a plain chat, never a
    /// sub-agent's chat), on a node that stages uploads.
    pub(crate) fn accepts_uploads(&self) -> Result<(), String> {
        if self.profile != Profile::Agentic {
            return Err(crate::uploads::NO_TOOLS.into());
        }
        if self.parent_session.is_some() {
            return Err("a sub-agent's chat takes no attached files".into());
        }
        if !self.node.uploads.enabled() {
            return Err("this node saves no uploaded files".into());
        }
        Ok(())
    }

    /// Save the staged files a prompt names into the chat's folder, under
    /// this node's own limits (T94). All or none: a file that fails takes
    /// back the ones saved before it.
    async fn save_uploads(
        &self,
        refs: Vec<harness_protocol::UploadRef>,
    ) -> Result<Vec<crate::uploads::Saved>, String> {
        if refs.is_empty() {
            return Ok(vec![]);
        }
        self.accepts_uploads()?;
        let limits = &self.node.cfg.policy.limits;
        if refs.len() > limits.uploads_per_message {
            return Err(format!(
                "at most {} attached files per message on this node",
                limits.uploads_per_message
            ));
        }
        // One file, one name in the message: an honest master cannot send the
        // same upload twice, and a hostile one saves nothing by it.
        let mut seen = std::collections::HashSet::new();
        if !refs.iter().all(|r| seen.insert(r.upload_id.as_str())) {
            return Err("an attached file is named more than once in this message".into());
        }
        let max = limits.upload_max_mb.saturating_mul(1 << 20);
        if let Some(r) = refs.iter().find(|r| r.size == 0 || r.size > max) {
            return Err(format!(
                "{}: this node saves files up to {} MiB",
                harness_core::event::shown_name(&r.name),
                limits.upload_max_mb
            ));
        }
        let folder = self.checked_folder()?;
        let mut saved: Vec<crate::uploads::Saved> = Vec::with_capacity(refs.len());
        for r in &refs {
            let staged = match self.node.uploads.ready(self.id, r) {
                Ok(p) => p,
                Err(e) => {
                    self.rollback_uploads(&saved).await;
                    return Err(e);
                }
            };
            // The name is text: scrubbed before the file is named, so the
            // path the event and the model's line give is the real one.
            let (dir, mut r2) = (folder.clone(), r.clone());
            let secrets = self.node.cfg.secrets.clone();
            r2.name = crate::uploads::scrubbed_name(&r.name, |n| secrets.scrub(n));
            let one = tokio::task::spawn_blocking(move || {
                crate::uploads::save_file(&dir, &staged, &r2.name, &r2.mime, r2.size, &r2.sha256)
            })
            .await
            .map_err(|e| format!("saving failed: {e}"))
            .and_then(|r| r);
            match one {
                Ok(s) => saved.push(s),
                Err(e) => {
                    self.rollback_uploads(&saved).await;
                    return Err(e);
                }
            }
        }
        // Saved: the staged copies are not needed again.
        for r in &refs {
            self.node.uploads.discard(self.id, &r.upload_id);
        }
        Ok(saved)
    }

    /// Take back files saved for a message that was not recorded.
    async fn rollback_uploads(&self, saved: &[crate::uploads::Saved]) {
        if saved.is_empty() {
            return;
        }
        let Ok(folder) = self.checked_folder() else {
            return;
        };
        let saved = saved.to_vec();
        let _ = tokio::task::spawn_blocking(move || {
            for s in &saved {
                crate::uploads::rollback(&folder, s);
            }
        })
        .await;
    }

    /// A context like a `read` call's of this session (T63): its folder
    /// and that folder's pin, a sub-agent's own folder, the node's
    /// protected folders and node.toml's `allow_outside_workspace`.
    fn read_ctx(&self) -> harness_tools::ToolCtx {
        let mut ctx = harness_tools::ToolCtx::new(self.cwd.clone());
        ctx.pin = self.pin();
        ctx.owned = self.owned();
        ctx.protected = Arc::new(self.node.cfg.policy.protected_dirs.clone());
        ctx.allow_outside_workspace = self.node.cfg.policy.allow_outside_workspace;
        ctx
    }

    /// The files a message mentions (`@path`, T63), read now under the
    /// `read` tool's rules (`harness_tools::mention`): the person's own
    /// act, so no approval card is asked for, as none is for an attached
    /// file; but node.toml's `denied_tools = ["read"]` holds (the node
    /// decides) and each is then named, not read. Only in an agentic chat;
    /// waited for at most the instruction files' bound.
    async fn read_mentions(&self, files: Vec<String>) -> Result<Vec<FileMention>, String> {
        use crate::instructions::{self as ins, Bounded};
        if files.is_empty() {
            return Ok(vec![]);
        }
        if self.profile != Profile::Agentic {
            return Err("this chat has no folder, so it has no files to mention".into());
        }
        if files.len() > harness_protocol::MAX_PROMPT_FILES
            || files
                .iter()
                .any(|f| f.len() > harness_protocol::MAX_PROMPT_FILE_BYTES)
        {
            return Err(format!(
                "at most {} mentioned files per message, each path at most {} bytes",
                harness_protocol::MAX_PROMPT_FILES,
                harness_protocol::MAX_PROMPT_FILE_BYTES
            ));
        }
        let ctx = self.read_ctx();
        let denied = self.node.cfg.policy.denies("read");
        match ins::bounded(
            move || harness_tools::mention::read_all(&ctx, &files, denied),
            &CancellationToken::new(),
            ins::READ_TIMEOUT,
        )
        .await
        {
            // File content, scrubbed as a `read` result is.
            Bounded::Done(mut m) => {
                let secrets = &self.node.cfg.secrets;
                for f in &mut m {
                    f.text = secrets.scrub(&f.text);
                    f.path = secrets.scrub(&f.path);
                }
                // A path that names nothing is the person's text, not a
                // mention (review of PR 120): no note to the model.
                m.retain(|f| f.state != MentionState::Missing);
                Ok(m)
            }
            Bounded::TimedOut => Err(format!(
                "the mentioned files took longer than {} s to read; send the message again, or \
                 without them",
                ins::READ_TIMEOUT.as_secs()
            )),
            Bounded::Cancelled | Bounded::Failed(_) => {
                Err("the mentioned files could not be read".into())
            }
        }
    }

    /// Paths of this chat's folder for the composer's `@` completion
    /// (T63, `harness_tools::mention::complete`): none in a chat without a
    /// folder, none on a node that denies `read`, none in a folder that no
    /// longer passes its checks.
    pub(crate) async fn complete_files(&self, query: String) -> Result<Vec<String>, String> {
        use crate::instructions::{self as ins, Bounded};
        if self.profile != Profile::Agentic
            || self.node.cfg.policy.denies("read")
            || self.check_cwd_still_allowed().is_err()
        {
            return Ok(vec![]);
        }
        let ctx = self.read_ctx();
        match ins::bounded(
            move || harness_tools::mention::complete(&ctx, &query),
            &CancellationToken::new(),
            ins::READ_TIMEOUT,
        )
        .await
        {
            Bounded::Done(r) => r,
            Bounded::TimedOut => Err("listing the folder took too long".into()),
            Bounded::Cancelled | Bounded::Failed(_) => Err("the folder could not be listed".into()),
        }
    }

    /// The page's preview of a Word or Excel file of this chat's folder
    /// (X.59, T99): the path is resolved as `read` resolves it (the pinned
    /// folder, a sub-agent's own, the protected places) but never outside
    /// the folder, whatever `allow_outside_workspace` says; a node that
    /// denies `read` shows nothing; a sandboxed and a host chat are read the
    /// same way (the sandbox confines the agent's shell, not the node's own
    /// read). Every refusal to read is the one `NotFound`.
    pub(crate) async fn preview_folder_file(
        &self,
        path: &str,
        sheet: Option<u32>,
    ) -> Result<serde_json::Value, harness_protocol::PreviewRefusal> {
        use harness_protocol::PreviewRefusal as R;
        if self.profile != Profile::Agentic
            || self.node.cfg.policy.denies("read")
            || self.check_cwd_still_allowed().is_err()
        {
            return Err(R::NotFound);
        }
        let helper = crate::docs::shared_helper().ok_or(R::NotAvailable)?;
        let mut ctx = self.read_ctx();
        ctx.allow_outside_workspace = false;
        use harness_tools::folder_preview::Refusal as F;
        harness_tools::folder_preview::preview(&ctx, helper.as_ref(), path, sheet)
            .await
            .map_err(|r| match r {
                F::NotFound => R::NotFound,
                F::FileTooLarge => R::FileTooLarge,
                F::TooLarge => R::TooLarge,
                F::Empty => R::Empty,
                F::NotOffice => R::NotOffice,
                F::Protected => R::Protected,
                F::Damaged => R::Damaged,
                F::NoSheet => R::NoSheet,
                F::Busy => R::Busy,
                F::NotAvailable => R::NotAvailable,
                F::Locked => R::Locked,
                F::NoWrite => R::NoWrite,
            })
    }

    /// A person's text changes to a Word or Excel file of this chat's folder,
    /// saved as a new file beside it (X.60, T100). Read as the preview reads
    /// it (the pinned folder, a sub-agent's own, the protected places, never
    /// outside the folder, never `.git`); written by the create-only write of
    /// `doc_edit` through the write side of the path policy. The node decides:
    /// a node that denies `read`, or the editing tool for this kind of file
    /// (node.toml `denied_tools`), or that does not offer it, writes nothing,
    /// whatever the page asks. Every refusal to read is the one `NotFound`,
    /// and no file-system error text leaves this function.
    pub(crate) async fn edit_folder_file(
        &self,
        path: &str,
        sheet: Option<u32>,
        changes: Vec<harness_protocol::PageChange>,
    ) -> Result<Saved, harness_protocol::PreviewRefusal> {
        use harness_protocol::PreviewRefusal as R;
        if self.profile != Profile::Agentic
            || self.node.cfg.policy.denies("read")
            || self.check_cwd_still_allowed().is_err()
        {
            return Err(R::NotFound);
        }
        // The same tools the agent's own edit is gated by. The extension
        // picks the tool; a name of neither kind needs one of them to
        // exist, and says nothing about the path.
        let usable = |tool: &str| crate::docs::page_edit_usable(&self.node.cfg, tool);
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let allowed = match ext.as_str() {
            "docx" | "docm" => usable(harness_core::docs::EDIT_DOC_TOOL),
            "xlsx" | "xlsm" => usable(harness_core::docs::EDIT_SHEET_TOOL),
            _ => {
                usable(harness_core::docs::EDIT_DOC_TOOL)
                    || usable(harness_core::docs::EDIT_SHEET_TOOL)
            }
        };
        if !allowed {
            return Err(R::NoWrite);
        }
        let helper = crate::docs::shared_helper().ok_or(R::NotAvailable)?;
        let mut ctx = self.read_ctx();
        ctx.allow_outside_workspace = false;
        let changes = changes
            .into_iter()
            .map(|c| harness_tools::folder_edit::Change {
                at: c.at,
                text: c.text,
            })
            .collect();
        use harness_tools::folder_edit::Edited;
        use harness_tools::folder_preview::Refusal as F;
        let done = harness_tools::folder_edit::edit(&ctx, helper.as_ref(), path, sheet, changes)
            .await
            .map_err(|r| match r {
                F::NotFound => R::NotFound,
                F::FileTooLarge => R::FileTooLarge,
                F::TooLarge => R::TooLarge,
                F::Empty => R::Empty,
                F::NotOffice => R::NotOffice,
                F::Protected => R::Protected,
                F::Damaged => R::Damaged,
                F::NoSheet => R::NoSheet,
                F::Busy => R::Busy,
                F::NotAvailable => R::NotAvailable,
                F::Locked => R::Locked,
                F::NoWrite => R::NoWrite,
            })?;
        let words = |rs: Vec<harness_tools::folder_edit::ChangeOutcome>| {
            rs.into_iter()
                .map(|r| harness_protocol::ChangeResult {
                    ok: r.ok,
                    why: r.why.map(change_failure),
                })
                .collect::<Vec<_>>()
        };
        Ok(match done {
            Edited::Saved { path, results } => Saved {
                path: Some(path),
                results: words(results),
            },
            Edited::Refused(results) => Saved {
                path: None,
                results: words(results),
            },
        })
    }

    /// Free the turn slot if `turn_id` still holds it.
    fn release(&self, turn_id: &str) {
        let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
        if t.as_ref().is_some_and(|t| t.turn_id == turn_id) {
            *t = None;
            // Ends queued while it ran are recorded after it.
            self.bg_flush.notify_one();
        }
    }

    /// The environment line for turn `turn_id` (`crate::environment`),
    /// composed now, before its `turn_started` records it, and kept for
    /// the turn's run: the node's clock, OS, shell and the repository's
    /// `HEAD`, read in the instruction files' bound. `None` for a chat or
    /// a folder that fails its check.
    async fn environment_line(&self, turn_id: &str) -> Option<String> {
        use crate::instructions::{self as ins, Bounded};
        if self.profile != Profile::Agentic {
            return None;
        }
        let dir = self.checked_folder().ok()?;
        let granted = match self.owned() {
            Some(o) => o.top.clone(),
            None => self.node.cfg.policy.root_of(&dir).ok()?,
        };
        let protected = self.node.cfg.policy.protected_dirs.clone();
        let now = jiff::Zoned::now();
        let read = ins::bounded(
            move || crate::environment::repo(&dir, &granted, &protected),
            &CancellationToken::new(),
            ins::READ_TIMEOUT,
        )
        .await;
        let repo = match read {
            Bounded::Done(r) => crate::environment::Looked::Read(r),
            Bounded::TimedOut => crate::environment::Looked::TimedOut,
            Bounded::Cancelled | Bounded::Failed(_) => crate::environment::Looked::Failed,
        };
        let mode = self.exec_mode_now();
        let was = *self
            .last_turn_mode
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let line = crate::environment::line(&now, &repo, mode, was);
        *self.turn_env.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((turn_id.to_string(), line.clone()));
        Some(line)
    }

    /// `extra` with the repository's instruction files
    /// ([`crate::instructions`]) in `instructions`, read now from this
    /// session's checked folder, for every agentic turn: the person's, a
    /// parent's task (a sub-agent reads its own folder: its parent's, or
    /// the copy of it the child works in) and a callback turn. Capped for
    /// the answering model's known window; waited for at most
    /// [`crate::instructions::READ_TIMEOUT`] and not past `cancel`. A
    /// folder that fails its check adds nothing here (the turn's own checks
    /// refuse it).
    ///
    /// When the agent may use the `skill` tool here, the skills found in
    /// the same read (`harness_tools::skills`, node.toml `[skills]`) go to
    /// `skills`, and their catalogue after the files, read in a bound of
    /// their own. `invoking`: the person's own message (never a delegated
    /// task or a callback turn), which invokes a skill with `/name`.
    async fn with_instructions(
        &self,
        mut extra: TurnExtra,
        agent: Option<&AgentSpec>,
        invoking: Option<&str>,
        cancel: &CancellationToken,
    ) -> TurnExtra {
        use crate::instructions::{self as ins, Bounded};
        use harness_tools::skills;
        let policy = &self.node.cfg.policy;
        // The `skill` tool here, and the master's stored skills for it
        // (X.34 step B, PLAN.md T81) where node.toml takes them.
        let skill_on = agent.is_none_or(|a| a.allows_tool(skills::NAME))
            && !policy.denies(skills::NAME)
            && self.node.cfg.backend.has_tool(skills::NAME);
        let stored: &[harness_core::skills::StoredSkillEntry] =
            match agent.filter(|_| skill_on && policy.skills.from_master) {
                Some(a) => &a.stored_skills,
                None => &[],
            };
        let denied = agent.map(|a| a.deny_skills.as_slice()).unwrap_or_default();
        // A chat on the master has no folder: stored skills only.
        if self.profile != Profile::Agentic {
            if !stored.is_empty() {
                let window = agent.and_then(|a| {
                    let model = a.model.clone().unwrap_or_else(|| self.model.clone());
                    a.context_windows.get(&model).copied()
                });
                let problems = ins::with_skills(
                    &mut extra,
                    Bounded::Done(Default::default()),
                    invoking,
                    denied,
                    stored,
                    skills::cap_for(window),
                );
                if !problems.is_empty() {
                    tracing::info!(session = %self.id, problems = ?problems, "skills: some were not used");
                }
            }
            return extra;
        }
        let Ok(dir) = self.checked_folder() else {
            return extra;
        };
        let granted = match self.owned() {
            Some(o) => o.top.clone(),
            None => match self.node.cfg.policy.root_of(&dir) {
                Ok(r) => r,
                Err(_) => return extra,
            },
        };
        let model = agent
            .and_then(|a| a.model.clone())
            .unwrap_or_else(|| self.model.clone());
        let window = agent.and_then(|a| a.context_windows.get(&model).copied());
        let cap = ins::cap_for(window);
        let mut roots = Vec::new();
        // The skill folders only for an agent that works on files or runs
        // commands: a skill folder is files (X.34 step B). One without has
        // the stored skills, which are text.
        if skill_on && agent.is_none_or(|a| a.works_on_files()) {
            // Checked when node.toml was loaded; a policy built in code
            // with a bad entry reads none of them.
            roots.extend(
                policy
                    .skills
                    .roots()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|path| skills::Root {
                        path,
                        origin: harness_core::skills::SkillOrigin::Node,
                        bound: None,
                    }),
            );
            if policy.skills.repository {
                roots.extend(skills::repo_roots(&dir, &granted));
            }
        }
        let protected = policy.protected_dirs.clone();
        // The instruction files first, in a bound of their own: a slow
        // skill folder must not cost the turn its AGENTS.md (review S3).
        // The node operator's own file (node.toml `[instructions]`) in a
        // bound of its own, beside the repository's: a stalled one costs
        // the turn neither the other's read (review of PR 120, H1).
        let files_protected = protected.clone();
        let user_file = policy.instructions.located.clone();
        let user_cap = ins::user_cap_for(window);
        let (user, files) = tokio::join!(
            async {
                match user_file {
                    Some(p) => Some(
                        ins::bounded(
                            move || ins::read_user(&p, user_cap),
                            cancel,
                            ins::READ_TIMEOUT,
                        )
                        .await,
                    ),
                    None => None,
                }
            },
            ins::bounded(
                move || ins::read(&dir, &granted, &files_protected, cap),
                cancel,
                ins::READ_TIMEOUT,
            )
        );
        let mut problems = user
            .map(|u| ins::with_user_file(&mut extra, u))
            .unwrap_or_default();
        match files {
            Bounded::Done(r) => {
                extra.instructions = r.text;
                problems.extend(r.problems);
            }
            // Not read this turn: the files the model has stay (X.1,
            // review R3); the note goes with this turn only.
            Bounded::TimedOut => {
                extra.instructions_unread = Some(ins::timed_out());
                problems.push(format!("timed out after {} s", ins::READ_TIMEOUT.as_secs()));
            }
            // The turn is stopping; the backend sees the cancel itself.
            Bounded::Cancelled => {
                extra.instructions_unread = Some(ins::not_read());
                extra.skills_unread = Some(ins::SKILLS_NOT_READ.into());
                return extra;
            }
            Bounded::Failed(e) => {
                tracing::warn!(session = %self.id, error = %e, "reading the instruction files failed");
                extra.instructions_unread = Some(ins::not_read());
            }
        }
        let skill_problems = if !roots.is_empty() {
            let found = ins::bounded(
                move || skills::discover(&roots, &protected),
                cancel,
                ins::READ_TIMEOUT,
            )
            .await;
            ins::with_skills(
                &mut extra,
                found,
                invoking,
                denied,
                stored,
                skills::cap_for(window),
            )
        } else if !stored.is_empty() {
            ins::with_skills(
                &mut extra,
                Bounded::Done(Default::default()),
                invoking,
                denied,
                stored,
                skills::cap_for(window),
            )
        } else {
            vec![]
        };
        if !problems.is_empty() {
            tracing::info!(
                session = %self.id,
                folder = %self.cwd.display(),
                problems = ?problems,
                "instruction files: some were not read in full"
            );
        }
        if !skill_problems.is_empty() {
            tracing::info!(
                session = %self.id,
                folder = %self.cwd.display(),
                problems = ?skill_problems,
                "skills: some were not used"
            );
        }
        extra
    }

    /// Run a turn that `begin` recorded.
    #[allow(clippy::too_many_arguments)]
    async fn run_turn(
        self: Arc<Self>,
        turn_id: String,
        user_text: String,
        history: Vec<RawEvent>,
        agent: Option<AgentSpec>,
        compare: Vec<String>,
        extra: TurnExtra,
        cancel: CancellationToken,
        by_stop: Arc<AtomicBool>,
    ) -> (TurnEndReason, Option<Usage>) {
        let checkpoint = self.checkpoint(&turn_id, CheckpointPhase::Before).await;
        if self.log_failed.load(Ordering::SeqCst) {
            // The checkpoint's event was not written: nothing else runs.
            let error = LogError("the checkpoint could not be recorded".into()).to_string();
            self.end_turn(&turn_id, TurnEndReason::Error, None, Some(error))
                .await;
            return (TurnEndReason::Error, None);
        }

        *self
            .connector_tools
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = agent
            .as_ref()
            .map(|a| a.connector_tools.clone())
            .unwrap_or_default();
        let extra_cause_none = extra.cause.is_none();
        let mut extra = self
            .with_instructions(
                extra,
                agent.as_ref(),
                // Only a person's own message invokes a skill (review S5).
                (self.parent_session.is_none() && extra_cause_none).then_some(user_text.as_str()),
                &cancel,
            )
            .await;
        // The environment line this turn's `turn_started` recorded: the
        // message is sent with exactly what the log rebuilds it with.
        extra.environment = self
            .turn_env
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .filter(|(t, _)| *t == turn_id)
            .map(|(_, l)| l);
        // Run the backend in its own task so a panic still ends the turn.
        let this = self.clone();
        let tid = turn_id.clone();
        let c = cancel.clone();
        let res = tokio::spawn(async move {
            let backend = this.node.cfg.backend.clone();
            backend
                .run_turn(TurnCtx {
                    session_id: this.id,
                    turn_id: tid,
                    history,
                    user_text,
                    cwd: this.cwd.clone(),
                    model: agent
                        .as_ref()
                        .and_then(|a| a.model.clone())
                        .unwrap_or_else(|| this.model.clone()),
                    agent,
                    compare,
                    parent_session: this.parent_session,
                    extra,
                    cancel: c,
                    host: &*this,
                })
                .await
        })
        .await;
        let (reason, usage, error) = match res {
            Ok(o) => (o.reason, o.usage, o.error),
            Err(e) => (
                TurnEndReason::Error,
                None,
                Some(format!("agent backend crashed: {e}")),
            ),
        };
        let mut collect = None;
        if let Some((root, before, head)) = checkpoint
            && !self.log_failed.load(Ordering::SeqCst)
        {
            self.checkpoint_after(&turn_id, &root, &before, head).await;
            collect = Some(root);
        }
        // The end is decided by who cancelled (review N-7): the node's stop
        // (`stopped`), or the person or the system before it
        // (`cancelled`). A turn the model had completed keeps `completed`
        // when the stop came during its after-turn checkpoint.
        let reason = match (cancel.is_cancelled(), reason) {
            (true, TurnEndReason::Error) | (false, _) => reason,
            (true, TurnEndReason::Completed) if by_stop.load(Ordering::SeqCst) => reason,
            (true, _) if by_stop.load(Ordering::SeqCst) => TurnEndReason::Stopped,
            (true, _) => TurnEndReason::Cancelled,
        };
        self.end_turn(&turn_id, reason, usage.clone(), error).await;
        // The checkpoint repository's garbage collection, when due, runs
        // after the turn has ended (A.18).
        if let Some(repo) = collect {
            crate::git::gc_in_background(repo);
        }
        (reason, usage)
    }

    /// Record the turn's error and end, each tried once, and free the slot.
    /// If the end could not be written, the next prompt writes it first.
    async fn end_turn(
        &self,
        turn_id: &str,
        reason: TurnEndReason,
        usage: Option<Usage>,
        error: Option<String>,
    ) {
        if let Some(message) = error {
            self.append_end(EventKind::Error(ErrorEvent {
                notice: false,
                turn_id: Some(turn_id.into()),
                message,
            }))
            .await;
        }
        // An approval whose answer was not written would stay pending in
        // the log after the turn: deny it first. If that cannot be written
        // either, the turn stays open for the next prompt to close.
        let unresolved =
            std::mem::take(&mut *self.unresolved.lock().unwrap_or_else(|p| p.into_inner()));
        let mut closed = true;
        for approval_id in unresolved {
            closed &= self
                .append_end(EventKind::ApprovalResolved(ApprovalResolved {
                    approval_id,
                    decision: Decision::Deny,
                    by: ResolvedBy::Cancelled,
                    note: None,
                }))
                .await;
        }
        let ended = closed
            && self
                .append_end(EventKind::TurnEnded(TurnEnded {
                    turn_id: turn_id.into(),
                    reason,
                    usage,
                }))
                .await;
        if !ended {
            self.unclosed.store(true, Ordering::SeqCst);
        }
        self.release(turn_id);
    }

    /// Where this session's checkpoints go, if it gets any.
    /// `Ok(None)`: this session takes none (a chat, checkpoints off, a
    /// sub-agent's folder); `Err`: it should, and cannot, with the reason.
    async fn checkpoint_repo(&self) -> Result<Option<crate::git::Repo>, String> {
        // A sub-agent's folder is removed when it ends: no checkpoints.
        if self.profile != Profile::Agentic
            || self.node.cfg.policy.checkpoints_keep == 0
            || self.owned().is_some()
        {
            return Ok(None);
        }
        let dir = self.checked_folder()?;
        crate::git::checkpoint_repo(
            &dir,
            self.node.cfg.checkpoints_dir.as_deref(),
            &self.bounds(&dir),
        )
        .await
    }

    /// Snapshot the workspace before a turn (agentic sessions).
    async fn checkpoint(
        &self,
        turn_id: &str,
        phase: CheckpointPhase,
    ) -> Option<(crate::git::Repo, String, Option<HeadAt>)> {
        let repo = match self.checkpoint_repo().await {
            Ok(r) => r?,
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "no checkpoint");
                self.no_checkpoint_notice(turn_id, &e).await;
                return None;
            }
        };
        let r = format!("refs/harness/{}/{turn_id}/before", self.id);
        let busy = crate::git::hold(&repo).await;
        let snap = crate::git::snapshot_full(&repo, &r, "harness: before turn").await;
        drop(busy);
        match snap {
            Ok(snap) => {
                let sha = snap.sha;
                // On failure the caller sees `log_failed` and stops; the
                // ref no event names goes.
                let recorded = self
                    .emit(EventKind::Checkpoint(Checkpoint {
                        turn_id: turn_id.into(),
                        phase,
                        git_ref: sha.clone(),
                        files: vec![],
                        excluded: snap.nested.shown,
                        excluded_more: snap.nested.more as u64,
                        head_moved: None,
                    }))
                    .await;
                if recorded.is_err() {
                    crate::git::delete_ref(&repo, &r).await;
                    return None;
                }
                self.special_notice(turn_id, &snap.special).await;
                self.pin_between_turns(turn_id, &repo, &sha).await;
                // Where HEAD points as the turn begins (X.42): kept in
                // memory for the `after` checkpoint, which says whether it
                // moved.
                let head = crate::git::head_now(&repo).await;
                Some((repo, sha, head))
            }
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint failed");
                self.no_checkpoint_notice(turn_id, &e).await;
                None
            }
        }
    }

    /// Tell the person, in the chat, that this turn has no checkpoint (so
    /// no undo), with the reason; an `error` note, as the chat shows today.
    /// A failed append sets `log_failed`, which ends the turn as for any
    /// other event.
    async fn no_checkpoint_notice(&self, turn_id: &str, reason: &str) {
        let _ = self
            .emit(EventKind::Error(ErrorEvent {
                notice: false,
                turn_id: Some(turn_id.into()),
                message: format!(
                    "This turn has no checkpoint, so it cannot be undone: the folder could \
                     not be saved ({reason})."
                ),
            }))
            .await;
    }

    /// Say in the chat which tracked files a snapshot held as gone because
    /// a named pipe, a socket or a device is there now (`git::add_tracked`).
    /// Once: the checkpoint index then no longer has them, so the next
    /// snapshot does not name them again.
    async fn special_notice(&self, turn_id: &str, special: &crate::git::Names) {
        if special.is_empty() {
            return;
        }
        let _ = self
            .emit(EventKind::Error(ErrorEvent {
                notice: false,
                turn_id: Some(turn_id.into()),
                message: format!(
                    "Left out of this turn's checkpoint: {} (a named pipe, socket or device \
                     where a file was; checkpoints hold files, links and folders only, so they \
                     hold the file as deleted, and an undo cannot bring the pipe back).",
                    special.listed()
                ),
            }))
            .await;
    }

    async fn checkpoint_after(
        &self,
        turn_id: &str,
        repo: &crate::git::Repo,
        before: &str,
        head_before: Option<HeadAt>,
    ) {
        let r = format!("refs/harness/{}/{turn_id}/after", self.id);
        let busy = crate::git::hold(repo).await;
        let res = async {
            // The folder replaced during the turn: nothing of what is there
            // now is snapshotted or named in the chat.
            self.checked_folder()?;
            let snap = crate::git::snapshot_full(repo, &r, "harness: after turn").await?;
            let files = crate::git::changed(repo, before, &snap.sha).await?;
            let head_moved = crate::git::head_moved(head_before, crate::git::head_now(repo).await);
            Ok::<_, String>((snap, files, head_moved))
        }
        .await;
        drop(busy);
        match res {
            Ok((snap, files, head_moved)) => {
                let sha = snap.sha.clone();
                let special = snap.special.clone();
                let changed_count = files.len();
                let ignore_changed: Vec<String> = files
                    .iter()
                    .map(|(_, p)| p)
                    .filter(|p| p.as_str() == ".gitignore" || p.ends_with("/.gitignore"))
                    .cloned()
                    .collect();
                let recorded = self
                    .emit(EventKind::Checkpoint(Checkpoint {
                        turn_id: turn_id.into(),
                        phase: CheckpointPhase::After,
                        git_ref: snap.sha,
                        files: files.into_iter().map(|(_, p)| p).collect(),
                        excluded: snap.nested.shown,
                        excluded_more: snap.nested.more as u64,
                        head_moved,
                    }))
                    .await;
                if recorded.is_ok() {
                    self.special_notice(turn_id, &special).await;
                    if let Some(message) = large_turn_note(changed_count) {
                        let _ = self
                            .emit(EventKind::Error(ErrorEvent {
                                notice: true,
                                turn_id: Some(turn_id.into()),
                                message,
                            }))
                            .await;
                    }
                    // S-6: files that ignore rules written during this turn
                    // now leave out are in neither snapshot, so an undo of
                    // the turn leaves them; say so.
                    if !ignore_changed.is_empty() {
                        let _ = self
                            .emit(EventKind::Error(ErrorEvent {
                                notice: false,
                                turn_id: Some(turn_id.into()),
                                message: format!(
                                    "This turn changed {}: files the new rules ignore are \
                                     not in this turn's checkpoint, so undoing the turn does \
                                     not remove the ones it made.",
                                    ignore_changed
                                        .iter()
                                        .take(crate::git::SHOWN_MAX)
                                        .map(|p| format!("`{p}`"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            }))
                            .await;
                    }
                    let group = format!("refs/harness/{}/{turn_id}", self.id);
                    self.pin_turn(turn_id, repo, &group, before, &sha).await;
                    *self.last_state.lock().unwrap_or_else(|p| p.into_inner()) = Some(sha);
                    crate::git::prune(repo, self.node.cfg.policy.checkpoints_keep).await;
                }
            }
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint failed");
                self.no_checkpoint_notice(turn_id, &e).await;
            }
        }
    }

    /// Copy what an undo of this turn needs into the checkpoint repository's
    /// own store (A.17, `git::pin_changed`), so it no longer depends on the
    /// user's repository keeping the old versions. When some are already
    /// gone (the turn itself rewrote history and pruned it) or the copy
    /// fails, the chat says the undo is incomplete, naming the files; the
    /// turn still completes. A failed append sets `log_failed` as for any
    /// other event.
    async fn pin_turn(
        &self,
        turn_id: &str,
        repo: &crate::git::Repo,
        group: &str,
        before: &str,
        after: &str,
    ) {
        let busy = crate::git::hold(repo).await;
        let pinned = crate::git::pin_changed(repo, group, before, after).await;
        drop(busy);
        let message = match pinned {
            Ok(p) if p.trees_gone => "This turn's undo is incomplete: the folder's earlier \
                 content was already gone from the git repository in this folder when the \
                 turn ended (its history was rewritten and cleaned up during the turn), so \
                 this turn cannot be undone."
                .to_string(),
            Ok(p) if p.missing.is_empty() => return,
            Ok(p) => format!(
                "This turn's undo is incomplete: the earlier versions of {} were already gone \
                 from the git repository in this folder when the turn ended (its history was \
                 rewritten and cleaned up during the turn), so undo cannot put them back and \
                 will refuse this turn.",
                p.missing.listed()
            ),
            Err(e) => {
                tracing::warn!(session = %self.id, error = %e, "checkpoint copies failed");
                format!(
                    "This turn's undo is incomplete: the earlier versions of the files it \
                     changed could not be copied into the checkpoint store ({e}), so undo \
                     depends on the git repository in this folder keeping them."
                )
            }
        };
        let _ = self
            .emit(EventKind::Error(ErrorEvent {
                notice: false,
                turn_id: Some(turn_id.into()),
                message,
            }))
            .await;
    }

    /// What the person changed in the folder since this session's last
    /// recorded state (their own edits and commits between turns) is copied
    /// like a turn's changes, in this turn's group, so a rewind across it
    /// does not depend on the user's repository either (A.17 review, F5).
    /// One tree diff of two snapshots that exist; no scan. A loss here is
    /// only logged: it is the person's own history, not this turn's.
    async fn pin_between_turns(&self, turn_id: &str, repo: &crate::git::Repo, before: &str) {
        let Some(prev) = self
            .last_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        else {
            return;
        };
        let group = format!("refs/harness/{}/{turn_id}", self.id);
        let busy = crate::git::hold(repo).await;
        let pinned = crate::git::pin_changed(repo, &group, &prev, before).await;
        drop(busy);
        match pinned {
            Ok(p) if !p.missing.is_empty() => tracing::warn!(
                session = %self.id, missing = %p.missing.listed(),
                "changes between turns: content already gone, not copied"
            ),
            Ok(_) => {}
            Err(e) => {
                tracing::debug!(session = %self.id, error = %e, "changes between turns not copied")
            }
        }
    }

    /// Record which parallel answer the conversation continues from.
    pub(crate) async fn select_variant(
        &self,
        group: String,
        message_id: String,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("wait for the answers to finish".into());
        }
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let known = events.iter().any(|e| {
            matches!(e.typed(), Ok(EventKind::AssistantMessage(m))
                if m.message_id == message_id && m.variant.as_ref().is_some_and(|v| v.group == group))
        });
        if !known {
            return Err("no such answer".into());
        }
        self.append(EventKind::VariantSelected(VariantSelected {
            group,
            message_id,
        }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Undo one turn's file changes, or rewind: undo it and every later
    /// turn (see `git::restore_plan`). In order:
    /// 1. the diff is read (a checkpoint whose tree is gone is refused);
    /// 2. the folder as it is now is snapshotted (the ref only);
    /// 3. the plan is made (the unforced checks), and exactly the blobs it
    ///    writes are checked and made safe (`git::secure_blobs`); a
    ///    refusal in 1 to 3 deletes the ref and logs nothing;
    /// 4. the snapshot is recorded as a `before_restore` checkpoint (so a
    ///    forced restore loses nothing);
    /// 5. the files are written; the restored state is recorded as that
    ///    checkpoint's `after` and as each undone turn's `restored`.
    ///
    /// Restoring the `before_restore` checkpoint's id undoes the restore.
    /// If the restore failed after step 2, that checkpoint has no `after`:
    /// restoring it then puts back the paths its event lists to their
    /// saved content, over the folder as it is (see `interrupted`).
    pub(crate) async fn restore(
        &self,
        turn_id: &str,
        force: bool,
        rewind: bool,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("wait for the running turn to finish".into());
        }
        // A command still running would write over what is restored (R17).
        if let Some(ids) = self
            .bg
            .as_ref()
            .map(|b| b.running())
            .filter(|i| !i.is_empty())
        {
            return Err(format!(
                "this chat's background commands are running ({}): they could write over what \
                 an undo puts back. Stop them, or wait for them to end, then undo",
                ids.join(", ")
            ));
        }
        self.restore_checked(turn_id, force, rewind).await
    }

    /// [`Self::restore`] after the caller made sure nothing of this chat
    /// runs: no turn, no background command. A caller that holds the turn
    /// slot itself (a conversation rewind that also puts the files back,
    /// `session_rewind.rs`) cannot go through `restore`, which would see
    /// its own claim as a running turn.
    pub(crate) async fn restore_checked(
        &self,
        turn_id: &str,
        force: bool,
        rewind: bool,
    ) -> Result<(), String> {
        self.check_cwd_still_allowed()?;
        let id = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let view = SessionView::fold(&events).map_err(|e| e.to_string())?;
        let cp = view
            .checkpoints
            .get(turn_id)
            .ok_or("this turn has no checkpoint")?;
        // A fork's copied turns are its parent's: their snapshots belong to
        // that chat (C.2). The fork's own turns undo as any.
        if cp.inherited {
            return Err(
                "this turn came from the chat this one was forked from; its changes are undone \
                 there, not here"
                    .into(),
            );
        }
        let incomplete = || "this turn's checkpoint is incomplete".to_string();
        let before = cp.before.clone().ok_or_else(incomplete)?;
        // A restore that failed after saving the folder: its
        // `before_restore` event lists the paths it may have written. (A
        // turn without its `after` stays incomplete: nothing says what it
        // changed.)
        let interrupted: Option<Vec<String>> = match &cp.after {
            Some(_) => None,
            None => Some(
                events
                    .iter()
                    .find_map(|e| match e.typed() {
                        Ok(EventKind::Checkpoint(c))
                            if c.turn_id == turn_id
                                && c.phase == CheckpointPhase::BeforeRestore =>
                        {
                            Some(c.files)
                        }
                        _ => None,
                    })
                    .ok_or_else(incomplete)?,
            ),
        };
        let after = cp.after.clone().unwrap_or_default();
        // Turns from this one on, in order, and the latest recorded state
        // (with when it was recorded, and when this turn's `after` was).
        let mut later: Vec<String> = Vec::new();
        let mut latest = after.clone();
        let mut latest_ms = i64::MAX;
        let mut after_ms = i64::MAX;
        let mut seen = false;
        for e in &events {
            if let Ok(EventKind::Checkpoint(c)) = e.typed() {
                if c.turn_id == turn_id {
                    seen = true;
                    if c.phase == CheckpointPhase::After && after_ms == i64::MAX {
                        after_ms = e.ts_ms;
                    }
                }
                if seen && c.phase == CheckpointPhase::After && !later.contains(&c.turn_id) {
                    later.push(c.turn_id.clone());
                }
                if !matches!(
                    c.phase,
                    CheckpointPhase::Before | CheckpointPhase::BeforeRestore
                ) {
                    latest = c.git_ref.clone();
                    latest_ms = e.ts_ms;
                }
            }
        }
        // The undone span (T57): from the turn's start to the expected
        // state's checkpoint (after it the changed-since check compares
        // content); an interrupted restore has none, so until now.
        let since = crate::others::span_start(&events, turn_id);
        let end = match (&interrupted, rewind) {
            (Some(_), _) => i64::MAX,
            (None, true) => latest_ms,
            (None, false) => after_ms,
        };
        let span = crate::others::Span {
            since,
            end,
            before: Some(before.clone()),
            after: match (&interrupted, rewind) {
                (Some(_), _) => None,
                (None, true) => Some(latest.clone()),
                (None, false) => Some(after.clone()),
            },
            own: crate::others::own_writes(&events, since, end),
            me: Some(self.id),
        };
        if !rewind && cp.restored {
            return Err("already undone".into());
        }
        let repo = self
            .checkpoint_repo()
            .await?
            .ok_or("checkpoints are not available for this folder any more")?;
        // Held until this restore ends, on every path.
        let _claim = self.claim_folder(&repo.root)?;
        // No garbage collection runs in the checkpoint repository while the
        // restore runs (it waits for one that is running): the copies
        // `secure_blobs` makes are protected by nothing else until the
        // files are written. Nothing below waits on a person.
        let busy = crate::git::hold(&repo).await;
        // `None`: an interrupted restore, put back over the folder as it is.
        let expected = match (&interrupted, rewind) {
            (Some(_), _) => None,
            (None, true) => Some(latest),
            (None, false) => Some(after),
        };
        for sha in std::iter::once(&before).chain(expected.as_ref()) {
            if !crate::git::exists(&repo, sha).await {
                return Err(format!(
                    "this checkpoint is no longer kept (only the latest {} changes are)",
                    self.node.cfg.policy.checkpoints_keep
                ));
            }
        }
        // The diff, read before the folder is saved: a checkpoint whose
        // tree is gone is refused here, leaving nothing behind.
        let checked = match &expected {
            Some(e) => Some(crate::git::restore_diff(&repo, &before, e).await?),
            None => None,
        };
        // The folder as it is, saved first: its own checkpoint, counted
        // against `checkpoints_keep` like a turn's. The same scan serves
        // the unforced check.
        let saved = format!("restore-{}", harness_core::new_id());
        let saved_ref = format!("refs/harness/{}/{saved}/before", self.id);
        let now = crate::git::snapshot_full(&repo, &saved_ref, "harness: before restore")
            .await
            .map_err(|e| {
                format!("nothing was restored: the folder could not be saved first ({e})")
            })?;
        let planned = async {
            let diff = match (checked, &interrupted) {
                (Some(d), _) => d,
                (None, files) => {
                    // An interrupted restore: back to its saved state, for
                    // the paths it may have written, over the folder now.
                    // Its "changed since" check compares the folder with
                    // itself, so it never refuses: edits made after the
                    // failed restore are overwritten without asking. They
                    // are in the snapshot just taken, which the note after
                    // the restore names.
                    let mut d = crate::git::restore_diff(&repo, &before, &now.sha).await?;
                    d.only(files.as_deref().unwrap_or_default());
                    d
                }
            };
            let plan = crate::git::restore_plan(&repo, &diff, &now.sha, !force).await;
            // Another chat's work in the folder since this turn began is
            // not this turn's to undo (T57): refused like a change made
            // after the turn, unless forced; a forced undo tells that chat.
            let touched: std::collections::BTreeSet<String> = match &plan {
                Ok(p) => p.touched.iter().map(|(_, p)| p.clone()).collect(),
                Err(_) => diff.paths().into_iter().collect(),
            };
            let foreign = self.foreign_work(&repo, &span, &touched).await;
            let plan = match plan {
                Ok(p) => p,
                // The refusals "Undo anyway" goes past also name it.
                Err(e)
                    if !foreign.is_empty()
                        && (e.contains("changed after the turn") || e.contains("Undo anyway")) =>
                {
                    return Err(format!("{e}; also, {}", foreign.why()));
                }
                Err(e) => return Err(e),
            };
            if !force && !foreign.is_empty() {
                return Err(foreign.refusal());
            }
            // Exactly the blobs the plan writes (it knows the folder's
            // nested repositories only from the snapshot just taken).
            crate::git::secure_blobs(&repo, &plan).await?;
            Ok::<_, String>((plan, foreign))
        }
        .await;
        let (plan, foreign) = match planned {
            Ok(t) => t,
            Err(e) => {
                crate::git::delete_ref(&repo, &saved_ref).await;
                return Err(e);
            }
        };
        let files: Vec<String> = plan.touched.iter().map(|(_, p)| p.clone()).collect();
        if let Err(e) = self
            .append(EventKind::Checkpoint(Checkpoint {
                turn_id: saved.clone(),
                phase: CheckpointPhase::BeforeRestore,
                git_ref: now.sha.clone(),
                files: files.clone(),
                excluded: now.nested.shown,
                excluded_more: now.nested.more as u64,
                head_moved: None,
            }))
            .await
        {
            crate::git::delete_ref(&repo, &saved_ref).await;
            return Err(format!("nothing was restored: {}", LogError(e.to_string())));
        }
        // The saved checkpoint is the way back, also if the write below
        // fails: the folder's versions of every path that differs from what
        // the restore writes are copied like a turn's (A.17); a loss is only
        // logged here.
        let group = format!("refs/harness/{}/{saved}", self.id);
        match crate::git::pin_changed(&repo, &group, &now.sha, &before).await {
            Ok(p) if !p.missing.is_empty() => tracing::warn!(
                session = %self.id, missing = %p.missing.listed(),
                "restore checkpoint: content already gone, not copied"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(session = %self.id, error = %e, "checkpoint copies failed"),
        }
        // From here on the saved state is in the log; say so on failure.
        let kept = |e: String| {
            format!(
                "{e}; the folder as it was before this restore is saved as \
                 checkpoint {saved} (ref {saved_ref})"
            )
        };
        // The folder is still the session's own right before the write.
        self.checked_folder().map_err(kept)?;
        crate::git::restore_paths(&repo, &plan, crate::git::WriteLimits::UNDO)
            .await
            .map_err(kept)?;
        let state = crate::git::snapshot_full(
            &repo,
            &format!("refs/harness/{}/{saved}/after", self.id),
            "harness: restored",
        )
        .await
        .map_err(kept)?;
        self.append(EventKind::Checkpoint(Checkpoint {
            turn_id: saved.clone(),
            phase: CheckpointPhase::After,
            git_ref: state.sha.clone(),
            files: files.clone(),
            excluded: state.nested.shown,
            excluded_more: state.nested.more as u64,
            head_moved: None,
        }))
        .await
        .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        let targets: Vec<String> = if rewind && interrupted.is_none() {
            later
                .into_iter()
                .filter(|t| view.checkpoints.get(t).is_some_and(|c| !c.restored))
                .collect()
        } else {
            vec![turn_id.to_string()]
        };
        for t in targets {
            self.append(EventKind::Checkpoint(Checkpoint {
                turn_id: t,
                phase: CheckpointPhase::Restored,
                git_ref: state.sha.clone(),
                files: files.clone(),
                excluded: plan.nested.shown.clone(),
                excluded_more: plan.nested.more as u64,
                head_moved: None,
            }))
            .await
            .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        }
        // Only when something about repositories changed, in words the
        // chat shows today (pre-existing ones are in `excluded` alone).
        let safety = interrupted.as_ref().map(|_| saved.as_str());
        if let Some(message) = restore_notice(files.len(), &plan, safety) {
            self.append(EventKind::Error(ErrorEvent {
                notice: false,
                turn_id: None,
                message,
            }))
            .await
            .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        }
        if let Some(message) = large_undo_note(files.len()) {
            self.append(EventKind::Error(ErrorEvent {
                notice: true,
                turn_id: None,
                message,
            }))
            .await
            .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        }
        if !foreign.is_empty() {
            // Forced over another chat's work: tell each such chat which of
            // its files were put back (nothing about this one; its turn
            // cannot start meanwhile, the folder is claimed), then say so
            // here, "told" only for a note that was written (review F7). A
            // crash between the files and these notes loses the notes.
            let sessions: Vec<(SessionId, Arc<Session>)> = {
                let map = self.node.sessions.lock().unwrap_or_else(|p| p.into_inner());
                foreign
                    .chats
                    .iter()
                    .filter_map(|(id, _, _)| map.get(id).map(|s| (*id, s.clone())))
                    .collect()
            };
            let mut told = 0usize;
            for (id, files, unlisted) in &foreign.chats {
                let Some((_, other)) = sessions.iter().find(|(s, _)| s == id) else {
                    continue;
                };
                let note = EventKind::Error(ErrorEvent {
                    notice: false,
                    turn_id: None,
                    message: crate::others::reverted_note(files, *unlisted),
                });
                match other.append(note).await {
                    Ok(_) => told += 1,
                    Err(e) => {
                        tracing::warn!(session = %id, error = %e, "could not tell a chat its files were put back")
                    }
                }
            }
            self.append(EventKind::Error(ErrorEvent {
                notice: false,
                turn_id: None,
                message: format!(
                    "This undo put back files another chat working in this folder changed or \
                     may have changed during the turn: {}.{}",
                    crate::others::listed(&foreign.all()),
                    match told {
                        0 => "",
                        1 => " That chat was told.",
                        _ => " Those chats were told.",
                    }
                ),
            }))
            .await
            .map_err(|e| kept(LogError(e.to_string()).to_string()))?;
        }
        *self.last_state.lock().unwrap_or_else(|p| p.into_inner()) = Some(state.sha.clone());
        crate::git::prune(&repo, self.node.cfg.policy.checkpoints_keep).await;
        drop(busy);
        crate::git::gc_in_background(repo);
        Ok(())
    }

    /// Other chats' work in this folder over `span` on the files an undo
    /// writes (`others::foreign_work`): the chats on this node whose folder
    /// overlaps the checkpoint's, other than this one (its sub-agents count
    /// for the turns its calls started them in: see the body), and the
    /// write tombstones of deleted ones (pruned here once older
    /// than every checkpoint kept for this folder). None of the chats can
    /// be running: the folder is claimed (or, for a diff, it is a view).
    async fn foreign_work(
        &self,
        repo: &crate::git::Repo,
        span: &crate::others::Span,
        touched: &std::collections::BTreeSet<String>,
    ) -> crate::others::Foreign {
        let all: Vec<(SessionId, Option<SessionId>, PathBuf)> = self
            .node
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .map(|s| (s.id, s.parent_session, s.cwd.clone()))
            .collect();
        // This chat's sub-agents (and theirs) work for its turns only in the
        // turns its own `delegate` calls started: the delegation's first
        // turn and a follow-up it sent. What they write then is the turn's
        // (the checkpoint holds it, Undo takes it back). Any other turn of
        // theirs (a person's message to that chat, a callback) is another
        // chat's work, and so is what a background one does outside this
        // chat's turns (PLAN.md T32, T57).
        let turns = self.turn_spans().await;
        let mut own: HashMap<SessionId, Vec<(i64, i64)>> = HashMap::new();
        own.insert(self.id, turns);
        let mut todo = vec![self.id];
        while let Some(up) = todo.pop() {
            let up_own = own.get(&up).cloned().unwrap_or_default();
            for (id, _, _) in all.iter().filter(|(_, p, _)| *p == Some(up)) {
                if own.contains_key(id) {
                    continue;
                }
                let started = self.delegated_spans(*id).await;
                own.insert(*id, crate::others::within(&started, &up_own));
                todo.push(*id);
            }
        }
        let root = real_path(&repo.root);
        let ids: Vec<(SessionId, PathBuf)> = all
            .iter()
            .filter(|(id, _, _)| *id != self.id)
            .map(|(id, _, cwd)| (*id, real_path(cwd)))
            .filter(|(_, cwd)| overlaps(cwd, &root))
            .collect();
        let sessions: Vec<(Arc<Session>, PathBuf)> = {
            let map = self.node.sessions.lock().unwrap_or_else(|p| p.into_inner());
            ids.into_iter()
                .filter_map(|(id, cwd)| map.get(&id).cloned().map(|s| (s, cwd)))
                .collect()
        };
        let mut others = Vec::with_capacity(sessions.len());
        for (s, cwd) in sessions {
            // Its own checkpoints, where its turns' changes are read
            // (none: every file the undo writes counts for it).
            let theirs = s.checkpoint_repo().await.ok().flatten();
            others.push(crate::others::Other {
                id: s.id,
                cwd,
                repo: theirs,
                // A sub-agent of this chat: only what it did outside the
                // turns this chat's calls started is another chat's.
                during: own.get(&s.id).cloned(),
            });
        }
        let store = &self.node.cfg.store;
        let folder = root.to_string_lossy().into_owned();
        if let Some(oldest) = crate::git::oldest_kept_ms(repo).await {
            let f = folder.clone();
            if let Err(e) = store
                .call(move |s| s.prune_write_tombstones(&f, oldest))
                .await
            {
                tracing::warn!(error = %e, "write tombstones not pruned");
            }
        }
        let tombstones = match store.call(|s| s.write_tombstones()).await {
            Ok(t) => t,
            // Unreadable: as if a deleted chat had worked here.
            Err(e) => {
                tracing::warn!(error = %e, "write tombstones unreadable");
                vec![(folder, i64::MIN, i64::MAX)]
            }
        };
        crate::others::foreign_work(store, repo, others, &tombstones, span, touched).await
    }

    /// The time spans of this chat's own turns (an open one runs until now).
    async fn turn_spans(&self) -> Vec<(i64, i64)> {
        let me = self.id;
        let events = self
            .node
            .cfg
            .store
            .call(move |s| s.events_since_turn_at(me, i64::MIN, &["turn_started", "turn_ended"]))
            .await
            .unwrap_or_default();
        let mut open: HashMap<String, i64> = HashMap::new();
        let mut spans = Vec::new();
        for e in &events {
            match e.typed() {
                Ok(EventKind::TurnStarted(t)) => {
                    open.insert(t.turn_id, e.ts_ms);
                }
                Ok(EventKind::TurnEnded(t)) => {
                    if let Some(from) = open.remove(&t.turn_id) {
                        spans.push((from, e.ts_ms));
                    }
                }
                _ => {}
            }
        }
        spans.extend(open.into_values().map(|from| (from, i64::MAX)));
        spans
    }

    /// The time spans of the turns of the sub-agent chat `child` that its
    /// parent's `delegate` call started: its first turn (nothing marks it
    /// but its place in the log, which `guard_first_turn` guarantees: a
    /// sub-agent's chat takes a first turn only from the delegation that
    /// created it; a fork's turns before the fork are another chat's copy,
    /// so a fork has none) and each follow-up
    /// (`turn_started.continued_by` names the call). Every other turn in
    /// that chat was started by someone else.
    async fn delegated_spans(&self, child: SessionId) -> Vec<(i64, i64)> {
        let events = self
            .node
            .cfg
            .store
            .call(move |s| {
                s.events_since_turn_at(
                    child,
                    i64::MIN,
                    &["session_created", "turn_started", "turn_ended"],
                )
            })
            .await
            .unwrap_or_default();
        let mut forked = false;
        let mut first = true;
        let mut open: HashMap<String, i64> = HashMap::new();
        let mut spans = Vec::new();
        for e in &events {
            match e.typed() {
                Ok(EventKind::SessionCreated(c)) => forked = c.forked.is_some(),
                Ok(EventKind::TurnStarted(t)) => {
                    let delegated = t.continued_by.is_some() || (first && !forked);
                    first = false;
                    if delegated {
                        open.insert(t.turn_id, e.ts_ms);
                    }
                }
                Ok(EventKind::TurnEnded(t)) => {
                    if let Some(from) = open.remove(&t.turn_id) {
                        spans.push((from, e.ts_ms));
                    }
                }
                _ => {}
            }
        }
        spans.extend(open.into_values().map(|from| (from, i64::MAX)));
        spans
    }

    /// Register a restore of `root` (the checkpoint's folder: the
    /// repository's top for a folder inside one) in the node's registry of
    /// folders being restored, refusing while another session has a turn
    /// running in an overlapping folder (the same, a containing or a
    /// contained one, by real path) or another restore of an overlapping
    /// folder is registered. Check and insert happen under the registry's
    /// lock, which a turn's start also takes (see [`Self::claim_turn`]), so
    /// no turn starts there while the restore runs. The entry goes when the
    /// returned guard drops, on every path. This session's own running turn
    /// (and so its sub-agents, which run inside it) is refused before this.
    /// Another session's sub-agent works in its own node-owned folder, but
    /// only inside its parent's running turn, and that parent's folder
    /// overlaps a restore here; its result is written into the parent's
    /// folder before that turn ends.
    fn claim_folder(&self, root: &std::path::Path) -> Result<RestoreClaim, String> {
        let root = real_path(root);
        // Real paths before the lock: no file system call under it.
        let others: Vec<Arc<Session>> = self
            .node
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|s| s.id != self.id)
            .cloned()
            .collect();
        let others: Vec<(Arc<Session>, PathBuf)> = others
            .into_iter()
            .map(|s| {
                let cwd = real_path(&s.cwd);
                (s, cwd)
            })
            .collect();
        let mut reg = self
            .node
            .restoring
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Under the registry's lock, which the stop takes after setting its
        // flag: either this sees the flag, or the stop sees this restore and
        // waits for it (review A19-4).
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        if reg.iter().any(|r| overlaps(r, &root)) {
            return Err(UNDO_RUNNING.into());
        }
        let busy: Vec<&Arc<Session>> = others
            .iter()
            .filter(|(s, cwd)| overlaps(cwd, &root) && s.is_running())
            .map(|(s, _)| s)
            .collect();
        if !busy.is_empty() {
            // This chat's own background sub-agents (none of its foreground
            // ones can run: its own turn does not): say which, and what to do.
            let mine = |s: &Session| {
                let mut at = s.parent_session;
                for _ in 0..64 {
                    match at {
                        Some(id) if id == self.id => return true,
                        Some(id) => {
                            at = others
                                .iter()
                                .find(|(o, _)| o.id == id)
                                .and_then(|(o, _)| o.parent_session);
                        }
                        None => return false,
                    }
                }
                false
            };
            if busy.iter().all(|s| mine(s)) {
                let mut names: Vec<String> = busy
                    .iter()
                    .map(|s| format!("@{}", s.running_agent().unwrap_or_else(|| "agent".into())))
                    .collect();
                names.sort();
                names.dedup();
                let (who, verb, them) = if names.len() == 1 {
                    ("a sub-agent", "is", "it")
                } else {
                    ("sub-agents", "are", "them")
                };
                return Err(format!(
                    "{who} of this chat ({}) {verb} still working in this folder: stop {them} or \
                     wait for {them} to finish",
                    names.join(", ")
                ));
            }
            return Err(
                "another chat is working in this folder right now; wait for it to finish \
                 or cancel it"
                    .into(),
            );
        }
        reg.push(root.clone());
        Ok(RestoreClaim {
            node: self.node.clone(),
            root,
        })
    }

    /// Take this session's turn slot for `turn`, unless a turn holds it or
    /// a restore of an overlapping folder is running (checked under the
    /// registry's lock, under which a restore registers).
    fn claim_turn(&self, turn: RunningTurn) -> Result<(), String> {
        let cwd = real_path(&self.cwd);
        let reg = self
            .node
            .restoring
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if reg.iter().any(|r| overlaps(r, &cwd)) {
            return Err(UNDO_RUNNING.into());
        }
        let mut t = self.turn.lock().unwrap_or_else(|p| p.into_inner());
        if t.is_some() {
            return Err(TURN_RUNNING.into());
        }
        // A person's `/compact` runs: its summary is taken first (X.20).
        if self
            .compacting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            return Err(harness_protocol::COMPACTING.into());
        }
        *t = Some(turn);
        // Asked again with the slot held (N.10b): the chat above either saw
        // this slot taken and refused its switch, or has raised its flag or
        // applied the new mode by now, which this reads. A check made
        // before the slot was taken could be stale by one switch.
        if let Err(e) = self.sub_agent_mode_mismatch() {
            *t = None;
            return Err(e);
        }
        drop(t);
        // The person (or a callback) goes first: a note being written is
        // dropped, and the next idle stretch writes it (X.16).
        self.cancel_note();
        Ok(())
    }

    /// The note of `cancel` is over: free its slot. A cancelled one's was
    /// taken by [`Self::cancel_note`] already (another may hold it now).
    fn end_note(&self, cancel: &CancellationToken) {
        let mut n = self.note.lock().unwrap_or_else(|p| p.into_inner());
        if !cancel.is_cancelled() {
            *n = None;
        }
    }

    /// Cancel the hand-off note being written, if one is.
    fn cancel_note(&self) {
        if let Some(c) = self.note.lock().unwrap_or_else(|p| p.into_inner()).take() {
            c.cancel();
        }
    }

    /// Run a delegated sub-task in a new child session and wait for it.
    #[allow(clippy::too_many_arguments)]
    async fn run_child(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
        folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        let p = self
            .start_child(
                agent,
                task,
                refs,
                observed,
                cancel,
                started,
                ChildRun::Foreground,
                folder,
            )
            .await?;
        Ok(self.run_prepared(p).await)
    }

    /// Run a prepared child's turn and give its outcome (merged back).
    async fn run_prepared(&self, p: Prepared) -> harness_core::backend::DelegateOutcome {
        let turn_id = p.msg.turn_id.clone();
        let (reason, usage) = p
            .child
            .clone()
            .run_turn(
                p.msg.turn_id.clone(),
                p.msg.model_text(),
                p.history,
                Some(p.agent),
                vec![],
                p.extra,
                p.cancel,
                p.by_stop,
            )
            .await;
        self.child_outcome(&p.child, p.iso, p.shared, &turn_id, reason, usage)
            .await
    }

    /// Make a new child session for a delegated sub-task, its folder (a
    /// writer's own) and its first turn's start, ready to run
    /// ([`Self::run_prepared`], or in the background, X.24). `max_tokens`:
    /// its own token cap, for a background child.
    #[allow(clippy::too_many_arguments)]
    async fn start_child(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
        run: ChildRun,
        folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<Prepared, String> {
        let max_tokens = run.max_tokens();
        // No new sub-agent while the node stops (its parent is cancelled
        // already; this is the window before the parent sees it).
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        let child_id = SessionId::new();
        let short: String = task.chars().take(60).collect();
        // The parent's folder is still its own before a child works there.
        let here = self.checked_folder()?;
        // A sub-agent that can change files works in this chat's folder
        // (`folder: "shared"`, the default) or, when it was asked for or
        // the node says so, in a folder of its own, so parallel sub-agents
        // do not trample each other and a child whose log failed leaves the
        // folder alone; a copy that cannot be made is not started.
        let iso = self.isolate(&agent, child_id, &here, folder).await?;
        let shared = iso.is_none() && self.can_change_files(&agent);
        let cwd = iso.as_ref().map(|i| i.cwd.clone()).unwrap_or(here);
        let child = match Session::create(
            self.node.clone(),
            child_id,
            NewSession {
                title: format!("@{}: {short}", agent.agent_id),
                cwd,
                model: agent.model.clone().unwrap_or_else(|| self.model.clone()),
                // Its parent's mode now; the parent's, as it is then, is its
                // ceiling at every call after (`current_mode`, T50).
                mode: self.current_mode(),
                profile: self.profile,
                agent_id: Some(agent.agent_id.clone()),
                incognito: self.incognito,
                parent_session: Some(self.id),
                // A sub-agent's chat has no background table at all.
                no_callbacks: true,
                // Its parent's, always (N.10a), as it is now (N.10b).
                exec_mode: self.exec_mode_now(),
                // A read-only grandchild works in its parent's node-owned folder.
                owned: iso
                    .as_ref()
                    .map(|i| i.child.owned())
                    .or_else(|| self.owned()),
                sub_folder: if iso.is_some() {
                    Some(harness_core::subagent::SubFolder::Own)
                } else {
                    shared.then_some(harness_core::subagent::SubFolder::Shared)
                },
            },
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                if let Some(i) = &iso {
                    crate::git::remove_folder(&i.child).await;
                }
                return Err(e.to_string());
            }
        };
        self.node
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(child_id, child.clone());
        started(child_id);
        let turn_id = new_id();
        // The only first turn this chat takes (`guard_first_turn`).
        *child
            .delegation_turn
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(turn_id.clone());
        // The parent turn's token-saving methods and its context from the
        // master; never the parent's history.
        let (savings, context, standing, began, parent_turn) = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| {
                (
                    t.savings.clone(),
                    t.context.clone(),
                    inherited(t.standing.as_ref(), &agent),
                    Some(t.began),
                    Some(t.turn_id.clone()),
                )
            })
            .unwrap_or_default();
        let parent_began = began.unwrap_or_else(std::time::SystemTime::now);
        // The parent turn this run is part of, or that it is a background
        // run of the chat's.
        let delegated_by = match run {
            ChildRun::Foreground => parent_turn.map(Delegation::Foreground),
            ChildRun::Background { .. } => Some(Delegation::Background),
        };
        let by_stop = Arc::new(AtomicBool::new(false));
        if let Err(e) = child.claim_turn(RunningTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
            savings: savings.clone(),
            context: context.clone(),
            standing: standing.clone(),
            publishes: Arc::default(),
            by_stop: by_stop.clone(),
            agent_id: Some(agent.agent_id.clone()),
            callback: false,
            compare: false,
            began: parent_began,
            delegated_by,
        }) {
            if let Some(i) = iso {
                crate::git::remove_folder(&i.child).await;
            }
            return Err(e);
        }
        // Claimed after a stop began (its folder took a while): refused, as
        // `prompt` does, so no child turn starts after the stop (R-7).
        if self.node.is_stopping() {
            child.release(&turn_id);
            if let Some(i) = iso {
                crate::git::remove_folder(&i.child).await;
            }
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // A sub-agent gets its task as text, never the images, and its
        // parent turn's context, quoted like the parent's.
        // Scrubbed as the log stores it (the parent model wrote it, maybe
        // from tool output): the child's model gets what the log holds and
        // its next request rebuilds byte for byte (review of PR 120).
        let msg = UserMessage {
            turn_id,
            text: self.node.cfg.secrets.scrub(&task),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            triggers: vec![],
            tags: vec![],
            files: vec![],
            uploads: vec![],
            attached: vec![],
        };
        let extra = TurnExtra {
            savings,
            context,
            standing,
            refs: Some(refs),
            observed: Some(observed),
            changes_since: Some(parent_began),
            max_tokens,
            ..Default::default()
        };
        let history = match child
            .begin(Some(&msg), &msg.turn_id, Some(&agent), true)
            .await
        {
            Ok((h, _)) => h,
            Err(e) => {
                child.release(&msg.turn_id);
                if let Some(i) = iso {
                    crate::git::remove_folder(&i.child).await;
                }
                return Err(e);
            }
        };
        Ok(Prepared {
            child,
            iso,
            shared,
            msg,
            history,
            agent,
            extra,
            cancel,
            by_stop,
        })
    }

    /// What a sub-agent's turn `turn_id` (its first or a follow-up) gives
    /// its parent: its changes merged back (`iso`, a writing child's
    /// folder), and its answer, `finish`, sources and end text read from
    /// that turn's part of its log only: a follow-up never reports an
    /// earlier run's words as its own.
    async fn child_outcome(
        &self,
        child: &Arc<Session>,
        iso: Option<Isolation>,
        shared: bool,
        turn_id: &str,
        reason: TurnEndReason,
        usage: Option<Usage>,
    ) -> harness_core::backend::DelegateOutcome {
        let child_id = child.id;
        // Each session's log gates its own effects: the parent's turn goes
        // on, but nothing of a child whose log failed (or whose parent's
        // did) reaches the parent's folder, since no event would record it.
        let log_failed =
            child.log_failed.load(Ordering::SeqCst) || self.log_failed.load(Ordering::SeqCst);
        let (merge_note, unapplied, applied) = match iso {
            Some(i) => {
                let (n, d, a) = self.merge_back(i, child_id, !log_failed).await;
                (Some(n), d, a)
            }
            None => (None, None, true),
        };
        let log = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(child_id, 0, usize::MAX >> 1))
            .await
            .unwrap_or_default();
        // This turn's events: from its `turn_started` on.
        let from = log
            .iter()
            .rposition(
                |e| matches!(e.typed(), Ok(EventKind::TurnStarted(t)) if t.turn_id == turn_id),
            )
            .unwrap_or(0);
        let log = &log[from..];
        let text = log
            .iter()
            .rev()
            .find_map(|e| match e.typed() {
                Ok(EventKind::AssistantMessage(m)) if !m.text.trim().is_empty() => Some(m.text),
                _ => None,
            })
            .unwrap_or_default();
        // Whatever the answer cites, the parent resolves through these.
        let sources = harness_core::refs::sources_in(log);
        let finish = harness_core::subagent::finish_in(log.iter().filter_map(|e| e.typed().ok()));
        // Why its turn ended, when it did not complete: the parent can tell
        // the step cap from the guard or a provider error.
        let error = log.iter().rev().find_map(|e| match e.typed() {
            Ok(EventKind::Error(err)) if !err.notice => Some(err.message),
            _ => None,
        });
        // The harness's own words, after the child's quoted report.
        let stopped = child
            .log_failed
            .load(Ordering::SeqCst)
            .then_some("the sub-agent stopped because its session log could not be written");
        // A sub-agent that worked in this chat's folder: nothing was merged,
        // so the lead is told where it worked and which files changed there
        // (from its turn's checkpoints; none when its record failed: then
        // nothing un-writes what it changed, and the turn's Undo is the way
        // back).
        let mut unapplied = unapplied;
        let merge_note = merge_note.or_else(|| {
            shared.then(|| {
                let files = log.iter().find_map(|e| match e.typed() {
                    Ok(EventKind::Checkpoint(c))
                        if c.turn_id == turn_id && c.phase == CheckpointPhase::After =>
                    {
                        Some(c.files)
                    }
                    _ => None,
                });
                // The names follow the note, quoted line by line.
                unapplied = harness_core::subagent::shared_files(files.as_deref(), log_failed);
                harness_core::subagent::shared_note(files.as_deref(), log_failed)
            })
        });
        let note = match (stopped, merge_note) {
            (Some(s), Some(n)) => Some(format!("{s}; {n}")),
            (Some(s), None) => Some(s.to_string()),
            (None, n) => n,
        };
        harness_core::backend::DelegateOutcome {
            child: child_id,
            text,
            reason,
            usage,
            sources,
            finish,
            error,
            note,
            unapplied,
            applied,
        }
    }

    /// Send `task` to `child_id`, a sub-agent this session started, as its
    /// next message, and wait for that turn (PLAN.md T68). The node decides
    /// from its own records, never from the id as the model wrote it: the
    /// child's `session_created` names this session as its parent, this
    /// session's log holds a finished `delegate` call whose result names
    /// it, it was started as `agent` (the delegate the call names, as the
    /// node bounds it now), it has had fewer than `MAX_CHILD_RUNS` runs and
    /// no turn of it runs. It runs with what a new child of that agent gets
    /// now (tools, the parent's mode as a ceiling at every call, the
    /// parent's execution mode, which it recorded at creation) and nothing
    /// more: one that changes files works in a fresh copy of this folder at
    /// the path its own had, merged back when it ends; one that only reads
    /// works in this folder, as before. Either way it keeps its
    /// conversation, not its files, and its opening says so.
    #[allow(clippy::too_many_arguments)]
    async fn run_continue(
        &self,
        child_id: SessionId,
        agent: AgentSpec,
        task: String,
        call_id: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        let p = self
            .start_continue(
                child_id,
                agent,
                task,
                call_id,
                refs,
                observed,
                cancel,
                started,
                ChildRun::Foreground,
            )
            .await?;
        Ok(self.run_prepared(p).await)
    }

    /// [`Self::run_continue`]'s checks and the follow-up's start, ready to
    /// run, in the foreground or in the background (X.24).
    #[allow(clippy::too_many_arguments)]
    async fn start_continue(
        &self,
        child_id: SessionId,
        agent: AgentSpec,
        task: String,
        call_id: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
        run: ChildRun,
    ) -> Result<Prepared, String> {
        let max_tokens = run.max_tokens();
        use harness_core::subagent::MAX_CHILD_RUNS;
        if self.node.is_stopping() {
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        // One sentence for unknown, deleted and foreign ids alike: the
        // model learns nothing about sessions that are not its own.
        let not_yours = || {
            format!(
                "`{child_id}` is not a sub-agent this chat started (a sub-agent can be given a \
                 follow-up only by the chat or sub-agent that started it, by the id its \
                 `delegate` result began with). Start a new one instead."
            )
        };
        let Some(child) = self.node.session(child_id) else {
            return Err(not_yours());
        };
        if child.parent_session != Some(self.id) || child.id == self.id {
            return Err(not_yours());
        }
        // Not in a mode its chat has left (N.10b): the model is told what
        // to do instead.
        child.sub_agent_mode_mismatch()?;
        // This session's own log names it as the result of one of its
        // `delegate` calls; the last such result says whether its changes
        // were applied.
        let me = self.id;
        let mine = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(me, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let head = format!("session: {child_id}\n");
        let mut delegate_calls = std::collections::HashSet::new();
        let mut last: Option<String> = None;
        // A background run of it (X.24): its entry while it runs, and what
        // its end said of its changes (none landed when the node restarted
        // before the end was delivered).
        let mut running_as: Option<String> = None;
        let mut landed_bg: Option<harness_core::subagent::Landed> = None;
        for e in &mine {
            match e.typed() {
                Ok(EventKind::ToolCallStarted(c)) if c.tool == "delegate" => {
                    delegate_calls.insert(c.call_id);
                }
                Ok(EventKind::ToolCallFinished(f))
                    if delegate_calls.contains(&f.call_id) && f.output.starts_with(&head) =>
                {
                    last = Some(f.output);
                    landed_bg = None;
                }
                Ok(EventKind::BackgroundStarted(b))
                    if b.child.as_ref().is_some_and(|c| c.session == child_id) =>
                {
                    running_as = Some(b.bg_id);
                }
                Ok(EventKind::BackgroundEnded(b))
                    if b.child.as_ref().is_some_and(|c| c.session == child_id) =>
                {
                    running_as = None;
                    landed_bg = b.child.map(|c| {
                        if c.result.is_empty() {
                            harness_core::subagent::Landed::None
                        } else {
                            harness_core::subagent::landed_in(&c.result)
                        }
                    });
                }
                _ => {}
            }
        }
        let Some(last) = last else {
            return Err(not_yours());
        };
        // Still running in the background, or ended with its result (and
        // a writer's merge) not delivered yet: one run at a time, and its
        // folder is not touched before its merge.
        if let Some(bg_id) = running_as.or_else(|| self.undelivered_child(child_id)) {
            return Err(format!(
                "@{} `{child_id}` is still running in the background as {bg_id} (or its result is \
                 on its way): one run at a time. You are told its result when it ends; give it \
                 the follow-up then, or stop it with bash_stop.",
                agent.agent_id
            ));
        }
        let landed_before = landed_bg.unwrap_or_else(|| harness_core::subagent::landed_in(&last));
        let theirs = self
            .node
            .cfg
            .store
            .call(move |s| s.events_after(child_id, 0, usize::MAX >> 1))
            .await
            .map_err(|e| e.to_string())?;
        let started_as = theirs.iter().find_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(m)) => Some(m.agent_id),
            _ => None,
        });
        let Some(Some(started_as)) = started_as else {
            return Err(not_yours());
        };
        if started_as != agent.agent_id {
            return Err(format!(
                "`{child_id}` is @{started_as}, not @{}: name @{started_as} to give it a \
                 follow-up, or start a new @{} without `session`.",
                agent.agent_id, agent.agent_id
            ));
        }
        let runs = theirs
            .iter()
            .filter(|e| matches!(e.typed(), Ok(EventKind::TurnStarted(_))))
            .count();
        if runs >= MAX_CHILD_RUNS {
            return Err(format!(
                "@{started_as} `{child_id}` has had {runs} runs, the most one sub-agent may have \
                 ({MAX_CHILD_RUNS}). Start a new one with a complete brief."
            ));
        }
        let here = self.checked_folder()?;
        // Whether it changed files when it was made: its folder was its own
        // (`<sessions>/<id>/files`). It changes files now only if a new one
        // of this agent would; it never gains a writable folder, and one
        // that wrote never works in this folder.
        let own_dir = self
            .sessions_dir()
            .map(|d| real_path(&d).join(child_id.to_string()));
        let wrote = own_dir.as_ref().is_some_and(|d| child.cwd.starts_with(d));
        // A child made in plan mode stays in it for life (its own recorded
        // mode is its floor, X.7): it changes nothing, whatever its agent
        // could, and works in this folder as it did.
        let made_in_plan = read_mode(&child.mode) == ApprovalMode::Plan;
        let writes = !made_in_plan && self.can_change_files(&agent);
        if wrote && self.current_mode() == ApprovalMode::Plan {
            return Err(format!(
                "[harness] plan mode: @{started_as} `{child_id}` was not continued: it changes \
                 files, and nothing may change in plan mode. Describe the work in your plan."
            ));
        }
        // A sub-agent that shares this chat's folder (a reader, or a writer
        // made with `folder: "shared"`: its folder is this one either way)
        // may be asked to write now where a new one of its agent would work
        // here too: the node's `shared`, and no writer under a sub-agent.
        let shared_now = !wrote
            && writes
            && self.node.cfg.policy.limits.sub_agent_folder
                == harness_core::subagent::SubFolder::Shared
            && self.parent_session.is_none();
        if !wrote && writes && !shared_now {
            // It worked in this chat's folder, and may not write there now.
            let why = if self.parent_session.is_some() {
                "a sub-agent's own sub-agents may only read: ask it to read, or make the change \
                 yourself"
            } else {
                "this node now gives every sub-agent that changes files a copy of its own \
                 (`sub_agent_folder = \"own\"`), and this one worked in this chat's folder. \
                 Start a new one instead"
            };
            return Err(format!(
                "@{started_as} `{child_id}` cannot be given a follow-up: its agent can change \
                 files now, but {why}."
            ));
        }
        if wrote != writes && !shared_now {
            return Err(format!(
                "@{started_as} `{child_id}` cannot be given a follow-up: its agent {} files now \
                 and it {} when it was started. Start a new one instead.",
                if writes {
                    "can change"
                } else {
                    "can no longer change"
                },
                if wrote { "could" } else { "could not" },
            ));
        }
        let turn_id = new_id();
        // As for a new child: the parent turn's context and its start (what
        // changed in the folder since is not taken as instructions, T65).
        let (savings, context, standing, began, parent_turn) = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| {
                (
                    t.savings.clone(),
                    t.context.clone(),
                    inherited(t.standing.as_ref(), &agent),
                    Some(t.began),
                    Some(t.turn_id.clone()),
                )
            })
            .unwrap_or_default();
        let parent_began = began.unwrap_or_else(std::time::SystemTime::now);
        // The parent turn this run is part of, or that it is a background
        // run of the chat's.
        let delegated_by = match run {
            ChildRun::Foreground => parent_turn.map(Delegation::Foreground),
            ChildRun::Background { .. } => Some(Delegation::Background),
        };
        let by_stop = Arc::new(AtomicBool::new(false));
        // Taken before its folder is touched: a second follow-up (another
        // call of the same step, or a person's message) is refused here.
        if child
            .claim_turn(RunningTurn {
                turn_id: turn_id.clone(),
                cancel: cancel.clone(),
                savings: savings.clone(),
                context: context.clone(),
                standing: standing.clone(),
                publishes: Arc::default(),
                by_stop: by_stop.clone(),
                agent_id: Some(agent.agent_id.clone()),
                callback: false,
                compare: false,
                began: parent_began,
                delegated_by,
            })
            .is_err()
        {
            return Err(format!(
                "@{started_as} `{child_id}` is still working: one message at a time. Wait for \
                 its result, or start a new one."
            ));
        }
        let iso = if wrote {
            match self
                .isolate(
                    &agent,
                    child_id,
                    &here,
                    Some(harness_core::subagent::SubFolder::Own),
                )
                .await
            {
                Ok(Some(i)) if i.cwd == child.cwd => Some(i),
                Ok(Some(i)) => {
                    crate::git::remove_folder(&i.child).await;
                    child.release(&turn_id);
                    return Err(format!(
                        "@{started_as} `{child_id}` cannot be given a follow-up: this folder is \
                         not where it was when the sub-agent started. Start a new one."
                    ));
                }
                Ok(None) => None,
                Err(e) => {
                    child.release(&turn_id);
                    // `isolate` speaks of a new child.
                    return Err(e.replacen("was not started", "was not continued", 1));
                }
            }
        } else {
            None
        };
        // Its folder as a new child's would be pinned: the fresh copy, or
        // this session's folder (with this session's own folder, for a
        // read-only child of a sub-agent).
        let (cwd, owned) = match &iso {
            Some(i) => (i.cwd.clone(), Some(i.child.owned())),
            None => (here.clone(), self.owned()),
        };
        if cwd != child.cwd {
            child.release(&turn_id);
            return Err(format!(
                "@{started_as} `{child_id}` cannot be given a follow-up: this folder is not \
                 where it was when the sub-agent started. Start a new one."
            ));
        }
        *child.pin_cell.lock().unwrap_or_else(|p| p.into_inner()) = pin_of(child.profile, &cwd);
        *child.owned_cell.lock().unwrap_or_else(|p| p.into_inner()) = owned;
        if self.node.is_stopping() {
            child.release(&turn_id);
            if let Some(i) = iso {
                crate::git::remove_folder(&i.child).await;
            }
            return Err(harness_protocol::NODE_STOPPING.into());
        }
        started(child_id);
        let note = if wrote {
            harness_core::subagent::follow_up_writes(landed_before)
        } else {
            harness_core::subagent::FOLLOW_UP_READS.to_string()
        };
        let msg = UserMessage {
            turn_id,
            text: self.node.cfg.secrets.scrub(&task),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            triggers: vec![],
            tags: vec![],
            files: vec![],
            uploads: vec![],
            attached: vec![],
        };
        let extra = TurnExtra {
            savings,
            context,
            standing,
            refs: Some(refs),
            observed: Some(observed),
            changes_since: Some(parent_began),
            max_tokens,
            ..Default::default()
        };
        let history = match child
            .begin_with(
                Some(&msg),
                &msg.turn_id,
                Some(&agent),
                Some((call_id.as_str(), note.as_str())),
                true,
            )
            .await
        {
            Ok((h, _)) => h,
            Err(e) => {
                child.release(&msg.turn_id);
                if let Some(i) = iso {
                    crate::git::remove_folder(&i.child).await;
                }
                return Err(e);
            }
        };
        Ok(Prepared {
            child,
            iso,
            shared: shared_now,
            msg,
            history,
            agent,
            extra,
            cancel,
            by_stop,
        })
    }

    /// Whether a sub-agent can change files here, so it needs a folder of
    /// its own: `AgentSpec::needs_own_folder` (PLAN.md T70) on its tools,
    /// narrowed by its own denials and by this node's policy. A reader's
    /// `bash` does not count (it runs in the read-only shell; the loop
    /// reads the same answer), nor do `publish` and MCP tools alone: those
    /// agents work in this folder, as on main.
    fn can_change_files(&self, agent: &AgentSpec) -> bool {
        let policy = &self.node.cfg.policy;
        // In plan mode (X.7) the child is made in plan mode and stays in it
        // (its own recorded mode is its floor for life): its `bash` is the
        // read-only shell and every tool that changes things is refused,
        // so it works in this folder, as a reader does.
        self.current_mode() != ApprovalMode::Plan
            && self.profile == Profile::Agentic
            && agent.needs_own_folder(|t| !policy.denies(t))
    }

    /// Prepare a folder for a sub-agent that can change files, seeded with
    /// the current working state (uncommitted edits included). `Ok(None)`:
    /// the sub-agent cannot change files and works in this folder; `Err`: it
    /// can, and no folder could be made, so it must not start. The text is
    /// the delegation's result, for the parent model. `here` is this
    /// session's checked folder.
    ///
    /// The folder is node-owned (`<data>/sessions/<child>/files`, see
    /// `git::ChildFolder`): outside the user's repository and outside every
    /// other session's writable area. Its files and its own git directory
    /// are written by the node before the child starts; no git process of
    /// the node runs with the user's git directory (R.1 follow-up).
    async fn isolate(
        &self,
        agent: &AgentSpec,
        child: SessionId,
        here: &std::path::Path,
        folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<Option<Isolation>, String> {
        use harness_core::subagent::SubFolder;
        if !self.can_change_files(agent) {
            return Ok(None);
        }
        let who = &agent.agent_id;
        let not_started = |e: String| {
            tracing::warn!(session = %self.id, error = %e, "no folder for a sub-agent that changes files; not started");
            format!(
                "@{who} was not started: it can change files (a tool that writes, publishes or \
                 is an MCP server's), and its own folder could not be made ({e}). Make the \
                 change yourself, or delegate to a sub-agent that only reads."
            )
        };
        // The node's word first: `own` there gives every writer a copy,
        // whatever the call asked; otherwise the call's (`shared` when it
        // left the field out). PLAN.md T32.
        let node_own = self.node.cfg.policy.limits.sub_agent_folder == SubFolder::Own;
        if !node_own && folder != Some(SubFolder::Own) {
            // In this chat's folder, which a sub-agent's own sub-agents
            // never get to write: one level of writers.
            if self.parent_session.is_some() {
                return Err(not_started(
                    "a sub-agent's own sub-agents may only read".to_string(),
                ));
            }
            return Ok(None);
        }
        if self.owned().is_some() {
            return Err(not_started(
                "a sub-agent's own sub-agents may only read".to_string(),
            ));
        }
        let Some(sessions) = self.sessions_dir() else {
            return Err(not_started(
                "this node keeps no checkpoint store, which sub-agent folders need".to_string(),
            ));
        };
        let sub = match crate::git::sub_repo(
            here,
            self.node.cfg.checkpoints_dir.as_deref(),
            &self.bounds(here),
        )
        .await
        {
            Ok(Some(s)) => s,
            Ok(None) => {
                let how = if node_own && folder == Some(SubFolder::Shared) {
                    "you asked for `folder: \"shared\"`, but this node's `sub_agent_folder = \
                     \"own\"` gives every sub-agent that changes files a copy of its own, and a \
                     copy needs a git repository; this folder is not one (within this node's \
                     workspace roots)"
                        .to_string()
                } else if node_own {
                    "this node's `sub_agent_folder = \"own\"` gives every sub-agent that changes \
                     files a copy of its own of a git repository, but this folder is not a git \
                     repository (within this node's workspace roots)"
                        .to_string()
                } else {
                    "`folder: \"own\"` gives a sub-agent that changes files a copy of its own of \
                     a git repository, but this folder is not a git repository (within this \
                     node's workspace roots); to work in this chat's folder instead, leave \
                     `folder` out"
                        .to_string()
                };
                return Err(format!(
                    "@{who} was not started: it can change files (a tool that writes, publishes \
                     or is an MCP server's), and {how}. Make the change yourself, delegate to \
                     a sub-agent that only reads, or ask the user to run `git init` in this \
                     folder."
                ));
            }
            Err(e) => return Err(not_started(e)),
        };
        let id = child.to_string();
        let base_ref = format!("refs/harness/{child}/base");
        let mut made: Option<crate::git::ChildFolder> = None;
        let res = async {
            let base = sub.base(&id).await?;
            let folder = sub.make_folder(&id, &base, &sessions).await?;
            made = Some(folder.clone());
            let rel = here
                .strip_prefix(&sub.repo.root)
                .unwrap_or(std::path::Path::new(""));
            let cwd = folder.path.join(rel);
            tokio::fs::create_dir_all(&cwd)
                .await
                .map_err(|e| format!("{}: {e}", cwd.display()))?;
            let cwd = cwd
                .canonicalize()
                .map_err(|e| format!("{}: {e}", cwd.display()))?;
            if !cwd.starts_with(&folder.path) {
                return Err(format!(
                    "{} leads out of the sub-agent's folder",
                    rel.display()
                ));
            }
            Ok::<_, String>(Isolation {
                sub: sub.clone(),
                child: folder,
                base,
                cwd,
            })
        }
        .await;
        match res {
            Ok(i) => Ok(Some(i)),
            Err(e) => {
                // Leave nothing behind: no half-made folder, no base ref.
                if let Some(f) = made {
                    crate::git::remove_folder(&f).await;
                }
                crate::git::delete_ref(&sub.repo, &base_ref).await;
                Err(not_started(e))
            }
        }
    }

    /// Bring a sub-agent's changes back into the main working directory:
    /// its folder is snapshotted in the shadow and the changed files are
    /// written into this session's folder with the restore machinery (no
    /// `git apply` under the user's config). `apply: false` keeps the
    /// result in the node's checkpoint store and changes nothing in the
    /// folder.
    async fn merge_back(
        &self,
        iso: Isolation,
        child: SessionId,
        apply: bool,
    ) -> (String, Option<String>, bool) {
        let id = child.to_string();
        let kept = "it is kept in this node's checkpoint store (sub-agent snapshots)";
        let (mut note, unapplied, landed) = match iso.sub.result(&iso.child, &id).await {
            Err(e) => (
                format!("its changes could not be collected: {e}"),
                None,
                false,
            ),
            Ok(result) => {
                let applied = match (apply, self.checked_folder()) {
                    (false, _) => Err(crate::git::NotApplied::Failed(
                        "a session log could not be written".into(),
                    )),
                    // Plan mode (X.7) switched on while the child worked:
                    // the node writes nothing into the folder either; the
                    // changes stay in the store and are shown.
                    (true, Ok(_)) if self.current_mode() == ApprovalMode::Plan => {
                        Err(crate::git::NotApplied::Failed(PLAN_NOT_APPLIED.into()))
                    }
                    (true, Ok(_)) => iso.sub.apply(&iso.base, &result, &id).await,
                    (true, Err(e)) => Err(crate::git::NotApplied::Failed(e)),
                };
                match applied {
                    Ok(a) => (applied_note(&a), None, true),
                    Err(e) => {
                        let (diff, cut) = self.unapplied(&iso, &result).await;
                        let follows = match (&diff, cut) {
                            (Some(_), Some(c)) => format!(
                                " Its changes against the folder it started from follow, every \
                                 line behind \"| \" ({c})."
                            ),
                            (Some(_), None) => " Its changes against the folder it started from \
                                                follow, every line behind \"| \"."
                                .to_string(),
                            (None, Some(c)) => format!(" Its changes cannot be shown: {c}."),
                            (None, None) => String::new(),
                        };
                        // A failed log keeps its words (nothing would record a
                        // merge); anything else says why it did not apply.
                        let why = if !apply {
                            "its changes were not applied, because a session log could not be \
                             written"
                                .to_string()
                        } else if e.nothing_written() {
                            format!("its changes were NOT applied: {}", e.text())
                        } else {
                            format!("{}: {}", harness_core::subagent::APPLIED_IN_PART, e.text())
                        };
                        let written = if e.nothing_written() {
                            "Nothing of this sub-agent's work was written"
                        } else {
                            "Check those files"
                        };
                        (
                            format!(
                                "{}. {written}; {kept} as {result}.{follows}",
                                why.trim_end_matches('.')
                            ),
                            diff,
                            false,
                        )
                    }
                }
            }
        };
        // What the result snapshot leaves out, said rather than dropped.
        let ignored = iso.sub.ignored(&iso.child).await;
        if !ignored.is_empty() {
            note.push_str(&format!(
                " {} path(s) the sub-agent made that the repository ignores were not brought \
                 back: {}.",
                ignored.shown.len() + ignored.more,
                ignored.listed()
            ));
        }
        crate::git::remove_folder(&iso.child).await;
        iso.sub.prune(self.node.cfg.policy.checkpoints_keep).await;
        (note, unapplied, landed)
    }

    /// A child's changes from its base, as lines for the parent, and what
    /// was left out of them (or why they cannot be read).
    async fn unapplied(&self, iso: &Isolation, result: &str) -> (Option<String>, Option<String>) {
        let secrets = &self.node.cfg.secrets;
        match iso
            .sub
            .changes(&iso.base, result, &|s: &str| secrets.scrub(s))
            .await
        {
            Ok(d) => {
                let (text, cut) = crate::git::unapplied_text(&d);
                ((!text.is_empty()).then_some(text), cut)
            }
            Err(e) => (None, Some(e)),
        }
    }

    /// Record an approval's answer; `false` if the log refused it (the
    /// approval then stays in `unresolved` for the turn's end to deny).
    async fn resolve(
        &self,
        approval_id: String,
        decision: Decision,
        by: ResolvedBy,
        note: Option<String>,
    ) -> bool {
        let written = self
            .emit(EventKind::ApprovalResolved(ApprovalResolved {
                approval_id: approval_id.clone(),
                decision,
                by,
                note,
            }))
            .await
            .is_ok();
        if written {
            self.unresolved
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|a| *a != approval_id);
        }
        written
    }

    pub(crate) fn id(&self) -> SessionId {
        self.id
    }

    /// The chat's folder as recorded.
    pub(crate) fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// The agent of the running turn, if one runs under an agent.
    fn running_agent(&self) -> Option<String> {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|t| t.agent_id.clone())
    }

    pub(crate) fn is_running(&self) -> bool {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    pub(crate) fn cancel(&self) {
        // The rest of a message's called agents (X.37) is not asked.
        self.stop_peers();
        if let Some(t) = self.turn.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            t.cancel.cancel();
        }
        self.cancel_note();
        if let Some(c) = self
            .compacting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            c.cancel();
        }
    }

    /// Mark the running turn as the stop's to end, unless it was cancelled
    /// already (by its person, or its parent): `NodeRuntime::stop` marks
    /// every running turn first and then cancels them.
    pub(crate) fn mark_stopped(&self) {
        if let Some(t) = self.turn.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
            && !t.cancel.is_cancelled()
        {
            t.by_stop.store(true, Ordering::SeqCst);
        }
    }

    /// The last turn's end could not be written (it is still open in the
    /// log; the next prompt or start closes it).
    pub(crate) fn is_unclosed(&self) -> bool {
        self.unclosed.load(Ordering::SeqCst)
    }

    /// A person's answer. `note` goes only with a denial: with an allow it
    /// is dropped here, whatever the master sent; it is cut to
    /// [`harness_core::policy::MAX_DENY_NOTE`] characters.
    pub(crate) fn respond(
        &self,
        approval_id: &str,
        decision: Decision,
        note: Option<String>,
    ) -> Result<(), String> {
        let note = note
            .filter(|_| !decision.allows())
            .map(|n| {
                n.chars()
                    .take(harness_core::policy::MAX_DENY_NOTE)
                    .collect::<String>()
            })
            .filter(|n| !n.trim().is_empty());
        let tx = self
            .approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(approval_id)
            .ok_or_else(|| "no such pending approval".to_string())?;
        tx.send((decision, note))
            .map_err(|_| "approval is no longer pending".to_string())
    }

    /// The person's removal of a call they allowed for this chat: recorded
    /// as `chat_allow_removed` before it stops covering the call.
    pub(crate) async fn revoke_allow(&self, grant_id: &str) -> Result<(), String> {
        // It stops covering the call first (review L4); put back if the log
        // does not take its removal.
        let Some(key) = self
            .grants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(grant_id)
        else {
            return Err(format!("no call allowed for this chat as {grant_id}"));
        };
        if let Err(e) = self
            .append(EventKind::ChatAllowRemoved(ChatAllowRemoved {
                grant_id: grant_id.into(),
            }))
            .await
        {
            self.grants
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(grant_id.into(), key);
            return Err(e.to_string());
        }
        Ok(())
    }

    /// How this session's commands run now: what its own log says (one
    /// rule everywhere: the master's checks read the same row). A sub-agent's
    /// chat has no switch of its own, so its mode is the one it was created
    /// with: its chat's at that time. `None` runs nothing.
    pub(crate) fn exec_mode_now(&self) -> Option<harness_core::policy::ExecMode> {
        *self.exec_mode.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A sub-agent's chat takes no turn while the chat above it is being
    /// switched, nor when that chat's mode is not known. And:
    /// a sub-agent's chat takes no turn once its recorded mode differs from
    /// the mode of the chat at the top of its parents (N.10b): the chat was
    /// switched after the sub-agent was made, and what the sub-agent was
    /// started to do was decided for the other shell. The sentence is for
    /// whoever asked (the model's `delegate` follow-up, or the person), and
    /// says what to do.
    fn sub_agent_mode_mismatch(&self) -> Result<(), String> {
        let Some(mut up) = self.parent_session else {
            return Ok(());
        };
        let mine = self.exec_mode_now();
        let mut root = None;
        for _ in 0..8 {
            match self.node.session(up) {
                Some(p) if p.id != self.id => {
                    if p.exec_switching.load(Ordering::SeqCst) {
                        return Err(exec_op::EXEC_SWITCHING.into());
                    }
                    root = Some(p.clone());
                    match p.parent_session {
                        Some(next) => up = next,
                        None => break,
                    }
                }
                _ => break,
            }
        }
        let Some(root) = root else { return Ok(()) };
        // A chat whose mode is not known runs nothing, and neither do
        // its sub-agents.
        let Some(r) = root.exec_mode_now() else {
            return Err(exec_op::EXEC_UNREADABLE.into());
        };
        let Some(m) = mine else { return Ok(()) };
        if m == r {
            return Ok(());
        }
        let word = |x: harness_core::policy::ExecMode| match x {
            harness_core::policy::ExecMode::Sandbox => "the sandbox",
            harness_core::policy::ExecMode::Host => "the host shell",
        };
        Err(format!(
            "this sub-agent ran in {} and this chat's commands now run in {}: start a new \
             sub-agent (a sub-agent's mode does not change)",
            word(m),
            word(r)
        ))
    }

    fn current_cancel(&self) -> Option<CancellationToken> {
        self.turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|t| t.cancel.clone())
    }
}

#[async_trait::async_trait]
impl TurnHost for Session {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        Session::emit(self, kind).await
    }

    fn step_limit(&self) -> Option<usize> {
        Some(self.current_steps())
    }

    async fn delegate(
        &self,
        agent: AgentSpec,
        task: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
        folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.run_child(agent, task, refs, observed, cancel, started, folder)
            .await
    }

    fn background_agents(&self) -> Result<(), String> {
        self.may_start_background()
    }

    async fn delegate_background(
        &self,
        agent: AgentSpec,
        task: String,
        follow_up: Option<SessionId>,
        call_id: String,
        observed: Arc<Vec<String>>,
        max_tokens: u64,
        folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<harness_core::backend::BackgroundDelegated, String> {
        self.start_background(
            agent, task, follow_up, call_id, observed, max_tokens, folder,
        )
        .await
    }

    async fn keep_image(&self, image: harness_core::backend::KeptImage) -> Result<(), String> {
        if self.incognito {
            return Err("images can't go into an incognito chat".into());
        }
        let sid = self.id.to_string();
        self.node
            .cfg
            .store
            .call(move |s| {
                let (held, has) = s.session_images_held(&sid, &image.sha256)?;
                if !has && held + image.bytes.len() as u64 > crate::images::CHAT_IMAGE_BYTES {
                    return Ok(Err(format!(
                        "the chat keeps at most {} MiB of images",
                        crate::images::CHAT_IMAGE_BYTES >> 20
                    )));
                }
                s.session_images_put(
                    &sid,
                    &[harness_store::session_images::SessionImage {
                        sha256: image.sha256,
                        mime: image.mime,
                        name: image.name,
                        bytes: image.bytes,
                    }],
                )?;
                Ok::<_, harness_store::StoreError>(Ok(()))
            })
            .await
            .map_err(|e| {
                tracing::warn!(session = %self.id, error = %e, "could not keep an image");
                "it could not be stored".to_string()
            })?
    }

    async fn continue_child(
        &self,
        child: SessionId,
        agent: AgentSpec,
        task: String,
        call_id: String,
        refs: SharedRefs,
        observed: Arc<Vec<String>>,
        cancel: CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.run_continue(child, agent, task, call_id, refs, observed, cancel, started)
            .await
    }

    async fn images(&self, sha256: &[String]) -> Vec<harness_core::backend::ImageData> {
        use base64::Engine as _;
        let (sid, wanted) = (self.id.to_string(), sha256.to_vec());
        match self
            .node
            .cfg
            .store
            .call(move |s| s.session_images(&sid, &wanted))
            .await
        {
            Ok(found) => found
                .into_iter()
                .map(|i| harness_core::backend::ImageData {
                    data: base64::engine::general_purpose::STANDARD
                        .encode(&i.bytes)
                        .into(),
                    sha256: i.sha256,
                    mime: i.mime,
                })
                .collect(),
            Err(e) => {
                // The model is then told the images' names instead.
                tracing::warn!(session = %self.id, error = %e, "could not read the chat's images");
                Vec::new()
            }
        }
    }

    fn artifacts(&self) -> Option<Arc<dyn harness_core::artifacts::ArtifactSink>> {
        let stages = self.node.stages.clone()?;
        let cwd = self.publish_cwd()?;
        let count = self
            .turn
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()?
            .publishes
            .clone();
        Some(Arc::new(crate::artifacts::Sink {
            node: self.node.clone(),
            stages,
            session: self.id,
            cwd,
            pin: self.pin(),
            count,
        }))
    }

    fn recall(&self) -> Option<Arc<dyn harness_core::recall::RecallSource>> {
        Some(Arc::new(crate::recall::LinkRecall {
            node: self.node.clone(),
            session: self.id,
        }))
    }

    fn history(&self) -> Option<Arc<dyn harness_core::history::HistorySource>> {
        Some(Arc::new(crate::history::OwnHistory {
            node: self.node.clone(),
            session: self.id,
        }))
    }

    fn attached(&self) -> Option<Arc<dyn harness_core::attached::AttachedSource>> {
        Some(Arc::new(crate::attached::LinkAttached {
            node: self.node.clone(),
            session: self.id,
        }))
    }

    /// Only a chat without a folder: where there is one the tools write the
    /// file into it themselves, as `write` does (PLAN.md T97).
    fn docs(&self) -> Option<Arc<dyn harness_core::docs::DocSource>> {
        (self.profile == Profile::Chat).then(|| {
            Arc::new(crate::docs::LinkDocs {
                node: self.node.clone(),
                session: self.id,
            }) as Arc<dyn harness_core::docs::DocSource>
        })
    }

    /// Only a chat with a folder: where there is none the master edits the
    /// chat's file (X.57, T98).
    fn doc_helper(&self) -> Option<Arc<dyn harness_core::docs::EditHelper>> {
        (self.profile != Profile::Chat)
            .then(crate::docs::shared_helper)
            .flatten()
    }

    fn board(&self) -> Option<Arc<dyn harness_core::board::BoardSource>> {
        Some(Arc::new(crate::board::LinkBoard {
            node: self.node.clone(),
            session: self.id,
        }))
    }

    fn stored_skills(&self) -> Option<Arc<dyn harness_core::skills::StoredSkillSource>> {
        Some(Arc::new(crate::stored_skills::LinkStoredSkills {
            node: self.node.clone(),
            session: self.id,
        }))
    }

    /// Only where node.toml takes connectors, and only for a turn that gets
    /// its chat's MCP tools ([`Self::chat_mcp`]'s rule: a chat's own turn,
    /// or a run the node started for a parent's `delegate` call; never a
    /// person's message in a sub-agent's chat).
    fn connectors(&self) -> Option<Arc<dyn harness_core::connectors::ConnectorSource>> {
        if !self.node.cfg.policy.mcp.master_connectors {
            return None;
        }
        self.chat_table(|_| ())?;
        Some(Arc::new(crate::connectors::LinkConnectors {
            node: self.node.clone(),
            session: self.id,
        }))
    }

    fn denial_note(&self, call_id: &str) -> Option<String> {
        self.deny_notes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(call_id)
    }

    fn confined(&self, call_id: &str) -> bool {
        self.confined
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(call_id)
    }

    fn tool_denied(&self, tool: &str) -> bool {
        self.node.cfg.policy.denies(tool)
    }

    fn runs_unasked(&self, tool: &str, risk: ToolRisk) -> bool {
        let mode = self.current_mode();
        !self.node.cfg.policy.denies(tool) && !mode.needs_approval(risk) && !mode.refuses(tool)
    }

    fn approval_mode(&self) -> Option<ApprovalMode> {
        (self.profile == Profile::Agentic).then(|| self.current_mode())
    }

    fn background(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.bg
            .clone()
            .map(|b| b as Arc<dyn std::any::Any + Send + Sync>)
    }

    fn mcp(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.chat_mcp_for_turn()
            .map(|m| m as Arc<dyn std::any::Any + Send + Sync>)
    }

    async fn background_notes(
        &self,
    ) -> Result<Vec<BackgroundEnded>, harness_core::backend::LogError> {
        let Some(bg) = &self.bg else {
            return Ok(Vec::new());
        };
        let queued = std::mem::take(&mut *self.bg_queue.lock().unwrap_or_else(|p| p.into_inner()));
        // The end of a command a rewound turn started is not the model's
        // to read (X.46): recorded as not delivered, outside the history.
        let queued = if queued.is_empty() {
            queued
        } else {
            // Only the kinds the rule reads: the whole log holds every
            // command's output, and reading it here for each turn that has
            // an end to hand over would cost a second copy of it.
            let log = match self.read_rewind_kinds().await {
                Ok(l) => l,
                Err(e) => {
                    self.bg_queue
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .extend(queued);
                    return Err(harness_core::backend::LogError(e));
                }
            };
            let (kept, gone) = rewind_op::split_rewound_ends(&log, queued);
            self.record_background(gone).await;
            kept
        };
        let mut out = Vec::new();
        let mut rest = queued.into_iter();
        while let Some(e) = rest.next() {
            // What is appended is scrubbed; so is what goes to the model
            // (T8D-2). A background sub-agent's end is merged and built
            // here, at the step boundary (X.24).
            let mut ev = self.end_event(&e, true).await;
            ev.tail = self.node.cfg.secrets.scrub(&ev.tail);
            if let Err(err) = Session::emit(self, EventKind::BackgroundEnded(ev.clone())).await {
                self.keep_child_end(ev);
                // The turn ends; these are recorded after it.
                let mut q = self.bg_queue.lock().unwrap_or_else(|p| p.into_inner());
                q.push(e);
                q.extend(rest);
                return Err(err);
            }
            bg.recorded();
            out.push(ev);
        }
        Ok(out)
    }

    async fn queued_messages(&self) -> Result<Vec<UserMessage>, LogError> {
        let _queue = self.queue.lock().await;
        // A callback turn reads none (ST-2): the person's turn after it does.
        let turn_id = match self.turn.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            Some(t) if !t.callback => Some(t.turn_id.clone()),
            _ => None,
        };
        let taken = std::mem::take(&mut *self.pending.lock().unwrap_or_else(|p| p.into_inner()));
        let (false, Some(turn_id)) = (taken.is_empty(), turn_id) else {
            *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = taken;
            return Ok(Vec::new());
        };
        // Read now, by this turn: no later turn is sent their context.
        self.queued_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let seqs = taken.iter().map(|(s, _)| *s).collect();
        if let Err(e) = Session::emit(
            self,
            EventKind::QueuedDelivered(QueuedDelivered { turn_id, seqs }),
        )
        .await
        {
            // The turn ends; the next one reads them.
            let mut p = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            let later = std::mem::take(&mut *p);
            *p = taken;
            p.extend(later);
            return Err(e);
        }
        Ok(taken.into_iter().map(|(_, m)| m).collect())
    }

    fn folder_pin(&self) -> Option<harness_core::folder::FolderPin> {
        self.pin()
    }

    fn owned_folder(&self) -> Option<harness_core::folder::OwnedFolder> {
        self.owned()
    }

    fn exec_mode(&self) -> Option<harness_core::policy::ExecMode> {
        self.exec_mode_now()
    }

    /// The identity of the chat at the top of this session's parents (a
    /// sub-agent follows its root chat's owner, never its own turn's
    /// sender); the node's fallback where the chat has none.
    fn commit_identity(&self) -> Option<harness_core::identity::Identity> {
        let mut top = None;
        let mut up = self.parent_session;
        for _ in 0..8 {
            let Some(s) = up.and_then(|p| self.node.session(p)) else {
                break;
            };
            up = s.parent_session;
            top = Some(s);
        }
        let held = |s: &Session| s.identity.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let found = match &top {
            Some(root) => held(root),
            None => held(self),
        };
        Some(found.unwrap_or_else(harness_core::identity::Identity::fallback))
    }

    async fn approve(&self, call_id: &str, tool: &str, input: &Value, risk: ToolRisk) -> Decision {
        self.approve_from(call_id, tool, input, risk, None, None)
            .await
    }

    async fn approve_from(
        &self,
        call_id: &str,
        tool: &str,
        input: &Value,
        risk: ToolRisk,
        origin: Option<&str>,
        preview: Option<&str>,
    ) -> Decision {
        let policy = &self.node.cfg.policy;
        if policy.denies(tool) {
            let approval_id = new_id();
            // Record the refusal so the user sees why. Denied either way.
            let requested = self
                .emit(EventKind::ApprovalRequested(ApprovalRequested {
                    approval_id: approval_id.clone(),
                    call_id: call_id.into(),
                    tool: tool.into(),
                    input: input.clone(),
                    reason: format!("`{tool}` is denied by this node's policy"),
                    turn_allow: false,
                    chat_allow: false,
                    preview: None,
                }))
                .await;
            if requested.is_ok() {
                self.unresolved
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(approval_id.clone());
                self.resolve(approval_id, Decision::Deny, ResolvedBy::Policy, None)
                    .await;
            }
            return Decision::Deny;
        }
        let mode = self.current_mode();
        // Plan mode (X.7): refused, never asked, and no card can lift it.
        // The loop refuses these before they get here, with the plain
        // sentence (`plan_refusal`); this is the node's own floor for a
        // call that reached it anyway (the mode changed in between).
        if mode.refuses(tool) {
            return Decision::Deny;
        }
        if !mode.needs_approval(risk) {
            return Decision::Allow;
        }
        // Accept edits (T50): a file change inside the folder runs unasked;
        // it is checkpointed and undone with the turn. The path is judged
        // here, by the node, from the call's own input.
        if mode.accepts_edits() && risk == ToolRisk::Mutating && self.edit_inside(tool, input) {
            self.confined
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(call_id.to_string());
            return Decision::Allow;
        }
        // "For this turn" is offered on every request a person answers: it
        // covers only this exact call, the one on the card (the loop keys it
        // with `harness_tools::call_key`, PLAN.md T4). The answer reaches
        // only this call: the approval id is bound to this one waiting
        // request, and the loop records the key of the call it is running.
        let background_start = is_background_start(tool, input);
        // "For this chat": the same exact call, allowed before in this chat
        // (never a background start: one click is one command). Only in
        // `ask` and `accept_edits`: `ask_all` means every call (T51), and
        // `auto` never gets here.
        let grants_apply = mode != ApprovalMode::AskAll && self.node.cfg.policy.chat_grants;
        // The origin is part of the key (review of PR 157, M3): a chat grant
        // for a connector's tool never covers a node server's of that name.
        let key = harness_tools::call_key(
            &self.cwd,
            &harness_tools::approval::keyed(tool, origin),
            input,
        );
        if grants_apply
            && !background_start
            && self
                .grants
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .values()
                .any(|k| *k == key)
        {
            return Decision::Allow;
        }
        // Not for a sub-agent's chat, nor a routine's (it keeps its
        // routine's setting, review L1).
        let chat_allow = grants_apply
            && !background_start
            && self.parent_session.is_none()
            && !self.no_callbacks;
        let approval_id = new_id();
        let mut reason = approval_reason(
            tool,
            risk,
            input,
            &self.node.cfg.policy,
            self.exec_mode_now(),
        );
        // A custom connector's tool (X.35 step 2): its own reason, never the
        // node server's (review of PR 157, M4).
        let connector = origin
            .and_then(|o| o.strip_prefix("connector:"))
            .map(str::to_string);
        if let Some(name) = connector {
            let raw = self
                .connector_tools
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .find(|e| e.name == tool && e.connector == name)
                .map(|e| e.tool.clone());
            reason = connector_reason(tool, &name, raw.as_deref());
        }
        // An MCP tool's card names the server and its own name of the tool
        // (review N2): a cleaned or hashed public name says little.
        else if let Some(m) = self
            .chat_mcp()
            .filter(|_| tool.starts_with(harness_tools::mcp::PREFIX))
            && let Some((server, raw)) = m.origin(tool).await
        {
            let from = if m.is_project(&server) {
                "; from the repository's .mcp.json, as you approved it"
            } else {
                "; from node.toml"
            };
            reason = format!(
                "{reason} (server `{server}`, its tool `{}`{from})",
                harness_core::event::shown_name(&raw)
            );
        }
        let (tx, rx) = oneshot::channel();
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(approval_id.clone(), tx);
        let requested = self
            .emit(EventKind::ApprovalRequested(ApprovalRequested {
                approval_id: approval_id.clone(),
                call_id: call_id.into(),
                tool: tool.into(),
                input: input.clone(),
                reason,
                // "For this turn" would cover every identical start of the
                // turn (N-2): a background start is asked for each time.
                turn_allow: !background_start,
                chat_allow,
                // Scrubbed with the rest of the event on its way into the log
                // (`append`), like tool output; never sent to the model.
                preview: preview.map(str::to_string),
            }))
            .await;
        if requested.is_ok() {
            self.unresolved
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(approval_id.clone());
        } else {
            // Nobody can see a request the log does not hold; the turn is
            // already stopping.
            self.approvals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&approval_id);
            return Decision::Deny;
        }
        let cancel = self.current_cancel().unwrap_or_default();
        // The cancel first (review N-8): an answer that arrives with it,
        // after a stop or a cancel began, allows nothing.
        let (decision, by, note) = tokio::select! {
            biased;
            _ = cancel.cancelled() => (Decision::Deny, ResolvedBy::Cancelled, None),
            r = rx => match r {
                Ok((d, note)) => (d, ResolvedBy::User, note),
                Err(_) => (Decision::Deny, ResolvedBy::Cancelled, None),
            },
        };
        // "For this chat" where it was not offered allows this one call,
        // and is recorded as that (review L2).
        let decision = if decision == Decision::AllowChat && !chat_allow {
            Decision::Allow
        } else {
            decision
        };
        // Plan mode (X.7) began while the card waited: the node's floor is
        // read again after the answer. A call it refuses is denied whatever
        // was answered, and so is every card plan mode's start denied
        // (`deny_waiting_for_plan`): recorded as the node's policy, with
        // the reason, never as the person's answer.
        let plan_denied = self
            .plan_denied
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&approval_id);
        let (decision, by, note) =
            if plan_denied || (decision.allows() && self.current_mode().refuses(tool)) {
                (
                    Decision::Deny,
                    ResolvedBy::Policy,
                    Some(PLAN_CARD_DENIED.to_string()),
                )
            } else {
                (decision, by, note)
            };
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&approval_id);
        // An approval the log does not hold allows nothing.
        if !self
            .resolve(approval_id.clone(), decision, by, note.clone())
            .await
        {
            return Decision::Deny;
        }
        if let Some(n) = note.filter(|_| by == ResolvedBy::User) {
            self.deny_notes
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(call_id.to_string(), n);
        }
        if decision == Decision::AllowChat {
            let grant_id = new_id();
            // The log first: a grant it does not hold covers nothing more
            // than this call.
            if self
                .emit(EventKind::ChatAllowAdded(ChatAllowAdded {
                    grant_id: grant_id.clone(),
                    approval_id,
                    tool: tool.into(),
                    input: input.clone(),
                    key: key.hex(),
                }))
                .await
                .is_ok()
            {
                self.grants
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(grant_id, key);
            }
            return Decision::Allow;
        }
        // Not offered for a background start, and not taken either: one
        // click is one command.
        if background_start && decision == Decision::AllowSession {
            return Decision::Allow;
        }
        decision
    }
}

/// A `bash` call that asks to keep running.
fn is_background_start(tool: &str, input: &Value) -> bool {
    tool == "bash" && input.get("background").and_then(Value::as_bool) == Some(true)
}

/// Refusal while a restore of an overlapping folder runs.
const UNDO_RUNNING: &str = "an undo is running in this folder; try again in a moment";

/// A folder registered as being restored; unregistered when dropped.
struct RestoreClaim {
    node: Arc<Inner>,
    root: PathBuf,
}

impl Drop for RestoreClaim {
    fn drop(&mut self) {
        let mut reg = self
            .node
            .restoring
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(i) = reg.iter().position(|r| *r == self.root) {
            reg.swap_remove(i);
        }
    }
}

/// Where `p` really is (as given when it cannot be resolved).
fn real_path(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The same folder, or one inside the other.
fn overlaps(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// The note at the end of a turn that changed more files than
/// `RECOMMENDED_MAX_FILES_PER_TURN` (T.9): a count and what an undo will do,
/// nothing else. `None` at or under the limit.
fn large_turn_note(files: usize) -> Option<String> {
    let max = harness_core::policy::RECOMMENDED_MAX_FILES_PER_TURN;
    (files > max).then(|| {
        format!(
            "This turn changed {files} files, more than the recommended {max}. \
             Undoing it will put back {files} files."
        )
    })
}

/// The same for an undo that put back more files than the limit.
fn large_undo_note(files: usize) -> Option<String> {
    let max = harness_core::policy::RECOMMENDED_MAX_FILES_PER_TURN;
    (files > max)
        .then(|| format!("This undo put back {files} files, more than the recommended {max}."))
}

/// The chat's note after an undo, only when repositories inside the folder
/// appeared or went away during the undone turn, or a `.git` sits in a
/// folder the undo wrote into. It starts with what was restored, so it does
/// not read as a failure. `None`: nothing to say (stable nested
/// repositories are in the checkpoint's `excluded` alone).
///
/// Also when a forced undo deleted files no checkpoint holds, and after a
/// restore of an interrupted restore (`safety`: the checkpoint that saved
/// the folder first), which overwrote its paths without checking for
/// later edits.
fn restore_notice(
    files: usize,
    plan: &crate::git::RestorePlan,
    safety: Option<&str>,
) -> Option<String> {
    if plan.changed_repos.is_empty()
        && plan.git_inside.is_empty()
        && plan.unsaved.is_empty()
        && safety.is_none()
    {
        return None;
    }
    let mut s = format!(
        "Undo restored {files} file{}.",
        if files == 1 { "" } else { "s" }
    );
    if let Some(id) = safety {
        s.push_str(&format!(
            " They were put back as saved before the failed undo, over any later edits; \
             the folder as it was just before is saved as checkpoint {id}."
        ));
    }
    if !plan.unsaved.is_empty() {
        s.push_str(&format!(
            " These files were deleted or written over and were not saved (they are in no \
             checkpoint: git ignores them, say): {}.",
            plan.unsaved.listed()
        ));
    }
    if !plan.git_inside.is_empty() {
        s.push_str(&format!(
            " {} now hold a `.git` (a repository made there, or a stray `.git`) that no \
             checkpoint covers: the files the checkpoint holds there were restored and the \
             `.git` was left as it is.",
            plan.git_inside.listed()
        ));
    }
    if !plan.changed_repos.is_empty() {
        s.push_str(&format!(
            " Git repositories inside the folder appeared or went away during the turn and \
             were left alone ({}): undo never covers a repository inside your folder; use \
             git in it to undo changes there.",
            plan.changed_repos.listed()
        ));
    }
    Some(s)
}

/// What an approval card says the tool would do. Publishing changes nothing
/// on this node, it sends a copy away, so it says that instead.
fn approval_reason(
    tool: &str,
    risk: ToolRisk,
    input: &Value,
    policy: &crate::NodePolicy,
    exec_mode: Option<harness_core::policy::ExecMode>,
) -> String {
    let reason = tool_reason(tool, risk, input, policy, exec_mode);
    // In a `host` chat the card is the only check (review N-e): it says so.
    // `publish` and `board_edit` run nothing here: the host shell is not
    // part of what they do.
    if exec_mode == Some(harness_core::policy::ExecMode::Host)
        && !matches!(tool, "publish" | "board_edit")
    {
        return format!(
            "{reason}; in the host shell: as this node's own user, without the sandbox"
        );
    }
    reason
}

/// The card of a custom connector's tool (X.35 step 2): what it is, where
/// the call goes from and with whose account.
fn connector_reason(tool: &str, connector: &str, raw: Option<&str>) -> String {
    let its = raw
        .map(|r| format!(" (its tool `{}`)", harness_core::event::shown_name(r)))
        .unwrap_or_default();
    format!(
        "`{tool}` is a tool of the connector `{connector}`{its}, an online service this \
         workspace's admin added; the call is sent from the server with the chat owner's \
         connected account, and the arguments shown are what is sent"
    )
}

/// What the card of `board_edit` says: it changes this project's task
/// board and nothing else (trial 2026-11-19, item 15: the general sentence
/// said it could change files or run programs).
const BOARD_EDIT_REASON: &str = "`board_edit` adds a card, moves one or adds a note to one on \
    this project's task board; it touches no file and runs nothing";

fn tool_reason(
    tool: &str,
    risk: ToolRisk,
    input: &Value,
    policy: &crate::NodePolicy,
    exec_mode: Option<harness_core::policy::ExecMode>,
) -> String {
    // A background start says what it is, in words (§4.1): the field alone
    // is easy to miss.
    if is_background_start(tool, input) {
        let ceiling = std::time::Duration::from_secs(policy.limits.bash_timeout_max_s);
        let t = harness_tools::background::timeout_for(
            input.get("timeout_secs").and_then(Value::as_u64),
            harness_tools::background::DEFAULT_TIMEOUT,
            ceiling,
        );
        let mut s = format!(
            "`bash` starts a program that KEEPS RUNNING in the background after this step, \
             with nobody watching, for up to {} (its timeout), unless it ends or is stopped first",
            harness_core::text::duration_words(t.as_millis() as u64)
        );
        // A sandbox chat's command has its own network namespace when the
        // network is off. A host chat's has none: only Landlock's TCP rule
        // (`network = "off"`, where the kernel enforces it) stops it from
        // listening, and a node that cannot says so in its gaps.
        let network_on = policy.sandbox.network == harness_tools::sandbox::Network::On;
        if exec_mode == Some(harness_core::policy::ExecMode::Host) {
            if network_on || policy.exec.host_net_open() {
                s.push_str(
                    "; nothing stops it from accepting connections on this machine's network",
                );
            }
        } else if network_on {
            s.push_str("; the network is on, so it can accept connections");
        }
        return s;
    }
    if tool.starts_with(harness_tools::mcp::PREFIX) {
        return format!(
            "`{tool}` is a tool of an MCP server this node runs: a program the \
             harness did not write, in this chat's folder; what it does is up to that server"
        );
    }
    match (tool, risk) {
        ("publish", _) => "`publish` uploads a copy of a file from this folder to Artifacts".into(),
        ("board_edit", _) => BOARD_EDIT_REASON.into(),
        (_, ToolRisk::Mutating) => format!("`{tool}` can change files or run programs"),
        // `ChatOnly` is never asked about (`needs_approval`).
        (_, ToolRisk::ReadOnly | ToolRisk::ChatOnly) => {
            format!("approval mode requires asking for `{tool}`")
        }
    }
}

/// The merge note for a child's result that was applied.
fn applied_note(a: &crate::git::Applied) -> String {
    let mut s = applied_count(a);
    if !a.nested_skipped.is_empty() {
        s.push_str(&format!(
            "; {} change(s) to nested repositories were not applied: {}",
            a.nested_skipped.shown.len() + a.nested_skipped.more,
            a.nested_skipped.listed()
        ));
    }
    s
}

fn applied_count(a: &crate::git::Applied) -> String {
    match (a.written, a.same) {
        (0, 0) => "no file changes".to_string(),
        (0, _) => "its changes were already in the folder".to_string(),
        (n, _) if a.merged.is_empty() => format!("{n} changed file(s) applied to the folder"),
        (n, _) => format!(
            "{n} changed file(s) applied to the folder, {} of them merged with changes the \
             folder had since the sub-agent started: {}",
            a.merged.shown.len() + a.merged.more,
            a.merged.listed()
        ),
    }
}

/// A turn's changes as a diff (X.12); its own file.
#[path = "session_diff.rs"]
mod diff;

/// A person's Retry of the failed last turn (X.21); its own file.
#[path = "session_peers.rs"]
mod peers;
#[path = "session_retry.rs"]
mod retry;

/// A person's rewind of the conversation to an earlier message (X.46); its
/// own file.
#[path = "session_rewind.rs"]
mod rewind_op;

/// The owner's switch of a chat's execution mode (N.10b); its own file.
#[path = "session_exec.rs"]
mod exec_op;

/// The owner's bookmark on a message (docs/pins.md); its own file.
#[path = "session_pin.rs"]
mod pin_op;

/// A person's fork of a chat at a finished turn (C.2); its own file.
#[path = "session_fork.rs"]
mod fork_op;

/// The chat's agent switch: refused while it works, recorded at the next
/// turn; its own file.
#[path = "session_agent.rs"]
mod agent;

/// A `/compact` focus cut to `COMPACT_FOCUS_MAX_CHARS` characters, saying
/// where (X.20).
fn cut_focus(f: &str) -> String {
    let max = harness_core::event::COMPACT_FOCUS_MAX_CHARS;
    if f.chars().count() <= max {
        return f.to_string();
    }
    let mut out: String = f.chars().take(max).collect();
    out.push_str(&format!(" [cut at {max} characters]"));
    out
}

/// What a sub-agent (new or continued, T68) inherits of its parent turn's
/// standing parts: all of them, but the person's Personalisation only when
/// the child's own switch is on (PLAN.md T74). Called where every child's
/// turn is built (`start_child`, `start_continue`), the background ones too.
fn inherited(
    standing: Option<&Vec<harness_core::backend::StandingPart>>,
    child: &AgentSpec,
) -> Option<Vec<harness_core::backend::StandingPart>> {
    standing.map(|parts| {
        parts
            .iter()
            .filter(|p| {
                !(child.no_personalisation
                    && p.part == harness_core::backend::StandingKey::Personal)
            })
            .cloned()
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_cards_say_what_the_tool_does() {
        let p = crate::NodePolicy::default();
        let none = Value::Null;
        let publish = approval_reason("publish", ToolRisk::Mutating, &none, &p, None);
        assert!(
            publish.contains("Artifacts") && !publish.contains("run programs"),
            "{publish}"
        );
        assert_eq!(
            approval_reason("bash", ToolRisk::Mutating, &none, &p, None),
            "`bash` can change files or run programs"
        );
    }

    /// Trial item 15: `board_edit` changes the task board, so its card says
    /// that, in the host shell too; `board` and every other tool keep theirs.
    #[test]
    fn the_board_edit_card_says_what_it_does() {
        use harness_core::policy::ExecMode;
        let p = crate::NodePolicy::default();
        let none = Value::Null;
        let want = "`board_edit` adds a card, moves one or adds a note to one on this \
                    project's task board; it touches no file and runs nothing";
        for mode in [None, Some(ExecMode::Sandbox), Some(ExecMode::Host)] {
            assert_eq!(
                approval_reason("board_edit", ToolRisk::Mutating, &none, &p, mode),
                want
            );
        }
        // Unchanged: the read-only tool, and others of the same risk.
        assert_eq!(
            approval_reason("board", ToolRisk::ReadOnly, &none, &p, None),
            "approval mode requires asking for `board`"
        );
        for t in ["write", "edit", "remember"] {
            assert_eq!(
                approval_reason(t, ToolRisk::Mutating, &none, &p, None),
                format!("`{t}` can change files or run programs")
            );
        }
        assert!(
            approval_reason("write", ToolRisk::Mutating, &none, &p, Some(ExecMode::Host))
                .contains("in the host shell")
        );
    }

    /// Review N-e: a `host` chat's card says the command runs in the host
    /// shell; a `sandbox` chat's says what it always said.
    #[test]
    fn a_host_chats_card_says_the_host_shell() {
        use harness_core::policy::ExecMode;
        let p = crate::NodePolicy::default();
        let none = Value::Null;
        let host = approval_reason("bash", ToolRisk::Mutating, &none, &p, Some(ExecMode::Host));
        assert!(
            host.starts_with("`bash` can change files or run programs")
                && host.contains("in the host shell")
                && host.contains("without the sandbox"),
            "{host}"
        );
        assert_eq!(
            approval_reason(
                "bash",
                ToolRisk::Mutating,
                &none,
                &p,
                Some(ExecMode::Sandbox)
            ),
            "`bash` can change files or run programs"
        );
    }

    /// §7 test 10: the card of a background start says it keeps running,
    /// and until when, in its reason, not only in the input field.
    #[test]
    fn a_background_start_says_it_keeps_running() {
        let p = crate::NodePolicy::default();
        let input =
            serde_json::json!({"command": "npm run dev", "background": true, "timeout_secs": 1800});
        let r = approval_reason("bash", ToolRisk::Mutating, &input, &p, None);
        assert!(
            r.contains("KEEPS RUNNING") && r.contains("30 min 0 s"),
            "{r}"
        );
        assert!(r.contains("accept connections"), "{r}");
        let fg = serde_json::json!({"command": "ls", "background": false});
        assert!(!approval_reason("bash", ToolRisk::Mutating, &fg, &p, None).contains("KEEPS"));
    }

    /// Review of PR 186: in a host chat the card says the command can
    /// accept connections whenever nothing stops it: the network on, or off
    /// on a node whose Landlock does not stop TCP (`GAP_HOST_NET_OPEN`);
    /// never where Landlock denies TCP.
    #[test]
    fn a_host_background_start_says_the_network_is_reachable_when_nothing_stops_it() {
        use harness_core::policy::ExecMode;
        use harness_tools::sandbox::Network;
        let input =
            serde_json::json!({"command": "npm run dev", "background": true, "timeout_secs": 600});
        let said = |net: Network, gaps: Vec<&'static str>| {
            let mut p = crate::NodePolicy::default();
            p.sandbox.network = net;
            p.exec = crate::exec::ExecOffers::new(Ok(()), Ok(()), gaps);
            approval_reason("bash", ToolRisk::Mutating, &input, &p, Some(ExecMode::Host))
                .contains("nothing stops it from accepting connections")
        };
        assert!(said(Network::On, vec![]), "the network on");
        assert!(
            said(Network::Off, vec![harness_protocol::GAP_HOST_NET_OPEN]),
            "off, but TCP is not stopped here"
        );
        assert!(
            !said(Network::Off, vec![harness_protocol::GAP_HOST_NETNS]),
            "off, and Landlock stops TCP"
        );
        // The sandbox chat keeps its sentence.
        let mut p = crate::NodePolicy::default();
        p.sandbox.network = Network::On;
        let r = approval_reason(
            "bash",
            ToolRisk::Mutating,
            &input,
            &p,
            Some(ExecMode::Sandbox),
        );
        assert!(
            r.contains("the network is on, so it can accept connections"),
            "{r}"
        );
    }
}

#[cfg(test)]
mod queued_context_tests {
    use super::QueuedContext;

    fn q(agent: Option<&str>, turn: &str) -> QueuedContext {
        QueuedContext {
            agent: agent.map(str::to_string),
            turn: turn.into(),
            context: None,
            standing: None,
        }
    }

    /// Review H1: a queued message's context goes only to the turn that
    /// reads it right after the turn it was queued in, run by the agent it
    /// was computed for. After a Stop (the next turn to end is a callback
    /// turn or another) or for another agent it is not used.
    #[test]
    fn a_queued_context_fits_only_its_turn_and_agent() {
        assert!(q(Some("general"), "t1").usable("t1", Some("general")));
        assert!(
            !q(Some("general"), "t1").usable("t1", Some("coder")),
            "another agent"
        );
        assert!(
            !q(Some("general"), "t1").usable("cb-2", Some("general")),
            "after a Stop"
        );
        assert!(q(None, "t1").usable("t1", None));
        assert!(!q(None, "t1").usable("t1", Some("coder")));
    }
}

#[cfg(test)]
mod callback_budget_tests {
    use super::*;

    fn at(seq: u64, k: EventKind) -> RawEvent {
        let (kind, payload) = k.into_parts();
        RawEvent {
            session_id: harness_core::SessionId::new(),
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    }

    fn started(seq: u64, cause: Option<TurnCause>) -> RawEvent {
        at(
            seq,
            EventKind::TurnStarted(TurnStarted {
                turn_id: format!("t{seq}"),
                agent: None,
                cause,
                environment: None,
                continued_by: None,
                retry: None,
            }),
        )
    }

    /// The callback budget counts callback turns since the person's last
    /// message; the turns of the agents that message called after the first
    /// (X.37) are the person's, not "turns without you".
    #[test]
    fn a_mention_turn_spends_none_of_the_callback_budget() {
        use harness_core::event::*;
        let msg: UserMessage =
            serde_json::from_value(serde_json::json!({"turn_id": "t1", "text": "x"})).unwrap();
        let mention = TurnCause::Mention(MentionCall {
            message_seq: 1,
            by: CalledBy::Person,
            tags: vec![],
        });
        let ended = at(
            4,
            EventKind::BackgroundEnded(BackgroundEnded {
                child: None,
                bg_id: "bg-1".into(),
                reason: BackgroundReason::Exited,
                exit_code: Some(0),
                signal: None,
                confirmed: BackgroundConfirmed::Ended,
                ran_ms: 1,
                output_bytes: 0,
                tail: String::new(),
                tail_cut: false,
                delivered: false,
            }),
        );
        let log = vec![
            at(1, EventKind::UserMessage(msg)),
            started(2, Some(mention.clone())),
            started(3, Some(mention)),
            ended,
        ];
        let (in_a_row, ids) = callback_check(&log, &["bg-1".into()]).unwrap();
        assert_eq!(
            (in_a_row, ids.as_slice()),
            (0, ["bg-1".to_string()].as_slice())
        );
        // A callback turn since the message counts.
        let mut log = log;
        log.insert(
            1,
            started(9, Some(TurnCause::Callback(vec!["bg-0".into()]))),
        );
        assert_eq!(callback_check(&log, &["bg-1".into()]).unwrap().0, 1);
    }
}

/// What a person's page edit made on this node: the new file's path (none
/// when a change was refused and nothing was written) and one result per
/// change.
pub(crate) struct Saved {
    pub path: Option<String>,
    pub results: Vec<harness_protocol::ChangeResult>,
}

fn change_failure(w: harness_tools::folder_edit::ChangeWhy) -> harness_protocol::ChangeFailure {
    use harness_protocol::ChangeFailure as C;
    use harness_tools::folder_edit::ChangeWhy as W;
    match w {
        W::BadPlace => C::BadPlace,
        W::Stale => C::Stale,
        W::Gone => C::Gone,
        W::NotEditable => C::NotEditable,
        W::BadFormula => C::BadFormula,
        W::TooLong => C::TooLong,
        W::Duplicate => C::Duplicate,
        W::Skipped => C::Skipped,
        W::Other => C::Other,
    }
}

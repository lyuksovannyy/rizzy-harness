//! `attached_file` on the master (README "Files in a chat without a
//! folder"; PLAN.md T95; docs/attached-files.md): a node's tool asks, for
//! the session it runs, to look into a file the person attached, and this
//! answers from the owner's stored copy.
//!
//! A built-in service over the master's own blob store: it runs no shell,
//! no file tool and no program the model names, and it never writes. The
//! file comes from the session alone: only a file whose id the chat's own
//! log names (`harness_store::session_files`, `logged`), never by owner,
//! never by an id the model invents, never one of another chat of the same
//! person; and only when the blob is the chat owner's. The chat must be a
//! plain chat (not a sub-agent's, not a temporary one), the node must offer
//! the tool, and the asking turn must be one the master granted it:
//! the log's open turn must not be a callback turn, must not have the tool
//! withheld (`AgentRef::withheld`), and its agent must have the switch
//! "Read attached files" (`Features::attached_files`).
//!
//! The work is bounded. The file is at most 10 MiB and is read once into
//! memory; parsing runs in `spawn_blocking` under a call time limit
//! ([`TIME_LIMIT`], checked inside every decompression), at most
//! [`AT_ONCE`] at a time on the whole master, with a short queue
//! ([`QUEUE`]); a turn may ask at most [`PER_TURN`] times. Decompression is
//! capped by `harness_attached` and nothing is held between calls.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use harness_attached::{self as peek, EntryKind};
use harness_core::SessionId;
use harness_core::attached::{
    AttachedAnswer, AttachedOp, AttachedRequest, EntryView, MAX_ENTRY_CHARS, MAX_ID_CHARS,
    ReadMode, TOOL,
};
use harness_protocol::MasterToNode;
use harness_store::OpenTurn;
use harness_store::session_files::SessionFile;

use crate::{Master, lock, storage};

/// Calls that parse at the same time, on the whole master.
pub const AT_ONCE: usize = 2;
/// Calls waiting for a place, at most; past it, refused at once.
pub const QUEUE: usize = 16;
/// How long a call waits for a place.
pub const WAIT: Duration = Duration::from_secs(10);
/// The most one call works (reading the copy and parsing it).
pub const TIME_LIMIT: Duration = Duration::from_secs(8);
/// Calls one turn may make (a listing in pages and a few reads).
pub const PER_TURN: u32 = 60;
/// Calls one chat may make in [`WINDOW`], whatever its turns: a node that
/// writes new `turn_started` events cannot reset this one.
pub const PER_WINDOW: usize = 240;
pub const WINDOW: Duration = Duration::from_secs(600);
/// Calls one node may have in progress or waiting at once.
pub const PER_NODE: usize = 4;
/// The largest copy served: the upload limit.
pub const MAX_FILE: u64 = 10 << 20;
/// Chats whose counts are kept; past it the oldest go, one at a time.
const KEPT: usize = 4096;

/// A map of per-chat values that forgets the oldest chat when it is full.
pub(crate) struct Recent<V> {
    map: HashMap<SessionId, V>,
    order: VecDeque<SessionId>,
}

impl<V> Default for Recent<V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl<V> Recent<V> {
    fn entry(&mut self, sid: SessionId, cap: usize, new: impl FnOnce() -> V) -> &mut V {
        if !self.map.contains_key(&sid) {
            while self.map.len() >= cap {
                match self.order.pop_front() {
                    Some(old) => {
                        self.map.remove(&old);
                    }
                    None => break,
                }
            }
            self.order.push_back(sid);
        }
        self.map.entry(sid).or_insert_with(new)
    }

    fn remove(&mut self, sid: SessionId) {
        if self.map.remove(&sid).is_some() {
            self.order.retain(|o| *o != sid);
        }
    }
}

/// What a refusal of the shared bounds says, for the service it is used by:
/// the attached-file reads (T95) and the files agents make (T97) have the
/// same limits and different words.
pub struct Words {
    /// A second call of a chat while one is in flight.
    pub one_at_a_time: &'static str,
    /// "this chat {did} {PER_WINDOW} times in the last N minutes".
    pub did: &'static str,
    /// The queue is full.
    pub queue: &'static str,
    /// No place came free in time.
    pub busy: &'static str,
}

const ATTACHED_WORDS: Words = Words {
    one_at_a_time: "one file read at a time in this chat: wait for the one before",
    did: "looked into attached files",
    queue: "too many file reads are waiting; try again in a moment",
    busy: "the server is busy reading other files; try again in a moment",
};

/// The same bounds for `doc_create` and `sheet_create` (`crate::made`).
pub(crate) const MADE_WORDS: Words = Words {
    one_at_a_time: "one file is made at a time in this chat: wait for the one before",
    did: "made files",
    queue: "too many files are waiting to be made; try again in a moment",
    busy: "the server is busy making other files; try again in a moment",
};

/// Calls waiting and parsing, and what each chat and node has used.
pub struct Grants {
    words: &'static Words,
    slots: Arc<tokio::sync::Semaphore>,
    waiting: AtomicUsize,
    wait: Duration,
    turns: Mutex<Recent<(String, u32)>>,
    window: Mutex<Recent<VecDeque<Instant>>>,
    busy: Mutex<HashSet<SessionId>>,
    nodes: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
    /// Tests only: unset in production, where `held` is one atomic load.
    gate: OnceLock<tokio::sync::watch::Sender<bool>>,
}

impl Default for Grants {
    fn default() -> Self {
        Self::with_wait(WAIT, &ATTACHED_WORDS)
    }
}

/// One call of a chat in flight; the chat may start another when it drops.
pub struct ChatGuard<'a> {
    grants: &'a Grants,
    sid: SessionId,
}

impl std::fmt::Debug for ChatGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChatGuard({})", self.sid)
    }
}

impl Drop for ChatGuard<'_> {
    fn drop(&mut self) {
        lock(&self.grants.busy).remove(&self.sid);
    }
}

impl Grants {
    pub(crate) fn with_wait(wait: Duration, words: &'static Words) -> Self {
        Self {
            words,
            slots: Arc::new(tokio::sync::Semaphore::new(AT_ONCE)),
            waiting: AtomicUsize::new(0),
            wait,
            turns: Mutex::default(),
            window: Mutex::default(),
            busy: Mutex::default(),
            nodes: Mutex::default(),
            gate: OnceLock::new(),
        }
    }

    /// Tests: from now on every call waits, once it holds its chat's place,
    /// until [`Grants::release`], so a call is provably in flight.
    pub(crate) fn hold(&self) {
        self.gate
            .get_or_init(|| tokio::sync::watch::channel(false).0);
    }

    /// Tests: let every waiting call, and every later one, go on.
    pub(crate) fn release(&self) {
        if let Some(g) = self.gate.get() {
            g.send_replace(true);
        }
    }

    /// Waits while the gate is armed and shut; returns at once otherwise.
    async fn held(&self) {
        if let Some(g) = self.gate.get() {
            let _ = g.subscribe().wait_for(|open| *open).await;
        }
    }

    pub fn forget(&self, sid: SessionId) {
        lock(&self.turns).remove(sid);
        lock(&self.window).remove(sid);
    }

    /// The one call a chat may have in flight, or why not. An honest model
    /// asks one at a time (the tool is not parallel-safe).
    pub(crate) fn chat(&self, sid: SessionId) -> Result<ChatGuard<'_>, String> {
        if !lock(&self.busy).insert(sid) {
            return Err(self.words.one_at_a_time.into());
        }
        Ok(ChatGuard { grants: self, sid })
    }

    /// The node's room for calls (taken before a task is spawned).
    pub(crate) fn node(&self, node_id: &str) -> Arc<tokio::sync::Semaphore> {
        let mut n = lock(&self.nodes);
        if n.len() >= KEPT && !n.contains_key(node_id) {
            n.clear();
        }
        n.entry(node_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(PER_NODE)))
            .clone()
    }

    /// Count one call of chat `sid`'s turn `turn`: [`PER_TURN`] a turn, and
    /// [`PER_WINDOW`] in [`WINDOW`] whatever the turns say.
    pub(crate) fn take(&self, sid: SessionId, turn: &str) -> Result<(), String> {
        let mut w = lock(&self.window);
        let q = w.entry(sid, KEPT, VecDeque::new);
        let now = Instant::now();
        while q.front().is_some_and(|t| now.duration_since(*t) > WINDOW) {
            q.pop_front();
        }
        if q.len() >= PER_WINDOW {
            return Err(format!(
                "this chat {} {PER_WINDOW} times in the last {} minutes; try again later",
                self.words.did,
                WINDOW.as_secs() / 60
            ));
        }
        let mut t = lock(&self.turns);
        let e = t.entry(sid, KEPT, || (turn.to_string(), 0));
        if e.0 != turn {
            *e = (turn.to_string(), 0);
        }
        if e.1 >= PER_TURN {
            return Err(format!(
                "this turn already {} {PER_TURN} times; work with what you have",
                self.words.did
            ));
        }
        e.1 += 1;
        q.push_back(now);
        Ok(())
    }

    /// Give back the count of a call that was refused for lack of room.
    pub(crate) fn give_back(&self, sid: SessionId, turn: &str) {
        if let Some(e) = lock(&self.turns).map.get_mut(&sid)
            && e.0 == turn
        {
            e.1 = e.1.saturating_sub(1);
        }
        if let Some(q) = lock(&self.window).map.get_mut(&sid) {
            q.pop_back();
        }
    }

    /// A place to parse in, or why not.
    pub(crate) async fn slot(&self) -> Result<tokio::sync::OwnedSemaphorePermit, String> {
        if self.waiting.fetch_add(1, Ordering::SeqCst) >= QUEUE {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            return Err(self.words.queue.into());
        }
        let got = tokio::time::timeout(self.wait, self.slots.clone().acquire_owned()).await;
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        match got {
            Ok(Ok(p)) => Ok(p),
            _ => Err(self.words.busy.into()),
        }
    }
}

/// Withhold the tool from an agent and every sub-agent it may hand work to.
pub fn deny(spec: &mut harness_core::agent::AgentSpec) {
    let deny = |list: &mut Vec<String>| {
        if !list.iter().any(|t| t == TOOL) {
            list.push(TOOL.into());
        }
    };
    deny(&mut spec.deny_tools);
    for d in &mut spec.delegates {
        deny(&mut d.deny_tools);
    }
}

/// Answer a node's `attached.request` from a task of its own (a read never
/// holds up the node's events). The answer goes to that node only.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, r: AttachedRequest) {
    // `attached.result` goes only to a node that announced the capability: a
    // request from one that did not is dropped, not answered.
    if !m.node_has(node_id, harness_protocol::CAP_ATTACHED) {
        tracing::warn!(node = %node_id, "attached.request from a node without the capability");
        return;
    }
    // The node's room first, as `recall` and the board do: a flood from one
    // node is refused here, before a task or a store read.
    let Ok(permit) = m.inner.attached.node(node_id).try_acquire_owned() else {
        m.send_unacked(
            node_id,
            MasterToNode::AttachedResult {
                req_id,
                answer: None,
                error: Some("too many file reads from this node at once; try again".into()),
            },
        );
        return;
    };
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let res = answer(&m, &node_id, sid, r).await;
        drop(permit);
        let (answer, error) = match res {
            Ok(a) => (Some(a), None),
            Err(e) => {
                // Lengths only: a refusal may carry text the model wrote.
                tracing::debug!(node = %node_id, session = %sid, len = e.len(), "attached_file refused");
                (None, Some(harness_core::text::clean_line(&e, 400)))
            }
        };
        m.send_unacked(
            &node_id,
            MasterToNode::AttachedResult {
                req_id,
                answer,
                error,
            },
        );
    });
}

/// The open turn of `row`, if it may use the tool: not a callback turn, the
/// tool not withheld from it, its agent with the switch.
async fn turn_may(m: &Master, row: &harness_store::SessionRow) -> Result<OpenTurn, String> {
    let sid = row.session_id;
    let open = m
        .inner
        .mirror
        .call(move |s| s.open_turn(sid))
        .await
        .ok()
        .flatten()
        .ok_or("attached files are not available outside a turn")?;
    if open.callback || open.withheld.iter().any(|t| t == TOOL) {
        return Err("attached files are not available in this turn".into());
    }
    let r = m.resolver_for(row.workspace_id.as_deref()).await;
    let on = open
        .agent_id
        .as_deref()
        .and_then(|id| r.agents().get(id))
        .is_some_and(|a| a.features.attached_files);
    if !on {
        return Err(NOT_OFFERED.into());
    }
    Ok(open)
}

/// What a turn is told when its agent has no switch.
pub const NOT_OFFERED: &str =
    "attached files are not available to this agent: its \"Read attached files\" switch is off";

/// The file `file_id` of chat `sid`, if its own log names it and it is the
/// chat owner's blob.
pub(crate) async fn file_of(
    m: &Master,
    row: &harness_store::SessionRow,
    file_id: &str,
) -> Result<SessionFile, String> {
    let (sid, id) = (row.session_id.to_string(), file_id.to_string());
    let file = m
        .inner
        .mirror
        .call(move |s| s.session_file(&sid, &id))
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "attached_file: the store failed");
            "the file could not be looked up".to_string()
        })?
        .ok_or("no such file in this chat: use the id the message names the file by")?;
    // The blob is the chat owner's, whatever a row says.
    if file.owner_id != row.owner_id {
        tracing::warn!(session = %row.session_id, "attached_file: a row's owner is not the chat's");
        return Err("no such file in this chat: use the id the message names the file by".into());
    }
    if file.size == 0 || file.size > MAX_FILE {
        return Err("this file is larger than attached files may be".into());
    }
    Ok(file)
}

/// The copy's bytes, read and decrypted off the async threads. Gone files
/// (the chat or the blob deleted meanwhile) are a sentence.
pub(crate) async fn bytes_of(m: &Master, file: &SessionFile) -> Result<Vec<u8>, String> {
    let gone = || "the file is gone: it was removed from the server".to_string();
    let path = storage::get(m)
        .and_then(|st| st.path(&file.blob_id))
        .ok_or_else(gone)?;
    let id = file.blob_id.clone();
    let key = m
        .inner
        .mirror
        .call(move |s| s.blob_key(&id))
        .await
        .ok()
        .flatten()
        .ok_or_else(gone)?;
    let (id, size) = (file.blob_id.clone(), file.size);
    tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut r = crate::blobs::Reader::open(&path, &id, &key, size)?;
        let mut out = Vec::with_capacity(size as usize);
        r.range(0, size - 1, |b| {
            out.extend_from_slice(b);
            Ok(())
        })?;
        Ok(out)
    })
    .await
    .map_err(|_| "reading the file failed".to_string())?
    .map_err(|e| {
        tracing::warn!(error = %e, "attached_file: a stored copy could not be read");
        gone()
    })
}

pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    q: AttachedRequest,
) -> Result<AttachedAnswer, String> {
    if !m.node_has(node_id, harness_protocol::CAP_ATTACHED) {
        return Err("this node does not offer attached files".into());
    }
    // One call in flight per chat, refused at once: the master-wide queue is
    // every user's, and one chat must not fill it.
    let _one = m.inner.attached.chat(sid)?;
    m.inner.attached.held().await;
    let (row, _top) = crate::recall::sessions(m, node_id, sid).await?;
    if row.parent_id.is_some() {
        return Err("a sub-agent's chat has no attached files".into());
    }
    if row.profile.as_deref() != Some("chat") {
        return Err("only a chat without a folder keeps attached files for this tool".into());
    }
    if row.incognito {
        return Err("a temporary chat keeps no attached files".into());
    }
    // Whether the asking turn may use it, from the log at the time of the
    // request, never from a flag a past prompt left.
    let open = turn_may(m, &row).await?;
    m.inner.attached.take(sid, &open.turn_id)?;
    if q.file_id.is_empty()
        || q.file_id.len() > MAX_ID_CHARS
        || !q
            .file_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("no such file in this chat: use the id the message names the file by".into());
    }
    if q.entry
        .as_ref()
        .is_some_and(|e| e.chars().count() > MAX_ENTRY_CHARS)
    {
        return Err(format!("`entry` is at most {MAX_ENTRY_CHARS} characters"));
    }
    let file = file_of(m, &row, &q.file_id).await?;
    // A refusal for lack of room is not a call: it gives its count back.
    let slot = match m.inner.attached.slot().await {
        Ok(s) => s,
        Err(e) => {
            m.inner.attached.give_back(sid, &open.turn_id);
            return Err(e);
        }
    };
    let started = Instant::now();
    let bytes = bytes_of(m, &file).await?;
    let deadline = started + TIME_LIMIT;
    // The place is held until the work really ends, also when the call
    // above gave up waiting for it.
    let work = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        run(&file, &bytes, &q, deadline)
    });
    match tokio::time::timeout(TIME_LIMIT + Duration::from_secs(2), work).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err("reading the file failed".into()),
        Err(_) => Err("reading the file took too long and was stopped".into()),
    }
}

/// The parse, off the async threads. Pure: bytes in, an answer out.
pub fn run(
    file: &SessionFile,
    bytes: &[u8],
    q: &AttachedRequest,
    deadline: Instant,
) -> Result<AttachedAnswer, String> {
    match q.op {
        AttachedOp::Info => {
            let d = peek::describe(bytes);
            Ok(AttachedAnswer::Info {
                name: file.name.clone(),
                bytes: file.size,
                sha256: file.sha256.clone(),
                label: d.label,
                mime: d.mime,
                archive: d.kind.is_archive(),
            })
        }
        AttachedOp::List => {
            let offset = usize::try_from(q.offset.unwrap_or(0)).unwrap_or(usize::MAX);
            let limit = usize::try_from(q.limit.unwrap_or(peek::LIST_PAGE as u64))
                .unwrap_or(peek::LIST_PAGE);
            let l = peek::list(bytes, offset, limit, deadline)?;
            let more = l.more();
            let entries = l
                .entries
                .iter()
                .map(|e| {
                    let (kind, detail) = match &e.kind {
                        EntryKind::File => ("file", String::new()),
                        EntryKind::Dir => ("folder", String::new()),
                        EntryKind::Link(t) => ("link", peek::clean_path(t)),
                        EntryKind::Other(w) => ("other", (*w).to_string()),
                    };
                    EntryView {
                        index: e.index as u64,
                        path: e.path.clone(),
                        size: e.size,
                        kind: kind.into(),
                        detail,
                    }
                })
                .collect();
            Ok(AttachedAnswer::List {
                archive: match l.kind {
                    peek::Kind::Zip => "ZIP archive",
                    peek::Kind::Tar => "tar archive",
                    peek::Kind::TarGz => "gzip-compressed tar archive",
                    peek::Kind::Gz => "gzip-compressed data",
                    peek::Kind::Plain => "file",
                }
                .into(),
                entries,
                offset: l.offset as u64,
                scanned: l.scanned as u64,
                complete: l.complete,
                more,
                note: l.note,
            })
        }
        AttachedOp::Read => {
            let mode = match q.mode.unwrap_or_default() {
                ReadMode::Auto => peek::Mode::Auto,
                ReadMode::Hex => peek::Mode::Hex,
                ReadMode::Strings => peek::Mode::Strings,
            };
            let req = peek::ReadReq {
                entry: q.entry.as_deref(),
                offset: q.offset,
                limit: q.limit,
                mode,
            };
            let o = peek::read(bytes, &req, deadline)?;
            Ok(AttachedAnswer::Read {
                mode: o.mode.into(),
                text: o.text,
                from: o.from,
                shown: o.shown,
                next: o.next,
                size: o.size,
                note: o.note,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid() -> SessionId {
        SessionId::new()
    }

    /// The queue is bounded: with every place taken and [`QUEUE`] calls
    /// waiting, the next is refused at once with its own sentence.
    #[tokio::test]
    async fn the_queue_is_bounded() {
        let g = Arc::new(Grants::with_wait(
            Duration::from_millis(400),
            &ATTACHED_WORDS,
        ));
        let mut held = Vec::new();
        for _ in 0..AT_ONCE {
            held.push(g.slot().await.unwrap());
        }
        let waiters: Vec<_> = (0..QUEUE)
            .map(|_| {
                let g = g.clone();
                tokio::spawn(async move { g.slot().await.map(|_| ()) })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = Instant::now();
        let e = g.slot().await.unwrap_err();
        assert!(e.contains("too many file reads are waiting"), "{e}");
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "refused at once"
        );
        // The ones in the queue give up with the busy sentence, not the queue's.
        for w in waiters {
            let e = w.await.unwrap().unwrap_err();
            assert!(e.contains("busy"), "{e}");
        }
        // A place that frees up is taken again.
        drop(held);
        assert!(g.slot().await.is_ok());
    }

    /// One call in flight per chat; another chat is not held up.
    #[test]
    fn a_chat_has_one_call_at_a_time() {
        let g = Grants::default();
        let (a, b) = (sid(), sid());
        let one = g.chat(a).unwrap();
        assert!(g.chat(a).unwrap_err().contains("one file read at a time"));
        assert!(g.chat(b).is_ok());
        drop(one);
        assert!(g.chat(a).is_ok());
    }

    /// The node's room: [`PER_NODE`] at once, then refused; another node's
    /// is its own.
    #[test]
    fn a_node_has_its_own_room() {
        let g = Grants::default();
        let held: Vec<_> = (0..PER_NODE)
            .map(|_| g.node("n1").try_acquire_owned().unwrap())
            .collect();
        assert!(g.node("n1").try_acquire_owned().is_err());
        assert!(g.node("n2").try_acquire_owned().is_ok());
        drop(held);
        assert!(g.node("n1").try_acquire_owned().is_ok());
    }

    /// 60 a turn, and 240 a window whatever the turns: a node that writes a
    /// new `turn_started` each time resets the first and not the second. A
    /// call refused for lack of room is given back.
    #[test]
    fn counts_per_turn_and_per_window_hold() {
        let g = Grants::default();
        let s = sid();
        for i in 0..PER_TURN {
            g.take(s, "t1").unwrap_or_else(|e| panic!("{i}: {e}"));
        }
        assert!(g.take(s, "t1").unwrap_err().contains("60 times"));
        // A new turn starts the turn's count again...
        let mut n = PER_TURN as usize;
        // ...but the window counts all of them.
        let mut last = Ok(());
        for i in 0..PER_WINDOW {
            last = g.take(s, &format!("turn{i}"));
            if last.is_err() {
                break;
            }
            n += 1;
        }
        assert_eq!(n, PER_WINDOW, "{last:?}");
        assert!(last.unwrap_err().contains("240 times"));
        // Another chat is not charged.
        assert!(g.take(sid(), "t").is_ok());
        // Giving back frees one.
        let s2 = sid();
        g.take(s2, "t").unwrap();
        g.give_back(s2, "t");
        for _ in 0..PER_TURN {
            g.take(s2, "t").unwrap();
        }
        assert!(g.take(s2, "t").is_err());
    }

    /// Past [`KEPT`] chats the oldest is forgotten, one at a time: the
    /// others keep their counts.
    #[test]
    fn the_oldest_chat_is_forgotten_first() {
        let g = Grants::default();
        let first = sid();
        for _ in 0..PER_TURN {
            g.take(first, "t").unwrap();
        }
        let second = sid();
        for _ in 0..PER_TURN {
            g.take(second, "t").unwrap();
        }
        for _ in 0..KEPT - 1 {
            g.take(sid(), "t").unwrap();
        }
        // `first` went; `second` is still full.
        assert!(g.take(second, "t").is_err());
        assert!(g.take(first, "t").is_ok());
    }
}

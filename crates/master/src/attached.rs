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

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
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
/// The largest copy served: the upload limit.
pub const MAX_FILE: u64 = 10 << 20;
/// Chats whose counts are kept; past it the oldest go.
const KEPT: usize = 4096;

/// Calls waiting and parsing, and the calls of each chat's open turn.
pub struct Grants {
    slots: std::sync::Arc<tokio::sync::Semaphore>,
    waiting: AtomicUsize,
    turns: Mutex<HashMap<SessionId, (String, u32)>>,
}

impl Default for Grants {
    fn default() -> Self {
        Self {
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(AT_ONCE)),
            waiting: AtomicUsize::new(0),
            turns: Mutex::default(),
        }
    }
}

impl Grants {
    pub fn forget(&self, sid: SessionId) {
        lock(&self.turns).remove(&sid);
    }

    fn take(&self, sid: SessionId, turn: &str) -> Result<(), String> {
        let mut t = lock(&self.turns);
        if t.len() >= KEPT && !t.contains_key(&sid) {
            t.clear();
        }
        let e = t.entry(sid).or_insert_with(|| (turn.to_string(), 0));
        if e.0 != turn {
            *e = (turn.to_string(), 0);
        }
        if e.1 >= PER_TURN {
            return Err(format!(
                "this turn already looked into attached files {PER_TURN} times; work with what you have"
            ));
        }
        e.1 += 1;
        Ok(())
    }

    /// A place to parse in, or why not.
    async fn slot(&self) -> Result<tokio::sync::OwnedSemaphorePermit, String> {
        if self.waiting.fetch_add(1, Ordering::SeqCst) >= QUEUE {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            return Err("too many file reads are waiting; try again in a moment".into());
        }
        let got = tokio::time::timeout(WAIT, self.slots.clone().acquire_owned()).await;
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        match got {
            Ok(Ok(p)) => Ok(p),
            _ => Err("the server is busy reading other files; try again in a moment".into()),
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
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let res = answer(&m, &node_id, sid, r).await;
        let (answer, error) = match res {
            Ok(a) => (Some(a), None),
            Err(e) => {
                tracing::debug!(node = %node_id, session = %sid, why = %e, "attached_file refused");
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
pub const NOT_OFFERED: &str = "attached files are not available to this agent: its \"Read attached files\" switch is off";

/// The file `file_id` of chat `sid`, if its own log names it and it is the
/// chat owner's blob.
async fn file_of(
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
async fn bytes_of(m: &Master, file: &SessionFile) -> Result<Vec<u8>, String> {
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
        || !q.file_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("no such file in this chat: use the id the message names the file by".into());
    }
    if q.entry.as_ref().is_some_and(|e| e.chars().count() > MAX_ENTRY_CHARS) {
        return Err(format!("`entry` is at most {MAX_ENTRY_CHARS} characters"));
    }
    let file = file_of(m, &row, &q.file_id).await?;
    let _slot = m.inner.attached.slot().await?;
    let started = Instant::now();
    let bytes = bytes_of(m, &file).await?;
    let deadline = started + TIME_LIMIT;
    let work = tokio::task::spawn_blocking(move || run(&file, &bytes, &q, deadline));
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

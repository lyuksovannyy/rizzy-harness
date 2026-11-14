//! What the agent has seen of each file, per session: the table behind the
//! staleness guard of `edit` and `write` (ADOPTION_PLAN T.2, PLAN.md T45).
//!
//! `read` records the file's SHA-256 (and whether it showed every line);
//! `edit` and `write` check it before they change a file and record what
//! they wrote. The event log is the authority: each such call's
//! `tool_call_finished` carries a [`FileSeen`], and at the start of every
//! turn the session's table is rebuilt from the log ([`SeenStore::sync`]),
//! so a node restart loses nothing but the snapshots below.
//!
//! A snapshot is the text the agent was shown, kept in memory (bounded) so
//! that a file changed on disk since can still be edited where it did not
//! change, and so a refusal can show the lines that did. Without one (after
//! a restart, for a large file, or once evicted) a changed file is refused
//! with "read it again": the guard degrades, it never wedges.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use harness_core::event::FileSeen;
use harness_core::{RawEvent, SessionId};
use sha2::{Digest, Sha256};

/// Largest file whose text is kept as a snapshot.
pub const SNAPSHOT_MAX_FILE: usize = 512 << 10;
/// Snapshot bytes kept over all sessions of the node; past it the least
/// recently used snapshots go (their hashes stay).
pub const SNAPSHOT_MAX_TOTAL: usize = 8 << 20;
/// Files remembered per session; past it the least recently used is
/// forgotten (the agent then has to read it again).
pub const MAX_FILES: usize = 4096;
/// Session tables kept; the least recently used goes (it is rebuilt from
/// the log on that session's next turn, without snapshots).
pub const MAX_SESSIONS: usize = 256;

/// Seq of an entry recorded in this turn, not yet in the log.
const UNLOGGED: u64 = u64::MAX;

pub type Sha = [u8; 32];

pub fn sha256(bytes: &[u8]) -> Sha {
    Sha256::digest(bytes).into()
}

pub fn hex(sha: &Sha) -> String {
    sha.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Sha> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// One file as the agent last saw it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub sha: Sha,
    /// Every line was shown (or written) since the last summary.
    pub full: bool,
    /// The text it was shown, if still kept.
    pub snapshot: Option<Arc<String>>,
}

#[derive(Debug, Clone)]
struct Entry {
    seen: Seen,
    /// Seq of the event that recorded it, or [`UNLOGGED`].
    seq: u64,
    used: u64,
}

#[derive(Default)]
struct Table {
    files: HashMap<PathBuf, Entry>,
    used: u64,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<SessionId, Table>,
    snap_bytes: usize,
    tick: u64,
}

impl Inner {
    fn tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    fn snap_len(e: &Entry) -> usize {
        e.seen.snapshot.as_ref().map_or(0, |s| s.len())
    }

    fn evict(&mut self) {
        while self.sessions.len() > MAX_SESSIONS {
            let Some(oldest) = self
                .sessions
                .iter()
                .min_by_key(|(_, t)| t.used)
                .map(|(s, _)| *s)
            else {
                break;
            };
            if let Some(t) = self.sessions.remove(&oldest) {
                self.snap_bytes -= t.files.values().map(Self::snap_len).sum::<usize>();
            }
        }
        while self.snap_bytes > SNAPSHOT_MAX_TOTAL {
            let oldest = self
                .sessions
                .iter_mut()
                .flat_map(|(_, t)| t.files.values_mut())
                .filter(|e| e.seen.snapshot.is_some())
                .min_by_key(|e| e.used);
            match oldest {
                Some(e) => {
                    let n = Self::snap_len(e);
                    e.seen.snapshot = None;
                    self.snap_bytes -= n;
                }
                None => break,
            }
        }
    }
}

/// The tables of every session on this node.
#[derive(Default)]
pub struct SeenStore {
    inner: Mutex<Inner>,
}

impl SeenStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The store as one session sees it (what a tool call is given).
    pub fn session(self: &Arc<Self>, session: SessionId) -> SessionSeen {
        SessionSeen {
            store: self.clone(),
            session,
        }
    }

    /// Rebuild `session`'s table from its log: per path the latest
    /// [`FileSeen`] wins; one recorded before the latest summary no longer
    /// counts as a whole read (the model has only the summary of it).
    /// Snapshots are kept where the hash is the one the log names.
    pub fn sync(&self, session: SessionId, history: &[RawEvent]) {
        let mut through = None;
        let mut latest: HashMap<PathBuf, (FileSeen, u64)> = HashMap::new();
        for e in history {
            match e.kind.as_str() {
                "history_summarized" => {
                    through = e
                        .payload
                        .get("through_seq")
                        .and_then(|v| v.as_u64())
                        .or(through)
                }
                "tool_call_finished" => {
                    if let Some(s) = e.payload.get("seen")
                        && let Ok(f) = serde_json::from_value::<FileSeen>(s.clone())
                    {
                        latest.insert(PathBuf::from(&f.path), (f, e.seq));
                    }
                }
                _ => {}
            }
        }
        let mut inner = self.lock();
        let now = inner.tick();
        let old = inner.sessions.remove(&session).unwrap_or_default();
        let mut table = Table {
            files: HashMap::new(),
            used: now,
        };
        for (path, (f, seq)) in latest {
            let Some(sha) = unhex(&f.sha256) else {
                continue;
            };
            let snapshot = old
                .files
                .get(&path)
                .filter(|e| e.seen.sha == sha)
                .and_then(|e| e.seen.snapshot.clone());
            let full = f.full && through.is_none_or(|t| seq > t);
            table.files.insert(
                path,
                Entry {
                    seen: Seen {
                        sha,
                        full,
                        snapshot,
                    },
                    seq,
                    used: now,
                },
            );
        }
        // Keep the most recent files only.
        if table.files.len() > MAX_FILES {
            let mut by_seq: Vec<(PathBuf, u64)> = table
                .files
                .iter()
                .map(|(p, e)| (p.clone(), e.seq))
                .collect();
            by_seq.sort_by_key(|(_, s)| *s);
            for (p, _) in &by_seq[..by_seq.len() - MAX_FILES] {
                table.files.remove(p);
            }
        }
        inner.snap_bytes -= old.files.values().map(Inner::snap_len).sum::<usize>();
        inner.snap_bytes += table.files.values().map(Inner::snap_len).sum::<usize>();
        inner.sessions.insert(session, table);
        inner.evict();
    }

    /// A summary now stands for the log up to `through`: what was read
    /// there no longer counts as a whole read.
    pub fn demote(&self, session: SessionId, through: u64) {
        if let Some(t) = self.lock().sessions.get_mut(&session) {
            for e in t.files.values_mut() {
                if e.seq <= through {
                    e.seen.full = false;
                }
            }
        }
    }
}

/// One session's table.
#[derive(Clone)]
pub struct SessionSeen {
    store: Arc<SeenStore>,
    session: SessionId,
}

impl Default for SessionSeen {
    /// A table of its own (tools run outside a session, tests).
    fn default() -> Self {
        Arc::new(SeenStore::default()).session(SessionId::new())
    }
}

impl SessionSeen {
    pub fn get(&self, path: &Path) -> Option<Seen> {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let t = inner.sessions.get_mut(&self.session)?;
        t.used = now;
        let e = t.files.get_mut(path)?;
        e.used = now;
        Some(e.seen.clone())
    }

    /// Record that the agent now knows `path` as `bytes` (a snapshot is
    /// kept when it is small text). Returns the event's record of it.
    pub fn record(&self, path: &Path, bytes: &[u8], full: bool) -> FileSeen {
        let sha = sha256(bytes);
        let snapshot = (bytes.len() <= SNAPSHOT_MAX_FILE)
            .then(|| std::str::from_utf8(bytes).ok())
            .flatten()
            .map(|s| Arc::new(s.to_string()));
        self.put(path, sha, full, snapshot)
    }

    /// [`Self::record`] for a file too large to hold: its hash only.
    pub fn record_hash(&self, path: &Path, sha: Sha, full: bool) -> FileSeen {
        self.put(path, sha, full, None)
    }

    fn put(&self, path: &Path, sha: Sha, full: bool, snapshot: Option<Arc<String>>) -> FileSeen {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let added = snapshot.as_ref().map_or(0, |s| s.len());
        let t = inner.sessions.entry(self.session).or_default();
        t.used = now;
        let old = t.files.insert(
            path.to_path_buf(),
            Entry {
                seen: Seen {
                    sha,
                    full,
                    snapshot,
                },
                seq: UNLOGGED,
                used: now,
            },
        );
        if t.files.len() > MAX_FILES
            && let Some(p) = t
                .files
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(p, _)| p.clone())
            && let Some(e) = t.files.remove(&p)
        {
            inner.snap_bytes -= Inner::snap_len(&e);
        }
        inner.snap_bytes -= old.as_ref().map_or(0, Inner::snap_len);
        inner.snap_bytes += added;
        inner.evict();
        FileSeen {
            path: path.display().to_string(),
            sha256: hex(&sha),
            full,
        }
    }
}

/// One `edit` or `write` at a time per real path, on the whole node (two
/// sessions, or parallel sub-agents, on one file): the guard's check and the
/// write happen under it, so no change slips between them.
pub(crate) async fn lock_path(path: &Path) -> PathLock {
    static LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
        LazyLock::new(Default::default);
    let m = LOCKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(path.to_path_buf())
        .or_default()
        .clone();
    let guard = m.clone().lock_owned().await;
    PathLock {
        locks: &LOCKS,
        path: path.to_path_buf(),
        m,
        guard: Some(guard),
    }
}

pub(crate) struct PathLock {
    locks: &'static Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
    path: PathBuf,
    m: Arc<tokio::sync::Mutex<()>>,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Drop for PathLock {
    fn drop(&mut self) {
        self.guard.take();
        let mut map = self.locks.lock().unwrap_or_else(|p| p.into_inner());
        // The map's and ours: nobody else holds or waits for it.
        if Arc::strong_count(&self.m) == 2 {
            map.remove(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::EventKind;
    use harness_core::event::{HistorySummarized, ToolCallFinished};

    fn raw(sid: SessionId, seq: u64, kind: EventKind) -> RawEvent {
        let (kind, payload) = kind.into_parts();
        RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    }

    fn finished(seen: FileSeen) -> EventKind {
        EventKind::ToolCallFinished(ToolCallFinished {
            seen: Some(seen),
            call_id: "c".into(),
            ok: true,
            output: String::new(),
            exit_code: None,
            truncated: false,
            sources: vec![],
            note: None,
        })
    }

    #[test]
    fn a_restart_rebuilds_the_hashes_from_the_log_without_snapshots() {
        let sid = SessionId::new();
        let first = Arc::new(SeenStore::default());
        let rec = first.session(sid).record(Path::new("/w/a"), b"x\n", true);
        assert!(
            first
                .session(sid)
                .get(Path::new("/w/a"))
                .unwrap()
                .snapshot
                .is_some()
        );
        let log = vec![raw(sid, 3, finished(rec))];
        // Same process: the snapshot is kept.
        first.sync(sid, &log);
        let s = first.session(sid).get(Path::new("/w/a")).unwrap();
        assert!(s.full && s.snapshot.is_some() && s.sha == sha256(b"x\n"));
        // A new process: the hash, not the text.
        let second = Arc::new(SeenStore::default());
        second.sync(sid, &log);
        let s = second.session(sid).get(Path::new("/w/a")).unwrap();
        assert!(s.full && s.snapshot.is_none() && s.sha == sha256(b"x\n"));
        // What the log does not hold (an append that failed) is gone.
        first.session(sid).record(Path::new("/w/b"), b"y", true);
        first.sync(sid, &log);
        assert!(first.session(sid).get(Path::new("/w/b")).is_none());
    }

    #[test]
    fn a_summary_turns_whole_reads_before_it_into_partial_ones() {
        let sid = SessionId::new();
        let s = Arc::new(SeenStore::default());
        let a = s.session(sid).record(Path::new("/w/a"), b"a", true);
        let b = s.session(sid).record(Path::new("/w/b"), b"b", true);
        let log = vec![
            raw(sid, 2, finished(a)),
            raw(
                sid,
                5,
                EventKind::HistorySummarized(HistorySummarized {
                    turn_id: "t".into(),
                    through_seq: 2,
                    text: "s".into(),
                }),
            ),
            raw(sid, 7, finished(b)),
        ];
        s.sync(sid, &log);
        assert!(!s.session(sid).get(Path::new("/w/a")).unwrap().full);
        assert!(s.session(sid).get(Path::new("/w/b")).unwrap().full);
        s.demote(sid, 7);
        assert!(!s.session(sid).get(Path::new("/w/b")).unwrap().full);
    }

    #[test]
    fn snapshots_are_bounded_over_all_sessions() {
        let s = Arc::new(SeenStore::default());
        let big = vec![b'a'; SNAPSHOT_MAX_FILE];
        let n = SNAPSHOT_MAX_TOTAL / SNAPSHOT_MAX_FILE + 4;
        for i in 0..n {
            let sid = SessionId::new();
            s.session(sid)
                .record(&PathBuf::from(format!("/w/{i}")), &big, true);
        }
        assert!(s.lock().snap_bytes <= SNAPSHOT_MAX_TOTAL);
        // Too large to keep at all: the hash only.
        let sid = SessionId::new();
        let huge = vec![b'a'; SNAPSHOT_MAX_FILE + 1];
        s.session(sid).record(Path::new("/w/huge"), &huge, true);
        let e = s.session(sid).get(Path::new("/w/huge")).unwrap();
        assert!(e.snapshot.is_none() && e.sha == sha256(&huge));
    }

    #[tokio::test]
    async fn the_path_lock_serialises_and_cleans_up() {
        let p = Path::new("/w/locked");
        let a = lock_path(p).await;
        let second = tokio::spawn(async move { lock_path(Path::new("/w/locked")).await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!second.is_finished());
        drop(a);
        drop(second.await.unwrap());
    }
}

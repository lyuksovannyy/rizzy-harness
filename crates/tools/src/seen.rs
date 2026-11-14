//! What the agent has seen of each file, per session: the table behind the
//! staleness guard of `edit` and `write` (ADOPTION_PLAN T.2, PLAN.md T45).
//!
//! `read` records the file's SHA-256; `edit` and `write` refuse a file the
//! agent has not read, or that changed since, and record what they wrote. The event log is the authority: each such call's
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
pub const MAX_FILES: usize = 1024;
/// Files remembered over all sessions of the node, least recently used
/// forgotten first. Worst case per entry: a path of `PATH_MAX` (4,096
/// bytes) plus about 200 bytes of entry and map overhead, so 4,096 entries
/// are at most about 17.6 MB, and with [`SNAPSHOT_MAX_TOTAL`] about 26 MB:
/// under the 50 MB idle rule even if every path were as long as allowed
/// (a typical path of 100 bytes makes it about 1.2 MB).
pub const MAX_ENTRIES: usize = 4096;
/// Session tables kept; the least recently used goes (it is rebuilt from
/// the log on that session's next turn, without snapshots).
pub const MAX_SESSIONS: usize = 256;

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
    /// The text it was shown, if still kept.
    pub snapshot: Option<Arc<String>>,
}

#[derive(Debug, Clone)]
struct Entry {
    seen: Seen,
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
    /// Entries over all tables.
    entries: usize,
    tick: u64,
}

impl Inner {
    fn drop_table(&mut self, sid: SessionId) {
        if let Some(t) = self.sessions.remove(&sid) {
            self.snap_bytes -= t.files.values().map(Self::snap_len).sum::<usize>();
            self.entries -= t.files.len();
        }
    }

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
            self.drop_table(oldest);
        }
        while self.entries > MAX_ENTRIES {
            let oldest = self
                .sessions
                .iter()
                .flat_map(|(s, t)| t.files.iter().map(move |(p, e)| (e.used, *s, p)))
                .min_by_key(|(used, _, _)| *used)
                .map(|(_, s, p)| (s, p.clone()));
            let Some((sid, path)) = oldest else { break };
            if let Some(t) = self.sessions.get_mut(&sid)
                && let Some(e) = t.files.remove(&path)
            {
                self.snap_bytes -= Self::snap_len(&e);
                self.entries -= 1;
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
    /// [`FileSeen`] wins. Snapshots are kept where the hash is the one the
    /// log names.
    pub fn sync(&self, session: SessionId, history: &[RawEvent]) {
        let mut latest: HashMap<PathBuf, (Sha, u64)> = HashMap::new();
        for e in history {
            if e.kind == "tool_call_finished"
                && let Some(s) = e.payload.get("seen")
                && let Ok(f) = serde_json::from_value::<FileSeen>(s.clone())
                && let Some(sha) = unhex(&f.sha256)
            {
                latest.insert(PathBuf::from(&f.path), (sha, e.seq));
            }
        }
        // Keep the most recent files only.
        if latest.len() > MAX_FILES {
            let mut by_seq: Vec<(PathBuf, u64)> =
                latest.iter().map(|(p, (_, s))| (p.clone(), *s)).collect();
            by_seq.sort_by_key(|(_, s)| *s);
            for (p, _) in &by_seq[..by_seq.len() - MAX_FILES] {
                latest.remove(p);
            }
        }
        let mut inner = self.lock();
        let now = inner.tick();
        let old = inner.sessions.get(&session).map(|t| {
            t.files
                .iter()
                .filter_map(|(p, e)| Some((p.clone(), (e.seen.sha, e.seen.snapshot.clone()?))))
                .collect::<HashMap<_, _>>()
        });
        inner.drop_table(session);
        let mut table = Table {
            files: HashMap::new(),
            used: now,
        };
        for (path, (sha, _)) in latest {
            let snapshot = old
                .as_ref()
                .and_then(|o| o.get(&path))
                .filter(|(s, _)| *s == sha)
                .map(|(_, t)| t.clone());
            table.files.insert(
                path,
                Entry {
                    seen: Seen { sha, snapshot },
                    used: now,
                },
            );
        }
        inner.snap_bytes += table.files.values().map(Inner::snap_len).sum::<usize>();
        inner.entries += table.files.len();
        inner.sessions.insert(session, table);
        inner.evict();
    }

    /// Drop everything kept for `session` (it was deleted).
    pub fn forget(&self, session: SessionId) {
        self.lock().drop_table(session);
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
    pub fn record(&self, path: &Path, bytes: &[u8]) -> FileSeen {
        let sha = sha256(bytes);
        let snapshot = (bytes.len() <= SNAPSHOT_MAX_FILE)
            .then(|| std::str::from_utf8(bytes).ok())
            .flatten()
            .map(|s| Arc::new(s.to_string()));
        self.put(path, sha, snapshot)
    }

    /// [`Self::record`] for a file too large to hold: its hash only.
    pub fn record_hash(&self, path: &Path, sha: Sha) -> FileSeen {
        self.put(path, sha, None)
    }

    fn put(&self, path: &Path, sha: Sha, snapshot: Option<Arc<String>>) -> FileSeen {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let added = snapshot.as_ref().map_or(0, |s| s.len());
        let t = inner.sessions.entry(self.session).or_default();
        t.used = now;
        let old = t.files.insert(
            path.to_path_buf(),
            Entry {
                seen: Seen { sha, snapshot },
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
            inner.entries -= 1;
        }
        match &old {
            Some(o) => inner.snap_bytes -= Inner::snap_len(o),
            None => inner.entries += 1,
        }
        inner.snap_bytes += added;
        inner.evict();
        FileSeen {
            path: path.display().to_string(),
            sha256: hex(&sha),
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
    use harness_core::event::ToolCallFinished;

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
        let rec = first.session(sid).record(Path::new("/w/a"), b"x\n");
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
        assert!(s.snapshot.is_some() && s.sha == sha256(b"x\n"));
        // A new process: the hash, not the text.
        let second = Arc::new(SeenStore::default());
        second.sync(sid, &log);
        let s = second.session(sid).get(Path::new("/w/a")).unwrap();
        assert!(s.snapshot.is_none() && s.sha == sha256(b"x\n"));
        // What the log does not hold (an append that failed) is gone.
        first.session(sid).record(Path::new("/w/b"), b"y");
        first.sync(sid, &log);
        assert!(first.session(sid).get(Path::new("/w/b")).is_none());
    }

    #[test]
    fn entries_are_bounded_over_all_sessions_and_a_deleted_session_is_forgotten() {
        let s = Arc::new(SeenStore::default());
        let sid = SessionId::new();
        s.session(sid).record(Path::new("/w/kept"), b"k");
        for i in 0..MAX_ENTRIES + 10 {
            // Spread over sessions, so the per-session bound is not what acts.
            let other = SessionId::new();
            s.session(other)
                .record(&PathBuf::from(format!("/w/{i}")), b"x");
            assert!(s.lock().entries <= MAX_ENTRIES);
        }
        let counted: usize = s.lock().sessions.values().map(|t| t.files.len()).sum();
        assert_eq!(counted, s.lock().entries);
        // The least recently used went first.
        assert!(s.session(sid).get(Path::new("/w/kept")).is_none());
        let sid = SessionId::new();
        s.session(sid).record(Path::new("/w/mine"), b"m");
        let before = s.lock().entries;
        s.forget(sid);
        assert!(s.session(sid).get(Path::new("/w/mine")).is_none());
        assert_eq!(s.lock().entries, before - 1);
    }

    #[test]
    fn snapshots_are_bounded_over_all_sessions() {
        let s = Arc::new(SeenStore::default());
        let big = vec![b'a'; SNAPSHOT_MAX_FILE];
        let n = SNAPSHOT_MAX_TOTAL / SNAPSHOT_MAX_FILE + 4;
        for i in 0..n {
            let sid = SessionId::new();
            s.session(sid)
                .record(&PathBuf::from(format!("/w/{i}")), &big);
        }
        assert!(s.lock().snap_bytes <= SNAPSHOT_MAX_TOTAL);
        // Too large to keep at all: the hash only.
        let sid = SessionId::new();
        let huge = vec![b'a'; SNAPSHOT_MAX_FILE + 1];
        s.session(sid).record(Path::new("/w/huge"), &huge);
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

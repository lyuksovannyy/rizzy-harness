//! What the agent has seen of each file, per session: the table behind the
//! staleness guard of `edit` and `write` (ADOPTION_PLAN T.2, PLAN.md T45).
//!
//! `read` records the file's SHA-256; `edit` and `write` refuse a file the
//! agent has not read, or that changed since, and record what they wrote. The event log is the authority: each such call's
//! `tool_call_finished` carries a [`FileSeen`] (and so does a file the
//! person mentioned with `@path` in its `user_message`, PLAN.md T63), and at the start of every
//! turn the session's table is rebuilt from the log ([`SeenStore::sync`]),
//! so a node restart loses nothing but the snapshots below.
//!
//! Versions and tags (the line form of `edit`, PLAN.md T45): per path the
//! last [`MAX_VERSIONS`] versions the agent was shown or wrote are kept, each
//! with the rows of it that were shown. A version's tag is the first four hex
//! characters of its SHA-256, upper case ([`tag`]): a lookup key, checked
//! against the kept text, not a security boundary (the guard compares the full
//! hash). A tag names at most one kept version of a path.
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
/// Snapshot bytes kept over all sessions of the node, counting every kept
/// version's text and its shown-row ranges (8 bytes each); past it the
/// versions of files nobody read go first (kept by a `grep` or a refusal,
/// never by a `read`, `write` or `edit` of the whole), then the least
/// recently used versions, the oldest of an entry first (the hashes stay). A range needs at least
/// one row between it and the next, and a row is at least one byte, so the ranges of a version are at most
/// 4 bytes per text byte: counted, they cannot add to the worst case, which
/// stays this constant (plus the entries' overhead below).
pub const SNAPSHOT_MAX_TOTAL: usize = 8 << 20;
/// Versions of one file kept per session (the newest this many that had a
/// snapshot; the one the guard holds is the last to go, then the oldest).
pub const MAX_VERSIONS: usize = 4;
/// Files remembered per session; past it one nobody read goes first (a
/// search's or a refusal's), then the least recently used (the agent then
/// has to read it again).
pub const MAX_FILES: usize = 1024;
/// Files remembered over all sessions of the node, those nobody read and then the least recently used
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

/// The tag of a version: the first four hex characters of its SHA-256, upper
/// case.
pub fn tag(sha: &Sha) -> String {
    format!("{:02X}{:02X}", sha[0], sha[1])
}

/// Row ranges, inclusive and 1-based, sorted and apart (never touching).
pub type Rows = Vec<(u32, u32)>;

/// Add rows `lo..=hi` (1-based) to `rows`, merging what touches.
pub fn add_rows(rows: &mut Rows, lo: usize, hi: usize) {
    if lo == 0 || hi < lo {
        return;
    }
    let (lo, hi) = (
        lo.min(u32::MAX as usize) as u32,
        hi.min(u32::MAX as usize) as u32,
    );
    // The common case, rows in order: no rebuild.
    match rows.last_mut() {
        None => rows.push((lo, hi)),
        Some(l) if lo > l.1.saturating_add(1) => rows.push((lo, hi)),
        Some(l) if lo >= l.0 => l.1 = l.1.max(hi),
        _ => merge_in(rows, vec![(lo, hi)]),
    }
}

/// Add every range of `new` (1-based, inclusive; empty and row-0 ranges are
/// skipped) to `rows`: one sort of `new` and one linear merge, not a rebuild
/// per range.
pub fn add_ranges(rows: &mut Rows, new: &[(usize, usize)]) {
    let clamp = |n: usize| n.min(u32::MAX as usize) as u32;
    let inc: Vec<(u32, u32)> = new
        .iter()
        .filter(|&&(a, b)| a >= 1 && b >= a)
        .map(|&(a, b)| (clamp(a), clamp(b)))
        .collect();
    merge_in(rows, inc);
}

/// Merge `inc` (any order) into `rows` (sorted and apart), keeping it so.
fn merge_in(rows: &mut Rows, mut inc: Vec<(u32, u32)>) {
    if inc.is_empty() {
        return;
    }
    inc.sort_unstable();
    let mut out = Rows::with_capacity(rows.len() + inc.len());
    let (mut i, mut j) = (0, 0);
    while i < rows.len() || j < inc.len() {
        let next = if j >= inc.len() || (i < rows.len() && rows[i] <= inc[j]) {
            i += 1;
            rows[i - 1]
        } else {
            j += 1;
            inc[j - 1]
        };
        if let Some(l) = out.last_mut()
            && next.0 <= l.1.saturating_add(1)
        {
            l.1 = l.1.max(next.1);
        } else {
            out.push(next);
        }
    }
    *rows = out;
}

/// One version of a file the agent was shown or wrote.
#[derive(Debug, Clone)]
pub struct Version {
    pub sha: Sha,
    /// Its text.
    pub text: Arc<String>,
    /// The rows of it the agent was shown (a row cut with `…` is not one).
    shown: Rows,
}

impl Version {
    /// Every row `lo..=hi` (1-based) was shown.
    pub fn covers(&self, lo: usize, hi: usize) -> bool {
        let (lo, hi) = (lo as u64, hi as u64);
        lo > hi
            || self
                .shown
                .iter()
                .any(|&(a, b)| u64::from(a) <= lo && hi <= u64::from(b))
    }

    fn bytes(&self) -> usize {
        self.text.len() + self.shown.len() * std::mem::size_of::<(u32, u32)>()
    }
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
    /// The hash the guard compares (the latest `read`, `write` or `edit`).
    sha: Sha,
    /// The guard holds `sha`: the agent read or wrote the file. An entry
    /// that only keeps versions a refusal printed is not a read of it.
    guarded: bool,
    /// Kept versions, the newest last (at most [`MAX_VERSIONS`], at most
    /// one per tag).
    vers: Vec<Version>,
    used: u64,
}

impl Entry {
    fn seen(&self) -> Seen {
        Seen {
            sha: self.sha,
            snapshot: self
                .vers
                .iter()
                .rev()
                .find(|v| v.sha == self.sha)
                .map(|v| v.text.clone()),
        }
    }

    /// Keep `text` (hash `sha`) as the newest version with `rows` shown:
    /// the same hash kept before has its rows merged in, an older version
    /// of the same tag goes, and only the newest [`MAX_VERSIONS`] stay.
    fn keep(&mut self, sha: Sha, text: Arc<String>, mut rows: Rows) {
        if let Some(i) = self.vers.iter().position(|v| v.sha == sha) {
            let old = self.vers.remove(i);
            merge_in(&mut rows, old.shown);
        }
        let t = tag(&sha);
        self.vers.retain(|v| tag(&v.sha) != t);
        self.vers.push(Version {
            sha,
            text,
            shown: rows,
        });
        while self.vers.len() > MAX_VERSIONS {
            let i = self.victim().unwrap_or(0);
            self.vers.remove(i);
        }
    }

    /// The oldest version that is not the one the guard holds (a file nobody
    /// read has none such held): a version nobody read goes before the text
    /// of what the agent really read, whatever bound is acting.
    fn victim(&self) -> Option<usize> {
        self.vers
            .iter()
            .position(|v| !(self.guarded && v.sha == self.sha))
    }
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
        e.vers.iter().map(Version::bytes).sum()
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
            // Entries nobody read first, then the least recently used.
            let oldest = self
                .sessions
                .iter()
                .flat_map(|(s, t)| t.files.iter().map(move |(p, e)| (e.guarded, e.used, *s, p)))
                .min_by_key(|(guarded, used, _, _)| (*guarded, *used))
                .map(|(_, _, s, p)| (s, p.clone()));
            let Some((sid, path)) = oldest else { break };
            if let Some(t) = self.sessions.get_mut(&sid)
                && let Some(e) = t.files.remove(&path)
            {
                self.snap_bytes -= Self::snap_len(&e);
                self.entries -= 1;
            }
        }
        while self.snap_bytes > SNAPSHOT_MAX_TOTAL {
            // A version nobody read (a search's rows, a refusal's, an older
            // text of a file) goes before any text the guard holds: what the
            // agent really read is worth more than what it only saw listed.
            // Among those, the oldest of the least recently used file.
            let spare = self
                .sessions
                .values_mut()
                .flat_map(|t| t.files.values_mut())
                .filter(|e| e.victim().is_some())
                .min_by_key(|e| e.used);
            if let Some(e) = spare {
                let i = e.victim().unwrap_or(0);
                self.snap_bytes -= e.vers.remove(i).bytes();
                continue;
            }
            // Only held texts are left: the least recently used file's oldest.
            let oldest = self
                .sessions
                .values_mut()
                .flat_map(|t| t.files.values_mut())
                .filter(|e| !e.vers.is_empty())
                .min_by_key(|e| e.used);
            match oldest {
                Some(e) => self.snap_bytes -= e.vers.remove(0).bytes(),
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
        let mut take = |s: &serde_json::Value, seq: u64| {
            if let Ok(f) = serde_json::from_value::<FileSeen>(s.clone())
                && let Some(sha) = unhex(&f.sha256)
            {
                latest.insert(PathBuf::from(&f.path), (sha, seq));
            }
        };
        for e in history {
            if e.kind == "tool_call_finished"
                && let Some(s) = e.payload.get("seen")
            {
                take(s, e.seq);
            }
            // A file the person mentioned (`@path`, T63) that the node read
            // all of and showed, as a `read` would record it.
            if e.kind == "user_message"
                && let Some(files) = e.payload.get("files").and_then(|f| f.as_array())
            {
                for s in files.iter().filter_map(|f| f.get("seen")) {
                    take(s, e.seq);
                }
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
        // Kept versions carry over where the log's latest hash of the file
        // is one of them (a version shown but not logged stays with it).
        let mut old: HashMap<PathBuf, Vec<Version>> = inner
            .sessions
            .get(&session)
            .map(|t| {
                t.files
                    .iter()
                    .map(|(p, e)| (p.clone(), e.vers.clone()))
                    .collect()
            })
            .unwrap_or_default();
        inner.drop_table(session);
        let mut table = Table {
            files: HashMap::new(),
            used: now,
        };
        for (path, (sha, _)) in latest {
            let vers = old
                .remove(&path)
                .filter(|v| v.iter().any(|x| x.sha == sha))
                .unwrap_or_default();
            table.files.insert(
                path,
                Entry {
                    sha,
                    guarded: true,
                    vers,
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
        e.guarded.then(|| e.seen())
    }

    /// The kept version of `path` whose tag is `tag` (any letter case).
    pub fn version(&self, path: &Path, tag: &str) -> Option<Version> {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let t = inner.sessions.get_mut(&self.session)?;
        t.used = now;
        let e = t.files.get_mut(path)?;
        e.used = now;
        e.vers
            .iter()
            .find(|v| self::tag(&v.sha).eq_ignore_ascii_case(tag))
            .cloned()
    }

    /// Record that the agent now knows `path` as `bytes` (a snapshot is
    /// kept when it is small text). Returns the event's record of it. No
    /// row of it counts as shown: [`Self::add_shown`] says which.
    pub fn record(&self, path: &Path, bytes: &[u8]) -> FileSeen {
        let (sha, snapshot) = Self::snap(bytes);
        self.put(path, sha, snapshot, true)
    }

    /// Keep `bytes` as a version of `path`, without changing the hash the
    /// guard compares and without making one: rows shown of a file that
    /// changed since it was read, or of a file never read, must not count as
    /// reading all of it (`write` and the `old_string` form still ask for a
    /// `read`).
    pub fn add_version(&self, path: &Path, bytes: &[u8]) {
        let (sha, snapshot) = Self::snap(bytes);
        self.put(path, sha, snapshot, false);
    }

    /// Count `rows` (1-based, inclusive) as shown of the kept version of
    /// `path` with hash `sha`. The proof is the finished text of the result
    /// that printed them, which the result cap does not cut
    /// ([`crate::shown::seal`]): a row that is not in the text the model
    /// receives is not one it was shown.
    pub(crate) fn add_shown(
        &self,
        path: &Path,
        sha: &Sha,
        rows: &[(usize, usize)],
        _proof: &crate::shown::Sealed,
    ) {
        let mut inner = self.store.lock();
        let Some(e) = inner
            .sessions
            .get_mut(&self.session)
            .and_then(|t| t.files.get_mut(path))
        else {
            return;
        };
        let Some(v) = e.vers.iter_mut().find(|v| v.sha == *sha) else {
            return;
        };
        let before = v.bytes();
        add_ranges(&mut v.shown, rows);
        let after = v.bytes();
        inner.snap_bytes = (inner.snap_bytes + after).saturating_sub(before);
        inner.evict();
    }

    /// The newest kept version of `path` that has rows shown, if any: what
    /// the agent last saw of a file it never read (a search's rows, a
    /// refusal's).
    pub fn shown_version(&self, path: &Path) -> Option<Version> {
        let inner = self.store.lock();
        inner
            .sessions
            .get(&self.session)?
            .files
            .get(path)?
            .vers
            .iter()
            .rev()
            .find(|v| !v.shown.is_empty())
            .cloned()
    }

    fn snap(bytes: &[u8]) -> (Sha, Option<Arc<String>>) {
        let snapshot = (bytes.len() <= SNAPSHOT_MAX_FILE)
            .then(|| std::str::from_utf8(bytes).ok())
            .flatten()
            .map(|s| Arc::new(s.to_string()));
        (sha256(bytes), snapshot)
    }

    /// Record what the turn's own message says the agent knows (a file
    /// the person mentioned, T63): its hash only, keyed on its path as
    /// given. It is newer than anything the table holds at the turn's
    /// start. A record that does not parse is skipped.
    pub fn note(&self, f: &FileSeen) {
        if let Some(sha) = unhex(&f.sha256) {
            self.put(Path::new(&f.path), sha, None, true);
        }
    }

    /// [`Self::note`] for a message queued while a turn ran: only where
    /// the table holds nothing of the file, so an older hash never
    /// replaces a newer `read` or `write` (review of PR 120). An older
    /// record kept means at worst "read it again", never a wrong accept;
    /// the next turn's [`SeenStore::sync`] orders them by the log.
    pub fn note_if_unknown(&self, f: &FileSeen) {
        if self.get(Path::new(&f.path)).is_none() {
            self.note(f);
        }
    }

    /// [`Self::record`] for a file too large to hold: its hash only.
    pub fn record_hash(&self, path: &Path, sha: Sha) -> FileSeen {
        self.put(path, sha, None, true)
    }

    /// `current`: the guard's hash becomes `sha`; otherwise only the
    /// version is kept.
    fn put(&self, path: &Path, sha: Sha, snapshot: Option<Arc<String>>, current: bool) -> FileSeen {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let t = inner.sessions.entry(self.session).or_default();
        t.used = now;
        let fresh = !t.files.contains_key(path);
        let e = t.files.entry(path.to_path_buf()).or_insert_with(|| Entry {
            sha,
            guarded: current,
            vers: Vec::new(),
            used: now,
        });
        let before = Inner::snap_len(e);
        e.used = now;
        if current {
            e.sha = sha;
            e.guarded = true;
        }
        if let Some(text) = snapshot {
            e.keep(sha, text, Rows::new());
        }
        let after = Inner::snap_len(e);
        let mut snap = inner.snap_bytes + after - before;
        let mut entries = inner.entries + usize::from(fresh);
        let t = inner.sessions.get_mut(&self.session).expect("just made");
        if t.files.len() > MAX_FILES
            // Entries nobody read first, then the least recently used.
            && let Some(p) = t
                .files
                .iter()
                .min_by_key(|(_, e)| (e.guarded, e.used))
                .map(|(p, _)| p.clone())
            && let Some(e) = t.files.remove(&p)
        {
            snap -= Inner::snap_len(&e);
            entries -= 1;
        }
        inner.snap_bytes = snap;
        inner.entries = entries;
        inner.evict();
        FileSeen {
            path: path.display().to_string(),
            sha256: hex(&sha),
            base: None,
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

    impl SessionSeen {
        fn mark(&self, p: &Path, sha: &Sha, rows: &[(usize, usize)]) {
            self.add_shown(p, sha, rows, &crate::shown::Sealed::model_wrote());
        }

        fn record_rows(&self, p: &Path, bytes: &[u8], rows: &[(usize, usize)]) -> FileSeen {
            let f = self.record(p, bytes);
            self.mark(p, &sha256(bytes), rows);
            f
        }

        fn version_rows(&self, p: &Path, bytes: &[u8], rows: &[(usize, usize)]) {
            self.add_version(p, bytes);
            self.mark(p, &sha256(bytes), rows);
        }
    }

    /// Review of PR 120: a queued message's older hash of a file does not
    /// replace a newer record of it; the turn's own message's does.
    #[test]
    fn a_queued_mention_never_replaces_a_newer_record() {
        let t = SessionSeen::default();
        let p = Path::new("/w/a.rs");
        let newer = t.record(p, b"written by the turn");
        let old = FileSeen {
            path: "/w/a.rs".into(),
            sha256: hex(&sha256(b"as the person sent it")),
            base: None,
        };
        t.note_if_unknown(&old);
        assert_eq!(hex(&t.get(p).unwrap().sha), newer.sha256);
        t.note(&old);
        assert_eq!(hex(&t.get(p).unwrap().sha), old.sha256);
        let q = Path::new("/w/b.rs");
        t.note_if_unknown(&FileSeen {
            path: "/w/b.rs".into(),
            ..old.clone()
        });
        assert_eq!(hex(&t.get(q).unwrap().sha), old.sha256);
    }
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
            image: None,
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

    /// Two texts whose tags are the same (16 bits: found by counting).
    fn colliding() -> (String, String) {
        let mut seen: HashMap<String, String> = HashMap::new();
        for i in 0.. {
            let t = format!("text {i}\n");
            let k = tag(&sha256(t.as_bytes()));
            if let Some(o) = seen.insert(k, t.clone()) {
                return (o, t);
            }
        }
        unreachable!()
    }

    #[test]
    fn a_tag_names_at_most_one_kept_version_of_a_path() {
        let (x, y) = colliding();
        assert_ne!(sha256(x.as_bytes()), sha256(y.as_bytes()));
        let t = tag(&sha256(x.as_bytes()));
        assert_eq!(t, tag(&sha256(y.as_bytes())));
        let s = SessionSeen::default();
        let p = Path::new("/w/a");
        s.record_rows(p, x.as_bytes(), &[(1, 1)]);
        assert_eq!(*s.version(p, &t).unwrap().text, x);
        // The newer one of the same tag replaces the older.
        s.record_rows(p, y.as_bytes(), &[]);
        let v = s.version(p, &t).unwrap();
        assert_eq!(*v.text, y);
        assert!(!v.covers(1, 1), "the older one's rows did not carry over");
        // Another path may hold either.
        s.record(Path::new("/w/b"), x.as_bytes());
        assert_eq!(*s.version(Path::new("/w/b"), &t).unwrap().text, x);
        // A lower-case tag finds it too.
        assert!(s.version(p, &t.to_lowercase()).is_some());
    }

    #[test]
    fn only_the_newest_versions_are_kept_and_their_rows_are_remembered() {
        let s = SessionSeen::default();
        let p = Path::new("/w/a");
        let texts: Vec<String> = (0..MAX_VERSIONS + 3)
            .map(|i| format!("v{i}\nrow\n"))
            .collect();
        for t in &texts {
            s.record_rows(p, t.as_bytes(), &[(1, 1)]);
        }
        let kept = |t: &String| s.version(p, &tag(&sha256(t.as_bytes()))).is_some();
        let n = texts.iter().filter(|t| kept(t)).count();
        assert_eq!(n, MAX_VERSIONS);
        assert!(texts[texts.len() - MAX_VERSIONS..].iter().all(kept));
        // The same text again merges its rows instead of taking a slot.
        let last = texts.last().unwrap();
        s.record_rows(p, last.as_bytes(), &[(2, 2)]);
        let v = s.version(p, &tag(&sha256(last.as_bytes()))).unwrap();
        assert!(v.covers(1, 2) && !v.covers(1, 3));
        assert_eq!(texts.iter().filter(|t| kept(t)).count(), MAX_VERSIONS);
        // The guard's hash is the newest recorded.
        assert_eq!(s.get(p).unwrap().sha, sha256(last.as_bytes()));
    }

    /// Review of PR 187: every range added out of order rebuilt the whole
    /// list (8,000 single rows took 233 ms), so a call with many ranges was
    /// quadratic.
    #[test]
    fn many_ranges_added_out_of_order_are_merged_in_linear_time() {
        let p = Path::new("/w/big.txt");
        let n = 20_000usize;
        // Every other row, last first: none touches another.
        let ranges: Vec<(usize, usize)> = (1..=n).rev().map(|i| (i * 2, i * 2)).collect();
        let s = SessionSeen::default();
        let start = std::time::Instant::now();
        s.record_rows(p, b"x\n", &ranges);
        let took = start.elapsed();
        let sha = sha256(b"x\n");
        let v = s.version(p, &tag(&sha)).unwrap();
        assert_eq!(v.shown.len(), n);
        assert!(
            v.shown.windows(2).all(|w| w[0].1 + 1 < w[1].0),
            "sorted, apart"
        );
        let start = std::time::Instant::now();
        s.mark(p, &sha, &ranges);
        let again = start.elapsed();
        let mut touching: Vec<(usize, usize)> =
            (1..=n).rev().map(|i| (i * 2 + 1, i * 2 + 1)).collect();
        touching.push((1, 1));
        s.mark(p, &sha, &touching);
        let v = s.version(p, &tag(&sha)).unwrap();
        assert_eq!(v.shown, vec![(1, (n * 2 + 1) as u32)]);
        assert!(
            took < std::time::Duration::from_millis(250)
                && again < std::time::Duration::from_millis(250),
            "{took:?} {again:?}"
        );
    }

    #[test]
    fn row_ranges_merge_and_cover() {
        let mut r = Rows::new();
        add_rows(&mut r, 5, 7);
        add_rows(&mut r, 1, 2);
        add_rows(&mut r, 9, 9);
        assert_eq!(r, vec![(1, 2), (5, 7), (9, 9)]);
        add_rows(&mut r, 3, 4);
        assert_eq!(r, vec![(1, 7), (9, 9)]);
        add_rows(&mut r, 8, 8);
        assert_eq!(r, vec![(1, 9)]);
        add_rows(&mut r, 0, 0);
        add_rows(&mut r, 4, 3);
        assert_eq!(r, vec![(1, 9)]);
        let v = Version {
            sha: sha256(b""),
            text: Arc::new(String::new()),
            shown: r,
        };
        assert!(v.covers(1, 9) && !v.covers(1, 10) && v.covers(5, 4));
    }

    #[test]
    fn kept_versions_and_their_rows_are_bounded_over_the_node() {
        let s = Arc::new(SeenStore::default());
        // Many versions of big files, rows scattered: counted, so bounded.
        let sid = SessionId::new();
        for f in 0..4 {
            for v in 0..MAX_VERSIONS + 2 {
                let mut text = String::new();
                let rows = SNAPSHOT_MAX_FILE / 2 - 1;
                for i in 0..rows {
                    text.push_str(if i % 2 == 0 { "a\n" } else { "\n" });
                }
                text.push_str(&format!("{f} {v}\n"));
                let scattered: Vec<(usize, usize)> = (1..rows).step_by(2).map(|r| (r, r)).collect();
                s.session(sid).record_rows(
                    &PathBuf::from(format!("/w/{f}")),
                    text.as_bytes(),
                    &scattered,
                );
                assert!(s.lock().snap_bytes <= SNAPSHOT_MAX_TOTAL);
            }
        }
        let counted: usize = s
            .lock()
            .sessions
            .values()
            .flat_map(|t| t.files.values())
            .map(Inner::snap_len)
            .sum();
        assert_eq!(counted, s.lock().snap_bytes);
        assert!(counted <= SNAPSHOT_MAX_TOTAL);
        // Never more than MAX_VERSIONS of one file.
        assert!(
            s.lock()
                .sessions
                .values()
                .flat_map(|t| t.files.values())
                .all(|e| e.vers.len() <= MAX_VERSIONS)
        );
    }

    /// Versions nobody read (a search's, a refusal's) go before any read
    /// file's when the node's snapshot bound is reached: a wide search must
    /// not push out what the agent really read.
    #[test]
    fn versions_nobody_read_are_evicted_before_read_ones() {
        let s = Arc::new(SeenStore::default());
        let sid = SessionId::new();
        let t = s.session(sid);
        let reads = SNAPSHOT_MAX_TOTAL / SNAPSHOT_MAX_FILE - 1;
        for i in 0..reads {
            t.record(
                &PathBuf::from(format!("/w/read{i}")),
                &vec![b'r'; SNAPSHOT_MAX_FILE],
            );
        }
        // More than the room that is left, newer than every read.
        for i in 0..4 {
            t.version_rows(
                &PathBuf::from(format!("/w/found{i}")),
                &vec![b'f'; SNAPSHOT_MAX_FILE],
                &[(1, 1)],
            );
            assert!(s.lock().snap_bytes <= SNAPSHOT_MAX_TOTAL);
        }
        for i in 0..reads {
            let got = t.get(&PathBuf::from(format!("/w/read{i}"))).unwrap();
            assert!(got.snapshot.is_some(), "read{i} lost its snapshot");
        }
        // And the other way: a read of a searched file makes it a read one.
        let p = PathBuf::from("/w/found3");
        t.record(&p, &vec![b'f'; SNAPSHOT_MAX_FILE]);
        assert!(t.get(&p).unwrap().snapshot.is_some());
    }

    /// Review of PR 188: one read file, then 1,029 entries a search had
    /// added, and the read file was unknown: the per-session bound took the
    /// least recently used entry whatever it was.
    #[test]
    fn a_flood_of_unread_entries_does_not_push_out_a_read_file() {
        let s = SessionSeen::default();
        let p = PathBuf::from("/w/read_me");
        s.record(&p, b"hello\n");
        for i in 0..MAX_FILES + 5 {
            s.version_rows(&PathBuf::from(format!("/w/found{i}")), b"x\n", &[(1, 1)]);
        }
        assert!(s.get(&p).is_some(), "the read file's guard was dropped");
        // The same over the node's bound on entries.
        let store = Arc::new(SeenStore::default());
        let reader = store.session(SessionId::new());
        reader.record(&p, b"hello\n");
        for _ in 0..8 {
            let t = store.session(SessionId::new());
            for i in 0..MAX_ENTRIES / 4 {
                t.version_rows(&PathBuf::from(format!("/w/found{i}")), b"x\n", &[(1, 1)]);
            }
        }
        assert!(store.lock().entries <= MAX_ENTRIES);
        assert!(
            reader.get(&p).is_some(),
            "the read file's guard was dropped"
        );
    }

    /// Review of PR 188: with four versions kept per path, a read file's own
    /// snapshot was the first to go while versions nobody read stayed.
    #[test]
    fn the_cap_of_versions_does_not_drop_the_guards_snapshot() {
        let s = SessionSeen::default();
        let p = PathBuf::from("/w/a");
        s.record(&p, b"orig\n");
        for i in 0..MAX_VERSIONS + 3 {
            s.version_rows(&p, format!("changed {i}\n").as_bytes(), &[(1, 1)]);
            let g = s.get(&p).unwrap();
            assert!(g.snapshot.is_some(), "lost after {i} other versions");
        }
        // The byte bound takes the same order: a version nobody read first.
        let store = Arc::new(SeenStore::default());
        let t = store.session(SessionId::new());
        let (big1, big2) = (vec![b'1'; 400_000], vec![b'2'; 400_000]);
        t.record(&p, &big1);
        t.version_rows(&p, &big2, &[(1, 1)]);
        for i in 0..19 {
            t.record(&PathBuf::from(format!("/w/other{i}")), &vec![b'o'; 400_000]);
        }
        assert!(store.lock().snap_bytes <= SNAPSHOT_MAX_TOTAL);
        assert!(
            t.get(&p).unwrap().snapshot.is_some(),
            "the byte bound dropped the guard's snapshot"
        );
    }

    #[test]
    fn a_rebuild_from_the_log_keeps_the_versions_of_a_file_the_log_names() {
        let sid = SessionId::new();
        let store = Arc::new(SeenStore::default());
        let t = store.session(sid);
        let p = Path::new("/w/a");
        let first = t.record_rows(p, b"one\n", &[(1, 1)]);
        t.record_rows(p, b"two\n", &[(1, 1)]);
        // The log holds only the first (the second's append failed, or it was a refusal's).
        store.sync(sid, &[raw(sid, 1, finished(first))]);
        let got = store.session(sid).get(p).unwrap();
        assert_eq!(got.sha, sha256(b"one\n"));
        let v = store.session(sid).version(p, &tag(&sha256(b"two\n")));
        assert!(
            v.is_some_and(|v| v.covers(1, 1)),
            "rows shown stay with the version"
        );
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

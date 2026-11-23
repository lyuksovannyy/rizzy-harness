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
//! with the rows of it that were shown. A version's tag is the first six hex
//! characters of its SHA-256, upper case ([`tag`]), longer when a tag of that
//! path was issued before with the same characters (the shortest longer
//! prefix of the hash that was not): a lookup key, checked against the kept
//! text, not a security boundary (the guard compares the full hash). A tag,
//! once issued for a path in a session, never names another version of it:
//! every kept version stores its tag, and the tags of versions that left the
//! table are remembered per path ([`MAX_GONE_TAGS`]) so no new version takes
//! one. A tag is compared as a whole string, never as a prefix.
//!
//! What this does not cover: the guarantee holds while the file's entry is in
//! the table and the node has not restarted. An entry dropped whole by the
//! bounds below forgets its tags, and after a restart the versions the log
//! does not hold (a search's, a refusal's) are forgotten: an old tag can then
//! equal a newer version's tag by chance, 1 in 16,777,216 per later version
//! (six hex characters), and its row numbers would land on that version.
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
/// Tags remembered per path for versions that left the table (the cap of
/// versions, the memory bounds, a rebuild), each with the hash it named: so
/// no new version takes a tag the model may still hold, and the same content
/// seen again gets the same tag back. Past it the oldest is forgotten, and
/// only then can a new version whose hash starts with that very tag take it:
/// it takes 17 evicted versions of one path in one session, then a chance of
/// one in 16,777,216 per new version, and a model that still holds the
/// forgotten tag from that long ago (the entry as a whole going, by the
/// bounds below, forgets all of its tags the same way). One is 33 bytes (the
/// hash and the tag's length: the tag is a prefix of the hash), so a full
/// list is 528 bytes, the `Vec` kept at exactly that capacity.
pub const MAX_GONE_TAGS: usize = 16;
/// The length of a tag before a collision lengthens it: six hex characters,
/// 1 in 16,777,216 for two versions to share one (about one token in a
/// header; four characters were 1 in 65,536).
pub const TAG_LEN: usize = 6;
/// Files remembered per session; past it one nobody read goes first (a
/// search's or a refusal's), then the least recently used (the agent then
/// has to read it again).
pub const MAX_FILES: usize = 1024;
/// Files remembered over all sessions of the node, those nobody read and
/// then the least recently used forgotten first. Worst case per entry: a
/// path of `PATH_MAX` (4,096 bytes) plus [`ENTRY_OVERHEAD`], so 4,096
/// entries are at most about 22.1 MB, and with [`SNAPSHOT_MAX_TOTAL`] about
/// 30.5 MB (`the_worst_case_of_the_table_is_under_the_idle_rule` prints and
/// bounds it): about 19 MB under the 50 MB idle rule even if every path were
/// as long as allowed, every entry kept four versions and had used all its
/// remembered tags (a typical path of 100 bytes makes it about 6 MB).
pub const MAX_ENTRIES: usize = 4096;
/// Bytes of one entry besides its path and its snapshot text, at the worst:
/// its map bucket (a `PathBuf`, the `Entry` of 96 bytes and a control byte,
/// at the worst load of 16/7 buckets per entry: 276), the four kept versions
/// (`Version` is 88 bytes: 352) with their tags (32 each as the allocator
/// gives them: 128), and the remembered tags (16 of 33 bytes: 528), 1,284
/// bytes; this constant is the sum rounded up. Checked against the real
/// `size_of` values by `the_worst_case_entry_is_accounted_as_the_comments_say`,
/// which fails when either side moves.
pub const ENTRY_OVERHEAD: usize = 1_300;
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

/// The base tag of a version: the first six hex characters of its SHA-256,
/// upper case. The tag a version really has (it is longer after a collision)
/// is [`Version::tag`].
pub fn tag(sha: &Sha) -> String {
    prefix(sha, TAG_LEN)
}

/// Hex character `i` (0-based) of `sha`, upper case.
fn digit(sha: &Sha, i: usize) -> char {
    let b = sha[i / 2];
    let v = if i.is_multiple_of(2) { b >> 4 } else { b & 15 };
    char::from(b"0123456789ABCDEF"[usize::from(v)])
}

/// The first `n` (at most 64) hex characters of `sha`, upper case, in a
/// string of exactly that capacity (a tag is kept for as long as its entry).
pub(crate) fn prefix(sha: &Sha, n: usize) -> String {
    let n = n.min(64);
    let mut out = String::with_capacity(n);
    out.extend((0..n).map(|i| digit(sha, i)));
    out
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
    // The room asked for above is the most the merge can need; what it did
    // not use is given back, so a version's rows hold what
    // [`Version::bytes`] counts (review of PR 196: 4,000 ranges that one
    // read then joined into one left room for 4,001).
    out.shrink_to_fit();
    *rows = out;
}

/// One version of a file the agent was shown or wrote.
#[derive(Debug, Clone)]
pub struct Version {
    pub sha: Sha,
    /// Its tag, as printed in headers: unique among the tags issued for the
    /// path (upper case).
    pub tag: String,
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
    /// one per hash and one per tag).
    vers: Vec<Version>,
    /// Tags issued for versions not kept, the oldest first (at most
    /// [`MAX_GONE_TAGS`]).
    gone: Vec<Issued>,
    used: u64,
}

/// A tag issued for a hash: the tag is the first `len` hex characters of
/// the hash, so the length is all that is stored (33 bytes, no allocation).
#[derive(Debug, Clone)]
struct Issued {
    sha: Sha,
    len: u8,
}

impl Issued {
    fn tag(&self) -> String {
        prefix(&self.sha, usize::from(self.len))
    }

    /// The tag is exactly `tag` (upper case), compared without making it.
    fn is(&self, tag: &str) -> bool {
        tag.len() == usize::from(self.len)
            && tag
                .bytes()
                .enumerate()
                .all(|(i, c)| digit(&self.sha, i) as u8 == c)
    }
}

/// Push to a `Vec` that is kept at its exact capacity: a push that must grow
/// asks for one place, not for twice the room.
fn push_exact<T>(v: &mut Vec<T>, x: T) {
    if v.len() == v.capacity() {
        v.reserve_exact(1);
    }
    v.push(x);
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

    fn taken(&self, tag: &str) -> bool {
        self.vers.iter().any(|v| v.tag == tag) || self.gone.iter().any(|g| g.is(tag))
    }

    /// The shortest prefix of `sha` (from `base` characters) that no tag
    /// issued for this path equals.
    fn fresh(&self, sha: &Sha, base: usize) -> String {
        (base.max(1)..=64)
            .map(|n| prefix(sha, n))
            .find(|t| !self.taken(t))
            .unwrap_or_else(|| prefix(sha, 64))
    }

    /// Remember a tag of `len` characters whose version is not kept,
    /// forgetting the oldest past the bound (before the push: the `Vec` never
    /// holds more than the bound, nor room for more).
    fn remember(&mut self, sha: Sha, len: usize) {
        self.gone.retain(|g| g.sha != sha);
        if self.gone.len() >= MAX_GONE_TAGS {
            self.gone.remove(0);
        }
        push_exact(
            &mut self.gone,
            Issued {
                sha,
                len: len.min(64) as u8,
            },
        );
    }

    /// The tag of `sha` on this path: the one it has (kept or remembered),
    /// else a new one, remembered at once, so that it is never given to
    /// another version, whatever is recorded next.
    fn issue(&mut self, sha: &Sha, base: usize) -> String {
        if let Some(v) = self.vers.iter().find(|v| v.sha == *sha) {
            return v.tag.clone();
        }
        if let Some(g) = self.gone.iter().find(|g| g.sha == *sha) {
            return g.tag();
        }
        let t = self.fresh(sha, base);
        self.remember(*sha, t.len());
        t
    }

    /// Take version `i` out (its tag is remembered); the bytes it held.
    fn drop_version(&mut self, i: usize) -> usize {
        let v = self.vers.remove(i);
        let bytes = v.bytes();
        self.remember(v.sha, v.tag.len());
        bytes
    }

    /// Keep `text` (hash `sha`) as the newest version with `rows` shown:
    /// the same hash kept before has its rows merged in, it keeps the tag it
    /// was issued (kept or remembered), and only the newest
    /// [`MAX_VERSIONS`] stay.
    fn keep(&mut self, sha: Sha, text: Arc<String>, mut rows: Rows, base: usize) {
        let tag = if let Some(i) = self.vers.iter().position(|v| v.sha == sha) {
            let old = self.vers.remove(i);
            merge_in(&mut rows, old.shown);
            old.tag
        } else {
            self.issue(&sha, base)
        };
        self.gone.retain(|g| g.sha != sha);
        // Room first: the `Vec` never holds more than the bound, nor room
        // for more.
        while self.vers.len() >= MAX_VERSIONS {
            let i = self.victim().unwrap_or(0);
            self.drop_version(i);
        }
        push_exact(
            &mut self.vers,
            Version {
                sha,
                tag,
                text,
                shown: rows,
            },
        );
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

    /// Apply the bounds. `keep` is the entry the current call made or
    /// touched: it is never the victim of the bounds on entries (a search's
    /// or a refusal's entry is unread, so it had the lowest key and went
    /// before the call returned: its retry got the same refusal for ever).
    fn evict(&mut self, keep: Option<(SessionId, &Path)>) {
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
                .filter(|(_, _, s, p)| keep != Some((*s, p.as_path())))
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
                self.snap_bytes -= e.drop_version(i);
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
                Some(e) => self.snap_bytes -= e.drop_version(0),
                None => break,
            }
        }
    }
}

#[cfg(test)]
impl SessionSeen {
    /// `n` files read in this session (paths `/fill/N`).
    pub(crate) fn fill_read(&self, n: usize) {
        for i in 0..n {
            self.record(&PathBuf::from(format!("/fill/{i}")), b"x\n");
        }
    }

    /// Entries, snapshot bytes and tags issued, over the whole store: what a
    /// call that must record nothing leaves as it was.
    pub(crate) fn table_state(&self) -> (usize, usize, usize) {
        let inner = self.store.lock();
        let tags = inner
            .sessions
            .values()
            .flat_map(|t| t.files.values())
            .map(|e| e.vers.len() + e.gone.len())
            .sum();
        (inner.entries, inner.snap_bytes, tags)
    }
}

/// The tables of every session on this node.
pub struct SeenStore {
    inner: Mutex<Inner>,
    /// Characters of a tag before a collision lengthens it ([`TAG_LEN`];
    /// other only in the tests, which make collisions common).
    tag_len: usize,
}

impl Default for SeenStore {
    fn default() -> Self {
        Self {
            inner: Mutex::default(),
            tag_len: TAG_LEN,
        }
    }
}

impl SeenStore {
    /// A store whose base tags are `n` characters long: collisions in nearly
    /// every case for a small `n`.
    #[cfg(test)]
    pub(crate) fn with_tag_len(n: usize) -> Self {
        Self {
            tag_len: n,
            ..Self::default()
        }
    }

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
        // Every hash the log names per path, in log order: their tags are
        // issued again in that order, so a version has the tag it had.
        let mut named: HashMap<PathBuf, Vec<Sha>> = HashMap::new();
        let mut take = |s: &serde_json::Value, seq: u64| {
            if let Ok(f) = serde_json::from_value::<FileSeen>(s.clone())
                && let Some(sha) = unhex(&f.sha256)
            {
                let seq_of = named.entry(PathBuf::from(&f.path)).or_default();
                if seq_of.last() != Some(&sha) {
                    seq_of.push(sha);
                }
                latest.insert(PathBuf::from(&f.path), (sha, seq));
            }
        };
        // A fork starts with no file read: what its copied events saw is
        // what its parent saw, and the files may differ now (C.2).
        let copied = history
            .first()
            .and_then(|e| match e.typed() {
                Ok(harness_core::EventKind::SessionCreated(c)) => {
                    Some(harness_core::fork::through(&c))
                }
                _ => None,
            })
            .unwrap_or(0);
        for e in history
            .iter()
            .filter(|e| !harness_core::fork::inherited(copied, e.seq))
        {
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
        let base = self.tag_len;
        let mut inner = self.lock();
        let now = inner.tick();
        // Kept versions carry over where the log's latest hash of the file
        // is one of them (a version shown but not logged stays with it).
        // The tags issued before carry over with them, and those of the
        // versions that do not are remembered.
        let mut old: HashMap<PathBuf, (Vec<Version>, Vec<Issued>)> = inner
            .sessions
            .get(&session)
            .map(|t| {
                t.files
                    .iter()
                    .map(|(p, e)| (p.clone(), (e.vers.clone(), e.gone.clone())))
                    .collect()
            })
            .unwrap_or_default();
        inner.drop_table(session);
        let mut table = Table {
            files: HashMap::new(),
            used: now,
        };
        for (path, (sha, _)) in latest {
            let (vers, gone) = old.remove(&path).unwrap_or_default();
            let mut e = Entry {
                sha,
                guarded: true,
                vers: Vec::new(),
                gone,
                used: now,
            };
            if vers.iter().any(|x| x.sha == sha) {
                e.vers = vers;
            } else {
                for v in vers {
                    e.remember(v.sha, v.tag.len());
                }
            }
            for h in named.remove(&path).unwrap_or_default() {
                if !e.vers.iter().any(|v| v.sha == h) && !e.gone.iter().any(|g| g.sha == h) {
                    let t = e.fresh(&h, base);
                    e.remember(h, t.len());
                }
            }
            table.files.insert(path, e);
        }
        // A file the log does not name (a search's, a refusal's) is not read
        // as far as the table goes, and its texts go; its tags stay, so no
        // later version of it takes one the model may still hold.
        for (path, (vers, gone)) in old {
            if table.files.len() >= MAX_FILES {
                break;
            }
            let Some(sha) = vers.last().map(|v| v.sha).or(gone.last().map(|g| g.sha)) else {
                continue;
            };
            let mut e = Entry {
                sha,
                guarded: false,
                vers: Vec::new(),
                gone,
                used: now,
            };
            for v in vers {
                e.remember(v.sha, v.tag.len());
            }
            table.files.insert(path, e);
        }
        inner.snap_bytes += table.files.values().map(Inner::snap_len).sum::<usize>();
        inner.entries += table.files.len();
        inner.sessions.insert(session, table);
        inner.evict(None);
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

    /// The kept version of `path` whose tag is `tag` (any letter case), the
    /// whole string: a prefix of a longer tag, or a longer string that starts
    /// with a tag, names nothing.
    pub fn version(&self, path: &Path, tag: &str) -> Option<Version> {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let t = inner.sessions.get_mut(&self.session)?;
        t.used = now;
        let e = t.files.get_mut(path)?;
        e.used = now;
        e.vers
            .iter()
            .find(|v| v.tag.eq_ignore_ascii_case(tag))
            .cloned()
    }

    /// The kept version of `path` with hash `sha`.
    pub fn version_of(&self, path: &Path, sha: &Sha) -> Option<Version> {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let t = inner.sessions.get_mut(&self.session)?;
        t.used = now;
        let e = t.files.get_mut(path)?;
        e.used = now;
        e.vers.iter().find(|v| v.sha == *sha).cloned()
    }

    /// The tag that `sha` has, or will have once recorded, on `path`: for a
    /// header printed before the version is. It is issued here for good (a
    /// later version never takes it), and the version recorded afterwards
    /// has it.
    pub fn tag_for(&self, path: &Path, sha: &Sha) -> String {
        let base = self.store.tag_len;
        self.update(path, *sha, false, |e| e.issue(sha, base))
    }

    /// The tag the kept version of `path` with hash `sha` has, if there is
    /// one: nothing is issued or recorded (for what a person's card, or a
    /// refusal that prints no rows, may say).
    pub fn peek_tag(&self, path: &Path, sha: &Sha) -> Option<String> {
        let inner = self.store.lock();
        inner
            .sessions
            .get(&self.session)?
            .files
            .get(path)?
            .vers
            .iter()
            .find(|v| v.sha == *sha)
            .map(|v| v.tag.clone())
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
        inner.evict(Some((self.session, path)));
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
        let base = self.store.tag_len;
        self.update(path, sha, current, |e| {
            if let Some(text) = snapshot {
                e.keep(sha, text, Rows::new(), base);
            }
        });
        FileSeen {
            path: path.display().to_string(),
            sha256: hex(&sha),
            base: None,
        }
    }

    /// Run `f` on `path`'s entry (made if there is none), with the bounds
    /// applied afterwards.
    fn update<R>(
        &self,
        path: &Path,
        sha: Sha,
        current: bool,
        f: impl FnOnce(&mut Entry) -> R,
    ) -> R {
        let mut inner = self.store.lock();
        let now = inner.tick();
        let t = inner.sessions.entry(self.session).or_default();
        t.used = now;
        let fresh = !t.files.contains_key(path);
        let e = t.files.entry(path.to_path_buf()).or_insert_with(|| Entry {
            sha,
            guarded: current,
            vers: Vec::new(),
            gone: Vec::new(),
            used: now,
        });
        let before = Inner::snap_len(e);
        e.used = now;
        if current {
            e.sha = sha;
            e.guarded = true;
        }
        let out = f(e);
        let after = Inner::snap_len(e);
        let mut snap = inner.snap_bytes + after - before;
        let mut entries = inner.entries + usize::from(fresh);
        let t = inner.sessions.get_mut(&self.session).expect("just made");
        if t.files.len() > MAX_FILES
            // Entries nobody read first, then the least recently used.
            && let Some(p) = t
                .files
                .iter()
                .filter(|(p, _)| p.as_path() != path)
                .min_by_key(|(_, e)| (e.guarded, e.used))
                .map(|(p, _)| p.clone())
            && let Some(e) = t.files.remove(&p)
        {
            snap -= Inner::snap_len(&e);
            entries -= 1;
        }
        inner.snap_bytes = snap;
        inner.entries = entries;
        inner.evict(Some((self.session, path)));
        out
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
    /// Ranges that later join into one must not leave their room behind.
    #[test]
    fn merged_rows_hold_no_more_than_they_count() {
        let mut rows: Rows = Vec::new();
        for i in 0..4_000u32 {
            merge_in(&mut rows, vec![(2 * i + 1, 2 * i + 1)]);
        }
        assert_eq!(rows.len(), 4_000);
        assert_eq!(rows.capacity(), rows.len());
        merge_in(&mut rows, vec![(1, 8_000)]);
        assert_eq!(rows, vec![(1, 8_000)]);
        assert_eq!(rows.capacity(), 1);
    }

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
            made: None,
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

    /// ADOPTION_PLAN C.2: a fork starts with no file read. What its copied
    /// events saw is what its parent saw, and the files may differ now; its
    /// own reads count as any chat's.
    #[test]
    fn a_fork_starts_with_an_empty_table_and_its_own_reads_count() {
        let sid = SessionId::new();
        let parent = SessionId::new();
        let creation = RawEvent {
            session_id: sid,
            seq: 1,
            ts_ms: 0,
            kind: "session_created".into(),
            payload: serde_json::json!({
                "title": "t (fork)", "cwd": "/w", "model": "m", "backend": "b",
                "node_id": "n", "approval_mode": "ask", "profile": "agentic",
                "forked": {"parent": parent.to_string(), "through_seq": 4},
            }),
        };
        let store = Arc::new(SeenStore::default());
        let t = store.session(sid);
        let theirs = t.record(Path::new("/w/theirs"), b"parent's read\n");
        let own = t.record(Path::new("/w/own"), b"fork's read\n");
        let log = vec![
            creation,
            raw(sid, 2, finished(theirs)),
            raw(sid, 5, finished(own)),
        ];
        store.sync(sid, &log);
        assert!(
            store.session(sid).get(Path::new("/w/theirs")).is_none(),
            "a copied read is not a read here"
        );
        assert!(store.session(sid).get(Path::new("/w/own")).is_some());
        // A chat that is no fork keeps both.
        let mut plain = log.clone();
        plain[0].payload.as_object_mut().unwrap().remove("forked");
        store.sync(sid, &plain);
        assert!(store.session(sid).get(Path::new("/w/theirs")).is_some());
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

    /// The base tag at the 4 characters these tests use (a collision of
    /// two versions is then found by counting; at [`TAG_LEN`] it is not).
    fn tag4(sha: &Sha) -> String {
        prefix(sha, 4)
    }

    /// A table whose base tags are 4 characters long.
    fn short() -> SessionSeen {
        Arc::new(SeenStore::with_tag_len(4)).session(SessionId::new())
    }

    /// Two texts whose 4-character tags are the same (16 bits: found by
    /// counting).
    fn colliding() -> (String, String) {
        let mut seen: HashMap<String, String> = HashMap::new();
        for i in 0.. {
            let t = format!("text {i}\n");
            let k = tag4(&sha256(t.as_bytes()));
            if let Some(o) = seen.insert(k, t.clone()) {
                return (o, t);
            }
        }
        unreachable!()
    }

    /// The tag a kept version has.
    fn tag_now(s: &SessionSeen, p: &Path, text: &str) -> String {
        s.version_of(p, &sha256(text.as_bytes())).unwrap().tag
    }

    /// `n` texts that start no tag of `x` and fill the table's versions.
    fn fillers(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("filler {i}\nrow\n")).collect()
    }

    #[test]
    fn colliding_versions_both_stay_and_the_later_one_has_a_longer_tag() {
        let (x, y) = colliding();
        assert_ne!(sha256(x.as_bytes()), sha256(y.as_bytes()));
        let t = tag4(&sha256(x.as_bytes()));
        assert_eq!(t, tag4(&sha256(y.as_bytes())));
        let s = short();
        let p = Path::new("/w/a");
        s.record_rows(p, x.as_bytes(), &[(1, 1)]);
        assert_eq!(*s.version(p, &t).unwrap().text, x);
        s.record_rows(p, y.as_bytes(), &[]);
        // The older keeps its tag, rows and place.
        let v = s.version(p, &t).unwrap();
        assert_eq!(*v.text, x);
        assert!(v.covers(1, 1));
        let ty = tag_now(&s, p, &y);
        assert!(ty.len() == 5 && ty.starts_with(&t), "{ty}");
        let v = s.version(p, &ty).unwrap();
        assert_eq!(*v.text, y);
        assert!(!v.covers(1, 1), "the older one's rows did not carry over");
        // Another path may hold either with the short tag.
        s.record(Path::new("/w/b"), y.as_bytes());
        assert_eq!(
            *s.version(Path::new("/w/b"), &t).unwrap().text,
            y,
            "the tags of one path bind only that path"
        );
        // A lower-case tag finds it too, a prefix or an extension does not.
        assert!(s.version(p, &ty.to_lowercase()).is_some());
        assert!(s.version(p, &format!("{ty}0")).is_none());
    }

    #[test]
    fn the_same_content_again_has_its_tag_and_a_tag_of_a_gone_version_is_not_reused() {
        let (x, y) = colliding();
        let t = tag4(&sha256(x.as_bytes()));
        let s = short();
        let p = Path::new("/w/a");
        s.record_rows(p, x.as_bytes(), &[(1, 1)]);
        s.record_rows(p, y.as_bytes(), &[]);
        let ty = tag_now(&s, p, &y);
        // x -> y -> x: the same tag, the rows merge.
        s.record_rows(p, x.as_bytes(), &[(2, 2)]);
        assert_eq!(tag_now(&s, p, &x), t);
        assert_eq!(tag_now(&s, p, &y), ty);
        assert!(s.version(p, &t).unwrap().covers(1, 2));
        // Both leave the table (more versions than are kept).
        for f in fillers(MAX_VERSIONS) {
            s.record_rows(p, f.as_bytes(), &[(1, 1)]);
        }
        assert!(s.version(p, &t).is_none() && s.version(p, &ty).is_none());
        // A third text with their base tag: longer than both.
        let t3 = (0..)
            .map(|i| format!("third {i}\n"))
            .find(|z| tag4(&sha256(z.as_bytes())) == t)
            .unwrap();
        s.record(p, t3.as_bytes());
        let tz = tag_now(&s, p, &t3);
        assert!(tz.len() == 5 && tz != ty && tz.starts_with(&t), "{tz} {ty}");
        assert!(s.version(p, &t).is_none(), "the gone tag names nothing");
        // The content that left comes back with its own tag, and not the
        // new one's.
        s.record(p, x.as_bytes());
        assert_eq!(tag_now(&s, p, &x), t);
        assert_eq!(*s.version(p, &t).unwrap().text, x);
        assert_eq!(*s.version(p, &tz).unwrap().text, t3);
    }

    #[test]
    fn the_remembered_tags_are_bounded() {
        let s = SessionSeen::default();
        let p = Path::new("/w/a");
        for i in 0..MAX_VERSIONS + 3 * MAX_GONE_TAGS {
            s.record(p, format!("v{i}\n").as_bytes());
        }
        let g = s.store.lock().sessions[&s.session].files[p].gone.len();
        assert_eq!(g, MAX_GONE_TAGS);
    }

    #[test]
    fn a_tag_is_never_matched_by_prefix_in_either_direction() {
        let (x, y) = colliding();
        let t = tag4(&sha256(x.as_bytes()));
        let s = short();
        let p = Path::new("/w/a");
        s.record(p, x.as_bytes());
        for f in fillers(MAX_VERSIONS) {
            s.record(p, f.as_bytes());
        }
        // `x` is gone; `y` has the longer tag.
        s.record(p, y.as_bytes());
        let ty = tag_now(&s, p, &y);
        assert_eq!(ty.len(), 5);
        assert!(s.version(p, &t).is_none(), "the first four characters");
        assert!(s.version(p, &format!("{ty}0")).is_none(), "an extension");
        assert!(s.version(p, &ty[..4]).is_none());
        assert!(s.version(p, &ty).is_some());
    }

    #[test]
    fn a_rebuild_from_the_log_issues_the_tags_the_versions_had() {
        let (x, y) = colliding();
        let t = tag4(&sha256(x.as_bytes()));
        let sid = SessionId::new();
        let store = Arc::new(SeenStore::with_tag_len(4));
        let s = store.session(sid);
        let p = Path::new("/w/a");
        let fx = s.record_rows(p, x.as_bytes(), &[(1, 1)]);
        let fy = s.record_rows(p, y.as_bytes(), &[(1, 1)]);
        let ty = tag_now(&s, p, &y);
        let log = vec![raw(sid, 1, finished(fx)), raw(sid, 2, finished(fy))];
        // The same process: nothing moves.
        store.sync(sid, &log);
        assert_eq!(tag_now(&s, p, &x), t);
        assert_eq!(tag_now(&s, p, &y), ty);
        // Again, after the versions left: the tags are still taken.
        for f in fillers(MAX_VERSIONS + 1) {
            s.record(p, f.as_bytes());
        }
        store.sync(sid, &log);
        assert!(s.version(p, &t).is_none() && s.version(p, &ty).is_none());
        for (first, second) in [(&x, &y), (&y, &x)] {
            // A new process: the log's hashes only. Whichever is seen first,
            // each has the tag it was issued.
            let again = Arc::new(SeenStore::with_tag_len(4));
            again.sync(sid, &log);
            let s2 = again.session(sid);
            s2.record(p, first.as_bytes());
            s2.record(p, second.as_bytes());
            assert_eq!(tag_now(&s2, p, &x), t);
            assert_eq!(tag_now(&s2, p, &y), ty);
        }
    }

    /// Random sequences of reads, searches' versions, and rebuilds from the
    /// log, over a few texts and one- and two-character base tags (so that
    /// collisions and evictions are the usual case): a tag, once seen for a
    /// path, names that text or nothing, never another; and a text has one
    /// tag for good. With restarts (only logged records) the same holds, as
    /// the log names every hash.
    #[test]
    fn a_tag_issued_never_names_another_version_in_random_sequences() {
        for seed in 0..1500u64 {
            for tag_len in [1usize, 2] {
                for restarts in [false, true] {
                    let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
                    let mut next = move |n: usize| {
                        x ^= x << 13;
                        x ^= x >> 7;
                        x ^= x << 17;
                        (x % n as u64) as usize
                    };
                    let sid = SessionId::new();
                    let mut store = Arc::new(SeenStore::with_tag_len(tag_len));
                    let paths = [Path::new("/w/a"), Path::new("/w/b")];
                    let texts: Vec<String> = (0..14).map(|i| format!("text {i}\nrow\n")).collect();
                    // The tag each (path, text) was seen with.
                    let mut tags: HashMap<(usize, usize), String> = HashMap::new();
                    let mut log: Vec<RawEvent> = Vec::new();
                    let note = |tags: &mut HashMap<(usize, usize), String>,
                                s: &SessionSeen,
                                pi: usize,
                                ti: usize| {
                        let Some(v) = s.version_of(paths[pi], &sha256(texts[ti].as_bytes())) else {
                            return;
                        };
                        let had = tags.entry((pi, ti)).or_insert_with(|| v.tag.clone());
                        assert_eq!(*had, v.tag, "seed {seed}: a text changed its tag");
                    };
                    for step in 0..60 {
                        let (pi, ti) = (next(2), next(texts.len()));
                        let s = store.session(sid);
                        match next(10) {
                            0..=4 => {
                                let f = s.record(paths[pi], texts[ti].as_bytes());
                                log.push(raw(sid, step as u64 + 1, finished(f)));
                            }
                            5..=7 if !restarts => s.add_version(paths[pi], texts[ti].as_bytes()),
                            8 => store.sync(sid, &log),
                            9 if restarts => {
                                store = Arc::new(SeenStore::with_tag_len(tag_len));
                                store.sync(sid, &log);
                            }
                            _ => {}
                        }
                        let s = store.session(sid);
                        note(&mut tags, &s, pi, ti);
                        // No two texts of a path share a tag, and a tag found
                        // names the text it was seen with.
                        for ((p2, t2), tag) in &tags {
                            if let Some(v) = s.version(paths[*p2], tag) {
                                assert_eq!(
                                    v.sha,
                                    sha256(texts[*t2].as_bytes()),
                                    "seed {seed} len {tag_len} restarts {restarts} step {step}: tag {tag} of text {t2} names another"
                                );
                            }
                        }
                        let mut seen_tags: HashMap<(usize, &String), usize> = HashMap::new();
                        for ((p2, t2), tag) in &tags {
                            if let Some(o) = seen_tags.insert((*p2, tag), *t2) {
                                assert_eq!(o, *t2, "seed {seed}: texts {o} and {t2} share {tag}");
                            }
                        }
                    }
                }
            }
        }
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
            tag: String::new(),
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

    #[test]
    fn the_base_tag_is_six_hex_characters() {
        assert_eq!(TAG_LEN, 6);
        let t = tag(&sha256(b"anything\n"));
        assert!(
            t.len() == 6
                && t.bytes()
                    .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
        );
        let s = SessionSeen::default();
        let p = Path::new("/w/a");
        s.record(p, b"anything\n");
        assert_eq!(tag_now(&s, p, "anything\n"), t);
    }

    /// Audit of PR 188: at the cap of entries a session holds, the entry a
    /// search or a refusal had just made had the lowest key and was the
    /// victim before the call returned: the refusal's retry got the same
    /// refusal for ever.
    #[test]
    fn a_new_entry_is_not_its_own_victim_at_the_session_cap() {
        let s = SessionSeen::default();
        s.fill_read(MAX_FILES);
        let p = Path::new("/w/found");
        s.version_rows(p, b"one\ntwo\n", &[(1, 2)]);
        let v = s.shown_version(p).expect("the new entry was evicted");
        assert!(v.covers(1, 2));
        // A tag issued for a header that is printed before the version is.
        let q = Path::new("/w/other");
        let sha = sha256(b"x\n");
        let t = s.tag_for(q, &sha);
        s.add_version(q, b"x\n");
        assert_eq!(s.version(q, &t).expect("evicted").sha, sha);
        assert_eq!(s.table_state().0, MAX_FILES);
        // Among the others the order is: unread first, then least recently
        // used: the first new entry took the oldest read one, the second the
        // unread first one.
        assert!(s.get(Path::new("/fill/0")).is_none());
        assert!(s.get(Path::new("/fill/1")).is_some());
        assert!(s.shown_version(p).is_none());
    }

    #[test]
    fn a_new_entry_is_not_its_own_victim_at_the_node_cap() {
        let store = Arc::new(SeenStore::default());
        for _ in 0..MAX_ENTRIES / MAX_FILES {
            store.session(SessionId::new()).fill_read(MAX_FILES);
        }
        let s = store.session(SessionId::new());
        let p = Path::new("/w/found");
        s.version_rows(p, b"one\ntwo\n", &[(1, 2)]);
        assert!(s.shown_version(p).is_some(), "the new entry was evicted");
        let q = Path::new("/w/other");
        let t = s.tag_for(q, &sha256(b"x\n"));
        s.add_version(q, b"x\n");
        assert!(s.version(q, &t).is_some());
        assert!(store.lock().entries <= MAX_ENTRIES);
    }

    /// Audit of PR 188: `remember` pushed the 33rd tag before removing the
    /// oldest (the `Vec` grew to twice the bound), a tag was a `String`
    /// that kept the capacity of the whole hash (64 bytes), so a full entry
    /// held about 4.6 KB, not the 2.3 KB the comments said.
    #[test]
    fn the_worst_case_entry_is_accounted_as_the_comments_say() {
        let s = SessionSeen::default();
        let p = Path::new("/w/a");
        for i in 0..MAX_VERSIONS + 3 * MAX_GONE_TAGS {
            s.record_rows(p, format!("v{i}\nrow\n").as_bytes(), &[(1, 1)]);
        }
        let inner = s.store.lock();
        let e = &inner.sessions[&s.session].files[p];
        assert_eq!(e.vers.len(), MAX_VERSIONS);
        assert_eq!(e.gone.len(), MAX_GONE_TAGS);
        let (issued, version, entry) = (
            std::mem::size_of::<Issued>(),
            std::mem::size_of::<Version>(),
            std::mem::size_of::<Entry>(),
        );
        eprintln!(
            "size_of Issued {issued}, Version {version}, Entry {entry}, PathBuf {}; \
             capacities: gone {}, vers {}",
            std::mem::size_of::<PathBuf>(),
            e.gone.capacity(),
            e.vers.capacity()
        );
        assert_eq!(issued, 33, "a hash and a length, no heap string");
        assert_eq!(e.gone.capacity(), MAX_GONE_TAGS, "removed after pushing");
        assert_eq!(e.vers.capacity(), MAX_VERSIONS, "removed after pushing");
        for v in &e.vers {
            assert_eq!(v.tag.capacity(), v.tag.len(), "a tag keeps no spare room");
        }
        // The map's bucket for the entry at its worst load (hashbrown grows
        // at 7/8 full: 16/7 buckets per entry after a growth), the kept
        // versions and their tags (32 bytes each, as the allocator gives
        // them), the remembered tags.
        let bucket = (std::mem::size_of::<PathBuf>() + entry + 1) * 16 / 7;
        let accounted =
            bucket + e.vers.capacity() * version + e.vers.len() * 32 + e.gone.capacity() * issued;
        eprintln!("accounted {accounted} bytes of {ENTRY_OVERHEAD}");
        assert!(
            accounted <= ENTRY_OVERHEAD,
            "{accounted} > {ENTRY_OVERHEAD}"
        );
        assert!(
            ENTRY_OVERHEAD < accounted + 100,
            "ENTRY_OVERHEAD says {ENTRY_OVERHEAD}, the entry is {accounted}: update the comments"
        );
        assert_eq!(MAX_GONE_TAGS * issued, 528);
    }

    /// The comments of [`MAX_ENTRIES`] and PLAN.md T45 state this total.
    #[test]
    fn the_worst_case_of_the_table_is_under_the_idle_rule() {
        let path_max = 4096;
        let total = MAX_ENTRIES * (path_max + ENTRY_OVERHEAD) + SNAPSHOT_MAX_TOTAL;
        eprintln!("worst case of the table: {total} bytes");
        assert!(total <= 31 << 20, "{total}");
        assert!(total < 50 << 20);
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

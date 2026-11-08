//! Pulling staged files from nodes into the blob store (docs/artifacts.md,
//! T18). Pull only: the master asks for each 256 KiB chunk (four in
//! flight), and a chunk is accepted only as the answer to its own request
//! from its own node, with exactly the bytes asked for. The file is hashed
//! and sealed as it arrives, in offset order; at the end its size and
//! sha256 must match what the node declared, or nothing is kept.
//!
//! Transfers run from their own pool (two per node, four on the server,
//! queued), never from the upload slots, and never block the node link:
//! `on_chunk` hands chunks over with `try_send`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use base64::Engine as _;
use harness_core::SessionId;
use harness_protocol::{CAP_ARTIFACTS, MAX_CHUNK, MasterToNode};
use harness_store::artifacts::{Committed, Pending};
use harness_store::blobs::NewBlob;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{Master, lock, storage};

const PER_NODE: usize = 2;
const TOTAL: usize = 4;
/// Requests in flight per transfer.
const WINDOW: usize = 4;
const IDLE: Duration = Duration::from_secs(60);
const HARD_CAP: Duration = Duration::from_secs(3600);
const BUSY_RETRY: Duration = Duration::from_millis(500);
/// Unsolicited chunks are logged at most this often.
const WARN_EVERY: Duration = Duration::from_secs(60);
const MB: u64 = 1024 * 1024;
const DISK_CHECK_EVERY: u64 = 16 * MB;

/// One `artifact.chunk` as its transfer gets it.
#[derive(Debug)]
pub struct Chunk {
    pub req_id: String,
    pub offset: u64,
    pub data: String,
    pub eof: bool,
    pub error: Option<String>,
}

/// Bytes received of a pending version, for the UI.
pub struct Progress {
    received: AtomicU64,
    total: u64,
}

/// The pull pool and the routes chunks take to their transfer.
pub struct Pulls {
    /// Transfers running or queued, by version: each is pulled once.
    active: std::sync::Mutex<HashMap<String, Arc<Progress>>>,
    per_node: std::sync::Mutex<HashMap<String, Arc<Semaphore>>>,
    total: Arc<Semaphore>,
    /// `(node_id, req_id)` → the transfer that asked.
    routes: std::sync::Mutex<HashMap<(String, String), mpsc::Sender<Chunk>>>,
    /// Cancelled when a node goes away (its transfers keep their versions
    /// pending) or is revoked.
    links: std::sync::Mutex<HashMap<String, CancellationToken>>,
    /// Idle and total limits (tests shorten them).
    limits: std::sync::Mutex<(Duration, Duration)>,
    warned: std::sync::Mutex<Option<Instant>>,
}

impl Default for Pulls {
    fn default() -> Self {
        Self {
            active: Default::default(),
            per_node: Default::default(),
            total: Arc::new(Semaphore::new(TOTAL)),
            routes: Default::default(),
            links: Default::default(),
            limits: std::sync::Mutex::new((IDLE, HARD_CAP)),
            warned: Default::default(),
        }
    }
}

impl Pulls {
    fn node_pool(&self, node_id: &str) -> Arc<Semaphore> {
        lock(&self.per_node)
            .entry(node_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_NODE)))
            .clone()
    }

    fn link(&self, node_id: &str) -> CancellationToken {
        lock(&self.links)
            .entry(node_id.to_string())
            .or_default()
            .clone()
    }

    fn warn(&self, node_id: &str, why: &str) {
        let mut w = lock(&self.warned);
        if w.is_none_or(|t| t.elapsed() >= WARN_EVERY) {
            *w = Some(Instant::now());
            tracing::warn!(node = %node_id, "dropped an artifact chunk: {why}");
        }
    }
}

/// Shorten the idle and total limits (tests).
pub fn set_timeouts(m: &Master, idle: Duration, total: Duration) {
    *lock(&m.inner.pulls.limits) = (idle, total);
}

/// `(received, total)` bytes of a version being pulled.
pub fn progress(m: &Master, version_id: &str) -> Option<(u64, u64)> {
    lock(&m.inner.pulls.active)
        .get(version_id)
        .map(|p| (p.received.load(Ordering::Relaxed), p.total))
}

/// A chunk from `node_id`: to the transfer whose request it answers, or
/// dropped (unsolicited, another node's, or a transfer that is not
/// keeping up). Never waits.
pub fn on_chunk(m: &Master, node_id: &str, c: Chunk) {
    let pulls = &m.inner.pulls;
    let route = lock(&pulls.routes)
        .get(&(node_id.to_string(), c.req_id.clone()))
        .cloned();
    match route {
        Some(tx) => {
            if tx.try_send(c).is_err() {
                pulls.warn(node_id, "its transfer is not keeping up");
            }
        }
        None => pulls.warn(node_id, "it answers no request of this master"),
    }
}

/// The node's link closed (or it was revoked): its transfers stop, and a
/// new link gets a fresh token.
pub fn node_down(m: &Master, node_id: &str) {
    if let Some(t) = lock(&m.inner.pulls.links).remove(node_id) {
        t.cancel();
    }
}

/// Tell `node_id` it may delete a stage (best effort; only to nodes that
/// announce the capability).
pub async fn release(m: &Master, node_id: &str, sid: SessionId, stage_id: &str) {
    if !m.node_has(node_id, CAP_ARTIFACTS) {
        return;
    }
    let stage = stage_id.to_string();
    let res = m
        .send_command(node_id, |req_id| MasterToNode::ArtifactRelease {
            req_id,
            session_id: sid,
            stage_id: stage,
        })
        .await;
    if let Err(e) = res {
        tracing::debug!(node = %node_id, error = %e, "stage release not acknowledged");
    }
}

/// Pull a pending version, once: queued for a slot of its node's and the
/// server's pool.
pub fn start(m: &Master, p: Pending) {
    {
        let mut active = lock(&m.inner.pulls.active);
        if active.contains_key(&p.version_id) {
            return;
        }
        active.insert(
            p.version_id.clone(),
            Arc::new(Progress {
                received: AtomicU64::new(0),
                total: p.size,
            }),
        );
    }
    let m = m.clone();
    tokio::spawn(async move {
        let node = m.inner.pulls.node_pool(&p.node_id);
        let total = m.inner.pulls.total.clone();
        let outcome = match (node.acquire_owned().await, total.acquire_owned().await) {
            (Ok(_n), Ok(_t)) => Transfer::new(&m, &p).run().await,
            _ => Outcome::Offline,
        };
        let offline = matches!(outcome, Outcome::Offline);
        finish(&m, &p, outcome).await;
        lock(&m.inner.pulls.active).remove(&p.version_id);
        // The node came back while this one was stopping: its hello found
        // the transfer still active, so the restart happens here.
        if offline && m.is_connected(&p.node_id) {
            crate::artifacts::on_node_up(&m, &p.node_id).await;
        }
    });
}

/// How a transfer ended.
#[derive(Debug)]
enum Outcome {
    /// Stored and ready (or no longer wanted).
    Done {
        media: bool,
    },
    /// The node went away: the version stays pending for its next hello.
    Offline,
    Failed(String),
}

async fn finish(m: &Master, p: &Pending, outcome: Outcome) {
    let sid = p.session_id.as_deref().and_then(|s| s.parse().ok());
    match outcome {
        Outcome::Offline => return,
        Outcome::Done { media } => {
            if media {
                m.changed("media", p.owner_id.clone());
            }
        }
        Outcome::Failed(why) => {
            tracing::info!(version = %p.version_id, reason = %why, "artifact transfer failed");
            let vid = p.version_id.clone();
            let _ = m
                .inner
                .mirror
                .call(move |s| s.artifact_fail(&vid, &why))
                .await;
        }
    }
    if let Some(sid) = sid {
        release(m, &p.node_id, sid, &p.stage_id).await;
    }
    let aid = p.artifact_id.clone();
    let shared = m
        .inner
        .mirror
        .call(move |s| s.artifact_get(&aid))
        .await
        .ok()
        .flatten()
        .is_some_and(|a| a.shared());
    crate::artifacts::notify(m, p.owner_id.clone(), shared);
}

/// One pull in progress.
struct Transfer<'a> {
    m: &'a Master,
    p: &'a Pending,
    rx: mpsc::Receiver<Chunk>,
    tx: mpsc::Sender<Chunk>,
    /// Request id → the offset it asked for.
    inflight: HashMap<String, u64>,
    /// Chunks that arrived ahead of the write position.
    ahead: BTreeMap<u64, Vec<u8>>,
    next: u64,
    written: u64,
}

impl Drop for Transfer<'_> {
    fn drop(&mut self) {
        let node = self.p.node_id.clone();
        let mut routes = lock(&self.m.inner.pulls.routes);
        for req in self.inflight.keys() {
            routes.remove(&(node.clone(), req.clone()));
        }
    }
}

impl<'a> Transfer<'a> {
    fn new(m: &'a Master, p: &'a Pending) -> Self {
        let (tx, rx) = mpsc::channel(4 * WINDOW);
        Self {
            m,
            p,
            rx,
            tx,
            inflight: HashMap::new(),
            ahead: BTreeMap::new(),
            next: 0,
            written: 0,
        }
    }

    /// The length a request at `offset` asks for (and its chunk must have).
    fn len_at(&self, offset: u64) -> u32 {
        (self.p.size - offset).min(u64::from(MAX_CHUNK)) as u32
    }

    /// Ask for the chunk at `offset`; false when the node is gone.
    fn ask(&mut self, sid: SessionId, offset: u64) -> bool {
        let req_id = harness_core::new_id();
        lock(&self.m.inner.pulls.routes)
            .insert((self.p.node_id.clone(), req_id.clone()), self.tx.clone());
        self.inflight.insert(req_id.clone(), offset);
        self.m.send_raw(
            &self.p.node_id,
            MasterToNode::ArtifactFetch {
                req_id,
                session_id: sid,
                stage_id: self.p.stage_id.clone(),
                offset,
                len: self.len_at(offset),
            },
        )
    }

    fn fill(&mut self, sid: SessionId) -> bool {
        while self.inflight.len() < WINDOW && self.next < self.p.size {
            let at = self.next;
            self.next += u64::from(self.len_at(at));
            if !self.ask(sid, at) {
                return false;
            }
        }
        true
    }

    async fn run(mut self) -> Outcome {
        let Some(sid) = self.p.session_id.as_deref().and_then(|s| s.parse().ok()) else {
            return Outcome::Failed("the version has no chat".into());
        };
        if !self.m.node_has(&self.p.node_id, CAP_ARTIFACTS) {
            return Outcome::Failed(crate::artifacts::TOO_OLD.into());
        }
        let Some(st) = storage::get(self.m) else {
            return Outcome::Failed("file storage is not available on this server".into());
        };
        let writer = match storage::TempWriter::start(st) {
            Ok(w) => w,
            Err(e) => return Outcome::Failed(format!("could not store the file: {e}")),
        };
        match self.receive(sid, &writer).await {
            Ok(()) => {}
            Err(o) => {
                writer.abandon().await;
                return o;
            }
        }
        let written = match writer.finish().await {
            Ok(w) => w,
            Err(e) => return Outcome::Failed(format!("could not store the file: {e}")),
        };
        if written.size != self.p.size || written.sha256 != self.p.sha256 {
            let _ = std::fs::remove_file(&written.path);
            return Outcome::Failed(
                "the file the node sent does not match its checksum; nothing was kept".into(),
            );
        }
        self.commit(written).await
    }

    /// Receive every chunk into `writer`, in offset order.
    async fn receive(
        &mut self,
        sid: SessionId,
        writer: &storage::TempWriter,
    ) -> Result<(), Outcome> {
        let (idle, total) = *lock(&self.m.inner.pulls.limits);
        let deadline = Instant::now() + total;
        let link = self.m.inner.pulls.link(&self.p.node_id);
        let progress = progress_of(self.m, &self.p.version_id);
        let mut checked = 0u64;
        if !self.fill(sid) {
            return Err(Outcome::Offline);
        }
        while self.written < self.p.size {
            let wait = idle.min(deadline.saturating_duration_since(Instant::now()));
            let c = tokio::select! {
                c = tokio::time::timeout(wait, self.rx.recv()) => c,
                _ = link.cancelled() => return Err(Outcome::Offline),
            };
            let c = match c {
                Ok(Some(c)) => c,
                Ok(None) => return Err(Outcome::Offline),
                Err(_) if !self.m.is_connected(&self.p.node_id) => return Err(Outcome::Offline),
                Err(_) if Instant::now() >= deadline => {
                    return Err(Outcome::Failed("the transfer took over an hour".into()));
                }
                Err(_) => {
                    return Err(Outcome::Failed("the node stopped sending the file".into()));
                }
            };
            let Some(offset) = self.inflight.remove(&c.req_id) else {
                continue;
            };
            lock(&self.m.inner.pulls.routes).remove(&(self.p.node_id.clone(), c.req_id.clone()));
            if let Some(bytes) = self.accept(offset, c).await? {
                self.ahead.insert(offset, bytes);
            } else if !self.ask(sid, offset) {
                // A busy answer, asked again.
                return Err(Outcome::Offline);
            }
            while let Some(b) = self.ahead.remove(&self.written) {
                self.written += b.len() as u64;
                if !writer.write(Bytes::from(b)).await {
                    return Err(Outcome::Failed("could not store the file".into()));
                }
            }
            if let Some(p) = &progress {
                p.received.store(self.written, Ordering::Relaxed);
            }
            if self.written - checked >= DISK_CHECK_EVERY {
                checked = self.written;
                storage::disk_ok(self.m, 0).map_err(Outcome::Failed)?;
            }
            if !self.fill(sid) {
                return Err(Outcome::Offline);
            }
        }
        Ok(())
    }

    /// Check one answer to the request at `offset`: its bytes, or `None`
    /// for a busy node (after the retry wait).
    async fn accept(&self, offset: u64, c: Chunk) -> Result<Option<Vec<u8>>, Outcome> {
        if c.offset != offset {
            return Err(Outcome::Failed(
                "the node answered with the wrong part of the file".into(),
            ));
        }
        match c.error.as_deref() {
            Some("busy") => {
                tokio::time::sleep(BUSY_RETRY).await;
                return Ok(None);
            }
            Some(e) => {
                let e = harness_core::text::clean_line(e, 200);
                return Err(Outcome::Failed(format!(
                    "the node could not read the file: {e}"
                )));
            }
            None => {}
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(c.data.as_bytes())
            .map_err(|_| Outcome::Failed("the node sent damaged data".into()))?;
        let want = u64::from(self.len_at(offset));
        let last = offset + want == self.p.size;
        if bytes.len() as u64 != want || c.eof != last {
            return Err(Outcome::Failed(
                "the node sent a different amount of data than the file has".into(),
            ));
        }
        Ok(Some(bytes))
    }

    /// Sniff, seal into a blob and make the version ready (retention, the
    /// name index and the Media item in the same transaction).
    async fn commit(&self, w: crate::blobs::Written) -> Outcome {
        let sniffed = crate::sniff::sniff(&w.head, Some(&self.p.filename), None);
        let blob = NewBlob {
            blob_id: w.blob_id.clone(),
            owner_id: self.p.owner_id.clone(),
            sha256: w.sha256.clone(),
            size: w.size,
            kind: sniffed.kind.into(),
            format: sniffed.format,
            mime: sniffed.mime,
            display: sniffed.display.into(),
            key: w.key.clone(),
            width: sniffed.width,
            height: sniffed.height,
        };
        let limits = storage::config(self.m).limits(self.p.owner_id.as_deref());
        let keep = crate::artifacts::config(self.m).keep_versions.max(1) as usize;
        let vid = self.p.version_id.clone();
        let res = storage::commit(self.m, w, move |s| {
            let c = s.artifact_commit(&vid, &blob, limits, keep)?;
            let keep_file = matches!(c, Committed::Ready { new_blob: true, .. });
            if let Committed::Ready { gone, .. } = &c
                && !gone.is_empty()
            {
                s.checkpoint()?;
            }
            Ok((c, keep_file))
        })
        .await;
        match res {
            Ok(Committed::Ready { gone, media, .. }) => {
                storage::unlink(self.m, gone).await;
                Outcome::Done { media }
            }
            Ok(Committed::NotPending) => Outcome::Done { media: false },
            Ok(Committed::OverQuota(q)) => Outcome::Failed(storage::full_message(&q)),
            Err((_, e)) => Outcome::Failed(e),
        }
    }
}

fn progress_of(m: &Master, version_id: &str) -> Option<Arc<Progress>> {
    lock(&m.inner.pulls.active).get(version_id).cloned()
}

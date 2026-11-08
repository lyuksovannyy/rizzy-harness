//! Artifacts: files agents publish from agentic chats (and users save from
//! them), kept as named lineages of versions (docs/artifacts.md). Stored
//! files, never rendered or run.
//!
//! A node records `artifact_staged` in the chat's log; when the mirror
//! applies it, `on_applied` validates it (fail-closed) and records a
//! pending version, and `artifact_pull` pulls the bytes. Owner, workspace
//! and project come from the publishing session's own mirror row, never
//! from its parent and never from the node (T7, T10, T18).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use harness_core::RawEvent;
use harness_core::artifacts::{ArtifactMeta, valid_stage_id};
use harness_core::event::{ArtifactStaged, EventKind};
use harness_protocol::CAP_ARTIFACTS;
use harness_store::SessionRow;
use harness_store::artifacts::{ArtifactRow, IngestCaps, Ingested, NewVersion, VersionRow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::Principal;
use crate::search::{SearchCtx, SearchResult};
use crate::workspaces::{self as ws, Access};
use crate::{Master, lock, storage};

/// `[artifacts]` in config.toml; left out of the file while it is the
/// default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArtifactsConfig {
    pub enabled: bool,
    /// Ready, unpinned versions kept per artifact.
    pub keep_versions: u32,
    /// Pinned versions allowed per artifact.
    pub max_pinned: u32,
}

impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_versions: 5,
            max_pinned: 10,
        }
    }
}

impl ArtifactsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Versions one node may have waiting to upload.
const PENDING_PER_NODE: u64 = 8;
/// `artifact_staged` events one node may send per hour; the rest are
/// ignored (no row), so a runaway node cannot fill the list.
const STAGED_PER_HOUR: usize = 30;
const HOUR: Duration = Duration::from_secs(3600);
/// Lineages with a ready version per (owner, workspace), and per workspace.
const LINEAGES_PER_OWNER: u64 = 200;
const LINEAGES_PER_WORKSPACE: u64 = 1000;
/// How long nodes keep a stage (node.toml's default): older events (a
/// mirror rebuilt from replay) are ignored, older pending versions fail.
pub const STAGE_TTL: Duration = Duration::from_secs(24 * 3600);
pub const TOO_OLD: &str = "the worker node is too old to publish; update it";

/// Per-node counts of recent `artifact_staged` events.
#[derive(Default)]
pub struct Throttle {
    seen: std::sync::Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Throttle {
    /// Count one event of `node_id`; false once it is over the hourly cap.
    fn admit(&self, node_id: &str) -> bool {
        let now = Instant::now();
        let mut seen = lock(&self.seen);
        let q = seen.entry(node_id.to_string()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) >= HOUR) {
            q.pop_front();
        }
        if q.len() >= STAGED_PER_HOUR {
            return false;
        }
        q.push_back(now);
        true
    }
}

/// The current `[artifacts]` settings (defaults without a managed config).
pub fn config(m: &Master) -> ArtifactsConfig {
    m.provider_runtime()
        .map(|rt| rt.config().artifacts)
        .unwrap_or_default()
}

/// Whether Artifacts takes files: switched on, with a blob store.
pub fn enabled(m: &Master) -> bool {
    storage::get(m).is_some() && config(m).enabled
}

/// The `artifacts` block of `/api/features` for `who` in `scope`: shown to
/// whoever may publish here, and to anyone who can see an artifact; `why`
/// explains an empty page to those who cannot publish.
pub async fn features(m: &Master, who: &Principal, scope: Option<&Access>) -> Value {
    let on = enabled(m);
    if !on {
        return json!({ "enabled": false, "show": false, "why": null });
    }
    let refusal = ws::agentic_refusal(who, scope);
    let visible = match &refusal {
        None => true,
        Some(_) => {
            let (viewer, w) = (who.user_id.clone(), scope.map(|a| a.id().to_string()));
            m.inner
                .mirror
                .call(move |s| s.artifact_count_visible(viewer.as_deref(), w.as_deref()))
                .await
                .is_ok_and(|n| n > 0)
        }
    };
    let why = refusal.map(|_| {
        let place = scope
            .filter(|a| !a.personal)
            .map(|a| format!(" in {}", a.workspace.name))
            .unwrap_or_default();
        format!("Artifacts are files agents publish from agentic chats; you can't run agentic chats{place}.")
    });
    json!({ "enabled": true, "show": visible, "why": why })
}

/// Tell the UIs a list changed: the owner's sockets for a private
/// artifact, every socket for a shared one (content-free).
pub fn notify(m: &Master, owner: Option<String>, shared: bool) {
    m.changed("artifacts", if shared { None } else { owner });
}

/// The kind a client labels: the blob's, as one of the search types.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "image" => "image",
        "video" => "video",
        "audio" => "audio",
        "binary" => "binary",
        "package" => "package",
        "archive" => "archive",
        "document" => "document",
        _ => "other",
    }
}

/// The search provider over artifact names, file names and notes, with
/// the list's visibility (`ctx.owner` is the viewer).
pub fn search(
    s: &harness_store::Store,
    ctx: &SearchCtx,
) -> harness_store::Result<Vec<SearchResult>> {
    let rows = s.artifact_search(
        ctx.owner,
        ctx.workspace,
        ctx.fts.as_deref(),
        ctx.limit,
        ctx.offset,
    )?;
    let mut out = Vec::with_capacity(rows.len());
    for (r, marked) in rows {
        let latest = s.artifact_latest(&r.artifact_id)?;
        out.push(search_row(ctx, r, marked, latest.as_ref()));
    }
    Ok(out)
}

fn search_row(
    ctx: &SearchCtx,
    r: ArtifactRow,
    marked: String,
    latest: Option<&VersionRow>,
) -> SearchResult {
    let blob = latest.and_then(|v| v.blob.as_ref());
    let preview = match latest {
        Some(v) if !v.notes.is_empty() => v.notes.clone(),
        Some(v) => {
            let target = serde_json::from_str::<ArtifactMeta>(&v.meta)
                .ok()
                .and_then(|m| m.target)
                .map(|t| format!(" · {t}"))
                .unwrap_or_default();
            format!(
                "v{} · {}{target}",
                v.seq,
                harness_core::artifacts::human_size(v.size)
            )
        }
        None => String::new(),
    };
    let thumb = match (latest, blob) {
        (Some(v), Some(b)) if b.kind == "image" && v.preview.has_thumb => Some(format!(
            "/api/artifacts/{}/versions/{}/thumb",
            r.artifact_id, v.version_id
        )),
        _ => None,
    };
    SearchResult {
        kind: "artifact",
        type_: kind_label(blob.map_or("other", |b| b.kind.as_str())),
        tier: crate::search::tier(ctx.tokens, &r.name),
        title: marked,
        preview,
        snippet: None,
        time_ms: r.updated_ms,
        project_id: r.project_id,
        running: None,
        thumb,
        id: r.artifact_id,
    }
}

/// After the mirror applied an artifact event of `node_id`'s.
pub async fn on_applied(m: &Master, node_id: &str, e: &RawEvent) {
    match e.kind.as_str() {
        "artifact_staged" => staged(m, node_id, e).await,
        // The chat shows it; the chat's artifact list changed.
        "artifact_stage_failed" => {
            let sid = e.session_id;
            if let Ok(Some(row)) = m.inner.mirror.call(move |s| s.session(sid)).await {
                notify(m, row.owner_id, false);
            }
        }
        _ => {}
    }
}

async fn audit(m: &Master, kind: &'static str, node_id: &str, e: &RawEvent, why: &str) {
    let detail = json!({ "node_id": node_id, "session_id": e.session_id, "why": why }).to_string();
    let _ = m.inner.mirror.call(move |s| s.audit(kind, &detail)).await;
}

/// One `artifact_staged`: ignored when stale or throttled, refused with no
/// row when its session has no owner or workspace, else a version (failed
/// with the reason when a check fails, so nothing is silently lost).
async fn staged(m: &Master, node_id: &str, e: &RawEvent) {
    let cutoff = harness_core::now_ms() - STAGE_TTL.as_millis() as i64;
    if e.ts_ms < cutoff {
        return;
    }
    if !m.inner.artifact_throttle.admit(node_id) {
        tracing::warn!(node = %node_id, "too many staged files this hour; ignoring");
        audit(m, "artifact.throttled", node_id, e, "over the hourly cap").await;
        return;
    }
    let Ok(EventKind::ArtifactStaged(a)) = e.typed() else {
        audit(m, "artifact.refused", node_id, e, "malformed event").await;
        return;
    };
    let sid = e.session_id;
    let row = m
        .inner
        .mirror
        .call(move |s| s.session(sid))
        .await
        .ok()
        .flatten();
    let Some((row, owner, workspace)) = row.and_then(|r| {
        let (o, w) = (r.owner_id.clone()?, r.workspace_id.clone()?);
        (r.node_id == node_id).then_some((r, o, w))
    }) else {
        audit(
            m,
            "artifact.refused",
            node_id,
            e,
            "the chat has no owner or workspace",
        )
        .await;
        return;
    };
    let uid = owner.clone();
    let Ok(Some(user)) = m.inner.mirror.call(move |s| s.user_by_id(&uid)).await else {
        audit(
            m,
            "artifact.refused",
            node_id,
            e,
            "the chat's owner is gone",
        )
        .await;
        return;
    };
    let who = Principal::from_user(user);
    let reason = refusal(m, node_id, &a, &row, &who).await;
    let v = new_version(m, node_id, &a, &row, owner.clone(), workspace).await;
    let caps = IngestCaps {
        pending_per_node: PENDING_PER_NODE,
        per_owner: LINEAGES_PER_OWNER,
        per_workspace: LINEAGES_PER_WORKSPACE,
    };
    let res = m
        .inner
        .mirror
        .call(move |s| s.artifact_ingest(&v, reason.as_deref(), caps))
        .await;
    match res {
        Ok(Ingested::Added {
            version_id,
            failed: None,
            ..
        }) => {
            let nid = node_id.to_string();
            let p = m
                .inner
                .mirror
                .call(move |s| s.artifact_pending(Some(&nid)))
                .await
                .unwrap_or_default()
                .into_iter()
                .find(|p| p.version_id == version_id);
            if let Some(p) = p {
                crate::artifact_pull::start(m, p);
            }
            notify(m, Some(owner), false);
        }
        Ok(Ingested::Added { .. }) => notify(m, Some(owner), false),
        Ok(Ingested::Duplicate) => {}
        Err(err) => tracing::warn!(error = %err, "could not record a staged file"),
    }
}

/// Why this staged file may not be taken, or `None`.
async fn refusal(
    m: &Master,
    node_id: &str,
    a: &ArtifactStaged,
    row: &SessionRow,
    who: &Principal,
) -> Option<String> {
    if !valid_stage_id(&a.stage_id) {
        return Some("the node sent a malformed stage id".into());
    }
    if a.sha256.len() != 64
        || !a
            .sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Some("the node sent a malformed checksum".into());
    }
    if row.profile.as_deref() != Some("agentic") || row.incognito {
        return Some("only agentic chats publish files".into());
    }
    if let Some(why) = ws::agentic_blocked(m, who, row).await {
        return Some(why);
    }
    if !m.node_has(node_id, CAP_ARTIFACTS) {
        return Some(TOO_OLD.into());
    }
    if !config(m).enabled {
        return Some("Artifacts are turned off on this server".into());
    }
    if storage::get(m).is_none() {
        return Some("file storage is not available on this server".into());
    }
    if a.size == 0 {
        return Some("the file is empty".into());
    }
    storage::check_room(m, who.user_id.as_deref(), a.size)
        .await
        .err()
}

/// The version row from a staged event, every node-sent string cleaned.
async fn new_version(
    m: &Master,
    node_id: &str,
    a: &ArtifactStaged,
    row: &SessionRow,
    owner: String,
    workspace: String,
) -> NewVersion {
    let name = harness_core::text::clean_line(
        &crate::attachments::clean_name(&a.name),
        harness_core::artifacts::MAX_NAME,
    );
    let filename =
        harness_core::text::clean_line(&crate::attachments::clean_name(&a.filename), 120);
    let mut meta = a.meta.clone();
    meta.source_path = Some(a.source_path.clone());
    let meta = serde_json::to_string(&meta.cleaned().fit()).unwrap_or_else(|_| "{}".into());
    let notes = a
        .notes
        .as_deref()
        .map(|n| harness_core::text::clean_line(n, harness_core::artifacts::MAX_NOTES))
        .unwrap_or_default();
    NewVersion {
        owner_id: owner,
        project_id: own_project(m, row, &workspace).await,
        workspace_id: workspace,
        name,
        node_id: node_id.to_string(),
        stage_id: harness_core::text::clean_line(&a.stage_id, 64),
        size: a.size,
        sha256: harness_core::text::clean_line(&a.sha256, 64),
        filename,
        packed: (a.packed.as_deref() == Some("tar.gz")).then(|| "tar.gz".into()),
        by_user: a.by == harness_core::artifacts::StagedBy::User,
        session_id: Some(row.session_id.to_string()),
        call_id: a
            .call_id
            .as_deref()
            .map(|c| harness_core::text::clean_line(c, 64)),
        meta,
        notes,
    }
}

/// The project of the session's own row (never its parent's), when the
/// project is in the chat's workspace.
async fn own_project(m: &Master, row: &SessionRow, workspace: &str) -> Option<String> {
    let sid = row.session_id;
    let link = m
        .inner
        .mirror
        .call(move |s| s.session_project(sid))
        .await
        .ok()??;
    (link.workspace_id == workspace).then_some(link.project_id)
}

/// A node said hello: restart its pending pulls, or fail them when it no
/// longer announces the capability.
pub async fn on_node_up(m: &Master, node_id: &str) {
    let nid = node_id.to_string();
    let pending = m
        .inner
        .mirror
        .call(move |s| s.artifact_pending(Some(&nid)))
        .await
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    if !m.node_has(node_id, CAP_ARTIFACTS) {
        fail_node(m, node_id, TOO_OLD).await;
        return;
    }
    for p in pending {
        crate::artifact_pull::start(m, p);
    }
}

/// After node `node_id` was revoked: its transfers stop and its pending
/// versions fail.
pub async fn on_node_revoked(m: &Master, node_id: &str) {
    crate::artifact_pull::node_down(m, node_id);
    fail_node(m, node_id, "the worker node was removed").await;
}

async fn fail_node(m: &Master, node_id: &str, reason: &'static str) {
    let nid = node_id.to_string();
    let res = m
        .inner
        .mirror
        .call(move |s| {
            let owners: Vec<Option<String>> = s
                .artifact_pending(Some(&nid))?
                .into_iter()
                .map(|p| p.owner_id)
                .collect();
            s.artifact_fail_node(&nid, reason)?;
            Ok::<_, harness_store::StoreError>(owners)
        })
        .await;
    match res {
        Ok(owners) => {
            for o in owners {
                notify(m, o, false);
            }
        }
        Err(e) => tracing::warn!(node = %node_id, error = %e, "could not fail pending files"),
    }
}

/// The hourly part of Artifacts (under the blob store's commit lock):
/// stale pending versions fail and their stages are released, failed
/// versions beyond retention and empty lineages go. Returns the blobs to
/// unlink.
pub async fn hourly(m: &Master) -> Vec<String> {
    let ttl = STAGE_TTL.as_millis() as i64;
    let res = m.inner.mirror.call(move |s| s.artifacts_hourly(ttl)).await;
    let swept = match res {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "artifact sweep failed");
            return vec![];
        }
    };
    // Released in the background: a slow node must not hold the blob
    // store's commit lock this runs under.
    for (node, sid, stage) in swept.expired {
        if let Some(sid) = sid.and_then(|s| s.parse().ok()) {
            let m = m.clone();
            tokio::spawn(
                async move { crate::artifact_pull::release(&m, &node, sid, &stage).await },
            );
        }
    }
    if swept.rows > 0 {
        m.changed("artifacts", None);
    }
    swept.gone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_throttle_caps_each_node_separately() {
        let t = Throttle::default();
        for _ in 0..STAGED_PER_HOUR {
            assert!(t.admit("a"));
        }
        assert!(!t.admit("a"), "the 31st in an hour");
        assert!(t.admit("b"));
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_label("package"), "package");
        assert_eq!(kind_label("bin"), "other");
    }
}

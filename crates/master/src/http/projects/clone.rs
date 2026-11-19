//! A new project's folder cloned from a forge repository (ADOPTION_PLAN
//! X.27 step 3, PLAN.md T86): `POST /api/projects` with `folder_on`,
//! `connection_id` and `full_name`, answered at once (202) with the
//! project in its pending state; `DELETE /api/projects/{id}/clone`
//! dismisses a failed one.
//!
//! **Pending, not a project.** While its node clones, the project is a row
//! of `project_clones` (crates/store), not of `projects`: every route that
//! takes a project id looks it up in `projects`, so a pending one is
//! unknown to all of them (a chat, a link, a folder request, a board, a
//! memory, a routine, the `.mcp.json` list: 404), and the list shows it
//! apart, under `pending`, as `cloning`. When the node answers with the
//! folder, the row goes and the project is written with the same id in
//! one transaction (as pull request 163 wrote it: folder, source); when the
//! clone fails, the row stays `failed` with the reason, shown to its
//! starter only, until they dismiss it (the newest ten a person's kept;
//! failed rows count against no limit). `projects_changed` goes to the
//! workspace at the start and at the end. A master that restarts fails
//! every running row at start (`Master::new`), since nothing waits for its
//! node's answer any more.
//!
//! In this order, so that nothing is opened or sent before it must be: the
//! caller is a person (the API token has no connections: 403, as on the
//! connection routes); the connection is the caller's own, looked up by
//! (caller, id), so anyone else's id is 404; its host is still allowed on
//! this server (else 409); the name has the shape that forge kind allows
//! (else 400: nothing else the person types becomes a URL); the node is
//! connected and announced `forge-clone` (an older node is never sent the
//! message: 409). Then, in a task of its own (a client that goes away
//! leaves no half-made state): the row as `cloning` (refused, 409, while
//! the same person clones the same repository onto the same node, or by
//! the workspace's project limit, which counts running clones); the
//! listing's limits (the person's one call in flight, the server's few,
//! the host's failure budget, the person's rate); the token, opened only
//! through `Master::connection_token`, and one forge call, `GET` of that
//! repository by name, with the listing's client and rules; the message to
//! the node (refused at once when the node is away or busy). Any refusal
//! up to here removes the row and is the request's answer; from here on
//! the request has answered 202 and the clone ends in the row. Why not a
//! listing: GitHub has no search by name, so a repository beyond its 1,000
//! most recently updated would not be found, and one exact call answers
//! what the clone needs (the name as the forge spells it, whether it is
//! private) with no paging. The token goes to the node only when the forge
//! says the repository is private; a public one is cloned with no
//! credential. The host sent is the connection's host, never one the
//! person names.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::delete;
use axum::{Json, Router};
use harness_store::NewProject;
use harness_store::project_clones::{CloneRow, NewClone};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::{ApiError, ApiResult, changed, store_err};
use crate::auth::Principal;
use crate::forge::{CheckError, ForgeHost};
use crate::forge_repos;
use crate::http::connections::{err, sealing_failed};
use crate::http::require;
use crate::project_folder::{self, CloneSource, FolderError};
use crate::projects::{self as p, ProjectSettings};
use crate::{Audience, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/projects/{id}/clone", delete(dismiss))
}

/// A pending project as `who` sees it: every member sees a running one
/// (its name, node and what is cloned, never whose connection it uses but
/// for `mine`); its starter alone sees a failed one, with the reason.
pub(super) fn pending_json(c: &CloneRow, who: &Principal) -> Option<Value> {
    let mine = who.user_id.as_deref() == Some(c.created_by.as_str());
    if c.failed.is_some() && !mine {
        return None;
    }
    Some(json!({
        "project_id": c.project_id,
        "workspace_id": c.workspace_id,
        "name": c.name,
        "node_id": c.node_id,
        "state": if c.failed.is_some() { "failed" } else { "cloning" },
        "source": { "host": c.host, "full_name": c.full_name },
        "started_ms": c.started_ms,
        "mine": mine,
        "reason": c.failed,
    }))
}

/// Check `who`'s request to clone `full_name` through their connection
/// `connection_id` onto `node` (the module's order up to the task), then
/// start it: the pending project, answered 202.
pub(super) async fn start(
    m: &Master,
    who: &Principal,
    connection_id: &str,
    full_name: &str,
    node: &str,
    new: NewProject,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let me = who.user_id.clone().ok_or_else(|| {
        err(
            StatusCode::FORBIDDEN,
            "connections belong to a person; the API token has none",
        )
    })?;
    let (u, i) = (me.clone(), connection_id.to_string());
    let row = m
        .inner
        .mirror
        .call(move |s| s.connection(&u, &i))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connection"))?;
    let h = m
        .forge_hosts()
        .into_iter()
        .find(|h| h.host == row.host)
        .ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "this server no longer allows that forge host, so nothing is cloned from it",
            )
        })?;
    if !forge_repos::valid_full_name(h.kind, full_name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            format!(
                "full_name must be a repository name {} allows (owner/name)",
                h.host
            ),
        ));
    }
    project_folder::may_clone_on(m, node).map_err(super::folder::folder_err)?;
    let job = Job {
        m: m.clone(),
        who: who.clone(),
        me,
        login: row.login,
        connection_id: connection_id.to_string(),
        host: h,
        full_name: full_name.to_string(),
        node: node.to_string(),
        new,
    };
    let (tx, rx) = oneshot::channel();
    tokio::spawn(job.run(tx));
    match rx.await {
        Ok(Ok(v)) => Ok((StatusCode::ACCEPTED, Json(v))),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the clone stopped before it started".into(),
        )),
    }
}

/// One clone, from its row to its end.
struct Job {
    m: Master,
    who: Principal,
    me: String,
    login: String,
    connection_id: String,
    host: ForgeHost,
    full_name: String,
    node: String,
    new: NewProject,
}

impl Job {
    async fn run(mut self, answer: oneshot::Sender<ApiResult<Value>>) {
        let id = self.new.project_id.clone();
        let c = NewClone {
            project_id: id.clone(),
            workspace_id: self.new.workspace_id.clone(),
            name: self.new.name.clone(),
            node_id: self.node.clone(),
            host: self.host.host.clone(),
            full_name: self.full_name.clone(),
            created_by: self.me.clone(),
        };
        if let Err(e) = self
            .m
            .inner
            .mirror
            .call(move |s| s.start_clone(&c, p::MAX_PROJECTS))
            .await
        {
            let _ = answer.send(Err(store_err(e)));
            return;
        }
        let sent = match self.lookup_and_send().await {
            Ok(s) => s,
            Err(e) => {
                let i = id.clone();
                if let Err(e) = self.m.inner.mirror.call(move |s| s.drop_clone(&i)).await {
                    tracing::warn!(error = %e, "a refused clone's row was not removed");
                }
                let _ = answer.send(Err(e));
                return;
            }
        };
        let i = id.clone();
        let row = self.m.inner.mirror.call(move |s| s.clone_of(&i)).await;
        let shown = match row {
            Ok(Some(r)) => pending_json(&r, &self.who),
            _ => None,
        };
        let _ = answer.send(Ok(shown.unwrap_or_else(|| json!({ "project_id": id }))));
        let ws = Audience::Workspace(self.new.workspace_id.clone());
        changed(&self.m, ws.clone());
        let ended = sent.answer().await;
        let m = self.m.clone();
        self.end(ended).await;
        changed(&m, ws);
    }

    /// The forge's word on the repository, then the message to the node.
    async fn lookup_and_send(&mut self) -> ApiResult<project_folder::Sent> {
        let m = &self.m;
        let h = &self.host;
        let _person = forge_repos::take_person(&m.inner.forge.listing_people, &self.me)
            .ok_or_else(|| {
                err(
                    StatusCode::TOO_MANY_REQUESTS,
                    "your earlier repository request is still running; try again when it has \
                     finished",
                )
            })?;
        let _slot = m.inner.forge.in_flight.try_acquire().map_err(|_| {
            err(
                StatusCode::TOO_MANY_REQUESTS,
                "this server is busy with repository requests; try again in a moment",
            )
        })?;
        m.forge_list_attempt(&self.me, &h.host)
            .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e))?;
        let token = match m.connection_token(&self.me, &self.connection_id).await {
            Ok(Some(t)) => t,
            Ok(None) => return Err(err(StatusCode::NOT_FOUND, "no such connection")),
            Err(e) => return Err(sealing_failed(e)),
        };
        let got = forge_repos::get_one(
            h,
            &token,
            &self.full_name,
            m.forge_timeout(),
            &m.forge_lookup(),
        )
        .await;
        let repo = match got {
            Ok(r) => r,
            Err(e) => {
                if forge_repos::host_at_fault(&e) {
                    m.forge_failed(&h.host);
                }
                tracing::info!(host = %h.host, error = %e, "a forge repository lookup failed");
                return Err(match e {
                    CheckError::Refused(host, code) => err(
                        StatusCode::BAD_GATEWAY,
                        format!(
                            "{host} refused this connection's token (HTTP {code}): it was revoked \
                             or has expired, lacks the scope to read the repository, or the forge \
                             is limiting the account; replace the token under Plugins → \
                             Connectors"
                        ),
                    ),
                    CheckError::Status(host, 404) => err(
                        StatusCode::NOT_FOUND,
                        format!("{host} has no such repository this connection can see"),
                    ),
                    e => err(StatusCode::BAD_GATEWAY, e.to_string()),
                });
            }
        };
        // The forge's own spelling, when it is the same name in another
        // case (GitHub's names are not case-sensitive); anything else (a
        // renamed or transferred repository the forge redirected to) is
        // refused.
        if !repo.full_name.eq_ignore_ascii_case(&self.full_name) {
            return Err(err(
                StatusCode::CONFLICT,
                format!(
                    "{} answered with another repository ({}): it was renamed or moved; choose it \
                     again",
                    h.host, repo.full_name
                ),
            ));
        }
        self.full_name = repo.full_name.clone();
        let (i, f) = (self.new.project_id.clone(), repo.full_name.clone());
        m.inner
            .mirror
            .call(move |s| s.set_clone_name(&i, &f))
            .await?;
        let (username, token) = if repo.private {
            (Some(self.login.clone()), Some(token))
        } else {
            drop(token);
            (None, None)
        };
        let src = CloneSource {
            host: h.host.clone(),
            full_name: repo.full_name,
            username,
            token,
        };
        project_folder::clone(m, &self.me, &self.node, &self.new.name, src)
            .map_err(super::folder::folder_err)
    }

    /// The node's answer: the project, or the row failed with the reason.
    async fn end(mut self, ended: Result<String, FolderError>) {
        let id = self.new.project_id.clone();
        let reason = match ended {
            Ok(path) => {
                let settings = ProjectSettings {
                    node_id: Some(self.node.clone()),
                    cwd: Some(path),
                    ..ProjectSettings::default()
                };
                self.new.settings = serde_json::to_string(&settings).unwrap_or_default();
                self.new.source = Some((self.host.host.clone(), self.full_name.clone()));
                let new = self.new;
                match self
                    .m
                    .inner
                    .mirror
                    .call(move |s| s.finish_clone(&new, p::MAX_PROJECTS))
                    .await
                {
                    Ok(_) => return,
                    Err(e) => {
                        let e = super::folder::orphaned(&settings, store_err(e));
                        tracing::warn!(error = %e.1, "a cloned project was not saved");
                        e.1
                    }
                }
            }
            Err(e) => e.to_string(),
        };
        let i = id.clone();
        if let Err(e) = self
            .m
            .inner
            .mirror
            .call(move |s| s.fail_clone(&i, &reason))
            .await
        {
            tracing::warn!(error = %e, "a failed clone was not recorded as failed");
        }
    }
}

/// Dismiss the caller's own failed clone: anyone else's, a running one or
/// an unknown id is 404 (a running one: 409, to its starter).
async fn dismiss(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let me = who
        .user_id
        .clone()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such failed clone"))?;
    let (i, u) = (id.clone(), me.clone());
    let row = m
        .inner
        .mirror
        .call(move |s| s.clone_of(&i))
        .await?
        .filter(|r| r.created_by == u)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such failed clone"))?;
    if row.failed.is_none() {
        return Err(err(
            StatusCode::CONFLICT,
            "this repository is still being cloned; it can be dismissed once it has failed",
        ));
    }
    let (i, u) = (id.clone(), me.clone());
    m.inner
        .mirror
        .call(move |s| s.dismiss_clone(&i, &u))
        .await?;
    changed(&m, Audience::Workspace(row.workspace_id));
    Ok(Json(json!({ "ok": true })))
}

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
//! leaves no half-made state), **before any row exists**: the duplicate
//! (an in-memory claim on person, node and repository taken by the request,
//! then a read of the table: the same repository twice is 409 however two
//! requests interleave, never a slot's 429 and never waiting for the first's
//! lookup), and the limits that need no forge call (the person's own clone
//! lookup slot, waited for up to 3 s, apart from the listing's; one of the
//! server's few forge slots; the host's failure budget; the person's rate),
//! so a request refused for them was never shown as cloning. Then the row as
//! `cloning` (refused, 409, by the table's own check or by the workspace's
//! project limit, which counts running clones); the token, opened only
//! through `Master::connection_token`, and one forge call, `GET` of that
//! repository by name, with the listing's client and rules; the message to
//! the node (refused at once when the node is away, or the clone budget,
//! apart from folder requests', is used up). Any refusal up to here removes
//! the row and is the request's answer; from here on the request has
//! answered 202 and the clone ends in the row. A `RowGuard` fails the row
//! with a plain reason if the job is dropped without ending it. When the node
//! answers, the starter's right is checked again (`why_not_now`) before the
//! project is written; a node's reason is cleaned and the token taken out of
//! it here, not trusted to the node. Why not a listing: GitHub has no search
//! by name, so a repository beyond its 1,000 most recently updated would not
//! be found, and one exact call answers what the clone needs (the name as the
//! forge spells it, whether it is private) with no paging. The token goes to
//! the node only when the forge says the repository is private; a public one
//! is cloned with no credential. The host sent is the connection's host,
//! never one the person names.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::delete;
use axum::{Json, Router};
use harness_core::{Secret, SecretSet};
use harness_store::NewProject;
use harness_store::project_clones::{CloneRow, NewClone};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::oneshot;

use super::{ApiError, ApiResult, changed, folder, store_err};
use crate::auth::Principal;
use crate::forge::{CheckError, ForgeHost};
use crate::forge_repos;
use crate::http::caller::User;
use crate::http::connections::{err, sealing_failed};
use crate::project_folder::{self, CloneSource, FolderError};
use crate::projects::{self as p, ProjectSettings};
use crate::workspaces::{self as ws, WsError};
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
    // The same repository twice at once (a double click, two tabs) is one
    // clone: the first request holds this until its row exists, the
    // second is told so.
    let claim = project_folder::claim(
        m,
        &format!(
            "clone:{me}:{node}:{}:{}",
            h.host,
            full_name.to_ascii_lowercase()
        ),
    )
    .ok_or_else(|| {
        err(
            StatusCode::CONFLICT,
            format!(
                "{}/{full_name} is being cloned onto this node for you already; it shows as \
                 cloning in your projects until it ends",
                h.host
            ),
        )
    })?;
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
        claim: Some(claim),
        secrets: SecretSet::default(),
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

/// How long a clone's lookup waits for the person's own earlier lookup
/// (another repository, started a moment before) to finish, before it is
/// refused 429. The same repository twice is refused 409 before any slot.
const SLOT_WAIT: Duration = Duration::from_secs(3);
/// The longest failure reason stored or shown, in characters (a refusal's
/// own text; an orphaned folder's is bounded by `folder::orphaned`).
const REASON_MAX: usize = 300;

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
    /// The same repository twice at once: held until the row exists.
    claim: Option<project_folder::Claim>,
    /// The token and its forms, once opened: taken out of whatever a node
    /// says before it is stored or shown.
    secrets: SecretSet,
}

/// The pending row, until the job has ended it: dropped for any other
/// reason (a panic, a store that failed twice), it fails the row with a
/// plain reason, so that nothing stays `cloning` until the next restart,
/// undismissable and holding a project slot.
struct RowGuard {
    m: Master,
    id: String,
    workspace: String,
    armed: bool,
}

impl RowGuard {
    fn new(m: &Master, id: &str, workspace: &str) -> Self {
        Self {
            m: m.clone(),
            id: id.to_string(),
            workspace: workspace.to_string(),
            armed: true,
        }
    }

    /// The row left `cloning` by the job's own hand.
    fn done(&mut self) {
        self.armed = false;
    }
}

/// What a clone that stopped without an answer says.
pub(super) const STOPPED: &str = "the server stopped this clone before the node answered. The \
     node may still have made its folder: if a new folder for it appears in that node's \
     projects folder, remove it, or open it with Existing folder";

impl Drop for RowGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // The store is synchronous: no task needed from a destructor.
        match self.m.inner.mirror.fail_clone(&self.id, STOPPED) {
            Ok(true) => changed(&self.m, Audience::Workspace(self.workspace.clone())),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "a stopped clone's row was not failed"),
        }
    }
}

/// The limits that need no forge call, held from before the row is made
/// until the repository lookup is done: the person's own lookup slot, a
/// server slot, and the person's rate.
struct Limits<'a> {
    _person: forge_repos::PersonSlot<'a>,
    _slot: tokio::sync::SemaphorePermit<'a>,
}

impl<'a> Limits<'a> {
    async fn take(m: &'a Master, me: &str, host: &str) -> ApiResult<Limits<'a>> {
        // Its own slot, apart from the listing's: a person's repository
        // listing in flight does not refuse their Create. Another clone's
        // lookup of theirs is waited for a moment, not refused at once.
        let start = tokio::time::Instant::now();
        let person = loop {
            if let Some(p) = forge_repos::take_person(&m.inner.forge.clone_people, me) {
                break p;
            }
            if start.elapsed() >= SLOT_WAIT {
                return Err(err(
                    StatusCode::TOO_MANY_REQUESTS,
                    "your earlier clone is still looking up its repository; try again in a moment",
                ));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let slot = m.inner.forge.in_flight.try_acquire().map_err(|_| {
            err(
                StatusCode::TOO_MANY_REQUESTS,
                "this server is busy with repository requests; try again in a moment",
            )
        })?;
        m.forge_list_attempt(me, host)
            .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e))?;
        Ok(Limits {
            _person: person,
            _slot: slot,
        })
    }
}

impl Job {
    async fn run(mut self, answer: oneshot::Sender<ApiResult<Value>>) {
        let id = self.new.project_id.clone();
        let m = self.m.clone();
        // Everything that needs no forge call comes before the row exists:
        // a request refused for it was never shown as cloning. The claim
        // (held since the request) makes the same repository twice at once
        // exactly one: the second is told it is cloning already (409), never
        // a slot's 429.
        let (u, n, h, f) = (
            self.me.clone(),
            self.node.clone(),
            self.host.host.clone(),
            self.full_name.clone(),
        );
        if let Err(e) = m
            .inner
            .mirror
            .call(move |s| s.check_not_cloning(&u, &n, &h, &f))
            .await
        {
            let _ = answer.send(Err(store_err(e)));
            return;
        }
        let limits = match Limits::take(&m, &self.me, &self.host.host).await {
            Ok(l) => l,
            Err(e) => {
                let _ = answer.send(Err(e));
                return;
            }
        };
        let c = NewClone {
            project_id: id.clone(),
            workspace_id: self.new.workspace_id.clone(),
            name: self.new.name.clone(),
            node_id: self.node.clone(),
            host: self.host.host.clone(),
            full_name: self.full_name.clone(),
            created_by: self.me.clone(),
        };
        if let Err(e) = m
            .inner
            .mirror
            .call(move |s| s.start_clone(&c, p::MAX_PROJECTS))
            .await
        {
            let _ = answer.send(Err(store_err(e)));
            return;
        }
        // The row exists: from here the database's own check refuses a
        // second one.
        self.claim = None;
        let mut guard = RowGuard::new(&m, &id, &self.new.workspace_id);
        let sent = match self.lookup_and_send().await {
            Ok(s) => s,
            Err(e) => {
                let i = id.clone();
                match m.inner.mirror.call(move |s| s.drop_clone(&i)).await {
                    Ok(()) => guard.done(),
                    Err(e) => tracing::warn!(error = %e, "a refused clone's row was not removed"),
                }
                let _ = answer.send(Err(e));
                return;
            }
        };
        drop(limits);
        let i = id.clone();
        let row = m.inner.mirror.call(move |s| s.clone_of(&i)).await;
        let shown = match row {
            Ok(Some(r)) => pending_json(&r, &self.who),
            _ => None,
        };
        let _ = answer.send(Ok(shown.unwrap_or_else(|| json!({ "project_id": id }))));
        let ws = Audience::Workspace(self.new.workspace_id.clone());
        changed(&m, ws.clone());
        let ended = sent.answer().await;
        if self.end(ended).await {
            guard.done();
        }
        changed(&m, ws);
    }

    /// The forge's word on the repository, then the message to the node.
    async fn lookup_and_send(&mut self) -> ApiResult<project_folder::Sent> {
        let m = &self.m;
        let h = &self.host;
        let token = match m.connection_token(&self.me, &self.connection_id).await {
            Ok(Some(t)) => t,
            Ok(None) => return Err(err(StatusCode::NOT_FOUND, "no such connection")),
            Err(e) => return Err(sealing_failed(e)),
        };
        self.secrets = secrets_of(&self.login, token.expose());
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

    /// Why the starter may not have the project any more, or `None` while
    /// they may: what the request checked for a new project, checked again
    /// with the account, membership and level as they are now (the clone ran
    /// up to the node's limit plus 60 s).
    async fn why_not_now(&self) -> Option<String> {
        let m = &self.m;
        let me = self.me.clone();
        let user = match m.inner.mirror.call(move |s| s.user_by_id(&me)).await {
            Ok(Some(u)) => u,
            Ok(None) => return Some("your account was removed while this was cloning".into()),
            Err(e) => return Some(store_err(e).1),
        };
        let who = Principal::from_user(user);
        let a = match ws::access(m, &who, &self.new.workspace_id).await {
            Ok(a) => a,
            Err(WsError::NotFound) => {
                return Some(
                    "you are no longer a member of this workspace, or it was removed, while \
                     this was cloning"
                        .into(),
                );
            }
            Err(e) => return Some(ApiError::from(e).1),
        };
        if !p::may_manage(&a) {
            return Some(
                "you may no longer change projects in this workspace: your role changed while \
                 this was cloning"
                    .into(),
            );
        }
        folder::may_make_on(m, &who, &a, &self.node)
            .err()
            .map(|e| e.1)
    }

    /// The node's answer: the project, or the row failed with the reason.
    /// Whether the row left `cloning` (false: nothing could be written; the
    /// guard tries once more).
    async fn end(mut self, ended: Result<String, FolderError>) -> bool {
        let id = self.new.project_id.clone();
        let reason = match ended {
            Ok(path) => {
                let settings = ProjectSettings {
                    node_id: Some(self.node.clone()),
                    cwd: Some(path),
                    ..ProjectSettings::default()
                };
                if let Some(why) = self.why_not_now().await {
                    folder::orphaned(&settings, ApiError(StatusCode::FORBIDDEN, why)).1
                } else {
                    self.new.settings = serde_json::to_string(&settings).unwrap_or_default();
                    self.new.source = Some((self.host.host.clone(), self.full_name.clone()));
                    let new = self.new.clone();
                    match self
                        .m
                        .inner
                        .mirror
                        .call(move |s| s.finish_clone(&new, p::MAX_PROJECTS))
                        .await
                    {
                        Ok(_) => return true,
                        Err(e) => {
                            let e = folder::orphaned(&settings, store_err(e));
                            tracing::warn!(error = %e.1, "a cloned project was not saved");
                            e.1
                        }
                    }
                }
            }
            // The node's own words: its credential, if it echoed it, taken
            // out, then cut.
            Err(FolderError::Refused(why)) => self
                .secrets
                .scrub(&project_folder::clean_text(&why))
                .chars()
                .take(REASON_MAX)
                .collect(),
            Err(e) => e.to_string(),
        };
        let i = id.clone();
        match self
            .m
            .inner
            .mirror
            .call(move |s| s.fail_clone(&i, &reason))
            .await
        {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(error = %e, "a failed clone was not recorded as failed");
                false
            }
        }
    }
}

/// The credential a clone sends and the forms it takes in a node's
/// message: itself, `login:token`, and the HTTP Basic header's base64 of
/// that (padded, unpadded, URL-safe).
fn secrets_of(login: &str, token: &str) -> SecretSet {
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE};
    let pair = format!("{login}:{token}");
    SecretSet::new(
        [
            token.to_string(),
            pair.clone(),
            STANDARD.encode(&pair),
            STANDARD_NO_PAD.encode(&pair),
            URL_SAFE.encode(&pair),
        ]
        .into_iter()
        .map(Secret::new)
        .collect(),
    )
}

/// Dismiss the caller's own failed clone: anyone else's, a running one or
/// an unknown id is 404 (a running one: 409, to its starter).
async fn dismiss(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Auth;
    use harness_core::Secret as Sec;
    use harness_store::{NewUser, Role, Store};

    /// A master with one running clone `c1` of user `u1`, and its workspace.
    fn master() -> (Master, String) {
        let store = Store::open_in_memory().unwrap();
        store
            .create_user(&NewUser {
                user_id: "u1".into(),
                username: "bo".into(),
                password_hash: "$argon2id$x".into(),
                role: Role::Member,
                exec_ceiling: harness_core::policy::ExecLevel::None,
                must_change_password: false,
            })
            .unwrap();
        let ws = store.create_workspace("A", "u1").unwrap().workspace_id;
        let m = Master::new(store, Auth::new(Sec::new("t".into()), vec![], false));
        // After the master started: it fails every running clone at start.
        m.inner
            .mirror
            .start_clone(
                &NewClone {
                    project_id: "c1".into(),
                    workspace_id: ws.clone(),
                    name: "P".into(),
                    node_id: "n1".into(),
                    host: "github.com".into(),
                    full_name: "ada/a".into(),
                    created_by: "u1".into(),
                },
                10,
            )
            .unwrap();
        (m, ws)
    }

    fn state(m: &Master) -> (bool, Option<String>) {
        let r = m.inner.mirror.clone_of("c1").unwrap().unwrap();
        (r.failed.is_some(), r.failed)
    }

    /// L4: a job that is dropped part-way (a panic, a task that ends, a store
    /// that failed twice) fails its row with a plain reason; one that ended
    /// the row itself leaves it as it made it.
    #[tokio::test]
    async fn a_job_dropped_part_way_fails_its_row() {
        let (m, ws) = master();
        drop(RowGuard::new(&m, "c1", &ws));
        let (failed, why) = state(&m);
        assert!(failed, "the row stayed cloning");
        assert!(why.unwrap().contains("stopped this clone"), "no reason");
    }

    #[tokio::test]
    async fn a_job_that_panics_fails_its_row() {
        let (m, ws) = master();
        let m2 = m.clone();
        let task = tokio::spawn(async move {
            let _guard = RowGuard::new(&m2, "c1", &ws);
            panic!("the job fell over");
        });
        assert!(task.await.is_err());
        assert!(state(&m).0, "the row stayed cloning after a panic");
    }

    #[tokio::test]
    async fn a_job_that_ended_its_row_leaves_it() {
        let (m, ws) = master();
        let mut g = RowGuard::new(&m, "c1", &ws);
        g.done();
        drop(g);
        assert!(!state(&m).0, "an ended job's guard failed the row");
    }

    #[test]
    fn the_credential_is_taken_out_in_each_form_it_may_take() {
        use base64::Engine as _;
        let token = "ghp_CANARYunit0000000000000000000000";
        let set = secrets_of("ada", token);
        for form in [
            token.to_string(),
            format!("ada:{token}"),
            base64::engine::general_purpose::STANDARD.encode(format!("ada:{token}")),
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(format!("ada:{token}")),
            base64::engine::general_purpose::URL_SAFE.encode(format!("ada:{token}")),
        ] {
            let said = set.scrub(&format!("failed: {form} twice {form}"));
            assert!(!said.contains(&form[form.len() / 2..]), "{form}: {said}");
            assert!(said.contains("[REDACTED]"), "{said}");
        }
    }
}

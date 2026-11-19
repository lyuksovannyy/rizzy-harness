//! A new project's folder cloned from a forge repository (ADOPTION_PLAN
//! X.27 step 3, PLAN.md T86): `POST /api/projects` with `folder_on`,
//! `connection_id` and `full_name`.
//!
//! In this order, so that nothing is opened or sent before it must be: the
//! caller is a person (the API token has no connections: 403, as on the
//! connection routes); the connection is the caller's own, looked up by
//! (caller, id), so anyone else's id is 404; its host is still allowed on
//! this server (else 409); the name has the shape that forge kind allows
//! (else 400: nothing else the person types becomes a URL); the node is
//! connected and announced `forge-clone` (an older node is never sent the
//! message: 409); the listing's limits (the person's one call in flight,
//! the server's few, the host's failure budget, the person's rate); then
//! the token, opened only through `Master::connection_token`, and one forge
//! call, `GET` of that repository by name, with the listing's client and
//! rules. Why not a listing: GitHub has no search by name, so a repository
//! beyond its 1,000 most recently updated would not be found, and one exact
//! call answers what the clone needs (the name as the forge spells it,
//! whether it is private) with no paging. The token goes to the node only
//! when the forge says the repository is private; a public one is cloned
//! with no credential. The host sent is the connection's host, never one
//! the person names.

use axum::http::StatusCode;

use super::ApiResult;
use crate::Master;
use crate::auth::Principal;
use crate::forge::CheckError;
use crate::forge_repos;
use crate::http::connections::{err, sealing_failed};
use crate::project_folder::{self, CloneSource};
use crate::projects::ProjectSettings;

/// The repository, as checked: what the node is sent, and what the project
/// keeps (host, owner/name).
pub(super) struct Checked {
    pub source: CloneSource,
}

impl Checked {
    pub fn recorded(&self) -> (String, String) {
        (self.source.host.clone(), self.source.full_name.clone())
    }
}

/// Check `who`'s request to clone `full_name` through their connection
/// `connection_id` onto `node` (see the module's order).
pub(super) async fn check(
    m: &Master,
    who: &Principal,
    connection_id: &str,
    full_name: &str,
    node: &str,
) -> ApiResult<Checked> {
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
    let _person =
        forge_repos::take_person(&m.inner.forge.listing_people, &me).ok_or_else(|| {
            err(
                StatusCode::TOO_MANY_REQUESTS,
                "your earlier repository request is still running; try again when it has finished",
            )
        })?;
    let _slot = m.inner.forge.in_flight.try_acquire().map_err(|_| {
        err(
            StatusCode::TOO_MANY_REQUESTS,
            "this server is busy with repository requests; try again in a moment",
        )
    })?;
    m.forge_list_attempt(&me, &h.host)
        .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e))?;
    let token = match m.connection_token(&me, connection_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(err(StatusCode::NOT_FOUND, "no such connection")),
        Err(e) => return Err(sealing_failed(e)),
    };
    let got =
        forge_repos::get_one(&h, &token, full_name, m.forge_timeout(), &m.forge_lookup()).await;
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
                        "{host} refused this connection's token (HTTP {code}): it was revoked or \
                         has expired, lacks the scope to read the repository, or the forge is \
                         limiting the account; replace the token under Plugins → Connectors"
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
    // The forge's own spelling, when it is the same name in another case
    // (GitHub's names are not case-sensitive); anything else (a renamed or
    // transferred repository the forge redirected to) is refused.
    if !repo.full_name.eq_ignore_ascii_case(full_name) {
        return Err(err(
            StatusCode::CONFLICT,
            format!(
                "{} answered with another repository ({}): it was renamed or moved; choose it again",
                h.host, repo.full_name
            ),
        ));
    }
    let (username, token) = if repo.private {
        (Some(row.login.clone()), Some(token))
    } else {
        drop(token);
        (None, None)
    };
    Ok(Checked {
        source: CloneSource {
            host: h.host.clone(),
            full_name: repo.full_name,
            username,
            token,
        },
    })
}

/// Ask `node` to clone `c` into a new folder named after `name`; the
/// settings that record it. Call after [`check`].
pub(super) async fn clone_on(
    m: &Master,
    who: &Principal,
    node: &str,
    name: &str,
    c: Checked,
    mut settings: ProjectSettings,
) -> ApiResult<ProjectSettings> {
    let user = who.user_id.as_deref().unwrap_or("api token");
    let path = project_folder::clone(m, user, node, name, c.source)
        .await
        .map_err(super::folder::folder_err)?;
    settings.node_id = Some(node.to_string());
    settings.cwd = Some(path);
    Ok(settings)
}

//! `GET /api/connections/{id}/repositories?q=<search>&page=<n>` (ADOPTION_PLAN
//! X.27 step 2, PLAN.md T83; the forge side is `crate::forge_repos`): one
//! page of the repositories the caller's own connection `id` can see.
//!
//! In this order, so that nothing is opened or sent before it must be:
//! the caller (a person: the API token is refused, as on every connection
//! route); the query (`q` and `page` only, each once); the connection,
//! looked up by (caller, id), so anyone else's id is 404; its host still
//! allowed (else 409, no call); a cached answer of the same (person,
//! connection, host API, sealed token, word, page); the limits (the host's shared failure
//! budget, then the person's listing limit); then the token, opened only
//! through `Master::connection_token`, which proves the sealing key first
//! (a missing or wrong key is 503 before any request). A forge that refuses
//! the token (401, 403) gets a message saying to replace it; the connection
//! is kept. Listing is a read: no audit line, as for the other reads.

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::ApiResult;
use super::connections::{err, person, sealing_failed};
use crate::Master;
use crate::forge::CheckError;
use crate::forge_repos::{self, Key};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/connections/{id}/repositories", get(list))
}

/// `q` and `page`, each at most once, nothing else; the message quotes
/// nothing of the query.
fn parse_query(raw: Option<&str>) -> Result<(Option<String>, u32), &'static str> {
    let (mut q, mut page) = (None, None);
    if let Some(raw) = raw.filter(|r| !r.is_empty()) {
        let u = reqwest::Url::parse(&format!("http://q/?{raw}"))
            .map_err(|_| "the query must be q and page only")?;
        for (k, v) in u.query_pairs() {
            let slot = match k.as_ref() {
                "q" => &mut q,
                "page" => &mut page,
                _ => return Err("the query takes q (a search word) and page only"),
            };
            if slot.replace(v.into_owned()).is_some() {
                return Err("q and page may each be given once");
            }
        }
    }
    let q = forge_repos::clean_query(q.as_deref())?;
    let page = forge_repos::parse_page(page.as_deref())?;
    Ok((q, page))
}

async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult<Json<Value>> {
    let me = person(&m, &headers).await?;
    let (q, page) = parse_query(raw.as_deref()).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let (u, i) = (me.clone(), id.clone());
    let (row, sealed) = m
        .inner
        .mirror
        .call(move |s| {
            Ok::<_, harness_store::StoreError>(match s.connection(&u, &i)? {
                Some(r) => s.connection_sealed(&u, &i)?.map(|b| (r, b)),
                None => None,
            })
        })
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connection"))?;
    // The cache's key for this token: a hash of the ciphertext, never the
    // token (nothing is opened here).
    let sealed: [u8; 32] = Sha256::digest(&sealed).into();
    let h = m
        .forge_hosts()
        .into_iter()
        .find(|h| h.host == row.host)
        .ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "this server no longer allows that forge host, so its repositories are not listed",
            )
        })?;
    let key = Key {
        user: me.clone(),
        connection: id.clone(),
        api_base: h.api_base.clone(),
        sealed,
        q: q.clone(),
        page,
    };
    if let Some(l) = m.inner.forge.repos.get(&key) {
        return Ok(Json(serde_json::to_value(l).unwrap_or_default()));
    }
    m.forge_list_attempt(&me, &h.host)
        .map_err(|e| err(StatusCode::TOO_MANY_REQUESTS, e))?;
    let token = match m.connection_token(&me, &id).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(err(StatusCode::NOT_FOUND, "no such connection")),
        Err(e) => return Err(sealing_failed(e)),
    };
    let got = forge_repos::list(
        &h,
        &token,
        &row.account_id,
        q.as_deref(),
        page,
        m.forge_timeout(),
        &m.forge_lookup(),
    )
    .await;
    drop(token);
    let l = got.map_err(|e| {
        m.forge_failed(&h.host);
        tracing::info!(host = %h.host, error = %e, "a forge repository listing failed");
        err(
            StatusCode::BAD_GATEWAY,
            match e {
                CheckError::Refused(host, code) => format!(
                    "{host} refused this connection's token (HTTP {code}): it was revoked or has \
                     expired, lacks the scope to list repositories, or the forge is limiting the \
                     account; replace the token under Plugins → Connectors"
                ),
                e => e.to_string(),
            },
        )
    })?;
    m.inner.forge.repos.put(key, l.clone());
    Ok(Json(serde_json::to_value(l).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::parse_query;

    #[test]
    fn the_query_takes_q_and_page_once() {
        assert_eq!(parse_query(None), Ok((None, 1)));
        assert_eq!(parse_query(Some("")), Ok((None, 1)));
        assert_eq!(
            parse_query(Some("q=a%26b+c&page=2")),
            Ok((Some("a&b c".into()), 2))
        );
        for bad in [
            "x=1",
            "q=a&q=b",
            "page=1&page=1",
            "q=%FF",
            "page=0",
            "q=a%07",
        ] {
            assert!(parse_query(Some(bad)).is_err(), "{bad}");
        }
    }
}

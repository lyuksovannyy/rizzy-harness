//! Opening a pull request (GitLab: a merge request) for an agent's
//! `pull_request` call (ADOPTION_PLAN X.27, PLAN.md T102,
//! `docs/forge-pr.md`): a built-in service the master answers for the node
//! that runs the session (PLAN.md §5). The token never leaves the master.
//!
//! **What is created** is the input of the one `tool_call_started` the
//! approval names, as the master's own mirror holds it: the event the page
//! drew the card from. The node's request names the approval and nothing
//! else, so a lying node cannot show one text and submit another. The
//! release checks are push's ([`crate::forge_push::release`]): the Allow
//! the master relayed from the chat's owner for that approval id, once,
//! the approval and its call both `pull_request` and each id unique in the
//! log, and the chat, turn, agent switch, owner, project source, host and
//! connection as they are now; then [`PER_HOUR`] per chat.
//!
//! **Where:** only the project's recorded source repository; head and base
//! are branches of it (no fork, no other repository). A missing base is
//! the forge's default branch, asked with one GET after the Allow. One POST
//! with the listing's client rules (https, public addresses unless the host
//! entry says otherwise, same-origin redirects, no proxy, the call's time
//! limit, [`MAX_ANSWER`] bytes). The forge's error text is bounded,
//! cleaned and free of the token.

use harness_core::SessionId;
use harness_core::forge::{PR_TOOL, PrRequest, parse_pr};
use harness_protocol::MasterToNode;

use crate::Master;
use crate::forge::{CheckError, ForgeHost, ForgeKind};

/// Pull requests one chat may open in any hour.
pub const PER_HOUR: usize = 5;

/// The most bytes of a forge's answer read.
pub const MAX_ANSWER: usize = 256 * 1024;

/// The longest error text passed on, in characters.
const ERROR_MAX: usize = 300;

/// What was opened, or found already open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub number: Option<u64>,
    pub url: Option<String>,
    pub existed: bool,
}

/// Answer a node's `forge.pull_request` from a task of its own; the answer
/// goes to that node only.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, approval_id: String) {
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let a = answer(&m, &node_id, sid, &approval_id).await;
        if let Err(e) = &a {
            tracing::info!(node = %node_id, session = %sid, why = %e, "pull request not opened");
        }
        let msg = match a {
            Ok(o) => MasterToNode::PullRequestResult {
                req_id,
                number: o.number,
                url: o.url,
                existed: o.existed,
                error: None,
            },
            Err(e) => MasterToNode::PullRequestResult {
                req_id,
                number: None,
                url: None,
                existed: false,
                error: Some(e),
            },
        };
        m.send_unacked(&node_id, msg);
    });
}

/// The pull request of approval `approval_id`, or why not.
pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    approval_id: &str,
) -> Result<Opened, String> {
    let r = crate::forge_push::release(
        m,
        node_id,
        sid,
        approval_id,
        PR_TOOL,
        harness_protocol::CAP_FORGE_PR,
        |f| f.pull_requests,
        PER_HOUR,
    )
    .await?;
    // The content: the mirror's input of the card's one call, by the same
    // rule the node applied before its card.
    let pr = parse_pr(&r.input)?;
    let host = m
        .forge_hosts()
        .into_iter()
        .find(|h| h.host == r.target.host)
        .ok_or("this server no longer allows that forge host")?;
    let token = m
        .connection_token(&r.owner, &r.target.connection_id)
        .await
        .map_err(|_| "the server's sealed storage could not open the token".to_string())?
        .ok_or("the connection was removed")?;
    let scrub = |s: &str| {
        let t = token.expose();
        let cleaned = if t.is_empty() {
            s.to_string()
        } else {
            s.replace(t.as_str(), "[REDACTED]")
        };
        harness_core::text::clean_line(&cleaned, ERROR_MAX)
    };
    let opened = open(m, &host, &token, &r.target.full_name, &pr)
        .await
        .map_err(|e| scrub(&e))?;
    crate::levels::audit(
        m,
        "forge.pull_request",
        serde_json::json!({
            "user_id": r.owner,
            "session_id": sid,
            "connection_id": r.target.connection_id,
            "host": r.target.host,
            "full_name": r.target.full_name,
            "head": pr.head,
            "number": opened.number,
            "existed": opened.existed,
        }),
    )
    .await;
    Ok(Opened {
        url: opened.url.map(|u| scrub(&u)),
        ..opened
    })
}

/// The base branch, asked of the forge when the call named none.
async fn base_of(
    m: &Master,
    h: &ForgeHost,
    token: &harness_core::Secret<String>,
    full_name: &str,
    pr: &PrRequest,
) -> Result<String, String> {
    if let Some(b) = &pr.base {
        return Ok(b.clone());
    }
    let repo =
        crate::forge_repos::get_one(h, token, full_name, m.forge_timeout(), &m.forge_lookup())
            .await
            .map_err(|e| format!("asking the forge for the default branch: {e}"))?;
    let b = repo
        .default_branch
        .ok_or("the forge names no default branch: give `base`")?;
    if b == pr.head {
        return Err("`head` is the repository's default branch: give another `base`".into());
    }
    Ok(b)
}

/// The one POST, its answer read.
async fn open(
    m: &Master,
    h: &ForgeHost,
    token: &harness_core::Secret<String>,
    full_name: &str,
    pr: &PrRequest,
) -> Result<Opened, String> {
    let base = base_of(m, h, token, full_name, pr).await?;
    let mut url = crate::forge_repos::repo_url(h, full_name)
        .ok_or("the repository name is not one this server takes")?;
    let body = match h.kind {
        ForgeKind::Github => serde_json::json!({
            "title": pr.title, "head": pr.head, "base": base, "body": pr.body, "draft": pr.draft,
        }),
        ForgeKind::Gitea | ForgeKind::Forgejo => serde_json::json!({
            // Gitea and Forgejo mark a draft by its title.
            "title": if pr.draft { format!("WIP: {}", pr.title) } else { pr.title.clone() },
            "head": pr.head, "base": base, "body": pr.body,
        }),
        ForgeKind::Gitlab => serde_json::json!({
            "title": if pr.draft { format!("Draft: {}", pr.title) } else { pr.title.clone() },
            "source_branch": pr.head, "target_branch": base, "description": pr.body,
        }),
    };
    url.path_segments_mut()
        .map_err(|_| "bad api_base")?
        .push(if h.kind == ForgeKind::Gitlab {
            "merge_requests"
        } else {
            "pulls"
        });
    let c =
        crate::forge::client(h, m.forge_timeout(), &m.forge_lookup()).map_err(|e| e.to_string())?;
    let headers =
        crate::forge::auth_headers(h.kind, token).map_err(|_| "the token cannot be sent")?;
    let (status, answer) =
        match crate::forge::post_json(&c, h, url, headers, &body, MAX_ANSWER).await {
            Ok(x) => x,
            Err(CheckError::Refused(host, code)) => {
                return Err(format!(
                    "{host} refused the token (HTTP {code}): it may lack the scope to open pull \
                     requests (GitHub: Pull requests read and write; GitLab: api; Gitea and \
                     Forgejo: write on repository)"
                ));
            }
            Err(e) => return Err(e.to_string()),
        };
    let v: Option<serde_json::Value> = serde_json::from_slice(&answer).ok();
    if (200..300).contains(&status) {
        let v = v.ok_or("the forge's answer is not JSON")?;
        return Ok(Opened {
            number: number_of(h.kind, &v),
            url: url_of(h.kind, &v),
            existed: false,
        });
    }
    let said = v.as_ref().map(message_of).unwrap_or_default();
    if matches!(status, 409 | 422) && said.to_ascii_lowercase().contains("already exist") {
        return Ok(Opened {
            number: said
                .split('!')
                .nth(1)
                .or_else(|| said.split('#').nth(1))
                .and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|n| n.parse().ok()),
            url: None,
            existed: true,
        });
    }
    Err(if said.is_empty() {
        format!("{} answered HTTP {status}", h.host)
    } else {
        format!("{} answered HTTP {status}: {said}", h.host)
    })
}

/// The pull request's number in the forge's answer.
fn number_of(kind: ForgeKind, v: &serde_json::Value) -> Option<u64> {
    let key = if kind == ForgeKind::Gitlab {
        "iid"
    } else {
        "number"
    };
    v.get(key).and_then(|n| n.as_u64())
}

/// Its web address: a http(s) URL of at most 512 bytes, else none.
fn url_of(kind: ForgeKind, v: &serde_json::Value) -> Option<String> {
    let key = if kind == ForgeKind::Gitlab {
        "web_url"
    } else {
        "html_url"
    };
    let s = v.get(key)?.as_str()?;
    let u = reqwest::Url::parse(s).ok()?;
    (matches!(u.scheme(), "https" | "http") && s.len() <= 512).then(|| u.to_string())
}

/// The forge's words for a refusal: `message` (a string, or GitLab's list)
/// and the messages of `errors`, joined, at most a few kilobytes.
fn message_of(v: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    match v.get("message") {
        Some(serde_json::Value::String(s)) => parts.push(s.clone()),
        Some(serde_json::Value::Array(a)) => {
            parts.extend(a.iter().filter_map(|x| x.as_str()).map(str::to_string))
        }
        _ => {}
    }
    if let Some(serde_json::Value::Array(errs)) = v.get("errors") {
        for e in errs.iter().take(5) {
            if let Some(s) = e.get("message").and_then(|m| m.as_str()) {
                parts.push(s.to_string());
            } else if let Some(s) = e.as_str() {
                parts.push(s.to_string());
            }
        }
    }
    let joined = parts.join("; ");
    joined.chars().take(4096).collect()
}

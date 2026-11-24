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
    let title = draft_title(h.kind, &pr.title, pr.draft)?;
    let body = match h.kind {
        ForgeKind::Github => serde_json::json!({
            "title": title, "head": pr.head, "base": base, "body": pr.body, "draft": pr.draft,
        }),
        ForgeKind::Gitea | ForgeKind::Forgejo => serde_json::json!({
            "title": title, "head": pr.head, "base": base, "body": pr.body,
        }),
        ForgeKind::Gitlab => serde_json::json!({
            "title": title, "source_branch": pr.head, "target_branch": base, "description": pr.body,
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
            url: url_of(h, &v),
            existed: false,
        });
    }
    let said = v.as_ref().map(message_of).unwrap_or_default();
    if matches!(status, 409 | 422) && said.to_ascii_lowercase().contains("already exist") {
        // The open one, with one bounded lookup where the forge has one.
        if let Some(found) = existing(&c, h, token, full_name, &pr.head, &base).await {
            return Ok(found);
        }
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

/// The longest address passed on, in bytes: within [`ERROR_MAX`], so the
/// cleaning of the answer never cuts it.
const URL_MAX: usize = ERROR_MAX;

/// Its web address, or none (the result then names the number only;
/// nothing a forge writes is trusted to point elsewhere): https, no user
/// name or password, on the project's own source host (as the URL parser
/// writes it: lower case; a trailing dot is another name), on the default
/// port or the port the host entry's `api_base` names on that same host (a
/// self-hosted forge on `:3000` serves its pages there too; `[forge] hosts`
/// names carry no port), at most [`URL_MAX`] bytes as written.
fn url_of(h: &ForgeHost, v: &serde_json::Value) -> Option<String> {
    let key = if h.kind == ForgeKind::Gitlab {
        "web_url"
    } else {
        "html_url"
    };
    let u = reqwest::Url::parse(v.get(key)?.as_str()?).ok()?;
    let port_ok = match u.port() {
        None => true,
        Some(p) => reqwest::Url::parse(&h.api_base)
            .is_ok_and(|a| a.host_str() == Some(h.host.as_str()) && a.port() == Some(p)),
    };
    let out = u.to_string();
    (u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.host_str() == Some(h.host.as_str())
        && port_ok
        && out.len() <= URL_MAX)
        .then_some(out)
}

/// The longest title a forge takes where a draft is marked by the title
/// (GitLab, Gitea, Forgejo: 255 characters).
const PREFIXED_TITLE_MAX: usize = 255;

/// The title sent: GitHub has a `draft` field; GitLab marks a draft by a
/// `Draft: ` prefix and Gitea and Forgejo by `WIP: `, added once (not when
/// the title already has one) and refused rather than cut when the title
/// would then pass the forge's limit.
fn draft_title(kind: ForgeKind, title: &str, draft: bool) -> Result<String, String> {
    let prefix = match kind {
        ForgeKind::Github => return Ok(title.to_string()),
        ForgeKind::Gitlab => "Draft: ",
        ForgeKind::Gitea | ForgeKind::Forgejo => "WIP: ",
    };
    let lower = title.to_ascii_lowercase();
    let marked = ["draft:", "[draft]", "(draft)", "wip:", "[wip]"]
        .iter()
        .any(|p| lower.starts_with(p));
    let out = if draft && !marked {
        format!("{prefix}{title}")
    } else {
        title.to_string()
    };
    if out.chars().count() > PREFIXED_TITLE_MAX {
        return Err(format!(
            "the title is too long for this forge (at most {PREFIXED_TITLE_MAX} characters, the \
             draft mark `{}` included); nothing was opened",
            prefix.trim()
        ));
    }
    Ok(out)
}

/// The open pull request for `head` into `base`, by one bounded GET where
/// the forge has such a query (GitHub; GitLab); none for Gitea and Forgejo.
async fn existing(
    c: &reqwest::Client,
    h: &ForgeHost,
    token: &harness_core::Secret<String>,
    full_name: &str,
    head: &str,
    base: &str,
) -> Option<Opened> {
    let mut url = crate::forge_repos::repo_url(h, full_name)?;
    match h.kind {
        ForgeKind::Github => {
            let owner = full_name.split('/').next()?;
            url.path_segments_mut().ok()?.push("pulls");
            url.query_pairs_mut()
                .append_pair("head", &format!("{owner}:{head}"))
                .append_pair("base", base)
                .append_pair("state", "open");
        }
        ForgeKind::Gitlab => {
            url.path_segments_mut().ok()?.push("merge_requests");
            url.query_pairs_mut()
                .append_pair("source_branch", head)
                .append_pair("target_branch", base)
                .append_pair("state", "opened");
        }
        ForgeKind::Gitea | ForgeKind::Forgejo => return None,
    }
    let headers = crate::forge::auth_headers(h.kind, token).ok()?;
    let (_, body) = crate::forge::get_url(c, h, url, headers, MAX_ANSWER)
        .await
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let first = v.as_array()?.first()?;
    Some(Opened {
        number: number_of(h.kind, first),
        url: url_of(h, first),
        existed: true,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_draft_mark_is_added_once_and_never_passes_the_limit() {
        assert_eq!(draft_title(ForgeKind::Github, "t", true).unwrap(), "t");
        assert_eq!(
            draft_title(ForgeKind::Gitlab, "t", true).unwrap(),
            "Draft: t"
        );
        assert_eq!(
            draft_title(ForgeKind::Gitlab, "Draft: t", true).unwrap(),
            "Draft: t"
        );
        assert_eq!(draft_title(ForgeKind::Gitea, "t", true).unwrap(), "WIP: t");
        assert_eq!(
            draft_title(ForgeKind::Forgejo, "WIP: t", true).unwrap(),
            "WIP: t"
        );
        assert_eq!(draft_title(ForgeKind::Gitlab, "t", false).unwrap(), "t");
        let long = "t".repeat(250);
        assert!(draft_title(ForgeKind::Gitlab, &long, true).is_err());
        assert!(draft_title(ForgeKind::Gitea, &long, false).is_ok());
        assert!(draft_title(ForgeKind::Github, &"t".repeat(256), true).is_ok());
        assert!(draft_title(ForgeKind::Gitlab, &"t".repeat(256), false).is_err());
    }

    /// (Review R5) The address passed on: https, the source host, the
    /// default port or `api_base`'s on that host, no user info, at most
    /// 300 bytes; anything else, the number alone.
    #[test]
    fn only_an_https_address_on_the_source_host_is_passed_on() {
        let h = |api_base: &str| ForgeHost {
            host: "git.example.com".into(),
            kind: ForgeKind::Gitea,
            api_base: api_base.into(),
            api_host: None,
            private_address: false,
        };
        let plain = h("https://git.example.com/api/v1");
        let on3000 = h("https://git.example.com:3000/api/v1");
        let of = |h: &ForgeHost, u: &str| url_of(h, &serde_json::json!({ "html_url": u }));
        let ok = "https://git.example.com/o/r/pulls/1";
        assert_eq!(of(&plain, ok).as_deref(), Some(ok));
        assert_eq!(
            of(&plain, "https://git.example.com:443/o/r/pulls/1").as_deref(),
            Some(ok),
            "the default port written out"
        );
        assert_eq!(
            of(&plain, "https://GIT.Example.COM/o/r/pulls/1").as_deref(),
            Some(ok),
            "the same host in upper case, passed on as the parser writes it"
        );
        for bad in [
            "http://git.example.com/o/r/pulls/1",
            "https://git.example.com:8443/o/r/pulls/1",
            "https://user@git.example.com/o/r/pulls/1",
            "https://user:pw@git.example.com/o/r/pulls/1",
            "https://git.example.com./o/r/pulls/1",
            "https://git.example.com.evil.example/o/r/pulls/1",
            "https://evil.example/git.example.com/o/r/pulls/1",
            "javascript:alert(1)",
            "/o/r/pulls/1",
        ] {
            assert_eq!(of(&plain, bad), None, "{bad}");
        }
        let long = format!("https://git.example.com/o/r/pulls/1?{}", "x".repeat(400));
        assert_eq!(of(&plain, &long), None, "a 400-character address");
        let fits = format!(
            "https://git.example.com/{}",
            "x".repeat(URL_MAX - "https://git.example.com/".len())
        );
        assert_eq!(of(&plain, &fits).as_deref(), Some(fits.as_str()));
        // The port the host entry's API is on, on that same host.
        let at3000 = "https://git.example.com:3000/o/r/pulls/1";
        assert_eq!(of(&on3000, at3000).as_deref(), Some(at3000));
        assert_eq!(
            of(&on3000, "https://git.example.com:3001/o/r/pulls/1"),
            None
        );
        assert_eq!(of(&plain, at3000), None);
        // GitLab's key is `web_url`.
        let gl = ForgeHost {
            kind: ForgeKind::Gitlab,
            ..h("https://git.example.com/api/v4")
        };
        assert_eq!(url_of(&gl, &serde_json::json!({ "html_url": ok })), None);
        assert_eq!(
            url_of(&gl, &serde_json::json!({ "web_url": ok })).as_deref(),
            Some(ok)
        );
    }
}

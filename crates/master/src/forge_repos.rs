//! Listing the repositories one of a person's forge connections can see
//! (ADOPTION_PLAN X.27 step 2, PLAN.md T83, README "Connections"), for the
//! Project popup of step 3. `GET /api/connections/{id}/repositories` is the
//! route (`http/connection_repos.rs`); this file is the forge side.
//!
//! * **The request** is built here from the entry's `api_base`, a fixed
//!   path and fixed parameters; the search word is one encoded query value
//!   and the page a number from 1 to [`MAX_PAGE`]. Nothing the forge
//!   returns is ever requested: a `Link` header is read only for "is there
//!   a next page".
//! * **Per forge** (from each forge's own API documentation):
//!   GitHub `GET /user/repos` (owner, collaborator, organisation member;
//!   sorted by `updated`); it has no search parameter, so with a search
//!   word one forge page of [`GITHUB_SEARCH_PER_PAGE`] is read and filtered
//!   here by name (`search: "within_page"`), and the word never reaches
//!   GitHub. GitLab `GET /projects?membership=true` with `search`, sorted by
//!   `last_activity_at`. Gitea and Forgejo `GET /repos/search` with `uid`
//!   (the account's id: repositories it owns or contributes to),
//!   `private=true` and `q`, sorted by `updated`.
//! * **The answer** is untrusted: each entry must have a numeric id, an
//!   `owner/name` of the shape that forge allows and a visibility, or it is
//!   left out and counted; the default branch must be a safe ref name, the
//!   update time a strict timestamp and "may push" a boolean the forge sent,
//!   or that field is empty; the description is cleaned and cut. No URL
//!   from the forge is passed on.
//! * **Bounds.** The shared client (`crate::forge::client`), [`MAX_BODY`],
//!   the call's time limit; per person [`LIST_PER_USER`] forge calls per
//!   [`LIST_WINDOW`], and the forge host's shared failures-only budget; a
//!   short [`Cache`] per (person, connection, token, word, page).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use harness_core::Secret;
use reqwest::header::HeaderMap;
use serde::Serialize;
use serde_json::Value;

use crate::forge::{CheckError, ForgeHost, ForgeKind};
use crate::lock;
use crate::remote::Lookup;

/// Repositories per page of ours.
pub const PER_PAGE: u32 = 30;
/// The GitHub forge page read for a search word (its maximum).
pub const GITHUB_SEARCH_PER_PAGE: u32 = 100;
/// The last page served: at most 300 repositories (1,000 scanned with a
/// GitHub search) are reachable by paging; a search word finds more.
pub const MAX_PAGE: u32 = 10;
/// The longest search word, in characters.
pub const MAX_QUERY: usize = 100;
/// The most of one listing answer read: 100 GitHub repositories are about
/// half of it.
pub const MAX_BODY: usize = 1024 * 1024;
pub const MAX_DESCRIPTION: usize = 200;
/// `owner/name` (GitLab: `group/…/name`) in bytes.
pub const MAX_FULL_NAME: usize = 512;
pub const MAX_REF: usize = 255;
/// Forge calls one person may make per [`LIST_WINDOW`] (answers from the
/// cache are free).
pub const LIST_PER_USER: usize = 20;
pub const LIST_WINDOW: Duration = Duration::from_secs(60);
/// How long an answer is kept.
pub const CACHE_TTL: Duration = Duration::from_secs(60);
pub const CACHE_ENTRIES: usize = 256;
pub const CACHE_PER_USER: usize = 32;
pub const CACHE_BYTES: usize = 4 * 1024 * 1024;
/// GitLab's Developer role, the first that may push (GitLab's permissions
/// documentation); protected branches can still refuse it.
const GITLAB_DEVELOPER: u64 = 30;

/// One repository, cleaned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Repo {
    /// The forge's id, as a string (numbers above 2^53 lose precision in
    /// JavaScript).
    pub id: String,
    pub full_name: String,
    /// Not public (GitLab's `internal` counts as private).
    pub private: bool,
    pub default_branch: Option<String>,
    pub description: Option<String>,
    pub updated_ms: Option<i64>,
    /// Whether the account may push, where the forge says so; the token's
    /// own scopes can still refuse a push.
    pub push: Option<bool>,
}

/// How a search word was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Search {
    None,
    /// The forge searched.
    Forge,
    /// GitHub: matches within the forge's page, filtered here.
    WithinPage,
}

/// One page of the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub repositories: Vec<Repo>,
    pub page: u32,
    pub has_more: bool,
    /// Entries the forge sent that were not passed on: malformed, twice,
    /// or beyond the page.
    pub left_out: u32,
    pub search: Search,
}

/// The search word: trimmed, empty is none; refused with control, bidi or
/// invisible characters, or longer than [`MAX_QUERY`].
pub fn clean_query(raw: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(q) = raw.map(str::trim).filter(|q| !q.is_empty()) else {
        return Ok(None);
    };
    if q.chars().count() > MAX_QUERY {
        return Err("the search word is too long (at most 100 characters)");
    }
    if q.chars()
        .any(|c| crate::remote::hidden(c) || c == '\u{FFFD}')
    {
        return Err("the search word has control or invisible characters");
    }
    Ok(Some(q.to_string()))
}

/// `page`: absent is 1; else ASCII digits with no leading zero, 1 to
/// [`MAX_PAGE`].
pub fn parse_page(raw: Option<&str>) -> Result<u32, &'static str> {
    const BAD: &str = "page must be a whole number from 1 to 10";
    let Some(p) = raw else { return Ok(1) };
    if p.is_empty() || p.starts_with('0') || !p.bytes().all(|b| b.is_ascii_digit()) {
        return Err(BAD);
    }
    match p.parse::<u32>() {
        Ok(n) if (1..=MAX_PAGE).contains(&n) => Ok(n),
        _ => Err(BAD),
    }
}

/// The request for `page` of `h`'s repositories as the account
/// `account_id` sees them, with `q`: the URL (from `api_base`), the forge
/// page size, and how the word is applied.
pub fn request_url(
    h: &ForgeHost,
    account_id: &str,
    q: Option<&str>,
    page: u32,
) -> Option<(reqwest::Url, u32, Search)> {
    let path = match h.kind {
        ForgeKind::Github => "/user/repos",
        ForgeKind::Gitlab => "/projects",
        ForgeKind::Gitea | ForgeKind::Forgejo => "/repos/search",
    };
    let mut u = reqwest::Url::parse(&format!("{}{path}", h.api_base)).ok()?;
    let page_s = page.to_string();
    let (per_page, search) = match h.kind {
        ForgeKind::Github if q.is_some() => (GITHUB_SEARCH_PER_PAGE, Search::WithinPage),
        _ if q.is_some() => (PER_PAGE, Search::Forge),
        _ => (PER_PAGE, Search::None),
    };
    let per_s = per_page.to_string();
    {
        let mut qp = u.query_pairs_mut();
        match h.kind {
            ForgeKind::Github => {
                qp.append_pair("visibility", "all")
                    .append_pair("affiliation", "owner,collaborator,organization_member")
                    .append_pair("sort", "updated")
                    .append_pair("direction", "desc")
                    .append_pair("per_page", &per_s)
                    .append_pair("page", &page_s);
            }
            ForgeKind::Gitlab => {
                qp.append_pair("membership", "true")
                    .append_pair("order_by", "last_activity_at")
                    .append_pair("sort", "desc")
                    .append_pair("per_page", &per_s)
                    .append_pair("page", &page_s);
                if let Some(q) = q {
                    qp.append_pair("search", q);
                }
            }
            ForgeKind::Gitea | ForgeKind::Forgejo => {
                // The stored account id was a JSON number when it was kept.
                if account_id.is_empty() || !account_id.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                qp.append_pair("uid", account_id)
                    .append_pair("private", "true")
                    .append_pair("sort", "updated")
                    .append_pair("order", "desc")
                    .append_pair("limit", &per_s)
                    .append_pair("page", &page_s);
                if let Some(q) = q {
                    qp.append_pair("q", q);
                }
            }
        }
    }
    Some((u, per_page, search))
}

/// Whether the forge says there is a next page: a `Link` with
/// `rel="next"` (all four), or GitLab's `x-next-page`. Never followed.
pub fn has_next(headers: &HeaderMap) -> bool {
    let link = headers.get_all("link").iter().any(|v| {
        v.to_str().is_ok_and(|s| {
            s.split(',').any(|part| {
                part.split(';').skip(1).any(|p| {
                    let p = p.trim();
                    p == "rel=\"next\"" || p == "rel=next"
                })
            })
        })
    });
    let gitlab = headers
        .get("x-next-page")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit()));
    link || gitlab
}

/// A path part: 1 to `max` of `[A-Za-z0-9._-]` (`dots`: with `.` and `_`),
/// not `.` or `..`, not starting with `-`.
fn part_ok(s: &str, max: usize, dots: bool) -> bool {
    (1..=max).contains(&s.len())
        && s != "."
        && s != ".."
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || (dots && matches!(b, b'.' | b'_')))
}

/// `owner/name` as `kind` allows it: GitHub an account of letters, digits
/// and `-` (39 at most) and a name of `[A-Za-z0-9._-]` (100); Gitea and
/// Forgejo `[A-Za-z0-9._-]` (40 and 100); GitLab 2 to 21 parts of
/// `[A-Za-z0-9._-]` (255 each), the last not ending `.git` or `.atom`.
pub fn valid_full_name(kind: ForgeKind, s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_FULL_NAME {
        return false;
    }
    let parts: Vec<&str> = s.split('/').collect();
    match kind {
        ForgeKind::Github => {
            parts.len() == 2
                && part_ok(parts[0], 39, false)
                && !parts[0].ends_with('-')
                && part_ok(parts[1], 100, true)
        }
        ForgeKind::Gitea | ForgeKind::Forgejo => {
            parts.len() == 2 && part_ok(parts[0], 40, true) && part_ok(parts[1], 100, true)
        }
        ForgeKind::Gitlab => {
            (2..=21).contains(&parts.len())
                && parts.iter().all(|p| part_ok(p, 255, true))
                && !s.ends_with(".git")
                && !s.ends_with(".atom")
        }
    }
}

/// A branch name git accepts (`git check-ref-format --branch`) that cannot
/// read as an option: 1 to [`MAX_REF`] bytes; not `@`; no leading `-` or
/// `/`, no trailing `/` or `.`; no `..`, `//` or `@{`; no space, control,
/// bidi or invisible character, nor `~ ^ : ? * [ \`; no part starting `.`
/// or ending `.lock`.
pub fn valid_ref(r: &str) -> bool {
    (1..=MAX_REF).contains(&r.len())
        && r != "@"
        && !r.starts_with('-')
        && !r.starts_with('/')
        && !r.ends_with('/')
        && !r.ends_with('.')
        && !r.contains("..")
        && !r.contains("//")
        && !r.contains("@{")
        && r.chars().all(|c| {
            !crate::remote::hidden(c)
                && !c.is_whitespace()
                && !matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && r.split('/')
            .all(|p| !p.starts_with('.') && !p.ends_with(".lock"))
}

/// A strict RFC 3339 timestamp (with its offset), as ms since the epoch.
fn timestamp(v: Option<&Value>) -> Option<i64> {
    v?.as_str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}

/// One entry, or `None` to leave it out.
fn entry(kind: ForgeKind, v: &Value) -> Option<Repo> {
    let id = v.get("id")?.as_u64().filter(|n| *n > 0)?;
    let name_key = match kind {
        ForgeKind::Gitlab => "path_with_namespace",
        _ => "full_name",
    };
    let full_name = v
        .get(name_key)?
        .as_str()
        .filter(|s| valid_full_name(kind, s))?;
    let private = match kind {
        ForgeKind::Gitlab => match v.get("visibility")?.as_str()? {
            "public" => false,
            "internal" | "private" => true,
            _ => return None,
        },
        _ => v.get("private")?.as_bool()?,
    };
    let default_branch = v
        .get("default_branch")
        .and_then(Value::as_str)
        .filter(|b| valid_ref(b))
        .map(str::to_string);
    let description = v
        .get("description")
        .and_then(Value::as_str)
        .and_then(|d| crate::remote::clean_text(d, MAX_DESCRIPTION));
    let updated_ms = timestamp(v.get(match kind {
        ForgeKind::Gitlab => "last_activity_at",
        _ => "updated_at",
    }));
    let push = match kind {
        ForgeKind::Gitlab => {
            let p = v.get("permissions");
            let level = |k: &str| {
                p.and_then(|p| p.get(k))
                    .and_then(|a| a.get("access_level"))
                    .and_then(Value::as_u64)
            };
            level("project_access")
                .max(level("group_access"))
                .map(|l| l >= GITLAB_DEVELOPER)
        }
        _ => v
            .get("permissions")
            .and_then(|p| p.get("push"))
            .and_then(Value::as_bool),
    };
    Some(Repo {
        id: id.to_string(),
        full_name: full_name.to_string(),
        private,
        default_branch,
        description,
        updated_ms,
        push,
    })
}

/// The entries of an answer: at most `per_page` read, each checked, ids
/// seen once; with `q` (GitHub), only names containing it (any case).
pub fn parse_listing(
    kind: ForgeKind,
    body: &Value,
    per_page: u32,
    q: Option<&str>,
) -> Result<(Vec<Repo>, u32), &'static str> {
    let items = match kind {
        ForgeKind::Gitea | ForgeKind::Forgejo => {
            if body.get("ok").and_then(Value::as_bool) != Some(true) {
                return Err("its search did not succeed");
            }
            body.get("data").and_then(Value::as_array)
        }
        _ => body.as_array(),
    }
    .ok_or("it is not a list of repositories")?;
    let per_page = per_page as usize;
    let mut left_out = items.len().saturating_sub(per_page);
    let needle = q.map(str::to_lowercase);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in items.iter().take(per_page) {
        match entry(kind, v) {
            Some(r) if seen.insert(r.id.clone()) => {
                if needle
                    .as_deref()
                    .is_none_or(|n| r.full_name.to_lowercase().contains(n))
                {
                    out.push(r);
                }
            }
            _ => left_out += 1,
        }
    }
    Ok((out, u32::try_from(left_out).unwrap_or(u32::MAX)))
}

/// Ask `h` for `page` of the repositories `token` (account `account_id`)
/// can see.
pub async fn list(
    h: &ForgeHost,
    token: &Secret<String>,
    account_id: &str,
    q: Option<&str>,
    page: u32,
    timeout: Duration,
    lookup: &Lookup,
) -> Result<Listing, CheckError> {
    let bad = |what| CheckError::BadAnswer(h.host.clone(), what);
    let (url, per_page, search) = request_url(h, account_id, q, page)
        .ok_or_else(|| CheckError::Unreachable(h.host.clone(), "bad api_base"))?;
    // The client's own limit covers the whole request, from the name
    // lookup to the end of the body.
    let c = crate::forge::client(h, timeout, lookup)?;
    let headers =
        crate::forge::auth_headers(h.kind, token).map_err(|_| bad("the token cannot be sent"))?;
    let (headers, body) = crate::forge::get_url(&c, h, url, headers, MAX_BODY).await?;
    let filter = (search == Search::WithinPage).then_some(q).flatten();
    let (repositories, left_out) = parse_listing(h.kind, &body, per_page, filter).map_err(bad)?;
    Ok(Listing {
        repositories,
        page,
        has_more: page < MAX_PAGE && has_next(&headers),
        left_out,
        search,
    })
}

/// What a cached answer is kept under: the person, the connection, the
/// host's `api_base`, a hash of the sealed token as stored (every seal has
/// a fresh random nonce, so a new token is always a new key, even for an
/// answer still in flight with the old one), the word and the page.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub user: String,
    pub connection: String,
    /// The host entry's API as it is now (an admin's edit is not answered
    /// from before it).
    pub api_base: String,
    pub sealed: [u8; 32],
    pub q: Option<String>,
    pub page: u32,
}

struct Kept {
    at: Instant,
    bytes: usize,
    listing: Listing,
}

/// Recent answers, bounded by entries ([`CACHE_ENTRIES`], [`CACHE_PER_USER`]
/// each) and bytes ([`CACHE_BYTES`]); the oldest goes first. Never shared
/// between people: the key names the person and the connection, and the
/// route finds the connection by (person, id) before it looks here.
#[derive(Default)]
pub struct Cache {
    map: Mutex<HashMap<Key, Kept>>,
}

impl Cache {
    pub fn get(&self, k: &Key) -> Option<Listing> {
        let mut m = lock(&self.map);
        match m.get(k) {
            Some(e) if e.at.elapsed() < CACHE_TTL => Some(e.listing.clone()),
            Some(_) => {
                m.remove(k);
                None
            }
            None => None,
        }
    }

    pub fn put(&self, k: Key, listing: Listing) {
        // At most 100 entries of bounded fields: far below CACHE_BYTES.
        let bytes = serde_json::to_vec(&listing).map_or(CACHE_BYTES, |b| b.len()) + 256;
        let mut m = lock(&self.map);
        m.retain(|_, e| e.at.elapsed() < CACHE_TTL);
        m.remove(&k);
        let oldest = |m: &HashMap<Key, Kept>, user: Option<&str>| {
            m.iter()
                .filter(|(k, _)| user.is_none_or(|u| k.user == u))
                .min_by_key(|(_, e)| e.at)
                .map(|(k, _)| k.clone())
        };
        while m.keys().filter(|o| o.user == k.user).count() >= CACHE_PER_USER {
            let Some(o) = oldest(&m, Some(&k.user)) else {
                break;
            };
            m.remove(&o);
        }
        while m.len() >= CACHE_ENTRIES
            || m.values().map(|e| e.bytes).sum::<usize>() + bytes > CACHE_BYTES
        {
            let Some(o) = oldest(&m, None) else { break };
            m.remove(&o);
        }
        m.insert(
            k,
            Kept {
                at: Instant::now(),
                bytes,
                listing,
            },
        );
    }

    /// Forget every answer of `user`'s connection `id` (a disconnect; a new
    /// token is a new key already).
    pub fn forget(&self, user: &str, id: &str) {
        lock(&self.map).retain(|k, _| !(k.user == user && k.connection == id));
    }

    pub fn len(&self) -> usize {
        lock(&self.map).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bytes(&self) -> usize {
        lock(&self.map).values().map(|e| e.bytes).sum()
    }
}

/// The per-person listing limit (`crate::remote::Attempts`; its target
/// side is unused: the host budget is the forge's shared one).
pub(crate) const LIMITS: crate::remote::Limits = crate::remote::Limits {
    per_user: LIST_PER_USER,
    per_target: usize::MAX,
    window: LIST_WINDOW,
    max_tracked: 10_000,
    busy: "too many repository listings on this server right now; try again in a minute",
    user_over: "too many repository listings from your account; try again in a minute",
    target_over: "too many repository listings; try again in a minute",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_each_forge() {
        for ok in ["ada/harness", "acme/.github", "Ada-L/My.Repo_1", "a/b"] {
            assert!(valid_full_name(ForgeKind::Github, ok), "{ok}");
        }
        for bad in [
            "ada/..",
            "ada/.",
            "../x",
            "-x/y",
            "ada-/y",
            "ada/-y",
            "a/b/c",
            "a_b/c",
            "a.b/c",
            "ada",
            "ada/",
            "/ada",
            "ada//b",
            "ada/b c",
            "ada/b\u{0}",
            "ada/b\u{202E}",
            "ada/ü",
            &format!("{}/x", "a".repeat(40)),
            &format!("a/{}", "b".repeat(101)),
        ] {
            assert!(!valid_full_name(ForgeKind::Github, bad), "{bad:?}");
        }
        assert!(valid_full_name(ForgeKind::Gitea, "org.x/y_z"));
        assert!(!valid_full_name(ForgeKind::Gitea, "a/b/c"));
        assert!(!valid_full_name(
            ForgeKind::Gitea,
            &format!("{}/x", "a".repeat(41))
        ));
        assert!(valid_full_name(ForgeKind::Gitlab, "grp/sub/proj"));
        let deep = vec!["g"; 21].join("/");
        assert!(valid_full_name(ForgeKind::Gitlab, &deep));
        assert!(!valid_full_name(ForgeKind::Gitlab, &format!("{deep}/x")));
        for bad in [
            "solo",
            "g/x.git",
            "g/x.atom",
            "g/../x",
            "g/-x",
            &"a/".repeat(300),
        ] {
            assert!(!valid_full_name(ForgeKind::Gitlab, bad), "{bad:?}");
        }
        assert!(!valid_full_name(
            ForgeKind::Gitlab,
            &format!("{}/{}/{}", "a".repeat(255), "b".repeat(255), "c")
        ));
    }

    #[test]
    fn branches_are_safe_ref_names() {
        for ok in ["main", "feature/x-1", "release-1.2", "ü-branch", "v1@2"] {
            assert!(valid_ref(ok), "{ok}");
        }
        for bad in [
            "",
            "@",
            "-x",
            "--upload-pack=touch /tmp/x",
            "/x",
            "x/",
            "x.",
            "a..b",
            "a//b",
            "a@{1}",
            "a b",
            "a\u{7}",
            "a\u{202E}b",
            "a\u{200B}",
            "a~1",
            "a^",
            "a:b",
            "a?",
            "a*",
            "a[",
            "a\\b",
            ".hidden",
            "x/.y",
            "x.lock",
            "x.lock/y",
            &"b".repeat(256),
        ] {
            assert!(!valid_ref(bad), "{bad:?}");
        }
    }

    #[test]
    fn query_and_page() {
        assert_eq!(clean_query(None), Ok(None));
        assert_eq!(clean_query(Some("  ")), Ok(None));
        assert_eq!(clean_query(Some(" a/b&c ")), Ok(Some("a/b&c".into())));
        assert!(clean_query(Some(&"x".repeat(MAX_QUERY))).is_ok());
        assert!(clean_query(Some(&"x".repeat(MAX_QUERY + 1))).is_err());
        assert!(clean_query(Some(&"ü".repeat(MAX_QUERY))).is_ok());
        for bad in [
            "a\u{7}",
            "a\nb",
            "a\u{202E}",
            "a\u{200B}b",
            "a\u{0}",
            "a\u{FFFD}",
        ] {
            assert!(clean_query(Some(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(parse_page(None), Ok(1));
        assert_eq!(parse_page(Some("1")), Ok(1));
        assert_eq!(parse_page(Some("10")), Ok(10));
        for bad in [
            "",
            "0",
            "-1",
            "11",
            "+1",
            "1.0",
            "abc",
            "99999999999999999999",
            "01",
            "010",
        ] {
            assert!(parse_page(Some(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn next_page_comes_only_from_rel_next() {
        let h = |pairs: &[(&'static str, &'static str)]| {
            let mut m = HeaderMap::new();
            for (k, v) in pairs {
                m.append(*k, v.parse().unwrap());
            }
            m
        };
        assert!(has_next(&h(&[(
            "link",
            "<http://x/?page=2>; rel=\"next\""
        )])));
        assert!(has_next(&h(&[(
            "link",
            "<http://x/?page=1>; rel=\"prev\", <http://x/?page=3>; rel=\"next\""
        )])));
        assert!(!has_next(&h(&[(
            "link",
            "<http://x/?rel=\"next\">; rel=\"last\""
        )])));
        assert!(!has_next(&h(&[("link", "rel=\"next\"")])));
        assert!(!has_next(&h(&[])));
        assert!(has_next(&h(&[("x-next-page", "2")])));
        assert!(!has_next(&h(&[("x-next-page", "")])));
        assert!(!has_next(&h(&[("x-next-page", "x")])));
    }

    #[test]
    fn a_huge_page_is_cut_and_duplicates_counted() {
        let items: Vec<Value> = (1..=100)
            .map(|i| serde_json::json!({"id": i % 40 + 1, "full_name": format!("a/r{i}"), "private": false}))
            .collect();
        let (r, left) = parse_listing(ForgeKind::Github, &Value::Array(items), 30, None).unwrap();
        assert_eq!((r.len(), left), (30, 70));
        let items: Vec<Value> = (0..30)
            .map(|_| serde_json::json!({"id": 5, "full_name": "a/b", "private": false}))
            .collect();
        let (r, left) = parse_listing(ForgeKind::Github, &Value::Array(items), 30, None).unwrap();
        assert_eq!((r.len(), left), (1, 29));
        assert!(parse_listing(ForgeKind::Gitea, &serde_json::json!([]), 30, None).is_err());
        assert!(parse_listing(ForgeKind::Github, &serde_json::json!({}), 30, None).is_err());
    }

    #[test]
    fn gitea_needs_a_numeric_account() {
        let h = ForgeHost {
            host: "git.example.com".into(),
            kind: ForgeKind::Gitea,
            api_base: "https://git.example.com/api/v1".into(),
            api_host: None,
            private_address: false,
        };
        assert!(request_url(&h, "42", None, 1).is_some());
        for bad in ["", "4 2", "42&uid=1", "-1"] {
            assert!(request_url(&h, bad, None, 1).is_none(), "{bad:?}");
        }
    }

    fn listing(n: usize) -> Listing {
        Listing {
            repositories: (0..n)
                .map(|i| Repo {
                    id: i.to_string(),
                    full_name: format!("a/{i}"),
                    private: false,
                    default_branch: None,
                    description: Some("d".repeat(MAX_DESCRIPTION)),
                    updated_ms: None,
                    push: None,
                })
                .collect(),
            page: 1,
            has_more: false,
            left_out: 0,
            search: Search::None,
        }
    }

    fn key(user: &str, q: usize) -> Key {
        Key {
            user: user.into(),
            connection: "c".into(),
            api_base: "https://x/api".into(),
            sealed: [1; 32],
            q: Some(q.to_string()),
            page: 1,
        }
    }

    #[test]
    fn the_cache_is_bounded_per_person_and_in_all() {
        let c = Cache::default();
        for i in 0..100 {
            c.put(key("alice", i), listing(1));
        }
        assert_eq!(c.len(), CACHE_PER_USER, "one person fills only her part");
        assert!(c.get(&key("alice", 99)).is_some());
        assert!(c.get(&key("alice", 0)).is_none(), "the oldest went");
        for u in 0..40 {
            for i in 0..10 {
                c.put(key(&format!("u{u}"), i), listing(1));
            }
        }
        assert!(c.len() <= CACHE_ENTRIES);
        // Big answers: bounded by bytes.
        let c = Cache::default();
        for i in 0..200 {
            c.put(key(&format!("b{i}"), 0), listing(100));
        }
        assert!(c.bytes() <= CACHE_BYTES, "{}", c.bytes());
        assert!(c.len() < 200);
        // Another person, another token: not the same key.
        let c = Cache::default();
        c.put(key("alice", 1), listing(1));
        assert!(c.get(&key("bob", 1)).is_none());
        let mut newer = key("alice", 1);
        newer.sealed = [2; 32];
        assert!(c.get(&newer).is_none());
        // Old answers expire.
        c.put(key("carol", 1), listing(1));
        lock(&c.map).get_mut(&key("carol", 1)).unwrap().at =
            Instant::now().checked_sub(CACHE_TTL).unwrap();
        assert!(c.get(&key("carol", 1)).is_none());
        c.forget("alice", "c");
        assert!(c.is_empty());
    }
}

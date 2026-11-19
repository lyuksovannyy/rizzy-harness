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
use std::fmt;

use serde::Serialize;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::forge::{CheckError, ForgeHost, ForgeKind};
use crate::lock;
use crate::remote::Lookup;

/// Repositories per page of ours.
pub const PER_PAGE: u32 = 30;
/// The GitHub forge page read for a search word (its maximum).
pub const GITHUB_SEARCH_PER_PAGE: u32 = 100;
/// The last page served: at most 300 repositories are reachable by paging.
/// On GitLab, Gitea and Forgejo a search word reaches the rest (the forge
/// searches them all); on GitHub a search filters pages of 100 here, so a
/// repository beyond the 1,000 most recently updated is never found.
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
/// One person's share of [`CACHE_BYTES`], so nobody's large answers push
/// everyone else's out.
pub const CACHE_PER_USER_BYTES: usize = 512 * 1024;
/// Listings in flight on the whole server, from before the token is opened
/// until the answer is built; one more is refused at once, not queued.
pub const MAX_IN_FLIGHT: usize = 2;
/// How long a forge's refusal of one token is remembered.
pub const REFUSAL_TTL: Duration = Duration::from_secs(60);
/// Refusals remembered at once.
pub const MAX_REFUSALS: usize = 1024;
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
                    // Groups' names match too (GitLab: "include ancestor
                    // namespaces when matching search criteria").
                    qp.append_pair("search", q)
                        .append_pair("search_namespaces", "true");
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

/// A JSON value cut down to what a field check needs. Arrays, objects,
/// floats, negative numbers and null are never a valid field here: they
/// become `Other` without being kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Loose {
    #[default]
    Absent,
    Bool(bool),
    U64(u64),
    Str(String),
    Other,
}

impl Loose {
    fn str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
    fn bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn u64(&self) -> Option<u64> {
        match self {
            Self::U64(n) => Some(*n),
            _ => None,
        }
    }
}

/// A value read and thrown away, keeping nothing. Unlike serde's
/// `IgnoredAny` (which serde_json skips without a depth limit) it goes
/// through `deserialize_any`, so the parser's nesting limit (128) holds
/// for skipped values too and an absurdly deep answer is refused, as the
/// whole-tree parse refused it.
struct Skip;

impl<'de> de::Deserialize<'de> for Skip {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Skip, D::Error> {
        d.deserialize_any(SkipVisitor)
    }
}

struct SkipVisitor;

impl<'de> Visitor<'de> for SkipVisitor {
    type Value = Skip;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Skip, E> {
        Ok(Skip)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Skip, E> {
        Ok(Skip)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Skip, E> {
        Ok(Skip)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Skip, E> {
        Ok(Skip)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<Skip, E> {
        Ok(Skip)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Skip, E> {
        Ok(Skip)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Skip, A::Error> {
        while a.next_element::<Skip>()?.is_some() {}
        Ok(Skip)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Skip, A::Error> {
        while a.next_entry::<Skip, Skip>()?.is_some() {}
        Ok(Skip)
    }
}

/// Skip whatever a visitor does not take, keeping nothing of it, and give
/// the visitor's default value.
macro_rules! skip_scalars {
    ($t:ty) => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<$t, E> {
            Ok(<$t>::default())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<$t, E> {
            Ok(<$t>::default())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<$t, E> {
            Ok(<$t>::default())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<$t, E> {
            Ok(<$t>::default())
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<$t, E> {
            Ok(<$t>::default())
        }
        fn visit_unit<E: de::Error>(self) -> Result<$t, E> {
            Ok(<$t>::default())
        }
    };
}
macro_rules! skip_seq {
    ($t:ty) => {
        fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<$t, A::Error> {
            while a.next_element::<Skip>()?.is_some() {}
            Ok(<$t>::default())
        }
    };
}
macro_rules! skip_map {
    ($t:ty) => {
        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<$t, A::Error> {
            while a.next_entry::<Skip, Skip>()?.is_some() {}
            Ok(<$t>::default())
        }
    };
}

/// One value as a [`Loose`]. A string is kept whole: it is a slice of the
/// answer already held (at most [`MAX_BODY`]), at most a page of entries is
/// read, and the checks after refuse what is too long.
struct LooseSeed;

impl<'de> DeserializeSeed<'de> for LooseSeed {
    type Value = Loose;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Loose, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for LooseSeed {
    type Value = Loose;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Loose, E> {
        Ok(Loose::Bool(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Loose, E> {
        Ok(Loose::U64(v))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Loose, E> {
        Ok(Loose::Other)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Loose, E> {
        Ok(Loose::Other)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Loose, E> {
        Ok(Loose::Other)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Loose, E> {
        Ok(Loose::Str(v.to_string()))
    }
    skip_seq!(Loose);
    skip_map!(Loose);
}

/// The keys read; any other is skipped.
enum Field {
    Id,
    FullName,
    PathWithNamespace,
    Private,
    Visibility,
    DefaultBranch,
    Description,
    UpdatedAt,
    LastActivityAt,
    Permissions,
    Push,
    ProjectAccess,
    GroupAccess,
    AccessLevel,
    Ok,
    Data,
    Other,
}

struct FieldSeed;

impl<'de> DeserializeSeed<'de> for FieldSeed {
    type Value = Field;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Field, D::Error> {
        d.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for FieldSeed {
    type Value = Field;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a key")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Field, E> {
        Ok(match v {
            "id" => Field::Id,
            "full_name" => Field::FullName,
            "path_with_namespace" => Field::PathWithNamespace,
            "private" => Field::Private,
            "visibility" => Field::Visibility,
            "default_branch" => Field::DefaultBranch,
            "description" => Field::Description,
            "updated_at" => Field::UpdatedAt,
            "last_activity_at" => Field::LastActivityAt,
            "permissions" => Field::Permissions,
            "push" => Field::Push,
            "project_access" => Field::ProjectAccess,
            "group_access" => Field::GroupAccess,
            "access_level" => Field::AccessLevel,
            "ok" => Field::Ok,
            "data" => Field::Data,
            _ => Field::Other,
        })
    }
}

/// `permissions`: GitHub's and Gitea's `push`, GitLab's two access levels.
#[derive(Debug, Default)]
struct Perm {
    push: Loose,
    project: Loose,
    group: Loose,
}

struct PermSeed;

impl<'de> DeserializeSeed<'de> for PermSeed {
    type Value = Perm;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Perm, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PermSeed {
    type Value = Perm;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("permissions")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Perm, A::Error> {
        let mut p = Perm::default();
        while let Some(k) = a.next_key_seed(FieldSeed)? {
            match k {
                Field::Push => p.push = a.next_value_seed(LooseSeed)?,
                Field::ProjectAccess => p.project = a.next_value_seed(LevelSeed)?,
                Field::GroupAccess => p.group = a.next_value_seed(LevelSeed)?,
                _ => {
                    a.next_value::<Skip>()?;
                }
            }
        }
        Ok(p)
    }
    skip_scalars!(Perm);
    skip_seq!(Perm);
}

/// GitLab's `{"access_level": n}` (null or anything else: absent).
struct LevelSeed;

impl<'de> DeserializeSeed<'de> for LevelSeed {
    type Value = Loose;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Loose, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for LevelSeed {
    type Value = Loose;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an access level")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Loose, A::Error> {
        let mut l = Loose::Absent;
        while let Some(k) = a.next_key_seed(FieldSeed)? {
            match k {
                Field::AccessLevel => l = a.next_value_seed(LooseSeed)?,
                _ => {
                    a.next_value::<Skip>()?;
                }
            }
        }
        Ok(l)
    }
    skip_scalars!(Loose);
    skip_seq!(Loose);
}

/// One entry as read: only the fields used; a key given
/// twice keeps its last value (as a parsed JSON object would).
#[derive(Debug, Default)]
struct Raw {
    id: Loose,
    full_name: Loose,
    path_with_namespace: Loose,
    private: Loose,
    visibility: Loose,
    default_branch: Loose,
    description: Loose,
    updated_at: Loose,
    last_activity_at: Loose,
    perm: Perm,
}

/// An entry: `None` when it is not an object.
struct EntrySeed;

impl<'de> DeserializeSeed<'de> for EntrySeed {
    type Value = Option<Raw>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Option<Raw>, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for EntrySeed {
    type Value = Option<Raw>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a repository")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Option<Raw>, A::Error> {
        let mut r = Raw::default();
        while let Some(k) = a.next_key_seed(FieldSeed)? {
            match k {
                Field::Id => r.id = a.next_value_seed(LooseSeed)?,
                Field::FullName => r.full_name = a.next_value_seed(LooseSeed)?,
                Field::PathWithNamespace => r.path_with_namespace = a.next_value_seed(LooseSeed)?,
                Field::Private => r.private = a.next_value_seed(LooseSeed)?,
                Field::Visibility => r.visibility = a.next_value_seed(LooseSeed)?,
                Field::DefaultBranch => r.default_branch = a.next_value_seed(LooseSeed)?,
                Field::Description => r.description = a.next_value_seed(LooseSeed)?,
                Field::UpdatedAt => r.updated_at = a.next_value_seed(LooseSeed)?,
                Field::LastActivityAt => r.last_activity_at = a.next_value_seed(LooseSeed)?,
                Field::Permissions => r.perm = a.next_value_seed(PermSeed)?,
                _ => {
                    a.next_value::<Skip>()?;
                }
            }
        }
        Ok(Some(r))
    }
    skip_scalars!(Option<Raw>);
    skip_seq!(Option<Raw>);
}

/// A list's first entries (at most the page size asked for) and how many
/// more it had (counted, not kept).
type List = (Vec<Option<Raw>>, usize);

/// A list: `None` when it is not one.
struct ListSeed {
    take: usize,
}

impl<'de> DeserializeSeed<'de> for ListSeed {
    type Value = Option<List>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Option<List>, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ListSeed {
    type Value = Option<List>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a list")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Option<List>, A::Error> {
        let mut items = Vec::new();
        while items.len() < self.take {
            match a.next_element_seed(EntrySeed)? {
                Some(e) => items.push(e),
                None => return Ok(Some((items, 0))),
            }
        }
        let mut more = 0usize;
        while a.next_element::<Skip>()?.is_some() {
            more += 1;
        }
        Ok(Some((items, more)))
    }
    skip_scalars!(Option<List>);
    skip_map!(Option<List>);
}

/// The whole answer: a list (GitHub, GitLab) or `{ok, data}` (Gitea,
/// Forgejo).
#[derive(Default)]
enum Top {
    List(List),
    Object {
        ok: Loose,
        data: Option<List>,
    },
    #[default]
    Other,
}

struct TopSeed {
    take: usize,
}

impl<'de> DeserializeSeed<'de> for TopSeed {
    type Value = Top;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Top, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for TopSeed {
    type Value = Top;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a list of repositories")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Top, A::Error> {
        Ok(ListSeed { take: self.take }
            .visit_seq(a)?
            .map_or(Top::Other, Top::List))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Top, A::Error> {
        let (mut ok, mut data) = (Loose::Absent, None);
        while let Some(k) = a.next_key_seed(FieldSeed)? {
            match k {
                Field::Ok => ok = a.next_value_seed(LooseSeed)?,
                Field::Data => data = a.next_value_seed(ListSeed { take: self.take })?,
                _ => {
                    a.next_value::<Skip>()?;
                }
            }
        }
        Ok(Top::Object { ok, data })
    }
    skip_scalars!(Top);
}

/// Characters of a description given to the cleaning: 4 × those kept,
/// with room for the spaces and hidden characters it drops.
const DESCRIPTION_READ: usize = 4 * MAX_DESCRIPTION;

/// The first `chars` characters of `s`.
fn prefix(s: &str, chars: usize) -> &str {
    s.char_indices().nth(chars).map_or(s, |(i, _)| &s[..i])
}

/// A strict RFC 3339 timestamp (with its offset), as ms since the epoch.
fn timestamp(v: &Loose) -> Option<i64> {
    v.str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}

/// One entry, or `None` to leave it out.
fn entry(kind: ForgeKind, v: &Raw) -> Option<Repo> {
    let id = v.id.u64().filter(|n| *n > 0)?;
    let full_name = match kind {
        ForgeKind::Gitlab => &v.path_with_namespace,
        _ => &v.full_name,
    }
    .str()
    .filter(|s| valid_full_name(kind, s))?;
    let private = match kind {
        ForgeKind::Gitlab => match v.visibility.str()? {
            "public" => false,
            "internal" | "private" => true,
            _ => return None,
        },
        _ => v.private.bool()?,
    };
    let default_branch = v
        .default_branch
        .str()
        .filter(|b| valid_ref(b))
        .map(str::to_string);
    // Cut before cleaning, which splits all of what it is given.
    let description = v
        .description
        .str()
        .and_then(|d| crate::remote::clean_text(prefix(d, DESCRIPTION_READ), MAX_DESCRIPTION));
    let updated_ms = timestamp(match kind {
        ForgeKind::Gitlab => &v.last_activity_at,
        _ => &v.updated_at,
    });
    let push = match kind {
        ForgeKind::Gitlab => v
            .perm
            .project
            .u64()
            .max(v.perm.group.u64())
            .map(|l| l >= GITLAB_DEVELOPER),
        _ => v.perm.push.bool(),
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

const NOT_JSON: &str = "it is not JSON";
const NOT_LIST: &str = "it is not a list of repositories";

/// The entries of an answer, read in one pass without building the JSON:
/// at most `per_page` entries are read (each into the few bounded fields
/// used, the rest skipped with [`Skip`]); later ones are only counted. Each is checked,
/// ids seen once; with `q` (GitHub), only names containing it (any case).
pub fn parse_listing(
    kind: ForgeKind,
    body: &[u8],
    per_page: u32,
    q: Option<&str>,
) -> Result<(Vec<Repo>, u32), &'static str> {
    let mut de = serde_json::Deserializer::from_slice(body);
    let top = TopSeed {
        take: per_page as usize,
    }
    .deserialize(&mut de)
    .map_err(|_| NOT_JSON)?;
    de.end().map_err(|_| NOT_JSON)?;
    let (items, more) = match (kind, top) {
        (ForgeKind::Gitea | ForgeKind::Forgejo, Top::Object { ok, data }) => {
            if ok.bool() != Some(true) {
                return Err("its search did not succeed");
            }
            data.ok_or(NOT_LIST)?
        }
        (ForgeKind::Gitea | ForgeKind::Forgejo, _) => return Err("its search did not succeed"),
        (_, Top::List(l)) => l,
        _ => return Err(NOT_LIST),
    };
    let mut left_out = more;
    let needle = q.map(str::to_lowercase);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in &items {
        match v.as_ref().and_then(|v| entry(kind, v)) {
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

/// The memory an entry holds, as near as can be told without asking the
/// allocator: the key's and the entry's own sizes, every string's capacity
/// and the list's, and a map slot.
fn held(k: &Key, l: &Listing) -> usize {
    let o = |x: &Option<String>| x.as_ref().map_or(0, String::capacity);
    std::mem::size_of::<Key>()
        + std::mem::size_of::<Kept>()
        + 64
        + k.user.capacity()
        + k.connection.capacity()
        + k.api_base.capacity()
        + o(&k.q)
        + l.repositories.capacity() * std::mem::size_of::<Repo>()
        + l.repositories
            .iter()
            .map(|r| {
                r.id.capacity() + r.full_name.capacity() + o(&r.default_branch) + o(&r.description)
            })
            .sum::<usize>()
}

/// Recent answers, bounded by entries ([`CACHE_ENTRIES`], [`CACHE_PER_USER`]
/// each) and held bytes ([`CACHE_BYTES`], [`CACHE_PER_USER_BYTES`] each);
/// the oldest goes first. Never shared
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
        // At most 100 entries of bounded fields: below a person's share.
        let bytes = held(&k, &listing);
        let mut m = lock(&self.map);
        m.retain(|_, e| e.at.elapsed() < CACHE_TTL);
        m.remove(&k);
        let oldest = |m: &HashMap<Key, Kept>, user: Option<&str>| {
            m.iter()
                .filter(|(k, _)| user.is_none_or(|u| k.user == u))
                .min_by_key(|(_, e)| e.at)
                .map(|(k, _)| k.clone())
        };
        let mine = |m: &HashMap<Key, Kept>| {
            m.iter()
                .filter(|(o, _)| o.user == k.user)
                .fold((0, 0), |(n, b), (_, e)| (n + 1, b + e.bytes))
        };
        while {
            let (n, b) = mine(&m);
            n >= CACHE_PER_USER || (n > 0 && b + bytes > CACHE_PER_USER_BYTES)
        } {
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

/// Whether a failed listing says the forge host itself is failing, so
/// that it counts against the host's shared budget: a connection or TLS
/// failure, a timeout, a body cut off or over the cap, a 5xx, a 3xx that is
/// not followed, a malformed answer to a 2xx. Every 4xx is the forge's
/// answer to this request (to the token, the account's rate, the word or
/// the page) and does not count: the person chose the word, so counting it
/// would let one person's search lock everyone out of the host.
pub fn host_at_fault(e: &CheckError) -> bool {
    !matches!(
        e,
        CheckError::Refused(..) | CheckError::Status(_, 400..=499)
    )
}

/// The forge refused this token or is limiting its account (401, 403,
/// 429): remembered for the token, for every word and page.
pub fn token_refused(e: &CheckError) -> bool {
    matches!(e, CheckError::Refused(..) | CheckError::Status(_, 429))
}

/// Another 4xx (400, 404, 409, 422 …): the forge's answer to this word and
/// page; remembered for them only.
pub fn request_refused(e: &CheckError) -> bool {
    matches!(e, CheckError::Status(_, 400..=499)) && !token_refused(e)
}

/// (person, connection, host's `api_base`, hash of the sealed token).
pub type TokenKey = (String, String, String, [u8; 32]);

/// What a refusal is remembered under: the token, and for a refusal of
/// one request also its word and page.
type RefusalKey = (TokenKey, Option<(Option<String>, u32)>);

/// Refusals, bounded ([`MAX_REFUSALS`], the oldest goes first). A replaced
/// token is a new key, so it is asked at once.
#[derive(Default)]
pub struct Refusals {
    map: Mutex<HashMap<RefusalKey, (Instant, String)>>,
}

impl Refusals {
    /// A refusal of this token, else of this word and page, within
    /// [`REFUSAL_TTL`].
    pub fn get(&self, token: &TokenKey, q: &Option<String>, page: u32) -> Option<String> {
        let mut m = lock(&self.map);
        for k in [
            (token.clone(), None),
            (token.clone(), Some((q.clone(), page))),
        ] {
            match m.get(&k) {
                Some((at, msg)) if at.elapsed() < REFUSAL_TTL => return Some(msg.clone()),
                Some(_) => {
                    m.remove(&k);
                }
                None => {}
            }
        }
        None
    }

    /// Remember a refusal: of the token (`request` none) or of one word and
    /// page.
    pub fn put(&self, token: TokenKey, request: Option<(Option<String>, u32)>, msg: String) {
        let k = (token, request);
        let mut m = lock(&self.map);
        m.retain(|_, (at, _)| at.elapsed() < REFUSAL_TTL);
        while m.len() >= MAX_REFUSALS {
            let Some(o) = m
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            m.remove(&o);
        }
        m.insert(k, (Instant::now(), msg));
    }

    pub fn len(&self) -> usize {
        lock(&self.map).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What a person is told while the host's shared budget of failures is
/// used up (the step-1 wording speaks of token checks they did not make).
pub const HOST_PAUSED: &str = "this forge host failed too many requests from this server lately, so listing its repositories is paused; try again in a few minutes";

/// A person's one listing in flight, given back when dropped.
pub struct PersonSlot<'a> {
    set: &'a Mutex<HashSet<String>>,
    user: String,
}

impl Drop for PersonSlot<'_> {
    fn drop(&mut self) {
        lock(self.set).remove(&self.user);
    }
}

/// Take `user`'s one listing slot; `None` while their earlier listing is
/// still in flight.
pub fn take_person<'a>(set: &'a Mutex<HashSet<String>>, user: &str) -> Option<PersonSlot<'a>> {
    if !lock(set).insert(user.to_string()) {
        return None;
    }
    Some(PersonSlot {
        set,
        user: user.to_string(),
    })
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
        let items: Vec<serde_json::Value> = (1..=100)
            .map(|i| serde_json::json!({"id": i % 40 + 1, "full_name": format!("a/r{i}"), "private": false}))
            .collect();
        let (r, left) = parse_listing(
            ForgeKind::Github,
            &serde_json::to_vec(&items).unwrap(),
            30,
            None,
        )
        .unwrap();
        assert_eq!((r.len(), left), (30, 70));
        let items: Vec<serde_json::Value> = (0..30)
            .map(|_| serde_json::json!({"id": 5, "full_name": "a/b", "private": false}))
            .collect();
        let (r, left) = parse_listing(
            ForgeKind::Github,
            &serde_json::to_vec(&items).unwrap(),
            30,
            None,
        )
        .unwrap();
        assert_eq!((r.len(), left), (1, 29));
        assert!(parse_listing(ForgeKind::Gitea, b"[]", 30, None).is_err());
        assert!(parse_listing(ForgeKind::Github, b"{}", 30, None).is_err());
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

    /// The largest listing a page can hold: 100 entries of the longest
    /// fields kept.
    fn heavy() -> Listing {
        Listing {
            repositories: (0..100)
                .map(|i| Repo {
                    id: i.to_string(),
                    full_name: format!("{}/{i:0>400}", "o".repeat(100)),
                    private: false,
                    default_branch: Some("b".repeat(MAX_REF)),
                    description: Some("\u{1F600}".repeat(MAX_DESCRIPTION)),
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

    #[test]
    fn refusals_are_remembered_bounded_and_expire() {
        let r = Refusals::default();
        let k = |i: usize| (format!("u{i}"), "c".to_string(), "b".to_string(), [0u8; 32]);
        let (none, word) = (None::<String>, Some("boom".to_string()));
        r.put(k(0), None, "no".into());
        assert_eq!(r.get(&k(0), &none, 1).as_deref(), Some("no"));
        assert_eq!(
            r.get(&k(0), &word, 3).as_deref(),
            Some("no"),
            "a refused token blocks every word"
        );
        let mut other = k(0);
        other.3 = [1; 32];
        assert_eq!(r.get(&other, &none, 1), None, "another token is asked");
        // A refused word blocks that word and page only.
        r.put(k(1), Some((word.clone(), 1)), "422".into());
        assert_eq!(r.get(&k(1), &word, 1).as_deref(), Some("422"));
        assert_eq!(r.get(&k(1), &word, 2), None);
        assert_eq!(r.get(&k(1), &Some("other".into()), 1), None);
        assert_eq!(r.get(&k(1), &none, 1), None);
        for i in 2..=MAX_REFUSALS + 10 {
            r.put(k(i), None, "no".into());
        }
        assert_eq!(r.len(), MAX_REFUSALS);
        assert_eq!(r.get(&k(0), &none, 1), None, "the oldest went");
        let last = (k(MAX_REFUSALS), None);
        lock(&r.map).get_mut(&last).unwrap().0 = Instant::now().checked_sub(REFUSAL_TTL).unwrap();
        assert_eq!(r.get(&k(MAX_REFUSALS), &none, 1), None, "expired");
        assert!(!r.is_empty());
    }

    #[test]
    fn only_host_failures_count_against_the_host() {
        let h = || "h".to_string();
        for e in [
            CheckError::Refused(h(), 401),
            CheckError::Refused(h(), 403),
            CheckError::Status(h(), 429),
        ] {
            assert!(
                !host_at_fault(&e) && token_refused(&e) && !request_refused(&e),
                "{e}"
            );
        }
        for code in [400, 404, 409, 422, 499] {
            let e = CheckError::Status(h(), code);
            assert!(
                !host_at_fault(&e) && !token_refused(&e) && request_refused(&e),
                "{e}"
            );
        }
        for e in [
            CheckError::Status(h(), 500),
            CheckError::Status(h(), 503),
            CheckError::Status(h(), 302),
            CheckError::Status(h(), 399),
            CheckError::Unreachable(h(), "x"),
            CheckError::BadAnswer(h(), "x"),
        ] {
            assert!(
                host_at_fault(&e) && !token_refused(&e) && !request_refused(&e),
                "{e}"
            );
        }
    }

    /// The parse keeps what a parsed JSON object would: the last of a key
    /// given twice; ids only as positive integers that fit 64 bits; nesting
    /// beyond the parser's limit is refused, not followed into.
    #[test]
    fn the_bounded_parse_keeps_the_old_outcomes() {
        let body = br#"[{"id":1,"id":2,"full_name":"a/b","full_name":"a/c","private":false},
            {"id":9223372036854775808,"full_name":"a/d","private":false},
            {"id":1.0,"full_name":"a/e","private":false},
            {"id":-3,"full_name":"a/f","private":false},
            {"id":"4","full_name":"a/g","private":false},
            {"id":18446744073709551616,"full_name":"a/h","private":false},
            {"id":5,"full_name":"a/i","private":false,"permissions":{"push":true},"permissions":[1]},
            {"id":6,"full_name":"a/j","private":false,"x":{"deep":[[[{"full_name":"z/z"}]]]}}]"#;
        let (r, left) = parse_listing(ForgeKind::Github, body, 30, None).unwrap();
        let names: Vec<_> = r
            .iter()
            .map(|r| (r.id.as_str(), r.full_name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("2", "a/c"),
                ("9223372036854775808", "a/d"),
                ("5", "a/i"),
                ("6", "a/j")
            ]
        );
        assert_eq!(left, 4);
        assert_eq!(r[2].push, None, "the later permissions win");
        // Deeper than the parser allows: an error, quickly, at the top,
        // inside an entry's skipped field, or past the page.
        let deep = format!("{}{}", "[".repeat(200_000), "]".repeat(200_000));
        assert_eq!(
            parse_listing(ForgeKind::Github, deep.as_bytes(), 30, None),
            Err(NOT_JSON)
        );
        let inner = format!(
            r#"[{{"id":1,"full_name":"a/b","private":false,"x":{}{}}}]"#,
            "[".repeat(200_000),
            "]".repeat(200_000)
        );
        assert_eq!(
            parse_listing(ForgeKind::Github, inner.as_bytes(), 30, None),
            Err(NOT_JSON)
        );
        let past = format!(
            r#"[{{"id":1,"full_name":"a/b","private":false}},{}{}]"#,
            "[".repeat(200_000),
            "]".repeat(200_000)
        );
        assert_eq!(
            parse_listing(ForgeKind::Github, past.as_bytes(), 1, None),
            Err(NOT_JSON)
        );
        // Trailing bytes and a cut answer are not JSON.
        assert_eq!(
            parse_listing(ForgeKind::Github, b"[] x", 30, None),
            Err(NOT_JSON)
        );
        assert_eq!(
            parse_listing(ForgeKind::Github, b"[{\"id\":1", 30, None),
            Err(NOT_JSON)
        );
        // Gitea's ok may come after its data; data that is not a list.
        let (r, _) = parse_listing(
            ForgeKind::Gitea,
            br#"{"data":[{"id":1,"full_name":"a/b","private":true}],"ok":true}"#,
            30,
            None,
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(
            parse_listing(ForgeKind::Gitea, br#"{"ok":true,"data":{}}"#, 30, None),
            Err(NOT_LIST)
        );
        // A long description is cut, a long name left out.
        let long = format!(
            r#"[{{"id":1,"full_name":"a/b","private":true,"description":"{}"}},{{"id":2,"full_name":"a/{}","private":true}}]"#,
            "\u{1F600}".repeat(100_000),
            "n".repeat(100_000)
        );
        let (r, left) = parse_listing(ForgeKind::Github, long.as_bytes(), 30, None).unwrap();
        assert_eq!((r.len(), left), (1, 1));
        assert_eq!(
            r[0].description.as_ref().unwrap().chars().count(),
            MAX_DESCRIPTION
        );
    }

    #[test]
    fn a_person_has_one_slot_given_back_when_dropped() {
        let set = Mutex::default();
        let a = take_person(&set, "alice").unwrap();
        assert!(take_person(&set, "alice").is_none());
        let b = take_person(&set, "bob").unwrap();
        drop(a);
        assert!(take_person(&set, "alice").is_some(), "dropped, given back");
        drop(b);
        assert!(lock(&set).is_empty());
    }

    #[test]
    fn a_description_is_cut_before_it_is_cleaned() {
        assert_eq!(prefix("abc", 2), "ab");
        assert_eq!(prefix("äöü", 2), "äö");
        assert_eq!(prefix("ab", 5), "ab");
        let body = format!(
            r#"[{{"id":1,"full_name":"a/b","private":true,"description":"{}x"}}]"#,
            " ".repeat(2 * DESCRIPTION_READ)
        );
        let (r, _) = parse_listing(ForgeKind::Github, body.as_bytes(), 30, None).unwrap();
        assert_eq!(r[0].description, None, "only the cut part is cleaned");
    }

    /// Review of PR 158, LOW 8: one person's large answers do not push
    /// another's out of the cache.
    #[test]
    fn one_person_cannot_flush_anothers_cache() {
        let c = Cache::default();
        c.put(key("bob", 1), listing(1));
        for i in 0..CACHE_PER_USER {
            c.put(key("alice", i), heavy());
        }
        assert!(c.get(&key("bob", 1)).is_some(), "bob's entry survives");
        assert!(
            c.len() < CACHE_PER_USER,
            "her bytes, not only her count, bound her: {}",
            c.len()
        );
        assert!(c.get(&key("alice", CACHE_PER_USER - 1)).is_some());
    }
}

//! Chat search and "similar earlier chats" (PLAN.md §0 omnibox).
//!
//! User text is never passed to FTS5 as-is: it is tokenized here and every
//! token is quoted, so query syntax (`"`, `*`, `NEAR`, `-`, `:`) in the input
//! has no effect.

use std::collections::HashMap;
use std::sync::OnceLock;

use harness_core::SessionId;
use harness_store::{HIT_END, HIT_START, SearchHit, SessionRow, Store};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;

/// Shared with the web app so client and server agree on what is time-sensitive.
const TIME_SENSITIVE_JSON: &str = include_str!("../../../web/src/lib/time-sensitive.json");

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "could", "do", "does", "for",
    "from", "get", "has", "have", "how", "i", "if", "in", "into", "is", "it", "its", "me", "my",
    "of", "on", "or", "please", "should", "so", "that", "the", "their", "then", "there", "this",
    "to", "was", "we", "what", "when", "where", "which", "who", "why", "will", "with", "would",
    "you", "your",
];

/// Minimum share of the query's meaningful words a chat must contain to be
/// suggested as "already asked".
pub const MIN_COVERAGE: f64 = 0.6;
pub const MAX_SUGGESTIONS: usize = 3;
const MIN_SUGGEST_TOKENS: usize = 3;

/// One search as every provider gets it (chats, projects, media,
/// artifacts). `tokens` come from `tokens`, `fts` from `fts_query`; `owner`
/// and `workspace` are the caller's visibility (`None`: the API token).
pub struct SearchCtx<'a> {
    pub tokens: &'a [String],
    pub fts: Option<String>,
    pub owner: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub limit: usize,
    pub offset: usize,
}

/// One row of the search panel. `title`, `preview` and `snippet` carry
/// `HIT_START` / `HIT_END` marks around matches.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    /// `chat` | `project` | `media` | `artifact`.
    pub kind: &'static str,
    /// A chat's session id, else a 32-hex id.
    pub id: String,
    pub title: String,
    /// What the client labels it: `chat`, `agentic`, `routine`, `project`,
    /// a media kind or an artifact kind.
    #[serde(rename = "type")]
    pub type_: &'static str,
    /// Line 2 of the row; empty when there is none.
    pub preview: String,
    /// Line 3: the matched excerpt, only when title and preview do not
    /// already show every term.
    pub snippet: Option<String>,
    pub time_ms: i64,
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    /// A same-origin path, never a URL from elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    /// 0: every term in the title; 1: some; 2: only in the body. The All
    /// tab sorts by it first.
    #[serde(skip)]
    pub tier: u8,
}

/// What each kind of result implements; it runs inside one `mirror.call`.
pub type Provider = fn(&Store, &SearchCtx) -> harness_store::Result<Vec<SearchResult>>;

/// A one-line preview of a message: its first sentence, as the user would
/// read it rendered. Fenced code, markdown markers, link targets and control
/// characters go; the first line with text is cut at its first sentence
/// boundary (UAX #29) and capped at `max` graphemes, an ellipsis included.
/// A message that is only code shows its first line of code.
pub fn first_sentence(text: &str, max: usize) -> String {
    let line = first_prose_line(text).or_else(|| first_code_line(text));
    let Some(line) = line else {
        return String::new();
    };
    let sentence = line.split_sentence_bounds().next().unwrap_or(&line).trim();
    cap_graphemes(sentence, max)
}

/// Whether `line` opens or closes a fenced code block.
fn is_fence(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with("```") || l.starts_with("~~~")
}

/// The first line outside fenced code that still has text once cleaned.
fn first_prose_line(text: &str) -> Option<String> {
    let mut in_code = false;
    for line in text.lines() {
        if is_fence(line) {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let clean = clean_line(line);
        if !clean.is_empty() {
            return Some(clean);
        }
    }
    None
}

/// The first line inside fenced code that has text.
fn first_code_line(text: &str) -> Option<String> {
    text.lines()
        .filter(|l| !is_fence(l))
        .map(strip_controls)
        .map(|l| l.trim().to_string())
        .find(|l| !l.is_empty())
}

/// C0 controls and DEL go; a tab reads as a space.
fn strip_controls(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            c if c.is_ascii_control() => None,
            c => Some(c),
        })
        .collect()
}

/// One line as rendered text: block markers (headings, quotes, list
/// bullets), emphasis and code marks, and link targets removed.
fn clean_line(line: &str) -> String {
    let mut l = strip_controls(line);
    l = l.trim().to_string();
    // Quotes and list bullets nest ("> - item").
    loop {
        let before = l.len();
        l = strip_block_marker(&l).trim_start().to_string();
        if l.len() == before {
            break;
        }
    }
    strip_inline(&l).trim().to_string()
}

/// One leading block marker: `#`-headings, `>`, `-`/`*`/`+` bullets,
/// `1.` / `1)` numbers.
fn strip_block_marker(l: &str) -> &str {
    if let Some(rest) = l.strip_prefix('>') {
        return rest;
    }
    let hashes = l.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && l[hashes..].starts_with(' ') {
        return &l[hashes..];
    }
    for b in ["- ", "* ", "+ "] {
        if let Some(rest) = l.strip_prefix(b) {
            return rest;
        }
    }
    let digits = l.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let rest = &l[digits..];
        if rest.starts_with(". ") || rest.starts_with(") ") {
            return &rest[1..];
        }
    }
    l
}

/// Inline markdown: `[label](target)` and `![alt](target)` keep their text,
/// code marks go, and emphasis runs (`*`, `**`, `__`, `~~`) go where they
/// touch a word on one side only, so `2 * 3` and `snake_case` stay.
fn strip_inline(l: &str) -> String {
    let chars: Vec<char> = l.chars().collect();
    let mut out = String::with_capacity(l.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            i += 1;
            continue;
        }
        if c == '['
            && let Some((label, next)) = link_label(&chars, i)
        {
            out.push_str(&label);
            i = next;
            continue;
        }
        if c == '`' {
            i += 1;
            continue;
        }
        if matches!(c, '*' | '_' | '~') {
            let run = chars[i..].iter().take_while(|&&x| x == c).count();
            let before = i.checked_sub(1).map(|j| chars[j]);
            let after = chars.get(i + run).copied();
            let word = |x: Option<char>| x.is_some_and(|x| !x.is_whitespace());
            let emphasis = (c == '*' || run >= 2) && word(before) != word(after);
            if !emphasis {
                out.extend(&chars[i..i + run]);
            }
            i += run;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// At `[` (index `at`): the label of a `[label](target)` link and the index
/// after its `)`, or `None` when it is not one.
fn link_label(chars: &[char], at: usize) -> Option<(String, usize)> {
    let close = at + 1 + chars[at + 1..].iter().position(|&c| c == ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = close + 2 + chars[close + 2..].iter().position(|&c| c == ')')?;
    Some((chars[at + 1..close].iter().collect(), end + 1))
}

/// `s` cut to at most `max` graphemes, the last one an ellipsis when cut.
fn cap_graphemes(s: &str, max: usize) -> String {
    if s.graphemes(true).count() <= max {
        return s.to_string();
    }
    let kept: String = s.graphemes(true).take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[derive(serde::Deserialize)]
struct TimeSensitive {
    words: Vec<String>,
    phrases: Vec<String>,
}

fn time_lists() -> &'static TimeSensitive {
    static T: OnceLock<TimeSensitive> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(TIME_SENSITIVE_JSON).expect("time-sensitive.json"))
}

/// Lowercased words worth matching on.
pub fn tokens(q: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in q
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2 && !STOPWORDS.contains(w))
    {
        if !out.iter().any(|x| x == w) {
            out.push(w.to_string());
        }
    }
    out.truncate(16);
    out
}

/// FTS5 query from tokens: each one quoted (tokens are alphanumeric, so the
/// quotes cannot be broken), the last one a prefix match while typing.
pub fn fts_query(tokens: &[String], any: bool) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    let n = tokens.len();
    let parts: Vec<String> = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i + 1 == n {
                format!("\"{t}\"*")
            } else {
                format!("\"{t}\"")
            }
        })
        .collect();
    Some(parts.join(if any { " OR " } else { " " }))
}

/// Questions whose answer changes over time ("weather today") are never
/// matched to old chats.
pub fn is_time_sensitive(q: &str) -> bool {
    let lower = q.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    let t = time_lists();
    t.words.iter().any(|w| words.contains(&w.as_str()))
        || t.phrases.iter().any(|p| lower.contains(p.as_str()))
}

/// Share of `tokens` found in `text` (last token as a prefix).
pub fn coverage(tokens: &[String], text: &str) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let n = tokens.len();
    let found = tokens
        .iter()
        .enumerate()
        .filter(|(i, t)| {
            if i + 1 == n {
                words.iter().any(|w| w.starts_with(t.as_str()))
            } else {
                words.iter().any(|w| w == *t)
            }
        })
        .count();
    found as f64 / n as f64
}

/// What a search can ask for, in the panel's tab order. `in=all` asks for
/// every kind the viewer has (`/api/features` → `search.kinds`).
pub const KINDS: [&str; 4] = ["chats", "projects", "media", "artifacts"];

/// The All tab: each kind's top results, merged to at most `ALL_CAP`. With
/// no words it lists the newest `ALL_NEWEST` across kinds instead.
const ALL_PER_KIND: usize = 10;
const ALL_CAP: usize = 30;
const ALL_NEWEST: usize = 20;
/// A chat preview's length, in graphemes.
const PREVIEW_LEN: usize = 140;

/// Each kind's provider. Media and artifacts belong to the files backend.
pub fn provider(kind: &str) -> Option<Provider> {
    match kind {
        "chats" => Some(chats),
        "projects" => Some(projects),
        "media" => Some(crate::media::search),
        "artifacts" => Some(crate::artifacts::search),
        _ => None,
    }
}

/// One search as the panel asks it, owned so it can move into
/// `mirror.call`.
pub struct Request {
    /// The typed text, already capped.
    pub q: String,
    /// The kinds to ask: one tab, or for All every kind the viewer has.
    pub kinds: Vec<&'static str>,
    pub all: bool,
    /// The caller's visibility (`None`: the API token).
    pub owner: Option<String>,
    pub workspace: Option<String>,
    /// A tab's page size and offset (its cursor); All ignores both.
    pub limit: usize,
    pub offset: usize,
}

/// One page of results; `next` is the offset of the following page.
pub struct Page {
    pub results: Vec<SearchResult>,
    pub next: Option<usize>,
}

/// Run `r`: a tab pages through its one provider, All merges every
/// provider's best (or newest) results and has no next page.
pub fn run(s: &Store, r: &Request) -> harness_store::Result<Page> {
    let tokens = tokens(&r.q);
    let fts = fts_query(&tokens, false);
    let ctx = |limit, offset| SearchCtx {
        tokens: &tokens,
        fts: fts.clone(),
        owner: r.owner.as_deref(),
        workspace: r.workspace.as_deref(),
        limit,
        offset,
    };
    if r.all {
        let (n, cap) = if tokens.is_empty() {
            (ALL_NEWEST, ALL_NEWEST)
        } else {
            (ALL_PER_KIND, ALL_CAP)
        };
        let mut lists = Vec::new();
        for p in r.kinds.iter().filter_map(|k| provider(k)) {
            let mut list = checked(p(s, &ctx(n, 0))?);
            // Nothing typed: nothing to rank by, only how new it is.
            if tokens.is_empty() {
                list.iter_mut().for_each(|x| x.tier = 0);
            }
            lists.push(list);
        }
        return Ok(Page {
            results: merge(lists, cap),
            next: None,
        });
    }
    let Some(p) = r.kinds.first().and_then(|k| provider(k)) else {
        return Ok(Page {
            results: Vec::new(),
            next: None,
        });
    };
    // One more than a page tells whether there is a next one.
    let mut results = checked(p(s, &ctx(r.limit.saturating_add(1), r.offset))?);
    let next = (results.len() > r.limit).then(|| r.offset + r.limit);
    results.truncate(r.limit);
    Ok(Page { results, next })
}

/// A provider's rows with every thumbnail a path on this origin (T14). The
/// files backend builds its own; one that names another site, or that a
/// browser would read as one (`//host`, `/\host`), is dropped here, so a
/// row can never make the panel load a foreign image.
fn checked(mut rows: Vec<SearchResult>) -> Vec<SearchResult> {
    for r in &mut rows {
        r.thumb = r.thumb.take().filter(|t| same_origin(t));
    }
    rows
}

fn same_origin(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("//") && !path.starts_with("/\\")
}

/// The All tab's order: every result whose title holds all the words
/// first (tier 0), then some (1), then body matches (2); newest first
/// within a tier. At most `cap`.
pub fn merge(lists: Vec<Vec<SearchResult>>, cap: usize) -> Vec<SearchResult> {
    let mut all: Vec<SearchResult> = lists.into_iter().flatten().collect();
    all.sort_by(|a, b| a.tier.cmp(&b.tier).then(b.time_ms.cmp(&a.time_ms)));
    all.truncate(cap);
    all
}

/// A result's tier from its title (or name): 0 when it holds every word,
/// 1 some, 2 none (something else matched). Nothing typed: 0.
pub fn tier(tokens: &[String], title: &str) -> u8 {
    if tokens.is_empty() {
        return 0;
    }
    match coverage(tokens, title) {
        c if c >= 1.0 => 0,
        c if c > 0.0 => 1,
        _ => 2,
    }
}

/// `text` with each whole word that matches a token wrapped in `HIT_START`
/// and `HIT_END`: case-insensitive, the last token also as a prefix (it may
/// still be typed), the same words `coverage` counts. `text` must be free
/// of control characters, so no mark can come from it.
pub fn mark(text: &str, tokens: &[String]) -> String {
    let Some((last, rest)) = tokens.split_last() else {
        return text.to_string();
    };
    let hit = |w: &str| {
        let w = w.to_lowercase();
        w.starts_with(last.as_str()) || rest.contains(&w)
    };
    let mut out = String::with_capacity(text.len() + 8);
    let mut left = text;
    while let Some(start) = left.find(char::is_alphanumeric) {
        out.push_str(&left[..start]);
        let tail = &left[start..];
        let end = tail
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(tail.len());
        let word = &tail[..end];
        if hit(word) {
            out.push(HIT_START);
            out.push_str(word);
            out.push(HIT_END);
        } else {
            out.push_str(word);
        }
        left = &tail[end..];
    }
    out.push_str(left);
    out
}

/// An FTS excerpt on one line: line breaks read as spaces, other controls
/// go, the hit marks stay.
fn one_line(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            HIT_START | HIT_END => Some(c),
            c if c.is_whitespace() && c.is_control() => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Whether the empty search lists chat `r` for `owner` in `workspace`: the
/// chat list's rows (`list_sessions`: the caller's own, in the scope's
/// workspace), less incognito and sub-agent chats. Routine run chats are
/// left out by the caller.
fn listed(r: &SessionRow, owner: Option<&str>, workspace: Option<&str>) -> bool {
    owner.is_none_or(|o| r.owner_id.as_deref() == Some(o))
        && workspace.is_none_or(|w| r.workspace_id.as_deref() == Some(w))
        && !r.incognito
        && r.parent_id.is_none()
}

/// Chats. With words: bm25 over titles and messages, every word required,
/// paged through the deduped best-first window, routine run chats included.
/// Without: the newest visible chats, never routine run chats (they would
/// crowd out the rest).
pub fn chats(s: &Store, ctx: &SearchCtx) -> harness_store::Result<Vec<SearchResult>> {
    let mut found: Vec<(SessionRow, Option<String>)> = Vec::new();
    match &ctx.fts {
        Some(fq) => {
            let want = ctx.offset.saturating_add(ctx.limit);
            for h in s
                .search(fq, ctx.owner, ctx.workspace, want)?
                .into_iter()
                .skip(ctx.offset)
            {
                if let Some(row) = s.session(h.session_id)? {
                    found.push((row, Some(h.snippet)));
                }
            }
        }
        None => {
            let rows: Vec<SessionRow> = s
                .sessions_of(ctx.owner)?
                .into_iter()
                .filter(|r| listed(r, ctx.owner, ctx.workspace))
                .collect();
            let ids: Vec<SessionId> = rows.iter().map(|r| r.session_id).collect();
            let runs = s.routine_marks(&ids)?;
            found.extend(
                rows.into_iter()
                    .filter(|r| !runs.contains_key(&r.session_id))
                    .skip(ctx.offset)
                    .take(ctx.limit)
                    .map(|r| (r, None)),
            );
        }
    }
    chat_results(s, &found, ctx.tokens)
}

/// Rows for `found` chats (with their FTS excerpt): routine marks, latest
/// messages and project links are read for these rows only.
fn chat_results(
    s: &Store,
    found: &[(SessionRow, Option<String>)],
    tokens: &[String],
) -> harness_store::Result<Vec<SearchResult>> {
    let ids: Vec<SessionId> = found.iter().map(|(r, _)| r.session_id).collect();
    let runs = s.routine_marks(&ids)?;
    let texts = s.latest_message_texts(&ids)?;
    let mut links = HashMap::new();
    for id in &ids {
        links.extend(
            s.project_links(Some(*id))?
                .into_iter()
                .map(|l| (l.session_id, l)),
        );
    }
    Ok(found
        .iter()
        .map(|(row, snippet)| {
            let id = row.session_id;
            chat_result(
                row,
                runs.contains_key(&id),
                texts.get(&id).map(String::as_str),
                snippet.as_deref(),
                crate::projects::effective(&links, row),
                tokens,
            )
        })
        .collect())
}

/// One chat's row. `preview` is the first sentence of its latest message;
/// the excerpt is kept only when title and preview do not already show
/// every word.
fn chat_result(
    row: &SessionRow,
    routine_run: bool,
    latest: Option<&str>,
    excerpt: Option<&str>,
    project: Option<&str>,
    tokens: &[String],
) -> SearchResult {
    let title = strip_controls(&row.title).trim().to_string();
    let preview = latest
        .map(|t| first_sentence(t, PREVIEW_LEN))
        .unwrap_or_default();
    let shown = format!("{title} {preview}");
    let snippet = excerpt
        .filter(|_| !tokens.is_empty() && coverage(tokens, &shown) < 1.0)
        .map(one_line);
    let type_ = if routine_run {
        "routine"
    } else if row.profile.as_deref() == Some("agentic") {
        "agentic"
    } else {
        "chat"
    };
    SearchResult {
        kind: "chat",
        id: row.session_id.to_string(),
        tier: tier(tokens, &title),
        title: mark(&title, tokens),
        type_,
        preview: mark(&preview, tokens),
        snippet,
        time_ms: row.updated_ms,
        project_id: project.map(str::to_string),
        running: Some(row.running),
        thumb: None,
    }
}

/// Projects of the workspace, which every member sees (guests too), as the
/// project list does; the API token without a workspace sees them all.
/// With words, every word must be in the name or description; best tier
/// first, then the newest. Without, the newest.
pub fn projects(s: &Store, ctx: &SearchCtx) -> harness_store::Result<Vec<SearchResult>> {
    let tokens = ctx.tokens;
    let mut found: Vec<SearchResult> = s
        .projects(ctx.workspace)?
        .iter()
        .filter_map(|p| {
            let name = strip_controls(&p.name).trim().to_string();
            let about = strip_controls(&p.description).trim().to_string();
            let all = tokens.is_empty() || coverage(tokens, &format!("{name} {about}")) >= 1.0;
            all.then(|| SearchResult {
                kind: "project",
                id: p.project_id.clone(),
                tier: tier(tokens, &name),
                title: mark(&name, tokens),
                type_: "project",
                preview: mark(&about, tokens),
                snippet: None,
                time_ms: p.updated_ms,
                project_id: None,
                running: None,
                thumb: None,
            })
        })
        .collect();
    found.sort_by(|a, b| a.tier.cmp(&b.tier).then(b.time_ms.cmp(&a.time_ms)));
    Ok(found.into_iter().skip(ctx.offset).take(ctx.limit).collect())
}

/// "You may have asked this before": strict, never for time-sensitive
/// text, chats only. `owner` and `workspace` filter as in `Store::search`
/// (`None`: no filter, for the API token only).
pub fn suggest(
    store: &Store,
    q: &str,
    owner: Option<&str>,
    workspace: Option<&str>,
) -> harness_store::Result<Vec<SearchResult>> {
    let toks = tokens(q);
    if toks.len() < MIN_SUGGEST_TOKENS || is_time_sensitive(q) {
        return Ok(Vec::new());
    }
    let Some(fq) = fts_query(&toks, true) else {
        return Ok(Vec::new());
    };
    let mut scored: Vec<(f64, SearchHit)> = Vec::new();
    for hit in store.search(&fq, owner, workspace, 20)? {
        let text = store.session_search_text(hit.session_id)?;
        let c = coverage(&toks, &text);
        if c >= MIN_COVERAGE {
            scored.push((c, hit));
        }
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.1.score
                    .partial_cmp(&b.1.score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let mut found = Vec::new();
    for (_, h) in scored.into_iter().take(MAX_SUGGESTIONS) {
        if let Some(row) = store.session(h.session_id)? {
            found.push((row, Some(h.snippet)));
        }
    }
    chat_results(store, &found, &toks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_drop_stopwords_and_syntax() {
        assert_eq!(
            tokens("How do I configure the Tokio runtime?"),
            ["configure", "tokio", "runtime"]
        );
        // FTS5 syntax in the input becomes plain words or disappears.
        assert_eq!(
            tokens("\"rust\" NEAR(x y) -foo col:bar *"),
            ["rust", "near", "foo", "col", "bar"]
        );
        assert!(tokens("a ? !").is_empty());
    }

    #[test]
    fn fts_query_quotes_everything() {
        let t = tokens("tokio runtime thr");
        assert_eq!(
            fts_query(&t, false).unwrap(),
            "\"tokio\" \"runtime\" \"thr\"*"
        );
        assert_eq!(
            fts_query(&t, true).unwrap(),
            "\"tokio\" OR \"runtime\" OR \"thr\"*"
        );
        assert!(fts_query(&[], true).is_none());
    }

    #[test]
    fn time_sensitivity() {
        assert!(is_time_sensitive("weather today"));
        assert!(is_time_sensitive("What's the latest rust release"));
        assert!(is_time_sensitive("any events this weekend?"));
        assert!(!is_time_sensitive("how do I configure tokio runtime"));
        assert!(
            !is_time_sensitive("knowledge of nowhere"),
            "word match, not substring"
        );
    }

    #[test]
    fn first_sentence_reads_like_the_rendered_message() {
        let f = |t: &str| first_sentence(t, 140);
        assert_eq!(f("Sure. Here is how it works."), "Sure.");
        assert_eq!(
            f("## The **fix**\n\nMore text."),
            "The fix",
            "markers go, the first line with text wins"
        );
        assert_eq!(
            f("> - See [the docs](https://example.com/x) and `cargo test`. Then go."),
            "See the docs and cargo test."
        );
        assert_eq!(f("![a chart](c.png) shows it"), "a chart shows it");
        assert_eq!(f("2 * 3 is snake_case math"), "2 * 3 is snake_case math");
        assert_eq!(f("1. First step. Second."), "First step.");
        // Code fences are skipped, so the prose after them is the preview.
        assert_eq!(
            f("```rust\nfn main() {}\n```\nThat compiles. Run it."),
            "That compiles."
        );
        // A message that is only code shows its first line of code.
        assert_eq!(f("```\n\nlet x = 1;\nlet y = 2;\n```"), "let x = 1;");
        // Control characters never reach the panel.
        assert_eq!(f("bell\u{7}\u{1b}[31m red\tok"), "bell[31m red ok");
        assert_eq!(f("   \n\n"), "");
    }

    #[test]
    fn first_sentence_cuts_on_unicode_boundaries() {
        // UAX #29 knows the ideographic full stop.
        assert_eq!(first_sentence("你好。今天天气很好。", 140), "你好。");
        // "e.g." followed by lowercase is not a sentence end.
        assert_eq!(
            first_sentence("Use a builder, e.g. tokio's. Done.", 140),
            "Use a builder, e.g. tokio's."
        );
        let long = "word ".repeat(60);
        let cut = first_sentence(&long, 140);
        assert_eq!(cut.graphemes(true).count(), 140, "{cut}");
        assert!(cut.ends_with("word…"), "{cut}");
        // A family emoji is one grapheme and is never split.
        let family = "👨‍👩‍👧";
        let cut = first_sentence(&family.repeat(10), 4);
        assert_eq!(cut, format!("{}…", family.repeat(3)));
    }

    #[test]
    fn coverage_counts_words_and_prefix() {
        let t = tokens("configure tokio runtime thr");
        let c = coverage(&t, "Tokio runtime config: how many worker threads");
        // tokio, runtime, thr* match; "configure" does not ("config").
        assert!((c - 0.75).abs() < 1e-9, "{c}");
    }

    const M: char = HIT_START;
    const E: char = HIT_END;

    #[test]
    fn mark_wraps_whole_words_and_the_last_as_a_prefix() {
        let t = tokens("tokio thr");
        assert_eq!(
            mark("Tokio runtime: worker threads, tokios (TOKIO)", &t),
            format!("{M}Tokio{E} runtime: worker {M}threads{E}, tokios ({M}TOKIO{E})"),
            "earlier words match whole, case-insensitively; the last as a prefix"
        );
        assert_eq!(
            mark("Grüße aus Köln", &tokens("köl")),
            format!("Grüße aus {M}Köln{E}")
        );
        assert_eq!(mark("nothing here", &t), "nothing here");
        assert_eq!(mark("tokio", &[]), "tokio", "nothing typed, nothing marked");
        assert_eq!(mark("", &t), "");
    }

    fn result(kind: &'static str, tier: u8, time_ms: i64) -> SearchResult {
        SearchResult {
            kind,
            id: format!("{kind}{time_ms}"),
            title: String::new(),
            type_: "chat",
            preview: String::new(),
            snippet: None,
            time_ms,
            project_id: None,
            running: None,
            thumb: None,
            tier,
        }
    }

    #[test]
    fn tiers_rank_titles_then_merge_newest_first() {
        let t = tokens("tokio runtime");
        assert_eq!(tier(&t, "Tokio runtime config"), 0);
        assert_eq!(tier(&t, "Lunch with tokio"), 1);
        assert_eq!(tier(&t, "Lunch"), 2);
        assert_eq!(tier(&[], "anything"), 0);
        let merged = merge(
            vec![
                vec![result("chat", 2, 300), result("chat", 0, 100)],
                vec![result("project", 1, 500), result("project", 0, 200)],
                vec![],
            ],
            3,
        );
        let ids: Vec<_> = merged.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["project200", "chat100", "project500"]);
    }

    /// Rows as the files backend's providers make them: a thumbnail stays
    /// only as a path on this origin, and media and artifact rows merge with
    /// the rest by tier and time like any other kind.
    #[test]
    fn file_rows_keep_only_same_origin_thumbnails() {
        let thumb = |kind, tier, time, t: &str| SearchResult {
            thumb: Some(t.to_string()),
            ..result(kind, tier, time)
        };
        let rows = checked(vec![
            thumb("media", 0, 400, "/api/media/0193/thumb"),
            thumb("artifact", 0, 300, "/api/artifacts/a1/versions/v1/thumb"),
            thumb("media", 1, 200, "https://evil.example/x.png"),
            thumb("artifact", 1, 100, "//evil.example/x.png"),
            thumb("media", 2, 50, "/\\evil.example/x.png"),
            thumb("artifact", 2, 40, "api/relative.png"),
        ]);
        let thumbs: Vec<_> = rows.iter().map(|r| r.thumb.as_deref()).collect();
        assert_eq!(
            thumbs,
            [
                Some("/api/media/0193/thumb"),
                Some("/api/artifacts/a1/versions/v1/thumb"),
                None,
                None,
                None,
                None
            ]
        );
        let merged = merge(vec![vec![result("chat", 1, 500)], rows], 4);
        let ids: Vec<_> = merged.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["media400", "artifact300", "chat500", "media200"]);
        // The JSON a row sends: `thumb` only when there is one.
        let v = serde_json::to_value(&merged[1]).unwrap();
        assert_eq!(v["thumb"], "/api/artifacts/a1/versions/v1/thumb");
        assert!(
            serde_json::to_value(&merged[3])
                .unwrap()
                .get("thumb")
                .is_none()
        );
    }

    fn row(profile: &str) -> SessionRow {
        SessionRow {
            session_id: SessionId::new(),
            node_id: "master".into(),
            title: "Tokio\u{7} runtime".into(),
            cwd: String::new(),
            model: "m".into(),
            created_ms: 1,
            updated_ms: 2,
            head_seq: 3,
            running: true,
            owner_id: Some("u1".into()),
            agent_id: None,
            profile: Some(profile.into()),
            incognito: false,
            parent_id: None,
            workspace_id: Some("w1".into()),
        }
    }

    #[test]
    fn chat_rows_say_what_they_are_and_keep_the_excerpt_only_when_needed() {
        let t = tokens("tokio");
        let r = chat_result(
            &row("chat"),
            false,
            Some("Use tokio. Then go."),
            Some("…a\ntokio…"),
            Some("p1"),
            &t,
        );
        assert_eq!(r.type_, "chat");
        assert_eq!(
            r.title,
            format!("{M}Tokio{E} runtime"),
            "controls never reach the row"
        );
        assert_eq!(r.preview, format!("Use {M}tokio{E}."));
        assert_eq!(r.snippet, None, "the title already shows every word");
        assert_eq!(r.project_id.as_deref(), Some("p1"));
        assert_eq!((r.running, r.time_ms, r.tier), (Some(true), 2, 0));
        assert_eq!(
            chat_result(&row("agentic"), false, None, None, None, &t).type_,
            "agentic"
        );
        // A routine's run chat says so, whatever its profile.
        assert_eq!(
            chat_result(&row("agentic"), true, None, None, None, &t).type_,
            "routine"
        );
        let t = tokens("kale");
        let r = chat_result(
            &row("chat"),
            false,
            Some("Other words."),
            Some(&format!("…a\n{M}kale{E}\u{7}…")),
            None,
            &t,
        );
        assert_eq!(
            r.snippet.as_deref(),
            Some(format!("…a {M}kale{E}…").as_str())
        );
        assert_eq!(r.tier, 2);
        // Nothing typed: no excerpt, whatever the index said.
        assert_eq!(
            chat_result(&row("chat"), false, None, Some("x"), None, &[]).snippet,
            None
        );
    }

    #[test]
    fn the_empty_search_lists_what_the_chat_list_does() {
        let mine = row("chat");
        assert!(listed(&mine, Some("u1"), Some("w1")));
        assert!(listed(&mine, None, None), "the API token sees every chat");
        assert!(
            !listed(&mine, Some("u2"), Some("w1")),
            "never another user's chat"
        );
        assert!(
            !listed(&mine, Some("u1"), Some("w2")),
            "only in the workspace asked for"
        );
        let incognito = SessionRow {
            incognito: true,
            ..row("chat")
        };
        assert!(!listed(&incognito, Some("u1"), Some("w1")));
        let sub = SessionRow {
            parent_id: Some("x".into()),
            ..row("chat")
        };
        assert!(!listed(&sub, Some("u1"), Some("w1")));
    }
}

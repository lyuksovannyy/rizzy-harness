//! Chat search and "similar earlier chats" (PLAN.md §0 omnibox).
//!
//! User text is never passed to FTS5 as-is: it is tokenized here and every
//! token is quoted, so query syntax (`"`, `*`, `NEAR`, `-`, `:`) in the input
//! has no effect.

use std::sync::OnceLock;

use harness_store::{SearchHit, Store};

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

/// Sidebar search: every word must match (last as a prefix).
pub fn search(
    store: &Store,
    q: &str,
    owner: Option<&str>,
    limit: usize,
) -> harness_store::Result<Vec<SearchHit>> {
    match fts_query(&tokens(q), false) {
        Some(fq) => store.search(&fq, owner, limit),
        None => Ok(Vec::new()),
    }
}

/// "You may have asked this before": strict, never for time-sensitive text.
pub fn suggest(
    store: &Store,
    q: &str,
    owner: Option<&str>,
) -> harness_store::Result<Vec<SearchHit>> {
    let toks = tokens(q);
    if toks.len() < MIN_SUGGEST_TOKENS || is_time_sensitive(q) {
        return Ok(Vec::new());
    }
    let Some(fq) = fts_query(&toks, true) else {
        return Ok(Vec::new());
    };
    let mut scored: Vec<(f64, SearchHit)> = Vec::new();
    for hit in store.search(&fq, owner, 20)? {
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
    Ok(scored
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, h)| h)
        .collect())
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
    fn coverage_counts_words_and_prefix() {
        let t = tokens("configure tokio runtime thr");
        let c = coverage(&t, "Tokio runtime config: how many worker threads");
        // tokio, runtime, thr* match; "configure" does not ("config").
        assert!((c - 0.75).abs() < 1e-9, "{c}");
    }
}

//! Chat search and "similar earlier chats" (PLAN.md §0 omnibox).
//!
//! User text is never passed to FTS5 as-is: it is tokenized here and every
//! token is quoted, so query syntax (`"`, `*`, `NEAR`, `-`, `:`) in the input
//! has no effect.

use std::sync::OnceLock;

use harness_store::{SearchHit, Store};
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

/// Sidebar search: every word must match (last as a prefix). `owner` and
/// `workspace` filter as in `Store::search` (`None`: no filter, for the API
/// token only).
pub fn search(
    store: &Store,
    q: &str,
    owner: Option<&str>,
    workspace: Option<&str>,
    limit: usize,
) -> harness_store::Result<Vec<SearchHit>> {
    match fts_query(&tokens(q), false) {
        Some(fq) => store.search(&fq, owner, workspace, limit),
        None => Ok(Vec::new()),
    }
}

/// "You may have asked this before": strict, never for time-sensitive text.
/// Filters like `search`.
pub fn suggest(
    store: &Store,
    q: &str,
    owner: Option<&str>,
    workspace: Option<&str>,
) -> harness_store::Result<Vec<SearchHit>> {
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
}

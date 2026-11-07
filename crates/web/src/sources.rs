//! How web results reach the model and the UI: each page gets a session-
//! stable label (`[S4]`, see `harness_core::refs::RefBook`) in the tool output,
//! and the same label, URL and cleaned title go into `ToolOutcome::sources`
//! so the UI can turn the model's `[S4]` into a link it did not write.

use harness_core::event::Source;
use harness_core::text::clean_line;
use harness_tools::ToolCtx;

use crate::SearchResult;

/// Appended to both tool descriptions.
macro_rules! cite_hint {
    () => {
        " Cite facts from these results with their label right after the claim, e.g. [S2]; \
         never write a source URL you did not get from a tool."
    };
}
pub(crate) use cite_hint;

/// Longest snippet kept from a search result.
const MAX_SNIPPET: usize = 500;

/// The form of a URL used as a source: http(s) only, no fragment (it never
/// reaches the server, so two URLs that differ only there are one page).
pub(crate) fn source_url(raw: &str) -> Option<url::Url> {
    let mut u = url::Url::parse(raw.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return None;
    }
    u.set_fragment(None);
    Some(u)
}

fn host(u: &url::Url) -> String {
    let h = u.host_str().unwrap_or_default();
    h.strip_prefix("www.").unwrap_or(h).to_string()
}

/// A search hit as the engine gave it, cleaned: an http(s) URL, a one-line
/// title without hidden characters (the host if none is left) and a short
/// one-line snippet. `None` for a hit without a usable URL.
pub(crate) fn search_result(title: &str, url: &str, snippet: &str) -> Option<SearchResult> {
    let u = source_url(url)?;
    let mut title = clean_line(title, Source::MAX_TITLE);
    if title.is_empty() {
        title = host(&u);
    }
    Some(SearchResult {
        title,
        url: u.to_string(),
        snippet: clean_line(snippet, MAX_SNIPPET),
    })
}

/// `[S4] <title>`, the URL and the snippet per result, and their sources.
pub(crate) fn search_output(ctx: &ToolCtx, results: &[SearchResult]) -> (String, Vec<Source>) {
    let mut out = Vec::with_capacity(results.len());
    let mut sources: Vec<Source> = Vec::with_capacity(results.len());
    for r in results {
        let id = ctx.label(&r.url);
        let mut entry = format!("[{id}] {}\n    {}", r.title, r.url);
        if !r.snippet.is_empty() {
            entry.push_str("\n    ");
            entry.push_str(&r.snippet);
        }
        out.push(entry);
        // An engine can list one page twice; it has one label and one source.
        if !sources.iter().any(|s| s.id == id) {
            sources.push(Source {
                id,
                url: r.url.clone(),
                title: r.title.clone(),
            });
        }
    }
    (out.join("\n"), sources)
}

/// `[S5] <title>`, `URL: <url>` and the page text, and its source. The
/// title is the page's own (cleaned), else the host.
pub(crate) fn fetch_output(
    ctx: &ToolCtx,
    page: &url::Url,
    title: Option<&str>,
    text: &str,
) -> (String, Source) {
    let url = page.to_string();
    let title = title
        .map(|t| clean_line(t, Source::MAX_TITLE))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| host(page));
    let id = ctx.label(&url);
    let output = format!("[{id}] {title}\nURL: {url}\n\n{text}");
    (output, Source { id, url, title })
}

/// The page's `<title>`, found by a plain scan of its first 64 KiB: the
/// all-text path builds no DOM, and the title is only a label.
pub(crate) fn html_title(body: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&body[..body.len().min(64 * 1024)]);
    // ASCII lowercasing keeps every byte offset, so `lower` indexes `head`.
    let lower = head.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    Some(decode_entities(&head[start..end]))
}

/// The character references a title or snippet commonly holds: the five
/// basic named ones and numeric ones. Anything else stays as written.
pub(crate) fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
            let name = &rest[1..j];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                _ => {
                    let n = match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => name.strip_prefix('#').and_then(|d| d.parse().ok()),
                    };
                    n.and_then(char::from_u32)
                }
            };
            c.map(|c| (c, j))
        });
        match decoded {
            Some((c, j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_cleaned_and_need_an_http_url() {
        let r = search_result(
            " Tokio\u{202E} \n docs ",
            "https://tokio.rs/tokio#intro",
            "An  async\n\truntime\u{E0041}",
        )
        .unwrap();
        assert_eq!(r.title, "Tokio docs");
        assert_eq!(r.url, "https://tokio.rs/tokio");
        assert_eq!(r.snippet, "An async runtime");
        assert_eq!(
            search_result("", "https://www.a.example", "")
                .unwrap()
                .title,
            "a.example"
        );
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "not a url",
            "data:,x",
        ] {
            assert!(search_result("t", bad, "").is_none(), "{bad}");
        }
        let long = "x".repeat(300);
        assert_eq!(
            search_result(&long, "https://a.example/", "")
                .unwrap()
                .title
                .len(),
            200
        );
    }

    #[test]
    fn outputs_carry_labels_that_match_their_sources() {
        let ctx = ToolCtx::new("/");
        let rs = [
            search_result("A", "https://a.example/", "first").unwrap(),
            search_result("B", "https://b.example/", "").unwrap(),
            search_result("A again", "https://a.example/", "dup").unwrap(),
        ];
        let (out, sources) = search_output(&ctx, &rs);
        assert_eq!(
            out,
            "[S1] A\n    https://a.example/\n    first\n[S2] B\n    https://b.example/\n\
             [S1] A again\n    https://a.example/\n    dup"
        );
        assert_eq!(sources.len(), 2);
        assert_eq!(
            (sources[1].id.as_str(), sources[1].url.as_str()),
            ("S2", "https://b.example/")
        );

        let page = url::Url::parse("https://www.c.example/p").unwrap();
        let (out, s) = fetch_output(&ctx, &page, None, "body");
        assert_eq!(out, "[S3] c.example\nURL: https://www.c.example/p\n\nbody");
        assert_eq!((s.id.as_str(), s.title.as_str()), ("S3", "c.example"));
        // The same page read again keeps its label.
        let (out, _) = fetch_output(&ctx, &page, Some("C\u{200B}"), "body");
        assert!(out.starts_with("[S3] C\nURL:"), "{out}");
    }

    #[test]
    fn titles_from_html() {
        let page = b"<html><HEAD><Title lang=en>Fish &amp; chips &#8211; &#x41;&bogus;</title>";
        assert_eq!(
            html_title(page).as_deref(),
            Some("Fish & chips \u{2013} A&bogus;")
        );
        assert_eq!(html_title(b"<p>no title</p>"), None);
        assert_eq!(
            decode_entities("a & b &lt;c&gt; &quot; &#xZZ; &#99999999;"),
            "a & b <c> \" &#xZZ; &#99999999;"
        );
        // Decoding is faithful; cleaning the title drops what it decoded to.
        let sneaky = html_title(b"<title>a&#x202E;b&#0;c&#xE0041;</title>").unwrap();
        assert_eq!(clean_line(&sneaky, Source::MAX_TITLE), "ab c");
    }
}

//! Measurement harness (run by hand, `--ignored --nocapture`, release): what
//! parsing a page with the readability pass's own parser costs on the worst
//! inputs inside the byte cap. The numbers are in PLAN.md T89.

use std::time::Instant;

fn cases(size: usize) -> Vec<(&'static str, String)> {
    let rep = |unit: &str, pre: &str| {
        let n = size / unit.len();
        format!("<html><body>{pre}{}x</body></html>", unit.repeat(n))
    };
    vec![
        ("ul", rep("<ul>", "")),
        ("ol", rep("<ol>", "")),
        ("dl", rep("<dl>", "")),
        ("nav", rep("<nav>", "")),
        ("form", rep("<form>", "")),
        ("table", rep("<table>", "")),
        ("details", rep("<details>", "")),
        ("blockquote", rep("<blockquote>", "")),
        ("div", rep("<div>", "")),
        ("noscript+div", rep("<div>", "<noscript>")),
        (
            "svg foreignObject section/",
            rep("<section/>", "<svg><foreignObject>"),
        ),
        ("optgroup", rep("<optgroup>", "")),
        ("math tbody", rep("<tbody>", "<math>")),
        ("svg font style g", rep("<g>", "<svg><font><style>")),
        ("table td nest", rep("<table><tr><td>", "")),
        ("b p pile", rep("<b><p>", "")),
        ("unclosed a", rep("<a href=x>", "")),
        ("unclosed font", rep("<font>", "")),
        ("unclosed p", rep("<p>w", "")),
        ("li", rep("<li>w", "<ul>")),
        ("million tiny elements", rep("<i></i>", "")),
        (
            "attributes",
            rep("<div a=1 b=2 c=3 d=4 e=5 f=6 g=7 h=8></div>", ""),
        ),
        (
            "one div, many attrs",
            format!("<div {}>x</div>", "a=1 ".repeat(size / 4)),
        ),
        ("comments", rep("<!--c-->", "")),
        ("text", rep("word ", "")),
    ]
}

fn sizes() -> Vec<usize> {
    std::env::var("COST_SIZES")
        .map(|v| {
            v.split(',')
                .map(|n| n.parse::<usize>().unwrap() * 1024)
                .collect()
        })
        .unwrap_or_else(|_| vec![256 * 1024, 1024 * 1024])
}

fn hwm() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("")
        .to_string()
}

#[test]
#[ignore]
fn parse_cost() {
    for size in sizes() {
        for (name, html) in cases(size) {
            let h = html.clone();
            let r = std::thread::Builder::new()
                .stack_size(128 << 20)
                .spawn(move || {
                    let t = Instant::now();
                    let doc = dom_query::Document::from(h.as_str());
                    let parse = t.elapsed();
                    let t = Instant::now();
                    let (d, n) = harness_web::tree_measure(&doc);
                    let measure = t.elapsed();
                    let t = Instant::now();
                    drop(doc);
                    (parse, measure, d, n, t.elapsed())
                })
                .unwrap()
                .join()
                .unwrap();
            println!(
                "COST {size:>8} {name:<28} parse {:>10.1?} measure {:>9.1?} drop {:>9.1?} depth {:>7} nodes {:>8} | {}",
                r.0,
                r.1,
                r.4,
                r.2,
                r.3,
                hwm()
            );
        }
    }
}

/// The counting first pass on the worst inputs for the tree builder's
/// re-opening of formatting elements, and the earlier worst cases: time and
/// the process's peak memory (release, `--ignored --nocapture`).
#[test]
#[ignore]
fn budget_cost() {
    let chunk: usize = std::env::var("CHUNK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2048);
    let n = 12_500usize;
    let fmt = |open: &str, close: &str, attr: &str, tag: &str| {
        let attrs: String = (0..n).map(|i| format!("<{tag} {attr}={i}>")).collect();
        format!("{}{attrs}{}", open.repeat(n), format!("{close}x").repeat(n))
    };
    // Depth stays under the nesting limit (500 formatting elements) while the
    // paragraphs that follow re-open all of them each time: 500 x 25,000.
    let pcase = |tag: &str, attr: &str| {
        let attrs: String = (0..500).map(|i| format!("<{tag} {attr}={i}>")).collect();
        format!("<p>{attrs}{}", "<p>x".repeat(25_000))
    };
    let cases: Vec<(String, String)> = vec![
        ("div b a".into(), fmt("<div>", "</div>", "a", "b")),
        ("div a href".into(), fmt("<div>", "</div>", "href", "a")),
        (
            "div font color".into(),
            fmt("<div>", "</div>", "color", "font"),
        ),
        ("div i id".into(), fmt("<div>", "</div>", "id", "i")),
        ("p b (no end tags)".into(), pcase("b", "a")),
        ("p a".into(), pcase("a", "href")),
        ("table foster b".into(), {
            let attrs: String = (0..500).map(|i| format!("<b a={i}>")).collect();
            format!("<table>{attrs}{}", "<tr><td>x".repeat(25_000))
        }),
        ("table foster b/td".into(), {
            let attrs: String = (0..500).map(|i| format!("<b a={i}>")).collect();
            format!(
                "<table><tr><td>{attrs}{}",
                "</td></tr><tr><td>x".repeat(20_000)
            )
        }),
        ("div b 250x250".into(), {
            let attrs: String = (0..250).map(|i| format!("<b a={i}>")).collect();
            format!("{}{attrs}{}", "<div>".repeat(250), "</div>x".repeat(250))
        }),
        (
            "a inside a".into(),
            "<a href=x>".repeat(2 * n) + &"</p>x".repeat(n),
        ),
        ("nested templates".into(), "<template>".repeat(2 * n)),
        ("template + b".into(), {
            let attrs: String = (0..n).map(|i| format!("<b a={i}>")).collect();
            format!(
                "{}{attrs}{}",
                "<template>".repeat(100),
                "</template>x".repeat(100)
            )
        }),
        (
            "div x 52k".into(),
            format!("<div>{}x", "<div>".repeat(52_000)),
        ),
        (
            "div x 210k (1 MiB)".into(),
            format!("<div>{}x", "<div>".repeat(209_000)),
        ),
        (
            "ul x 250k".into(),
            format!("<ul>{}x", "<ul>".repeat(250_000)),
        ),
        (
            "foreignObject section/".into(),
            format!("<svg><foreignObject>{}x", "<section/>".repeat(100_000)),
        ),
        ("optgroup".into(), "<optgroup>".repeat(100_000)),
        ("million tiny".into(), "<i></i>".repeat(150_000)),
        ("text 1MiB".into(), "word ".repeat(200_000)),
        ("b p pile".into(), "<b><p>".repeat(170_000)),
    ];
    for (name, html) in cases {
        let t = Instant::now();
        let h = html.clone();
        let out = std::thread::Builder::new()
            .stack_size(128 << 20)
            .spawn(move || harness_web::budget_probe(&h, chunk))
            .unwrap()
            .join()
            .unwrap();
        println!(
            "BUDGET chunk {chunk} {name:<26} {:>8} bytes {:>10} {:>10.1?} | {}",
            html.len(),
            out,
            t.elapsed(),
            hwm()
        );
    }
}

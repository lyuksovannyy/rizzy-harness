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

//! A bounded differential test (review of PR 179, round 2): documents built
//! from a vocabulary of the markup that made a hand-written depth scan
//! disagree with the real tree builder, put through the tool's real entry
//! point. A fixed seed, so a failure reproduces. For every document the tool
//! either refuses it as too deeply nested (or too large) or reads it, within a
//! bound generous for a loaded machine, and never fails any other way (a
//! timeout, a panicked reader).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::Path;
use axum::response::Html;
use axum::routing::get;
use harness_tools::{Tool, ToolCtx};
use harness_web::WebFetch;
use serde_json::json;

struct R(u64);
impl R {
    fn n(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn r(&mut self, m: usize) -> usize {
        (self.n() % (m as u64)) as usize
    }
}

const TOK: &[&str] = &[
    "<div>",
    "</div>",
    "<span>",
    "</span>",
    "<b>",
    "</b>",
    "<i>",
    "<a href=x>",
    "</a>",
    "<font>",
    "<p>",
    "</p>",
    "<li>",
    "<ul>",
    "</ul>",
    "<dl>",
    "<dd>",
    "<dt>",
    "<table>",
    "</table>",
    "<tr>",
    "<td>",
    "<th>",
    "</tr>",
    "<tbody>",
    "<select>",
    "</select>",
    "<option>",
    "<optgroup>",
    "<template>",
    "</template>",
    "<svg>",
    "</svg>",
    "<math>",
    "</math>",
    "<g>",
    "<path/>",
    "<foreignObject>",
    "<desc>",
    "<title>",
    "</title>",
    "<annotation-xml encoding=text/html>",
    "<mi>",
    "<mtext>",
    "<span/>",
    "<div/>",
    "<br/>",
    "<br>",
    "<img src=a>",
    "<input>",
    "<![CDATA[",
    "]]>",
    "<!-->",
    "<!--->",
    "<!--!>",
    "<!-- x -->",
    "--!>",
    "<!x>",
    "<?x>",
    "</ x>",
    "</>",
    "<!DOCTYPE html>",
    "<script>",
    "</script>",
    "<style>",
    "</style>",
    "<textarea>",
    "</textarea>",
    "<xmp>",
    "</xmp>",
    "<noscript>",
    "</noscript>",
    "<plaintext>",
    "<iframe>",
    "</iframe>",
    "<noembed>",
    "<noframes>",
    "<object>",
    "<applet>",
    "<marquee>",
    "<button>",
    "<form>",
    "</form>",
    "<h1>",
    "<h2>",
    "</h1>",
    "<nobr>",
    "<ruby>",
    "<rt>",
    "<rp>",
    "<center>",
    "<pre>",
    "<listing>",
    "<frameset>",
    "<frame>",
    "<head>",
    "<body>",
    "<html>",
    "<caption>",
    "<colgroup>",
    "<col>",
    "<details>",
    "<summary>",
    "<fieldset>",
    "<legend>",
    "<menu>",
    "<dir>",
    "<li/>",
    "<a/>",
    "<A>",
    "<DIV>",
    "<Div>",
    "<DiV x='>'>",
    "<div a=\">\">",
    "<div a='\">'>",
    "<div a=>",
    "<div =>",
    "<div/ >",
    "<div / a>",
    "\0",
    "&lt;",
    "&amp;<div>",
    "x",
    "<image>",
    "<isindex>",
    "<keygen>",
    "<hr>",
    "<frameset>",
    "<rb>",
    "<rtc>",
    "<tt>",
    "<big>",
    "<em>",
    "<strong>",
    "<small>",
    "<s>",
    "<u>",
    "<code>",
    "<blockquote>",
    "<section>",
    "<main>",
    "<article>",
    "<aside>",
    "<nav>",
    "<figure>",
    "<address>",
    "<dialog>",
    "<slot>",
    "<bdi>",
    "<label>",
    "<canvas>",
    "<video>",
    "<audio>",
    "<picture>",
    "<source>",
    "<track>",
    "<wbr>",
    "<embed>",
    "<param>",
    "<link>",
    "<meta>",
    "<base>",
    "<basefont>",
    "<bgsound>",
    "<mglyph>",
    "<malignmark>",
    "<ms>",
    "<mn>",
    "<mo>",
    "<semantics>",
    "<mprescripts>",
];

/// The `i`-th document of the run: the same bytes every time.
fn doc(seed: u64, i: u64) -> String {
    let mut r = R(seed.wrapping_add(i).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut out: Vec<u8> = Vec::new();
    for _ in 0..1 + r.r(4) {
        let mut chunk: Vec<u8> = Vec::new();
        for _ in 0..1 + r.r(6) {
            let t = TOK[r.r(TOK.len())];
            if t == "\\0" {
                chunk.push(0);
            } else {
                chunk.extend_from_slice(t.as_bytes());
            }
            if r.r(15) == 0 {
                chunk.extend_from_slice(&[0xff, 0xfe]);
            }
        }
        let reps = [1, 1, 2, 10, 50, 200, 600, 1500][r.r(8)];
        for _ in 0..reps {
            out.extend_from_slice(&chunk);
            if out.len() > 120_000 {
                break;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

const SEED: u64 = 20_261_120;
const DOCS: u64 = 600;

#[tokio::test(flavor = "multi_thread")]
async fn every_generated_page_is_refused_or_read_within_the_bound() {
    let app = Router::new().route(
        "/d/{i}",
        get(|Path(i): Path<u64>| async move { Html(doc(SEED, i)) }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let f = WebFetch::allowing_private_start();
    let (mut read, mut refused, mut slowest) = (0, 0, Duration::ZERO);
    for i in 0..DOCS {
        let u = format!("http://{a}/d/{i}");
        let mut c = ToolCtx::new(std::env::temp_dir());
        c.observed = Arc::new(vec![u.clone()]);
        c.savings.web_readability = i % 2 == 0;
        let t = Instant::now();
        let o = f.run(&c, json!({"url": u})).await;
        let took = t.elapsed();
        slowest = slowest.max(took);
        if o.ok {
            read += 1;
        } else {
            assert!(
                o.output.contains("nested too deeply") || o.output.contains("too many elements"),
                "document {i} failed another way: {}",
                o.output
            );
            refused += 1;
        }
        assert!(took < Duration::from_secs(60), "document {i} took {took:?}");
    }
    println!("DIFF read={read} refused={refused} slowest={slowest:?}");
    // The vocabulary reaches both outcomes.
    assert!(
        read > 100 && refused > 100,
        "read {read}, refused {refused}"
    );
}

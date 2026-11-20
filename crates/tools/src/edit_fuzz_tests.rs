//! Random cases for the line form of `edit` (PR 187's review found by this
//! that a range swallowed rows inserted into it since the tag).
//!
//! A seeded generator (no dependency) makes a file and a call; an independent
//! reference applies the operations to a vector of rows from the highest row
//! down. Modes: (0) the file is the tagged version; (1) the agent's own first
//! edit changed the file and the first tag is used again; (2) the file
//! changed on disk. For a stale call that landed, two oracles: the result is
//! some valid reading (each named row is a run of equal rows in the file as
//! it is, in order), and, strictly, no row of the file as it is that the call
//! did not name was lost (all but as many rows as it named are still there,
//! in order).
//!
//! The suite runs seeds `1..=3000`. For a long run set
//! `HARNESS_EDIT_FUZZ_START` and `HARNESS_EDIT_FUZZ_N`, for example
//! `HARNESS_EDIT_FUZZ_N=30000 cargo test -p harness-tools --lib edit_fuzz`.
use serde_json::{Value, json};

use crate::{Edit, Read, Tool, ToolCtx};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn chance(&mut self, p: usize) -> bool {
        self.below(100) < p
    }
}

const ALPHA: &[&str] = &["", "a", "b", "a", "}", "  x", "dup", "dup", "é", "{", ""];

#[derive(Clone, Debug)]
struct Fx {
    rows: Vec<String>,
    eol: &'static str,
    final_nl: bool,
    bom: bool,
}
impl Fx {
    fn render(&self) -> String {
        let mut s = String::new();
        if self.bom {
            s.push('\u{feff}');
        }
        if self.rows.is_empty() {
            return s;
        }
        s.push_str(&self.rows.join(self.eol));
        if self.final_nl {
            s.push_str(self.eol);
        }
        s
    }
}

#[derive(Clone, Debug)]
enum O {
    Rep(usize, usize, Vec<String>),
    Ins(usize, Vec<String>),
}
impl O {
    fn key(&self) -> (usize, u8) {
        match self {
            O::Rep(f, _, _) => (f - 1, 1),
            O::Ins(a, _) => (*a, 0),
        }
    }
    fn lines(&self) -> &Vec<String> {
        match self {
            O::Rep(_, _, l) | O::Ins(_, l) => l,
        }
    }
}

fn reference(fx: &Fx, ops: &[O]) -> Fx {
    let n = fx.rows.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i <= n {
        for o in ops {
            if let O::Ins(a, l) = o
                && *a == i
            {
                out.extend(l.iter().cloned());
            }
        }
        let mut jumped = false;
        for o in ops {
            if let O::Rep(f, t, l) = o
                && f - 1 == i
            {
                out.extend(l.iter().cloned());
                i = *t;
                jumped = true;
                break;
            }
        }
        if jumped {
            continue;
        }
        if i < n {
            out.push(fx.rows[i].clone());
        }
        i += 1;
    }
    let final_nl = if n == 0 { true } else { fx.final_nl };
    Fx {
        rows: out,
        eol: fx.eol,
        final_nl,
        bom: fx.bom,
    }
}

fn text_of(rng: &mut Rng, lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let sep = if rng.chance(30) { "\r\n" } else { "\n" };
    let mut s = lines.join(sep);
    if lines.len() == 1 && lines[0].is_empty() {
        return sep.to_string();
    }
    if rng.chance(70) || lines.last().unwrap().is_empty() {
        s.push_str(sep);
    }
    s
}

fn gen_lines(rng: &mut Rng, max: usize) -> Vec<String> {
    (0..rng.below(max + 1))
        .map(|_| ALPHA[rng.below(ALPHA.len())].to_string())
        .collect()
}

fn gen_ops(rng: &mut Rng, n: usize) -> Vec<O> {
    let mut ops = Vec::new();
    let mut b = 0;
    loop {
        if rng.chance(25) {
            ops.push(O::Ins(b, gen_lines(rng, 3)));
        }
        if b < n && rng.chance(30) {
            let len = 1 + rng.below(4);
            let to = (b + len).min(n);
            ops.push(O::Rep(b + 1, to, gen_lines(rng, 3)));
            b = to;
            continue;
        }
        b += 1;
        if b > n {
            break;
        }
    }
    if ops.is_empty() {
        if n == 0 || rng.chance(50) {
            ops.push(O::Ins(rng.below(n + 1), gen_lines(rng, 3)));
        } else {
            let f = 1 + rng.below(n);
            ops.push(O::Rep(f, f, gen_lines(rng, 3)));
        }
    }
    ops
}

fn ops_json(rng: &mut Rng, ops: &[O]) -> Value {
    let mut v: Vec<Value> = ops
        .iter()
        .map(|o| match o {
            O::Rep(f, t, l) => json!({"from": f, "to": t, "text": text_of(rng, l)}),
            O::Ins(a, l) => json!({"after": a, "text": text_of(rng, l)}),
        })
        .collect();
    // any order
    for i in (1..v.len()).rev() {
        let j = rng.below(i + 1);
        v.swap(i, j);
    }
    Value::Array(v)
}

fn gen_fx(rng: &mut Rng) -> Fx {
    let n = if rng.chance(10) { 0 } else { rng.below(41) };
    let mut rows: Vec<String> = Vec::new();
    while rows.len() < n {
        if !rows.is_empty() && rng.chance(25) {
            // repeat a block on purpose
            let len = 1 + rng.below(4);
            let at = rng.below(rows.len());
            let blk: Vec<String> = rows[at..(at + len).min(rows.len())].to_vec();
            rows.extend(blk);
        } else {
            rows.push(ALPHA[rng.below(ALPHA.len())].to_string());
        }
    }
    rows.truncate(n);
    let mut fx = Fx {
        rows,
        eol: if rng.chance(40) { "\r\n" } else { "\n" },
        final_nl: rng.chance(60),
        bom: rng.chance(15),
    };
    canon(&mut fx);
    fx
}
/// A single empty row without a final newline is not representable.
fn canon(fx: &mut Fx) {
    if fx.rows.len() == 1 && fx.rows[0].is_empty() && !fx.final_nl {
        fx.final_nl = true;
    }
}

fn tag_of(out: &str) -> String {
    let l = out.lines().next().unwrap();
    l[l.rfind('#').unwrap() + 1..l.len() - 1].to_string()
}

fn parse_fx(text: &str) -> Fx {
    let bom = text.starts_with('\u{feff}');
    let t = text.trim_start_matches('\u{feff}');
    let eol = if t.contains("\r\n") { "\r\n" } else { "\n" };
    let final_nl = t.ends_with('\n');
    let mut rows: Vec<String> = t
        .split_inclusive('\n')
        .map(|r| {
            let r = r.strip_suffix('\n').unwrap_or(r);
            r.strip_suffix('\r').unwrap_or(r).to_string()
        })
        .collect();
    if rows.is_empty() {
        rows = vec![];
    }
    Fx {
        rows,
        eol,
        final_nl: if t.is_empty() { true } else { final_nl },
        bom,
    }
}

/// The two versions an embedding maps between.
struct Emb<'a> {
    t: &'a Fx,
    l: &'a Fx,
    /// Only the two ends of a range must match (the old, wrong reading).
    loose: bool,
}

/// every embedding result of `ops` (named in T) into L, content-only.
fn embeddings(t: &Fx, l: &Fx, ops: &[O], loose: bool) -> Vec<String> {
    let mut sorted: Vec<O> = ops.to_vec();
    sorted.sort_by_key(|o| o.key());
    let mut results = Vec::new();
    let mut acc: Vec<O> = Vec::new();
    rec(&Emb { t, l, loose }, &sorted, 0, 0, &mut acc, &mut results);
    results
}
fn rec(e: &Emb, ops: &[O], k: usize, min_start: usize, acc: &mut Vec<O>, res: &mut Vec<String>) {
    let (t, l, loose) = (e.t, e.l, e.loose);
    if k == ops.len() {
        res.push(reference(l, acc).render());
        return;
    }
    let nt = t.rows.len();
    let nl = l.rows.len();
    match &ops[k] {
        O::Rep(f, to, text) => {
            let len = to - f + 1;
            if loose {
                for (p, first) in l.rows.iter().enumerate() {
                    if p < min_start || *first != t.rows[f - 1] {
                        continue;
                    }
                    for (q, last) in l.rows.iter().enumerate().skip(p) {
                        if *last == t.rows[to - 1] {
                            acc.push(O::Rep(p + 1, q + 1, text.clone()));
                            rec(e, ops, k + 1, q + 1, acc, res);
                            acc.pop();
                        }
                    }
                }
                return;
            }
            for p in 0..nl {
                if p + len > nl || p < min_start {
                    continue;
                }
                if (0..len).all(|i| l.rows[p + i] == t.rows[f - 1 + i]) {
                    acc.push(O::Rep(p + 1, p + len, text.clone()));
                    rec(e, ops, k + 1, p + len, acc, res);
                    acc.pop();
                }
            }
        }
        O::Ins(a, text) => {
            // boundary b in L: rows a-1 and a of T at b-1 and b
            for b in min_start..=nl {
                let left_ok = if *a >= 1 {
                    b >= 1 && l.rows[b - 1] == t.rows[a - 1]
                } else {
                    b == 0
                };
                let right_ok = if *a < nt {
                    b < nl && l.rows[b] == t.rows[*a]
                } else {
                    b == nl
                };
                if left_ok && right_ok {
                    acc.push(O::Ins(b, text.clone()));
                    rec(e, ops, k + 1, b, acc, res);
                    acc.pop();
                }
            }
        }
    }
}

/// Length of the longest common subsequence of two row lists.
fn lcs_len(a: &[String], b: &[String]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    for x in a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                prev[j + 1].max(cur[j])
            };
        }
        prev = cur;
    }
    prev[b.len()]
}

#[derive(Default, Debug)]
struct Stats {
    cases: usize,
    wrote: usize,
    refused: usize,
    bad: Vec<String>,
    outside_weak: usize,
    false_refusals: usize,
    cardrun_mismatch: usize,
    gap: usize,
    swallowed: usize,
}

async fn do_edit(ctx: &ToolCtx, input: &Value) -> (bool, String, Option<String>) {
    let refusal = Edit.refusal(ctx, input);
    let preview = Edit.preview(ctx, input);
    if let Some(r) = refusal {
        return (false, r, preview);
    }
    let o = Edit.run(ctx, input.clone()).await;
    (o.ok, o.output, preview)
}

async fn one(seed: u64, st: &mut Stats) {
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    for _ in 0..3 {
        rng.next();
    }
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    let fx = gen_fx(&mut rng);
    let text = fx.render();
    let p = dir.path().join("f.txt");
    std::fs::write(&p, &text).unwrap();
    let t = parse_fx(&text);
    let r = Read.run(&ctx, json!({"path": "f.txt"})).await;
    assert!(r.ok, "{}", r.output);
    let tag0 = tag_of(&r.output);
    let mode = rng.below(3);
    st.cases += 1;
    let ctxinfo = |extra: &str| format!("seed {seed} mode {mode} {extra}\nfile {:?}", text);
    // mode 0: plain
    let mut live = t.clone();
    if mode == 1 {
        // a first edit changes rows
        let ops1 = gen_ops(&mut rng, t.rows.len());
        let j = ops_json(&mut rng, &ops1);
        let (ok, out, _) = do_edit(&ctx, &json!({"path":"f.txt","tag":tag0,"lines":j})).await;
        let exp = reference(&t, &ops1).render();
        let now = std::fs::read_to_string(&p).unwrap();
        if ok && now != exp {
            st.bad.push(format!(
                "{} EDIT1 wrong\nops {:?}\ngot {:?}\nexp {:?}",
                ctxinfo(""),
                ops1,
                now,
                exp
            ));
            return;
        }
        if !ok && now != text {
            st.bad
                .push(format!("{} EDIT1 refused but changed {out}", ctxinfo("")));
            return;
        }
        live = parse_fx(&now);
    } else if mode == 2 {
        // external change
        let mut rows = t.rows.clone();
        for _ in 0..1 + rng.below(3) {
            match rng.below(7) {
                0 if !rows.is_empty() => {
                    let i = rng.below(rows.len());
                    rows.remove(i);
                }
                1 => {
                    let i = rng.below(rows.len() + 1);
                    let l = gen_lines(&mut rng, 3);
                    for (k, x) in l.into_iter().enumerate() {
                        rows.insert(i + k, x);
                    }
                }
                2 if !rows.is_empty() => {
                    let i = rng.below(rows.len());
                    rows[i] = ALPHA[rng.below(ALPHA.len())].to_string();
                }
                3 if !rows.is_empty() => {
                    // duplicate a block right next to itself
                    let a = rng.below(rows.len());
                    let b = (a + 1 + rng.below(4)).min(rows.len());
                    let blk: Vec<String> = rows[a..b].to_vec();
                    for (k, x) in blk.into_iter().enumerate() {
                        rows.insert(b + k, x);
                    }
                }
                4 => {
                    // same rows other order
                    for i in (1..rows.len()).rev() {
                        let j = rng.below(i + 1);
                        rows.swap(i, j);
                    }
                }
                5 if rows.len() >= 2 => {
                    let a = rng.below(rows.len() - 1);
                    rows.swap(a, a + 1);
                }
                _ => {}
            }
        }
        let mut l = Fx {
            rows,
            eol: t.eol,
            final_nl: if rng.chance(10) {
                !t.final_nl
            } else {
                t.final_nl
            },
            bom: t.bom,
        };
        canon(&mut l);
        std::fs::write(&p, l.render()).unwrap();
        live = parse_fx(&l.render());
    }
    // the edit under test, tagged tag0, rows of T
    if t.rows.is_empty() && mode != 0 {
        return;
    }
    let ops = gen_ops(&mut rng, t.rows.len());
    let j = ops_json(&mut rng, &ops);
    let before = std::fs::read_to_string(&p).unwrap();
    let input = json!({"path":"f.txt","tag":tag0,"lines":j});
    let (ok, out, preview) = do_edit(&ctx, &input).await;
    let after = std::fs::read_to_string(&p).unwrap();
    if !ok {
        st.refused += 1;
        if after != before {
            st.bad
                .push(format!("{} REFUSED BUT CHANGED\n{out}", ctxinfo("")));
        }
        if mode == 0 && reference(&t, &ops).render() != before {
            st.false_refusals += 1;
            st.bad.push(format!(
                "{} FALSE REFUSAL (mode 0)\nops {:?}\n{out}",
                ctxinfo(""),
                ops
            ));
        }
        return;
    }
    st.wrote += 1;
    if mode == 0 || (live.render() == t.render()) {
        let exp = reference(&t, &ops).render();
        if after != exp {
            st.bad.push(format!(
                "{} WRONG RESULT\nops {:?}\ngot {:?}\nexp {:?}",
                ctxinfo(""),
                ops,
                after,
                exp
            ));
        }
    } else {
        let ex = embeddings(&t, &live, &ops, false);
        if !ex.contains(&after) && after != before {
            let lo = embeddings(&t, &live, &ops, true);
            if lo.contains(&after) {
                st.gap += 1;
                if st.gap <= 3 {
                    st.bad.push(format!("{} GAP-SWALLOW (range spans rows inserted since the tag)\nT {:?}\nL {:?}\nops {:?}\ngot {:?}", ctxinfo(""), t.rows, live.rows, ops, after));
                }
                return;
            }
            st.outside_weak += 1;
            st.bad.push(format!(
                "{} STALE WROTE OUTSIDE ANY VALID READING\nT rows {:?}\nL rows {:?}\nops {:?}\ngot {:?}",
                ctxinfo(""), t.rows, live.rows, ops, after
            ));
        }
        // Strictly: only the rows the call named may be gone.
        let named: usize = ops
            .iter()
            .map(|o| match o {
                O::Rep(f, to, _) => to - f + 1,
                O::Ins(..) => 0,
            })
            .sum();
        // Text that ends with a newline has two readings when the file had
        // no final newline before: "b\n" is the row `b` with a final
        // newline, and it is the rows `b` and an empty one without. Deleting
        // the last row of such a file, when the row before it is empty,
        // leaves the second reading: the empty row is still there, as the
        // text's last newline. So the result is also read that way, and the
        // check holds under one reading or the other, with no slack in
        // either (review of PR 187, round 2).
        let got = parse_fx(&after).rows;
        let kept = lcs_len(&live.rows, &got);
        let lost = |kept: usize| live.rows.len() > kept + named;
        let other_reading = || {
            let mut rows = got.clone();
            rows.push(String::new());
            lcs_len(&live.rows, &rows)
        };
        if lost(kept) && !(!live.final_nl && after.ends_with('\n') && !lost(other_reading())) {
            st.swallowed += 1;
            st.bad.push(format!(
                "{} STALE EDIT LOST ROWS IT DID NOT NAME ({} of {} rows kept, {} named)\nT rows {:?}\nL rows {:?}\nops {:?}\ngot {:?}",
                ctxinfo(""), kept, live.rows.len(), named, t.rows, live.rows, ops, after
            ));
        }
    }
    // preview vs what ran (only if a preview and the run agreed to write)
    if let Some(pv) = preview {
        // Reconstruct ops in live coordinates from the leads.
        let mut sorted = ops.clone();
        sorted.sort_by_key(|o| o.key());
        let mut recon: Vec<O> = Vec::new();
        let mut it = sorted.iter();
        let mut trunc = false;
        for line in pv.lines() {
            if line.starts_with('[') || line.starts_with("the file changed") {
                if line.starts_with("[") {
                    trunc = true;
                }
                continue;
            }
            let o = |it: &mut std::slice::Iter<O>| it.next().map(|o| o.lines().clone());
            if let Some(rest) = line
                .strip_prefix("rows ")
                .and_then(|r| r.strip_suffix(" are replaced:"))
            {
                let (a, b) = rest.split_once('-').unwrap();
                if let Some(l) = o(&mut it) {
                    recon.push(O::Rep(a.parse().unwrap(), b.parse().unwrap(), l));
                }
            } else if let Some(rest) = line
                .strip_prefix("row ")
                .and_then(|r| r.strip_suffix(" is replaced:"))
            {
                let a: usize = rest.parse().unwrap();
                if let Some(l) = o(&mut it) {
                    recon.push(O::Rep(a, a, l));
                }
            } else if line == "inserted at the top of the file:" {
                if let Some(l) = o(&mut it) {
                    recon.push(O::Ins(0, l));
                }
            } else if let Some(rest) = line
                .strip_prefix("inserted after row ")
                .and_then(|r| r.strip_suffix(':'))
                && let Some(l) = o(&mut it)
            {
                recon.push(O::Ins(rest.parse().unwrap(), l));
            }
        }
        if !trunc && recon.len() == ops.len() {
            let exp = reference(&parse_fx(&before), &recon).render();
            if exp != after {
                st.cardrun_mismatch += 1;
                st.bad.push(format!(
                    "{} PREVIEW != RUN\npreview:\n{pv}\nbefore {:?}\nafter {:?}\nreconstructed expectation {:?}",
                    ctxinfo(""), before, after, exp
                ));
            }
        }
    }
}

async fn run_seeds(start: u64, n: u64) -> Stats {
    let mut st = Stats::default();
    for seed in start..start + n {
        one(seed, &mut st).await;
        if st.bad.len() >= 12 {
            break;
        }
    }
    eprintln!(
        "seeds {start}..{} cases {} wrote {} refused {} outside_weak {} false_refusals {} \
         cardrun {} gap {} swallowed {}",
        start + n,
        st.cases,
        st.wrote,
        st.refused,
        st.outside_weak,
        st.false_refusals,
        st.cardrun_mismatch,
        st.gap,
        st.swallowed
    );
    for b in &st.bad {
        eprintln!("---\n{b}");
    }
    st
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

#[tokio::test]
async fn fuzz_line_form() {
    let st = run_seeds(
        env_u64("HARNESS_EDIT_FUZZ_START", 1),
        env_u64("HARNESS_EDIT_FUZZ_N", 3000),
    )
    .await;
    assert!(st.bad.is_empty(), "{} problems", st.bad.len());
    // The generator reaches both outcomes, and the stale modes.
    assert!(st.wrote > 100 && st.refused > 100, "{st:?}");
}

/// Seeds the review found rows swallowed with (a range spanning rows the
/// agent's own earlier edit inserted, the first tag reused).
#[tokio::test]
async fn fuzz_seeds_of_the_review_stay_fixed() {
    for seed in [90, 240, 1208] {
        let st = run_seeds(seed, 1).await;
        assert!(st.bad.is_empty(), "seed {seed}: {} problems", st.bad.len());
    }
}

/// The same generator, with the tag and the shown rows coming from a search
/// instead of a `read`: a pattern that matches a random subset of the
/// file's row values. An edit that names a row the search did not print
/// must be refused and change nothing; an edit of printed rows only must
/// equal the reference (the file is the tagged version here, so there is
/// no staleness to excuse a difference). A search with no match records
/// nothing: the tag is then unknown and every call is refused.
async fn one_search(seed: u64, st: &mut Stats) {
    use crate::Grep;
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0x5EA2C4_u64 | 1);
    for _ in 0..3 {
        rng.next();
    }
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    let fx = gen_fx(&mut rng);
    let text = fx.render();
    let p = dir.path().join("f.txt");
    std::fs::write(&p, &text).unwrap();
    let t = parse_fx(&text);
    st.cases += 1;
    let info = |extra: &str| format!("seed {seed} search {extra}\nfile {:?}", text);
    let mut values: Vec<&String> = t.rows.iter().collect();
    values.sort();
    values.dedup();
    // How much of the file the pattern matches varies, so that calls naming
    // only matched rows are common as well as calls that do not.
    let pct = [30, 70, 95][rng.below(3)];
    let chosen: Vec<&String> = values.into_iter().filter(|_| rng.chance(pct)).collect();
    let pattern = if chosen.is_empty() {
        "zzz-no-such-row".to_string()
    } else {
        let alt: Vec<String> = chosen.iter().map(|v| regex::escape(v)).collect();
        format!("^\\x{{feff}}?(?:{})$", alt.join("|"))
    };
    let matched: Vec<bool> = t.rows.iter().map(|r| chosen.contains(&r)).collect();
    let r = Grep.run(&ctx, json!({"pattern": pattern})).await;
    assert!(r.ok, "{}", r.output);
    let any = matched.iter().any(|m| *m);
    let tag = match r.output.lines().next() {
        Some(l) if l.starts_with("[f.txt#") => tag_of(&r.output),
        _ => {
            if any {
                st.bad.push(format!(
                    "{} MATCHED BUT NO TAG\n{}",
                    info(&pattern),
                    r.output
                ));
                return;
            }
            crate::seen::tag(&crate::seen::sha256(text.as_bytes()))
        }
    };
    if !any && r.output.lines().next().is_some_and(|l| l.starts_with('[')) {
        st.bad.push(format!(
            "{} TAG WITHOUT A MATCH\n{}",
            info(&pattern),
            r.output
        ));
        return;
    }
    let ops = gen_ops(&mut rng, t.rows.len());
    let named_ok = ops.iter().all(|o| match o {
        O::Rep(f, to, _) => (*f..=*to).all(|r| matched[r - 1]),
        O::Ins(a, _) => *a == 0 || matched[a - 1],
    });
    let exp = reference(&t, &ops).render();
    let j = ops_json(&mut rng, &ops);
    let input = json!({"path": "f.txt", "tag": tag, "lines": j});
    let (ok, out, _) = do_edit(&ctx, &input).await;
    let after = std::fs::read_to_string(&p).unwrap();
    let expect_write = any && named_ok && exp != text;
    if ok {
        st.wrote += 1;
        if !any || !named_ok {
            st.bad.push(format!(
                "{} WROTE ROWS THE SEARCH DID NOT SHOW\nmatched {matched:?}\nops {ops:?}\n{out}",
                info(&pattern)
            ));
        } else if after != exp {
            st.bad.push(format!(
                "{} WRONG RESULT\nops {ops:?}\ngot {after:?}\nexp {exp:?}",
                info(&pattern)
            ));
        }
    } else {
        st.refused += 1;
        if after != text {
            st.bad
                .push(format!("{} REFUSED BUT CHANGED\n{out}", info(&pattern)));
        }
        if expect_write {
            st.false_refusals += 1;
            st.bad.push(format!(
                "{} FALSE REFUSAL\nmatched {matched:?}\nops {ops:?}\n{out}",
                info(&pattern)
            ));
        }
    }
}

#[tokio::test]
async fn fuzz_search_tags() {
    let (start, n) = (
        env_u64("HARNESS_EDIT_FUZZ_START", 1),
        env_u64("HARNESS_EDIT_FUZZ_N", 3000),
    );
    let mut st = Stats::default();
    for seed in start..start + n {
        one_search(seed, &mut st).await;
        if st.bad.len() >= 12 {
            break;
        }
    }
    eprintln!(
        "search seeds {start}..{} cases {} wrote {} refused {} false_refusals {}",
        start + n,
        st.cases,
        st.wrote,
        st.refused,
        st.false_refusals
    );
    for b in &st.bad {
        eprintln!("---\n{b}");
    }
    assert!(st.bad.is_empty(), "{} problems", st.bad.len());
    assert!(st.wrote > 50 && st.refused > 50, "{st:?}");
}

// Rich messages end to end, against the real binary with the mock provider.
// Each item of the build plan's e2e list (rich_plan 12.4) is checked once,
// in the section named next to it; the sections also cover what the reviews
// of each step added:
//   1  rich: demo (highlighting, math, callout, image chip, prices, link
//      confirm, no requests to model-chosen hosts)   rich/markdown.cjs
//   2  lazy chunks: none for an echo, math and highlight for the demo
//                                                     rich/markdown.cjs
//   3  ui: choices (skeleton, card, tap, answer, locked after reload)
//   9  stale question (Not answered, late answer 409)
//   6  rejected, unknown and second blocks
//   5  ui: secret (no input for the key, Name works)
//   4  ui: form (Send waits, inline error, people: 3)   rich/blocks.cjs
//   8  progress: 3 on the local node (one card, snapshots, reload)
//                                                     rich/progress.cjs
//   11 ui: chart (marks, Show data with 3 rows)       rich/chart.cjs
//   10 remember: the memory notice                    rich/memory.cjs
//   7  cite: rust (pills, [S9], Sources (2), web row)  rich/cite.cjs
// Every check is written so that it fails when its feature is gone: a
// "none" is always paired with a "one" of the same thing (no chunk for an
// echo, then exactly one for the demo), and states are compared whole.
// Usage: node web/e2e/rich.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { check, log, sleep, chat } = require('./rich/lib.cjs');

const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-rich-'));
const R = `${S}/repo`;
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(R, { recursive: true });
const URL_ = 'http://127.0.0.1:7797/';
const SECTIONS = ['markdown', 'blocks', 'progress', 'chart', 'memory', 'cite'];

// A fake SearXNG on loopback (configured by the admin, so allowed). A
// query with "slow" in it answers after a while, so the running row shows;
// one with "other" finds a new page and one already known.
const searx = http.createServer((req, res) => {
  const q = new URL(req.url, 'http://x').searchParams.get('q') || '';
  res.setHeader('content-type', 'application/json');
  const first = q.includes('other')
    ? { title: 'Other page', url: 'https://c.example/other', content: 'Something else' }
    : { title: 'Rust language', url: 'https://a.example/rust', content: 'A language empowering everyone' };
  setTimeout(() => res.end(JSON.stringify({ results: [
    first,
    { title: 'The Rust book', url: 'https://b.example/book', content: 'Learn Rust' },
  ] })), q.includes('slow') ? 2500 : 0);
});

async function signUp(p, code) {
  await p.goto(URL_);
  await p.fill('input[placeholder="claim code"]', code);
  await p.fill('input[placeholder="username"]', 'alex');
  await p.fill('input[type=password]', 'admin password');
  await p.click('text=Create account');
  await p.waitForSelector('text=Connect an AI provider');
  await p.click('text=Skip: use the built-in demo');
  await p.click('text=Skip: keywords only');
  await p.waitForSelector('text=About you');
  await p.click('button:has-text("Continue")');
  await p.waitForSelector('h2:has-text("Search engine")');
  await p.click('button:has-text("Finish")');
  await p.waitForSelector('h1.greeting');
}

(async () => {
  await new Promise((r) => searx.listen(0, '127.0.0.1', r));
  fs.writeFileSync(`${S}/data/harness.toml`, [
    '[provider]', 'kind = "mock"', 'mock_delay_ms = 20', '',
    '[web]', 'search = "searxng"', `searxng_url = "http://127.0.0.1:${searx.address().port}"`, '',
  ].join('\n'));
  fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${R}"]\n`);
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7797'], { detached: true, stdio: 'ignore' });
  let b;
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
    const p = await ctx.newPage();
    const errors = [];
    // Every request of every page, also those the CSP blocks (Chromium
    // reports them, as failed).
    const requests = [];
    p.on('pageerror', (e) => errors.push(e.message));
    ctx.on('request', (r) => requests.push(r.url()));
    const chunks = (name) => requests.filter((u) => new RegExp(`/assets/${name}-[^/]*\\.js$`).test(u));
    await signUp(p, code);
    const t = { p, ctx, S, requests, chunks, ...chat(p, R) };
    for (const name of SECTIONS) await require(`./rich/${name}.cjs`)(t);
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
  } finally {
    if (b) await b.close();
    process.kill(-srv.pid, 'SIGKILL');
    searx.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });

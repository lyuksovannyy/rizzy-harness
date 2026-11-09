// Phase 9 end-to-end: parallel answers (compare two models, continue from
// one), dictation through a fake transcription service (Chromium's fake
// microphone), and undoing an agentic turn's file changes.
// Usage: node web/e2e/phase9.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e9-'));
const D = `${S}/data`, R = `${S}/repo`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(R, { recursive: true });
const URL_ = 'http://127.0.0.1:7793/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

// Fake OpenAI-compatible transcription endpoint.
let audioBytes = 0;
const whisper = http.createServer((req, res) => {
  let n = 0;
  req.on('data', (c) => (n += c.length));
  req.on('end', () => {
    audioBytes = n;
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ text: 'dictated hello' }));
  });
});

(async () => {
  await new Promise((r) => whisper.listen(0, '127.0.0.1', r));
  fs.writeFileSync(`${D}/config.toml`, [
    // Two demo models to compare (demo::mock and demo::mock-big).
    '[[providers]]', 'id = "demo"', 'name = "Demo"', 'preset = "demo"', 'kind = "mock"', 'mock_delay_ms = 5',
    'models = [{ id = "mock" }, { id = "mock-big" }]', '',
    '[voice]', `url = "http://127.0.0.1:${whisper.address().port}/v1"`, 'model = "tiny"', '',
  ].join('\n'));
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${R}"]\n`);
  const git = (...a) => execFileSync('git', ['-C', R, ...a], { env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' } });
  git('init', '-q');
  fs.writeFileSync(`${R}/a.txt`, 'original\n');
  git('add', '.');
  git('commit', '-qm', 'init');

  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', '127.0.0.1:7793'], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    const b = await chromium.launch({ args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] });
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 }, permissions: ['microphone'] });
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    // The demo skip keeps the configured demo provider and its two models.
    const models = await p.evaluate(() => fetch('/api/models').then((r) => r.json()));
    const ids = models.models.map((m) => m.id);
    check(ids.includes('demo::mock') && ids.includes('demo::mock-big'), 'two demo models: ' + ids.join(', '));

    // 1. Dictation fills the bar.
    await p.click('button[aria-label="dictate"]');
    await sleep(1200);
    await p.click('button[aria-label="stop dictation"]');
    await p.waitForFunction(() => document.querySelector('.composer textarea').value.includes('dictated hello'));
    log('dictation:', await p.inputValue('.composer textarea'), `(${audioBytes} bytes sent)`);
    check(audioBytes > 0, 'audio reached the transcription service');

    // 2. Parallel answers.
    await p.fill('.composer textarea', 'compare these answers');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.selectOption('select[aria-label="compare with"]', 'demo::mock-big');
    await p.click('.composer .send');
    await p.waitForSelector('.variants .msg.variant >> nth=1');
    await p.waitForSelector('text=[mock-big] You said: compare these answers');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    await p.click('.msg.variant:has-text("[mock-big]") button:has-text("Continue with this")');
    await p.waitForSelector('.msg.variant.chosen:has-text("[mock-big]")');
    await p.screenshot({ path: `${S}/p9-1-compare.png` });
    await p.reload();
    await p.waitForSelector('.msg.variant.chosen:has-text("[mock-big]")');
    log('compare: two answers side by side; choice persisted');

    // 3. Undo an agentic turn's file changes.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'run: echo changed > a.txt; echo new > b.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.agentic-row input', R);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.undo >> text=2 files changed');
    check(fs.readFileSync(`${R}/a.txt`, 'utf8') === 'changed\n', 'turn changed the file');
    await p.click('button:has-text("Undo file changes")');
    await p.waitForSelector('.undo >> text=undone');
    check(fs.readFileSync(`${R}/a.txt`, 'utf8') === 'original\n', 'undo restored a.txt');
    check(!fs.existsSync(`${R}/b.txt`), 'undo removed the created file');
    check(execFileSync('git', ['-C', R, 'status', '--porcelain']).toString().trim() === '', 'repo clean again; index untouched');
    await p.screenshot({ path: `${S}/p9-2-undo.png` });
    log('undo restored the files; git status clean');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/p9-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    whisper.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });

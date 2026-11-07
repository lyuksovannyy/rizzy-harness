// Phase 7 end-to-end: memory. Onboarding turns on embeddings (fake server),
// the model proposes a memory, the user confirms it, a later chat recalls it
// by meaning ("puppy" finds "dog").
// Usage: node web/e2e/phase7.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e7-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/harness.toml`, '[provider]\nkind = "mock"\nmock_delay_ms = 2\n');
const URL_ = 'http://127.0.0.1:7795/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

// Fake OpenAI-compatible /embeddings: one dimension per topic.
let embedCalls = 0;
const emb = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    embedCalls++;
    const { input } = JSON.parse(body);
    const vec = (t) => {
      t = t.toLowerCase();
      if (/dog|puppy/.test(t)) return [1, 0, 0];
      if (/tram|bus/.test(t)) return [0, 1, 0];
      return [0, 0, 1];
    };
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ data: input.map((t, i) => ({ index: i, embedding: vec(t) })) }));
  });
});

(async () => {
  await new Promise((r) => emb.listen(0, '127.0.0.1', r));
  const embUrl = `http://127.0.0.1:${emb.address().port}/v1`;
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7795'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 860 } });
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.waitForSelector('text=Connect an AI provider');
    await p.click('text=Skip: use the built-in demo');

    // Memory step: a wrong endpoint is caught by the test call...
    await p.waitForSelector('h2:has-text("Memory")');
    await p.click('[role=radiogroup][aria-label=recall] button:has-text("Meaning")');
    // Only the demo provider is set up, so embeddings come from Ollama.
    await p.waitForSelector('[role=radiogroup][aria-label="embeddings source"] button[aria-checked=true]:has-text("Ollama")');
    const endpoint = 'label:has-text("Ollama endpoint") input';
    await p.fill(endpoint, 'http://127.0.0.1:1/v1');
    await p.click('text=Save and continue');
    await p.waitForSelector('.error-text');
    log('bad endpoint refused:', (await p.innerText('.error-text')).slice(0, 60));
    // ...the right one is saved.
    await p.fill(endpoint, embUrl);
    await p.click('[role=radiogroup][aria-label="embeddings model preset"] button:has-text("Fast")');
    await p.screenshot({ path: `${S}/p7-1-onboarding.png` });
    await p.click('text=Save and continue');
    await p.waitForSelector('text=About you');
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    const toml = fs.readFileSync(`${S}/data/harness.toml`, 'utf8');
    check(toml.includes(embUrl) && toml.includes('preset = "fast"'), 'memory config saved');

    // The model proposes a memory; Home offers to review it.
    await p.fill('.composer textarea', 'remember: Alex has a dog named Pixel');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=Tool said: Proposed.');
    await p.click('.new-chat');
    await p.waitForSelector('.review-link');
    log('home:', await p.innerText('.review-link'));
    await p.click('.review-link');
    await p.waitForSelector('.fact.proposed:has-text("Pixel")');
    await p.screenshot({ path: `${S}/p7-2-review.png` });

    // Not used before confirming.
    await p.click('.new-chat');
    await p.fill('.composer textarea', 'memory? how is my puppy doing');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=Memory: nothing recalled.');
    log('unconfirmed proposal not recalled');

    // Confirm; now "puppy" recalls the dog fact by meaning.
    await p.goto(`${URL_}#/settings/memory`);
    await p.click('.fact.proposed button:has-text("Remember")');
    await p.waitForSelector('.fact.active:has-text("Pixel")');
    await p.click('.new-chat');
    await p.fill('.composer textarea', 'memory? how is my puppy doing');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant >> text=Alex has a dog named Pixel');
    log('recalled by meaning; embedding calls:', embedCalls);

    // Forget removes it.
    await p.goto(`${URL_}#/settings/memory`);
    await p.click('.fact.active button:has-text("Forget")');
    await p.waitForSelector('text=Nothing yet.');
    await p.screenshot({ path: `${S}/p7-3-memory.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    emb.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });

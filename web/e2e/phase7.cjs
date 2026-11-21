// Phase 7 end-to-end: memory. Onboarding turns on embeddings (fake server),
// the model's `remember` call from the person's own message in their Personal
// workspace is kept at once (the chat says so, with Undo and a link to the
// list), a later chat recalls it by meaning ("puppy" finds "dog"), Forget
// removes it.
// Usage: node web/e2e/phase7.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e7-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 2\nmodels = [{ id = "mock" }]\n');
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
    const code = await claimCode(`${S}/data/claim-code`, URL_);
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
    await p.click('form button.primary:has-text("Save and continue")');
    await p.waitForSelector('.error-text');
    log('bad endpoint refused:', (await p.innerText('.error-text')).slice(0, 60));
    // ...the right one is saved.
    await p.fill(endpoint, embUrl);
    await p.click('[role=radiogroup][aria-label="embeddings model preset"] button:has-text("Fast")');
    await p.screenshot({ path: `${S}/p7-1-onboarding.png` });
    await p.click('form button.primary:has-text("Save and continue")');
    await p.waitForSelector('text=About you');
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    const toml = fs.readFileSync(`${S}/data/config.toml`, 'utf8');
    check(toml.includes(embUrl) && toml.includes('preset = "fast"'), 'memory config saved');

    // The model's remember call is kept at once: the chat says so, with Undo.
    await p.fill('.composer textarea', 'remember: Alex has a dog named Pixel');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=Tool said: Sent to memory');
    await p.waitForSelector('.memory-fact:has-text("Pixel")');
    await p.waitForSelector('button.memory-undo');
    log('kept at once, with Undo:', (await p.innerText('.memory-undo')).trim());
    await p.screenshot({ path: `${S}/p7-1b-kept.png` });
    await p.click('.memory-review');
    await p.waitForSelector('.fact.active:has-text("Pixel")');
    check((await p.locator('.fact.proposed').count()) === 0, 'nothing waits for review');
    await p.screenshot({ path: `${S}/p7-2-review.png` });

    // A later chat recalls it: "puppy" finds the dog fact by meaning.
    await p.click('.new-chat');
    await p.fill('.composer textarea', 'memory? how is my puppy doing');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant >> text=Alex has a dog named Pixel');
    log('recalled by meaning; embedding calls:', embedCalls);

    // Forget removes it.
    await p.goto(`${URL_}#/settings/account/memory`);
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
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

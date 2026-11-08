// Phase 8 end-to-end with two real processes: a master with a node listener
// and a remote node that joins with a one-time command from the UI, runs an
// approved tool, serves Ultracode's parallel sub-agents, and is revoked.
// Usage: node web/e2e/phase8.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e8-'));
const A = `${S}/master`, B = `${S}/remote`;
for (const d of [A, B, `${B}/ws`]) fs.mkdirSync(d, { recursive: true });
fs.writeFileSync(`${A}/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 10\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${A}/node.toml`, `workspace_roots = ["${A}"]\n`);
fs.writeFileSync(`${B}/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 10\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${B}/node.toml`, `workspace_roots = ["${B}/ws"]\n`);
const URL_ = 'http://127.0.0.1:7794/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', A, 'serve', '--bind', '127.0.0.1:7794', '--node-listen', '127.0.0.1:7744'], { detached: true, stdio: 'ignore' });
  let nodeProc = null;
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${A}/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 900 } });
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // Add a node from the UI.
    await p.goto(`${URL_}#/settings/server/nodes`);
    await p.click('text=Create join command');
    const cmd = await p.innerText('.join-cmd');
    await p.screenshot({ path: `${S}/p8-1-join.png` });
    const token = /--token (\S+)/.exec(cmd)[1];
    const fp = /--fingerprint (\S+)/.exec(cmd)[1];
    const url = /join (wss:\/\/\S+)/.exec(cmd)[1];
    log('join url:', url, '| fingerprint', fp.slice(0, 20) + '…');
    // A wrong fingerprint is refused before anything is sent.
    let refused = false;
    try {
      execFileSync(BIN, ['--data-dir', `${S}/evil`, 'node', 'join', url, '--token', token, '--fingerprint', 'sha256:' + '0'.repeat(64)], { stdio: 'pipe' });
    } catch { refused = true; }
    check(refused, 'pinned fingerprint mismatch must fail');
    const out = execFileSync(BIN, ['--data-dir', B, 'node', 'join', url, '--token', token, '--fingerprint', fp]).toString();
    log(out.split('\n')[0]);
    const nodeId = /node `([^`]+)`/.exec(out)[1];
    nodeProc = spawn(BIN, ['--data-dir', B, 'node', 'run'], { detached: true, stdio: 'ignore' });
    await p.waitForSelector(`tr:has-text("${nodeId}") >> text=online`, { timeout: 15000 });
    log('remote node online:', nodeId);

    // An agentic task on the remote node, with an approval.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'run: echo remote-ok > out.txt && cat out.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.selectOption('.agentic-row select', nodeId);
    await p.fill('.agentic-row input', `${B}/ws`);
    await p.click('.composer .send');
    await p.waitForSelector('.approval');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('text=Tool said: remote-ok');
    check(fs.readFileSync(`${B}/ws/out.txt`, 'utf8').trim() === 'remote-ok', 'file written on the remote node');
    log('tool ran on the remote node after approval');

    // Ultracode: parallel sub-agents in child sessions.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'delegate: scout=look around | writer=draft a note');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.selectOption('.composer select[aria-label="agent"]', 'ultracode');
    await p.selectOption('.agentic-row select', nodeId);
    await p.fill('.agentic-row input', `${B}/ws`);
    await p.click('.composer .send');
    await p.waitForSelector('.tool >> text=@writer: draft a note');
    await p.waitForFunction(() => document.querySelectorAll('.tool.tool-ok').length >= 2);
    await p.screenshot({ path: `${S}/p8-2-ultracode.png` });
    const recents = await p.innerText('.sidebar');
    check(!recents.includes('@scout:'), 'sub-agent sessions stay out of Recents');
    await p.click('.tool:has-text("@writer: draft") .child-link');
    await p.waitForSelector('text=Back to the main session');
    await p.waitForSelector('text=You said: draft a note');
    log('child session reachable from the tool card');

    // Revoke: the node is cut off and its process stops.
    await p.goto(`${URL_}#/settings/server/nodes`);
    p.once('dialog', (d) => d.accept());
    await p.click(`tr:has-text("${nodeId}") button:has-text("Revoke")`);
    await p.waitForSelector(`tr:has-text("${nodeId}") >> text=revoked`);
    let exited = false;
    for (let i = 0; i < 60 && !exited; i++) {
      await sleep(250);
      exited = nodeProc.exitCode !== null;
    }
    check(exited, 'revoked node process should stop');
    log('revoked; node exited with', nodeProc.exitCode);
    await p.screenshot({ path: `${S}/p8-3-revoked.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/p8-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    if (nodeProc && nodeProc.exitCode === null) process.kill(-nodeProc.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });

// Server setup over SSH in the browser, manual mode, against the fake SSH
// server example: check identity, confirm, password once, checklist,
// harness's key installed, re-check with the key only.
// Usage: node web/e2e/setup.cjs <harness-binary> <fake_sshd-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const SSHD = path.resolve(process.argv[3] || 'target/debug/examples/fake_sshd');
const S = process.argv[4] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2es-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/harness.toml`, '[provider]\nkind = "mock"\nmock_delay_ms = 5\n');
const URL_ = 'http://127.0.0.1:7791/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const sshd = spawn(SSHD, [], { stdio: ['ignore', 'pipe', 'inherit'] });
  const info = await new Promise((r) => sshd.stdout.once('data', (d) => r(d.toString())));
  const port = /port=(\d+)/.exec(info)[1];
  const fp = /fingerprint=(\S+)/.exec(info)[1];
  const home = /home=(\S+)/.exec(info)[1];
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7791', '--node-listen', '127.0.0.1:0'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 1000 } });
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await p.fill('input[placeholder="claim code"]', code);
    await p.fill('input[placeholder="username"]', 'alex');
    await p.fill('input[type=password]', 'admin password');
    await p.click('text=Create account');
    await p.click('text=Skip: use the built-in demo');
    await p.click('text=Skip: keywords only');
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    await p.goto(`${URL_}#/settings`);
    await p.click('.tabs button:has-text("Nodes")');

    await p.fill('input[placeholder="203.0.113.7"]', '127.0.0.1');
    await p.fill('input[type=number]', port);
    await p.fill('label:has-text("User") input', 'deploy');
    await p.click('text=Check the server\'s identity');
    await p.waitForSelector(`code:has-text("${fp}")`);
    log('host key shown:', fp.slice(0, 20) + '…');
    await p.fill('input[type=password]', 'pw-harness-test');
    check(await p.isDisabled('button:has-text("Set up automatically")'), 'cannot start before confirming the fingerprint');
    await p.check('text=It matches');
    await p.click('text=Manual');
    await p.screenshot({ path: `${S}/s-1-form.png` });
    await p.click('button:has-text("Check the server")');
    await p.waitForSelector('.setup-step.st-ok:has-text("Harness login key")');
    await p.waitForSelector('.setup-step:has-text("Harness node") >> text=--token');
    await p.screenshot({ path: `${S}/s-2-checklist.png`, fullPage: true });
    const steps = await p.$$eval('.setup-step', (els) => els.map((e) => e.querySelector('b').textContent + ':' + e.className.split('st-')[1]));
    log('checklist:', steps.join(', '));
    const ak = fs.readFileSync(`${home}/.ssh/authorized_keys`, 'utf8');
    check(ak.includes('harness'), 'harness key installed');
    check(!JSON.stringify(fs.readdirSync(`${S}/data`, { recursive: true })).includes('pw-harness'), 'no password file');
    await p.click('button:has-text("Check again")');
    await sleep(800);
    check(!(await p.$('.error-text')), 'recheck with harness key works');
    log('re-check ok with harness key');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/s-fail.png`, fullPage: true }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    sshd.kill('SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });

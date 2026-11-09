// Server setup over SSH in the browser, manual mode, against the fake SSH
// server example: check identity, confirm, password once, checklist,
// harness's key installed, re-check with the key only.
// Usage: node web/e2e/setup.cjs <harness-binary> <fake_sshd-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const SSHD = path.resolve(process.argv[3] || 'target/debug/examples/fake_sshd');
const S = process.argv[4] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2es-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
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
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 1000 } });
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
    await p.goto(`${URL_}#/settings/server/nodes`);

    await p.fill('input[placeholder="203.0.113.7"]', '127.0.0.1');
    await p.fill('input[type=number]', port);
    await p.fill('label:has-text("User") input', 'deploy');
    await p.click('text=Check the server\'s identity');
    await p.waitForSelector(`code:has-text("${fp}")`);
    log('host key shown:', fp.slice(0, 20) + '…');
    await p.fill('input[type=password]', 'pw-harness-test');
    check(await p.isDisabled('button:has-text("Set up automatically")'), 'cannot start before confirming the fingerprint');
    await p.check('text=It matches');
    // Automatic mode with a normal user: explained and blocked.
    await p.waitForSelector('.hint-warn >> text=needs the root login');
    check(await p.isDisabled('button:has-text("Set up automatically")'), 'automatic needs root');
    // Docker shows the root-equivalence warning.
    await p.selectOption('select:near(:text("Container engine"))', 'docker');
    await p.waitForSelector('.hint-warn >> text=effectively root');
    await p.selectOption('select:near(:text("Container engine"))', 'podman');
    await p.click('text=Manual');
    // Manual mode with root: warned, not refused.
    await p.fill('label:has-text("User") input', 'root');
    await p.waitForSelector('.hint-warn >> text=use a normal user');
    await p.fill('label:has-text("User") input', 'deploy');
    await p.screenshot({ path: `${S}/s-1-form.png` });
    await p.click('button:has-text("Check the server")');
    await p.waitForSelector('.setup-step.st-ok:has-text("Harness login key")');
    // The fake server runs commands as this test's user; as root, harness asks.
    // "Keep" avoids creating a real system user on the test machine.
    const asked = await p.waitForSelector('.setup-step:has-text("Harness\'s own user") button:has-text("Keep using root")', { timeout: 3000 }).catch(() => null);
    if (asked) {
      log('asked about its own user (root login)');
      await asked.click();
      await p.waitForSelector('.setup-step.st-warning:has-text("Harness\'s own user")');
    }
    await p.waitForSelector('.setup-step:has-text("Harness node") >> text=--token');
    await p.waitForSelector('.setup-step:has-text("Container engine") >> text=rootless Podman');
    await p.waitForSelector('.setup-step:has-text("Operating system")');
    await p.screenshot({ path: `${S}/s-2-checklist.png`, fullPage: true });
    const steps = await p.$$eval('.setup-steps:not(.managed) .setup-step', (els) => els.map((e) => e.querySelector('b').textContent + ':' + e.className.split('st-')[1]));
    log('checklist:', steps.join(', '));
    const ak = fs.readFileSync(`${home}/.ssh/authorized_keys`, 'utf8');
    check(ak.includes('harness'), 'harness key installed');
    check(!JSON.stringify(fs.readdirSync(`${S}/data`, { recursive: true })).includes('pw-harness'), 'no password file');
    await p.click('button:has-text("Check again")');
    await sleep(800);
    check(!(await p.$('.error-text')), 'recheck with harness key works');
    log('re-check ok with harness key');

    // Saved setup: managed after the fact.
    const srvCard = '.setup-step:has(b:text-is("127.0.0.1"))';
    await p.waitForSelector(`${srvCard} select`);
    check((await p.$eval(`${srvCard} select`, (e) => e.value)) === 'manual', 'mode saved as manual');
    await p.waitForSelector(`${srvCard} summary:has-text("Commands: firewall on")`);
    check(!(await p.$(`${srvCard} button:has-text("Turn the firewall")`)), 'no firewall button in manual mode');
    // The fake server still allows passwords: the watchdog reports it.
    await p.click(`${srvCard} button:has-text("Check now")`);
    await p.waitForSelector(`${srvCard} .error-text:has-text("assword")`);
    log('watchdog: password logins reported');
    await p.goto(URL_);
    await p.waitForSelector('.attention >> text=127.0.0.1');
    log('node problem shown under the bar');
    await p.goto(`${URL_}#/settings/server/nodes`);
    await p.waitForSelector(`${srvCard} select`);
    // Turning the SSH watch off clears the problem.
    await p.click(`${srvCard} label:has-text("Watch SSH") input`);  // controlled: flips after the save
    await p.waitForSelector(`${srvCard} label:has-text("Watch SSH") input:not(:checked)`);
    await p.waitForSelector(`${srvCard} .error-text`, { state: 'detached' });
    // Switch to automatic: the firewall button appears; turning it on asks for a root login.
    await p.selectOption(`${srvCard} select`, 'auto');
    await p.click(`${srvCard} button:has-text("Turn the firewall on")`);
    await p.waitForSelector(`${srvCard} textarea[placeholder*="root's private key"]`);
    await p.click(`${srvCard} button:has-text("Cancel")`);
    await p.screenshot({ path: `${S}/s-3-managed.png`, fullPage: true });
    log('mode switch, watch toggle, root-login prompt ok');

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

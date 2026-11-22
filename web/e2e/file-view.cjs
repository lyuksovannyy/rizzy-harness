// Read-only files in a plain chat (T95, ADOPTION_PLAN X.54) end to end: a zip
// attached in an Ask chat is kept (the chip says read-only with the size, the
// sent message shows the file), the agent lists it and reads a text entry as
// numbered lines and a binary entry as a hex dump through `attached_file`
// (the calls show as tool blocks), an agent without the switch is refused
// with the switch's name, and the Agents page shows the switch on for the
// Assistant. At desktop and phone width.
// Usage: node web/e2e/file-view.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync } = require('child_process');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-fileview-'));
const D = `${S}/data`, F = `${S}/files`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(F, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;

// A real zip made with Python's zipfile: a text entry, a binary one, a folder.
function makeFiles() {
  execFileSync('python3', ['-I', '-c', `
import zipfile, sys
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("hello.txt", "alpha line\\nbeta line\\ngamma line\\n" * 20)
    z.writestr("data/blob.bin", bytes([0, 1, 2, 3, 250, 251, 252, 253]) * 8)
    z.writestr("data/", "")
`, `${F}/Q3 report.zip`]);
}

(async () => {
  makeFiles();
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/config.toml`, [
    '[[providers]]', 'id = "demo"', 'name = "Demo"', 'preset = "demo"', 'kind = "mock"', 'mock_delay_ms = 2',
    'models = [{ id = "mock", vision = true }]', '',
  ].join('\n'));
  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    // The upload's answer, as the page got it (a response body is gone from
    // the inspector by the time a handler asks).
    await ctx.addInitScript(() => {
      const f = window.fetch.bind(window);
      window.__up = [];
      window.fetch = async (u, o = {}) => {
        const r = await f(u, o);
        if (String(u).startsWith('/api/attachments') && (o.method || 'GET') === 'POST') {
          r.clone().json().then((body) => window.__up.push({ status: r.status, body })).catch(() => {});
        }
        return r;
      };
    });
    const uploads = () => p.evaluate(() => window.__up);
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // 1. A plain (Ask) chat keeps the zip: the chip says read-only, with its size.
    await p.click('.mode-switch button:has-text("Ask")');
    await p.setInputFiles('.composer input[type=file]', `${F}/Q3 report.zip`);
    await p.waitForSelector('.composer .chip:has-text("read-only")');
    const chip = (await p.textContent('.composer .chip')).trim();
    check(/Q3 report\.zip/.test(chip) && /\d+(\.\d)? (B|KB)/.test(chip) && /read-only/.test(chip), 'the chip: ' + chip);
    await p.waitForFunction(() => window.__up.some((u) => u.status === 200));
    const up = (await uploads()).find((u) => u.status === 200);
    check(up && up.body.kept === true && !up.body.saved, 'the server kept it: ' + JSON.stringify(up));
    const fid = up.body.attachment_id;
    await p.screenshot({ path: `${S}/fileview-1-chip.png` });
    log('chip before sending:', chip);

    // 2. Send: the agent lists the archive with its tool.
    await p.fill('.composer textarea', `script:[{"tool":"attached_file","input":{"op":"list","file_id":"${fid}"}}]`);
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip:has-text("read-only")');
    const sent = (await p.textContent('.msg.user .chip:has-text("read-only")')).trim();
    check(/Q3 report\.zip/.test(sent) && /read-only/.test(sent) && /\(\d+(\.\d)? (B|KB)/.test(sent), 'the sent chip: ' + sent);
    await p.waitForSelector('.msg.assistant:has-text("Entries of the attached ZIP archive")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const sid = p.url().split('/').filter(Boolean).pop();
    const ev = await p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
    const um = ev.find((e) => e.kind === 'user_message' && e.payload.attached?.length);
    check(um && um.payload.attached[0].file_id === fid && um.payload.attached[0].name === 'Q3 report.zip', 'user_message.attached: ' + JSON.stringify(um && um.payload.attached));
    check(!um.payload.uploads, 'nothing saved to a folder');
    const finished = ev.filter((e) => e.kind === 'tool_call_finished').map((e) => e.payload);
    check(finished.length === 1 && finished[0].ok && finished[0].output.includes('data/blob.bin') && finished[0].output.includes('| 0\tfile'), 'the listing: ' + (finished[0] && finished[0].output.slice(0, 300)));
    check(await p.$('.tool-card, [class*="tool"]'), 'the call shows as a tool block');
    await p.screenshot({ path: `${S}/fileview-2-list.png` });
    log('listed:', finished[0].output.split('\n').slice(1, 5).join(' / '));

    // 3. A text entry as numbered lines, then a binary entry as a hex dump.
    await p.fill('.composer textarea', `script:[{"tool":"attached_file","input":{"op":"read","file_id":"${fid}","entry":"hello.txt","limit":3}}]`);
    await p.keyboard.press('Enter');
    await p.waitForFunction(() => document.querySelectorAll('.msg.assistant').length >= 2);
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    await p.fill('.composer textarea', `script:[{"tool":"attached_file","input":{"op":"read","file_id":"${fid}","entry":"data/blob.bin"}}]`);
    await p.keyboard.press('Enter');
    await p.waitForFunction(() => document.querySelectorAll('.msg.assistant').length >= 3);
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const ev2 = await p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
    const outs = ev2.filter((e) => e.kind === 'tool_call_finished').map((e) => e.payload);
    check(outs.length === 3 && outs.every((o) => o.ok), 'three calls: ' + outs.map((o) => o.output.slice(0, 80)).join(' || '));
    check(outs[1].output.includes('| 1\talpha line') && outs[1].output.includes('| 3\tgamma line') && outs[1].output.includes('call again with offset 4'), 'text lines: ' + outs[1].output);
    check(outs[2].output.includes('a hex dump') && outs[2].output.includes('| 00000000  00 01 02 03 fa fb fc fd'), 'hex dump: ' + outs[2].output);
    await p.screenshot({ path: `${S}/fileview-3-read.png`, fullPage: false });
    log('text entry:', outs[1].output.split('\n').slice(1, 4).join(' / '));
    log('binary entry:', outs[2].output.split('\n')[1]);

    // 4. An agent without the switch is refused, naming the switch; the
    // Agents page shows it on for the Assistant.
    const refused = await p.evaluate(async () => {
      const r = await fetch('/api/attachments?name=x.zip&agent=researcher', { method: 'POST', body: new Uint8Array([80, 75, 3, 4, 0, 1, 2, 3]) });
      return { status: r.status, body: await r.json() };
    });
    check(refused.status === 422 && /Researcher cannot read attached files/.test(refused.body.error) && /Read attached files/.test(refused.body.error), 'refusal: ' + JSON.stringify(refused));
    log('an agent without the switch:', refused.body.error);
    await p.goto(URL_ + '#/agents/general');
    await p.waitForSelector('h1:has-text("Assistant")');
    const on = await p.$eval('label.feature:has(.feature-label:text-is("Read attached files")) input', (n) => n.checked);
    check(on, 'the switch is on for the Assistant');
    await p.locator('label.feature:has(.feature-label:text-is("Read attached files"))').scrollIntoViewIfNeeded();
    await p.screenshot({ path: `${S}/fileview-4-agent-switch.png` });
    await p.goto(URL_ + '#/agents/researcher');
    await p.waitForSelector('h1:has-text("Researcher")');
    const off = await p.$eval('label.feature:has(.feature-label:text-is("Read attached files")) input', (n) => n.checked);
    check(!off, 'the switch is off for the Researcher');

    // 5. The same at phone width: the chip, the sent file and the page.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.goto(URL_);
    await p.waitForSelector('.composer textarea');
    await p.click('.mode-switch button:has-text("Ask")').catch(() => {});
    await p.setInputFiles('.composer input[type=file]', `${F}/Q3 report.zip`);
    await p.waitForSelector('.composer .chip:has-text("read-only")');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px (chip)');
    await p.screenshot({ path: `${S}/fileview-5-phone-chip.png` });
    await p.waitForFunction(() => window.__up.some((u) => u.status === 200));
    const fid2 = (await uploads()).find((u) => u.status === 200).body.attachment_id;
    await p.fill('.composer textarea', `script:[{"tool":"attached_file","input":{"op":"info","file_id":"${fid2}"}}]`);
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip:has-text("read-only")');
    await p.waitForSelector('.msg.assistant:has-text("Attached file")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px (chat)');
    await p.screenshot({ path: `${S}/fileview-6-phone-chat.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/fileview-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

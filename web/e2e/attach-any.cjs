// Attaching any file (T94, ADOPTION_PLAN X.53) end to end: in a coding chat a
// zip is saved into the chat's folder under attachments/ (the chip says so
// before sending, the sent message shows the file with its size, the agent
// can list it with its own tools), a text file and an image beside it behave
// as before, a file over the limit is refused with the number, the same zip
// twice does not overwrite the first, and a plain chat refuses the zip with
// the reason. At desktop and phone width.
// Usage: node web/e2e/attach-any.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync } = require('child_process');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-attach-'));
const D = `${S}/data`, R = `${S}/repo`, F = `${S}/files`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(R, { recursive: true });
fs.mkdirSync(F, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;

// A real (small) zip made with Python's zipfile, and a one-pixel PNG.
function makeFiles() {
  execFileSync('python3', ['-I', '-c', `
import zipfile, sys
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("hello.txt", "hello from a zip\\n" * 100)
    z.writestr("data/numbers.csv", "a,b\\n1,2\\n")
`, `${F}/Q3 report.zip`]);
  fs.writeFileSync(`${F}/notes.md`, '# Plan\nbuy oat milk on friday\n');
  fs.writeFileSync(`${F}/dot.png`, Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64'));
  // 1 MiB + 1 of bytes with a NUL: over the node's 1 MiB limit set below.
  fs.writeFileSync(`${F}/too-big.bin`, Buffer.alloc(1024 * 1024 + 1, 0));
}

async function listing(sid) {
  return fs.existsSync(`${R}/attachments`) ? fs.readdirSync(`${R}/attachments`).sort() : [];
}

(async () => {
  makeFiles();
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${R}"]\nmax_approval_mode = "auto"\n[limits]\nupload_max_mb = 1\n`);
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
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // 1. A plain chat refuses the zip with the reason, at attach time.
    await p.click('.mode-switch button:has-text("Ask")');
    await p.setInputFiles('.composer input[type=file]', `${F}/Q3 report.zip`);
    await p.waitForSelector('.error-text:has-text("cannot open files")');
    const reason = (await p.textContent('.error-text')).trim();
    check(reason.includes('switch to Agentic') && reason.includes('PDF or an image'), 'the reason: ' + reason);
    check((await p.$$('.composer ul.chips li')).length === 0, 'no chip for a refused file');
    await p.screenshot({ path: `${S}/attach-1-plain-refused.png` });
    log('a plain chat refused the zip:', reason);

    // 2. An agentic chat in the folder; attach the zip: the chip says it is
    // saved to the folder, with its size.
    await p.fill('.composer textarea', 'hello');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p);
    await p.setInputFiles('.composer input[type=file]', `${F}/Q3 report.zip`);
    await p.waitForSelector('.composer .chip:has-text("saved to the folder")');
    const chip = (await p.textContent('.composer .chip')).trim();
    check(/Q3 report\.zip/.test(chip) && /\d+(\.\d)? (B|KB)/.test(chip), 'the chip: ' + chip);
    await p.screenshot({ path: `${S}/attach-2-chip.png` });
    log('chip before sending:', chip);
    await p.fill('.composer textarea', 'please unpack the zip');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip:has-text("Q3 report.zip")');
    await p.waitForSelector('.msg.assistant');
    const sent = (await p.textContent('.msg.user .chip')).trim();
    check(/Q3 report\.zip/.test(sent) && /\(\d+(\.\d)? (B|KB)\)/.test(sent), 'the sent chip shows the size: ' + sent);
    check(fs.existsSync(`${R}/attachments/Q3 report.zip`), 'the zip is in the chat folder: ' + (await listing()));
    check(fs.statSync(`${R}/attachments/Q3 report.zip`).size === fs.statSync(`${F}/Q3 report.zip`).size, 'same size on disk');
    await p.screenshot({ path: `${S}/attach-3-sent.png` });
    log('saved:', (await listing()).join(', '));

    // The log says where it went, and the agent finds it with its own tools.
    const sid = p.url().split('/').filter(Boolean).pop();
    const ev = await p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
    const um = ev.filter((e) => e.kind === 'user_message').find((e) => e.payload.uploads?.length);
    check(um && um.payload.uploads[0].path === 'attachments/Q3 report.zip', 'user_message.uploads: ' + JSON.stringify(um && um.payload.uploads));
    await p.fill('.composer textarea', 'script:[{"tool":"glob","input":{"pattern":"attachments/*"}}]');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant:has-text("Tool said: attachments/Q3 report.zip")');
    log('the agent lists the file with its own tools');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));

    // 3. A text file and an image beside a zip: as before. The zip a second
    // time gets its own name.
    await p.setInputFiles('.composer input[type=file]', [`${F}/notes.md`, `${F}/Q3 report.zip`]);
    await p.waitForSelector('.composer .chip:has-text("notes.md")');
    await p.waitForSelector('.composer .chip:has-text("saved to the folder")');
    const notesChip = await p.$eval('.composer .chip:has-text("notes.md")', (n) => n.textContent.trim());
    check(!/saved/.test(notesChip), 'the text file is not "saved": ' + notesChip);
    await p.setInputFiles('.composer input[type=file]', `${F}/dot.png`);
    await p.waitForSelector('.composer .image-chip');
    await p.fill('.composer textarea', 'what is in my notes, and look at this dot');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user:has-text("what is in my notes") .chip:has-text("notes.md")');
    await p.waitForSelector('.msg.assistant:has-text("I see 1 image")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const files = await listing();
    check(files.join('|') === 'Q3 report (2).zip|Q3 report.zip', 'the second zip did not overwrite: ' + files);
    check(!files.includes('notes.md') && !files.includes('dot.png'), 'text and images are not saved: ' + files);
    await p.screenshot({ path: `${S}/attach-4-mixed.png` });
    log('same zip twice:', files.join(', '));

    // 4. A file over the node's limit is refused with the number.
    await p.setInputFiles('.composer input[type=file]', `${F}/too-big.bin`);
    await p.waitForSelector('.error-text:has-text("1 MB")');
    log('over the limit:', (await p.textContent('.error-text')).trim());
    await p.screenshot({ path: `${S}/attach-5-too-big.png` });

    // 5. The same at phone width.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.reload();
    await p.waitForSelector('.composer textarea');
    await p.setInputFiles('.composer input[type=file]', `${F}/Q3 report.zip`);
    await p.waitForSelector('.composer .chip:has-text("saved to the folder")');
    const overflow = await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
    check(!overflow, 'no horizontal scroll at 390 px');
    await p.screenshot({ path: `${S}/attach-6-phone-chip.png` });
    await p.fill('.composer textarea', 'and once more on a phone');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user:has-text("on a phone") .chip');
    await p.waitForSelector('.msg.assistant:has-text("You said: and once more on a phone")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    check((await listing()).includes('Q3 report (3).zip'), 'third copy: ' + (await listing()));
    await p.screenshot({ path: `${S}/attach-7-phone-sent.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/attach-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

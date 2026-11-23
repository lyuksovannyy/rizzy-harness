// Editing documents (T98, ADOPTION_PLAN X.57) end to end, with the real binary
// and the demo provider's scripted tool calls: in a plain chat the agent makes
// a Word file, looks at it with doc_view, edits it with doc_edit and the result
// is a NEW file with its own Download whose bytes are the log's hash, while the
// first file is unchanged; in a coding chat it edits a file of the folder, the
// original stays as it was and the new file is beside it. The tool blocks show
// each edit's outcome. The helper that reads the file is the binary's own
// `doc-edit`, confined: on a machine without Landlock the edit is refused
// and this script says so and stops.
// Usage: node web/e2e/doc-edit.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-docedit-'));
const D = `${S}/data`, R = `${S}/repo`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(R, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;
const events = (p, sid) => p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
const part = (file, name) => execFileSync('python3', ['-I', '-c', 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; sys.stdout.write(z.read(sys.argv[2]).decode())', file, name]).toString();

const DOC = JSON.stringify([{ tool: 'doc_create', input: { path: 'Plan', markdown: '# Plan\n\nShip the **document tools** before Friday.\n\n- reader\n- writer\n' } }]);

(async () => {
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${R}"]\nmax_approval_mode = "auto"\n`);
  fs.writeFileSync(`${D}/config.toml`, [
    '[[providers]]', 'id = "demo"', 'name = "Demo"', 'preset = "demo"', 'kind = "mock"', 'mock_delay_ms = 2',
    'models = [{ id = "mock", vision = true }]', '',
  ].join('\n'));
  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: true });
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

    const say = async (calls, wait) => {
      await p.fill('.composer textarea', 'script: ' + JSON.stringify(calls));
      await p.click('.composer .send');
      await p.waitForSelector(wait);
      await p.waitForFunction(() => !document.querySelector('.send.stop'));
    };
    const finished = async (sid) => (await events(p, sid)).filter((e) => e.kind === 'tool_call_finished').map((e) => e.payload);

    // 1. A plain chat: make a file, view it, edit it.
    await p.click('.mode-switch button:has-text("Ask")');
    await p.fill('.composer textarea', 'script: ' + DOC);
    await p.click('.composer .send');
    await p.waitForSelector('.artifact-card:has-text("Plan.docx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const sid = p.url().split('/').filter(Boolean).pop();
    const made = (await finished(sid)).find((f) => f.made).made;
    const first = `${S}/dl-plan.docx`;
    const [dl] = await Promise.all([p.waitForEvent('download'), p.locator('.artifact-card:has-text("Plan.docx") a:has-text("Download")').click()]);
    await dl.saveAs(first);
    const firstHash = crypto.createHash('sha256').update(fs.readFileSync(first)).digest('hex');

    await say([{ tool: 'doc_view', input: { file: made.file_id } }], '.tool-card:has-text("doc_view")');
    const view = (await finished(sid)).find((f) => /Ship the document tools/.test(f.output || '') && !f.made);
    if (view && /Landlock|cannot edit documents safely/.test(view.output)) {
      log('skipped: this machine cannot confine the document editor (Landlock ABI 4 needed)');
      await b.close();
      return;
    }
    check(view && view.ok, 'doc_view worked: ' + JSON.stringify(view));
    const line = view.output.split('\n').find((l) => l.includes('Ship the document tools'));
    const tag = line.replace(/^\S+\s*/, '').split(/\s+/)[0];
    check(/^p\d+#[0-9a-f]{4}$/.test(tag), 'a tag in the view: ' + line);
    await p.screenshot({ path: `${S}/docedit-1-view.png` });

    await say([{ tool: 'doc_edit', input: { file: made.file_id, edits: [
      { op: 'replace_text', at: tag, find: 'before Friday', replace: 'before Monday' },
      { op: 'insert_after', at: tag, markdown: 'A **new** paragraph.' },
    ] } }], '.artifact-card:has-text("Plan (edited).docx")');
    const edit = (await finished(sid)).filter((f) => f.made).pop();
    check(edit.ok && edit.made.name === 'Plan (edited).docx' && edit.made.file_id && edit.made.file_id !== made.file_id, 'a new file: ' + JSON.stringify(edit.made));
    check(/replaced 1 occurrence/.test(edit.output) && /inserted 1 paragraph/.test(edit.output), 'each edit\'s outcome in the result: ' + edit.output);
    const card = p.locator('.artifact-card:has-text("Plan (edited).docx")');
    const second = `${S}/dl-plan-edited.docx`;
    const [dl2] = await Promise.all([p.waitForEvent('download'), card.locator('a:has-text("Download")').click()]);
    await dl2.saveAs(second);
    const bytes = fs.readFileSync(second);
    check(crypto.createHash('sha256').update(bytes).digest('hex') === edit.made.sha256, 'the bytes are the log\'s hash');
    const xml = part(second, 'word/document.xml');
    check(xml.includes('before Monday') && xml.includes('A ') && xml.includes('w:val="Heading1"'), 'the edit is in the file and the heading style is still there');
    check(crypto.createHash('sha256').update(fs.readFileSync(first)).digest('hex') === firstHash, 'the first download is unchanged');
    const [dl3] = await Promise.all([p.waitForEvent('download'), p.locator('.artifact-card:has-text("Plan.docx") a:has-text("Download")').click()]);
    await dl3.saveAs(`${S}/dl-plan-again.docx`);
    check(crypto.createHash('sha256').update(fs.readFileSync(`${S}/dl-plan-again.docx`)).digest('hex') === firstHash, 'the original made file is unchanged on the server');
    await p.screenshot({ path: `${S}/docedit-2-edited.png` });
    log('plain chat edit:', edit.made.name, bytes.length, 'bytes');

    // A refused edit keeps nothing and says which edit and why.
    await say([{ tool: 'doc_edit', input: { file: made.file_id, edits: [{ op: 'delete', at: 'p9#0000' }] } }], '.tool-card:has-text("no paragraph")');
    const refused = (await finished(sid)).pop();
    check(!refused.ok && !refused.made && /edit 1/.test(refused.output), 'a refusal: ' + refused.output);

    // 2. A coding chat: a file of the folder, the original stays, a new file beside it.
    fs.copyFileSync(first, `${R}/plan.docx`);
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p, 'Assistant');
    await say([{ tool: 'doc_view', input: { file: 'plan.docx' } }], '.tool-card:has-text("doc_view")');
    const sid2 = p.url().split('/').filter(Boolean).pop();
    const v2 = (await finished(sid2)).pop();
    const tag2 = v2.output.split('\n').find((l) => l.includes('Ship the document tools')).replace(/^\S+\s*/, '').split(/\s+/)[0];
    await say([{ tool: 'doc_edit', input: { file: 'plan.docx', edits: [{ op: 'replace_text', at: tag2, find: 'Friday', replace: 'Tuesday' }] } }], '.artifact-card:has-text("plan (edited).docx")');
    check(crypto.createHash('sha256').update(fs.readFileSync(`${R}/plan.docx`)).digest('hex') === firstHash, 'the original in the folder is untouched');
    check(fs.existsSync(`${R}/plan (edited).docx`), 'the new file beside it: ' + fs.readdirSync(R));
    check(part(`${R}/plan (edited).docx`, 'word/document.xml').includes('before Tuesday'), 'the edit is in it');
    const chip = (await p.locator('.artifact-card:has-text("plan (edited).docx")').textContent()).replace(/\s+/g, ' ');
    check(chip.includes('plan (edited).docx') && !/Download/.test(chip), 'the chip shows the path: ' + chip);
    const made2 = (await finished(sid2)).filter((f) => f.made).pop();
    check(made2.seen && made2.seen.base === '' && !made2.made.file_id, 'the log: ' + JSON.stringify(made2));
    await p.screenshot({ path: `${S}/docedit-3-coding.png` });
    log('coding chat:', chip);

    // 3. Phone width: no horizontal scroll.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.reload();
    await p.waitForSelector('.artifact-card:has-text("plan (edited).docx")');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px');
    await p.screenshot({ path: `${S}/docedit-4-phone.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/docedit-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

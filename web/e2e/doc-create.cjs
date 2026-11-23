// Documents the agent makes (T97, ADOPTION_PLAN X.56) end to end, with the real
// binary and the demo provider's scripted tool calls: in a plain chat a Word
// file and an Excel file are made by the server and each shows as a chip on its
// tool block with a Download that gives the right bytes (a zip that is a real
// document, the hash the log names); in a coding chat the file is written into
// the folder, the chip shows the path, a second call gets a new name and a
// file that was already there is untouched. At desktop and phone width.
// Usage: node web/e2e/doc-create.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-doccreate-'));
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

/** The text of one part of a zip, read with Python (the checker is not ours). */
function part(file, name) {
  return execFileSync('python3', ['-I', '-c', 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; sys.stdout.write(z.read(sys.argv[2]).decode())', file, name]).toString();
}

const DOC = JSON.stringify([{ tool: 'doc_create', input: { path: 'Quarterly plan', markdown: '# Quarterly plan\n\nShip the **document tools** before Friday.\n\n- reader\n- writer\n\n| Owner | Due |\n|---|--:|\n| Dana | 2026-12-01 |\n' } }]);
const SHEET = JSON.stringify([{ tool: 'sheet_create', input: { path: 'budget', sheets: [{ name: 'Budget', columns: [{ header: 'Item' }, { header: 'Cost', format: 'currency' }, { header: 'Due', format: 'date' }], rows: [['Coffee beans', 42.5, '2026-12-01'], ['Total', { f: 'SUM(B2:B2)' }, null]], bold_last_row: true }] } }]);
const FOLDER_DOC = JSON.stringify([{ tool: 'doc_create', input: { path: 'reports/plan', markdown: '# Plan\n\ntext' } }]);

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

    const say = async (text, chip) => {
      await p.fill('.composer textarea', 'script: ' + text);
      await p.click('.composer .send');
      await p.waitForSelector(`.artifact-card:has-text("${chip}")`);
      await p.waitForFunction(() => !document.querySelector('.send.stop'));
    };

    // 1. A plain chat: a Word file, with a Download that gives the real file.
    await p.click('.mode-switch button:has-text("Ask")');
    await say(DOC, 'Quarterly plan.docx');
    const sid = p.url().split('/').filter(Boolean).pop();
    const card = p.locator('.artifact-card:has-text("Quarterly plan.docx")');
    const text = (await card.textContent()).replace(/\s+/g, ' ');
    check(/Quarterly plan\.docx · \d+(\.\d)? (B|KB)/.test(text) && /Download/.test(text), 'the chip: ' + text);
    await p.screenshot({ path: `${S}/doccreate-1-plain-doc.png` });
    const [dl] = await Promise.all([p.waitForEvent('download'), card.locator('a:has-text("Download")').click()]);
    const docFile = `${S}/dl-plan.docx`;
    await dl.saveAs(docFile);
    check(dl.suggestedFilename() === 'Quarterly plan.docx', 'the download name: ' + dl.suggestedFilename());
    const bytes = fs.readFileSync(docFile);
    check(bytes.slice(0, 2).toString() === 'PK', 'a zip');
    const docXml = part(docFile, 'word/document.xml');
    check(docXml.includes('Quarterly plan') && docXml.includes('<w:tbl>') && docXml.includes('w:val="Heading1"'), 'a real document: ' + docXml.slice(0, 200));
    const ev = await events(p, sid);
    const fin = ev.find((e) => e.kind === 'tool_call_finished' && e.payload.made);
    check(fin && fin.payload.made.file_id && fin.payload.made.name === 'Quarterly plan.docx', 'the log names the file: ' + JSON.stringify(fin && fin.payload.made));
    check(crypto.createHash('sha256').update(bytes).digest('hex') === fin.payload.made.sha256, 'the bytes are the log\'s hash');
    check(!/[a-z]:\\|\/home\/|\/tmp\//i.test(JSON.stringify(fin.payload.made)), 'no path of any machine in a plain chat');
    log('plain chat docx:', text, bytes.length, 'bytes');

    // 2. An Excel file in the same chat.
    await say(SHEET, 'budget.xlsx');
    const card2 = p.locator('.artifact-card:has-text("budget.xlsx")');
    const [dl2] = await Promise.all([p.waitForEvent('download'), card2.locator('a:has-text("Download")').click()]);
    const xlsxFile = `${S}/dl-budget.xlsx`;
    await dl2.saveAs(xlsxFile);
    const wb = part(xlsxFile, 'xl/workbook.xml');
    const sheet = part(xlsxFile, 'xl/worksheets/sheet1.xml');
    check(wb.includes('name="Budget"') && wb.includes('fullCalcOnLoad="1"') && sheet.includes('<f>SUM(B2:B2)</f>') && sheet.includes('<v>46357</v>'), 'a real workbook: ' + wb.slice(0, 200));
    await p.screenshot({ path: `${S}/doccreate-2-plain-sheet.png` });
    log('plain chat xlsx ok');

    // 3. Phone width: both chips fit, no horizontal scroll.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.reload();
    await p.waitForSelector('.artifact-card:has-text("budget.xlsx")');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px');
    await p.screenshot({ path: `${S}/doccreate-3-phone.png` });
    await p.setViewportSize({ width: 1440, height: 900 });

    // 4. A coding chat: the file is in the folder, the chip shows the path.
    fs.mkdirSync(`${R}/reports`, { recursive: true });
    fs.writeFileSync(`${R}/reports/plan.docx`, 'the person\'s own file');
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p, 'Assistant');
    await say(FOLDER_DOC, 'plan (2).docx');
    check(fs.readFileSync(`${R}/reports/plan.docx`, 'utf8') === 'the person\'s own file', 'an existing file is untouched');
    check(fs.existsSync(`${R}/reports/plan (2).docx`), 'the new file: ' + fs.readdirSync(`${R}/reports`));
    const sid2 = p.url().split('/').filter(Boolean).pop();
    const chip = (await p.locator('.artifact-card:has-text("plan (2).docx")').textContent()).replace(/\s+/g, ' ');
    check(chip.includes('reports/plan (2).docx') && !/Download/.test(chip), 'the chip shows the path, no download: ' + chip);
    const made2 = (await events(p, sid2)).find((e) => e.kind === 'tool_call_finished' && e.payload.made).payload;
    check(made2.made.path.endsWith('reports/plan (2).docx') && !made2.made.file_id && made2.seen && made2.seen.base === '', 'the log: ' + JSON.stringify(made2));
    part(`${R}/reports/plan (2).docx`, 'word/document.xml');
    await p.screenshot({ path: `${S}/doccreate-4-coding.png` });
    log('coding chat:', chip);
    await p.setViewportSize({ width: 390, height: 780 });
    await p.reload();
    await p.waitForSelector('.artifact-card:has-text("plan (2).docx")');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px (coding chat)');
    await p.screenshot({ path: `${S}/doccreate-5-coding-phone.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/doccreate-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

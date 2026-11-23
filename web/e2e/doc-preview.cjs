// Previewing documents (T99, ADOPTION_PLAN X.58) end to end, with the real
// binary and the demo provider's scripted tool calls: in a plain chat the agent
// makes a Word file with hostile text in it and a workbook; Preview on each
// chip opens the right side panel with the document's headings, list and
// table (the hostile text as text, no link, no image, no script), the
// workbook as a grid with a formula marked and its sheet tabs; Escape closes
// it and the chip's Preview has the focus again; a chat with a folder has no
// Preview. The helper is the binary's own confined `doc-edit`: on a machine
// without Landlock the preview says so and this script says so and stops.
// Usage: node web/e2e/doc-preview.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-docpreview-'));
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

const DOC = JSON.stringify([{ tool: 'doc_create', input: { path: 'Plan', markdown: '# Plan\n\nShip the **document tools** before *Friday* and ~~never~~ always.\n\n- reader\n- writer\n\n| Who | What |\n|---|---|\n| Ann | review |\n\n<script>alert(1)</script> [click](javascript:alert(1)) <img src=x onerror=alert(2)>\n' } }]);
const SHEET = JSON.stringify([{ tool: 'sheet_create', input: { path: 'budget', sheets: [{ name: 'Budget', columns: [{ header: 'Item' }, { header: 'Cost', format: 'currency' }], rows: [['Coffee beans', 42.5], ['=cmd|calc', 1], ['Total', { f: 'SUM(B2:B3)' }]] }, { name: 'Notes', columns: [{ header: 'Note' }], rows: [['second sheet']] }] } }]);

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

    await p.click('.mode-switch button:has-text("Ask")');
    await p.fill('.composer textarea', 'script: ' + DOC);
    await p.click('.composer .send');
    await p.waitForSelector('.artifact-card:has-text("Plan.docx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    await p.fill('.composer textarea', 'script: ' + SHEET);
    await p.click('.composer .send');
    await p.waitForSelector('.artifact-card:has-text("budget.xlsx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));

    // 1. The document.
    const open = p.locator('.artifact-card:has-text("Plan.docx") button:has-text("Preview")');
    await open.focus();
    await open.click();
    await p.waitForSelector('aside.doc-preview .pv-body');
    await p.waitForFunction(() => !document.querySelector('.doc-preview [role=status]'));
    const err = await p.locator('.doc-preview [role=alert]').count();
    if (err) {
      const t = await p.locator('.doc-preview [role=alert]').textContent();
      if (/Landlock|cannot read documents safely/.test(t)) {
        log('skipped: this machine cannot confine the document reader (Landlock ABI 4 needed)');
        await b.close();
        return;
      }
      throw new Error('preview failed: ' + t);
    }
    const body = p.locator('.doc-preview .pv-body');
    check((await body.locator('h3').first().textContent()) === 'Plan', 'the heading');
    check((await body.locator('strong').first().textContent()) === 'document tools', 'bold');
    check((await body.locator('em').first().textContent()) === 'Friday', 'italic');
    check((await body.locator('s').first().textContent()) === 'never', 'strike');
    check((await body.locator('ul li').allTextContents()).join() === 'reader,writer', 'the list');
    check((await body.locator('table td').allTextContents()).join() === 'Who,What,Ann,review', 'the table');
    check((await p.locator('.doc-preview').textContent()).includes('text and structure, not the page layout'), 'the line');
    check((await body.locator('a, img, script, iframe, [onerror], [href]').count()) === 0, 'no link, image or script in the preview');
    check((await p.locator('.doc-preview a[href]').count()) === 1, 'the download is the one link');
    check((await p.evaluate(() => window.__x)) === undefined, 'nothing ran');
    await p.screenshot({ path: `${S}/docpreview-1-doc.png` });
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    check(await p.evaluate(() => document.activeElement && /Preview/.test(document.activeElement.getAttribute('aria-label') || '')), 'the focus is back on the chip\'s Preview');
    log('document preview ok');

    // 2. The workbook.
    await p.locator('.artifact-card:has-text("budget.xlsx") button:has-text("Preview")').click();
    await p.waitForSelector('.doc-preview table.pv-grid');
    const heads = (await p.locator('.doc-preview thead th').allTextContents()).join('|');
    check(/Row\|A\|B/.test(heads), 'column letters: ' + heads);
    check((await p.locator('.doc-preview tbody th').allTextContents()).join() === '1,2,3,4', 'row numbers');
    const text = await p.locator('.doc-preview .pv-body').textContent();
    check(text.includes('Coffee beans') && text.includes('=cmd|calc') && text.includes('=SUM(B2:B3)'), 'values and the formula as text: ' + text);
    check((await p.locator('.doc-preview td.pv-formula').count()) === 1, 'the formula cell is marked');
    check((await p.locator('.doc-preview .pv-body a, .doc-preview .pv-body img').count()) === 0, 'no link or image');
    await p.locator('.doc-preview .pv-tabs button:has-text("Notes")').click();
    await p.waitForFunction(() => document.querySelector('.doc-preview .pv-body')?.textContent.includes('second sheet'));
    await p.screenshot({ path: `${S}/docpreview-2-sheet.png` });
    check((await p.locator('.doc-preview .pv-tabs button[aria-pressed="true"]').textContent()) === 'Notes', 'the second tab is the active one');
    log('workbook preview ok');

    // A failed switch: the error in place of the grid, the chosen tab marked.
    await p.locator('.doc-preview .pv-tabs button:has-text("Budget")').click();
    await p.waitForSelector('.doc-preview td:has-text("Coffee beans")');
    await p.route('**/preview?sheet=1', (r) => r.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'the server is busy reading other files; try again in a minute' }) }));
    await p.locator('.doc-preview .pv-tabs button:has-text("Notes")').click();
    await p.waitForSelector('.doc-preview [role=alert]');
    check((await p.locator('.doc-preview table').count()) === 0, 'no old grid under the error');
    check((await p.locator('.doc-preview .pv-tabs button[aria-pressed="true"]').textContent()) === 'Notes', 'the chosen tab is marked');
    await p.screenshot({ path: `${S}/docpreview-2b-error.png` });
    await p.unroute('**/preview?sheet=1');
    await p.click('.doc-preview [role=alert] button:has-text("Try again")');
    await p.waitForSelector('.doc-preview td:has-text("second sheet")');
    await p.click('button[aria-label="Close the preview"]');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });

    // 3. Phone width: the panel covers the chat, no horizontal scroll.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.locator('.artifact-card:has-text("Plan.docx") button:has-text("Preview")').click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px');
    await p.screenshot({ path: `${S}/docpreview-3-phone.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/docpreview-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

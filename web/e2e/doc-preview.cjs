// Previewing documents (T99, ADOPTION_PLAN X.58) end to end, with the real
// binary and the demo provider's scripted tool calls: in a plain chat the agent
// makes a Word file with hostile text in it and a workbook; Preview on each
// chip opens the right side panel with the document's headings, list and
// table (the hostile text as text, no link, no image, no script), the
// workbook as a grid with a formula marked and its sheet tabs; Escape closes
// it and the chip's Preview has the focus again; in a coding chat (X.59) the made
// file's chip and the sent message's attached .docx chip have the same Preview,
// read by the node by path, no Download, and a vanished file is "unknown file".
// X.60 (changing text and saving a copy): in the plain chat a heading, a list
// item and three cells are changed in the panel, Save puts the copy in the
// composer as a held file, the message carries it, the copy's own preview and
// download hold the new text and the original is byte-identical; in the coding
// chat the node writes `<name> (edited)` beside the original, an @ mention
// lands in the message, and the original is byte-identical there too.
// The helper is the binary's own confined `doc-edit`: on a machine
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
const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
/** The bytes of a link on the page, fetched as the page would (its login). */
const fetched = (p, href) => p.evaluate(async (h) => { const r = await fetch(h); const b = new Uint8Array(await r.arrayBuffer()); let s = ''; for (const x of b) s += String.fromCharCode(x); return btoa(s); }, href).then((b64) => Buffer.from(b64, 'base64'));
const part = (file, name) => execFileSync('python3', ['-I', '-c', 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; sys.stdout.write(z.read(sys.argv[2]).decode())', file, name]).toString();

const FOLDER_SHEET = JSON.stringify([{ tool: 'sheet_create', input: { path: 'reports/budget', sheets: [{ name: 'Budget', columns: [{ header: 'Item' }, { header: 'Cost', format: 'currency' }], rows: [['Coffee beans', 42.5], ['Tea', 3]] }] } }]);
const FOLDER_DOC = JSON.stringify([{ tool: 'doc_create', input: { path: 'reports/plan', markdown: '# Plan\n\ntext' } }]);
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
    await p.waitForTimeout(500);
    await p.screenshot({ path: `${S}/docpreview-2b-error.png` });
    await p.unroute('**/preview?sheet=1');
    await p.click('.doc-preview [role=alert] button:has-text("Try again")');
    await p.waitForSelector('.doc-preview td:has-text("second sheet")');
    await p.click('button[aria-label="Close the preview"]');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });


    // 2c. Change text and save a copy (X.60), in a chat without a folder.
    const doc0 = p.locator('.artifact-card:has-text("Plan.docx") button:has-text("Preview")');
    await doc0.click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    const href0 = await p.locator('.doc-preview a[href]').getAttribute('href');
    const orig0 = sha(await fetched(p, href0));
    await p.click('.doc-preview button[aria-label="Edit Paragraph 1"]');
    await p.fill('.doc-preview textarea[aria-label="Paragraph 1"]', 'Plan v2');
    await p.keyboard.press('Enter');
    await p.click('.doc-preview button[aria-label^="Edit Paragraph"]:has-text("reader")');
    await p.fill('.doc-preview textarea[aria-label^="Paragraph"]', 'reviewer');
    await p.keyboard.press('Enter');
    check((await p.locator('.doc-preview .pv-editbar').textContent()).includes('2 changes'), 'two changes are counted');
    check((await p.locator('.doc-preview .pv-changed').count()) === 2, 'the changed places are marked');
    // Keyboard: the places are buttons in the tab order, with a visible focus ring.
    await p.locator('.doc-preview button[aria-label="Edit Paragraph 2"]').focus();
    check(await p.evaluate(() => getComputedStyle(document.activeElement).outlineStyle !== 'none'), 'a focus ring on a place');
    await p.screenshot({ path: `${S}/doc-click-edit-1-plain-desktop.png` });
    await p.click('.doc-preview button:text-is("Save a copy")');
    await p.waitForSelector('.doc-preview .pv-saved');
    check((await p.locator('.doc-preview .pv-saved').textContent()).includes('A copy named “Plan (edited).docx” was saved and added to the message.'), 'the saved sentence');
    check((await p.locator('.composer').textContent()).includes('Plan (edited).docx'), 'the copy is a file in the composer');
    check((await p.locator('.doc-preview .pv-body').textContent()).includes('Plan v2'), 'the copy is shown');
    await p.screenshot({ path: `${S}/doc-click-edit-2-plain-saved.png` });
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    await p.fill('.composer textarea', 'here is the edited copy');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip:has-text("Plan (edited).docx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    await p.locator('.msg.user .chip:has-text("Plan (edited).docx") button:has-text("Preview")').click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check((await p.locator('.doc-preview .pv-body h3').first().textContent()) === 'Plan v2', 'the sent copy reads the new heading');
    check((await p.locator('.doc-preview .pv-body ul li').allTextContents()).join() === 'reviewer,writer', 'the sent copy reads the new list item');
    const copyBytes = await fetched(p, await p.locator('.doc-preview a[href]').getAttribute('href'));
    fs.writeFileSync(`${S}/copy-plain.docx`, copyBytes);
    const xml = part(`${S}/copy-plain.docx`, 'word/document.xml');
    check(xml.includes('Plan v2') && xml.includes('reviewer') && !xml.includes('>reader<'), 'the downloaded copy holds the new text');
    await p.keyboard.press('Escape');
    await doc0.click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check(sha(await fetched(p, await p.locator('.doc-preview a[href]').getAttribute('href'))) === orig0, 'the original is byte-identical');
    check((await p.locator('.doc-preview .pv-body h3').first().textContent()) === 'Plan', 'the original still reads Plan');
    // Phone width: the field and the edit bar fit.
    await p.setViewportSize({ width: 375, height: 780 });
    await p.click('.doc-preview button[aria-label="Edit Paragraph 1"]');
    await p.fill('.doc-preview textarea[aria-label="Paragraph 1"]', 'Phone');
    await p.keyboard.press('Enter');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 375 px while editing');
    await p.screenshot({ path: `${S}/doc-click-edit-3-plain-phone.png` });
    p.once('dialog', (d) => d.accept());
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    await p.setViewportSize({ width: 1440, height: 900 });
    // A workbook: three cells, one a formula.
    await p.locator('.artifact-card:has-text("budget.xlsx") button:has-text("Preview")').click();
    await p.waitForSelector('.doc-preview table.pv-grid');
    for (const [cell, v] of [['B2', '50'], ['A2', 'Beans'], ['B3', '=B2*2']]) {
      await p.click(`.doc-preview button[aria-label="Edit Cell ${cell}"]`);
      await p.fill(`.doc-preview input[aria-label="Cell ${cell}"]`, v);
      await p.keyboard.press('Enter');
    }
    await p.click('.doc-preview button:text-is("Save a copy")');
    await p.waitForSelector('.doc-preview .pv-saved');
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    await p.fill('.composer textarea', 'and the budget');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip:has-text("budget (edited).xlsx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    await p.locator('.msg.user .chip:has-text("budget (edited).xlsx") button:has-text("Preview")').click();
    await p.waitForSelector('.doc-preview table.pv-grid');
    const grid = await p.locator('.doc-preview .pv-body').textContent();
    check(grid.includes('Beans') && grid.includes('50') && grid.includes('=B2*2'), 'the sent workbook reads the new cells: ' + grid);
    await p.keyboard.press('Escape');
    log('plain chat: change text and save a copy ok');

    // 3. Phone width: the panel covers the chat, no horizontal scroll.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.locator('.artifact-card:has-text("Plan.docx") button:has-text("Preview")').click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)), 'no horizontal scroll at 390 px');
    await p.screenshot({ path: `${S}/docpreview-3-phone.png` });

    // 4. A coding chat: the same Preview on the made file's chip and on the
    // sent message's chip of an attached .docx (saved into the folder); the
    // node reads the file by its path; no Download for a folder file; a path
    // the node refuses is the one plain sentence.
    await p.setViewportSize({ width: 1440, height: 900 });
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p, 'Assistant');
    fs.mkdirSync(`${S}/in`, { recursive: true });
    const memo = `${S}/in/memo.docx`;
    execFileSync('python3', ['-I', '-c', `
import sys, zipfile
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
with zipfile.ZipFile(sys.argv[1], 'w') as z:
    z.writestr('[Content_Types].xml', '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
    z.writestr('_rels/.rels', '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
    z.writestr('word/document.xml', '<?xml version="1.0"?><w:document xmlns:w="' + W + '"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Plan</w:t></w:r></w:p><w:p><w:r><w:t>Attached by the person.</w:t></w:r></w:p></w:body></w:document>')
`, memo]);
    await p.setInputFiles('.composer input[type=file]', memo);
    await p.fill('.composer textarea', 'script: ' + FOLDER_DOC);
    await p.click('.composer .send');
    await p.waitForSelector('.artifact-card:has-text("plan.docx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const folderOpen = p.locator('.artifact-card:has-text("plan.docx") button:has-text("Preview")');
    await folderOpen.click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check((await p.locator('.doc-preview .pv-body h3').first().textContent()) === 'Plan', 'the folder file heading');
    check((await p.locator('.doc-preview a[href]').count()) === 0, 'no download link for a folder file');
    check((await p.locator('.doc-preview .pv-body a, .doc-preview .pv-body img').count()) === 0, 'no link or image');
    await p.screenshot({ path: `${S}/docpreview-4-coding-made.png` });
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    const sent = p.locator('.msg.user .chip:has-text("memo.docx") button:has-text("Preview")');
    await sent.click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check((await p.locator('.doc-preview .pv-body h3').first().textContent()) === 'Plan', 'the attached file heading');
    await p.screenshot({ path: `${S}/docpreview-5-coding-attached.png` });
    // A file the node will not show: the same sentence a missing one gets.
    fs.unlinkSync(`${R}/attachments/memo.docx`);
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    await sent.click();
    await p.waitForSelector('.doc-preview [role=alert]');
    check((await p.locator('.doc-preview [role=alert]').textContent()).includes('unknown file'), 'a missing file is "unknown file"');
    await p.screenshot({ path: `${S}/docpreview-6-coding-gone.png` });

    // 5. Change text and save a copy (X.60), in a chat with a folder: the node
    // writes the copy beside the original, an @ mention lands in the message,
    // the original is byte-identical.
    await p.fill('.composer textarea', 'script: ' + FOLDER_SHEET);
    await p.click('.composer .send');
    await p.waitForSelector('.artifact-card:has-text("budget.xlsx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const orig1 = sha(fs.readFileSync(`${R}/reports/plan.docx`));
    await folderOpen.click();
    await p.waitForSelector('aside.doc-preview .pv-body h3');
    check((await p.locator('.doc-preview a[href]').count()) === 0, 'still no download for a folder file');
    await p.click('.doc-preview button[aria-label="Edit Paragraph 1"]');
    await p.fill('.doc-preview textarea[aria-label="Paragraph 1"]', 'Plan, changed');
    await p.keyboard.press('Enter');
    await p.screenshot({ path: `${S}/doc-click-edit-4-coding-desktop.png` });
    await p.click('.doc-preview button:text-is("Save a copy")');
    await p.waitForFunction(() => document.querySelector('.doc-preview .pv-saved')?.textContent.includes('plan (edited).docx'));
    check((await p.locator('.doc-preview .pv-saved').textContent()).includes('A copy named “plan (edited).docx” was saved and added to the message.'), 'the same sentence as the plain chat');
    check((await p.locator('.composer textarea').inputValue()).includes('@"reports/plan (edited).docx"'), 'the mention is in the message');
    check((await p.locator('.doc-preview .pv-body h3').first().textContent()) === 'Plan, changed', 'the panel shows the copy the node wrote');
    check(fs.existsSync(`${R}/reports/plan (edited).docx`), 'the copy is in the folder');
    check(part(`${R}/reports/plan (edited).docx`, 'word/document.xml').includes('Plan, changed'), 'the copy holds the new text');
    check(sha(fs.readFileSync(`${R}/reports/plan.docx`)) === orig1, 'the original is byte-identical');
    await p.screenshot({ path: `${S}/doc-click-edit-5-coding-saved.png` });
    // The workbook of the folder: a number and a formula.
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    await p.locator('.artifact-card:has-text("budget.xlsx") button:has-text("Preview")').first().click();
    await p.waitForSelector('.doc-preview table.pv-grid');
    for (const [cell, v] of [['B2', '99'], ['B3', '=B2+1']]) {
      await p.click(`.doc-preview button[aria-label="Edit Cell ${cell}"]`);
      await p.fill(`.doc-preview input[aria-label="Cell ${cell}"]`, v);
      await p.keyboard.press('Enter');
    }
    await p.click('.doc-preview button:text-is("Save a copy")');
    await p.waitForFunction(() => document.querySelector('.doc-preview .pv-saved')?.textContent.includes('budget (edited).xlsx'));
    check(fs.existsSync(`${R}/reports/budget (edited).xlsx`), 'the workbook copy is in the folder');
    const sx = part(`${R}/reports/budget (edited).xlsx`, 'xl/worksheets/sheet1.xml');
    check(sx.includes('<v>99</v>') && sx.includes('B2+1'), 'the workbook copy holds the new cell and formula');
    await p.keyboard.press('Escape');
    await p.waitForSelector('aside.doc-preview', { state: 'detached' });
    // The message with the mentions goes: the node reads them.
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user:has-text("plan (edited).docx")');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    check((await p.locator('.msg.user').last().textContent()).includes('budget (edited).xlsx'), 'the sent message names both copies');
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('coding chat: change text and save a copy ok');
    log('coding chat preview ok');
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/docpreview-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

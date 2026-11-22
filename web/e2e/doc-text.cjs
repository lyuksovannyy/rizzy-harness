// Office documents read as text (T96, ADOPTION_PLAN X.55) end to end: a Word
// and an Excel file attached in a plain (Ask) chat and in a coding chat reach
// the model as text (the user_message event holds the text of each) and are
// kept as files as well (the log names the kept file; in a coding chat it is
// in the chat's folder); each shows as one chip, in the box and when sent. A
// document that cannot be read is kept as a plain file and the chip says so.
// At desktop and phone width.
// Usage: node web/e2e/doc-text.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync } = require('child_process');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-doctext-'));
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

// Small real files made with Python's zipfile (no binary fixtures).
function makeFiles() {
  execFileSync('python3', ['-I', '-c', `
import zipfile, sys
d = sys.argv[1]
CT = '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>'
W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
def p(t, style=None):
    ppr = '<w:pPr><w:pStyle w:val="%s"/></w:pPr>' % style if style else ''
    return '<w:p>%s<w:r><w:t>%s</w:t></w:r></w:p>' % (ppr, t)
body = p('Quarterly plan', 'Heading1') + p('Ship the document reader before Friday.') + '<w:tbl><w:tr><w:tc>' + p('Owner') + '</w:tc><w:tc>' + p('Dana') + '</w:tc></w:tr></w:tbl>'
with zipfile.ZipFile(d + "/plan.docx", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", CT)
    z.writestr("word/document.xml", '<?xml version="1.0"?><w:document %s><w:body>%s</w:body></w:document>' % (W, body))
S_ = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"'
with zipfile.ZipFile(d + "/budget.xlsx", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", CT)
    z.writestr("xl/workbook.xml", '<workbook %s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets></workbook>' % S_)
    z.writestr("xl/_rels/workbook.xml.rels", '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>')
    z.writestr("xl/sharedStrings.xml", '<sst %s><si><t>Item</t></si><si><t>Coffee beans</t></si></sst>' % S_)
    z.writestr("xl/worksheets/sheet1.xml", '<worksheet %s><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42.5</v></c></row><row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><v>7</v></c></row></sheetData></worksheet>' % S_)
with zipfile.ZipFile(d + "/broken.docx", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", CT)
    z.writestr("word/document.xml", '<w:document %s><w:body><w:p><w:r><w:t>x</w:q></w:r></w:p></w:body></w:document>' % W)
`, F]);
}

const events = (p, sid) => p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);

(async () => {
  makeFiles();
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

    // 1. A plain (Ask) chat: both documents are text and kept files, one chip each.
    await p.click('.mode-switch button:has-text("Ask")');
    await p.setInputFiles('.composer input[type=file]', [`${F}/plan.docx`, `${F}/budget.xlsx`]);
    await p.waitForSelector('.composer .chip:has-text("budget.xlsx")');
    await p.waitForFunction(() => window.__up.filter((u) => u.status === 200).length >= 2);
    const chips = (await p.$$eval('.composer ul.chips li', (n) => n.map((x) => x.textContent.trim()))).sort();
    check(chips.length === 2 && chips[0] === 'budget.xlsx' && chips[1] === 'plan.docx', 'one chip each: ' + JSON.stringify(chips));
    const ups = await uploads();
    check(ups.every((u) => u.body.file_kept === true && u.body.chars > 10 && !u.body.kept), 'text and file: ' + JSON.stringify(ups));
    await p.screenshot({ path: `${S}/doctext-1-plain-chips.png` });
    log('plain chat chips:', chips.join(', '));
    await p.fill('.composer textarea', 'what is in these two files?');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip');
    await p.waitForSelector('.msg.assistant');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    const sentChips = await p.$$eval('.msg.user ul.chips li', (n) => n.map((x) => x.textContent.trim()));
    check(sentChips.length === 2 && sentChips.some((c) => /^plan\.docx/.test(c)) && sentChips.some((c) => /^budget\.xlsx/.test(c)), 'sent chips, one per document: ' + JSON.stringify(sentChips));
    const sid = p.url().split('/').filter(Boolean).pop();
    const ev = await events(p, sid);
    const um = ev.find((e) => e.kind === 'user_message');
    const texts = um.payload.attachments.map((a) => a.text).join('\n');
    check(texts.includes('# Quarterly plan') && texts.includes('Owner | Dana'), 'the Word text: ' + texts);
    check(texts.includes('## Sheet: Budget') && texts.includes('Item\t42.5') && texts.includes('Coffee beans\t7'), 'the Excel text: ' + texts);
    check(um.payload.attached.length === 2 && !um.payload.uploads, 'and the files are kept for attached_file: ' + JSON.stringify(um.payload.attached));
    await p.screenshot({ path: `${S}/doctext-2-plain-sent.png` });
    log('plain chat: text of both reached the model; both files kept');

    // 2. A coding chat: the text goes and the file is in the chat's folder.
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p);
    await p.setInputFiles('.composer input[type=file]', `${F}/plan.docx`);
    await p.waitForSelector('.composer .chip:has-text("plan.docx")');
    await p.waitForFunction(() => window.__up.some((u) => u.body && u.body.file_kept && u.status === 200));
    await p.fill('.composer textarea', 'summarise the plan');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.user .chip:has-text("plan.docx")');
    await p.waitForSelector('.msg.assistant');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    check(fs.existsSync(`${R}/attachments/plan.docx`) && fs.statSync(`${R}/attachments/plan.docx`).size === fs.statSync(`${F}/plan.docx`).size, 'the file is in the folder: ' + (fs.existsSync(`${R}/attachments`) ? fs.readdirSync(`${R}/attachments`) : 'no attachments/'));
    const sid2 = p.url().split('/').filter(Boolean).pop();
    const um2 = (await events(p, sid2)).find((e) => e.kind === 'user_message');
    check(um2.payload.attachments[0].text.includes('# Quarterly plan'), 'text in the message: ' + JSON.stringify(um2.payload.attachments));
    check(um2.payload.uploads[0].path === 'attachments/plan.docx', 'upload path: ' + JSON.stringify(um2.payload.uploads));
    const sent2 = await p.$$eval('.msg.user ul.chips li', (n) => n.map((x) => x.textContent.trim()));
    check(sent2.length === 1 && /^plan\.docx/.test(sent2[0]), 'one chip in a coding chat: ' + JSON.stringify(sent2));
    await p.screenshot({ path: `${S}/doctext-3-coding-sent.png` });
    log('coding chat: text in the message, file in', `${R}/attachments`);

    // 3. A document that cannot be read: a plain file, with the sentence.
    await p.setInputFiles('.composer input[type=file]', `${F}/broken.docx`);
    await p.waitForSelector('.composer .chip-sentence:has-text("could not be read")');
    const bad = (await p.textContent('.composer .chip:has-text("broken.docx")')).trim();
    const why = (await p.textContent('.composer .chip-sentence')).trim();
    check(/broken\.docx/.test(bad) && /saved to the folder/.test(bad) && /^this document could not be read/.test(why), 'the chip says so: ' + bad + ' / ' + why);
    await p.screenshot({ path: `${S}/doctext-4-unreadable.png` });
    log('unreadable:', bad, '/', why);

    // 4. Phone width.
    await p.setViewportSize({ width: 390, height: 780 });
    await p.reload();
    await p.waitForSelector('.composer textarea');
    await p.setInputFiles('.composer input[type=file]', [`${F}/plan.docx`, `${F}/broken.docx`]);
    await p.waitForSelector('.composer .chip-sentence:has-text("could not be read")');
    const overflow = await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
    check(!overflow, 'no horizontal scroll at 390 px');
    await p.screenshot({ path: `${S}/doctext-5-phone.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/doctext-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

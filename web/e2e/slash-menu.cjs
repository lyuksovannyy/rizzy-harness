// The composer's `/` menu (X.65) end to end: a coding chat in a repository with
// three skills (one `user-invocable: false`) lists /compact and the two others
// when `/` is typed; typing filters; Down and Enter complete without sending;
// the completed `/ship` is then sent as before; a path opens nothing; the
// mouse completes; Escape keeps the text; a plain chat lists only /compact;
// at phone width the menu fits.
// Usage: node web/e2e/slash-menu.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-slash-'));
const D = `${S}/data`, W = `${S}/work`, HOME = `${S}/home`;
const R = `${W}/repo`;
for (const d of [D, R, HOME]) fs.mkdirSync(d, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;
const put = (file, text) => { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, text); };

(async () => {
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${W}"]\nmax_approval_mode = "ask"\n`);
  put(`${R}/a.txt`, 'one\n');
  fs.mkdirSync(`${R}/.git`, { recursive: true });
  put(`${R}/.agents/skills/deploy/SKILL.md`, '---\nname: deploy\ndescription: Ship the build to staging\n---\nDeploy steps.\n');
  put(`${R}/.agents/skills/ship/SKILL.md`, '---\nname: ship\ndescription: <img src=x onerror=alert(1)> Cut a release\n---\nShip steps.\n');
  put(`${R}/.claude/skills/quiet/SKILL.md`, '---\nname: quiet\ndescription: Only for the model\nuser-invocable: false\n---\nQuiet steps.\n');

  // The node's home is scratch: the owner's own skill folders stay out of it.
  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore', env: { ...process.env, HOME } });
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    const menuCalls = [];
    const otherCalls = [];
    p.on('request', (r) => {
      const u = r.url();
      if (/\/api\/sessions\/[^/]+\/skills-menu$/.test(u)) menuCalls.push(u);
    });
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // The new-chat composer has no menu: nothing there to complete to.
    await p.fill('.composer textarea', '/');
    await p.waitForTimeout(300);
    check((await p.$('.slash-menu')) === null, 'a menu in the new-chat composer');
    check(menuCalls.length === 0, 'asked before a chat existed');

    // 1. A coding chat in the repository.
    await p.fill('.composer textarea', 'run: cat a.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.msg.assistant:not(.streaming)');
    const names = () => p.$$eval('.slash-menu li b', (l) => l.map((x) => x.textContent));

    // 2. `/` lists the command and the skills a person can ask for, not the hidden one.
    await p.click('.composer textarea');
    await p.keyboard.type('/');
    await p.waitForSelector('.slash-menu li');
    const all = await names();
    check(JSON.stringify(all) === JSON.stringify(['/compact', '/deploy', '/ship']), 'entries: ' + all);
    check((await p.getAttribute('.slash-menu', 'role')) === 'listbox', 'not a listbox');
    check((await p.getAttribute('.slash-menu li.active', 'aria-selected')) === 'true', 'no active option');
    const said = await p.textContent('.slash-menu + [role=status]');
    check(/^\/compact \[focus\], .*, 1 of 3$/.test(said), 'announcement: ' + said);
    // Repository text is text.
    check((await p.$('.slash-menu img')) === null, 'markup in a description became an element');
    check((await p.textContent('.slash-menu')).includes('<img src=x onerror=alert(1)>'), 'the description is shown as text');
    await p.screenshot({ path: `${S}/slash-1-open.png` });

    // 3. Filter, then Down and Enter: completed, nothing sent.
    await p.keyboard.type('s');
    const filtered = await names();
    check(JSON.stringify(filtered) === JSON.stringify(['/ship']), 'filtered: ' + filtered);
    await p.keyboard.press('Backspace');
    await p.keyboard.press('ArrowDown');
    check((await p.textContent('.slash-menu li.active b')) === '/deploy', 'Down');
    const userBefore = (await p.$$('.msg.user')).length;
    await p.keyboard.press('Enter');
    check((await p.inputValue('.composer textarea')) === '/deploy ', 'completed: ' + await p.inputValue('.composer textarea'));
    check((await p.$('.slash-menu')) === null, 'the menu stayed open');
    check((await p.$$('.msg.user')).length === userBefore, 'Enter sent the message');
    const caret = await p.evaluate(() => document.querySelector('.composer textarea').selectionStart);
    check(caret === 8, 'caret: ' + caret);
    await p.screenshot({ path: `${S}/slash-2-completed.png` });

    // 4. Send it as before: the message goes with the skill named.
    await p.keyboard.type('to staging');
    await p.keyboard.press('Enter');
    await p.waitForFunction((n) => document.querySelectorAll('.msg.user').length === n + 1, userBefore);
    await p.waitForSelector('.msg.assistant:not(.streaming)');
    check((await p.$$eval('.msg.user', (l) => l.at(-1).textContent)).includes('/deploy to staging'), 'the sent message');

    // 5. A path opens nothing; a later slash opens nothing; Escape keeps the text.
    await p.fill('.composer textarea', '/usr/bin');
    await p.waitForTimeout(200);
    check((await p.$('.slash-menu')) === null, 'a menu for a path');
    await p.fill('.composer textarea', 'see /de');
    await p.waitForTimeout(200);
    check((await p.$('.slash-menu')) === null, 'a menu for a later slash');
    await p.fill('.composer textarea', '/sh');
    await p.waitForSelector('.slash-menu li');
    await p.keyboard.press('Escape');
    check((await p.$('.slash-menu')) === null, 'Escape left the menu open');
    check((await p.inputValue('.composer textarea')) === '/sh', 'Escape changed the text');

    // 6. The mouse completes.
    await p.fill('.composer textarea', '/');
    await p.waitForSelector('.slash-menu li');
    await p.click('.slash-menu li:has-text("/ship")');
    check((await p.inputValue('.composer textarea')) === '/ship ', 'mouse: ' + await p.inputValue('.composer textarea'));

    // 7. `/compact` in full is still the command.
    const users = (await p.$$('.msg.user')).length;
    await p.fill('.composer textarea', '/compact');
    await p.waitForSelector('.slash-menu li');
    await p.keyboard.press('Enter');
    // The command ran on the node (a short chat is refused, in words); no message was sent.
    await p.waitForSelector('text=too short to summarise');
    check((await p.$$('.msg.user')).length === users, '/compact was sent as a message');
    log('/compact in full ran as a command');

    // 8. One read for the whole visit to the menu (kept for a short time), never polled.
    check(menuCalls.length === 1, 'reads of the list: ' + menuCalls.length);
    const idle = menuCalls.length;
    await p.waitForTimeout(2000);
    check(menuCalls.length === idle, 'the menu polled');

    // 9. A phone: the menu fits.
    await p.setViewportSize({ width: 390, height: 800 });
    await p.reload();
    await p.waitForSelector('.composer textarea');
    await p.fill('.composer textarea', '/');
    await p.waitForSelector('.slash-menu li');
    const phone = await p.evaluate(() => {
      const r = document.querySelector('.slash-menu').getBoundingClientRect();
      return { overflow: document.documentElement.scrollWidth > window.innerWidth, left: r.left, right: r.right };
    });
    check(!phone.overflow && phone.left >= 0 && phone.right <= 390, 'phone menu: ' + JSON.stringify(phone));
    await p.screenshot({ path: `${S}/slash-3-phone.png` });
    await p.setViewportSize({ width: 1280, height: 900 });

    // 10. A plain chat lists only the built-in command.
    await p.goto(URL_);
    await p.click('text=New chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'hello there');
    await p.click('.composer .send');
    await p.waitForSelector('.msg.assistant:not(.streaming)');
    await p.click('.composer textarea');
    await p.keyboard.type('/');
    await p.waitForSelector('.slash-menu li');
    await p.waitForTimeout(500);
    const plain = await names();
    check(JSON.stringify(plain) === JSON.stringify(['/compact']), 'plain chat entries: ' + plain);
    await p.screenshot({ path: `${S}/slash-4-plain.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/slash-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

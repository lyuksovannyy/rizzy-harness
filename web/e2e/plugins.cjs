// X.34 step A end to end: the Plugins screen. The embedded node has a skill
// in its own skill folder (whose description holds markup) and a node.toml
// connector; the sidebar's Plugins lists both with the node that has them,
// the markup as text; the kind filter and the search narrow the list; the
// owner assigns the skill to one agent (read back through the API) and makes
// it Shared again; X.34 step B: the owner writes a skill from the Add menu,
// opens it (text only), and a chat of the Assistant loads it with the demo
// provider; the page fits a phone's width. (What a turn then carries is
// crates/master/tests/it/plugins_http.rs and stored_skills_http.rs.)
// Usage: node web/e2e/plugins.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eplugins-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.mkdirSync(`${S}/skills/deploy`, { recursive: true });
fs.writeFileSync(`${S}/skills/deploy/SKILL.md`, '---\nname: deploy\ndescription: Ship it <b>now</b>\n---\nSteps.\n');
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(
  `${S}/data/node.toml`,
  `workspace_roots = ["${S}/ws"]\n\n[execution]\nhost = true\n\n[skills]\nroots = ["${S}/skills"]\n\n[mcp_servers.notes]\ncommand = "python3"\nargs = ["-c", "pass"]\n`,
);
const URL_ = 'http://127.0.0.1:7973/';
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7973'], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
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
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');
    const api = (method, url, body) => p.evaluate(async ([m, u, b]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined });
      return { status: r.status, body: await r.json().catch(() => null) };
    }, [method, url, body]);
    const me = await api('GET', '/api/me');
    const ws = me.body.workspace.workspace_id;

    // The sidebar's Plugins (it replaced the "Connections" placeholder).
    check(await p.locator('nav[aria-label="Main"] a:has-text("Connections")').count() === 0, 'no Connections placeholder');
    await p.click('nav[aria-label="Main"] a:has-text("Plugins")');
    await p.waitForSelector('h1:text-is("Plugins")');
    let rows = p.locator('.plugin-row');
    for (let i = 0; i < 100 && (await rows.count()) < 2; i++) {
      await sleep(100);
      await p.reload();
      await p.waitForSelector('h1:text-is("Plugins")');
      rows = p.locator('.plugin-row');
    }
    const names = await p.locator('.plugin-row strong').allTextContents();
    check(JSON.stringify(names) === JSON.stringify(['notes', 'deploy']), `both listed: ${names}`);
    const deploy = p.locator('.plugin-row', { hasText: 'deploy' });
    const text = await deploy.textContent();
    check(text.includes('Ship it <b>now</b>'), `the markup is text: ${text}`);
    check(await deploy.locator('b').count() === 0, 'no markup rendered');
    check(text.includes('Shared: every agent'), 'Shared by default');
    check(/Node: /.test(text), 'the node that has it');
    await p.screenshot({ path: `${S}/plugins-1-list.png` });
    log('lists the skill and the connector');

    await p.click('[role="radio"]:text-is("Connectors")');
    check(JSON.stringify(await p.locator('.plugin-row strong').allTextContents()) === '["notes"]', 'the filter');
    await p.click('[role="radio"]:text-is("All")');
    await p.fill('input[aria-label="Search plugins"]', 'ship');
    check(JSON.stringify(await p.locator('.plugin-row strong').allTextContents()) === '["deploy"]', 'the search reads descriptions');
    await p.fill('input[aria-label="Search plugins"]', '');
    log('filter and search');

    // Assign the skill to the Software Developer only.
    await deploy.locator('button:text-is("Change")').click();
    await deploy.locator('[role="radio"]:text-is("Assigned")').click();
    await deploy.locator('label', { hasText: '@coder' }).locator('input').check();
    await deploy.locator('button:text-is("Save")').click();
    await p.waitForSelector('.plugin-row:has-text("Assigned: Software Developer")');
    await p.screenshot({ path: `${S}/plugins-2-assigned.png` });
    const listed = await api('GET', `/api/workspaces/${ws}/plugins`);
    const item = listed.body.items.find((i) => i.name === 'deploy');
    check(item.access === 'assigned' && JSON.stringify(item.agents) === '["coder"]', `saved: ${JSON.stringify(item)}`);
    log('assigned deploy to @coder');

    // Shared again from the screen.
    await deploy.locator('button:text-is("Change")').click();
    await deploy.locator('[role="radio"]:text-is("Shared")').click();
    await deploy.locator('button:text-is("Save")').click();
    await p.waitForSelector('.plugin-row:has-text("deploy"):has-text("Shared: every agent")');
    log('Shared again');

    // Step B: write a skill here, open it, and have the Assistant load it.
    await p.click('button:text-is("Add")');
    await p.click('[role="menuitem"]:text-is("Write a skill")');
    await p.getByLabel('Name').fill('release-notes');
    await p.getByLabel('Description: what it is for and when to use it').fill('Write release notes <i>from</i> the log');
    await p.getByLabel('Instructions').fill('Group the changes by area.\n<b>Never</b> invent one.');
    await p.click('.plugin-editor button:text-is("Save")');
    const stored = p.locator('.plugin-row', { hasText: 'release-notes' });
    await stored.waitFor();
    check((await stored.textContent()).includes('Workspace'), 'shown as the workspace\'s');
    await stored.locator('button:text-is("Open")').click();
    await stored.locator('pre.skill-body').waitFor();
    check((await stored.locator('pre.skill-body').textContent()).includes('<b>Never</b> invent one.'), 'the body as text');
    check(await stored.locator('b, i').count() === 0, 'no markup rendered');
    await p.screenshot({ path: `${S}/plugins-4-stored.png` });
    log('wrote and opened a stored skill');
    const chat = await api('POST', '/api/sessions', { profile: 'chat', workspace_id: ws });
    check(chat.status === 200, `chat: ${JSON.stringify(chat.body)}`);
    const sid = chat.body.session_id;
    const script = JSON.stringify([{ tool: 'skill', input: { name: 'release-notes' } }]);
    const sent = await api('POST', `/api/sessions/${sid}/prompt`, { text: `script: ${script}` });
    check(sent.status === 200, `prompt: ${JSON.stringify(sent.body)}`);
    let done = null;
    for (let i = 0; i < 300 && !done; i++) {
      await sleep(100);
      const evs = (await api('GET', `/api/sessions/${sid}/events?limit=5000`)).body.events || [];
      if (evs.some((e) => e.kind === 'turn_ended')) done = evs;
    }
    check(done, 'the turn ended');
    const fin = done.find((e) => e.kind === 'tool_call_finished');
    check(fin && fin.payload.ok && fin.payload.output.endsWith('<b>Never</b> invent one.'), `the Assistant loaded it: ${JSON.stringify(fin)}`);
    log('the Assistant loaded the stored skill');

    // A narrow window: the page still fits.
    await p.setViewportSize({ width: 380, height: 800 });
    const wide = await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
    check(!wide, 'no sideways scroll at phone width');
    await p.screenshot({ path: `${S}/plugins-3-narrow.png` });

    check(errors.length === 0, `no page errors: ${errors.join('; ')}`);
    log('ok');
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/plugins-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

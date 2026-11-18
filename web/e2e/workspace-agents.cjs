// X.33 end to end: workspace agents. In Settings → Workspace → Agents the
// owner of a workspace (here their Personal one) makes an agent only that
// workspace has; it is marked "workspace" there and on the sidebar Agents
// page, where it opens read-only (that page changes the server's agents);
// the chat picker of that workspace offers it; a team workspace neither
// lists it nor runs it; and a built-in gets this workspace's additional
// instructions under the server's.
// Usage: node web/e2e/workspace-agents.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ewsagents-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
const URL_ = 'http://127.0.0.1:7967/';
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7967'], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
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

    // Settings → Workspace → Agents (the Personal workspace): New agent.
    await p.goto(`${URL_}#/settings/workspace/agents`);
    await p.waitForSelector('.agent-list');
    await p.click('button:text-is("New agent")');
    await p.fill('input[aria-label="Name"]', 'Gardener');
    await p.fill('textarea.agent-prompt-input', 'You tend gardens. GARDEN-PROMPT');
    await p.click('button:text-is("Create agent")');
    await p.waitForSelector('input[aria-label="Name"]');
    await p.click('text=← All agents');
    const row = p.locator('li.agent-item', { hasText: '@gardener' });
    await row.waitFor();
    check((await row.textContent()).includes('workspace'), 'the new agent is marked workspace');
    check(await row.locator('[aria-label="Delete Gardener"]').count() === 1, 'the workspace may delete its own agent');
    check(await p.locator('[aria-label="Delete Assistant"]').count() === 0, 'a built-in has no delete');
    await p.screenshot({ path: `${S}/wsagents-1-settings.png` });
    log('made @gardener in the workspace');

    // A built-in gets this workspace's additional instructions.
    await p.locator('li.agent-item', { hasText: '@general' }).locator('.agent-open').click();
    await p.waitForSelector('text=Additional instructions for this workspace');
    await p.fill('#' + (await p.locator('label:text-is("Additional instructions for this workspace")').getAttribute('for')), 'WORKSPACE-RULE');
    await p.click('button:text-is("Save")');
    await p.waitForSelector('text=Saved.');
    await p.click('text=← All agents');
    log('added workspace instructions to the Assistant');

    // The sidebar Agents page lists it, marked, and opens it read-only.
    await p.goto(`${URL_}#/agents`);
    const side = p.locator('li.agent-item', { hasText: '@gardener' });
    await side.waitFor();
    check((await side.textContent()).includes('workspace'), 'marked workspace on the Agents page');
    check(await side.locator('[aria-label="Edit Gardener"]').count() === 0, 'the server page does not edit a workspace agent');
    await side.locator('.agent-open').click();
    await p.waitForSelector('h1:text-is("Gardener")');
    check(await p.locator('input[aria-label="Name"]').count() === 0, 'read-only there');
    await p.screenshot({ path: `${S}/wsagents-2-sidebar.png` });
    log('the sidebar page shows it read-only');

    // The chat runs it, with the instructions where they belong.
    const me = await api('GET', '/api/me');
    const personal = me.body.workspace.workspace_id;
    const chat = await api('POST', '/api/sessions', { profile: 'chat', agent_id: 'gardener', workspace_id: personal });
    check(chat.status === 200, `a chat with @gardener (${chat.status} ${JSON.stringify(chat.body)})`);

    // A team workspace: not listed, not usable.
    const team = await api('POST', '/api/workspaces', { name: 'Acme' });
    check(team.status === 200, 'a team workspace');
    const tid = team.body.workspace_id;
    const listed = await api('GET', `/api/agents?workspace=${tid}`);
    check(!listed.body.agents.some((a) => a.id === 'gardener'), 'another workspace does not list it');
    const general = listed.body.agents.find((a) => a.id === 'general');
    check(general && general.scope === 'server', 'the Assistant is the server\'s there');
    const refused = await api('POST', '/api/sessions', { profile: 'chat', agent_id: 'gardener', workspace_id: tid });
    check(refused.status === 400, `another workspace refuses it (${refused.status})`);
    const page = await api('GET', `/api/agents/general?workspace=${tid}`);
    check(page.body.instructions === '', 'the workspace\'s instructions stay in it');
    log('a team workspace neither lists nor runs it');

    check(errors.length === 0, `no page errors: ${errors.join('; ')}`);
    log('ok');
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/wsagents-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

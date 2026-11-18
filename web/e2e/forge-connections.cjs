// X.27 step 1 end to end: a person connects a forge account on the Plugins
// screen with a pasted token, sees it as @login, replaces the token and
// disconnects. The forge is a fake on 127.0.0.1 (a Gitea API), allowed by
// the server's config.toml with private_address = true; the token is made
// up. Afterwards the token is in no answer the page got, not in the page,
// not in the server's log and not in any file of the data directory in the
// clear (`master.db` holds it sealed).
// Usage: node web/e2e/forge-connections.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eforge-')));
const TOKEN = 'e2e_CANARY_fake_token_0000000000000001';
const TOKEN2 = 'e2e_CANARY_fake_token_0000000000000002';
const URL_ = 'http://127.0.0.1:7976/';
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

const seen = [];
const forge = http.createServer((req, res) => {
  seen.push({ url: req.url, auth: req.headers.authorization || null });
  if (req.url === '/api/v1/user' && [`token ${TOKEN}`, `token ${TOKEN2}`].includes(req.headers.authorization)) {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ id: 77, login: 'ada', full_name: 'Ada <b>L</b>', email: 'ada@example.com' }));
  } else {
    res.writeHead(401, { 'content-type': 'text/plain' });
    res.end(`bad token ${req.headers.authorization}`);
  }
});

function walk(d) {
  return fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)]));
}

(async () => {
  await new Promise((r) => forge.listen(0, '127.0.0.1', r));
  const port = forge.address().port;
  fs.mkdirSync(`${S}/data`, { recursive: true });
  fs.writeFileSync(
    `${S}/data/config.toml`,
    '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n\n' +
      `[[forge.hosts]]\nhost = "127.0.0.1"\nkind = "gitea"\napi_base = "http://127.0.0.1:${port}/api/v1"\nprivate_address = true\n`,
  );
  // The embedded node reads no skill folder of the developer's.
  fs.writeFileSync(`${S}/data/node.toml`, '[skills]\nroots = []\n');
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7976'], {
    detached: true,
    env: { ...process.env, RUST_LOG: 'debug' },
    stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')],
  });
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    const answers = [];
    p.on('pageerror', (e) => errors.push(e.message));
    p.on('response', async (r) => {
      if (r.url().includes('/api/')) answers.push(`${r.status()} ${JSON.stringify(r.headers())} ${await r.text().catch(() => '')}`);
    });
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');

    await p.click('nav[aria-label="Main"] a:has-text("Plugins")');
    await p.waitForSelector('h2:text-is("Connectors")');
    const item = p.locator('.connection-row', { hasText: '127.0.0.1' });
    check((await item.textContent()).includes('Gitea'), 'the configured host is listed');
    check((await p.locator('.connection-row').count()) === 3, 'github.com, gitlab.com and the configured host');
    check((await p.textContent('section[aria-label="Forge accounts"]')).includes('cloning and pushing come next'), 'says what is not done');

    // A wrong token: refused, nothing stored.
    await item.locator('button:text-is("Connect")').click();
    check((await item.textContent()).includes(`http://127.0.0.1:${port}`) || (await item.textContent()).includes('127.0.0.1/user/settings/applications'), 'where a token is made');
    await item.locator('input[type="password"]').fill('e2e_wrong_token_0000000000');
    await item.locator('button:text-is("Save")').click();
    await p.waitForSelector('text=refused this token');
    log('a wrong token is refused');

    await item.locator('input[type="password"]').fill(TOKEN);
    await item.locator('button:text-is("Save")').click();
    await item.locator('text=@ada').waitFor();
    const shown = await item.textContent();
    check(shown.includes('Ada <b>L</b>'), `forge text as text: ${shown}`);
    check((await item.locator('b').count()) === 0, 'no markup rendered');
    check(shown.includes('does not report'), 'no scopes from Gitea');
    await p.screenshot({ path: `${S}/forge-1-connected.png` });
    log('connected @ada');

    await item.locator('button:text-is("Replace token")').click();
    await item.locator('input[type="password"]').fill(TOKEN2);
    await item.locator('button:text-is("Save")').click();
    await item.locator('button:text-is("Replace token")').waitFor();
    check(seen.at(-1).auth === `token ${TOKEN2}`, 'the new token was checked');
    log('replaced the token');

    // Still there after a reload, the token nowhere in the page.
    await p.reload();
    await p.waitForSelector('text=@ada');
    const html = await p.content();
    check(!html.includes(TOKEN) && !html.includes(TOKEN2), 'the token is not in the page');

    p.once('dialog', (d) => d.accept());
    await item.locator('button:text-is("Disconnect")').click();
    await item.locator('button:text-is("Connect")').waitFor();
    check((await item.locator('text=@ada').count()) === 0, 'disconnected');
    log('disconnected');

    await p.setViewportSize({ width: 380, height: 800 });
    const wide = await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
    check(!wide, 'no sideways scroll at phone width');
    await p.screenshot({ path: `${S}/forge-2-narrow.png` });

    for (const t of [TOKEN, TOKEN2]) {
      check(!answers.join('\n').includes(t), 'no API answer holds the token');
      check(!fs.readFileSync(`${S}/serve.log`, 'utf8').includes(t), 'the server log does not hold the token');
      for (const f of walk(`${S}/data`)) check(!fs.readFileSync(f).includes(t), `${f} does not hold the token in the clear`);
      check(!seen.some((s) => s.url.includes(t)), 'never in a URL');
    }
    check(fs.statSync(`${S}/data/keys/secrets.key`).mode % 0o1000 === 0o600, 'the key file is 0600');
    log('the token is nowhere in the clear');

    check(errors.length === 0, `no page errors: ${errors.join('; ')}`);
    log('ok');
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/forge-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    forge.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

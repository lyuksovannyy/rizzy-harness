// The login key (Gate 3 P-1) in a real browser. The cookie reaches every
// port of the host, so the cookie alone must authenticate nothing: the page
// sends its key as `x-harness-login` and as the socket's first message, and
// keeps it in its own origin's localStorage (`harness.login`). See
// crates/master/src/auth.rs (module doc), crates/master/src/ws.rs and
// web/src/api.ts.
//
// Checks: after sign-in the page works and a reload stays signed in; a new
// tab of the origin has the key; the key never appears in a URL or a cookie;
// the API refuses the cookie without the key (from the page and from
// Playwright's request context), and takes it with the key; a page on
// another port of the same host gets nothing from the API or the socket;
// the socket closes with 4401 when its first message is not the key; log
// out clears the key, and the old cookie and key are dead after it.
// Usage: node web/e2e/login-key.cjs <harness-binary> [work-dir]
const { chromium, request } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2elk-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
const PORT = 7801;
const URL_ = `http://127.0.0.1:${PORT}/`;
const KEY = 'harness.login';
const log = (...a) => console.log('•', ...a);
const check = (c, m) => { if (!c) throw new Error(m); };

/** A page on another port of the same host, as another local dev server
 * would serve it: it tries the harness API and socket with the browser's
 * cookie and reports what it could read. */
function otherPort() {
  const page = `<!doctype html><meta charset="utf-8"><title>other port</title><script>
window.probe = async (base) => {
  const out = {};
  for (const [name, init] of [
    ['get', { credentials: 'include' }],
    ['no-cors', { credentials: 'include', mode: 'no-cors' }],
    ['post', { credentials: 'include', method: 'POST', headers: { 'content-type': 'text/plain' }, body: '{}' }],
  ]) {
    try {
      const r = await fetch(base + 'api/sessions', init);
      out[name] = { status: r.status, type: r.type, body: (await r.text()).slice(0, 200) };
    } catch (e) {
      out[name] = { error: String(e) };
    }
  }
  out.ws = await new Promise((resolve) => {
    const ws = new WebSocket(base.replace('http', 'ws') + 'api/ws');
    const got = [];
    ws.onopen = () => ws.send(JSON.stringify({ type: 'subscribe', session_id: 'x', after_seq: 0 }));
    ws.onmessage = (m) => got.push(String(m.data).slice(0, 200));
    ws.onclose = (e) => resolve({ code: e.code, got });
    setTimeout(() => resolve({ code: 'still open', got }), 8000);
  });
  return out;
};
</script><p>other port</p>`;
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => res.writeHead(200, { 'content-type': 'text/html' }).end(page));
    srv.listen(0, '127.0.0.1', () => resolve({ srv, url: `http://127.0.0.1:${srv.address().port}/` }));
  });
}

/** From the harness page: a socket whose first message is `first` (or
 * none), and how it ends within `ms`. */
const socket = (p, first, ms = 8000) => p.evaluate(([first, ms]) => new Promise((resolve) => {
  const ws = new WebSocket(`ws://${location.host}/api/ws`);
  const got = [];
  ws.onopen = () => { if (first !== null) ws.send(JSON.stringify(first)); };
  ws.onmessage = (m) => got.push(String(m.data).slice(0, 200));
  ws.onclose = (e) => resolve({ code: e.code, got });
  setTimeout(() => { ws.close(); resolve({ code: 'still open', got }); }, ms);
}), [first, ms]);

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: 'ignore' });
  const other = await otherPort();
  let p;
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
    p = await ctx.newPage();
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    const urls = [];
    ctx.on('request', (r) => urls.push(r.url()));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // 1. Signed in: the key is in this origin's storage, the page works, and
    // a reload and a new tab stay signed in.
    const key = await p.evaluate((k) => localStorage.getItem(k), KEY);
    check(typeof key === 'string' && key.length >= 32, `a login key in localStorage (${key && key.length} chars)`);
    await p.keyboard.type('hello key');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=You said: hello key');
    await p.reload();
    await p.waitForSelector('.msg.assistant >> text=You said: hello key');
    check(await p.evaluate((k) => localStorage.getItem(k), KEY) === key, 'the key survives a reload');
    const tab = await ctx.newPage();
    await tab.goto(URL_);
    await tab.waitForSelector('h1.greeting');
    await tab.close();
    log('signed in: chat answered, reload and a new tab stay signed in');

    const cookies = await ctx.cookies(URL_);
    check(cookies.length > 0, 'a login cookie is set');
    check(cookies.every((c) => c.httpOnly && c.sameSite === 'Strict'), 'the cookie is HttpOnly and SameSite=Strict: ' + JSON.stringify(cookies.map((c) => [c.name, c.httpOnly, c.sameSite])));
    check(cookies.every((c) => !c.value.includes(key)), 'the key is not in a cookie');
    check(!(await p.evaluate(() => document.cookie)).length, 'no cookie readable from script');

    // 2. The cookie without the key is refused; with it, taken. From the
    // page (the browser sends the cookie; no header is added here)...
    const fromPage = await p.evaluate(async (k) => {
      const plain = await fetch('/api/sessions');
      const wrong = await fetch('/api/sessions', { headers: { 'x-harness-login': 'A'.repeat(k.length) } });
      const keyed = await fetch('/api/sessions', { headers: { 'x-harness-login': k } });
      const write = await fetch('/api/sessions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' });
      return { plain: plain.status, wrong: wrong.status, keyed: keyed.status, write: write.status };
    }, key);
    check(fromPage.plain === 401 && fromPage.wrong === 401 && fromPage.write === 401, 'page fetch without the right key is refused: ' + JSON.stringify(fromPage));
    check(fromPage.keyed === 200, 'page fetch with the key works: ' + JSON.stringify(fromPage));
    // ...and from outside a browser with the page's cookies, as a program
    // on this machine that got the cookie would send them.
    const jar = cookies.map((c) => `${c.name}=${c.value}`).join('; ');
    const api = await request.newContext({ baseURL: URL_ });
    const bare = await api.get('api/sessions', { headers: { cookie: jar } });
    const me = await api.get('api/me', { headers: { cookie: jar } });
    const withKey = await api.get('api/sessions', { headers: { cookie: jar, 'x-harness-login': key } });
    check(bare.status() === 401 && me.status() === 401, `cookie alone refused by the API (sessions ${bare.status()}, me ${me.status()})`);
    check(withKey.status() === 200, `cookie with the key accepted (${withKey.status()})`);
    log('API: cookie alone 401 (page and request context), wrong key 401, with the key 200');

    // 3. A page on another port of the same host: the browser sends the
    // cookie (same site), but nothing comes back that the page can read,
    // and the socket does not open for it.
    const q = await ctx.newPage();
    await q.goto(other.url);
    const probe = await q.evaluate((base) => window.probe(base), URL_);
    for (const name of ['get', 'post']) {
      check(probe[name].error && /TypeError|Failed to fetch/.test(probe[name].error), `other port ${name}: no readable answer: ${JSON.stringify(probe[name])}`);
    }
    check(probe['no-cors'].type === 'opaque' && probe['no-cors'].body === '', `other port no-cors: opaque and empty: ${JSON.stringify(probe['no-cors'])}`);
    check(probe.ws.code !== 'still open' && probe.ws.got.length === 0, `other port socket: closed, nothing received: ${JSON.stringify(probe.ws)}`);
    // What the server itself answers to that browser request (the page
    // cannot see it): its Origin is refused before any lookup.
    const foreign = await api.get('api/sessions', { headers: { cookie: jar, origin: other.url.replace(/\/$/, '') } });
    check(foreign.status() === 403, `the server refuses another port's Origin (${foreign.status()})`);
    await q.close();
    log('other port: fetch', probe.get.error.split(':')[0], '| no-cors opaque | socket closed', probe.ws.code, '| server says', foreign.status());

    // 4. The socket: the key must be the first message.
    const notKey = await socket(p, { type: 'subscribe', session_id: '00000000-0000-0000-0000-000000000000', after_seq: 0 });
    check(notKey.code === 4401 && notKey.got.length === 0, 'first message not the key: closed 4401 with nothing sent: ' + JSON.stringify(notKey));
    const wrongKey = await socket(p, { type: 'login', key: 'A'.repeat(key.length) });
    check(wrongKey.code === 4401 && wrongKey.got.length === 0, 'wrong key: closed 4401: ' + JSON.stringify(wrongKey));
    const silent = await socket(p, null, 9000);
    check(silent.code === 4401 && silent.got.length === 0, 'no first message: closed 4401 after the wait: ' + JSON.stringify(silent));
    const right = await socket(p, { type: 'login', key }, 2000);
    check(right.code === 'still open', 'the right key keeps the socket open: ' + JSON.stringify(right));
    log('socket: not the key 4401, wrong key 4401, silence 4401, right key stays open');

    // 5. Log out clears the key; the old cookie and key are dead.
    await p.click('.sidebar-foot .nav-item.account');
    await p.getByRole('menuitem', { name: 'Log out', exact: true }).click();
    await p.waitForSelector('button:has-text("Log in")');
    check(await p.evaluate((k) => localStorage.getItem(k), KEY) === null, 'log out removes the key from localStorage');
    const after = await api.get('api/sessions', { headers: { cookie: jar, 'x-harness-login': key } });
    check(after.status() === 401, `old cookie and key refused after log out (${after.status()})`);
    await p.reload();
    await p.waitForSelector('button:has-text("Log in")');
    log('log out: key gone from localStorage; old cookie + key 401');

    check(urls.every((u) => !u.includes(key)), 'the key never appears in a URL');
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    await api.dispose();
    await b.close();
    log('screenshots in', S);
  } catch (e) {
    if (p) await p.screenshot({ path: `${S}/lk-fail.png` }).catch(() => {});
    throw e;
  } finally {
    try { process.kill(-srv.pid, 'SIGKILL'); } catch { /* already gone */ }
    other.srv.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

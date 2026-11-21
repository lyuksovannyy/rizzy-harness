// A chat's cost estimate against the real binary: a model priced in
// config.toml, a chat on it, the header's "≈" figure labelled an estimate;
// the admin switches the server's currency in Settings → AI providers and
// the chat shows it. The demo model's calls go through the provider router
// like any other, so this checks the per-model record end to end.
// Usage: node web/e2e/cost.cjs <harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ecost-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(
  `${S}/data/config.toml`,
  '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\n' +
    'models = [{ id = "mock", price = { input = 3000.0, output = 9000.0 } }]\n',
);
const PORT = 7794;
const URL_ = `http://127.0.0.1:${PORT}/`;
const log = (...a) => console.log('•', ...a);
const check = (c, m) => { if (!c) throw new Error(m); };

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: 'ignore' });
  let p;
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 } });
    await keyed(ctx);
    p = await ctx.newPage();
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    await p.fill('.composer textarea', 'hello');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const meter = p.locator('.cost-meter');
    await meter.waitFor({ timeout: 30000 });
    const text = await meter.textContent();
    check(/^≈ \$\d/.test(text), 'the strip shows a dollar estimate: ' + text);
    const hint = await meter.getAttribute('title');
    check(hint.includes('not the bill'), 'labelled an estimate: ' + hint);
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    const ev = await p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
    const usage = ev.find((e) => e.kind === 'turn_ended').payload.usage;
    check(usage.by_model && usage.by_model['demo::mock'], 'the turn recorded its model: ' + JSON.stringify(usage));
    check(usage.by_model['demo::mock'].input_tokens === usage.input_tokens, 'every token under it: ' + JSON.stringify(usage));
    log('header:', text, '| by_model:', JSON.stringify(usage.by_model));
    await p.screenshot({ path: `${S}/cost-1.png` });

    await p.goto(`${URL_}#/settings/server/providers`);
    const cur = p.locator('label:has-text("Currency of model prices") input');
    await cur.waitFor();
    check((await cur.inputValue()) === 'USD', 'USD by default');
    await cur.fill('eur');
    await p.click('label:has-text("Currency of model prices") ~ button:text-is("Save")');
    await p.waitForFunction(() => {
      const i = [...document.querySelectorAll('label')].find((l) => l.textContent.startsWith('Currency of model prices'));
      return i && i.parentElement.querySelector('button').disabled;
    });
    const toml = fs.readFileSync(`${S}/data/config.toml`, 'utf8');
    check(/\[pricing\]\s*currency = "EUR"/.test(toml), 'config.toml:\n' + toml);
    await p.goto(`${URL_}#/s/${sid}`);
    await p.reload();
    await meter.waitFor({ timeout: 30000 });
    const eur = await meter.textContent();
    check(eur.includes('€'), 'the chat shows the new currency: ' + eur);
    log('after the currency change:', eur);
    await p.screenshot({ path: `${S}/cost-2.png` });
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    await b.close();
    log('screenshots in', S);
  } catch (e) {
    if (p) await p.screenshot({ path: `${S}/cost-fail.png`, fullPage: true }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });

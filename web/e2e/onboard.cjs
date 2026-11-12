// Helpers shared by the e2e scripts for the first-run screens (not a script).
const fs = require('fs');

/**
 * The claim code of a server just started, once it has written the code to
 * `file` and answers on `url`. A cold debug build on a busy machine can take
 * longer than any fixed sleep.
 */
async function claimCode(file, url, timeoutMs = 60000) {
  const until = Date.now() + timeoutMs;
  for (;;) {
    try {
      const code = fs.readFileSync(file, 'utf8').trim();
      if (code && (await fetch(url)).ok) return code;
    } catch {
      // Not written or not listening yet.
    }
    if (Date.now() > until) throw new Error(`no server at ${url} (${file})`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

/**
 * Claim the server as its owner: the claim code opens the account fields,
 * the new password is typed twice.
 */
async function createOwner(p, code, username = 'alex', password = 'admin password') {
  await p.fill('input[aria-label="claim code"]', code);
  await p.fill('input[placeholder="username"]', username);
  await p.fill('input[autocomplete="new-password"]:not([placeholder="confirm password"])', password);
  await p.fill('input[placeholder="confirm password"]', password);
  await p.click('button:has-text("Create account")');
}

/** Onboarding's memory step: keep keyword recall and go on. */
async function keywordsOnly(p) {
  await p.click('[role=radiogroup][aria-label=recall] button:has-text("Keywords only")');
  await p.click('form button.primary:has-text("Save and continue")');
  await p.waitForSelector('h2:has-text("About you")');
}

/** Gate 3 P-1: the login cookie alone authenticates nothing. The scripts'
 * own in-page `fetch('/api/…')` calls send the login key as the app does
 * (it keeps it in localStorage, `harness.login`). Call on a context or a
 * page before its first navigation. */
async function keyed(target) {
  await target.addInitScript(() => {
    const f = window.fetch.bind(window);
    window.fetch = (u, o = {}) => {
      const k = localStorage.getItem('harness.login');
      if (!k || !String(u).startsWith('/api/')) return f(u, o);
      const h = new Headers(o.headers);
      if (!h.has('x-harness-login')) h.set('x-harness-login', k);
      return f(u, { ...o, headers: h });
    };
  });
}

/** Pick the composer's agent. Since the eight-agent roster (9c32186) the
 * default Assistant has no file or command tools, so an agentic chat that
 * runs a command or publishes must name the Software Developer. */
async function pickAgent(p, name = 'Software Developer') {
  await p.click('.composer button.select-button[aria-label^="agent:"]');
  await p.click(`[role=listbox][aria-label=agent] [role=option]:has-text("${name}")`);
  await p.waitForSelector(`.composer button.select-button[aria-label="agent: ${name}"]`);
}

module.exports = { claimCode, createOwner, keywordsOnly, keyed, pickAgent };

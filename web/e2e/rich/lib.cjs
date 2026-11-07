// Helpers shared by the rich-message e2e sections (see ../rich.cjs).
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/**
 * Polls `get` until it returns `want` (compared as JSON). After a reload the
 * log can reach the page in more than one batch, so a single read can catch
 * a card a moment before its answer arrives.
 */
async function eventually(get, want, msg, timeout = 5000) {
  const end = Date.now() + timeout;
  let got = await get();
  while (!same(got, want) && Date.now() < end) {
    await sleep(100);
    got = await get();
  }
  check(same(got, want), `${msg}: ${JSON.stringify(got)}`);
}

/** WCAG contrast of an element's text colour on another's background. */
async function contrast(fg, bg) {
  const rgb = (s) => s.match(/[\d.]+/g).slice(0, 3).map(Number);
  const lum = ([r, g, b]) => [r, g, b]
    .map((c) => (c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
    .reduce((a, c, i) => a + c * [0.2126, 0.7152, 0.0722][i], 0);
  const a = lum(rgb(await fg.evaluate((el) => getComputedStyle(el).color)));
  const b = lum(rgb(await bg.evaluate((el) => getComputedStyle(el).backgroundColor)));
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** The characters of an element's first text node, in on-screen order. */
const visualText = (loc) => loc.evaluate((el) => {
  const t = el.firstChild;
  const xs = [];
  for (let i = 0; i < t.length; i++) {
    const r = document.createRange();
    r.setStart(t, i);
    r.setEnd(t, i + 1);
    xs.push([t.data[i], r.getBoundingClientRect().x]);
  }
  return xs.sort((a, b) => a[1] - b[1]).map((x) => x[0]).join('');
});

// The hidden set of web/src/md/hidden.ts that a page can show (the BMP part).
const HIDDEN = /[\u{061C}\u{200B}-\u{200F}\u{202A}-\u{202E}\u{2060}-\u{2064}\u{2066}-\u{2069}\u{FEFF}\u{E000}-\u{F8FF}]/u;

/** Chat helpers bound to one page; `repo` is the agentic workspace. */
function chat(p, repo) {
  const count = () => p.locator('.msg.assistant').count();
  const idle = () => p.waitForFunction(() => !document.querySelector('.send.stop'));
  // Messages are taken by index from the top: `.last()` would move on as
  // later answers arrive.
  const lastMsg = async () => p.locator('.msg.assistant').nth((await count()) - 1);
  const waitAnswer = async (before) => {
    await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
    await idle();
    return lastMsg();
  };
  return {
    idle,
    lastMsg,
    /** A new chat (unless the page is on one already) in `mode`. */
    async newChat(mode) {
      if (!(await p.locator('h1.greeting').isVisible())) {
        await p.click('.new-chat');
        await p.waitForSelector('h1.greeting');
      }
      await p.click(`.mode-switch button:has-text("${mode}")`);
      if (mode === 'Agentic') await p.fill('.agentic-row input', repo);
    },
    /** Sends a message without waiting for the answer. */
    async type(text) {
      await p.fill('.composer textarea', text);
      await p.locator('.composer textarea').press('Enter');
    },
    /** Sends a message and returns the answer once the turn is over. */
    async send(text) {
      const before = await count();
      await this.type(text);
      return waitAnswer(before);
    },
    /** Clicks Send on a card and returns the answer to it. */
    async submit(card) {
      const before = await count();
      await card.locator('.ui-send').click();
      return waitAnswer(before);
    },
    /** Picks an option on a card and returns the answer to it. */
    async pick(card, label) {
      const before = await count();
      await card.locator(`.ui-option:has-text("${label}")`).click();
      return waitAnswer(before);
    },
  };
}

module.exports = { log, check, sleep, eventually, contrast, visualText, HIDDEN, chat };

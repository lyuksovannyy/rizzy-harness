// rich_plan 12.4 item 10, as memory works since the owner's decision of
// 2026-11-14 (README, Memory; e2e/memory-default.cjs): in the Personal
// workspace a fact the model remembers is saved at once, and the chat shows
// a note saying so with Undo, never a tool card; the fact is then on the
// Memory page as a kept fact (it was a proposal to keep before).
const { check, log } = require('./lib.cjs');

module.exports = async function memory(t) {
  const { p, S } = t;
  await t.newChat('Ask');
  await t.send('remember: likes tea');
  const notice = p.locator('.tool-memory');
  await notice.waitFor();
  // "Memory: …" while the server files the call, then what it did.
  await p.waitForFunction(
    () => document.querySelector('.tool-memory')?.textContent?.includes('Saved to your facts: “likes tea”'),
    null,
    { timeout: 15000 },
  );
  check((await p.locator('.tool-head').count()) === 0, 'a note, not a tool card');
  check((await notice.locator('.memory-undo').count()) === 1, 'Undo in the note');
  await notice.screenshot({ path: `${S}/rich-8-memory.png` });
  await p.goto(new URL('#/settings/account/memory', p.url()).toString());
  await p.waitForSelector('.fact:has-text("likes tea")');
  check((await p.locator('.fact.proposed:has-text("likes tea")').count()) === 0, 'kept, not waiting');
  log('item 10: the fact is saved at once, the note says so with Undo, and it is on the Memory page');
};

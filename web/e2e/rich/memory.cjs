// rich_plan 12.4 item 10: a memory the model proposes is a note in the chat
// linking to the Memory page, where it waits to be kept or discarded.
const { check, log } = require('./lib.cjs');

module.exports = async function memory(t) {
  const { p, S } = t;
  await t.newChat('Ask');
  await t.send('remember: likes tea');
  const notice = p.locator('.tool-memory');
  await notice.waitFor();
  const text = await notice.innerText();
  check(text.includes('Suggested a memory: “likes tea”') && text.includes('Not used until you keep it.'), 'notice: ' + text);
  check((await p.locator('.tool-head').count()) === 0, 'a note, not a tool card');
  check((await notice.locator('button').count()) === 0, 'no Keep or Discard in the chat');
  const review = notice.locator('a:has-text("Review in Memory")');
  check((await review.getAttribute('href')) === '#/settings/memory', 'links to the Memory page');
  await notice.screenshot({ path: `${S}/rich-8-memory.png` });
  await review.click();
  await p.waitForSelector('.fact.proposed:has-text("likes tea")');
  log('item 10: the proposal is a note with "Review in Memory", and waits there');
};

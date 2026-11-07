// rich_plan 12.4 item 7: the labels of a web result become pills naming the
// host, an unknown label stays text, a footer lists what was cited, and the
// web row is in the harness's words. Also: the hover card, sources open
// without a confirm, labels stable across turns and a reload, the shimmer,
// and the labels a delegated sub-task hands back; from the final reviews,
// focus back on the pill after Escape in the card and a readable unknown
// label.
const { check, log, contrast, focused } = require('./lib.cjs');

module.exports = async function cite(t) {
  const { p, ctx, S } = t;
  await t.newChat('Ask');
  const cited = await t.send('cite: rust');
  const row = p.locator('.tool-web .web-status');
  check((await row.innerText()) === 'Searched “rust” · 2 results', 'web row: ' + (await row.innerText()));
  const pills = await cited.locator('.cite').allInnerTexts();
  check(JSON.stringify(pills) === JSON.stringify(['a.example', 'b.example', 'a.example +1']), 'pills: ' + JSON.stringify(pills));
  check((await cited.locator('.cite').first().getAttribute('href')) === 'https://a.example/rust', 'a pill links to its source');
  check((await cited.locator('.cite-missing').innerText()) === '[S9]', 'an unknown label stays text');
  check((await cited.locator('.cite-missing').getAttribute('title')) === 'No such source', 'with a note');
  // Muted by its colour alone: dimmed with opacity it fell to 3.3:1.
  const missing = cited.locator('.cite-missing');
  const ratio = await contrast(missing, p.locator('body'));
  check((await missing.evaluate((el) => getComputedStyle(el).opacity)) === '1' && ratio >= 4.5, `unknown label at ${ratio.toFixed(2)}:1`);
  check((await cited.locator('.sources summary').innerText()) === 'Sources (2)', 'footer counts the cited sources');
  check((await cited.locator('.sources li').count()) === 2, 'footer lists two sources');
  check((await cited.locator('.md').innerText()).includes('Unknown [S9].'), 'text around the labels stays');
  log('item 7: pills', pills.join(', '), '· [S9] plain · footer of 2 · row:', await row.innerText());

  // Hover or focus shows the title and the full URL.
  await cited.locator('.cite').first().hover();
  const card = cited.locator('.cite-card');
  await card.waitFor();
  check((await card.locator('.cite-card-title').innerText()) === 'Rust language', 'card title');
  check((await card.locator('.cite-card-url').innerText()) === 'https://a.example/rust', 'card URL');
  await p.screenshot({ path: `${S}/rich-4-cite.png` });
  await p.mouse.move(0, 0);
  await card.waitFor({ state: 'detached' });
  await cited.locator('.cite').nth(2).focus();
  check((await cited.locator('.cite-card .cite-card-item').count()) === 2, 'a group card lists both sources');
  await p.keyboard.press('Escape');
  check((await cited.locator('.cite-card').count()) === 0, 'Escape closes the card');
  // Tab goes from the pill into its card; Escape there closes the card and
  // puts focus back on the pill, not at the top of the page.
  // (The third pill still has focus, so the first one: a focus event opens it.)
  const group = cited.locator('.cite').first();
  await group.focus();
  await p.keyboard.press('Tab');
  check((await focused(cited.locator('.cite-card-item').first())) === true, 'Tab moves into the card');
  await p.keyboard.press('Escape');
  check((await cited.locator('.cite-card').count()) === 0 && (await focused(group)) === true, 'Escape in the card: closed, focus on the pill');

  // A source needs no confirm: the click opens it (the page is stubbed).
  await ctx.route(/^https:\/\/a\.example\//, (r) => r.fulfill({ status: 200, contentType: 'text/plain', body: 'source page' }));
  const [popup] = await Promise.all([ctx.waitForEvent('page'), cited.locator('.cite').first().click()]);
  check(popup.url() === 'https://a.example/rust', 'opened ' + popup.url());
  check((await cited.locator('.link-confirm').count()) === 0, 'no confirm for a source');
  await popup.close();

  // The finished row opens to the query, host · title per result, and (one
  // more click) the text the model read.
  await p.locator('.tool-web .tool-head').click();
  check((await p.locator('.tool-web .web-input-value').innerText()) === 'rust', 'the query in full');
  const results = await p.locator('.tool-web .web-results li').allInnerTexts();
  check(JSON.stringify(results) === JSON.stringify(['a.example · Rust language', 'b.example · The Rust book']), 'results: ' + JSON.stringify(results));
  await p.locator('.tool-web details.web-output summary').click();
  check((await p.locator('.tool-web details.web-output pre').innerText()).startsWith('[S1] Rust language'), 'the output the model read');

  // Labels are stable for the session: the same pages cite the same way in
  // a later turn, and a reload resolves them again.
  const again = await (await t.send('cite: rust again')).locator('.cite').allInnerTexts();
  check(JSON.stringify(again) === JSON.stringify(pills), 'same labels in a later turn: ' + JSON.stringify(again));
  await p.reload();
  await p.locator('.msg.assistant .cite').first().waitFor();
  check((await p.locator('.msg.assistant').first().locator('.cite').allInnerTexts()).join() === pills.join(), 'pills after reload');
  log('cite: source opens without a confirm, results listed, labels stable across turns and reload');

  // While a search runs, its row shimmers, unless motion is reduced.
  await t.type('cite: slow rust');
  const running = p.locator('.tool-web .web-status.shimmer:has-text("Searching the web for “slow rust”")');
  await running.waitFor();
  check((await running.evaluate((el) => getComputedStyle(el).animationName)) === 'shimmer', 'shimmer runs');
  await p.emulateMedia({ reducedMotion: 'reduce' });
  check((await running.evaluate((el) => getComputedStyle(el).animationName)) === 'none', 'no shimmer under reduced motion');
  await p.emulateMedia({ reducedMotion: 'no-preference' });
  await p.waitForSelector('.tool-web .web-status:has-text("Searched “slow rust” · 2 results")');
  await t.idle();
  log('web row: shimmer while running, none under reduced motion');

  // A sub-task's citations: the sub-agent numbers on from this session (its
  // new page is S3, a known one keeps S2), and the labels it hands back
  // resolve to its pages here, not to this session's S1.
  await t.newChat('Ask');
  await p.selectOption('.composer select[aria-label="agent"]', 'ultracode');
  await t.send('cite: rust');
  await p.waitForSelector('.tool-web .web-status:has-text("Searched “rust” · 2 results")');
  await t.send('delegate: scout=cite: other rust');
  const relayed = p.locator('.msg.assistant:has-text("Sub-agent said:")');
  const subPills = await relayed.locator('.cite').allInnerTexts();
  check(JSON.stringify(subPills) === JSON.stringify(['c.example', 'b.example', 'c.example +1']), 'sub-task pills: ' + JSON.stringify(subPills));
  check((await relayed.locator('.cite').first().getAttribute('href')) === 'https://c.example/other', 'the sub-task page');
  check((await relayed.locator('.md').innerText()).includes('Unknown [S9].'), 'its unknown label stays text');
  log('delegate: the sub-task cites on from the session, and its labels resolve here:', subPills.join(', '));
};

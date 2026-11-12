// rich_plan 12.4 item 11: a chart is our own SVG from a chunk of its own,
// every value named on hover and to the keyboard, the numbers a click away.
const { check, log } = require('./lib.cjs');

module.exports = async function chart(t) {
  const { p, S, chunks } = t;
  await t.newChat('Ask');
  check(chunks('Chart').length === 0, 'the chart chunk waits for a chart');
  await t.type('ui: chart');
  await p.waitForSelector('.msg.assistant.streaming .ui-skeleton:has-text("Preparing chart…")');
  await t.idle();
  const msg = await t.lastMsg();
  const svg = msg.locator('.ui-card-chart svg.ui-chart');
  await svg.waitFor();
  check(chunks('Chart').length === 1, 'chart chunk loaded once');
  check((await msg.locator('.ui-from').innerText()) === 'from Assistant', 'framed as the agent\'s');
  const marks = svg.locator('.ui-chart-mark');
  check((await marks.count()) === 5, `5 bars, none for the missing value (got ${await marks.count()})`);
  const fills = await marks.evaluateAll((els) => [...new Set(els.map((el) => getComputedStyle(el).fill))]);
  check(fills.length === 2 && fills.every((f) => f.startsWith('rgb')), 'two series colours: ' + JSON.stringify(fills));
  check(JSON.stringify(await msg.locator('.ui-chart-legend li').allInnerTexts()) === '["debug","release"]', 'legend');
  check((await svg.getAttribute('aria-label')).startsWith('Bar chart: Build time by crate. 2 series: debug, release.'), 'summary');
  check((await marks.nth(3).getAttribute('aria-label')) === 'release, web: 22 s', 'a bar names its value');
  await marks.nth(3).hover();
  check((await msg.locator('.ui-chart-readout').innerText()) === '22 s release, web', 'hover readout');
  // One tab stop; the arrow keys walk the series.
  check((await svg.locator('[tabindex="0"]').count()) === 1, 'one tab stop');
  await marks.nth(0).focus();
  await p.keyboard.press('ArrowRight');
  check((await p.evaluate(() => document.activeElement.getAttribute('aria-label'))) === 'debug, web: 8 s', 'ArrowRight moves along the series');
  await p.keyboard.press('ArrowUp');
  check((await p.evaluate(() => document.activeElement.getAttribute('aria-label'))) === 'release, web: 22 s', 'ArrowUp moves to the next series');
  await msg.screenshot({ path: `${S}/rich-7-chart.png` });
  await p.emulateMedia({ colorScheme: 'dark' });
  await msg.screenshot({ path: `${S}/rich-7b-chart-dark.png` });
  await p.emulateMedia({ colorScheme: 'light' });

  // On a phone the chart is laid out at the width it is shown at, so its
  // tick text stays readable instead of shrinking with the picture.
  await p.setViewportSize({ width: 390, height: 844 });
  await p.waitForFunction(() => {
    const s = document.querySelector('svg.ui-chart');
    return s && Math.abs(s.viewBox.baseVal.width - s.getBoundingClientRect().width) < 2;
  });
  const tickPx = await svg.locator('.ui-chart-ticks text').first().evaluate((el) => el.getBoundingClientRect().height);
  check(tickPx >= 12, `tick text ${tickPx}px tall on a phone`);
  check((await marks.count()) === 5, 'the same bars on a phone');
  await msg.screenshot({ path: `${S}/rich-7c-chart-phone.png` });
  await p.setViewportSize({ width: 1280, height: 900 });

  // Show data: the same numbers as a table; Copy CSV gives them in full.
  const rows = msg.locator('.ui-chart-table tbody tr');
  check(!(await rows.first().isVisible()), 'the table waits for Show data');
  await msg.locator('.ui-chart-tools button:has-text("Show data")').click();
  check((await rows.count()) === 3 && (await rows.first().isVisible()), 'Show data shows 3 rows');
  check(JSON.stringify(await rows.allInnerTexts()) === JSON.stringify(['core\t12.5\t40', 'web\t8\t22', 'master\t30\t']), 'rows: ' + JSON.stringify(await rows.allInnerTexts()));
  await msg.locator('.ui-chart-tools button:has-text("Copy CSV")').click();
  await msg.locator('.ui-chart-tools button:has-text("Copied")').waitFor();
  const csv = await p.evaluate(() => navigator.clipboard.readText());
  check(csv === 'crate,debug (s),release (s)\r\ncore,12.5,40\r\nweb,8,22\r\nmaster,30,', 'csv: ' + JSON.stringify(csv));
  await p.reload();
  await p.locator('svg.ui-chart .ui-chart-mark').first().waitFor();
  check((await p.locator('svg.ui-chart .ui-chart-mark').count()) === 5, 'the chart after reload');
  log('item 11: 5 bars in 2 colours, legend, hover and keyboard readouts, Show data with 3 rows, CSV, after reload');
};

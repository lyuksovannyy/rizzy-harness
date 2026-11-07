// rich_plan 12.4 item 8: progress in an agentic turn on the local node, plus
// a turn stopped during a step and one stopped while a block streams.
const { check, log, eventually, contrast } = require('./lib.cjs');

const ALLOW = '.approval button:has-text("Allow bash for this session")';

/** Everything the progress cards of a chat show. */
const progressState = async (p) => {
  const card = p.locator('.ui-progress');
  return {
    cards: await card.count(),
    framed: await p.locator('.ui-card .ui-progress').count(),
    snapshots: await p.locator('.ui-progress-snapshot').evaluateAll((els) => els.map((el) => el.textContent)),
    snapshotFrom: await p.locator('.ui-card-snapshot:has(.ui-progress-snapshot) > .ui-from').allInnerTexts(),
    done: await card.locator('.ui-step-done').count(),
    steps: await card.locator('.ui-step').count(),
    status: await card.locator('.ui-progress-status').innerText(),
    bar: await card.locator('.ui-bar').getAttribute('aria-valuenow'),
    spinning: await p.locator('.ui-progress .spin, .ui-progress-snapshot .spin').count(),
  };
};
const FINAL = {
  cards: 1,
  framed: 1,
  snapshots: ['Working through 3 steps · 0/3 · Step 1', 'Working through 3 steps · 1/3 · Step 2'],
  snapshotFrom: ['from General', 'from General'],
  done: 3,
  steps: 3,
  status: 'Done',
  bar: '100',
  spinning: 0,
};

module.exports = async function progress(t) {
  const { p, S } = t;

  // The mock moves one block id on over three messages, each but the last
  // calling bash. While the first call waits for approval, the first block
  // is the live card: its active step spins, unless motion is reduced.
  await t.newChat('Agentic');
  await t.type('progress: 3');
  const allow = p.locator(ALLOW);
  await allow.waitFor();
  const spinner = p.locator('.ui-card .ui-progress .ui-step-active .spin');
  await spinner.waitFor();
  check((await p.locator('.ui-progress-status').innerText()) === 'In progress', 'live card is in progress');
  check((await spinner.evaluate((el) => getComputedStyle(el).animationName)) === 'spin', 'the active step spins');
  await p.emulateMedia({ reducedMotion: 'reduce' });
  check((await spinner.evaluate((el) => getComputedStyle(el).animationName)) === 'none', 'no spin under reduced motion');
  await p.emulateMedia({ reducedMotion: 'no-preference' });
  await allow.click();
  await t.idle();
  // The turn's latest block is the card; the earlier two are snapshot lines.
  await p.locator('.msg.assistant:has-text("All 3 steps done.") .ui-progress').waitFor();
  await eventually(() => progressState(p), FINAL, 'progress');
  check((await p.locator('.msg.assistant').count()) === 3, 'three messages, one per step');
  await p.screenshot({ path: `${S}/rich-6-progress.png`, fullPage: true });
  await p.reload();
  await p.locator('.msg.assistant:has-text("All 3 steps done.") .ui-progress').waitFor();
  await eventually(() => progressState(p), FINAL, 'progress after reload');
  log('item 8: one card with 3 done steps and 2 snapshot lines, also after reload; the live step spins, except under reduced motion');

  // A turn stopped while a step runs: nothing will move the card on, so its
  // active step has stopped, nothing spins, and the header says why.
  await t.newChat('Agentic');
  await t.type('progress: 2');
  await p.locator(ALLOW).waitFor();
  await p.locator('.ui-card .ui-progress .ui-step-active .spin').waitFor();
  await p.click('.send.stop');
  await t.idle();
  const stopped = p.locator('.ui-progress');
  await stopped.locator('.ui-step-stopped').waitFor();
  await eventually(async () => ({
    status: await stopped.locator('.ui-progress-status').innerText(),
    steps: await stopped.locator('.ui-step').evaluateAll((els) => els.map((el) => el.className)),
    spinning: await stopped.locator('.spin').count(),
  }), { status: 'Stopped · turn cancelled', steps: ['ui-step ui-step-stopped', 'ui-step ui-step-pending'], spinning: 0 }, 'stopped');
  // The status colours are readable on the card in both themes: 4.5:1 for
  // the status words, 3:1 for the step icon.
  for (const colorScheme of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme });
    const card = p.locator('.ui-card-progress');
    const words = await contrast(stopped.locator('.ui-progress-status'), card);
    const icon = await contrast(stopped.locator('.ui-step-stopped .ui-icon'), card);
    check(words >= 4.5 && icon >= 3, `${colorScheme}: stopped status ${words.toFixed(2)}:1, icon ${icon.toFixed(2)}:1`);
  }
  await p.emulateMedia({ colorScheme: 'light' });
  log('progress: a cancelled turn leaves the card stopped, with the reason, readable in both themes');

  // A turn stopped while the next progress message streams: that message
  // never reached Rust, so its fence has no block. It is a harness note,
  // not half a JSON body as code, live and after a reload.
  await t.newChat('Agentic');
  await t.type('progress: 3');
  await p.locator(ALLOW).click();
  await p.locator('.msg.assistant.streaming .ui-skeleton').waitFor();
  await p.click('.send.stop');
  await t.idle();
  const cutState = async () => {
    const last = p.locator('.msg.assistant').last();
    const text = await last.innerText();
    return {
      cut: await last.locator('.ui-cut').allInnerTexts(),
      code: await last.locator('pre, .codeblock').count(),
      json: text.includes('ui:progress') || text.includes('"steps"'),
      card: await p.locator('.ui-card-progress .ui-progress-status').innerText(),
    };
  };
  const cut = { cut: ['Cut off before it was finished'], code: 0, json: false, card: 'Stopped · turn cancelled' };
  await p.locator('.msg.assistant .ui-cut').waitFor();
  await eventually(cutState, cut, 'cut off');
  await p.screenshot({ path: `${S}/rich-6b-cut.png`, fullPage: true });
  await p.reload();
  await p.locator('.msg.assistant .ui-cut').waitFor();
  await eventually(cutState, cut, 'cut off after reload');
  log('progress: a message stopped mid-block shows "cut off", never its partial JSON, also after reload');
};

// rich_plan 12.4 items 3 (ui: choices), 9 (stale), 6 (errors), 5 (ui:
// secret) and 4 (ui: form), in one chat so a reload can show them all
// again from the log.
const { check, log, eventually } = require('./lib.cjs');

/** What an answered or locked choices card shows. */
const choicesState = async (card) => ({
  chosen: await card.locator('.ui-option-chosen .ui-option-label').allInnerTexts(),
  enabled: await card.locator('.ui-option:not([disabled])').count(),
  state: await card.locator('.ui-state').innerText(),
});
const ANSWERED = { chosen: ['Blue'], enabled: 0, state: 'Answered' };

module.exports = async function blocks(t) {
  const { p, S, chunks } = t;

  // 3. While the fence streams it is a typed skeleton, never the JSON; once
  // the message is final, the card Rust validated, framed by the harness
  // ("from <agent>"), from its own lazy chunk.
  await t.newChat('Ask');
  await t.type('ui: choices');
  await p.waitForSelector('.msg.assistant.streaming .ui-skeleton:has-text("Preparing question…")');
  // The reply goes on after the fence, so once that text shows (and the
  // message still streams) the whole JSON body has arrived: one snapshot.
  const streamed = await (await p.waitForFunction(() => {
    const md = document.querySelector('.msg.assistant.streaming .md');
    return md && md.innerText.includes('Tap the one') ? md.innerText : false;
  })).jsonValue();
  check(streamed.includes('Preparing question…'), 'skeleton while streaming: ' + streamed);
  check(!streamed.includes('"options"') && !streamed.includes('colour"'), 'no raw JSON while streaming: ' + streamed);
  check(chunks('UiBlock').length === 0, 'the card chunk waits for the final message');
  await t.idle();
  const choices = await t.lastMsg();
  await choices.locator('.ui-card .ui-choices').waitFor();
  check((await choices.locator('.ui-skeleton').count()) === 0, 'the skeleton gives way to the card');
  check((await choices.locator('.ui-from').innerText()) === 'Question from General', 'harness frame names the agent');
  check((await choices.locator('.ui-footer').innerText()) === 'Sends your answers as a message. Never type passwords or keys here.', 'fixed footer');
  check((await choices.locator('.ui-title').innerText()) === 'Which colour do you like best?', 'the question');
  const labels = await choices.locator('.ui-option-label').allInnerTexts();
  check(JSON.stringify(labels) === '["Blue","Green","Red"]', 'options: ' + JSON.stringify(labels));
  check((await choices.locator('.codeblock').count()) === 0, 'no code for a shown block');
  check(chunks('UiBlock').length === 1, 'card chunk loaded once');

  // A tap answers: the user's bubble names the option, the model gets the
  // canonical text of ids, and the card is answered and locked.
  const echo = await t.pick(choices, 'Blue');
  const bubble = await p.locator('.msg.user .bubble.reply').last().innerText();
  check(bubble.includes('Answered “Which colour do you like best?”') && bubble.includes('Blue'), 'reply bubble: ' + bubble);
  check((await echo.innerText()).includes('You said: Choice answer · colour: blue'), 'the model got the canonical answer');
  await eventually(() => choicesState(choices), ANSWERED, 'answered');
  await choices.screenshot({ path: `${S}/rich-5a-answered.png` });
  log('item 3: skeleton while streaming, then the card; a tap answers with ids and locks it');

  // 9. Stale: typing past a question closes it, and the node refuses a late
  // answer for it with 409.
  const stale = await t.send('ui: choices');
  await stale.locator('.ui-option:not([disabled])').first().waitFor();
  await t.send('green, please');
  await eventually(async () => ({ state: (await stale.locator('.ui-state').innerText()).startsWith('Not answered'), enabled: await stale.locator('.ui-option:not([disabled])').count() }),
    { state: true, enabled: 0 }, 'stale card');
  const late = await p.evaluate(async () => {
    const sid = location.hash.split('/').pop();
    const ev = await (await fetch(`/api/sessions/${sid}/events`)).json();
    const asked = ev.events.filter((e) => e.kind === 'assistant_message' && e.payload.blocks).pop();
    const r = await fetch(`/api/sessions/${sid}/prompt`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ text: '', reply: { message_id: asked.payload.message_id, block_id: 'colour', values: { selected: ['red'] } } }),
    });
    return { status: r.status, error: (await r.json()).error };
  });
  check(late.status === 409 && late.error === 'this form is no longer open', 'late answer: ' + JSON.stringify(late));
  log('item 9: prose closes the question ("Not answered"); a late answer gets', late.status);

  // 6. A block Rust rejected is its code plus the reason, an unknown type is
  // plain code, and a second question in one message is rejected while the
  // first shows.
  const bad = await t.send('ui: bad');
  await bad.locator('.ui-error').waitFor();
  check((await bad.locator('.codeblock .lang').innerText()) === 'ui:choices', 'rejected block shows as code');
  check((await bad.locator('.ui-error').innerText()) === 'Could not show this question: options[0].label: longer than 80 characters',
    'error note: ' + (await bad.locator('.ui-error').innerText()));
  check((await bad.locator('.ui-card').count()) === 0, 'no card for a rejected block');
  const unknown = await t.send('ui: unknown');
  await unknown.locator('.codeblock').waitFor();
  check((await unknown.locator('.codeblock .lang').innerText()) === 'ui:widget', 'unknown type is plain code');
  check((await unknown.locator('.ui-error, .ui-card, .ui-skeleton').count()) === 0, 'and no error for it');
  const two = await t.send('ui: two');
  await two.locator('.ui-error').waitFor();
  check((await two.locator('.ui-card').count()) === 1 && (await two.locator('.codeblock').count()) === 1, 'one card, one code block');
  check((await two.locator('.ui-error').innerText()) === 'Could not show this question: only one choices or form block per message', 'second question rejected');
  log('item 6: rejected block = code + reason, unknown type = code, one question per message');

  // 5. A form field that asks for a key is only the warning, with nothing
  // to type into; the other field of the form is an ordinary field.
  const secret = await t.send('ui: secret');
  await secret.locator('.ui-card .ui-secret').waitFor();
  const fields = secret.locator('.ui-fields > li');
  check((await fields.count()) === 2, 'two fields');
  check((await fields.nth(0).locator('.ui-secret').innerText()).startsWith('API key: This asks for a secret.'), 'secret note');
  check((await fields.nth(1).locator('.ui-secret').count()) === 0 && (await fields.nth(1).locator('.ui-field-label').innerText()) === 'Name', 'Name is an ordinary field');
  check((await secret.getByLabel('API key').count()) === 0, 'no input for the key');
  await secret.screenshot({ path: `${S}/rich-5c-secret.png` });
  await secret.getByLabel('Name', { exact: true }).fill('Ana');
  const named = await (await t.submit(secret)).innerText();
  check(named.includes('name: "Ana"') && named.includes('not shown (asks for a secret): key'), 'secret form answer: ' + named);
  log('item 5: no input for the key, Name answered');

  // 4. A form: Send waits for the required field, a value out of range is
  // an inline error, and the answer reaches the model as ids and values.
  const trip = await t.send('ui: form');
  await trip.locator('.ui-card .ui-form').waitFor();
  check((await trip.locator('.ui-from').innerText()) === 'Form from General', 'form header');
  check(await trip.locator('.ui-send').isDisabled(), 'Send off until Travellers is filled');
  const people = trip.getByLabel('Travellers', { exact: true });
  await people.fill('13');
  await trip.locator('.ui-field-error').waitFor();
  check((await trip.locator('.ui-field-error').innerText()) === 'Must be at most 12', 'inline error: ' + (await trip.locator('.ui-field-error').innerText()));
  check(await trip.locator('.ui-send').isDisabled(), 'Send off while a value is wrong');
  await people.fill('3');
  check((await trip.locator('.ui-field-error').count()) === 0, 'error gone');
  await trip.getByLabel('Lisbon').check();
  check(!(await trip.locator('.ui-send').isDisabled()), 'Send on once the form is valid');
  await p.screenshot({ path: `${S}/rich-5b-form.png`, fullPage: true });
  const tripEcho = await (await t.submit(trip)).innerText();
  check(tripEcho.includes('Form answer · trip') && tripEcho.includes('people: 3') && tripEcho.includes('dest: lis'), 'form answer: ' + tripEcho);
  const answers = await trip.locator('.ui-answers dd').allInnerTexts();
  check(JSON.stringify(answers) === '["3","Lisbon"]' && (await trip.locator('.ui-card').locator('input, textarea, select, button').count()) === 0,
    'answered form shows its values read-only: ' + JSON.stringify(answers));
  const tripBubble = await p.locator('.msg.user .bubble.reply').last().innerText();
  check(tripBubble.includes('Answered “Trip details”') && tripBubble.includes('Travellers: 3') && tripBubble.includes('Destination: Lisbon'), 'form bubble: ' + tripBubble);
  log('item 4: Send waits for Travellers, 13 is an inline error, the answer lists people: 3');
  await p.screenshot({ path: `${S}/rich-5-ui.png`, fullPage: true });

  // The stored blocks render the same after a reload: the answered card is
  // answered and locked (item 3), the stale one not answered.
  await p.reload();
  await p.locator('.msg.assistant .ui-card').first().waitFor();
  await eventually(async () => ({
    cards: await p.locator('.msg.assistant .ui-card').count(),
    errors: await p.locator('.msg.assistant .ui-error').count(),
    replies: await p.locator('.msg.user .bubble.reply').count(),
    first: await choicesState(p.locator('.msg.assistant .ui-card-choices').first()),
    stale: (await p.locator('.msg.assistant .ui-card-choices').nth(1).locator('.ui-state').innerText()).startsWith('Not answered'),
  }), { cards: 5, errors: 2, replies: 3, first: ANSWERED, stale: true }, 'after reload');
  log('ui cards after reload: answered and stale cards stay locked');
};

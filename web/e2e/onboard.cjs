// Helpers shared by the e2e scripts for the first-run screens (not a script).

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

module.exports = { createOwner, keywordsOnly };

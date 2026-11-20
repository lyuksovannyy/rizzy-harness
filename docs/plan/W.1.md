# W.1

## Covers

Web checks not runnable here

## Change

Node 22 (CI's version) and pnpm 12.17.0 through `corepack`, plus Playwright's browsers; steps in the README's development section.
Owner action.
Fallback: a manually triggered CI job running one named e2e script against the built binary.

## Done when, and size

The four `pnpm` checks pass locally on `integration/gate3`, and `node web/e2e/onboard.cjs target/debug/rizzy-harness` runs.
**S**


# W.7 (done)

## Covers

Result, 2026-11-12 (`fix/W.7-browser-scripts`)

## Change

All eleven failures were scripts that had fallen behind deliberate changes (the agent roster: the default Assistant has no shell; session folders outside the node's data directory; the in-page delete dialog; the lazy search panel; per-message footnote ids; private addresses refused unless listed).
No fault was found in the product.
New `web/e2e/login-key.cjs`: in Chromium the cookie without the key is 401, a page on another port of the same host reads nothing, the socket closes with 4401 without the key, and sign-out removes the key. `web/e2e/run.sh` runs one or all; the README has the setup.

## Done when, and size

All 16 scripts passed three times in a row for the author and once more in review.
Left over: **W.8**.


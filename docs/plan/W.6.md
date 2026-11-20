# W.6 **sec** (built, PR open)

## Covers

The approval takes the composer's place (the owner, 2026-11-12)

## Change

While a call waits for approval in the open chat, the composer's text field is replaced by the choice (the three buttons of R.6, with the card above them); the field comes back when the call is answered.
A browser notification is sent when the tab is hidden, after permission asked on a click, with fixed words and the chat's place in the list, never the command, a path or a title.
No button has focus when the choice appears, and keys are ignored for a short moment after it does, so a line being typed cannot answer it.
A draft being typed is kept.

## Done when, and size

Component tests, written first: Enter and Space typed as the choice appears approve nothing; the draft survives; the notification carries no call text; a second pending call is shown after the first is answered; a hidden character in the call is still marked (R.6's tests stay).
A browser script for the whole flow. `pnpm size` passes; the choice loads with the conversation chunk.
**Status:** built on `feat/approval-in-composer` (`ApprovalChoice.tsx`, one component for the composer and the card; a 700 ms key guard; one call at a time in the node's order; the notification was already built); `web/e2e/approval-in-composer.cjs` is written but not yet run against the real binary.
**M**


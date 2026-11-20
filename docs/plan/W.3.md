# W.3

## Covers

PR 4 follow-ups

## Change

The "for this turn" button, labelled as A.2 makes it ("this exact command, for this turn"), offered after the first approval; no reconnect after close code 4401; request id on errors; messages for the new 403, 422, 429 and 503. **Status (branch `feat/web-batch-2`):** done: no reconnect after 4401 (the shell reconnects only if `/api/me` still accepts the login), the request id on `HttpError` and shown on the generic error displays (delete, rename, retry, Settings save, account menu), plain sentences for a bare 403, 422, 429 (with Retry-After) and 503 (`lib/errorText.ts`).
Left: the "for this turn" button; the other displays (most call sites still show `message` without the id).

## Done when, and size

Component tests each; the approval e2e flow passes.
**M**


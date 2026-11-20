# A.25 **sec**

## Fixes

Reviews of PRs 130, 134 and 137: each found a node payload of the wrong shape (`images_withheld.calls` not a list, a background `child` not an object, `thinking` not an object) that crashed the TS fold or a chat row, fixed in one place each; no error boundary anywhere in `web/src` (41 `lazy(` screens and parts).
The review of PR 156 rendered every fuzzed state (2,816 renders, 12 failed): rows read tables and view maps through the prototype (a tool named `constructor`, which a model can send; a turn id `toString`; a status `__proto__`), enum values were unchecked, the table could lose a field unnoticed, and one bad `ui:` card cost the chat

## Change

**One guard where events enter the fold, checked against Rust's schema; own-key lookups; a boundary per row and per screen.** `web/src/fold.ts` checks every payload against a table of each kind's fields: a field serde requires that is missing or of the wrong type, or an enum value outside its list, drops the event into `unknown_events` (the Rust fold's rule) with one console line (kind and seq only); an optional field or array element of the wrong type is left out and the event still shows (deliberately unlike the Rust fold, which drops it: review of PR 137); unknown fields are left out. `crates/core/tests/browser_schema.rs` writes `web/src/__fixtures__/schema.json` from serde itself (every kind built in full by `tests/common`, required, enum values by exhaustive `match`), and fails when it is stale; `web/src/schema.test.ts` fails when the table and it differ.
Every table and view map read by a string from an event, a model, a node or the server goes through `own` (`web/src/emptyView.ts`); `ui:` card specs are checked by shape (`web/src/ui/shape.ts`), form drafts read own keys; `RowBoundary` around each conversation item (a waiting approval keeps only Deny), `ScreenBoundary` around the shell's screen and the app's root (a failed chunk says the app may have been updated, with Reload; never reloads by itself).
The fold left the first load (103.7 to 101.3 KB).
Rejected: a schema library (a dependency, and the bundle); a `try` around the fold alone (a throw halfway leaves a half-applied event); automatic reload (loops while an upgrade rolls out); approving a call whose row could not be drawn.

## Done when

**Done (branch `fix/web-fold-guard`):** tests `web/src/foldguard.test.ts` (every kind and field replaced by bad values: dropped with the state unchanged but the counter, or folded well typed; enum values; prototype keys; the three review crashes), `web/src/components/renderfuzz.test.tsx` (the reviewer's render fuzz: every fixture leaf replaced by prototype names and odd numbers, folded and rendered through the conversation, zero failures; the named findings), `web/src/schema.test.ts`, `crates/core/tests/browser_schema.rs`, `web/src/ownkeys.test.tsx` (each table with five prototype names), `web/src/ui/uishapes.test.tsx` (wrong shapes, every fixture spec, every spec leaf fuzzed), `web/src/components/rowboundary.test.tsx`, `screenboundary.test.tsx`, `shell.test.tsx`, `live.test.ts` (held events in order, a failed load).


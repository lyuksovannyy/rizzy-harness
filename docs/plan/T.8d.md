# T.8d **sec**

## Adds

Background commands (the owner, 2026-11-12).
**Design: `docs/background-commands.md` (C.15), which proposes changes to this row: the master starts callback turns, a background command must state its timeout, callback turns have their own step cap.
Step 1 (background commands without callbacks) is done and merged (2026-11-13, pull request 71; `cli/tests/background.rs`; reviewed in `docs/security-review/2026-11-T8d-background-commands.md`, T8D-1 to T8D-6 fixed; what differs from the note is in its §11).
Step 2 (the person's strip with Stop and Stop all, the owner-only stop route, and the default of 8, so the feature is on) is done on branch `feat/T.8d-step2-stop`, not merged and not yet reviewed (note §12; `master/tests/it/background_stop_http.rs`, `web/e2e/background.cjs`).
Step 3 (callback turns: the master's watcher and admission, `session.callback`, the node's budget `callback_turns` and cap `callback_max_steps`, `turn_started.cause`, `callback_refused`, the attention rows and the chat list's running mark) is done on branch `feat/T.8d-step3-callbacks`, not merged and not yet reviewed (note §13; `cli/tests/callbacks.rs`, `cli/tests/callbacks_real.rs`, `master/tests/it/callbacks_http.rs`, `web/e2e/callbacks.cjs`).** `bash` may start a command in the background: the node tracks it, records its end as an event and starts a turn that tells the agent ("call the agent back when done").
Every command states a timeout, 60 s when none is given, never above the node's `bash_timeout_max_s`.
Bounds from `node.toml`: how many run at once per session, how many callbacks a session may take without a person's message, output kept per command.
A background command is its own bubblewrap sandbox the node holds, so T.8a's kill covers it.

## Safeguards and abuse tests, written first

Tests first: the T.8a escapers are gone 2 s after a background command's cancel, its timeout and the session's end; none survives `kill -9` of the node; a callback arrives once, in the owning session, and counts against the step limit; an agent cannot raise its own bounds; a callback into a deleted chat starts nothing.

## Invariants

Light process; one writer (the session actor appends the end event); node decides

## Podman / Node

No / No

## Depends

T.8a

## Size

**L**


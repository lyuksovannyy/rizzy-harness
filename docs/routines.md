# Routines

Status: phase A built (schedules, Run now, runs, results, the API). The webhook source, untrusted
runs and run-chat retention come in phase B; they are described here as designed and marked
**[B]**. The contract this follows is the sidebar areas contract (§8).

## What a routine is

A routine is a saved instruction: "on this schedule (or when this webhook is called **[B]**),
start agent A with prompt P in workspace W, as me". It has:

- **one workspace**, which never changes, and **one owner**; it is private to its owner (T10).
  Nobody else sees it, server admins included. The API token reads every routine but owns none;
- **what runs** (`RoutineSpec`): a chat or an agentic session, the agent, optionally a model,
  effort, a project, and for agentic work the node, folder and approval mode;
- **triggers**: either 1 to 8 **schedules**, or exactly **one webhook** **[B]**. Mixing them is
  refused ("A routine runs on a schedule or on one event, not both"), so a prompt always knows
  which variables it has. **Run now** is always available, also while the routine is disabled;
- **runs**: each fire is a run, and each run starts **a new chat** owned by the routine's owner in
  W, titled `<routine name> · YYYY-MM-DD HH:MM` in the owner's time zone (never event data).

Deleting an account or a workspace deletes its routines; the chats their runs started stay.

## Schedules (`crates/master/src/routines/schedule.rs`)

| type | config | notes |
|---|---|---|
| `cron` | `{type, expr: "30 8 * * 1-5", tz}` | exactly 5 fields, minute resolution; seconds, years and `@` nicknames are refused |
| `interval` | `{type, every_minutes, anchor_ms}` | runs at `anchor + k·every`, so it never drifts; the anchor defaults to the minute it was saved |
| `once` | `{type, at: "2026-11-10T09:00", tz}` | a local date and time; a time already past simply never runs |

- The time zone is stored per trigger and defaults to the owner's preference (else UTC). Changing
  the preference later never moves a saved schedule.
- Cron is evaluated by **croner 4.0.2** (MIT, `default-features = false, features = ["jiff"]`)
  in jiff time zones; interval and once are plain jiff. Every kind answers one question,
  `next_after(t)`, so nothing else looks inside a schedule.
- A time skipped by a spring-forward gap fires once, at the first valid instant after the gap
  (03:30 on 28 March 2027 in Riga runs at 04:00 EEST). A time repeated when clocks go back fires
  once, the first time.
- A schedule may not run more often than every `min_interval_minutes` (5): cron is checked over
  its next 50 occurrences.
- **Missed runs.** An occurrence is missed when it is more than its grace late: half the gap to
  the next occurrence, at most an hour. With `catch_up = skip` (the default) all the occurrences
  that went by become one `missed` run with `repeats`; with `once`, one late run. Either way the
  trigger moves past now: there are never several runs.

## Prompt templates (`template.rs`)

`{{ path }}`, hand-rolled. A path is dot-separated segments (`[A-Za-z_][A-Za-z0-9_-]{0,63}` or a
decimal index, at most 8); `\{{` writes a literal `{{`. Unknown namespaces or fields, a bad path or
an unterminated `{{` are refused at save with the offset (UTF-16 code units, as a textarea counts).

| namespace | fields |
|---|---|
| `now` | `date`, `time`, `datetime`, `weekday`, `iso`, in the trigger's zone (Run now and intervals: the owner's) |
| `routine` | `name`, `id` |
| `run` | `trigger` (`schedule`, `webhook`, `manual`), `number`, `scheduled` |
| `user` | `name` (display name, else username) |
| `workspace` | `name` |
| `last_run` | `status`, `at` (empty for the first run) |
| `event` **[B]** | webhook routines only: `event.body.<path>`, `event.text`, `event.received_at`, `event.content_type` |

Rendering is a single pass: an inserted value is never scanned again, so `{{user.name}}` inside a
webhook body stays literal. Values lose control characters (but newlines and tabs) and bidi
controls; numbers and booleans are JSON text, null is empty, objects and arrays compact JSON. A
value is cut at 4,000 characters, the prompt at 32,000 (event values are cut first; still too long
and the run fails). A variable without a value renders empty with a warning on the run, or skips
the run (`on_missing = skip`). The editor never renders templates itself: `POST
/api/routines/preview` does.

## Running (`runner.rs`)

All routine time comes from one clock in the scheduler's handle (`Clock::wall()` in production,
`Clock::manual` in tests), read by the scheduler, the executor, the timeout sweep, `hourly` and
`now.*`. Tests start routines with `routines::start_with(&master, clock, false)` and drive
`tick` and `sweep_timeouts` themselves.

- **Scheduler.** Every at most 60 s, or when a routine is saved or run, it takes the due triggers
  of enabled, unpaused routines. Each occurrence is **claimed once**: one store transaction moves
  the trigger from the occurrence it saw to the next one and records the run, only if the trigger
  still stood there. A double fire is impossible.
- **Overlap** is always "skip" in v1: while a run is pending, running or waiting, a new fire is
  recorded as `skipped`, and consecutive skips fold into one row (`repeats`).
- **Admission, at every fire.** The owner's account must exist, be enabled and not owe a password
  change; they must be a member of W with the `routines` permission, and for an agentic routine
  hold the account's agentic grant and the `agentic` permission; the agent, model and node the
  spec names must still exist. **Any refusal** records a `blocked` run with the reason, pauses the
  routine (`paused_reason = blocked`), writes an audit line and shows in the owner's attention
  list. Blocked runs are never retried.
- **Caps at run time.** More than `max_concurrent_per_user` of the owner's runs going → `skipped`.
  Over `max_runs_per_user_per_day` → `skipped`, and the routine is paused with `cap` until the
  day's window has room again (that one pause is the day's attention item).
- **Execution.** Render the prompt; take one of `max_concurrent_runs` places (a run waiting more
  than 10 minutes fails "server busy"); start the chat as the owner; store the chat on the run
  (`running`) before prompting; prompt with the routine's own agent (it beats a leading
  @mention), the owner's model context and a note "Started by routine ‹name› (‹trigger›) at
  ‹time›, not typed by the user just now". The context goes after the history, so the system
  prompt stays cache-stable.
- **Failures.** A node that is offline (or does not acknowledge within 15 s) before the turn
  starts is retried after 1, 5 and 15 minutes, never past the trigger's next occurrence; then the
  run `failed`. An unknown agent or a node refusal blocks the routine. A turn that ends with an
  error is never retried (agentic side effects). Five failures in a row (`error`, `failed`,
  `timed_out`, `interrupted`) pause the routine (`failures`); a completed run resets the count.
- **Watcher.** It subscribes to the master's live events before reconciling, so nothing applied
  in between is missed. A run's turn is the first turn of the chat it started: `turn_started`
  binds it, `approval_requested` parks it as `waiting` (also when the approval is in a sub-agent
  session the chat delegated to: found through the session's `parent_id`), the resolution of the
  last pending approval makes it `running` again, and its `turn_ended` finishes it with its usage.
  Later turns, the owner continuing the chat, are not the run's. When the watcher falls behind it
  reconciles every active run from the mirror.
- **Timeouts** (every 60 s): a run waiting for an approval longer than `approval_timeout_hours`
  (24), or running longer than its `max_runtime_min` (counted from when it last started or
  resumed running), is cancelled and recorded as `timed_out`.
- **Restart.** `pending` runs become `interrupted` and are never retried (turns are not resumed,
  PLAN §2). `running` and `waiting` runs are followed again, or finished from their chat's first
  `turn_ended`.
- **Losing access mid-run.** Agentic turns are cancelled by the existing revocation sweep and the
  run records `cancelled`; a chat-profile run finishes its current turn. Either way the next fire
  is blocked and pauses the routine.

## Approvals in agentic routines

Agentic routines run in `ask` or `ask_all` only (the node clamps further, T5); the API refuses
`auto` even where node.toml allows it. **Nothing is approved without a click.** Read-only tools run
unattended, so review-and-report routines finish on their own; `bash`, `edit`, `write` and
`publish` park the run. A parked run is an ordinary chat with a pending approval: it shows in
"Waiting for you" with the usual notification, the owner approves it through the normal
endpoint, and after 24 hours it is cancelled. `allow_session` lasts one run (each run is a new
chat).

**The one place a run's approval mode is chosen** is `runner::approval_for(spec, untrusted,
node)`, which returns the spec's `ask` or `ask_all`. It is the seam for trusted routines (below).

## Untrusted runs **[B]**

A run is untrusted when any `event.*` value went into its prompt (a webhook call, or Run now with a
sample event). Such a run, **and every later turn in its chat**, gets no memory (no recall, no
project facts, no `remember`; the project brief and board stay), no `web_fetch` or `web_search`
(on the agent and every delegate), the routine's own agent, a fixed title, a new chat and a note
that parts of the conversation come from an external event. The flag sticks to the chat:
`routines::on_owner_prompt`, called before every prompt the owner sends, returns the run's flag and
the turn runs untrusted (`PromptOpts.untrusted`, built in the master). It fails closed: if the run
cannot be read, the turn is untrusted.

The flag lives on the run row, which is never purged while its chat exists (see Retention). A
routine's deletion cascades its runs; phase B keeps untrusted chats untrusted past that (a marker
in `kv` written with the run, or the chat deleted with the routine).

## Results, Recent and attention (`results.rs`)

- Every chat row (`GET /api/sessions`, `GET /api/sessions/{id}`) carries `routine: RoutineMark |
  null`: `{routine_id, name, run_id, status, seen, promoted, untrusted}`.
- **The chat list leaves out run chats that are seen, not continued and not going** (pending,
  running or waiting) unless asked with `?routine_runs=1`. So Recent shows a run chat while it
  runs, waits or has an unseen result, and keeps it once the owner prompts in it (`promoted`).
- **Seen:** `POST /api/routines/{id}/runs/{run_id}/seen`, idempotent, marks a **finished** run seen
  (a run still going has no result to see yet; the call is accepted and changes nothing).
- `GET /api/attention` gains `routines`: per routine one row for its newest unseen finished run
  (`kind = result` for completed, `failed` for error, failed, timed_out, interrupted, cancelled or
  blocked, with `unseen` counting them), and one `paused` row while the routine is paused. Skipped
  and missed runs never appear. A run waiting for an approval appears in `items` like any chat.
  All of the caller's workspaces, newest 20.
- Every run state change sends `routines_changed` to the owner's sockets only.

## Retention

Hourly (`routines::hourly`, from the master's one maintenance loop): runs beyond the newest
`keep_runs` (200) of each routine, and runs older than 90 days, are forgotten, **except runs whose
chat still exists** (the chat's mark and untrusted flag live on its run) and runs still going.
**[B]** Run-chat retention: per routine the newest `keep_run_chats` (30) seen, uncontinued run
chats are kept and older ones deleted (at most 100 an hour server-wide). Deleting a run's chat, by
the owner or by retention, clears the run's rendered prompt, event and warnings; status, times,
trigger and usage stay.

## Permissions and audit

| action | who |
|---|---|
| see a routine and its runs | its owner; the API token |
| create, edit, Run now (and rotate a secret **[B]**) | its owner, holding `routines` in W now (+ the agentic grant and `agentic` for an agentic routine) |
| delete | its owner, always (so an owner who lost the permission can clear routines away) |
| anything on someone else's routine | 404 |

Guests have no `routines` permission. `GET /api/features?workspace=` shows the Routines entry
(`routines.show`) to whoever may own routines there, and to an owner who still has routines there
they may no longer run; `why` explains the rest ("Guests can't run routines in Acme").

Audit kinds: `routine.created`, `routine.updated`, `routine.deleted`, `routine.paused`,
`routine.run`, `routine.blocked` (and `hook.secret_rotated`, `hook.rejected` **[B]**). Lines carry
ids and reasons, never prompt text.

## Config: `[routines]` in config.toml

| key | default |
|---|---|
| `enabled` | true (off: nothing fires and nothing new is saved) |
| `webhooks` | true |
| `min_interval_minutes` | 5 |
| `max_per_user` / `max_per_workspace` | 25 / 100 (409 at save) |
| `max_triggers` | 8 |
| `max_concurrent_runs` / `max_concurrent_per_user` | 4 / 2 |
| `max_runs_per_user_per_day` | 200 |
| `hook_rate_per_minute` | 10 **[B]** |
| `keep_runs` | 200 per routine (and none older than 90 days) |
| `keep_run_chats` | 30 **[B]** |
| `approval_timeout_hours` | 24 |

## API (`crates/master/src/http/routines.rs`)

Errors are `{error}`; a prompt error also has `offset`. 400 validation, 403 permission (with the
reason), 404 not the owner, 409 a cap, a name clash, a paused routine or routines turned off.

| method and path | answer |
|---|---|
| `GET /api/routines?workspace=` | `{routines, workspace_id, can_create, can_agentic, why, limits, variables, sources, orphaned}` (`sources` is empty until webhooks land) |
| `POST /api/routines` `{workspace_id?, name, enabled?, spec, triggers:[{kind, config}]}` | 201 `{routine, secrets}` |
| `GET /api/routines/{id}` | `Routine` |
| `PATCH /api/routines/{id}` `{name?, enabled?, spec?, triggers?}` | `{routine, secrets}`; a trigger with its `trigger_id` keeps it; enabling clears a pause and starts the schedules from now |
| `DELETE /api/routines/{id}` | `{ok}` |
| `POST /api/routines/{id}/run` | 202 `{run_id}` or 202 `{skipped: "overlap" \| "cap"}`; 409 while paused |
| `GET /api/routines/{id}/runs?before=&limit=50` | `{runs, next}` (newest first; `next` is the `before` of the next page) |
| `GET /api/routines/{id}/runs/{run_id}` | `Run` with `prompt` and `event` |
| `POST /api/routines/{id}/runs/{run_id}/seen` | `{ok}` |
| `POST /api/routines/preview` `{workspace_id?, routine_id?, prompt?, triggers?, sample_event?}` | `{next_runs, chats_per_day, rendered, untrusted, warnings, errors:[{offset, message}], event_paths}` |

A `Routine` is `{routine_id, workspace_id, name, enabled, paused_reason, spec, triggers, next_run_ms,
last_run, unseen, blocked, event_paths, created_ms, updated_ms}`; `blocked` is a live check of why
the owner may not run it now. A `Run` is `{run_id, routine_id, trigger_kind, scheduled_ms,
started_ms, ended_ms, status, reason, session_id, chat_deleted, usage, warnings, untrusted, attempt,
repeats, seen, answer_preview}`; `answer_preview` is the first sentence of the chat's latest
message. `chats_per_day` counts the schedules' runs over the next week, divided by seven.

Phase A answers 400 "webhooks are not available yet" for a webhook trigger, and for a `sample_event`
on Run now; the preview ignores `sample_event`.

## Later

**Trusted routines** (the seam only). A per-routine allowlist of tools that run without a click
(`RoutineSpec.trusted_tools`, refused today as an unknown field), honoured only on nodes whose
node.toml allows `auto`, enforced by the node (T5), never for untrusted runs or any later turn of
an untrusted chat. It plugs into `runner::approval_for` on the master, an additive
`session.create.preapproved` behind a new node capability, and the node's `approve()`.

**Moved out of v1:** internal event sources (`chat.finished`, `node.offline`/`online`,
`artifact.ready`) behind an internal bus, each one more `impl EventSource`; `same_chat` output;
`overlap = queue`; `last_run.answer` (which would count as untrusted after an untrusted run); an
opt-in for web tools with event data; signed webhooks and pull sources.

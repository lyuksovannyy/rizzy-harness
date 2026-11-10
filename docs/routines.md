# Routines

Status: phase C built (phase A's schedules, Run now, runs, results and API; phase B's webhook
source, untrusted runs, sample events and run-chat retention; phase C's hardening: the untrusted
mark on delegates' chats, retention under load, the approval seam). The contract this follows is
the sidebar areas contract (§8).

## What a routine is

A routine is a saved instruction: "on this schedule (or when this webhook is called), start agent
A with prompt P in workspace W, as me". It has:

- **one workspace**, which never changes, and **one owner**; it is private to its owner (T10).
  Nobody else sees it, server admins included. The API token reads every routine but owns none;
- **what runs** (`RoutineSpec`): a chat or an agentic session, the agent, optionally a model,
  effort, a project, and for agentic work the node, folder and approval mode;
- **triggers**: either 1 to 8 **schedules**, or exactly **one webhook**. Mixing them is
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
| `event` | webhook routines only: `event.body.<path>` (a JSON body), `event.text` (a text body), `event.received_at` (the owner's zone), `event.content_type` |

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
  hold the account's agentic grant and the `agentic` permission, and be allowed on its node (the
  embedded `local` node is for server admins unless `[nodes] local_node_users = "granted"`);
  the agent, model and node the
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

### The approval seam (`runner::approval_for`)

**The one place a run's approval mode is chosen** is `runner::approval_for(spec, untrusted,
node)`. In v1 it returns the spec's `ask` or `ask_all` and ignores the other two arguments; they
are there for trusted routines (Later), which plug in here and nowhere else on the master. It is
called once per run, when the run's chat is created; the owner's later turns in that chat never go
through it.

What a trusted mode would have to change:

- **The spec**: a new `RoutineSpec.trusted_tools: string[]` (today an unknown field, refused with
  400), checked at save against the agent's tools, and offered only for schedule routines.
- **`approval_for`** returns the mode plus that allowlist, and the allowlist only when the run is
  not `untrusted` and `node` announces a new capability `preapproved_tools` (`Master::node_has`).
- **The chat**: `CreateSpec` and the protocol's `session.create` gain an additive `preapproved`
  list, sent only to a node with that capability.
- **The node**: its `approve()` allows a listed tool without asking only if node.toml allows
  unattended approval (`max_approval_mode = "auto"`), records `resolved_by: "policy"`, and drops
  the list when the run's turn ends, so the owner's own turns there ask as usual. A delegate's chat
  asks as today unless the protocol says otherwise.
- **The UI**: a "Run these tools without asking" list in Advanced, for schedule routines on capable
  nodes.

What must stay:

- **Untrusted stays untrusted.** A run with any `event.*` in its prompt (every webhook routine that
  uses its event, and Run now with a sample event) is untrusted, gets no allowlist, and its chat is
  marked for good: every later turn there, a delegate's chat under it, and those chats after the
  routine, its runs or the parent chat are gone. Trust is never a property of a webhook.
- `auto` stays refused at save; `approval_mode` stays `ask`/`ask_all`, and the node still clamps it
  (T5): the node, not the master, decides whether anything runs unattended.
- Everything not on the list still parks the run in "Waiting for you" and times out after 24 hours.
- Access is re-checked at every fire (account, membership, `routines`, and for agentic routines the
  grant and `agentic`); a refusal blocks and pauses the routine as today.
- Untrusted runs keep their protections whatever the approval mode: no memory, no `web_fetch` or
  `web_search`, the routine's own agent, the note.

## Webhooks (`crates/master/src/http/hooks.rs`)

`POST /api/hooks/{trigger_id}` with `Authorization: Bearer <secret>` fires a webhook routine.
The routine's "When" card shows the line to try it with (the page's origin plus the trigger's
`url_path`; the secret inline right after a save or a rotation, `$HOOK_SECRET` later), and it
works as shown:

```sh
curl -X POST 'http://127.0.0.1:7777/api/hooks/<trigger_id>' \
  -H "Authorization: Bearer $HOOK_SECRET" \
  -H 'Content-Type: application/json' \
  -d '{"text":"hello"}'
# 202 {"run_id":"…"}; a prompt "Got {{event.body.text}}" renders as "Got hello".
```

- **The secret** is 256 bits of random hex, shown once (in the create or edit answer under
  `secrets`, or by rotation), stored only as its SHA-256 and compared in constant time. It is
  never read from the URL; the trigger id in the path is not a credential. The API token is not
  accepted here. `auth::guard` applies (Host allowlist, Origin when present).
- **The body**: at most 64 KiB. `application/json` becomes `event.body` (nested at most 32 deep),
  `text/plain` becomes `event.text`; anything else is 415. Headers and the query string never
  reach the event.
- **The order of checks is fixed**, so nothing about a routine shows before the secret is proven
  and no throttle lets someone without the secret lock out the sender who has it
  (`routines/buckets.rs`, token buckets on the routine clock):

  | step | check | refusal |
  |---|---|---|
  | 1 | the body limit (by the extractor) | 413 |
  | 2 | the trigger id; an unknown one is still compared, against a dummy hash, then counted in the server-wide **unknown-id bucket** (100 per 15 min, unknown ids only) | 429 when empty, else 401 |
  | 3 | the secret; a wrong one is counted in the trigger's **bad-secret bucket** (20 per 15 min, failing calls only: the right secret is always checked and never refused by it) | 429 when empty, else the same 401 |
  | 4 | the trigger's **call rate** (`hook_rate_per_minute`, 10, burst 5), after the secret | 429 |
  | 5 | the routine disabled or paused, or webhooks off on the server | 409 |
  | 6 | the content type and body | 415, 400 |
  | 7 | the `Idempotency-Key` (at most 200 characters, kept as its SHA-256 for 24 h) | a repeat answers 202 `{run_id, duplicate: true}` with the first call's run |
  | 8 | overlap and caps | 202 `{skipped: "overlap" \| "cap"}`, else 202 `{run_id}` |

  Every 429 has `Retry-After`. An unknown id and a wrong secret get the same 401 body. A key is
  remembered once its call started a run; a call skipped for overlap left nothing to repeat, so a
  retry of it is a new call.
- **Audit**: the run's `routine.run` line carries the body's size and SHA-256, never the body;
  refusals write `hook.rejected` at most once per trigger (and once for unknown ids) every 15
  minutes, with the count.
- **The run** keeps a copy of the event (`routine_runs.event`, at most 16 KiB: long strings are
  cut first, then the body is dropped and the copy marked `truncated`), which the Runs tab shows
  and from which `event_paths` come; the prompt renders from the whole event, held in memory until
  the run starts (also across its offline retries).
- **Reachability**: the master binds loopback by default (T1), so hooks come from local processes,
  over WireGuard, or through a TLS proxy.

A new event source is one more `impl EventSource` (`events.rs`: its kind, its fixed variables, a
free prefix such as `event.body`, who may use it, its trigger config) plus its trigger kind; the
template check, the picker (`sources` in the list) and the preview take their `event.*` from it.

## Untrusted runs

A run is untrusted when any `event.*` value went into its prompt (a webhook call, or Run now with a
sample event). Such a run, **and every later turn in its chat**, gets no memory (no recall, no
project facts, no `remember`; the project brief and board stay), no `web_fetch` or `web_search`
(on the agent and every delegate), the routine's own agent (it beats an @mention in the event), a
fixed title, a new chat and a note that parts of the conversation come from an external event.
Event data never reaches tool arguments, the folder, node, model, agent or title. Agentic runs stay
`ask`/`ask_all`.

**The flag sticks to the chat.** When a run turns untrusted, the same store transaction marks its
chat in `routine_untrusted_chats` (a table added in phase B; the frozen tables are unchanged).
`routines::on_owner_prompt`, called before every prompt the owner sends, answers from that mark
(`Store::chat_untrusted`), so every later turn runs untrusted (`PromptOpts.untrusted`, built in
the master). The mark outlives the run: deleting the routine, or its workspace, cascades the runs
but not the mark. Only deleting the chat removes it. It fails closed: if the mark cannot be read,
the turn is untrusted. The chat's banner comes from `SessionRow.routine.untrusted`, so it shows
while the routine exists; once the routine is deleted the chat has no routine mark, but its turns
stay untrusted.

**Delegates' chats.** A sub-agent the run delegates to works in a chat of its own, which belongs to
the same owner and holds the task the parent wrote from the event. The owner can open and prompt
it directly, so it counts as untrusted when its parent is marked (delegation is one level deep, so
one hop up is the whole chain). Deleting the parent chat, by the owner or by retention, leaves the
delegates' chats behind; they take the mark over in the same transaction.

**Every way a chat goes on keeps it.** The mark belongs to the chat, not to the run, the routine,
the agent, the project or the workspace, and every turn the owner starts passes through
`on_owner_prompt` (the runner and the HTTP prompt handler are the only callers of
`Master::prompt`). So
undoing a turn (`/turns/{turn}/restore`), choosing a parallel answer (`/variants`), switching the
chat's agent (`/agent`), moving the chat into a project (its brief and board, never its facts) or
out of it, turning the routine off or deleting it, and the owner leaving the routine's workspace
(the chat falls back to their Personal workspace, where their memory is on) all leave it
untrusted. `hooks_http.rs` tests each of these over HTTP. Code that acts on a chat outside a
prompt (filing a `remember` call, say) asks `routines::chat_untrusted`, which fails closed.

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

Hourly (`routines::hourly`, from the master's one maintenance loop), in this order:

1. **Run-chat retention**: per routine the newest `keep_run_chats` (30) run chats the owner has
   seen and not continued are kept; older ones are deleted with `Master::delete_session`, as the
   owner would (at most 100 an hour server-wide, oldest first). Chats of runs still going, unseen
   or continued are never deleted.
2. **Runs**: runs beyond the newest `keep_runs` (200) of each routine, and runs older than 90 days,
   are forgotten, **except runs whose chat still exists** (the chat's routine mark lives on its
   run) and runs still going.
3. Expired `Idempotency-Key`s, and webhook throttles that are full again.

Deleting a run's chat, by the owner or by retention, clears the run's rendered prompt, event and
warnings, and the chat's untrusted mark (its delegates' chats keep one of their own); status,
times, trigger and usage stay. Because retention goes first, a run older than 90 days whose chat
it just deleted is forgotten in the same hour. A backlog drains at 100 chats an hour, oldest first
across routines (`retention_drains_a_backlog_at_100_chats_an_hour`).

## Permissions and audit

| action | who |
|---|---|
| see a routine and its runs | its owner; the API token |
| create, edit, Run now, rotate a secret | its owner, holding `routines` in W now (+ the agentic grant, `agentic` and the right to its node for an agentic routine) |
| delete | its owner, always (so an owner who lost the permission can clear routines away) |
| anything on someone else's routine | 404 |

Guests have no `routines` permission. `GET /api/features?workspace=` shows the Routines entry
(`routines.show`) to whoever may own routines there, and to an owner who still has routines there
they may no longer run; `why` explains the rest ("Guests can't run routines in Acme").

Audit kinds: `routine.created`, `routine.updated`, `routine.deleted`, `routine.paused`,
`routine.run`, `routine.blocked`, `hook.secret_rotated`, `hook.rejected` (throttled). Lines carry
ids, reasons, sizes and hashes, never prompt text, event content or a secret. Tracing lines carry
ids and store errors only. `webhook_content_never_reaches_logs_or_audit` holds both to that: it
sends marked bodies, headers, a query string, an `Idempotency-Key` and wrong secrets through every
webhook answer (401, 413, 415, 400, 409, 429, a duplicate, overlap, a skipped run, a run that went
through) and finds none of them in the audit rows or in any log line (every level of the harness
targets, info and up of the rest).

## Config: `[routines]` in config.toml

| key | default |
|---|---|
| `enabled` | true (off: nothing fires and nothing new is saved) |
| `webhooks` | true (off: no webhook trigger can be saved and hooks answer 409) |
| `min_interval_minutes` | 5 |
| `max_per_user` / `max_per_workspace` | 25 / 100 (409 at save) |
| `max_triggers` | 8 |
| `max_concurrent_runs` / `max_concurrent_per_user` | 4 / 2 |
| `max_runs_per_user_per_day` | 200 |
| `hook_rate_per_minute` | 10 (burst 5) |
| `keep_runs` | 200 per routine (and none older than 90 days) |
| `keep_run_chats` | 30 |
| `approval_timeout_hours` | 24 |

## API (`crates/master/src/http/routines.rs`)

Errors are `{error}`; a prompt error also has `offset`. 400 validation, 403 permission (with the
reason), 404 not the owner, 409 a cap, a name clash, a paused routine or routines turned off.

| method and path | answer |
|---|---|
| `GET /api/routines?workspace=` | `{routines, workspace_id, can_create, can_agentic, why, limits, variables, sources, orphaned}`; `sources` is `[{kind: "webhook", vars, free_prefix: "event.body"}]` while webhooks are on |
| `POST /api/routines` `{workspace_id?, name, enabled?, spec, triggers:[{kind, config}]}` | 201 `{routine, secrets}`; `secrets` maps each new webhook trigger's id to its secret, shown this once |
| `GET /api/routines/{id}` | `Routine` |
| `PATCH /api/routines/{id}` `{name?, enabled?, spec?, triggers?}` | `{routine, secrets}`; a trigger with its `trigger_id` keeps it (a webhook its URL and secret; its kind cannot change); a new webhook trigger gets a secret; enabling clears a pause and starts the schedules from now |
| `DELETE /api/routines/{id}` | `{ok}` |
| `POST /api/routines/{id}/run` `{event?}` | 202 `{run_id}` or 202 `{skipped: "overlap" \| "cap"}`; 409 while paused. `event` is a sample event, webhook routines only |
| `GET /api/routines/{id}/runs?before=&limit=50` | `{runs, next}` (newest first; `next` is the `before` of the next page) |
| `GET /api/routines/{id}/runs/{run_id}` | `Run` with `prompt` and `event` |
| `POST /api/routines/{id}/runs/{run_id}/seen` | `{ok}` |
| `POST /api/routines/{id}/triggers/{tid}/secret` | `{secret}`: a new secret for a webhook trigger; the old one stops at once (400 for a schedule) |
| `POST /api/routines/preview` `{workspace_id?, routine_id?, prompt?, triggers?, sample_event?}` | `{next_runs, chats_per_day, rendered, untrusted, warnings, errors:[{offset, message}], event_paths}` |
| `POST /api/hooks/{tid}` | see Webhooks |

A `Routine` is `{routine_id, workspace_id, name, enabled, paused_reason, spec, triggers, next_run_ms,
last_run, unseen, blocked, event_paths, created_ms, updated_ms}`; `blocked` is a live check of why
the owner may not run it now. A `Run` is `{run_id, routine_id, trigger_kind, scheduled_ms,
started_ms, ended_ms, status, reason, session_id, chat_deleted, usage, warnings, untrusted, attempt,
repeats, seen, answer_preview}`; `answer_preview` is the first sentence of the chat's latest
message. `chats_per_day` counts the schedules' runs over the next week, divided by seven.

A webhook trigger also has `url_path` (`/api/hooks/<trigger_id>`) and `secret_set`, never the
secret. `event_paths` (the owner's view only) are the template paths of the newest event a run of
the routine kept: `event.text` for a text body, else `event.body.<path>` for the body's values
(an array by its first item, keys a template cannot name left out), at most 100.

**Sample events** (`sample_event` in the preview, `event` on Run now) stand in for a webhook call: a
JSON string is a text body (`event.text`), anything else a JSON body (`event.body`), at most 64 KiB
and 32 deep, received now. The preview renders with it and reports `untrusted` and its
`event_paths`; a Run now with one is untrusted as soon as the prompt uses it.

## Later

**Trusted routines** (the seam only). A per-routine allowlist of tools that run without a click,
honoured only on nodes whose node.toml allows `auto`, enforced by the node (T5), never for
untrusted runs or any later turn of an untrusted chat. What it would change, and what must stay,
is under "The approval seam" above.

**Moved out of v1:** internal event sources (`chat.finished`, `node.offline`/`online`,
`artifact.ready`) behind an internal bus, each one more `impl EventSource`; `same_chat` output;
`overlap = queue`; `last_run.answer` (which would count as untrusted after an untrusted run); an
opt-in for web tools with event data; signed webhooks and pull sources.

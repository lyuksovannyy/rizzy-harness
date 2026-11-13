# Background commands: design note

Status: **the six open questions were answered by the owner on 2026-11-13 (§0)**; where §0
differs from a recommendation further down, §0 holds, and the affected rows are corrected. The
rest of the decisions table (§3) stands as recommended. **Step 1 of §8 is built** (merged in pull
request 71), reviewed (`docs/security-review/2026-11-T8d-background-commands.md`) and fixed; what
the code does differently from this note is in §11. **Step 2 is built** and merged (pull request
73): the person's strip and stop, and the feature **on by default** (§12). **Step 3 (callback
turns) is built** on branch `feat/T.8d-step3-callbacks`, not merged and not yet reviewed; every
difference from §4 is in §13. Task C.15 of `ADOPTION_PLAN.md`; the feature is task T.8d.

**Summary.** `bash` gains `background: true`. The node starts the command in its own contained
sandbox (T.8a to T.8c) under its own guard (T.8e), exactly as a foreground command, but holds it
in a per-session table instead of in the call. The call returns after at most 3 s with an id and
the first output. When the command ends (exit, timeout, stop), the session actor records
`background_ended` with the reason and the last 4 KiB of output. If a turn is running, the agent
sees it as a note at its next step. If the session is idle, **the master** starts a callback turn
after the same admission checks a routine passes, and only while the session's budget of turns
without a person's message lasts. Every command has a timeout: at least 5 s, never above
`bash_timeout_max_s`; a foreground command that states none gets 60 s, a background command must
state it and is refused without. Everything dies with the node,
the session's deletion, the owner losing access, or its own timeout. Bounds come from `[limits]`
in `node.toml` only.

Claims about code were read on `main` at `6e99313` (T.8c merged; pull request 63 not read).
"The others" are read from the project's research and the cloned repositories under
`.claude/research-repos/`, as data; anything not found there says "not verified".

## 0. Decided by the owner (2026-11-13)

| # | Question (§10) | Answer | What it changes below |
|---|---|---|---|
| Q1 | Does Stop of a turn stop its background commands? | No, as recommended; **and each background command can be stopped on its own**, by the person and by the agent. | Nothing: `bash_stop { id }` for the agent, Stop per command and Stop all for the person (§4.7, §4.11). |
| Q2 | Callback turns in `auto` sessions without a click? | Yes, as recommended, within `callback_turns` and `callback_max_steps`. | Nothing. |
| Q3 | The timeout and its ceiling | **The agent may set it from 5 s up to the node's existing ceiling** (`bash_timeout_max_s`); 60 s for a foreground command that states none. No second ceiling. | R3 and §4.2: a floor of 5 s is new (it was 1). |
| Q4 | The foreground default | **The range and the clamp are shared by both kinds; the 60 s default is foreground's only** (Q5). | R3 and §4.2: the foreground default drops from 600 s to 60 s, as recommended. |
| Q5 | Must a background command name its timeout? | **Yes: it must name its timeout, and is refused without** (decided by the owner on 2026-11-13, after a first reading of Q3 and Q4 as one 60 s default for both kinds). | R3 and §4.2: a background call without `timeout_secs` is refused before anything starts, with the range in words. |
| Q6 | On by default, and the default bounds | On. **`background_per_session = 8`, `background_per_node = 64`.** Step 1 shipped it off (`background_per_session = 0`, the coordinator's decision after review T8D-4: a person could neither see a running command nor stop it with a control); **step 2 gives the person the strip and its stop and sets the default to the owner's 8** (§12). | R8 and §4.5. Kept output in the node's memory is at most `background_per_node × (background_output_kb + 32 KiB)`, 16.3 MiB at the defaults (64 running rings of 256 KiB, and at most 64 ended ones cut to 32 KiB: §11), and 64 running commands are about 300 processes beside the node (measured, §11). The node's idle size does not change. |

## Terms

- **Background command**: a `bash` call with `background: true` still running after its call
  returned. Named `bg-<n>`, numbered per session from the log.
- **End**: the command's namespace is gone (exit, timeout, a stop, the node's death), as its
  guard reports it (`crates/tools/src/contain.rs:552-564`).
- **Step boundary**: after one model call and its tool results are recorded, before the next
  (`crates/agent-native/src/lib.rs:1000`), as in `docs/node-update.md`.
- **Callback turn**: a turn started because background commands ended while the session was
  idle, with no person's message.
- **Unattended run**: the callback turns in a row since the person's last message.

## 1. What exists

| Area | On `main` | File |
|---|---|---|
| The tool's arguments | `command` and an optional `timeout_secs`; the schema states the node's ceiling. No other argument. | `crates/tools/src/bash.rs:161-176` |
| The timeout | `timeout_secs` (at least 1) or `ctx.timeout`, then `min(bash_timeout_max)`. `ctx.timeout` is `NativeConfig::tool_timeout`, **600 s**, set nowhere from `node.toml`. The ceiling is `[limits] bash_timeout_max_s`, default and maximum 3600. | `crates/tools/src/bash.rs:17-21`, `:186-195`; `crates/agent-native/src/lib.rs:201`, `:1866`; `crates/tools/src/lib.rs:194`; `crates/node/src/policy.rs:166`, `:284-291` |
| How a command is held | A contained command is only ever a `ContainedChild`: its guard and the node's end of the guard's channel. Dropping it anywhere closes the channel, which is the kill order; `kill -9` of the node closes it the same way. The call owns it for its whole run. | `crates/tools/src/contain.rs:564-576`, `:695-711`; `crates/tools/src/bash.rs:224-236` |
| How it is stopped | `stop(graceful)`: a TERM byte, half-close, the guard's 2 s grace, the kill, the guard's report (`Ended`, `StillAlive`, `Unconfirmed`) within `STOP_BOUND`. | `crates/tools/src/contain.rs:539-541`, `:584-655`; `crates/tools/src/guard.rs:72-92` |
| A dead node's leftovers | The guard takes "my parent is no longer `--node`" as the kill order, checked every 500 ms; at start the node sweeps marked namespaces a dead node left. | `crates/tools/src/guard.rs:13-24`, `:92`; `crates/tools/src/contain.rs:924`, `:981` |
| Without containment | `contain = "off"` or the Landlock-only fallback: a process group, `KillGroupOnDrop`, no guard, no confirmation; a `setsid` child escapes (T.8a's control case). | `crates/tools/src/bash.rs:26-37`, `:368-387` |
| Output | Kept up to 4 × 30,000 characters (or 2,000,000 with filtering) for the model; streamed live as `tool_output_delta` up to `tool_stream_bytes`. Full outputs of filtered calls sit in an in-memory store per session, 64 per session, 128 MiB per node, gone at restart. Secrets are scrubbed at append. | `crates/tools/src/bash.rs:249-258`; `crates/tools/src/lib.rs:43`; `crates/tools/src/outputs.rs:1-22`; `crates/node/src/session.rs:276-284` |
| Cancel and abandon | The turn's token cancels the call; a tool still running `TOOL_CANCEL_GRACE` (5 s) later is abandoned. | `crates/agent-native/src/lib.rs:44`, `:1886-1931` |
| The step limit | `max_steps` model calls per turn (default 50, `[limits] max_steps`); a turn ends when a model answer has no tool call. | `crates/agent-native/src/lib.rs:1000`, `:1214-1220`, `:1464-1471`; `crates/core/src/policy.rs:57` |
| Approval | Keyed by the exact call (`call_key` over tool and input), so `background: true` is part of what is approved. The card shows every input field and a reason from the tool's name and risk only. `approve` waits for an answer or the turn's cancel, with no timeout. | `crates/agent-native/src/lib.rs:1837-1857`; `crates/node/src/session.rs:1775-1860`, `:1950-1956`; `web/src/components/CallInput.tsx` |
| Who writes events | One `Session` per session: `append` under the `head` mutex, contiguous `seq`; `record` appends outside a turn's flow (a staged file). | `crates/node/src/session.rs:33-37`, `:276-326`, `:370-376` |
| How turns start | Only `Session::prompt`, from the master's `session.prompt`; the node holds no agent definitions ("Resolved by the master; the node does not store agent definitions"). Routines start turns through the same `Master::prompt`. | `crates/node/src/session.rs:455-526`; `crates/node/src/lib.rs:731-759`; `crates/protocol/src/lib.rs:152-158`; `crates/master/src/lib.rs:1530` |
| After a restart | `recover_interrupted` denies pending approvals, writes an error, ends the turn `interrupted`. | `crates/node/src/session.rs:222-274`; `crates/node/src/lib.rs:330-337` |
| Delete | Cancels the turn, waits up to 10 s, drops the session and its temp folder, deletes the log. | `crates/node/src/lib.rs:888-926` |
| Archive | Does not exist: no archive of chats in the master, the store or the protocol. | (searched `crates/master/src`, `crates/store/src`, `crates/protocol/src`) |
| Revocation | The master cancels the running turns of sessions whose owner lost access; "running" is the mirror's `running` column, set by `turn_started` and `turn_ended`. | `crates/master/src/workspaces.rs:517-597`; `crates/store/src/lib.rs:822-830`, `:1537` |
| Between turns | What changed in the folder since the last recorded state is copied into the turn's checkpoint group as **the person's** changes. | `crates/node/src/session.rs:954-984` |
| Event kinds | 19 kinds; the master stores them opaquely; the Rust fold (`crates/core/src/view.rs`) and the TypeScript fold (`web/src/fold.ts:42`) each list them; one event is at most 4 MiB. | `crates/core/src/event.rs:57-80`; `crates/store/src/lib.rs:480`; `PLAN.md` §2 |

What this means today: a `bash` call cannot leave anything running. A dev server lives exactly as
long as its call and the turn waits on it the whole time. That is the problem.

## 2. Stress test

1. **Problem and evidence.** The owner's direction of 2026-11-12: "Processes an agent leaves
   running outlive their command -> ai able to run background commands which will call agent
   back when done". Since T.8a and T.8e a command that returns kills everything it started
   (`crates/tools/src/bash.rs:288-290`, `:370-374`, `PLAN.md` T9), so an agent cannot run a dev server, a
   watcher or a 20-minute build and keep working. The study found 11 of 16 harnesses document
   background processes (`research/harnesses-round-1.md:1532`).
2. **Simplest thing.** A flag on `bash` and the existing `ContainedChild` moved from the call
   into a per-session table: the containment, the guard, the kill on the node's death and the
   approval path are reused unchanged. Deleting something does not solve it. No library: the
   process side is already ours.
3. **The others** (§6). Most use one shell tool with a flag and tell the agent when the command
   ends; their users complain about polling loops, wake storms, orphans, model-set "no timeout"
   and approvals that are not re-checked. We copy the flag and the notification, bound the wake
   chain by default, refuse no-timeout, and keep every command inside the node's containment.
4. **How it breaks** (§5, §4.7). A prompt-injected agent gets a process that lives up to the
   ceiling with nobody watching, and turns that run with nobody watching. A crash kills
   everything (good) and must be visible in the log. A full table refuses. The master being
   away delays callbacks, never duplicates them.
5. **Invariants touched.** "The node decides": every bound is `[limits]`, and the node checks
   every callback it is asked to run. "The master never runs tools": it only decides whether to
   start a turn, as for routines. "One writer per session": every new event is appended by the
   owning session actor. "No secret in an event": output is scrubbed where it enters the node,
   before any cut. "The harness process stays light": output is bounded in memory, at most
   `background_per_node × (background_output_kb + 32 KiB)` per node (16.3 MiB at the defaults,
   §0, §11). **`PLAN.md` T9 changes wording**: a command's processes may outlive its call, never
   its timeout, its session, its owner's access or the node (§5.4).
6. **How we will know.** The abuse tests of §7 first, then each step's tests in §8.

**Verdict: do it, differently from the T.8d row in three points.** (a) The master, not the node,
starts a callback turn: the node does not have the agent definition, the memory context or the
owner's access checks (`crates/protocol/src/lib.rs:155-158`). (b) A background command must
state its timeout; the 60 s default is for foreground calls. (c) A callback turn cannot "count
against the step limit" of the turn that started the command, which has ended; it has its own,
smaller step cap and counts against a per-session budget of unattended turns.

## 3. Decisions (recommendations)

| # | Question | Recommendation | Main alternative and its cost |
|---|---|---|---|
| R1 | Tool shape | `bash { command, timeout_secs, background }`; read with `bash_output { id? }` (no id lists), stop with `bash_stop { id }`. Both new tools read-only risk. | One `background` tool with an action: risk is per tool (`Tool::risk` has no input), so reading and stopping share one class and one `denied_tools` entry. A separate start tool: every existing `bash` deny, allow-list and approval would have to be repeated for it. |
| R2 | What returns at once | Waits up to 3 s: if the command ended, an ordinary result and no background command; else the id, the deadline and the output so far. | Return at once: "port in use" or "command not found" then costs a callback turn. Codex's caller-chosen wait: more to approve and to misuse. |
| R3 | Timeouts (**decided, §0**) | Foreground: `timeout_secs` or 60 s. Background: `timeout_secs` required, refused without. Both: at least 5 s, capped by `bash_timeout_max_s`. No zero, no "none". | A 60 s default for background too: every dev server the model forgets to time dies after a minute and costs a callback. |
| R4 | Who starts a callback turn | The node records the end; the master starts the turn after a routine's admission checks; the node accepts it only if the ends are undelivered and its budget allows. | The node starts it: no agent spec, no memory, no access re-check, runs after the owner was removed. |
| R5 | A turn is running | The end becomes a note at the next step boundary, recorded there; no second turn, no step used. Ends that miss the turn's last step are delivered as if the session were idle. | A second turn queued after: the agent acts on stale state for the rest of the turn. |
| R6 | What the callback carries | Id, the command's first 200 characters, why it ended, exit code or signal, the guard's confirmation, run time, total output bytes, the last 4 KiB of output (scrubbed, marked as program output), and how to read more. | The full output: unattended tokens and a larger injection surface for nothing `bash_output` cannot give. |
| R7 | Commands the agent or the person stopped | No callback; the stop's own result or the person's click is the end. | Callback on every end: a turn spent on "I stopped it". |
| R8 | Bounds (`[limits]`; **the first two decided, §0**) | `background_per_session = 8`, `background_per_node = 64`, `callback_turns = 3`, `callback_max_steps = 20`, `background_output_kb = 256`; ceiling `bash_timeout_max_s`. | Codex-style 64 with LRU eviction: kills a server the person relies on, silently. |
| R9 | At the cap | Refuse the start, naming the running ids. | Evict the oldest (Codex, Hermes). |
| R10 | Callback turns and approvals | The session's own mode, nothing auto-approved, approvals park as in any turn; untrusted chats get no callback turns. | Clamp callback turns to `ask`: makes `auto` sessions stall at every callback. |
| R11 | Cancel of a turn (Stop) | Does **not** stop background commands; the chat's strip has Stop per command and Stop all. | Stop kills all: the panic button is total, and the feature dies the first time a person interrupts to say something. |
| R12 | Node restart or crash | Commands die (as now); at start `background_ended { reason: "node_restarted" }` for each; no callback turn. | A callback per session after a crash: a storm of unattended turns nobody asked for. |
| R13 | Drain | Does not wait for background commands; stops them at exit with their tails recorded; no callbacks while draining. | Wait for them: a drain held up to an hour by a dev server. |
| R14 | Sub-agents | Refused. | Allowed, called back in the parent: the child's folder is gone and the parent is blocked in `delegate`. |
| R15 | Routines | Allowed (behind a click: routines run `ask`/`ask_all`); callback turns are later turns, not the run's. | Refused: a routine that builds and reports is a fair use. |
| R16 | Without containment | Refused unless the node contains the shell (bubblewrap and the guard). | Allowed with a process group: no confirmed end, `setsid` escapes, contradicts T9. |
| R17 | Undo while one runs | Restore of that session's folder refused while its background commands run, with the reason. | Allowed: the command writes over what was restored. |
| R18 | Output storage | A head (16 KiB) and tail ring per command in memory, kept 30 minutes after the end; never on disk, never in the folder. | A log file the agent reads with `read` (opencode, Claude Code): a file in the folder pollutes checkpoints; one in the data directory is outside every grant. |
| R19 | Events | `background_started`, `background_ended`, `turn_started.cause`; schema changed in place. | Reuse `tool_call_finished` late: breaks the one-result-per-call fold. |

## 4. Design

### 4.1 The tool

```text
bash {
  command: string,
  timeout_secs?: integer,   // required when background is true (refused without); 60 when absent otherwise
  background?: boolean      // default false
}
bash_output { id?: string, from?: integer }   // read-only
bash_stop { id: string }                      // read-only risk
```

**Start.** `background: true` goes through everything a `bash` call goes through today: the
agent's tool allow-list, `denied_tools`, the approval (keyed with the flag in the input, so an
"allow this exact call" for the foreground form never covers the background one), the contained
spawn. Then the call waits up to 3 s:

- the command ended: the result is the ordinary one (`[exit code: N]`); nothing is tracked;
- still running: the node moves its `ContainedChild` into the session's table and the call
  returns `[running in the background as bg-3, stopped at 14:32 (timeout 1800 s); you will be
  told when it ends; do not poll]` followed by the output so far.

Refused with a reason the model can act on: no `timeout_secs` (§0, Q5); the session or node table full
(naming the running ids); a sub-agent session (R14); a node without containment (R16); a chat
the master marked untrusted is not refused (the click is the gate there, §4.10).

The tool description says three things the model cannot find out otherwise: a background
command is invisible to later commands (each `bash` call has its own PID namespace, T.8a, so
`ps` and `kill` do not see it; only `bash_stop` stops it); with the network off it cannot be
reached at all from a later command (§5.2); and it is told when the command ends, so it must
not poll.

**Read.** `bash_output { id }` returns the status (running, or how it ended), the head if it was
cut, and the output from `from` (a byte offset it was given last time) or the tail, capped at
`MAX_OUTPUT_CHARS` (30,000, `crates/tools/src/lib.rs:43`). Without `id`: the session's
background commands, running and recently ended. Read-only, so `ask` does not ask; `ask_all`
does.

**Stop.** `bash_stop { id }`: `ContainedChild::stop(true)`, the result is the guard's report, the
same wording a cancelled call gets today (`crates/tools/src/bash.rs:458-505`). Read-only risk:
it can only end what this session started, and asking for a click to stop a process is the wrong
way round. A stop by the agent records the end and calls nobody back (R7).

**Approval card.** `approval_reason` takes the tool and its risk only
(`crates/node/src/session.rs:1950`). For a background call it must say it in words: "`bash` runs
a program that keeps running after this step, for up to 30 min" and, with the network on, "it
can accept connections". The field `background: true` alone is easy to miss.

### 4.2 Timeouts

The owner's rule, "each command require timeout specified (defaults to 60 seconds)", read
literally gives every background command 60 s when the model forgets. A dev server that dies
after a minute is the commonest failure of that rule, and each one costs a callback turn. So:

**Decided (§0): the range and the clamp are shared; the 60 s default is foreground's only; a
background command must state its timeout** (Q5, the owner, 2026-11-13). The start's result
states the deadline in words.

| Call | Timeout | Before T.8d |
|---|---|---|
| Foreground or background, `timeout_secs` given | clamped to 5 s at least and `bash_timeout_max_s` at most | foreground: at least 1, same ceiling |
| Foreground, none given | 60 s | 600 s (`NativeConfig::tool_timeout`) |
| Background, none given | **refused** before anything starts, naming the range (5 s to the ceiling) | n/a |

The foreground change is user-visible: a build or test run that takes two minutes and states no
timeout is stopped at 60 s, where today it runs. The timeout result already says so; it gains a
line: "state `timeout_secs` (up to N) or run it with `background: true`". The schema description
says the default.

The ceiling stays one key, `bash_timeout_max_s` (default 3600, range 1 to 3600,
`crates/node/src/policy.rs:284-291`). A dev server for a whole working day would need the range
raised; that is an owner's question (§10, Q3).

**Not copied: promotion at the timeout.** Claude Code, Hermes and Oh My Pi move a foreground
command that reaches its timeout to the background instead of killing it (§6). That turns a
command a person approved as "runs for up to 60 s while I watch" into one that runs unattended
for longer. Here a timeout ends the command. A later "move to background" control for the
**person** (Cline's shape) needs no new approval and could come with the UI step.

### 4.3 Where a background command lives on the node

A table on the `Session` (`crates/node/src/session.rs:33`), next to `turn`:

```text
background: Mutex<BTreeMap<BgId, Running>>
Running { child: ContainedChild, call_id, started_ms, deadline, output: Ring, stop: Option<Who> }
```

One task per command owns the wait: it reads the pipes into the ring (head 16 KiB, then a tail
ring up to `background_output_kb`; the pipe channel stays bounded, `crates/tools/src/bash.rs:65-69`,
so a flood is read and dropped, never buffered), and waits for the exit, the deadline or a stop.
Its end goes to the session actor (§4.4), then the ring is kept 30 minutes for `bash_output` and
dropped. The task holds a weak reference to the session, so a deleted session drops the table,
which closes every channel, which is every guard's kill order.

The node counts `background_per_node` across sessions in `Inner`. The spawning thread does not
matter: T.8a tested that a command started from a thread that then ends is not killed, and the
guard's death signal names its own single thread (`crates/tools/src/guard.rs:39-44`).

**Not measured:** the memory of one guard process held for an hour, and whether the guard's
500 ms parent check and its report behave the same after an hour as after the seconds the T.8e
tests cover. Both are tests in step 1.

### 4.4 The end and the callback

**Recording the end, exactly once.** Only the command's own task observes the end, and it hands
it to the session actor once. The actor appends `background_ended` (§4.8):

- **no turn running**: at once, through `record` (`crates/node/src/session.rs:370`);
- **a turn running**: the end is queued, and the loop takes the queue at the next step boundary
  through a new `TurnHost` method; the host appends the events there and returns their model
  text, which the loop adds after the step's tool results. The event sits in the log exactly
  where the model saw it, so `history_to_messages` (`crates/agent-native/src/lib.rs:550`) renders
  it in the same place on every later turn and the log alone rebuilds what the model saw;
- **the turn ends with ends still queued** (its last model answer had no tool call): `end_turn`
  appends them after `turn_ended`, and they are treated as ends of an idle session.

If an append fails, the end stays queued and is tried again at the next step boundary, the next
release of the turn slot, the next end of that chat, and on every pass of a node stop; a stop
that still cannot write it, once the commands are gone, names the chat and exits 1, and the next
start records it as `node_restarted`. An end that waits for a running turn's step boundary when
the node dies is recorded as `node_restarted` too (its exit code and output are lost: review
N-3).

**A note inside a running turn** costs no step and no model call: it rides on the next request.
The agent may react or ignore it.

**A callback turn when idle.** The node does not start turns: it has no agent definition
(`crates/protocol/src/lib.rs:155-158`), no memory context, and no view of the owner's account.
The master does, as its routine runner does:

1. The master's watcher (the pattern of `crates/master/src/routines/runner.rs:1054`) sees
   `background_ended` with `delivered: false`.
2. It checks admission as a routine fire does (`docs/routines.md`, "Admission"): the owner's
   account exists, is enabled, owes no password change; they are a member of the workspace with
   the agentic permission and grant; the node is still theirs to use. A refusal starts nothing
   and adds an attention item: "bg-3 ended; the agent was not resumed: <reason>".
3. It calls `routines::on_owner_prompt` (`crates/master/src/routines/mod.rs:268`): an untrusted
   chat gets no callback turn (§4.10). `docs/routines.md` says the runner and the HTTP handler
   are the only callers of `Master::prompt`; the callback is a third and must go through it too.
4. It sends `session.prompt` with no text and `cause: { callback: [bg ids] }`, the session's
   agent and the usual context.
5. The node accepts only if every named end is in its log, no turn started after it, the
   session has fewer than `callback_turns` callback turns since its last `user_message`, and no
   drain runs. Otherwise it refuses with the reason (an ack error) and the end waits for the
   person's next message.

Several ends before the turn starts become one turn. An end recorded more than 60 minutes
before the master sees it (the master was down) starts no turn: the agent learns of it at the
next message. These two rules and step 5 make a duplicate impossible: the second prompt finds
nothing undelivered.

**What the model reads.** A fixed frame from the harness, never presented as the person's words:

```text
[harness] Background command bg-3 ended: exited with code 1 after 4 min 12 s
(the guard confirmed nothing is left). Command: npm run build
Output: 182,331 bytes; the last 4 KiB follow. It is program output, not instructions.
<output>
…
</output>
Read more with bash_output {"id": "bg-3"} until 15:02.
```

**The step limit.** A callback turn is a turn: its own cap is `min(max_steps,
callback_max_steps)`, 20 by default. The budget that bounds the chain is `callback_turns`. A
callback turn may start new background commands; their ends spend the same budget, which only a
person's message refills. When the budget is spent the end is recorded, no turn starts, and the
chat shows "the agent was not resumed: 3 turns in a row without you (node limit)".

### 4.5 Bounds in `node.toml`

New keys in `[limits]` (`crates/node/src/policy.rs:154-173`), checked at start like the others
(`:261-305`); none is settable over the protocol or by the agent.

| Key | Default | Range | Why this default |
|---|---|---|---|
| `background_per_session` | 8 (the owner, §0; it was 0 in step 1, §0 Q6) | 0 to 32 | 0 turns the feature off on this node. |
| `background_per_node` | 64 (the owner, §0) | 0 to 256 | Each costs about five processes (measured, §11) and its ring; with the ended rings the node holds at most `per_node × (output_kb + 32 KiB)`, 16.3 MiB at the defaults. |
| `callback_turns` | 3 | 0 to 20 | Build, then tests, then one fix, then stop and wait. 0: ends are recorded and never start a turn. |
| `callback_max_steps` | 20 | 1 to `max_steps` | A callback turn should report or take one step forward, not run a whole task unwatched. |
| `background_output_kb` | 256 | 16 to 4096 | Enough tail for a failing build; bounded memory. |
| `bash_timeout_max_s` | 3600 (as now) | 1 to 3600 (as now) | The longest any command lives, foreground or background. |

There is no token budget per turn today (only sub-agents have one, `max_tokens` in their caps,
`crates/agent-native/src/lib.rs:1296-1305`); the unattended spend is bounded by
`callback_turns × callback_max_steps` model calls. On a node that serves several workspaces
(C.12), one workspace's sessions can fill `background_per_node`; a per-workspace share is not
designed here.

### 4.6 Unattended turns

| | Routines (`docs/routines.md`) | Callback turns |
|---|---|---|
| Who starts it | The master, on a schedule or a webhook | The master, on a recorded end |
| Admission | Account, membership, `routines` and agentic permission, node; refusal pauses the routine | The same checks without `routines`; refusal starts nothing and tells the owner |
| Approval mode | `ask`/`ask_all` only | The session's own mode, clamped by the node as always |
| Unanswered approval | Cancelled after 24 h | Parks like any turn; no timeout (as for a person's turn today) |
| Untrusted input | Webhook data marks the chat untrusted for good | Untrusted chats get no callback turns; output is marked as program output |
| Bound | Per user, per workspace, per day; overlap skipped | `callback_turns` in a row, `callback_max_steps` each |
| Seen afterwards | Run status, attention item, unseen marker | `turn_started.cause` in the chat, an attention item per unseen callback turn |

**`auto`.** A session the person set to `auto`, on a node whose `max_approval_mode` allows it,
runs its callback turns without clicks. That is the point of the feature in `auto`; it is also
the new risk (§5.3). It is bounded by the two keys above and can be turned off per node with
`callback_turns = 0`. Whether `auto` callbacks should need their own switch is Q2 (§10).

**An approval nobody answers** keeps the turn and its slot, exactly as a person's turn waiting on
an approval does today (`crates/node/src/session.rs:1838-1844`): the card is in "Waiting for
you", and a person's new message is refused "a turn is already running" until they answer or
stop it. Background ends in the meantime queue as notes.

**Afterwards.** The chat shows a callback turn as started by "bg-3 ended (exit 1)" instead of a
message bubble, and the attention list carries it until seen, so a person who comes back sees
what ran without them.

### 4.7 Lifecycle

| Event | What happens to background commands | Event recorded | Callback |
|---|---|---|---|
| The command exits | Ended by its guard | `background_ended { reason: "exited" }` | Yes |
| Its timeout | `stop(true)` | `reason: "timed_out"` | Yes |
| `bash_stop` | `stop(true)` | `reason: "stopped_by_agent"` | No |
| A person's Stop on the command, or Stop all | `stop(true)`; new `session.stop_background { session_id, id? }` | `reason: "stopped_by_user"` | No |
| Stop of the turn (cancel) | Keep running (R11) | none | n/a |
| Session deleted | All stopped before the log is deleted; `delete` waits up to `STOP_BOUND` for them in parallel | none (the log is gone) | No |
| Archive | Does not exist; when it does, archive stops them like a person's Stop all | | |
| Owner loses agentic or the grant, account disabled or deleted | The revocation sweep sends `session.stop_background` beside its cancel, to every agentic chat of the owner that they may no longer drive (no count decides, §11) | `reason: "access_revoked"` | No (admission refuses) |
| Owner leaves a team workspace | The chat falls back to the owner's Personal workspace, where they may go on in it themselves; its commands are stopped, and callbacks refused, only if the owner may not run agentic chats there (§13) | `reason: "access_revoked"` only then | As the owner's own prompt would be |
| Node restart or crash | Dead with the node (guard: channel closed or parent changed, within 500 ms; the start's sweep for the rest) | At start, for each started and not ended: `reason: "node_restarted"`, tail empty (the ring was in memory) | No (R12) |
| Drain (`docs/node-update.md` §7) | Not waited for. The drain's note lists them. At exit: `stop(true)` | `reason: "node_stopping"`, with tails | No (new turns are refused while draining) |
| Master unreachable at the end | Nothing changes on the node; the event is stored and replays at the next hello | as usual | Only if seen within 60 min (§4.4) |
| Undo of a turn in that folder | Refused while that session has background commands running | none | n/a |

**Why Stop of a turn does not stop them.** A person stops a turn to redirect the agent far more
often than to kill a server it started; killing on Stop makes the dev server die every time the
person interrupts. The cost is the panic case: a person who presses Stop because the agent is
doing something bad must also press Stop all. The strip with both buttons sits next to Stop while
any command runs. This is Q1 (§10).

**Between turns.** `pin_between_turns` copies what changed in the folder since the last turn as
the person's changes (`crates/node/src/session.rs:954-984`). With background commands, some of
those changes are the command's. The copy is still right for undo; any text that says "your
changes" must say "changed between turns".

### 4.8 Events and storage

New kinds, a schema change in place (no migration before the first tag):

| Kind | Payload | Appended |
|---|---|---|
| `background_started` | `bg_id, call_id, turn_id, timeout_s, ends_by_ms` | In the turn, before the start call's `tool_call_finished` |
| `background_ended` | `bg_id, reason, exit_code?, signal?, confirmed ("ended" \| "still_alive" \| "unconfirmed" \| "unknown"), ran_ms, output_bytes, tail, tail_cut, delivered (bool)` | By the session actor: at the step boundary (`delivered: true`), between turns, or at start |
| `turn_started` gains `cause?` | `{ callback: [bg_id] }`; absent for a person's turn | At the start of a callback turn, which has no `user_message` |

`reason` is one of `exited`, `timed_out`, `stopped_by_agent`, `stopped_by_user`,
`access_revoked`, `node_restarted`, `node_stopping`.

**Not in them:** the environment, the full output, any secret (the payload is scrubbed at append,
`crates/node/src/session.rs:279-284`), the command (it is already in the start call's input). The
`tail` is untrusted, model-visible text: the UI shows it as terminal text, never rendered.
4 KiB is far under the 4 MiB event limit (`crates/store/src/lib.rs:480`).

**What changes with them:** `EventKind` and `name()` (`crates/core/src/event.rs:59-123`); the
Rust fold (`crates/core/src/view.rs`) gains running and undelivered background commands; the
TypeScript fold (`web/src/fold.ts:42`) and the shared fixture (`web/src/__fixtures__/session.json`,
`crates/core/tests/fold_fixture.rs`); `history_to_messages` renders `background_ended` as the
frame of §4.4; the master's mirror (`crates/store/src/lib.rs:822`) gains a count of running
background commands per session, for the sweep and the list; `PLAN.md` §2's table. Old masters
store unknown kinds opaquely (`PLAN.md` §2), so a newer node does not break an older master.

**Protocol.** `session.prompt` gains `cause?`; new `session.stop_background { req_id, session_id,
bg_id? }`; the node announces capability `background` in `node.hello`, and the master starts
callbacks and shows the controls only for nodes that announce it.

### 4.9 What the master must change

Nothing that runs a tool. The watcher and admission of §4.4; `session.stop_background` from an
HTTP endpoint in `crates/master/src/http/<area>.rs` (not `http.rs`) for the person's stop, owner
only (T10); the revocation sweep (`crates/master/src/workspaces.rs:517-597`) stopping background
commands of sessions with a live count, not only running turns; the attention item for unseen
callback turns and for refused callbacks.

### 4.10 Sub-agents, routines, untrusted chats

**Sub-agents: refused.** A sub-agent's session ends when its turn returns; its folder is removed
then (`crates/node/src/session.rs:65-68`), and the parent is blocked in `delegate` the whole
time. A callback would go to a session that no longer exists. The refusal says: "return, and the
parent can start it". Cheap to lift later if a need appears.

**Routine chats: allowed.** A routine runs in `ask` or `ask_all`, so a background start parks for
the owner's click. The run is the chat's first turn (`docs/routines.md`, "Watcher"), so callback
turns are later turns and do not change the run's status. They do use the session's
`callback_turns` like any chat.

**Untrusted chats** (webhook routines, `routine_untrusted_chats`): a background start is allowed
behind a click; its end is recorded and **starts no turn**. An event-driven chat that acts again
while nobody watches is the shape T16 exists to prevent.

### 4.11 The UI and its cost

What a person must see: a strip above the composer while any background command of the chat
runs (id, the command's head, time left, Stop, and Stop all when there are two or more); on the
start call's card, "running in the background" until its end, then the end and the tail; a
callback turn marked as such; an attention item. A live view of the output is not in the first
UI step (it would put deltas outside turns into the log; a pull request for the tail, like
`artifact.fetch`, is a later step).

**Budget.** The first-load ceiling is 104 KB gzipped against a last measured 99.4 KB
(`web/scripts/size.mjs:69-88`). The strip and the card states are small and sit in the
conversation, which is in the first load: an estimate of 1 to 1.5 KB, **not measured**. It fits;
it must be measured on the branch and given its own line in `size.mjs`.

## 5. How it breaks: security

### 5.1 What the existing containment already stops

Each background command is the same contained command a foreground call runs, so: nothing it
starts escapes its PID namespace and nothing outlives the node (T.8a, T.8e; `PLAN.md` T9); it
sees only the node's grants in its file view, fixed when it starts (T.8c); with the network off
it has its own network namespace (T.8b); it holds neither its guard nor the guard's channel; the
start went through `denied_tools`, the agent's tools and the approval.

Two of those are stronger than they look here. **A process that waits for a later grant gets
nothing:** its file view and its network namespace are built once, at its start, and a later
approval is keyed to one exact call in one turn. **A later command cannot reach it:** not by
`ps`, `kill` or `/proc` (its own PID namespace), so an injected later command cannot signal or
debug it.

### 5.2 What this newly allows

- **Time without a person.** Today a command lives at most its timeout and the turn waits on it,
  so a person who is watching the turn is watching the command. Now it lives up to the ceiling
  (an hour by default) after the turn ended, `background_per_node` at a time.
- **Listening sockets with the network on.** A server in the host's network namespace, bound to
  `0.0.0.0`, is reachable from the LAN for its whole lifetime. A foreground server could do the
  same for the length of its call; the difference is lifetime and nobody watching. A browser that
  opens it sends the master's cookie to it (browsers send cookies to every port of a host,
  `PLAN.md` T2); the login key (Gate 3 P-1) is what keeps that harmless and must stay.
- **With the network off, no dev server at all.** Each command's own network namespace (T.8b)
  means a server started in the background cannot be reached by the agent's next command, and
  Landlock refuses a TCP listener inside anyway. The tool description says so. A per-session
  network namespace shared by its commands would fix it; not designed here.
- **CPU, memory, processes.** There are no cgroups (C.11 chose a PID namespace, no delegation).
  A miner or a fork bomb in a background command runs until its timeout; the per-user process
  limit and the machine's memory are the only bounds, as for a foreground command, now for
  longer and several at once. Bounded by count × ceiling. Lowering a background command's CPU
  priority is cheap in principle; how to do it without `unsafe` is **not verified**. Claude
  Code caps the memory of a session's commands with a memory cgroup when it can create one and
  runs them uncapped when it cannot (`research/round1/synth/01-loop-tools-editing-shell.md:876`);
  the same needs the cgroup delegation C.11 avoided, so it is not proposed here.
- **Turns without a person** (callback turns), bounded in §4.5, and in `auto` without clicks.
- **An injection channel into a later turn.** The tail goes into a model request later, maybe
  in a turn nobody watches. It is scrubbed where the output entered the node and again in the
  loop, capped at 4 KiB, and quoted line by line (every line behind `| `, control characters
  replaced, one harness line before it saying how many lines follow), so the program can neither
  end the quote nor write a line that begins like the harness's. It reaches the provider inside
  the step's last tool result, or in front of the person's next message under a fixed separator,
  never as a user-role message of its own, and it is never URL provenance (review T8D-2, T8D-3).
- **Disk.** Output never reaches disk (a ring in memory). Files the command writes do, without a
  bound, as for a foreground command; `PLAN.md` T28 covers the store failing on a full disk.
- **Grants that outlive the turn.** C.3 (not built) binds credential grants to a turn and renews
  them while the turn lives. A background command must not receive a turn-bound credential: it
  would hold it after the turn. C.3 must say so when it is built.

### 5.3 The prompt-injected agent's uses, and the answer

| Use | Answer |
|---|---|
| Start a long-lived listener as a foothold | Behind the bash approval under `ask`; the card says "keeps running" and "can accept connections"; the strip shows it; it dies at its timeout, at the owner's loss of access, with the session and the node. |
| Start a miner | The same, plus the count. Not prevented in `auto`: bounded. |
| Wait for the person to grant something | Its view and network are fixed at its start (§5.1). |
| Chain callbacks to act while nobody watches | `callback_turns` (3) in a row, reset only by a person's message; `callback_max_steps` (20); no callbacks in untrusted chats; no callbacks for the agent's own stops; approvals still park. |
| Put instructions in its output for a later turn | Quoted line by line, scrubbed, capped, inside a tool result or before the person's message, never a message of its own; no URL provenance; no web tools in untrusted chats as today. It is still text the model reads: a model that obeys quoted program output is not stopped by the quote. |
| Fill the node | Per-session and per-node counts; a bounded ring per command; the pipe channel stays bounded. |
| Raise its own bounds | No argument or event changes `[limits]`; the master's `cause` is checked against the node's log and budget. |
| Survive the session's deletion or the owner's removal | Stopped on both (§4.7), tested in §7. |
| Restart itself forever | Each restart is a new start: approved under `ask`, counted, and in `auto` bounded by the callback budget. |

### 5.4 `PLAN.md` changes when built

T9 today reads "Nothing an agent's command starts outlives it". It becomes: a command's processes
outlive its `bash` call only as a background command the node holds, and never outlive its
timeout, its session, its owner's access or the node. A new row, draft:

> **T36 (draft)** A steered agent keeps a process running or keeps acting after the person left:
> a listener, a miner, a chain of callback turns, output that steers a later unattended turn.
> Background commands only where the shell is contained; each states a timeout under
> `bash_timeout_max_s`; per-session and per-node counts; stopped at the session's deletion, the
> owner's loss of access and the node's end, with an event for each; callback turns started by
> the master after a routine's admission checks, at most `callback_turns` in a row without a
> person's message, `callback_max_steps` each, none in untrusted chats; output scrubbed, framed
> and capped. Details: `docs/background-commands.md`.

## 6. What the others do

Sources: the project's research first, then the cloned repositories, read as data. Paths under
`research/` are in this repository; others are under `.claude/research-repos/`.

| Harness | Shape | Timeout | Told when it ends | Limits and lifecycle | Source |
|---|---|---|---|---|---|
| Claude Code | `Bash` with `run_in_background`; output to a file read with `Read`; `/tasks` lists and stops; Ctrl+B backgrounds a running command; a foreground command at its timeout moves to the background; `Monitor` turns output lines into events with a mandatory deadline (5 min default, 30 max) | 2 min default, 10 min max | Not verified for an idle session | Cleaned at exit; count limit not verified | `research/round1/synth/01-loop-tools-editing-shell.md:758-765`, `:775`, `:926` |
| Codex | `exec_command` with `yield_time_ms` (default 10 s) returns output so far and a session id; `write_stdin` polls (empty input) or writes | Yield 250 ms to 30 s; poll wait up to `background_terminal_max_timeout` (300 s) | No notification found: the model polls (not verified beyond the files read) | 64 processes, oldest pruned; 1 MiB buffer each; all ended at session shutdown | `codex/codex-rs/core/src/unified_exec/mod.rs:68-77`; `codex/codex-rs/config/src/config_toml.rs:349`; `research/round2/synth/01-loop-dispatch-durability.md:40`; `research/round1/synth/01-loop-tools-editing-shell.md:776` |
| opencode | `shell` with `background`; the reply names an output file; "DO NOT poll" in the description | Foreground 2 min; background none by default; experimental: a timeout moves the call to the background | Yes: a synthetic message with the output; delivered at the next step when busy; idle start not verified | Restart marks it cancelled and writes a notice | `opencode/packages/core/src/tool/plugin/shell.ts:24-66`, `:165-195`, `:229` |
| Hermes Agent | `terminal(background, notify_on_complete, watch_patterns)` plus a `process` tool (list, poll, wait, log, kill, write) | Foreground cap 600 s; a longer request is promoted to a tracked background job | Yes, with the last 2,000 characters; watch patterns rate-limited | 64 tracked, LRU; 200,000-character buffer; a crash checkpoint restores processes | `research/round2/synth/01-loop-dispatch-durability.md:120`; `hermes-agent/tools/process_registry.py:60-100`; `research/round1/synth/01-loop-tools-editing-shell.md:784` |
| Oh My Pi | `bash` with `async`; auto-background after 60 s | 300 s default, 1 to 3600 | Yes: an idle session gets a follow-up turn | 15 running jobs; approval checked once at the start, not again | `oh-my-pi/packages/coding-agent/src/async/job-manager.ts:12`, `:44`; `research/round2/synth/02-approvals-permission-rules.md:542` |
| DeepSeek harness | `bash` with `run_in_background`; `job_output`, `job_list`, `job_kill` | No background timeout | Yes: next step when busy, a new turn when idle; unbounded unless `maxConsecutiveWakes`, reset by a user message; jobs the model killed do not wake it | 10 per session; registry not restored after restart; a missing end shown as incomplete | `deepseek-harness/packages/jobs/tool-jobs/README.md:42`, `:57`, `:95`; `deepseek-harness/docs/subsystems/jobs.md:386` |
| OpenClaw | `exec` with `yieldMs`, `background`, `durable` (a systemd user service adopted after restart) plus a `process` tool | Applies after the call returns; 0 disables | Not verified | Stop leaves `background` commands running | `openclaw/docs/tools/exec.md:21`, `:78-100` |
| Cline | Only a person can detach a running command ("Proceed While Running") | 30 s per the research; not verified in code | Not verified | A detached command outlives the run | `research/harnesses-round-2-appendix.md:1653-1654`; `cline/CHANGELOG.md:2912`, `:1722` |
| goose | No background flag; leftovers of a normal exit are left alive on purpose | `timeout_secs`, 0 means none | No | Children survive a goose crash | `research/round2/synth/03-sandbox-containers-execution.md` |
| Pi | None by design: use tmux | No default | No | Children survive SIGKILL | `research/round1/synth/01-loop-tools-editing-shell.md:780` |
| OpenHands | The cloned repository holds the app and launchers only; no background mechanism found; the research says none is described | | | | `research/round1/synth/01-loop-tools-editing-shell.md:782` |

**What users complain about, in the material:** models polling in loops (opencode's and
DeepSeek's descriptions forbid it; OpenClaw's loop detector has a "poll with no progress"
class); wake storms (DeepSeek's README: unbounded wakes let an unattended agent chain forever,
and a cap can stall it; a Hermes code comment cites watch patterns forcing a full-context turn
every time); a foreground timeout killing dev servers and causing retry loops (the reason for
Claude Code's, Hermes' and opencode's promotion); orphans after crashes (goose, Pi, Cline);
model-set "no timeout" (goose, Oh My Pi); approval checked once (Oh My Pi); work lost at restart
(DeepSeek, opencode). Actual user issue texts were not in the material.

| Idea | Verdict | Why |
|---|---|---|
| One shell tool with a flag (Claude Code, opencode, Oh My Pi, DeepSeek) | **Copy** | Every existing control on `bash` covers it for free. |
| Tell the agent at the end; next step when busy, a turn when idle (opencode, Oh My Pi, DeepSeek, Hermes) | **Copy, improved** | Bounded by default, reset by a person's message (DeepSeek has the cap, unset by default); the master re-checks access before each turn; none of them does that. |
| No wake for the agent's own stop (DeepSeek) | **Copy** | A turn spent on "I stopped it" is waste. |
| A short first wait with the output so far (Codex's `yield_time_ms`) | **Copy, fixed at 3 s** | Catches instant failures in the same step; a model-chosen wait is one more thing to misuse. |
| Polling tools as the main path (Codex) | **Skip** | The polling complaint; the callback replaces it. `bash_output` exists for reading, not waiting. |
| No timeout for background commands (opencode, DeepSeek, OpenClaw's 0) | **Skip** | The owner's rule; a forgotten server would live until the node dies. |
| Foreground promoted to background at the timeout (Claude Code, Hermes, Oh My Pi) | **Skip for the agent; later for the person** | Widens what a person approved; a person's own "move to background" needs no new approval. |
| Watch patterns and heartbeats (Hermes, Claude Code's `Monitor`) | **Skip** | Each match is a full-context turn: the wake storm. Later, if asked for, as a note at the next step only. |
| Evict the oldest at the cap (Codex, Hermes) | **Skip** | Refusing is visible; eviction kills what someone relies on. |
| Write to stdin (Codex, Hermes) | **Skip** | stdin is `/dev/null` (`crates/tools/src/bash.rs:217`); interactive programs are out of scope. |
| Survive a restart (OpenClaw `durable`, Hermes' checkpoint) | **Skip** | Contradicts T9 and the guard's design; a restart is recorded instead, which opencode and DeepSeek also do. |
| Output in a file the agent reads (opencode, Claude Code) | **Skip** | In the folder it pollutes checkpoints; elsewhere it is outside the grants; a ring with `bash_output` is bounded and scrubbed. |
| Only a person may detach (Cline) | **Skip as the only way** | The agent needs to start servers itself; the person's control comes with the UI. |
| Approval checked once (Oh My Pi) | **Improve** | A background command keeps the node's policy and stop path (the research's own recommendation, `research/harnesses-round-2-appendix.md:3059`). |

**Not verified about the others:** Claude Code's count limit, whether it wakes an idle session,
and the names `BashOutput`/`KillShell` (not in the research); whether Codex ever tells the model
of an exit and what its cancel does; whether opencode starts a turn in an idle session; Cline's
default timeout in code and any automatic notification; OpenHands' agent-side shell (not in the
clone); Hermes' retention (the research says 7 days, the code 30 minutes); Oh My Pi's job tool
names.

## 7. Abuse tests to write first

Each fails on `main` (where there is no background command, the test asserts the refusal or the
behaviour this note requires, and fails because the argument is ignored and the command runs in
the foreground).

1. The T.8a escapers (`setsid`, `nohup … &`, double fork, SIGTERM-ignoring child) started in a
   background command are all gone 2 s after: its timeout, `bash_stop`, a person's stop, the
   session's deletion, the owner's removal from the workspace, and `kill -9` of the node.
2. A background command asked for above `bash_timeout_max_s` runs to the ceiling, not beyond;
   one without `timeout_secs` is refused (§0, Q5).
3. The ninth start in a session and the sixty-fifth on a node are refused with the running ids
   (the defaults of §0; the tests use smaller caps); nothing is evicted.
4. No argument, tool input, event or `session.prompt` field raises a `[limits]` bound: a
   `cause` naming an id the log does not hold, an end already delivered, or a fourth callback in a
   row is refused by the node.
5. A callback arrives exactly once, in the owning session: two ends at once give one turn; a
   master that sends the callback twice gets one turn and one refusal; another session's id is
   refused.
6. A callback into a deleted chat starts nothing; a callback for an owner removed from the
   workspace starts nothing and the command was stopped; one into an untrusted chat starts
   nothing.
7. A sub-agent's background start is refused; a node with `contain = "off"` refuses every
   background start.
8. Output: a flood of 5 GB keeps node memory under the ring bound; a secret in the output does
   not appear in `background_ended`; the tail reaches the model inside the frame.
9. After `kill -9` of the node and a start, each running background command has a
   `node_restarted` end and no turn was started.
10. The approval card of a background call says it keeps running (the reason text, not only the
    field).

## 8. Build order

| Step | What | Tests (fail before, pass after) | User-visible documentation | Size |
|---|---|---|---|---|
| 1 | **Background commands without callbacks.** `background` and the required timeout; the per-session table; `bash_output`, `bash_stop`; `background_started`/`background_ended`; the note at the next step and at the next person's message; the restart end; delete, revocation (`session.stop_background`, the mirror count) and drain stops; refusals for sub-agents, uncontained nodes and full tables; the approval reason; foreground default 60 s; `[limits]` keys except the callback ones. | §7 tests 1 to 4, 7 to 10; a dev server started in one step answers `curl` in the next with the network on; a foreground call without a timeout ends at 60 s (600 s on `main`); a one-hour background command's guard still confirms its end (`cli/tests/guard.rs` style). | README: the tool, the keys, network off; `PLAN.md` §2 kinds, T9 wording, T36. | **L** |
| 2 | **The person's controls** (built, §12). The strip, Stop and Stop all, card states, the HTTP endpoint (owner only); the bundle line in `size.mjs`; the default of 8. | A stop from the UI ends the command with `stopped_by_user`; another user's stop is 404; the first load stays under 104 KB; an e2e flow in `web/e2e/`. | README: what the strip shows. | **M** |
| 3 | **Callback turns.** The master's watcher and admission, `cause`, the node's budget check, `callback_turns`, `callback_max_steps`, the attention items, untrusted chats. | §7 tests 5 and 6; a fourth callback in a row is refused and the chat says why; a person's message refills the budget; a callback turn's steps stop at 20; an end seen after 60 min starts no turn. | README: unattended turns, their bounds and how to turn them off; `docs/routines.md`: the third caller of `Master::prompt`. | **M** |
| 4 | Later, each its own decision: a live tail for the person; a person's "move to background"; a per-session network namespace for network-off dev servers; a lower CPU priority. | | | |

Step 1 is useful alone (servers and long builds work; the agent learns of ends at its next step
or the person's next message), but it was not safe to turn on alone: the person could neither see
a running command nor stop it with a control until step 2, so step 1 shipped with the feature off
(review T8D-4, §0 Q6) and step 2 turns it on. Step 2 before step 3, so the person can see and stop
what runs before anything acts unwatched.

## 9. What I am least sure of

1. **That the master can start a callback turn with exactly a person's context.** Read:
   `Master::prompt`'s signature (`crates/master/src/lib.rs:1530`) and the routine runner's
   pattern; not read: how the agent, memory and project context are resolved inside it.
   Settled by reading `Master::prompt` in full and writing step 3's first test against it.
2. **The note after tool results in one request.** **Decided (review T8D-3):** no note is a
   user-role message. At a step boundary it is appended to the step's last tool result (the
   fallback this item named); an end waiting for the person goes in front of their next message
   under a fixed separator. Both are ordinary message shapes for every provider; no provider was
   contacted to test them.
3. **The guard held for an hour.** The T.8e tests cover seconds. Its 500 ms parent check, its
   memory and its report over an hour, and three processes per command at 64 per node, are not
   measured (step 1 measured them: §11). Settled by step 1's long test and an RSS reading.

Also unsure, smaller: whether 60 s for foreground calls breaks common builds often enough to
annoy (Q4 is the owner's).

## 10. Open, for the owner

**Answered on 2026-11-13: see §0.** Nothing is left open: Q5 was decided the same day (a
background command must name its timeout, refused without). The questions are kept as asked.

- **Q1. Does Stop of a turn stop its background commands?** Recommended: no; a Stop all sits
  next to Stop while any run. The other choice (Stop ends everything) makes the panic button
  total and the feature fragile: every interruption kills the dev server.
- **Q2. Callback turns in `auto` sessions without a click?** Recommended: yes, bounded by
  `callback_turns` and `callback_max_steps`, off per node with `callback_turns = 0`. The other
  choice (callback turns clamped to `ask`) makes every `auto` session stall at the first callback
  until someone clicks.
- **Q3. The ceiling for background commands.** Recommended: keep `bash_timeout_max_s` (one hour
  at most) for both. The other choice (a separate `background_timeout_max_s` up to 24 h) lets a
  dev server last a working day, at the cost of a process that can live a day unwatched.
- **Q4. The foreground default drops from 600 s to 60 s.** Recommended: yes, as directed, with the
  timeout result saying how to ask for more or go background. The other choice (keep 600 s for
  foreground, 60 s only where a timeout is missing on a background call) avoids killing
  two-minute builds but leaves the turn blocked ten minutes by a forgotten server.
- **Q5. A background command must name its timeout.** Recommended: yes (refused without). The
  other choice (60 s by default, as the direction reads) kills every forgotten dev server after a
  minute and spends a callback turn on it.
- **Q6. On by default?** Recommended: on, with the defaults of §4.5; a node owner turns it off
  with `background_per_session = 0`. The other choice (off until a node opts in) is safer for
  existing nodes and means no one uses it until they read the README.

## 11. Step 1 as built (2026-11-13)

Built on `feat/T.8d-background-commands`: §4.1 to §4.3, §4.5 (without the callback keys), §4.7
except the person's stop, §4.8 without `turn_started.cause`, and the master's part of revocation.
Where the code differs from the sections above, the code is described here.

- **The tool** (`crates/tools/src/bash.rs`, `crates/tools/src/background.rs`). `bash { command,
  timeout_secs?, background? }`; a background call without `timeout_secs` is refused before
  anything starts, naming the range (§0, Q5). One clamp for both kinds
  (`background::timeout_for`): at least 5 s, at most `bash_timeout_max_s`, and where node.toml
  sets the ceiling below 5 s the ceiling wins. The foreground default is
  `NativeConfig::tool_timeout`, now 60 s. `bash_output { id? }` has no `from` offset (not needed
  for step 1); its text keeps the first third and the end of what is held, with a marker, inside
  the result's cap. A `bash_stop` of a command that already ended says so.
- **The ring** keeps `min(16 KiB, budget / 4)` of the head, the rest of `background_output_kb`
  as the tail, scrubbed where the output enters (the streaming scrubber of the live stream), cut
  on character boundaries; `output_bytes` counts the bytes the pipes read. **When the command
  ends** its ring is cut to its head and the last 16 KiB (`ENDED_MAX` = 32 KiB); a chat keeps
  output for its newest `background_per_session` ended commands and the node for its newest
  `background_per_node`, older entries say their output is no longer kept, and a chat lists at
  most 64 ended entries. Every table is pruned by time (30 minutes after an end) by one node task
  every minute and at every start, whether its chat calls in or not. The bound on what the node
  holds is `background_per_node × (background_output_kb × 1 KiB + 32 KiB)` (review T8D-1).
- **A node slot** is given back only for a confirmed end; a command whose guard reported it
  still alive, or gave no report, keeps its slot until the guard has exited (review N-4).
- **The table** is the node's (`Registry`, made from `[limits]` at start) and is handed to the
  loop through `TurnHost::background`, type-erased (`Arc<dyn Any>`), because `harness-core` does
  not know the tools; the loop gives it to a turn's tools only when the turn is not a
  sub-agent's (`TurnCtx::parent_session`), and the node makes none for a sub-agent's or a chat
  session. A node-wide refusal names only the asking chat's ids, never another chat's.
- **The note.** `background_ended` is rendered by `BackgroundEnded::model_note` from the event
  alone, the same at the step boundary and in later history. It does not name the command (the
  event does not carry it, §4.8) and says "kept for 30 minutes after the end" instead of a time
  of day. The output is quoted line by line (every line behind `| `, control characters but tab
  replaced) under one first line that says how many lines follow, so exactly one line of a note
  starts with `[harness]` whatever the program prints; and in every tool result sent to the
  provider a line of the result's own text that starts with `[harness]` is sent as
  `\[harness]…` (`escape_leads`, review R-2), so a foreground command of the same step cannot put
  a note of its own beside the real one. The log and the UI keep the program's text as it was.
  Inside the loop it is a `ChatMsg::Note`, which is
  **not** URL provenance (a `ChatMsg::User` would have made program output trusted for
  `web_fetch`: `observed_text` takes every user message); at the provider boundary
  (`place_notes`) a note joins the step's last tool result, or goes in front of the person's
  next message under `--- the person's message follows, unchanged ---`: no note is ever a
  user-role message of its own. The loop scrubs every note, and the node returns the scrubbed
  end it appended (review T8D-2, T8D-3). An end the agent ordered itself gives no note.
- **The step boundary** is `TurnHost::background_notes`, called after a step's tool results and
  before the next model call; an end queued when the turn's last answer had no tool call is
  recorded after `turn_ended` (the slot's release wakes the session's end recorder).
- **Delete** stops the session's commands (all at once, waiting at most `STOP_BOUND` + 3 s)
  before its log goes, and records nothing; dropping a session's table stops them too.
- **Revocation.** `session.stop_background { session_id, bg_id?, by: user | access_revoked }`,
  answered inline by the node, sent by the master only to nodes announcing `CAP_BACKGROUND`. The
  revocation sweep and the hello's sweep send it to **every** agentic chat of a blocked owner on
  such a node, after its cancel, whatever the mirror shows: no count decides (review N-1, N-8),
  and the mirror's count and its `sessions.background` column were removed again (so this branch
  changes no table). A revoked chat's table then refuses starts and stops a start handed over
  during its first wait, until its owner prompts the chat. The HTTP endpoint is step 2's (§12).
- **The node's stop** orders every command to stop on each pass of its wait, retries ends the
  log did not take, and waits until every end is recorded or, once the commands are gone, a
  retry has failed; those chats are in its result and the process exits 1 (review T8D-6). It
  waits for running commands and unrecorded ends, not for node slots: a recorded still-alive
  end, which keeps its slot until its guard exits, is logged once with its chat and id and does
  not hold the stop (review R-1).
- **A start's result and its live stream** are built from the ring, through its scrubber, so
  a secret the first wait cuts is in neither (review R-4).
- **The approval card** of a background start does not offer "for this turn", and an answer
  that sends it allows that one start only (review N-2).
- **After a restart** each command the log shows running gets `background_ended { reason:
  node_restarted, confirmed: unknown, ran_ms: 0 }`.
- **An undo** is refused while **that session's** commands run; another session in the same
  folder is not checked.
- **Off by default** in step 1 (`background_per_session = 0`, review T8D-4; on since step 2,
  §12). While it is off
  (or in a sub-agent's turn) the model is not offered `bash_output` or `bash_stop`, and `bash`'s
  description and schema say nothing of `background`; a start that asks anyway is refused with
  the node's reason. In step 1, with it on, a
  person could end a command only by deleting the chat, stopping the node, or the agent's
  `bash_stop` (step 2 adds the strip's Stop, §12). Routine chats and untrusted chats may start background commands behind their
  click like any chat (§4.10); nothing on the node treats them differently in step 1.
- **Not said elsewhere:** one user's chats can hold every slot of the node, and a chat refused
  at the node's cap learns that other chats fill it (N-5); a delete whose log removal fails has
  stopped the commands already, and their starts get `node_restarted` at the next start (N-7).

**Measured** (debug build, this machine, `cli/tests/background.rs` `measure_*`, run by hand,
2026-11-13). 64 commands in eight chats, each ring filled (600,000 bytes printed into 256 KiB):
the node process's RSS grew by 22.0 MiB (the rings' 16 MiB, their tasks and pipe buffers), and
301 processes were added beside it, about 4.7 a command (the guard, bubblewrap, its init,
`sandbox-exec`'s shell and the command), not the 3 §0 estimated; the 65th start in a full chat
was refused naming its eight ids; stopping all 64 at once took 19 ms. A start's call returns at
its first wait, 3.00 s; the spawn itself is a foreground command's. A guard held for 600 s
(not the hour §9 asks for): its RSS went from 20.2 MiB to 7.1 MiB (pages it no longer touches
reclaimed), and its stop was confirmed in 335 ms. A flood (`yes`, 68 GB in 15 s) kept a 64 KiB
ring; the test process grew 12.2 MiB (its runtime's threads), not with the flood. After the
review's fixes (2026-11-13): twelve 4.5 MB starts in one turn with `background_per_session = 2`,
`background_per_node = 2`, `background_output_kb = 4096` left 64 KiB of output in the tables
(the bound is 8256 KiB; before the fix the review measured 86 MiB of RSS for the same run). The
test process's RSS still grew by about 50 to 60 MiB in that test: its own copy of every event,
the twelve calls' live streams of up to 1 MiB each and the allocator; the tables' own count is
the measure the test asserts.

**Not built of step 1:** the person's stop and its HTTP endpoint (step 2, §12), `turn_started.cause`
and `callback_turns`/`callback_max_steps` (step 3), a live view of the output.

## 12. Step 2 as built (2026-11-13)

Built on `feat/T.8d-step2-stop`: §4.11's strip and the person's stop of §4.7 and §4.9, and the
owner's default of 8 (§0 Q6). Not built of §4.11: the start call's card states ("running in the
background" until the end; the card shows the start's result, the chat a line for the end, as in
step 1), the callback turn's mark and the attention items (step 3).

- **The route** (`crates/master/src/http/background.rs`): `POST
  /api/sessions/{id}/background/{bg_id}/stop` for one, `POST /api/sessions/{id}/background/stop`
  for all. Authorisation is the per-chat routes' own (`visible_session`): the owner, or the API
  token; anyone else, server admins and workspace admins included, gets the 404 of a chat that
  does not exist. Like the turn's cancel it needs no settled password and no agentic grant
  (stopping is always allowed, Gate 3 P-7), and it is exempt from the diverged-session refusal
  (`session.stop_background` writes nothing the master must accept first). The login key and the
  page's Origin are required by the guard every cookie write passes (Gate 3 P-1, AUDIT L6); no
  scheme of its own. A `bg_id` that is not `bg-<1 to 9 digits>` is 400.
- **The master answers nothing itself and reads no log for a stop.** Every stop is relayed as
  `session.stop_background { bg_id?, by: user }`; the node stops the command and records
  `stopped_by_user` (step 1's code, unchanged), takes a stop of a command it still keeps as ended
  without a second end, and refuses an id it does not have ("no background command bg-9 in this
  chat (it has bg-1)"), which is passed on as 409. The answer is 200 `{ok: true}`; the end
  arrives as the event. The master waits for the node's ack only, not for the end. As first
  built the master answered "already ended" from its own log, and Stop all with nothing the log
  showed running, without asking the node. Both are removed: the read scanned the chat's whole
  event log under the mirror's lock (review S2-2), and a log that is behind, or read through a
  bounded window, let a chat that starts thousands of short commands hide a long-running one
  from Stop all.
- **A node that is away** (not connected): 503, "the node … is away, so the stop cannot reach it;
  its background commands end with that node or at their timeout, and the chat shows the end when
  the node is back". Nothing is queued for later: a node that comes back after a crash records
  `node_restarted` for each, and one whose link only dropped still has them running, with the
  strip's Stop working again. **A node without `CAP_BACKGROUND`**: 409, sent nothing. A node that
  is stopping: 503 (it stops them itself, `node_stopping`). No ack in time: 504.
- **The strip** (`web/src/components/BackgroundStrip.tsx`, rows from `web/src/lib/background.ts`):
  above the reply box while a command of the open chat runs, one row per command in start order:
  the id, the first non-empty line of the command cut at 80 characters (the start call's input,
  the model's text: React text, hidden characters marked by `showHidden`, never HTML; `title` is
  the same text), the time left, Stop; "Stop all" with two or more (one request). A click shows
  "stopping…" and disables that Stop for 10 s (then "Stop again"); a refusal shows the server's
  reason on the row and Stop works again; the log's end turns the row into "ended: stopped" for
  6 s, the timeline keeps its `background_ended` line. Its header says the commands keep running
  when the turn stops, so the chat's Stop (which does not stop them, R11) leaves the strip on
  screen beside the composer's Stop.
- **State of record.** Every row is the fold's (`SessionView.background`, which gained
  `timeout_s` in both folds and the shared fixture): a reload and a second tab show the same
  rows. The tab's own state is only its clicks ("stopping…", a failure, the brief "ended"); a
  second tab sees the end, not the other tab's click.
- **The clock.** `ends_by_ms` is the node's clock, the browser counts with its own. The time left
  is `min(max(ends_by_ms − now, 0), timeout_s)`: a browser behind the node shows at most the
  command's own timeout, one ahead shows less, down to "at its timeout", never negative; it is in
  whole seconds, minutes or hours, and nothing acts on it: only the log's end removes a row. No
  server time is fetched: the node's and the master's clocks can differ as well, and a second
  correction would claim an exactness the deadline does not have.
- **A node that is away** (the node list says `connected: false`; a node the list does not name
  is not taken as away, and the server's 503 says it if it is): the strip says so in words, each
  row shows "node away" instead of the time left, and no Stop is offered.
- **The chunk.** The strip is its own lazy chunk (`BackgroundStrip-*.js`), imported by the
  conversation on the first running command of the open chat and then kept mounted for that
  chat (it hides itself) so an end the tab asked for can show; a chat with none never loads it.
  `size.mjs` checks it stays out of the first load. Measured: 1.9 KB gzipped (its stylesheet
  0.4 KB), the conversation +0.1 KB; the first load unchanged at 99.5 KB of the 104 KB ceiling;
  the total 370.5 KB (368.4 before), its soft line raised by 2.1 KB to 370.6.
- **On by default.** `background_per_session` defaults to 8 (`DEFAULT_PER_SESSION`). The step 1
  tests that meant "off" now set 0 explicitly (`a_node_with_the_feature_off_offers_no_background_tools`,
  `the_default_node_takes_a_background_start_and_0_refuses_it`), and the latter also shows that a
  node with the defaults takes a start.
- **Not done:** the start card's states (above); a person's stop is not recorded as who stopped
  it beyond the reason (`stopped_by_user`: the chat has one owner); the review proofs in
  `crates/cli/tests/support/review_t8d/` still assert step 1's default (T8D-4's proof expects a
  refusal with the defaults) and are the reviewer's to update.

## 13. Step 3 as built (2026-11-13)

Built on `feat/T.8d-step3-callbacks`: §4.4's callback turn when idle and its step limit, §4.5's
two callback keys, §4.6, §4.8's `turn_started.cause`, §4.9's watcher, admission and attention, the
callback rows of §4.7, and what step 2 left open (the chat list shows a running command). Where
the code differs from the sections above, the code is described here.

- **The request is its own message**, `session.callback { req_id, session_id, bg_ids, agent?,
  savings? }`, not `session.prompt` with a `cause`: a callback has no text, attachment, image,
  reply, parallel answer or context, and a message without those fields cannot carry them
  (a `text` field sent anyway is ignored by serde). Sent only to nodes announcing
  `CAP_CALLBACK` (`callback`). At most `MAX_CALLBACK_IDS` (64) distinct ids. The node's answer
  that finds nothing to resume begins `nothing to resume` (`CALLBACK_NOTHING`; the master drops it
  quietly); a refusal on the node's rules begins `the agent was not resumed` (`CALLBACK_REFUSED`).
- **The master's side** is `crates/master/src/callbacks.rs` and goes through `Master::prompt`
  with `PromptOpts.callback` (its third caller): the chat's own agent as for a prompt; the turn
  gets **no per-turn context and no memory** (no recall, no project facts, no `remember`): the
  context block would not rebuild from the log (below), and program output must not steer what is
  remembered. Any text, context, attachment, image, reply or `untrusted` with it is refused.
- **The watcher** is called from `Master::apply` for every event the mirror takes (not from the
  broadcast, which can lag). An end that may call back (`BackgroundEnded::calls_back`: it exited or
  timed out and no step was told of it, R7, R12) starts one check of its chat after 300 ms; ends
  that come during a check make it look once more, so ends together are one request. **Its
  position is the log**: a check reads the ends after the chat's last `turn_started` and last
  `callback_refused`, less those a refusal of the master's covers (its attention mark's `seq`).
  At the master's start (`Master::start_callbacks`, from `serve`) every chat whose log moved in
  the last 65 minutes is checked once. A node that is away, does not ack in time or is stopping is
  tried again every 15 s; a node without `CAP_CALLBACK` is not asked. Duplicates are the node's to
  refuse (below), so a restart asks again without harm, and nothing is lost but by the 60-minute
  rule. The rule uses the end's `ts_ms`, the node's clock, at each check: an end the master could
  not resume within 60 minutes (it was down, the node away) is refused "it ended more than 60
  minutes before the agent could be resumed". The age is the master's own: when the mirror took
  the end (kept in memory per chat); after a master restart, the end's `ts_ms`, or now when that
  is in the future, from the first check on (review S3-5). So a node clock hours behind or ahead
  changes nothing, and the retries of one chat end 60 minutes after the master first saw the end.
  At the master's start only chats with a `background_ended` in the last 65 minutes are checked,
  and a check reads only the `background_ended` rows after the floor, in the SQL (S3-3).
- **Admission**, as §4.4 step 2 and exactly what the owner could do themselves at that moment: a
  chat **without an owner account** (made with the API token) is never resumed; nor a sub-agent's
  (`parent_id`); the account must exist, be enabled and owe no password change; then
  `workspaces::agentic_blocked` (grant, `agentic`, the local node rule) on the chat's effective
  workspace, as for the owner's own prompt. When its owner leaves a team workspace a chat falls
  back to their Personal workspace (`EFFECTIVE_WS`), where they may go on in it, so it is resumed
  there if they may run agentic chats there; nodes and folders are not workspace-scoped, so a
  removed member could open the same folder from Personal anyway. (A check of the workspace a
  command started in was built first and removed on the review's decision: it failed open three
  ways and guarded nothing the owner could not do by hand.)
- **Routine and untrusted chats** (§4.10 said routine chats are allowed; the step 3 brief says
  none): every routine run's chat is created with `no_callbacks` (`CreateSpec`,
  `session.create`), which the node records in `session_created` and holds to for good; the master
  also refuses a chat with a routine mark, and an untrusted one through `routines::chat_untrusted`
  (fails closed). It does not call `routines::on_owner_prompt`, which would mark a run's chat
  continued; it reads the same mark.
- **The node decides** (`Session::callback`), under the turn slot and the session's end gate
  (`bg_gate`): an end is checked against the slot and appended under the gate, and a turn holds it
  from its history read to its `turn_started` (a person's turn in `begin` too), so an end whose
  check passed just before the slot was taken is in the log the turn reads, and every later one
  is queued for the turn (review S3-4; as first built the end could land between the read and
  `turn_started`): every id
  must name an end in its log that may call back, with no turn and no `callback_refused` after it
  (else "nothing to resume", recorded nowhere; a turn already running is that too); then, on its
  rules, refused and recorded as `callback_refused { bg_ids, reason }`: a `no_callbacks` chat, a
  table a revocation closed (until the owner writes), `callback_turns = 0`, or `callback_turns`
  callback turns since the last `user_message`. A node that is stopping refuses with
  `NODE_STOPPING`. Then `turn_started { cause: { callback: [ids] } }` and the turn, with **no
  `user_message`**, the session's own mode (clamped at load), its table not reopened, **no
  sub-agents** (the spec's delegates cleared) and the cap `min(max_steps, callback_max_steps)`
  (`TurnExtra.max_steps`; the end says "stopped after N steps: a turn nobody started may take no
  more (`callback_max_steps` in node.toml)"). It takes a checkpoint like every turn.
- **Off is silent.** A node with `callback_turns = 0` does not announce `CAP_CALLBACK`, so the
  master asks nothing and raises no row, and the chat records nothing: the end waits for the
  person as before step 3. Asked anyway, the node still refuses and records why.
- **Memory** (review S3-1): `file_proposal` files nothing for a `remember` call whose turn's
  `turn_started` has a `cause` (read from the mirror, so a restart or a refused message changes
  nothing; it fails closed), a callback leaves the chat's recorded turn memory alone, and a
  person's prompt the node refuses puts the earlier entry back.
- **The demo provider never obeys program output** (review S3-2): it answers a callback turn's
  opening "Noted: the command ended." Only `mock_obeys_output = true` beside `mock_delay_ms` in
  config.toml (`mock::Obedient`, set by the step 3 tests and `web/e2e/callbacks.cjs`) makes it read
  the first quoted line of output as the message: the steered model the tests need.
- **Known, not fixed (S3-6):** an owner disabled in the milliseconds between admission and
  `turn_started` gets one callback turn, bounded by `callback_max_steps`: the same shape as the
  owner's own prompt racing a disable.
- **`callback_refused`** is a new event kind (not in §4.8): the chat says "bg-4 ended (exit 1); the
  agent was not resumed: 3 turns in a row without you (node limit)" from it, after a reload too;
  the model never reads it. Recorded once per refusal: a second ask for the same ends finds them
  covered.
- **What the model reads.** Not §4.4's frame (its `<output>` block is what review T8D-3
  replaced): one user-role message, `ChatMsg::Opening`, whose first line is the harness's
  (`CALLBACK_LEAD`, "[harness] The person has NOT written anything. The harness started this turn
  because background command(s) bg-3 ended while the chat was idle; …") followed by the ended
  commands' notes exactly as step 1 renders them (every output line behind `| `, control
  characters and line separators replaced). `provider::open_callback` takes the notes at the end
  of the history into it; `history_to_messages` does the same at the turn's `turn_started` on
  every later request, from the log alone, so the bytes are the same (cache prefix; also after a
  restart). `place_notes` sends it as a user message; `user_text()` is `None` for it, so it is not
  URL provenance, not `own_words`, and the summariser writes it without "User:". **Where the
  bytes are not the same:** a later compaction whose cut falls at a callback turn's
  `turn_started` puts the ended commands' notes into the summary, and the rebuilt opening is then
  the lead line alone (the cache prefix moves at a compaction anyway).
- **Every place that reads "the last user message"**, and what it does with a callback turn:
  `observed_text` (URL provenance): not provenance. The loop's `own_words`: empty. The node's
  budget: refilled by `user_message` only, which a callback turn has none of. `ui::check_reply`
  and the store's `open_message_author` (an answer to a `ui:` question is still taken after a
  callback turn: the person has not answered yet). The search index: indexes `user_message` and
  `assistant_message` text; the opening is no event, so it is never indexed as the person's; the
  callback turn's answers are the assistant's, as usual. Memory: no recall, no `remember`, the
  turn's memory recorded as none (`file_proposal` files nothing). Titles: nothing derives one from
  a message. The mirror's `asks_user`: a callback turn ends the previous turn's question mark, and
  one ending with a question makes its own. Routines: the watcher binds only a run chat's first
  turn, and run chats get none. The UI: `turnStart` (a user message or a callback) bounds a turn
  for the live part of the timeline, fallback notes, the agent's face and progress blocks; a
  question block is answered only by a user message; the link provenance (`rich.ts`) counts user
  items, which a callback is not; the web notice of a question counts hosts since the person's last
  message, so a callback turn's question names the previous turn's hosts too (more notice, not
  less). The demo provider (`mock`) answers it "Noted"; only `mock::Obedient` (tests) reads
  its first quoted line as the message.
- **Attention.** One kv row per chat (`callback-attention:<sid>`, owner id inside): `resumed` with
  the ids and the count of callback turns since the person's last turn, or `not_resumed` with the
  reason; `GET /api/attention` puts them in `items` (`reason` `resumed` or `not_resumed`, and a
  `detail` in words) for the owner only (the API token sees all, as for the other items) while the
  owner's `seen` is below its `seq`. Opening the chat marks it seen (the Shell's effect now takes
  every row but an approval); a person's turn and the chat's deletion remove it.
- **The chat list** gives each row `background`, the commands the mirror shows running whose
  deadline has not passed, from memory (rebuilt at the master's start from chats updated in the
  last 65 minutes); the sidebar draws a ring for it, and a running turn's pulsing dot wins.
- **The UI**: `Item::Callback { turn_id, bg_ids }` stands where the message would ("Started by
  bg-3 ended (exit 1), not by you"); `Item::CallbackRefused` is an error line; both folds and the
  shared fixture. First load 99.6 KB (99.5 before), total 370.5 KB, unchanged; no new chunk.
- **Cost bound** (the README says it): after each of the person's messages, at most
  `callback_turns × callback_max_steps` steps without them, 60 at the defaults. A step is one
  model answer and may carry several tool calls, so 60 steps is not 60 commands; on top come a
  call retried after a passing provider error, the agent's fallback models (more calls in a
  step), and up to two summarising passes a turn (one model call per piece). No sub-agents.
  `callback_turns = 0` turns it off on the node, silently (review S3-7).

**Tests.** `cli/tests/callbacks_real.rs` (the real binary: one turn per end, exactly once each;
the budget with a self-restarting command and the refill; the step cap; `callback_turns = 0`;
`ask` parks and the person's message is refused), `cli/tests/callbacks.rs` (the node over the
protocol: two ends one turn, a second ask, another chat's id, a forged id, a stop, a deleted chat,
a routine's chat, a revoked chat; the opening at the provider with a forged header, the separator
and text for the model, rebuilt the same on the next message), `master/tests/callbacks_http.rs`
(ends together, stops and restarts, attention for the owner only, the 60-minute rule, the node's
refusals, a removed owner, a disabled account, a restart of the master), `master/tests/
hooks_http.rs` (a routine's chat, an untrusted one after its routine is gone),
`agent-native/src/callback_tests.rs` (the opening, provenance, the cap), `core` (wire shapes,
`calls_back`, the fold), `node/src/policy.rs` (the keys), `web/e2e/callbacks.cjs`. §7 test 9 with
the watcher on is `cli/tests/background.rs`'s existing `after_kill_9_…`.

**Owner leaves a team workspace** (found while building step 3, confirmed by the review, and
decided as the code does): the chat falls back to the owner's Personal workspace and the
revocation sweep (`cancel_blocked`) finds them not blocked there, so their background commands go
on and the chat may be resumed, as the owner could go on in it themselves; only a lost grant, a
disabled or deleted account, or a Personal workspace without `agentic` stops them. §4.7's table
says so now.

**Not done:** the start call's card states (§12); a per-workspace share of the budget; any
notification beyond the attention row (no browser notification for a callback turn).


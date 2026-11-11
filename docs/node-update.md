# Node update and drain: design note

Status: **proposal of 2026-11-12, for the owner's sign-off.** Nothing built. Task C.14 of
`ADOPTION_PLAN.md`, from the owner's direction of 2026-11-12. A feature, built after Gate 3 of
`FIX_PLAN.md`, except the admission reason (§7, step 1), which fixes built behaviour. The version
requirement (§2) can do nothing useful before the first tagged release; the drain (§3) can.

**Summary.** The master may say which version it requires of a node, and by when. It never sends
code, a URL or a command. Where a new version comes from is the node owner's choice, in
`node.toml`. When a node restarts, for an update or because its supervisor stops it, it
**drains**: no new turns; each running agent is told and chooses to finish or to pause with a
summary; after five minutes an agent that has not answered is no longer waited on; at thirty
minutes what is left is cancelled through the normal cancel path; the node exits and its
supervisor starts whatever binary is installed. Each affected chat then says what happened and
can be continued with one action.

Claims about code were read on `main` at `f3bf1bc`.

## Terms

- **Requirement**: the master's "this server requires node version ≥ V", *recommended* or
  *mandatory* with a *deadline* after which older nodes are refused at admission.
- **Drain**: the node's own shutdown sequence (§3). Started by the node, never by the master.
- **Step boundary**: after one model call and its tool results are recorded, before the next
  (the loop at `crates/agent-native/src/lib.rs:1000`), or the end of the turn.
- **Responded**: after the drain note, the agent completed one model call (§3.2).

## 1. What exists, and what this must not bring back

| Area | On `main` | File |
|---|---|---|
| Versions in the handshake | `node.hello` carries `protocol_version` (the constant `1`) and `version`, the crate version. `master.welcome` carries `protocol_version` only. | `crates/protocol/src/lib.rs:10`, `:79-88`, `:130`; `crates/node/src/lib.rs:271-285` |
| Mismatch at the master | `add_link` refuses `protocol_version != 1`, but **after** the link sent `Accepted`, so the reason never reaches the node: it logs "peer closed the link" and reconnects with backoff up to 30 s, forever. | `crates/master/src/lib.rs:806-810`; `crates/master/src/nodes.rs:200-216`; `crates/link/src/client.rs:52-63` |
| Mismatch at the node | A different version in `welcome` is a warning. | `crates/node/src/lib.rs:535-539` |
| A refusal that reaches the node | `ServerMsg::Rejected { reason }` during authentication: the client stops and `node run` exits with the reason. | `crates/link/src/wire.rs:24-28`; `crates/link/src/client.rs:56` |
| Unknown message | A frame that does not parse closes the link; unknown *fields* are ignored. | `crates/link/src/pump.rs:61-62` |
| Version string | `0.1.0` in every build; CI checks that a release tag equals it. | `Cargo.toml:6`; `.github/workflows/ci.yml:82-86` |
| Stop signal | `serve` and `node run` stop on SIGINT only; SIGTERM kills. The README unit sets `KillSignal=SIGINT`, `Restart=always`; the image `STOPSIGNAL SIGINT`. No stop closes running turns; the next start does. | `crates/cli/src/main.rs:413`, `:616`; `README.md:979-983`; `Dockerfile:37-38` |
| After a crash | `recover_interrupted` denies pending approvals (`interrupted`), writes an error, ends the turn `interrupted`. | `crates/node/src/session.rs:187-238` |
| Cancel | Bash gets SIGTERM to its group, SIGKILL 2 s later; a tool still running 5 s after cancel is abandoned. Sub-agents run under the parent's token. | `crates/tools/src/bash.rs:24`, `:240-252`; `crates/agent-native/src/lib.rs:44`; `crates/node/src/session.rs:1476-1480`, `:1508-1517` |
| Restore | Refused while a turn runs; an interrupted restore is recoverable from its `before_restore` event. | `crates/node/src/session.rs:886-930` |
| Routines | An offline node is retried after 1, 5, 15 minutes; a node *refusal* blocks the routine; `interrupted` counts toward the five failures that pause it. | `crates/master/src/routines/runner.rs:47`, `:724-740` |
| Containers | Nodes are not containerised by design; the image is the server and runs no agentic node. | `Dockerfile:1-3`, `:34`; `.github/workflows/ci.yml:104` |

`MasterToNode` (`crates/protocol/src/lib.rs:126-263`) has nothing about versions or shutdown.

**Removed earlier.** Commit `1fa8f27` (`FIX_PLAN.md` 1.2, 1.4, Q2, Q3; `AUDIT.md` S13, D4, L2)
deleted SSH node setup, in which the master logged into servers as root, uploaded its own binary
and rewrote firewall and sshd, and the HTTP endpoints that installed graphify from PyPI and
rewrote `node.toml`. A test keeps those routes gone (`crates/master/tests/settings_http.rs:346`);
`PLAN.md` T5 says "no endpoint writes it or installs software". An update channel from the
master would be the same thing by another door: whoever holds the master would choose what runs
on every node. Two leftovers are stale: `Dockerfile:4-5` and `AUDIT.md:502` still say the master
uploads its binary. And `FIX_PLAN.md` 5.11 plans release assets "verified against a hash the
master pins": a hash the master pins is code the master chooses. If 5.11 is built, the trust
root must be on the node (§2.3).

## 2. The requirement: stated by the master, met by the node's owner

### 2.1 What the master may say

`min_version` (semver, compared with the node's `CARGO_PKG_VERSION`); `mandatory`; `deadline_ms`
(with `mandatory`, at least 24 hours ahead); `note` (200 characters, cleaned like hostnames,
`crates/master/src/lib.rs:800-804`, shown to people, never to an agent). Nothing else: there is
no field for a URL, digest, binary, script or command, and no code path from this message to a
network call, a process start or a file write (§6, test 1).

A server admin sets it (Settings → Server → Nodes); the master also derives a mandatory one when
it drops an old protocol (§5.1). It is stored on the master and sent at every admission, so a
node that was offline receives it at its next connect.

### 2.2 What the node does with it

It arrives only over the authenticated link from the paired master; there is no other channel.
A `min_version` at or below the running version is ignored: a node never moves to an older
version because the master says so. Otherwise the node records it and acts by its mode:

| Mode | What the node does | Status |
|---|---|---|
| `off` | Logs it once per connect; serves until refused. | Build |
| `notify` | As `off`, and the master shows it to server admins and to the admins of every workspace the node is attached to. | Build, **default** |
| `restart` | As `notify`; and when the binary at the install path is a version ≥ `min_version`, the node drains (§3), exits 75, and its supervisor starts the new binary. No newer binary: as `notify`. | Build, opt-in |
| `self` | Fetches, verifies and installs a release itself. | **Not built**; `node run` refuses to start with it. |

**Why `restart` waits for a newer binary.** The requirement brings no new version onto the
machine. A node that restarted on the requirement alone would restart into the same binary, be
refused again, and have interrupted everyone's work for nothing. "Newer binary installed" means
the owner's packaging did its job (apt, Nix, a copied release, a timer of their own). How the
node reads the installed file's version is **not established**: running it with `version` adds
no trust (the supervisor will run it anyway), but `std::env::current_exe` may name a replaced
file with a `(deleted)` suffix. Test 13 settles it.

### 2.3 `self`, bluntly

It makes the node a software installer with a key-management problem. First would have to
exist: tagged releases, assets per architecture (`FIX_PLAN.md` 5.11), signatures (none today,
`AUDIT.md:534`), a key pinned in `node.toml` with rotation and revocation, and a rollback when
the new binary does not start. One cost no tooling removes: the node would have to write its own
binary. The README unit keeps it root-owned in `/usr/local/bin` under `ProtectSystem=strict`
(`README.md:972-990`), so an agent that escapes the sandbox cannot make itself permanent; a
self-updating node gives that up. If wanted, self-update belongs in a separate root updater that
verifies and replaces the file, and `restart` mode picks the result up. That is packaging, which
we can document instead of owning.

### 2.4 Defaults per install kind

- **By hand, with the README's systemd unit:** `notify`. `restart` helps only once the owner has
  something that installs binaries; they opt in.
- **A node in a container:** not a supported kind (`Dockerfile:1-3`). Whoever does it uses
  `notify` and the runtime's own image update, whose stop triggers the drain (§3.1).
- **The built-in `local` node:** no mode. It is the master's process and version
  (`crates/cli/src/main.rs:376`), so no requirement applies; updating it is restarting the
  server (§5.2).

### 2.5 Settings

```toml
[update]
mode = "notify"               # "off" | "notify" | "restart"; "self" is refused at start
ask_minutes = 5               # the agent has this long to answer the drain note
force_after_minutes = 30      # from the start of the drain to the forced cancel
hard_stop_seconds = 60        # bound on the clean cancel before the process exits anyway
min_minutes_between = 60      # no new update drain within this long after one
```

Checked at start: `ask_minutes` 1 to 30, `force_after_minutes` from `ask_minutes` to 1440,
`hard_stop_seconds` 10 to 300. **The master can change none of them**, shorter or longer: its
deadline is when it stops admitting the node, not a timer on the node. The hard bound is 60 s, not
a few seconds, because the after-turn checkpoint took up to 15.5 s on B.3's slow disk
(`docs/checkpoint-cost.md`).

## 3. The drain

### 3.1 What starts one

| Trigger | Who | Exit status |
|---|---|---|
| SIGTERM, or a first SIGINT | The owner or the supervisor (`systemctl restart`, `podman stop`, Ctrl-C) | 0 |
| A requirement, in `restart` mode, with a newer binary installed | The node | 75 |
| A second signal during a drain | The owner | hard stop now |

The signal trigger is most of the value: every restart becomes graceful, for any reason, on every
install kind, and it is how an owner says "update now" after installing a binary. It needs
supervisors that wait. systemd kills after 90 s by default and Docker after 10 s, so the README
unit gains `TimeoutStopSec=35min` and drops `KillSignal=SIGINT`, and a compose file sets
`stop_grace_period: 35m`.

### 3.2 The note and the agent's answer

At the drain's start the node appends `node_draining` (§4.2) to every session with a running
turn. From the next step boundary the model sees a fixed note from the harness, neither a user
message nor tool output:

> This node must restart (update to 1.3.0 / stopped by its owner). If you are nearly done,
> finish. Otherwise stop at a safe point: call `drain_answer` with `pause` and say what remains.
> Work still running at 14:32 will be cancelled.

The tool `drain_answer { choice: "finish" | "pause", remaining?: string }` is added to that
turn's tools from then on and exists nowhere else (one prompt-cache miss per turn per drain).

| The first model call after the note | Outcome |
|---|---|
| calls `drain_answer` with `pause` | **Pause.** No other call in that message runs; the turn ends `completed` with `drain.outcome = "paused"`; `remaining` is in the recorded call. |
| calls it with `finish`, or calls other tools, or ends the turn | **Finish.** The turn runs to its end, bounded by the forced point. An agent that ignores the note has chosen finish by acting. |
| has not completed within `ask_minutes` | **Not answered** (§3.3). |

### 3.3 Timeline

| From drain start | What happens |
|---|---|
| 0 | New turns, sessions, restores and artifact stages are acked `ok: false`, `error: "draining"`, with `retry_after_ms`; the UI shows a notice, not an error ("N is restarting for an update, back in about 30 minutes"). Approval answers and cancels are still taken. The node sends `node.draining` (§4.1). |
| 0 to 5 min | Each agent gets the note at its next step boundary and answers. |
| 5 min | A turn that has not answered is no longer waited on. **If it is inside a tool call, that call is stopped** (the tool's own cancel, output kept) and returns "stopped: the node is restarting", so the agent reaches a boundary with the note in front of it and answers. The turn is not cancelled. A turn waiting on its provider is left to the provider's timeouts; one waiting on an approval, §3.4. This is Q3. |
| 5 to 30 min | Graceful phase: nothing is cancelled. When the last turn has ended and no restore runs, the node exits at once. |
| 30 min | **Forced.** Every remaining turn is cancelled through the normal path: process trees killed within the existing grace, approvals denied, the turn closed with `drain.outcome = "cut"`, the after checkpoint taken if it can be. |
| + 60 s | A cancel that has not closed its turn (a stuck store, a hung git call) is abandoned and the node exits. The turn stays open in the log; the marker (§4.3) says why; the next start closes it with `recover_interrupted`, naming the update. Whether tool processes survive this is C.11's question (B.1). |
| exit | Up to `hard_stop_seconds` for the master to acknowledge what was sent; session containers stopped; the marker removed if every turn closed. A hand-started node prints "drained; start the new binary". |

### 3.4 Cases

- **Sub-agents.** A child runs under its parent's token, and the parent is blocked in the
  delegate call, so the parent cannot answer while its child works. Children get the note too; a
  child's `pause` returns its summary to the parent as its result; **the parent's answer then
  decides for the whole turn.** At the forced point, cancelling the parent cancels the children.
- **An approval waiting on a person** is not the agent responding. At 0 it stays open and its
  card shows the forced time; a person may still answer, and an allowed command runs until the
  forced point. At 5 minutes nothing changes for it. At 30 it is denied, `resolved_by:
  "interrupted"`, as `close_open_turn` does (`crates/node/src/session.rs:215-222`).
- **Routines.** Any node refusal blocks a routine today (`runner.rs:735-740`), so the runner must
  treat `draining` like an offline node and retry at `retry_after_ms`. A running routine turn is
  asked like any other; paused or cut, the run records `interrupted` with the reason, **not
  counted toward the five failures**, and is not re-run (agentic side effects,
  `docs/routines.md`).
- **A restore in progress** is never cut by the node: the drain waits for it, past the thirty
  minutes and past the hard stop. If the supervisor kills the process anyway, its
  `before_restore` event lists the paths it may have written and a second restore puts them back.
- **Unacknowledged events.** On `main`, `node.db` is the log and the next hello and `resume`
  replay what the master lacks. With the spool (`docs/nodes-credentials-environments.md` §4.12)
  it is fsynced on append, survives the restart and is sent at the next hello.
- **Containers (N.4).** The node stops each session's container when its turn ends; at start it
  stops any labelled `rizzy.ws` container it has no session for.
- **Several workspaces.** One drain per node; every attached workspace's admins are told.
- **A restart during a drain.** The marker survives; the next start closes open turns naming the
  update.

### 3.5 Abuse

- **Repeated requirements.** A requirement alone never drains a node: `restart` also needs a
  newer binary the owner installed, after which the requirement is met; `min_minutes_between`
  is a second fence. A master can still refuse a node after a deadline and cancel any session,
  which it can already do (revocation, `session.cancel`).
- **A prompt-injected agent** cannot start a drain (no tool or message exists outside one),
  cancel it (no API) or extend it (the node fixes the forced time at the start; `finish` does not
  move it). Text imitating the note can make an agent stop early, which injected text already
  can. `remaining` is model text, rendered like any assistant message.
- **Another person on a shared node** cannot trigger one: only a signal or the node itself can.

## 4. What changes elsewhere

### 4.1 Protocol (nothing carries code)

| Change | Direction | Older peers |
|---|---|---|
| `Hello::Auth` gains `version`, `protocol_version` | node to master | an old master ignores them |
| `master.welcome` gains `update: Option<Requirement>` | master to node | an old node ignores it |
| New `node.update_required { requirement }`, while connected | master to node | sent only to nodes listing the new capability `update` (as `CAP_ARTIFACTS`, `crates/protocol/src/lib.rs:22-25`) |
| New `node.draining { drain_id, reason, started_ms, ask_until_ms, force_at_ms, turns }` | node to master | sent only if the welcome announced it: an old master would drop the link |
| `ack` gains `retry_after_ms` | node to master | ignored |

### 4.2 Events (a schema change, allowed before the first tag)

- **New kind `node_draining { drain_id, reason, required_version?, ask_until_ms, force_at_ms }`**
  in each session with a running turn. No existing kind fits: the note is neither the person's
  message nor an error, and the fold must replay it to the model the same way after a restart.
- **`TurnEnded` gains `drain: Option<{ drain_id, outcome: "finished" | "paused" | "cut" }>`**
  beside `reason` (`crates/core/src/event.rs:512-517`): finished and paused end `completed`, cut
  ends `cancelled`, or `interrupted` when the next start closed it.
- The rest uses existing kinds: tool-call events for `drain_answer`, `approval_resolved` with
  `interrupted`, `error` naming the update.

### 4.3 The marker

`<data>/drain.json` (id, reason, version, start), written at drain start, removed at a clean exit.
`recover_interrupted` reads it to write "The node restarted for an update to 1.3.0 while this
turn was running" and set `drain.outcome = "cut"`. If it cannot be written, recovery falls back
to today's message.

### 4.4 In the chat after the restart

One line per drained turn: "Node N restarted (update to 1.3.0, 14:32). This turn finished /
paused: ‹remaining› / was cut off after 30 minutes." Paused and cut turns show **Continue**, which
sends a fixed prompt as the person's own message. Nothing resumes by itself (turns are not
resumed, `PLAN.md` §2). OpenCode injects a restart message and resumes automatically, up to ten
attempts (`research/round2/synth/05-links-workers-protocols.md` §2, OpenCode, item 8); we take
the plain message, not the automatic resume.

## 5. The master, `local` and supervisors

### 5.1 Version gate and skew

OpenClaw refuses a worker whose build differs and OpenHands one older than a minimum (`05` §6.9).
Ours, at authentication and **before `Accepted`**: a protocol outside the master's range, or a
version below a mandatory requirement past its deadline, gets `Rejected { reason }`, which the
node logs and exits on. Today's check after `Accepted` goes. Otherwise the node is admitted with
the requirement in `welcome`.

**Skew.** A node must understand "update required" and the drain before it can be asked to
drain, so the first release that has them is installed by hand everywhere. After that the master
must admit nodes one protocol version behind until a deadline, or it cannot tell them anything.
That is compatibility code, which `CLAUDE.md` forbids before the first tagged release, and until
then every build is `0.1.0` (`Cargo.toml:6`). So **before the first tag the requirement side has
nothing to do**: a protocol change is a refusal at admission, now with a reason. The drain on a
signal is useful from the day it lands. **Upgrade order: master first, then nodes.**

### 5.2 `local` and `serve`

A server update restarts the master, and with it `local` and the in-process chat runtime
(`crates/cli/src/main.rs:376`, `:384`). The operator initiates it (a package upgrade and
`systemctl restart`, `docker compose up -d` with a new image, Podman's auto-update); the master
never restarts itself. On the stop signal `serve` drains `local`'s and the chat runtime's turns
**while still serving HTTP**, so people see the drain and can answer approvals; remote nodes keep
working. Then HTTP gets its existing 30 s grace (`crates/master/src/serve.rs:62`). The image runs
no `local` (`Dockerfile:34`), so there only chat turns drain, and they usually end in seconds.

### 5.3 Supervisors

| Install kind | Settings | Result |
|---|---|---|
| systemd (README unit) | `KillSignal` default (SIGTERM), `TimeoutStopSec=35min`, `Restart=always` | `systemctl restart` drains; a `restart`-mode drain exits 75 and systemd starts the new binary. |
| Podman auto-update (server image only) | a 35-minute stop timeout | its stop drains `serve`. |
| Hand-started | none | the node exits and says so. |

### 5.4 Invariants

**The node decides:** mode, durations and trigger are in `node.toml`; the master states a version
and a deadline and can only refuse admission, which it already can. **The master never runs
tools**, nor an installer. **One writer:** `node_draining` and the outcomes are appended by the
session actor. **Nothing the model writes is trusted:** `drain_answer` picks finish or pause for
its own turn only. **No compatibility code before the first tag:** the requirement side waits
(§5.1). No secret is in any new field; the cost in memory is a timer and a state per drain.

### 5.5 Threat-model row (draft, for `PLAN.md` when built)

| # | Threat | Mitigation |
|---|---|---|
| T34 | A compromised or careless master uses "update" to run code on nodes or to keep them from working; a prompt-injected agent starts, cancels or stretches a restart | **The master states a version, never delivers one** (`[update]` in `node.toml`; no field for a URL, binary or command; `self` refused at start). A node never moves to an older version and takes a requirement only from its paired master over the link. A requirement drains a node only in `restart` mode, only when a newer binary is already installed, at most once per `min_minutes_between`; after a deadline the master can only refuse admission, as revocation already can. The drain's durations are in `node.toml` and cannot be changed over the link; the agent's only input is finish or pause for its own turn, bounded by `force_after_minutes`; the node never cuts a restore. Tests: `docs/node-update.md` §6. **Residual:** an owner who installs a bad binary gets a restart loop; there is no rollback. |

## 6. Tests the build commits to

1. `a_requirement_with_url_binary_or_command_fields_does_nothing_else`: the fields are ignored; no process, connection or file write follows.
2. `a_requirement_for_a_lower_or_equal_version_is_ignored`: no notice and no drain, in every mode.
3. `a_requirement_from_an_unpaired_sender_is_refused`: a connection that fails authentication never reaches the requirement code.
4. `new_turns_are_refused_during_a_drain_with_the_reason`: ack `draining` with `retry_after_ms`, shown as a notice.
5. `a_paused_turn_ends_with_its_summary_and_continues_after_restart`: `paused`, the summary logged, Continue starts a turn on the restarted node.
6. `an_agent_without_a_step_boundary_is_moved_on_after_ask_minutes`: a `sleep 3600` is stopped at the mark and the agent then gets the note.
7. `the_forced_point_cancels_cleanly_and_names_the_update`: no process of the tree remains; `turn_ended` is `cancelled` with `cut`.
8. `a_restore_in_progress_is_not_cut_by_the_drain`: the node exits only after the restore, past the forced point.
9. `unacknowledged_events_survive_the_restart`: events not acknowledged before exit reach the master afterwards.
10. `repeated_requirements_cannot_keep_a_node_from_working`: one per second with no newer binary, no drain; with one, a single drain, then none within `min_minutes_between`.
11. `an_agent_cannot_start_or_extend_a_drain`: no `drain_answer` outside a drain; `finish` does not move `force_at_ms`.
12. `a_protocol_mismatch_is_refused_before_accepted_with_the_reason`: the node logs it and exits instead of reconnecting forever.
13. `restart_mode_reads_the_installed_binary_not_the_running_one`: after the file is replaced, the node sees the new version.
14. `a_draining_refusal_does_not_block_a_routine`: the run is retried after the drain; the routine is not paused.
15. `a_hard_stop_leaves_a_turn_that_the_next_start_closes_naming_the_update`: the marker drives the message.
16. `an_approval_left_open_is_denied_at_the_forced_point`: `resolved_by: "interrupted"`.

## 7. Build order and size

| Step | What | Size | When |
|---|---|---|---|
| 1 | Protocol check before `Accepted`, `Rejected` with the reason (test 12) | **S** | Fixes built behaviour: may go before Gate 3 |
| 2 | Drain on SIGTERM and SIGINT: phases, refusals, cancel, marker, events; README and compose timeouts (tests 4, 7, 8, 9, 15, 16) | **L** | After Gate 3 |
| 3 | The note, `drain_answer`, pause, Continue (tests 5, 6, 11) | **M** | With 2 |
| 4 | Routines treat `draining` as a retry (test 14) | **S** | With 2 |
| 5 | Requirement: storage, admin setting, `welcome`, `node.update_required`, `off`, `notify` (tests 1 to 3) | **M** | After the first tagged release |
| 6 | `restart` mode (tests 10, 13) | **S** | After 5 |
| 7 | `self` | not planned | §2.3 |

About two to three weeks in all. Nothing from the nodes design is needed first: steps 2 to 4
work on today's `node.db`. N.1 then adds the spool flush and notices per attached workspace, and
N.4 stopping and reconciling containers, each with its test.

## 8. Questions for the owner

| # | Decision | Recommended | If you choose the other |
|---|---|---|---|
| Q1 | Which modes, and the default | **`off`, `notify`, `restart`; default `notify`;** `restart` acts only when a newer binary is installed. | `restart` on the requirement alone: a restart into the same binary, refused again, everyone's work interrupted for nothing. Default `restart` with the rule: harmless, but nothing happens on most hand installs. |
| Q2 | `self` update before signed releases | **No, and never inside the node:** after signed releases, a separate root updater if anyone asks. | The node user must own its binary, so an escaped agent can make itself permanent; plus keys, rotation and rollback to own. |
| Q3 | An unanswered turn's running command at five minutes | **Stop that command, not the turn,** so the agent answers with the note in front of it. | Leave it running: the five-minute mark changes only a label, and that agent never chooses before the forced point. |
| Q4 | May the master cut a node before its drain completes | **No.** A deadline applies at admission only. | The link closes at the deadline: turns run on, but nobody can answer approvals or see progress until the forced point. |

## 9. Not settled

- How the node reads the installed binary's version (test 13).
- Whether tool processes survive a hard stop (C.11, experiment B.1).
- Podman auto-update's stop behaviour with a long stop timeout: to be checked against its manual
  before the README changes.

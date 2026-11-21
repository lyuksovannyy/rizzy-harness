# Execution modes and levels (N.10a)

Decision O13 (the owner, 2026-11-12 to 2026-11-14; `ADOPTION_PLAN.md`,
"Execution levels"): who may run what on a node is one ordered level,
`none` < `container` < `sandbox` < `host`, under the node's own ceiling, and
every check is `session mode <= effective level`, with the node refusing any
mode above its own `[execution]` ceiling. Part 1 (below, up to "The master")
is the node's side and the session's mode; part 2 ("The master: levels") is
the account's ceiling and the level per member, which replaced the
account-wide `can_agentic` switch, the role-derived `agentic` permission and
part 1's interim rule. The `container` mode (rootless Podman, N.4) does not
exist yet: `container` is a level that allows no mode until it does.

## The modes

| Mode | How a command runs | What still holds |
|---|---|---|
| `sandbox` | `sandbox-guard` → `bwrap` (its own PID namespace, its own file view built from the grants, `/proc` of its own where it can be mounted) → `sandbox-exec` (Landlock) → `bash` (T.8a to T.8f) | For the shell: nothing a command starts outlives it (from Landlock ABI 9; below, the socket gap `doctor` names); the file view; with `network = "off"`, a network namespace of its own; background commands. **Not** contained: what the node itself starts for the chat, under Landlock alone: `graph_query`'s graphify (review N-a, unchanged here: it runs `graphify update .` over the files the chat wrote, as the node's user), the node's checkpoint git and PDF reader |
| `host` | `sandbox-exec` (Landlock, as `[sandbox] mode` says) → `bash`, as the node's OS user, no bubblewrap; with `[sandbox] mode = "off"` bash directly | Landlock's file rules where `[sandbox] mode` is `required` or `best-effort`: the session folder, its temp folder, the system folders and the `[sandbox] read`/`write` entries; TCP denied with `network = "off"`. **Not** held: no PID namespace (what a foreground command starts can keep running after the call, the chat and the node; a background command, allowed since 2026-11-20, runs under the guard without bubblewrap, which ends it and what it started and confirms that, but not what it only asked another service to start), no file view (`/proc` lists every process, metadata of every path is visible), UDP and DNS go out with the network off, and below Landlock ABI 9 any service of the node's user reachable over a Unix socket (the session bus: `systemd-run --user`) runs anything with every right of that user. Background commands are allowed (the owner's decision of 2026-11-20, replacing review N-c's refusal): the session has a background table and its command runs under the node's guard without bubblewrap (`sandbox-guard --host`; PLAN.md T9), refused where the node has no sandbox wrapper to start it from (`[sandbox] mode = "off"`). Any approval mode, `auto` included, within `max_approval_mode` (T50: review N-e's clamp to `ask` was removed by the owner's decision of 2026-11-14, in their words: "default accept mode is Auto for every chat type, no limits for apart chat types or agentic host access modes, so each can use auto, always accept, manual, etc..."); new chats start in `auto`, so a host chat on Auto runs every command as the node's user with no question asked and nothing stands between a prompt-injected agent and the machine. Its approval card, in Ask, says it runs in the host shell (review N-e) |

Landlock alone counts as `host`, not `sandbox` (the owner, 2026-11-13): it
gives no PID namespace and no confirmed kill, which the rest of the sandbox
work relies on. A node inside the published image (no bubblewrap, no
Podman) therefore offers agent chats only with `[execution] host = true`.
The warning and the README describe `host` as "nothing between the commands
and the machine", although Landlock's file rules may still apply: they are
not a wall a steered agent cannot get around on most kernels in use (the
session bus above), and nothing relies on them for `host`.

## The ceiling: `[execution]` in node.toml

```toml
[execution]
sandbox = "auto"   # auto | strict | off
host = false       # the unconfined shell; off unless the node's admin turns it on
```

What a node offers is computed once, at start, from this table and its own
self-test, never from anything received (`ExecOffers::from_start`,
`node/src/exec.rs` `ExecOffers`):

* `sandbox` is offered only where the containment probe passes at start.
  `auto` offers it with its gaps named (below Landlock ABI 9, the
  network-off gap, the shared-`/proc` form, an image not held, the binary
  in a session's reach, a `[sandbox] write` entry holding other programs'
  sockets or a folder on `PATH`); `strict` only where none of these is
  left (review L-c; of the socket folders only `/tmp` and
  `/run/user/<uid>` are looked for: a socket elsewhere in a write entry is
  not detected, so `strict` does not promise that one); `off` never
  (bubblewrap is not probed). The shared-`/proc` form counts under `auto`:
  every process still dies with the namespace and the file view is the
  same; what it loses is that `/proc` lists the outer processes. It is the
  form bubblewrap takes inside a default rootless container
  (`docs/container-experiment.md` §7), where the alternative would be no
  `sandbox` at all.
* **The start is the only moment that counts.** The probes run up to
  `START_PROBE_TRIES` (3) times, `START_PROBE_PAUSE` (1 s) apart, each
  failure logged (review L-b: one failure of the moment used to remove the
  mode until a restart); a node that fails all of them offers no `sandbox`
  until it is restarted. No later probe adds the mode. The cost (review
  RN-7): a probe that fails slowly fails three times as slowly at start,
  and a bubblewrap that hangs for good still hangs the start, as before
  the retries (the start's probes have no bound).
* `host` is offered only with `host = true`, and not where the Landlock
  sandbox is required and does not work (`[sandbox] mode = "required"`
  refuses the node in every mode, as before).
* A node that offers neither runs no agentic session and says so plainly
  at start (`node run` refuses to start; `serve`'s node logs it at `error`),
  in `doctor` (`FAIL execution modes: NONE …`) and in every refusal:
  "this node offers no way to run commands: install bubblewrap, or set
  `[execution] host = true` in node.toml and read what that means".

**Existing data directories do not open** (review RN-6). The master's
session table gained a column (`sessions.exec_mode`), changed in place
with no migration (none before the first tag): a data directory made by
an earlier build makes `serve` exit with "no such column: s.exec_mode".
Remove or recreate it before this build runs on it.

**The break.** Before, `contain = "auto"` with a failed probe ran the shell
with Landlock alone and only warned. It no longer runs commands: such a
node offers no `sandbox`, and unless its admin sets `host = true` it offers
nothing. `[sandbox] contain` is gone (no compatibility before the first
tag): `"auto"` and `"required"` are `[execution] sandbox = "auto"` and
`"strict"`; `"off"` was "run uncontained", which is now the `host` mode and
says so. The step up from Landlock alone to contained (review T8B-1,
re-check R-1) is removed with it: a node that could not contain at start
does not offer `sandbox` until it is restarted, so nothing would use a
containment found later. The step up from the network-off gap to the
namespace stays.

## The hello

The node announces `exec:sandbox` and `exec:host` for what it offers, and
the gaps of those modes (`harness_protocol::GAPS`):

* `gap:sandbox:netns`: network off, bubblewrap could not make a namespace
  at start (UDP and DNS go out; Landlock stops TCP);
* `gap:sandbox:sockets`: below Landlock ABI 9 (a socket in a granted
  folder reaches a service that starts processes outside);
* `gap:sandbox:proc`: the shared-`/proc` form;
* Background commands in a `host` chat on a node with `[sandbox] mode =
  "off"` (the owner, 2026-11-20: "dont, like if user selected host project
  workspace, why do we need to limit it? its his choice"): the guard is
  started directly from the node's own image, which `start` holds before
  anything else (no wrapper, no `@rizzy-image`); the foreground call is
  unchanged. A node that could not hold its image still refuses them.
* `gap:host:netns`: network off, a `host` chat, Landlock stops TCP (UDP and
  DNS go out);
* `gap:host:net-open`: network off, a `host` chat where nothing applies it
  (`[sandbox] mode = "off"`, or Landlock not enforcing the TCP rule):
  everything goes out (review L-d).

The master keeps the known ones in its node list (`exec_modes`,
`exec_gaps`) and shows them only to accounts whose ceiling runs agent chats
(review L-i); the chat header names the chat's mode and says its gaps (on hover) in
plain words (reviews T8B-3, L-e). **The hello is a snapshot** of the node
at connect: a gap closed later by the step up (the network namespace) is
still shown until the node's next hello.

## The session's mode

* `session.create` carries `exec_mode` as a request (`Option`). The node
  chooses (`ExecOffers::choose`): a mode it offers as asked; without one,
  `sandbox`, or a refusal on a node that offers only `host`: that mode is
  only ever given when asked for by name (review L-a); a mode it does not
  offer is refused whatever the master sends. Recorded in `session_created.exec_mode`; no
  later command carries it except the owner's switch (below).
* At load the mode is the log's (`harness_core::exec_mode`: the latest
  `exec_mode_changed`, else `session_created.exec_mode`; one rule for the
  node, the view, the master's mirror and the browser's fold). Before every
  turn, callback, restore and delegation (`Session::checked_folder`) it is
  checked against what the node offers now: a session whose mode is gone
  (bubblewrap removed, `host` turned off) runs nothing and says why. The
  node never moves it to another mode on its own, in either direction; its
  owner may (below). A mode the log names that this build does not know is
  unreadable and fails closed: nothing runs, and a switch does not repair it.
* Chat-profile sessions have no mode; a create that asks for one is refused.
* A sub-agent's session is created in its parent's mode as it is then
  (`run_child`; its `session_created` says so) and keeps it: one rule
  everywhere, a session's mode is what its own log says, which is also what
  the master's row and its level checks read. It has no switch. One made
  BEFORE its chat was switched takes no more turns once its mode differs
  from that of the chat at the top of its parents (`checked_folder`, every
  turn, retry, callback, peer call and restore; and the `delegate`
  follow-up): "this sub-agent ran in the sandbox and this chat's commands now
  run in the host shell: start a new sub-agent" (and the reverse); it takes
  turns again if the chat returns to its mode.
* The backend picks the confinement by the mode (`NativeConfig::
  confinement`): `sandbox` sessions get the node's contained sandbox, which
  refuses a shell command when it has no containment rather than run it
  with Landlock alone (`Sandbox::require_contain`); `host` sessions get its
  uncontained twin (`Sandbox::uncontained`: the same resolved rules and
  held image, no containment, and it can never gain one). The backend is
  given each mode's confinement only where the node offers that mode; the
  other refuses every command (`ExecOffers::confinement_for`, review L-f),
  so a log that names a mode the node does not offer starts nothing even
  if a check before the turn were missed. The node's checkpoint git uses
  the twin; graphify uses the session's own confinement through its
  non-shell path, which is Landlock alone in both modes (review N-a).
* MCP servers (X.3, PLAN.md T47) use the session's confinement through the
  shell path, as `bash` does: in a `sandbox` session each server is a
  contained command (guard, bubblewrap, its file view, its network rule)
  whose stdin is a FIFO the guard hands over (`guard::STDIN_FIFO_ENV`); in
  a `host` session it runs uncontained in its own process group. One per
  session, never for a sub-agent. A repository's approved server (X.36, T82) starts the same way
  in either mode (the owner, 2026-11-20); one whose entry has `env` values starts only in a
  `sandbox` session, the only place the values can be kept off the processes before it.
* The node's `session.create` audit line names the mode asked for (review
  N-d).

## Switching a chat's mode (N.10b)

Decided by the owner on 2026-11-21 ("Yes, build it"; "when chat is
active/running sandbox change is unavailable but if chat is stopped then no
problem"), reversing "chosen at creation, never changed" (N.10a).

**Who and when.** The chat's owner, for an existing agentic chat of a
person, while nothing of it runs: no turn of its own or of a sub-agent of
it (any depth), no `/compact`, no approval waiting, no background command
or sub-agent. Nobody else: not an admin, not a
workspace admin, not the API token on a person's chat (the 404 of a chat
that does not exist); not a chat without tools, a sub-agent's chat (it
follows its parent) or a routine's (always `sandbox`); not a chat nobody
owns.

**The route** `POST /api/sessions/{id}/exec-mode` `{ "mode": "sandbox" |
"host" }` (`master/src/http/exec_mode.rs`, `Master::set_exec_mode`) makes
the checks of a create, in the same order, through the same functions: the
owner's effective level in the chat's workspace allows the mode
(`workspaces::switch_refusal`, which is `levels::refusal` then the node
rule, not `work_refusal`: that one asks the mode the chat has NOW, and a
host chat whose owner was lowered must still be able to leave it); a
connected node announced the mode (`mode_refusal`) and the switch
(`CAP_EXEC_SWITCH`; an older node: "update the node"); the log is not
diverged. Every request for `host` is audited whatever refuses it
(`session.exec_mode.host`: who, the chat, workspace, node, from, to,
outcome), a granted switch to `sandbox` too. The master then sends
`session.set_exec_mode { session_id, mode }`.

**The node decides again** (`node/src/session_exec.rs`), under the turn
slot as a rewind does: the chat is an agentic chat of a person; the node
offers the mode (`ExecOffers::choose`, as at create: `host = true`, the
`[execution]` and `[sandbox] mode = "required"` rules); no turn runs or is
being stopped, no `/compact`, no background command or sub-agent runs or
has an end not recorded, no approval waits, no agent of a message waits.
The refusal says what to stop.

**What it does, in this order, all before the ack:**

1. Stops the chat's MCP servers (node.toml's and the repository's approved
   ones) and waits until they are gone: they were started under the old
   confinement, and the next turn starts them under the new one. One that
   does not die in ten seconds refuses the switch, with nothing recorded.
2. Drops every chat grant ("allow for this chat"), one `chat_allow_removed`
   each, so the list shows it: a call allowed in one shell is asked again in
   the other, and again when the chat comes back. The smallest correct
   mechanism: the grant key stays as it is (no change to what existing logs
   hold), and nothing of a mode is in it to forget. A restart between this
   and step 3 leaves the old mode with fewer grants.
3. Records `exec_mode_changed { mode, from }` (additive; no account id, as
   no event has one; the audit line names the person) and applies it.

**What was built under the old mode, and what happens to it:** the MCP
servers (stopped); the chat grants (dropped); the background table
(nothing runs at a switch: refused otherwise); sub-agents (none works at a
switch: refused otherwise; one made before it takes no more turns, one made
after it is created in the new mode); the per-call
confinement, the bash tool's description and the approval card's wording
(read from the session's mode at each call and turn: nothing kept); the
environment line of the next turn (it names the mode and, once, the
change: "ran in the sandbox until this turn and now run in the host shell",
recorded in `turn_started.environment`, so the model's history rebuilds
byte for byte). Not dropped, because both modes share them: the chat's
folder and its session temp folder (the residual below: what one mode
writes, the other may run). The provider's prompt cache of the tool
descriptions is lost once per switch.

**The approval mode is the person's and stays.** In `auto` the next turn
(a message, a callback, a peer's) runs host commands unasked; the web
warning says so.

**Switching back does not undo the host's side effects.** What host
commands started outside their own process group (a tmux server, a
service, a container) keeps running uncontained after the chat is back in
the sandbox, and what a sandbox turn wrote in the folder runs with the
host's rights once a host command executes it (a script, a `Makefile`,
`.git/hooks`).

**State, not conversation.** A rewind does not move the mode back (as it
never moves the approval mode back; the divider in the conversation is
hidden with its turns, the mode is not). A fork starts in the mode its
parent has NOW: its copied events carry the changes up to its cut, and when
the parent was switched after the cut the node appends an
`exec_mode_changed` of the fork's own, right after `session_forked`, so its
log, fold and mirror say the truth (the fork route's level check reads the
parent's row, which is that mode). The master's mirror
(`sessions.exec_mode`) follows the latest event; a mode it cannot read stays
as written and fails closed (`MODE_UNKNOWN`).

## The master: levels (part 2)

**The levels** (`master/src/levels.rs`, `ExecLevel` in `core/src/policy.rs`):

* **An account's ceiling**, `users.exec_ceiling`, set by server admins
  (`PUT /api/users/{id}/exec-ceiling`), never their own. A new account
  starts at `none`, also one a server admin creates: the create form does
  not ask (the API takes `exec_ceiling`, audited), because the level is set
  in the list, where `host` goes through its warning; with every member
  also starting at `none`, an account made by mistake runs nothing. The
  first admin, made by the claim, is a server admin. **A server admin's
  ceiling counts as `host`** while they are one (`levels::ceiling_of`); what
  is stored for them applies once they are not (`exec_ceiling_stored` in the
  users list). A disabled account's counts as `none`.
* **A member's level in a team workspace**, `workspace_members.exec_level`,
  set by its admins (`PUT /api/workspaces/{id}/members/{user}/exec-level`):
  every new member starts at `none`; nobody changes their own; nobody
  grants above their own effective level there; only an owner grants
  `host`; an owner's level counts as `host` and cannot be set (so a team
  owner's effective level is their ceiling). **Decided (the owner, 2026-11-14, review L2-2):** a role carries its
  level, so making someone an owner gives them `host` there whatever the
  granting owner's own level; their account's ceiling still caps it. (A
  self-made owner at a `sandbox` ceiling can thus give a friend whose ceiling
  is `host` that level in their team by making them co-owner; until nodes
  belong to workspaces, N.1, it gives nothing the friend's ceiling did not
  already allow on any node.) Members, not only admins, are
  "in" the workspace at `none`: they read and chat there, nothing more.
* **A Personal workspace** has no level of its own: the ceiling applies
  (`levels::workspace_level` counts it as `host`). This is what closes the
  hole part 1's review found (H1): every account owns its Personal
  workspace and may own a team it made, so ownership never raises anyone
  past the ceiling a server admin set.
* **The effective level** is the lower of the two (`levels::effective`).
  Roles stay for their other permissions (settings, members, routines);
  `Perm::Agentic` is gone, and a guest can run agent chats if given a
  level (a role says what someone may administer, the level what they may
  run).
* **The API token** is the machine owner's root key: it acts as an owner of
  every workspace and is `host`; its own chats have no owner. On a member's
  chat it gets the member's answer (below).

**Where it is checked:**

* **Create:** `levels::create_mode` in the HTTP handler (the mode asked for,
  else the most confined one the level allows; never `host` by default),
  and again in `Master::create_session` for whoever calls it (the owner of
  the spec, `workspaces::create_mode`): routines, tests, a future caller.
* **Every later command that makes a node work on a session:**
  `Master::send_command` asks `workspaces::work_refusal` for every command
  `writes_to` names (prompt, callback, allowing an approval, restore,
  `set_agent`, `select_variant`, artifact staging), so no caller skips it.
  It is decided by the chat's **owner**, not the caller: their effective
  level in the chat's workspace must allow the chat's recorded mode, and
  the node rule must allow them. It fails closed: an agentic row without a
  readable mode, an owner who cannot be loaded, disabled or gone, or a
  session the mirror does not know does no work. Denying an approval,
  cancelling, stopping background commands and deleting are never refused.
  The HTTP gate (`agentic_gate`) asks the same first, so a refused prompt
  keeps its uploads. Before part 2 `set_agent` and `select_variant` had no
  gate at all (on `main` `set_agent` answered ok for an account without the
  grant).
* **Callbacks** (admission, `agentic_blocked`) and **artifact publishing**
  (`artifacts::refusal`, the staging check) ask the same; **routines** fire at their owner's
  level on the day they fire (`runner::why_blocked` needs `sandbox`, since a
  routine's chat is always `sandbox`; "Run now" is refused at once).
* **Reading** (a chat's events, its row, the list) is never refused: the
  row carries `blocked` with the reason.

**Lowering** a ceiling or a workspace level runs the revocation sweep for
that member, and every command a node acks is checked again once acked
(`Master::recheck_after_ack`, review L2-1: the sweep reads `running` from
the mirror and misses a turn whose message was on its way): every running turn and every background command of their
chats whose mode is now above their level stops, and only those (the sweep
asks once per owner, workspace, node and recorded mode: review RN-1). A
server admin demoted falls to their stored ceiling, and the sweep runs as
for any role change. **A removed member's** chats from that workspace
return to their Personal workspace (as before), where their ceiling applies
from their next action; what was running in them for the team (turns,
background commands) is stopped at the removal
(`workspaces::stop_removed`), whatever its mode; deleting a team workspace
does the same for every member's chats in it (review L2-3). Leaving thus
also unblocks a chat the team's level blocked (review L2-6, recorded on
N.1).

**Changes are conditional and audited.** Each level change is written only
while the level (and, for a member, their role) is still what the guards
read, and while the setter still stands as read (their role, their level
there, their ceiling, server admin or not; review L2-9): two admins
changing one member at once, or a grant by someone lowered meanwhile, get
409. Nobody changes a member whose level is above their own (review L2-10,
decided): an admin below `host` cannot undo an owner's `host` grant. The
member list shows each member's level as set and as it counts to those who
manage members; anyone else sees only their own (review L2-7). Roles and
levels stay separate (L2-5, decided): the role select says so. Each writes one audit line in the same transaction
(`level.account`, `level.workspace`: `by` (user id or `api token`), `user`,
`workspace`, `from`, `to`; no chat content). A role change that makes or
unmakes an owner or a server admin, disabling an account, an account
created above `none`, and a removal each write one too, with a `reason`.

**The UI:** Settings → Users shows and sets each account's level (a server
admin's shows "host"); Workspace admin → Members shows and sets each
member's level, offering only levels the person setting it may grant;
granting `host` in either place shows the host warning (`HostWarning`,
grant variant: the same risks) before anything is saved, and cancelling
saves nothing. The new-chat choice offers `host` only at the `host` level
(`mayPickHost`); elsewhere the start screen's agentic row still names the
mode, as a label ("Commands sandbox", the owner's decision 8 of 2026-11-17,
ADOPTION_PLAN X.26); at `none` or `container` the composer says why agent
chats are off, naming which level and who sets it.

**What the node knows:** nothing of levels. It knows its own ceiling and
refuses any mode above it whatever the master sends
(`node/tests/exec_modes.rs`). A stolen master can still ask for any mode up
to the node's ceiling: the levels protect against members, not against the
master (O13, change 2).

**What a level in a team does not do yet:** nodes are not tied to
workspaces (C.12, N.1), so a member whose ceiling is `sandbox` can run a
`sandbox` chat on any remote node, a team's included, from their Personal
workspace, whatever their level in that team. Until N.1, the account's
ceiling is the boundary between people; a workspace's levels are the
team's rule within the workspace.

**The schema** changed in place (version 1, no compatibility before the
first tag): `users.can_agentic` became `users.exec_ceiling`, and
`workspace_members` gained `exec_level`. A data directory made by an
earlier build is refused at start as a schema mismatch (fix plan 4.3).

## Not built, and where it belongs

* **A node that serves several workspaces refusing `host`** (O13, change
  2). The node cannot know today: workspaces are the master's, and a node
  has no list of its attachments. It belongs with the node's attached
  list (C.12, built in N.1): the node refuses `host` at start, and in its
  hello, when that list has more than one entry.
* **Levels tied to nodes**: a node attached to a workspace (C.12, N.1), so
  a workspace's levels bound what its members run on its nodes.
* **Files one mode writes and another executes.** A `sandbox` session can
  write what a `host` session in the same folder later runs (a script, a
  `Makefile`, `.git/hooks`, `.git/config`): the `host` session then runs it
  with every right of the node's user. This cannot be closed by the modes;
  the warning and the README say it. C.8's read-only `.git` binds (O13,
  change 4) are what narrows it.

## `[sandbox] mode = "off"` and a host chat

With `mode = "off"` a `host` chat's shell is the node's own OS user with no
file rule: it can read and change the node's data directory: other people's
chats kept on this node, the store of approved repository MCP servers (so it
can approve a server for anyone, and the server then starts in their chats,
sandboxed ones too, after the node restarts; PLAN.md T82, T39), the node's
keys and its token. This is a property of that configuration, not a defect to
patch around: use `mode = "off"` with `[execution] host = true` only where
everyone who may open a host chat is trusted like the node's owner. The node's
start and `doctor` print one warning for the combination (`host chats: ...`;
`cli/tests/contain.rs` `doctor_warns_where_a_host_chat_can_reach_the_data_directory`).
With the default `mode = "required"` Landlock keeps the data directory out of
a host chat's reach (`cli/tests/exec_modes.rs`
`a_host_chats_shell_cannot_read_or_change_the_data_directory`).

## Tests

* `tools/src/sandbox.rs`: `without_bubblewrap_the_sandbox_mode_is_not_offered`,
  `the_sandbox_modes_shell_is_never_landlock_alone`.
* `node/src/exec.rs` (the choice, the refusals, the hello), `node/src/policy.rs`
  (`[execution]` parsing; `[sandbox] contain` refused).
* The switch (N.10b): `core/src/exec_mode.rs` (the one rule), `core/src/view.rs`
  `the_latest_exec_mode_change_is_the_chats_mode`, `store/tests/event_log.rs`
  `the_exec_mode_on_the_row_follows_the_latest_change`, `node/tests/exec_switch.rs`
  (an idle chat switches both ways; refused while a turn or an approval
  waits, for a routine, a chat, a mode not offered; a chat grant does not
  carry; MCP servers stop and restart under the new mode; the model is told;
  restart and fork; an unknown mode runs nothing; a sub-agent after a switch;
  a rewind across a switch; nothing a model writes switches),
  `master/tests/it/exec_switch_http.rs`, `web/src/components/execswitch.test.tsx`,
  `web/e2e/exec-switch.cjs`.
* `node/tests/exec_modes.rs`: a node offering nothing; a test master asking
  for a mode the node does not offer; recorded and never changed; a mode
  gone after a restart runs nothing (both directions); a sub-agent's mode;
  chat sessions.
* `agent-native/tests/host_forwarding.rs`
  `the_sessions_mode_picks_its_confinement_through_the_loop`.
* `cli/tests/exec_modes.rs`: on one node offering both, a `host` session's
  command runs outside bubblewrap and a `sandbox` session's inside; a
  background command in `host` is accepted and ended with the guard's
  confirmation (`a_host_session_gets_a_background_command_under_the_guard`). `cli/tests/contain.rs`
  `doctor_marks_each_containment_state`, `doctor_warns_where_a_host_chat_can_reach_the_data_directory`,
  `cli/tests/network_off.rs`.
* `master/tests/it/exec_mode_http.rs`: `host` needs the `host` level whatever one owns (H1), and the node list.
* `web/src/components/execchoice.test.tsx`; `web/e2e/exec-modes.cjs`
  against the real binary.
* Part 2: `master/tests/it/exec_levels_http.rs` (every action refused at
  `none`, `set_agent` and `select_variant` included; `sandbox` refused
  `host`; lowering stops only the chats above the level, ceiling and
  workspace level; a demoted server admin; who grants what; a self-made
  owner at a `sandbox` ceiling; Personal; a removed member's turn; audit
  lines; the API token), `master/tests/it/routines_http.rs`
  `a_routine_fires_at_its_owners_level_on_the_day_it_fires`,
  `store/tests/workspaces.rs`
  `levels_start_at_none_and_change_only_from_what_was_read` (two admins at
  once), `core/src/policy.rs` and `master/src/levels.rs` unit tests,
  `web/src/components/users.test.tsx` and `workspace/workspace.test.tsx`
  (the warning before a grant, cancel saves nothing), `web/e2e/exec-levels.cjs`
  against the real binary.
* The security review's fixes (2026-11): `master/tests/it/exec_mode_http.rs`
  `host_needs_the_host_level_not_ownership` (was `host_is_for_server_admins_only`), `a_node_that_did_not_announce_the_mode_gets_no_chat`,
  `a_host_chat_whose_owner_is_no_longer_an_admin_does_no_more_work`,
  `the_mode_check_on_a_row_fails_closed_and_refuses_host_to_a_member`,
  `the_modes_are_not_shown_to_an_account_without_the_grant`;
  `master/tests/it/routines_http.rs` `a_routines_chat_is_always_in_the_sandbox`;
  `node/tests/exec_modes.rs` `host_is_never_the_default`,
  `a_host_session_takes_auto_within_the_nodes_ceiling` (it replaced `a_host_session_is_never_in_auto`
  with the owner's decision of 2026-11-14), `a_host_session_is_offered_background_commands` (the model is offered the tools since the owner's decision of 2026-11-20; before it was not: N-c),
  `an_agentic_session_without_a_recorded_mode_runs_nothing`; `node/src/exec.rs`
  `a_mode_not_offered_gets_a_confinement_that_runs_nothing`; `node/src/session.rs`
  `a_host_chats_card_says_the_host_shell`; `cli/tests/network_off.rs`
  `a_probe_that_fails_for_a_moment_at_start_is_tried_again`; `cli/tests/contain.rs`
  `strict_does_not_offer_the_sandbox_with_a_write_entry_gap`; `store/tests/event_log.rs`
  `the_recorded_execution_mode_is_on_the_row`.

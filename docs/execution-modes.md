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
| `host` | `sandbox-exec` (Landlock, as `[sandbox] mode` says) → `bash`, as the node's OS user, no bubblewrap; with `[sandbox] mode = "off"` bash directly | Landlock's file rules where `[sandbox] mode` is `required` or `best-effort`: the session folder, its temp folder, the system folders and the `[sandbox] read`/`write` entries; TCP denied with `network = "off"`. **Not** held: no PID namespace (what a command starts can keep running after the call, the chat and the node), no file view (`/proc` lists every process, metadata of every path is visible), UDP and DNS go out with the network off, and below Landlock ABI 9 any service of the node's user reachable over a Unix socket (the session bus: `systemd-run --user`) runs anything with every right of that user. No background commands (the session has no background table, so they are not offered: review N-c). Never approval mode `auto`: clamped to `ask` at create and at load, whatever is asked or `max_approval_mode` allows, and its approval card says it runs in the host shell (review N-e) |

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
* `gap:host:netns`: network off, a `host` chat, Landlock stops TCP (UDP and
  DNS go out);
* `gap:host:net-open`: network off, a `host` chat where nothing applies it
  (`[sandbox] mode = "off"`, or Landlock not enforcing the TCP rule):
  everything goes out (review L-d).

The master keeps the known ones in its node list (`exec_modes`,
`exec_gaps`) and shows them only to accounts whose ceiling runs agent chats
(review L-i); the chat header names the chat's mode and says its gaps in
plain words (reviews T8B-3, L-e). **The hello is a snapshot** of the node
at connect: a gap closed later by the step up (the network namespace) is
still shown until the node's next hello.

## The session's mode

* `session.create` carries `exec_mode` as a request (`Option`). The node
  chooses (`ExecOffers::choose`): a mode it offers as asked; without one,
  `sandbox`, or a refusal on a node that offers only `host`: that mode is
  only ever given when asked for by name (review L-a); a mode it does not
  offer is refused whatever the master sends. Recorded in `session_created.exec_mode`; no
  later command carries it.
* At load the recorded mode is taken as it is. Before every turn, callback,
  restore and delegation (`Session::checked_folder`) it is checked against
  what the node offers now: a session whose mode is gone (bubblewrap
  removed, `host` turned off) runs nothing and says why. It is never moved
  to another mode, in either direction.
* Chat-profile sessions have no mode; a create that asks for one is refused.
* A sub-agent's session gets its parent's mode (`run_child`).
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
* The node's `session.create` audit line names the mode asked for (review
  N-d).

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
  owner's effective level is their ceiling). **Open:** making someone an
  owner gives them that `host`, and the role rule (only owners make owners)
  does not look at the granting owner's level, so a self-made owner at a
  `sandbox` ceiling can give a friend whose ceiling is `host` the `host`
  level in their team by making them co-owner. Requiring the granter to be
  at `host` was tried and dropped: an owner below `host` could then never
  name a co-owner, and the last-owner rule would keep them in their team
  for good. The two ways out (owners' level stored, the creator's set to
  `host`; or the guard plus a way to hand over ownership) are the owner's
  decision. Until nodes belong to workspaces (N.1) it gives nothing the
  friend's own ceiling did not already allow on any node. Members, not only admins, are
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
(`mayPickHost`); at `none` or `container` the composer says why agent
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

## Tests

* `tools/src/sandbox.rs`: `without_bubblewrap_the_sandbox_mode_is_not_offered`,
  `the_sandbox_modes_shell_is_never_landlock_alone`.
* `node/src/exec.rs` (the choice, the refusals, the hello), `node/src/policy.rs`
  (`[execution]` parsing; `[sandbox] contain` refused).
* `node/tests/exec_modes.rs`: a node offering nothing; a test master asking
  for a mode the node does not offer; recorded and never changed; a mode
  gone after a restart runs nothing (both directions); a sub-agent's mode;
  chat sessions.
* `agent-native/tests/host_forwarding.rs`
  `the_sessions_mode_picks_its_confinement_through_the_loop`.
* `cli/tests/exec_modes.rs`: on one node offering both, a `host` session's
  command runs outside bubblewrap and a `sandbox` session's inside; no
  background command in `host`. `cli/tests/contain.rs`
  `doctor_marks_each_containment_state`, `cli/tests/network_off.rs`.
* `master/tests/exec_mode_http.rs`: `host` needs the `host` level whatever one owns (H1), and the node list.
* `web/src/components/execchoice.test.tsx`; `web/e2e/exec-modes.cjs`
  against the real binary.
* Part 2: `master/tests/exec_levels_http.rs` (every action refused at
  `none`, `set_agent` and `select_variant` included; `sandbox` refused
  `host`; lowering stops only the chats above the level, ceiling and
  workspace level; a demoted server admin; who grants what; a self-made
  owner at a `sandbox` ceiling; Personal; a removed member's turn; audit
  lines; the API token), `master/tests/routines_http.rs`
  `a_routine_fires_at_its_owners_level_on_the_day_it_fires`,
  `store/tests/workspaces.rs`
  `levels_start_at_none_and_change_only_from_what_was_read` (two admins at
  once), `core/src/policy.rs` and `master/src/levels.rs` unit tests,
  `web/src/components/users.test.tsx` and `workspace/workspace.test.tsx`
  (the warning before a grant, cancel saves nothing), `web/e2e/exec-levels.cjs`
  against the real binary.
* The security review's fixes (2026-11): `master/tests/exec_mode_http.rs`
  `host_needs_the_host_level_not_ownership` (was `host_is_for_server_admins_only`), `a_node_that_did_not_announce_the_mode_gets_no_chat`,
  `a_host_chat_whose_owner_is_no_longer_an_admin_does_no_more_work`,
  `the_mode_check_on_a_row_fails_closed_and_refuses_host_to_a_member`,
  `the_modes_are_not_shown_to_an_account_without_the_grant`;
  `master/tests/routines_http.rs` `a_routines_chat_is_always_in_the_sandbox`;
  `node/tests/exec_modes.rs` `host_is_never_the_default`,
  `a_host_session_is_never_in_auto`, `a_host_session_is_not_offered_background_commands`,
  `an_agentic_session_without_a_recorded_mode_runs_nothing`; `node/src/exec.rs`
  `a_mode_not_offered_gets_a_confinement_that_runs_nothing`; `node/src/session.rs`
  `a_host_chats_card_says_the_host_shell`; `cli/tests/network_off.rs`
  `a_probe_that_fails_for_a_moment_at_start_is_tried_again`; `cli/tests/contain.rs`
  `strict_does_not_offer_the_sandbox_with_a_write_entry_gap`; `store/tests/event_log.rs`
  `the_recorded_execution_mode_is_on_the_row`.

# Execution modes (N.10a part 1)

Decision O13 (the owner, 2026-11-12 and 2026-11-13; `ADOPTION_PLAN.md`,
"Execution levels"): who may run what on a node is one ordered level per
member per workspace, `none` < `container` < `sandbox` < `host`, under the
node's own ceiling, and every check is
`session mode <= member's level <= node's ceiling`. This note is the node's
side and the session's mode (part 1). The level per member, which replaces
the role-derived `agentic` permission, is part 2 and not built yet; until
then the master applies an interim rule (below). `container` (rootless
Podman, N.4) does not exist yet; the type orders below `sandbox` when it
comes.

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
self-test, never from anything received (`cli/src/main.rs` `exec_offers`,
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
  until it is restarted. No later probe adds the mode.
* `host` is offered only with `host = true`, and not where the Landlock
  sandbox is required and does not work (`[sandbox] mode = "required"`
  refuses the node in every mode, as before).
* A node that offers neither runs no agentic session and says so plainly
  at start (`node run` refuses to start; `serve`'s node logs it at `error`),
  in `doctor` (`FAIL execution modes: NONE …`) and in every refusal:
  "this node offers no way to run commands: install bubblewrap, or set
  `[execution] host = true` in node.toml and read what that means".

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
`exec_gaps`) and shows them only to accounts with the agentic grant
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

## The master, until part 2

`POST /api/sessions` takes `exec_mode` (`workspaces::exec_mode_for`):

* `host` only for a server admin (the API token counts as admin), on every
  node (review H1: the earlier "workspace's owner" arm admitted everyone,
  since every account owns its Personal workspace and may own a team it
  made itself).
* Every agentic create carries a mode: `sandbox` unless `host` is asked
  for by name (`Master::create_session`), so a node whose only mode is
  `host` refuses rather than hand it over by default.
* A node is sent a mode only if its hello named it (`exec:<mode>`); one
  that names none (a node from before modes) gets no agentic chat, refused
  by the master naming the node (review L-g).
* The recorded mode is mirrored onto the chat's row (`sessions.exec_mode`)
  and the create's rule is applied again in `agentic_blocked`, on every
  prompt, allowing approval, restore and callback admission; an agentic
  row without a readable mode is refused (fails closed). A server admin
  who is demoted loses their `host` chats at the next action, and the
  revocation sweep stops a running turn as for a lost grant (review L-h).
* Every create that asks for `host` writes `session.create.host` to the
  audit log, granted or refused: user, workspace, node; no content (review
  N-d).
* The UI offers `host` only to a server admin, binds a confirmed choice to
  the node it was confirmed for (review L-j), and never offers `auto`
  (the composer sends no approval mode; the warning says a host chat
  always asks).
* A routine's run chat always asks for `sandbox` (unattended, often on
  input from outside); a node without `sandbox` refuses the run.
* Callbacks and the actions on an existing chat act on its recorded mode.

A stolen master can still ask for any mode up to the node's ceiling, as it
can today: the modes protect against members, not against the master (O13,
change 2).

## Not built, and where it belongs

* **A node that serves several workspaces refusing `host`** (O13, change
  2). The node cannot know today: workspaces are the master's, and a node
  has no list of its attachments. It belongs with the node's attached
  list (C.12, built in N.1): the node refuses `host` at start, and in its
  hello, when that list has more than one entry.
* **The level per member** (part 2): replaces `Perm::Agentic`, is checked
  where `agentic_blocked` checks today (create and every action). Until
  then `host` is for server admins only.
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
* `master/tests/exec_mode_http.rs`: the interim rule and the node list.
* `web/src/components/execchoice.test.tsx`; `web/e2e/exec-modes.cjs`
  against the real binary.
* The security review's fixes (2026-11): `master/tests/exec_mode_http.rs`
  `host_is_for_server_admins_only`, `a_node_that_did_not_announce_the_mode_gets_no_chat`,
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

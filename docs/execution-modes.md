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
| `sandbox` | `sandbox-guard` → `bwrap` (its own PID namespace, its own file view built from the grants, `/proc` of its own where it can be mounted) → `sandbox-exec` (Landlock) → `bash` (T.8a to T.8f) | Nothing a command starts outlives it (from Landlock ABI 9; below, the socket gap `doctor` names); the file view; with `network = "off"`, a network namespace of its own; background commands |
| `host` | `sandbox-exec` (Landlock, as `[sandbox] mode` says) → `bash`, as the node's OS user, no bubblewrap; with `[sandbox] mode = "off"` bash directly | Landlock's file rules where `[sandbox] mode` is `required` or `best-effort`: the session folder, its temp folder, the system folders and the `[sandbox] read`/`write` entries; TCP denied with `network = "off"`. **Not** held: no PID namespace (what a command starts can keep running after the call, the chat and the node), no file view (`/proc` lists every process, metadata of every path is visible), UDP and DNS go out with the network off, and below Landlock ABI 9 any service of the node's user reachable over a Unix socket (the session bus: `systemd-run --user`) runs anything with every right of that user. No background commands |

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
  network-off gap, an image not held, the binary in a session's reach);
  `strict` only where none is left (what `[sandbox] contain = "required"`
  promised); `off` never (bubblewrap is not probed). The shared-`/proc`
  form counts: every process still dies with the namespace and the file
  view is the same; what it loses is that `/proc` lists the outer processes
  (said in the log and `doctor` as before). It is the form inside a
  default rootless container, where the alternative would be no `sandbox`
  at all.
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
the gaps of those modes: `gap:sandbox:netns` (network off, bubblewrap could
not make a namespace at start) and `gap:host:netns` (network off; `host`
never has one). The master keeps the known ones in its node list
(`exec_modes`, `exec_gaps`) for the UI; the chat header names the chat's
mode and says its gaps in plain words (review T8B-3: before, only the
start-up log and `doctor` said them). A gap closed later by the step up is
still shown until the node's next hello.

## The session's mode

* `session.create` carries `exec_mode` as a request (`Option`). The node
  chooses (`ExecOffers::choose`): a mode it offers as asked; without one,
  the most confined it offers; a mode it does not offer is refused
  whatever the master sends. Recorded in `session_created.exec_mode`; no
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
  held image, no containment, and it can never gain one). The node's own
  helpers (checkpoint git, graphify) use the twin, as before.

## The master, until part 2

`POST /api/sessions` takes `exec_mode` (`workspaces::exec_mode_for`):

* `host` only for the workspace's owner or a server admin (the API token
  counts as admin); on the embedded `local` node only for a server admin:
  it runs as the server's user, next to its keys and every account's chats.
* Without a request, a caller who may ask for `host` gets the node's
  default; anyone else is sent `sandbox` explicitly, so a node whose only
  mode is `host` refuses them instead of handing it over by default.
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
  where `agentic_blocked` checks today (create and every action), and makes
  a demoted member's `host` chats stop at their next action. Until then a
  workspace owner keeps their `host` chats after losing the role.
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

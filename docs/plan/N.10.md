# N.10 **sec**

## Note phase

none (O13)

## Adds beyond the note

Execution levels.
**N.10a part 1 done (branch `feat/N.10a-node-modes`; `docs/execution-modes.md`, PLAN.md T39): the node's side and the session's mode.** `[execution]` in node.toml is the ceiling (`sandbox = "auto" | "strict" | "off"`, which replaces `[sandbox] contain`; `host = false` by default); what a node offers is computed at start from it and its own self-test (`sandbox` only where the bubblewrap probe passes, the shared-`/proc` form included; Landlock alone never), announced at hello as `exec:sandbox`, `exec:host` with the gaps `gap:sandbox:netns`, `gap:host:netns` (review T8B-3, closed as far as a hello can); a node offering neither runs no agent chat and says so at start, in `doctor` and in the refusal (the break: `auto` with a failed probe no longer runs the shell with Landlock alone, and its step up from Landlock alone is removed). `session.create` carries the mode as a request, the node chooses (refuses one it does not offer, else the most confined), records it in `session_created`, checks it before every turn, callback, restore and delegation (gone after a restart: runs nothing, never moved); sub-agents get the parent's; chats none.
The backend picks the confinement by the recorded mode: `host` sessions get the uncontained twin, `sandbox` sessions a sandbox that refuses rather than run without bubblewrap.
Interim master rule (part 1 only; replaced by the levels of part 2, below): `host` only for the workspace's owner or a server admin, on `local` only for a server admin; anyone else is sent `sandbox` explicitly; a routine's chat asks for `sandbox`.
The UI names the chat's mode and gaps, and offers `host` behind the warning.
**Not built:** the node serving several workspaces refusing `host` (it cannot know: belongs with C.12's attached list, N.1).
**N.10a part 2 built** (branch `feat/N.10a-part2-levels`, not yet reviewed; `docs/execution-modes.md` "The master: levels", PLAN.md T39): the account's ceiling (`users.exec_ceiling`, server admins set it, `none` for every new account, a server admin's counts as `host`) replaces `can_agentic`; a level per member per team workspace (`none` for every new member, set by its admins, never one's own, never above one's own, `host` only by an owner, an owner's `host`) replaces `Perm::Agentic`; the effective level is the lower (in Personal, the ceiling) and replaces part 1's interim rule.
Checked at create (handler and `Master::create_session`) and on every command that makes a node work, in `Master::send_command`, by the chat's owner (`set_agent` and `select_variant` included, which had no gate); a lowering stops only the chats above the new level through the sweep; a removal stops what ran for the team; every change one audit line, conditional on what was read; the UI sets both behind the host warning.
**Not done here:** levels tied to a workspace's nodes (needs the node's attached list, C.12 / N.1): until then a member's level in a team does not keep them off that team's node from their Personal workspace at their ceiling, and leaving a team unblocks the chats its level blocked (they return to Personal; review L2-6, recorded on N.1).
Review of part 2 (2026-11-14): fit after fixes; L2-1, L2-3, L2-4, L2-5, L2-7 to L2-10 fixed on the branch;
L2-2 decided by the owner: a role carries its level (a co-owner gets `host`, capped by their ceiling).
The level per member below.
**Master:** a level per member per workspace (replaces `Perm::Agentic`), checked where `agentic_gate` checks today: create and every prompt, approval, restore and routine firing.
A session's mode (`container`, `sandbox`, `host`) is chosen at creation from what the level and the node allow, the most confined one by default, recorded in the session's first event and moved only by its owner while nothing of the chat runs (N.10b, the owner's decision of 2026-11-21: "Yes, build it"; before, never changed); sub-agents inherit it as it is then; nothing in a model's output or a tool result selects it.
**Node:** `[execution]` states the ceiling; the node refuses a mode above it whatever the master sends, and offers the master only modes its own self-test can enforce.
**UI:** the member list shows and sets the level; the chat names its mode, and also shows the node's gaps: not contained, and network off without a network namespace (review T8B-3; today only the node's start-up log and `doctor` say so).
Choosing `host` shows a warning before it is saved, both when an owner grants the level and when a member picks the mode for a chat (the owner, 2026-11-12: "display warning when selected and say the risks").
It says, in plain words: the agent's commands run as the node's own user with nothing between them and the machine; they can read and change every file that user can, its keys and other projects included; what they start can keep running after the chat ends; one web page or file the agent reads can turn all of that against you.
The approval card is then the only check.
**N.10a** (the level, the ceiling, the checks, the modes `sandbox` and `host`) needs only T.8a; `container` is offered once N.4 exists.

**N.10b built** (branch `feat/exec-switch`, plan row X.52; `docs/execution-modes.md` "Switching a chat's mode", PLAN.md T39): the owner of an existing agentic chat switches it between `sandbox` and `host` while nothing of it runs; the rule "never moved to another mode, in either direction" of N.10a is replaced by it (README, `docs/execution-modes.md`, T39).

## Abuse tests beyond the note's

Tests first: a member at `none` is refused on every action that starts work, with the reason; a member at `container` is refused a `sandbox` session by the master, and by the node when a test master asks for it anyway; lowering a level blocks that member's sessions above it at their next action and stops a running turn; a routine fires at its owner's level on the day it fires; an admin cannot grant above their own level or raise themselves; `host` asked of a node serving two workspaces is refused by the node; a `container` session cannot write `repo.git/hooks/post-checkout` or `repo.git/config`; a sub-agent's mode equals its parent's; in a browser, granting `host` and choosing it for a chat each show the warning with its risks before anything is saved, and cancelling saves nothing

## Invariants

Node decides (the ceiling); nothing the model writes is trusted; chats private

## Podman / Node

**Yes** (`container`) / Yes

## Depends

T.8a; N.4 for `container`; `host_shell_shared` still rules `sandbox` on a shared node

## Size

**M** (N.10a); the rest with N.4


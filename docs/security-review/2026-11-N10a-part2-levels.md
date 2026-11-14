# N.10a part 2 review, 2026-11-14: execution levels per member

Reviewed: `feat/N.10a-part2-levels` at `aa32b39` (`git diff origin/main...HEAD`:
the owner's decision `a17ed38`, tests `94c6289`, server `8339e15`, UI
`aa32b39`). One independent reviewer who did not write the code, with proofs
in `crates/cli/tests/support/review_n10a2/` (`run.sh` copies
`review_n10a2_levels.rs` into `crates/master/tests`, runs it, removes it;
its `gap_*` tests pass while the gap they show exists). The fixes are at
`cefbe0b`. On the owner's direction of 2026-11-14 (harness features before
further hardening) there was no second re-check round.

## 1. What was built

The owner's decisions: one ordered level, `none` < `container` < `sandbox` <
`host`. The account-wide yes or no is replaced by an **account ceiling** set
by server admins; a member's effective level in a workspace is the lower of
the ceiling and their level there; in a Personal workspace it is the ceiling;
a server admin's ceiling counts as `host`. Workspace levels start at `none`;
nobody sets one above their own effective level or changes their own;
`host` is the owner's to grant; **an owner's level counts as `host`** (a role
carries its level: decided by the owner, see L2-2). A chat's mode must be at
most its owner's effective level; the node's own ceiling still decides what
the node offers.

The check runs at create (handler and `Master::create_session`) and on every
command that makes a node work, in `Master::send_command`, decided by the
chat's owner and failing closed; changing a chat's agent and picking a
parallel answer are checked for the first time. Lowering a level, a
removal, or a workspace's deletion stops what is above the new level, or
what ran there. Every level change is one audit line holding only ids and
the two levels. `users.can_agentic` became `users.exec_ceiling`, and
`workspace_members` gained `exec_level` (schema version 1 changed in place:
a data directory made before this is refused at start).

## 2. Verdict of the review

**Fit after fixes; nothing critical or high.** Every decision takes the
lower of the ceiling and the workspace level, so part 1's H1 class (a
workspace role standing in for a right) is closed. The check in
`send_command` holds for callers that skip the web layer (proved by calling
the `Master` directly). Lowering stops only the chats above the new level,
across workspaces too, so part 1's RN-1 class holds (proved).

| Id | Severity | What | Done |
|---|---|---|---|
| L2-1 | Low, older race | A prompt checked just before a lowering could be sent after the sweep had read `running = false`, and run on at the old level (unattended in `auto`) | The level is checked again once the node acks a work command; the session is stopped if it now refuses |
| L2-2 | Note today | Making someone co-owner gives them `host` in that team, past what the granting owner has; after N.1 that would be `host` on the team's nodes | **Decided by the owner: a role carries its level**; recorded in the design note and the N.10 row |
| L2-3 | Low | Deleting a workspace did not stop what removing a member stops | Both delete paths stop what ran there |
| L2-4 | Low | A callback test expected a callback after a removal that, on a real node, stops the command | The test asserts the stop and checks admission with a command started afterwards |
| L2-5 | Note | A guest with a level runs agent chats; demoting to guest stops nothing | As designed (a role says what someone administers, a level what they run); the role select says so |
| L2-6 | Note | Leaving a team unblocks chats the team blocked | Recorded on the N.10 and N.1 rows: once nodes belong to workspaces, a chat's node is checked against its current workspace |
| L2-7 | Note | Every member saw every level; an owner at a `none` ceiling was shown as `host` | Only those who manage members see others' levels, as set and as they count |
| L2-8 | Low | Docs false against the code; the Users page could not set a server admin's ceiling, so a demotion dropped them to `none` | Docs corrected; the select is shown for admins too, and a demotion asks first, naming the level they will have |
| L2-9 | Note | The conditional write checked the target, not the setter | The setter's role, level, ceiling and admin flag are part of the condition; a stale reading gets 409 |
| L2-10 | Note | A workspace admin below `host` could undo an owner's `host` grant | Nobody changes a member whose level is above their own |

After the fixes, four of the reviewer's `gap_*` proofs fail as they should:
deletion, an admin below `host` undoing a grant, the member list shown to a
guest, and the co-owner proof at its line where the granting owner now may
not change a member above their own level (the co-owner still gets `host`,
as decided). The guest and leaving proofs still pass, as decided or
recorded.

## 3. What remains

- **A team level is only partly a boundary until nodes belong to workspaces
  (N.1):** a member at `none` in a team can run a sandbox chat on that
  team's node from their Personal workspace if their ceiling allows. Until
  then the account ceiling is the boundary between people.
- **A role carries its level** (L2-2): a co-owner gets `host` in the team.
  The owner also wants roles that can be added and node access as a switch
  on the role; turning the per-member level into a role permission is a
  later, small task.
- `container` exists as a level; no node offers it until N.4.
- Three of the tests the review asked for pass on the code before the fixes
  too (coverage, not failing first): direct `Master` callers, lowering
  across workspaces, a demoted owner's team turn.

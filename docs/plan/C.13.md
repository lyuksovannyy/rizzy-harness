# C.13

## Decision

Undo and checkpoints, second design (owner's direction, 2026-11-11, answering O11).
**Note: `docs/undo-and-stash.md`** (2026-11-12, proposal, revised the same day after the owner's answers: undo stays available after a commit by anyone and restores working files only; non-conflicting unstash is seamless; a conflict is skipped, written beside with a suffix, or merged by the agent; a stash can be deleted by the person or the agent and expires at the first midnight after 30 days): §1 what exists, what is new and what changed, §2 undo and commits and A.17, §3 the stash, its expiry, unstash and the agent's tools, §4 code changes, cost (B.3) and residuals, §5 abuse cases, §6 tests, §7 build order, §8 questions and answers (later the same day: undo after a commit applies the inverse change to the working files and creates no commit; Q2 decided by approval mode; Q3 accepted with detection; Q1 not answered; a soft limit of 100 changed files per turn, §2.6, task T.9)

## What the note settles

The owner wants undo rethought around the user's own repository instead of a full shadow history.
In the owner's words, restated: (1) the shadow repository saves only what is **not committed**; when a new commit appears in the user's repository the shadow drops what that commit now holds, and undo for those files goes "by the actual committed files"; (2) after each agent turn the shadow records the present state as a shadow commit, uncommitted files and folders without a repository included; (3) the user's own edits are tracked too; (4) undoing an agent turn when the user has since changed files **asks the user** whether to stash those changes, and the agent can view a stash.
Borrowing objects from the user's repository stays.
The note settles, with the owner: what undo does when the turn's changes were committed (recommended: never move HEAD or the index; say which commit holds them and leave that to git); what a stash is (recommended: the pre-restore checkpoint A.9 already takes, given a name, a list, and a read-only tool for the agent); whether stashed edits are put back automatically when they do not overlap the undone changes; how long a stash is kept (it holds the user's work, so not under `checkpoints_keep`); what happens when the base commit is gone (B.2); and how much of A.9, A.15 and A.16 stays as it is.
Experiment B.3's numbers go into it.

## Tests it commits to

The note names them.
At least: undo after the user committed the turn's changes moves no ref and loses nothing; a stash survives pruning; the agent can read a stash and cannot write one; declining the stash prompt changes nothing.


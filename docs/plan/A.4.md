# A.4 **sec**

## Fixes

Exec 4; A8; missed 1

## Change

**A writer sub-agent gets its worktree or is refused.**
When the worktree cannot be made or the folder is not a repository (`session.rs:676-686`, `:789-798`), a child with `write`, `edit` or `bash` fails the spawn with a reason the parent sees.
Scout and Reviewer still run. `09` §6.1.
Since 2026-11-15 (live test: a second writer's work was refused whole and out of reach) its result is merged back per file, three ways, all or nothing per child; when not applied, the parent gets the conflicting files and the child's diff, quoted (PLAN.md T32).

## Done when

Non-git folder, Teamlead delegates to Software Developer: refused; forced worktree failure: refused;
Scout in a non-git folder runs.
**S**

## Lands on

gate3


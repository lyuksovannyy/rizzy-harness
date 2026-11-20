# A.15 **sec**

## Fixes

A14

## Change

**(After B.5; changed by its result.)**
B.5 showed that refusing is the wrong fix for nested repositories: the restore is not what loses the work, and one `git init` in a sub-folder silently disabled checkpoints for the whole folder.
So: **nested repositories are excluded from checkpoints and never touched by a restore;** they can no longer make the snapshot of the rest fail; an undo never lists one as restored and says which ones it did not cover.
Unchanged: **a restore is refused while another session has a turn running in the same folder.**

## Done when

B.5's twelve tests assert the fixed behaviour; a nested repository with no commit no longer stops the checkpoint; hostile config in a nested repository never runs; restore while a second session in the folder runs a turn is refused.
**M**

## Lands on

gate3


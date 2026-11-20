# A.17 **sec**

## Fixes

B.2; O11's answer (undo stays available after commits and history changes)

## Change

**Copy the old versions of the files a turn changed into the shadow's own store at the end of the turn,** with the trees a diff of the turn reads, as a pack with a `.keep` file that names the turn and is released when its checkpoint is pruned.
Content already gone when the turn ends (the agent pruned it inside its turn) is named in the chat.
No folder scan, no gc.

## Done when

Undo works after the user rewrites history and prunes, and after the shadow's own gc; the in-turn prune is named in the chat and the turn ends normally; only changed files are copied and the store stays bounded over 60 turns; nothing is written into the user's repository and none of its config runs.
**M**

## Lands on

`main`, after A.18


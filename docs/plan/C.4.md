# C.4

## Decision

What "always allow" may mean, where a rule lives

## What the note settles

Three levels: once; this exact call for this turn (A.2); a persistent rule, never a click, in a node-side file beside `node.toml` written by the node's owner (the UI may propose its text).
One project root on one node; never a prefix for wrappers or interpreters (`sudo`, `env`, `bash`, `python*`, `xargs`, `find`, `git -c`); never lifts the sandbox; never covers network, escalation or MCP; no repository file adds or loosens one; the file is always-ask for the agent. `02` §6 items 2, 5.

## Tests it commits to

`git status` never allows `git push`; a repository rules file changes nothing; the agent cannot write the rules file; no rule runs outside Landlock.


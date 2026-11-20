# X.31 **sec** (T76)

## Adds

Three owner decisions of 2026-11-17

## Safeguards and abuse tests

**Done on branch `fix/owner-decisions-2026-11-17`.** (1) Asked "a file your global git ignore covers (`.env`, `.idea/`) is never checkpointed, so Undo cannot restore an agent's edit to it, and nothing says so: should Undo say so, or should checkpoints ignore the global ignore file?", the owner: "should ignore".
The node's git no longer follows the user's global ignore file (checkpoints, the between-turns copy, stashes, sub-agents' folders and merge-back); the repository's `.gitignore` and `.git/info/exclude` apply as before (docs/undo-and-stash.md §2.7, PLAN.md T76, measured cost there; no size cap built). (2) Asked for a first-byte timeout setting for Ask chats, the owner: "would be nice". `[chat] model_first_byte_timeout_s` in config.toml (the node's key, unit, range 10..3600 and default 600; out of range stops the start); the silent-provider error names it too (README "Configuration", "Chats and web tools"). (3) Which built-in agents take the person's Personalisation, the owner: "decided by agent, scout dont, others maybe, default one - yes". `personalisation: false` in Scout's and Reviewer's definitions (the "others maybe" choice: both report to a lead in a strict form);
Assistant and every other built-in on (docs/agent-pages.md, README "Profile and Personalisation").


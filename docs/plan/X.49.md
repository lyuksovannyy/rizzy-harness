# X.49 **sec** (T32)

## Adds

A sub-agent that can write works in the chat's own folder by default; a copy of its own becomes the delegating agent's choice (`delegate`'s `folder`: `"shared"` or `"own"`), and a writing sub-agent is no longer refused in a folder that is not a git repository (owner's decision, 2026-11-21).
`[limits] sub_agent_folder = "shared" | "own"` in node.toml (default `"shared"`) is the node's word and wins over what a model asks.
No per-agent switch: this is how delegation works, not a new power of an agent (anything a shared sub-agent writes, the lead's own tools or an `own` sub-agent's merge could write too).

## Safeguards and abuse tests

The lead's turn checkpoint covers what a sub-agent wrote in a turn the lead's `delegate` call started (its first run or a follow-up), so Undo and "Show changes" are complete and exact for that work, and a background sub-agent's writes during the lead's turn are the turn's too.
Any other turn in a sub-agent's chat (a person's message to it) and a background sub-agent's writes between the lead's turns or after the last are another chat's work for T57: Undo asks first, naming the files, and "Undo anyway" is the person's choice.
An Undo while a background sub-agent still works in the folder is refused, naming the sub-agent and saying to stop it or wait.
The staleness guard of `edit` and `write` is what stops a blind overwrite between two writers; `bash` is not covered, and the docs say so.
The result of a shared sub-agent names the files that changed while it worked (quoted lines, each name cut, at most 20), and says plainly when its record failed after it changed files.
Tests: `node/tests/shared_folder.rs`, `node/tests/shared_folder_undo.rs`, `cli/tests/sub_agent_sandbox.rs`, `agent-native/src/delegate_availability_tests.rs`, `core/src/subagent.rs`, `node/src/policy.rs`; the web card and banner in `web/src/components/delegateresult.test.tsx` and `conversation.test.tsx`.

## Invariants

The node decides; one writer per session; the master runs no shell; nothing the model writes is trusted

## Podman / Node

No / Yes

## Depends

A.4, X.24, X.22

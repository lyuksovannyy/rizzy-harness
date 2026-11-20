# X.41

## Adds

Sub-agents for coding (the pair trial of 2026-11-20, `trial/REPORT.md` §6: a Pair Coder got every sub-agent with the switch on, only an agent file's `delegates:` narrowed it; the advisor's answer was a folded result on a card that did not name its model; the reply box was shown in a sub-agent-only advisor's chat although sending is refused).
Not done: an advisor pinned to a chat by the harness, giving a sub-agent the chat's history or a digest, a person writing to a sub-agent-only agent, a rule the harness enforces that an agent must consult first.

## Safeguards and abuse tests

**Done on branch `feat/subagent-comfort`** (README "Delegating agents", docs/agent-pages.md).
An agent made on the Agents page carries an optional `delegates` list (API `delegate_to`, at most 32 handles, no repeats, not itself; a new entry must be an agent offered as a sub-agent in that scope); at turn time it only narrows what `with_layers` already offers (`crates/agents/src/edits.rs`), so an entry that is gone, not offered, from another layer or the agent itself changes nothing, and the list is optional (absent: every sub-agent; present: only those, and `[]` means nobody), so a list whose entries are all gone, or emptied by a delete, leaves no delegates; deleting an agent removes its handle from every list that could name it.
The page shows it as a multi-select under the switches (built-ins read-only, gone entries marked).
The `delegate` card names the model of the child's chat (read from its log once) and shows the child's report open as a message through the same `Markdown` component (cut with "Show all"); in the nested chat of an agent with `as_agent: false` the reply box is replaced by one line and a link to the parent (the UI reads the same fact the server refuses on; no new field).
No threat row: nothing here widens what an agent may do.


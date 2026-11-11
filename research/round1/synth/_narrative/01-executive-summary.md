## 1. Executive summary

Seventeen harnesses were surveyed from public documentation on 2026-11-10, a quarter of the entries re-checked by independent reviewers, and the results merged into 332 cross-harness features. Four conclusions should change what we do next.

### What should change our plans

1. **Merge the sandbox before claiming any security lead.** On `main` the shell runs unconfined as the node's user and can read the provider key. Our compensating controls are permission layers, and Claude Code, Cursor, Oh My Pi, Cline and OpenClaw say in writing that permission layers are not a security boundary (see F34 in `round1/synth/02-permissions-sandbox-security.md`). Pull request 4 adds a fail-closed Landlock sandbox; its residuals (pathname Unix sockets before Linux 7.1, UDP and DNS) are why Codex rejects Landlock (https://learn.chatgpt.com/docs/sandboxing.md). Merge it, then report per node whether enforcement is full or partial (DeepSeek harness).
2. **Fix four correctness gaps before adding features:** compaction keyed to a fixed estimate with no overflow recovery (fifteen of sixteen key it to the real window), no provider retry, no input while a turn runs (twelve of sixteen accept it), and no web tools or memory proposals on agentic nodes. The last one breaks the new roster: Software Developer has no web search and a delegated Researcher has no web tools.
3. **Decide where model credentials live, before Gate 3 work on the link.** No other surveyed design lets the execution side outrank the control side (see the control-side table in `round1/synth/09-remote-distributed-architecture.md`). But the field is moving keys off workers, and every node of ours holds a long-lived provider key. A node-local credential proxy outside the sandbox, as Hermes Agent documents, keeps authority on the node. The evidence does not settle this; the owner must.
4. **Trim the roster and give each auxiliary task its own model slot.** Smol is a slot dressed as an agent; Senior Teamlead duplicates Software Developer; a Plan agent with a node-enforced plan mode is missing. Slots should default to the main model with their own fallback chain (Hermes), not share one "small" key (Claude Code's coupling trap; see "Auxiliary model slots" in `round1/synth/04-planning-subagents-background.md`).

### What the ecosystem has converged on

MCP and `SKILL.md` skills (sixteen of sixteen), `AGENTS.md` instruction files (fourteen), approval modes and per-command rules (fifteen), automatic compaction (sixteen), sub-agents with a per-agent model, custom agent files, plan mode, scheduled runs and webhook triggers. Workers dial out to the control side; servers bind to loopback by default; credentials move behind proxies (see `round1/synth/_parts/common.md`).

### Where we stand

Strengths: `node.toml` is the only authority, which no peer documents; the owner runs the control side, so transcripts stay home; multi-user accounts with private chats in the open tier; child processes start from an empty environment; `web_fetch` refuses URLs that did not come from the user or a tool; agent-written memory needs owner approval, which only Antigravity and Hermes (opt-in) also require; hardened shadow-repo checkpoints; unattended routines that fail safe.

Gaps: no shell sandbox on `main`; no MCP, skills, hooks or repository instruction files; no plan mode; a diff view showing one edit call; approvals bound to a tool name for a whole turn; provider keys on every node.

### How far to trust this

This is documentation, mostly read through a summarising fetch tool, not code. About 626 of roughly 2,400 entries were reviewed; status labels were the commonest error. "Nothing documented" is the weakest evidence and was wrong at least once. Claude Code's survey is ten times larger than the others, which biases comparisons. Round 2 reads source for the open-source harnesses; twenty questions are shortlisted.

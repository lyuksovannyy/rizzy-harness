# X.2 **sec**

## Adds

Instruction files and skills, text only

## Safeguards and abuse tests

Node-controlled roots; repository skills only after a per-project trust decision bound to content; capped catalog; bodies through the approval path; nothing runs at load; instructions survive compaction (B.9). `10` §6.3 item 5. **Instruction files: done** (2026-11-14): the node reads `AGENTS.md` (else `CLAUDE.md`) in each folder from the repository top down to the session folder, capped at the smaller of 32 KiB and about a tenth of the model's known window with a note, again every turn (5 s timeout), into the per-turn context (so it survives compaction), sub-agents and callback turns too; only regular files inside the repository and the granted folder, never URL provenance (`crates/node/src/instructions.rs`, PLAN.md T44, README "Repository instructions").
Cost: about 1k tokens for a typical file, up to the cap, sent with the first message and again only when it changes (X.1).
**Folders below the session folder: done** (2026-11-15, B.9, PLAN.md T65): loaded by the loop when a call first touches a file there, with their own caps (16 KiB a file, 32 KiB and 16 files together), the same reading rules and no trust decision.
Later: a per-file cap for the root files, `@` imports, more file names (RI-8), the turn's line in the UI naming the files, the 100-file rule below.
**The node operator's file: done** (2026-11-15): node.toml `[instructions] user_file`, default `AGENTS.md` in the node's protected data directory (Codex's `$CODEX_HOME/AGENTS.md`, Claude Code's `~/.claude/CLAUDE.md`, DeepSeek's home `AGENTS.md`: research/round2/codex.agents.md 160-172, research/round1/claude-code.md 1266-1296, research/round2/deepseek-harness.agents.md 178-190), its own standing section ahead of the repository's files with its own 16 KiB cap outside their 32 KiB (Codex keeps it out of the project budget), read every turn, replaced or withdrawn when it changes, the regular file at that path only (no link followed), for every chat on the node (PLAN.md T62, README "Your own instructions").
Not done: per-user files on a shared node.
**Skills: done** (2026-11-14): `SKILL.md` folders (the Agent Skills format other harnesses read) in `.agents/skills` and `.claude/skills` of each repository folder from the top down to the session folder and in node.toml `[skills] roots` (default `~/.agents/skills`, `~/.claude/skills`; the node's roots win a name), found every turn in the instruction files' read; a catalogue of names and descriptions at the end of their block (at most 150 skills and 8,000 bytes or about 2% of the window, shortened then cut with a note: Codex's budget, OpenClaw's count and degradation); the read-only `skill` tool returns a body (24 KiB cap) with its folder and files, and reads those files (`file`) inside the skill's folder only; `disable-model-invocation` honoured; `/name` at the start of a message asks for a skill (the `/` menu is X.65); the coding summary names skills still followed;
Developer, Teamlead, Writer, Reviewer, Scout and custom agents that touch files have the tool (`crates/tools/src/skills.rs`, PLAN.md T46, README "Skills").
**The per-project trust decision was dropped:** skills carry no more authority than `AGENTS.md`, which ships without one; node.toml turns repository skills (`repository = false`) or all skills (`denied_tools`) off.
Bodies are read-only tool calls under the normal approval path.
Later: nested skill folders (Codex recurses to depth 6), skills from other harnesses' folders (`.codex`, `.opencode`, `.cursor`), the catalogue is sent only when it changes (X.1, done).

## Invariants

Node decides

## Podman / Node

No / Yes

## Depends

X.1, B.9

## Size

**M**


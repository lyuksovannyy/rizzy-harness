## 10. Source references

### Where the references are

This report cites a URL for each claim it makes, but it is not the reference list. Every finding's evidence is in its row in the ten synthesis files (`round1/synth/01-*.md` to `10-*.md`), one row per harness with its own URL and check value. Every URL a survey agent opened is listed, with its page title and what it was used for, in the "Sources" section (section 8) of that harness's survey file. Reviewers' re-checks are recorded in each survey's `## 9. Review` section and, for Claude Code, in the three review files.

Two surveys abbreviate their URLs, which is why their counts below are low: Hermes Agent cites paths relative to `https://hermes-agent.nousresearch.com/docs/`, and OpenHands writes `D:` for `https://docs.openhands.dev`. The synthesis files expand or repeat these as the surveys wrote them.

### Primary source roots per harness

Docs roots, repositories and changelogs are from `round1/00-targets.md`. "URLs in file" is the count of distinct URLs in that survey file, from `round1/synth/_parts/stats.json`.

| Harness | Docs root | Repository | Changelog or releases | Survey file | URLs in file |
|---|---|---|---|---|---|
| Codex | https://learn.chatgpt.com/docs (index https://learn.chatgpt.com/docs/llms.txt) | https://github.com/openai/codex | https://learn.chatgpt.com/docs/changelog, https://github.com/openai/codex/releases | `round1/codex.md` | 151 |
| OpenCodex | https://opencodex.me/ | https://github.com/lidge-jun/opencodex | https://github.com/lidge-jun/opencodex/releases | `round1/opencodex.md` | 44 |
| OpenCode | https://opencode.ai/docs/ (index https://opencode.ai/docs/llms.txt) | https://github.com/anomalyco/opencode | https://opencode.ai/changelog, https://github.com/anomalyco/opencode/releases | `round1/opencode.md` | 74 |
| Claude Code | https://code.claude.com/docs/en/overview (index https://code.claude.com/docs/llms.txt) | https://github.com/anthropics/claude-code (changelog, plugins, examples; no product source) | https://code.claude.com/docs/en/changelog, https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md | `round1/claude-code.md` | 263 |
| Claude Desktop | https://code.claude.com/docs/en/desktop | none public | https://support.claude.com/en/articles/12138966-release-notes, https://claude.com/docs/cowork/changelog.md | `round1/claude-desktop.md` | 65 |
| DeepSeek harness | https://deepseek-harness.github.io/deepseek-harness/ (index https://deepseek-harness.github.io/deepseek-harness/llms.txt) | https://github.com/deepseek-ai/deepseek-harness | no changelog page; git tags | `round1/deepseek-harness.md` | 75 |
| Oh My Pi | https://omp.sh/docs | https://github.com/can1357/oh-my-pi | GitHub releases (no changelog page on omp.sh) | `round1/oh-my-pi.md` | 61 |
| Pi | https://pi.dev/docs/latest | https://github.com/earendil-works/pi | https://pi.dev/changelog | `round1/pi.md` | 91 |
| Cursor | https://cursor.com/docs (index https://cursor.com/llms.txt) | https://github.com/cursor/cursor (no licence, no code releases) | https://cursor.com/changelog, https://cursor.com/docs/cli/changelog | `round1/cursor.md` | 84 |
| Antigravity | https://antigravity.google/docs (index https://antigravity.google/llms.txt) | https://github.com/google-antigravity/antigravity-cli (distribution only), https://github.com/google-antigravity/antigravity-sdk-python | https://antigravity.google/docs/changelog | `round1/antigravity.md` | 71 |
| OpenHands | https://docs.openhands.dev/ (index https://docs.openhands.dev/llms.txt) | https://github.com/OpenHands/OpenHands | https://github.com/OpenHands/OpenHands/releases | `round1/openhands.md` | 31 (abbreviated) |
| OpenClaw | https://docs.openclaw.ai/ (index https://docs.openclaw.ai/llms.txt) | https://github.com/openclaw/openclaw | https://docs.openclaw.ai/releases | `round1/openclaw.md` | 138 |
| goose | https://goose-docs.ai/ (index https://goose-docs.ai/llms.txt) | https://github.com/aaif-goose/goose | https://github.com/aaif-goose/goose/releases | `round1/goose.md` | 102 |
| Hermes Agent | https://hermes-agent.nousresearch.com/docs (index https://hermes-agent.nousresearch.com/docs/llms.txt) | https://github.com/NousResearch/hermes-agent | GitHub releases | `round1/hermes-agent.md` | 5 (relative paths) |
| Zed/ACP | https://zed.dev/docs/ai/overview, https://agentclientprotocol.com/ | https://github.com/zed-industries/zed | https://zed.dev/releases/stable | `round1/zed-acp.md` | 98 |
| Warp | https://docs.warp.dev/ (index https://docs.warp.dev/llms.txt) | https://github.com/warpdotdev/warp | https://docs.warp.dev/changelog | `round1/warp.md` | 115 |
| Cline | https://docs.cline.bot/ (index https://docs.cline.bot/llms.txt) | https://github.com/cline/cline | https://github.com/cline/cline/releases | `round1/cline.md` | 93 |

### The study's own files

All paths are under `research/`.

| File | What it is |
|---|---|
| `round1/00-targets.md` | Scout output: identity, vendor, surfaces, licence, newest release and primary URLs for each harness; the additions, what was skipped and why. |
| `round1/SURVEY-BRIEF.md` | The instructions every per-harness survey agent followed. |
| `round1/REVIEW-BRIEF.md` | The instructions every independent reviewer followed. |
| `round1/SYNTH-BRIEF.md` | The instructions every synthesis agent followed, including the canonical short names. |
| `round1/codex.md` ... `round1/cline.md` | Seventeen per-harness surveys, each with a `## 9. Review` section except Claude Code. |
| `round1/claude-code.review-1.md`, `-2.md`, `-3.md` | The three independent reviews of the Claude Code survey; their correction lists override the survey text. |
| `round1/own-rizzy-harness.md` | Our own inventory, read from branch `main` at commit `a268048`, with an "unmerged branches" section for `integration/gate3` and `feat/agent-roster`. |
| `round1/synth/01-loop-tools-editing-shell.md` | Synthesis: agent loop, tools, editing, shell (61 features). |
| `round1/synth/02-permissions-sandbox-security.md` | Synthesis: permissions, sandboxing, security and privacy (34 features). |
| `round1/synth/03-context-memory.md` | Synthesis: context management, memory and instructions (26 features). |
| `round1/synth/04-planning-subagents-background.md` | Synthesis: planning, sub-agents, background work, model slots; holds the roster and slot tables (42 features). |
| `round1/synth/05-sessions-git.md` | Synthesis: sessions, checkpoints, git (29 features). |
| `round1/synth/06-extensibility.md` | Synthesis: MCP, skills, hooks, plugins, SDKs, ACP (35 features). |
| `round1/synth/07-models-observability.md` | Synthesis: models, providers, usage, cost, telemetry (25 features). |
| `round1/synth/08-ui-browser-collaboration.md` | Synthesis: UI surfaces, browser and computer use, teams (24 features). |
| `round1/synth/09-remote-distributed-architecture.md` | Synthesis: remote, headless, CI, distributed execution; holds the control-side versus execution-side table (33 features). |
| `round1/synth/10-config-licence-distinctive.md` | Synthesis: configuration, licence, everything else (23 features). |
| `round1/synth/_parts/*.md`, `stats.json` | Mechanical extracts of the synthesis files (common, single-harness, ours, corrections, Round 2 questions, distinctive, overhyped, index, matrices) and the counts used in this report. |
| `round1/synth/_narrative/*.md` | The prose sections of this report. |

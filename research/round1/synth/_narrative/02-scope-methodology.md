## 2. Research scope and methodology

### What was studied

Round 1 covers seventeen harnesses as they were documented on 2026-11-10. Ten were named by the owner and seven were added by the scout stage.

The owner's ten names, and how each was pinned to one product (all from `round1/00-targets.md`):

| Name given | Resolved to | How the ambiguity was settled |
|---|---|---|
| Codex | the whole OpenAI Codex family: open CLI, SDK and App Server in `openai/codex`, plus the closed IDE extension, Codex Cloud, mobile and the Codex mode of the ChatGPT desktop app | The docs moved from `developers.openai.com/codex` into `learn.chatgpt.com/docs`, and the standalone desktop app became a mode of the ChatGPT desktop app. Surveyed as one family. |
| OpenCodex | `lidge-jun/opencodex`, a local provider gateway with a web dashboard | No project of this name is a harness. Four candidates were found; this one is the only one with real adoption and daily releases. Confidence is medium-low. `RyensX/OpenCodex` (browser remote control for Codex Desktop) is the alternative and was partly covered as Part B of the survey. **The owner must confirm which one was meant.** |
| OpenCode | `anomalyco/opencode` (formerly `sst/opencode`), V2 docs | The archived Go project `opencode-ai/opencode` (now Crush) is unrelated. V1 pages are still served and were kept apart. |
| Claude Code | Anthropic's Claude Code, including the Agent SDK | The public repository holds the changelog and plugins, not the product source. |
| Claude Desktop | Claude Desktop: Chat, Cowork and the Code tab | Surveyed as "Claude Code plus a desktop shell plus Cowork", recording only what the desktop adds. |
| DeepSeek harness | DeepSeek Harness (`dsh`), DeepSeek's own open-source product | The brief's hypothesis that this was a community project was wrong. Community DeepSeek-first agents (Codewhale, DeepSeek-Reasonix, deepcode-cli) are listed in `00-targets.md` in case the owner meant one of them. |
| Oh My Pi | `can1357/oh-my-pi` (`omp`) | A hard fork of Pi that is now its own distribution with a Rust core; not a GitHub fork object. |
| Pi | `earendil-works/pi`, owned by Earendil Inc. | Moved from `badlogic/pi-mono`; Inflection's Pi assistant ignored. |
| Cursor | Cursor, now owned by SpaceX | Includes Cloud Agents and Self-Hosted Machines. |
| Antigravity | Google Antigravity: App, CLI `agy`, IDE, extensions, SDK | No longer only an IDE; Gemini CLI was folded into the Antigravity CLI. |

The seven additions, chosen for a self-hosted Rust harness with a web dashboard and a master/node split, with weight on remote execution, web UI, sandboxing, approvals, sub-agents, memory and extensibility, then permissive licence and being alive:

| Addition | Why it was added |
|---|---|
| OpenHands (Agent Canvas) | The same product shape: self-hosted web control centre over pluggable execution backends. |
| OpenClaw | The same topology: one gateway with paired nodes that declare capabilities, exec approvals, a web control UI. |
| goose | The mature Rust harness under foundation governance, ACP-first. |
| Hermes Agent | Strongest on memory and agent-written skills; credential-injecting egress proxy. |
| Zed agent and ACP | Rust, per-tool permissions, and the protocol most harnesses here now speak. Copyleft: ideas only. |
| Warp | Rust, cloud agent fleet with a published API. AGPL client: ideas only. |
| Cline | The reference approval and checkpoint UX among open IDE agents, now also remote. |

OpenClaw and Hermes Agent are general personal agents first. They are in scope because rizzy-harness is also an assistant, but the scout flagged that the owner named only coding tools; Amp and jcode were the proposed replacements if the owner wants coding harnesses only.

### What was skipped and why

From `round1/00-targets.md`:

| Skipped | Reason |
|---|---|
| Gemini CLI | Superseded for consumers by the Antigravity CLI on 2026-06-18. |
| Roo Code | Repository archived; last release 2026-05-15. |
| Continue | README says read-only and no longer maintained. |
| Aider | Stalled; last release 2025-08-09. Worth reading once for repo map and edit formats, not a live competitor. |
| Windsurf | Domain redirects to Devin Desktop. |
| Qwen Code, Kimi Code, ZCode, Mistral Vibe, MiniMax Code | Live but tied to one model family; nothing distinctive found. |
| Junie, Auggie | Closed, proprietary. |
| ForgeCode, Every Code | Small, or a Codex fork. |
| Amp, GitHub Copilot CLI and cloud agent, jcode | Recommended as a half-size next tier; not surveyed in this round. |
| T3 Code, Grok Build, Codewhale, Kilo Code, Crush, Devin, Factory Droid, Kiro | Skip unless a specific question comes up (T3 Code for web UX, Kiro for specs, Codewhale for its receipts log). |

### The four stages

| Stage | Output | What it did |
|---|---|---|
| 1. Scout | `round1/00-targets.md` | Identified each harness, its vendor, surfaces, licence, newest release, and the primary URLs to start from. Repository figures came from the GitHub API on 2026-11-10. |
| 2. Per-harness survey | seventeen files `round1/<slug>.md` | One agent per harness, working to `round1/SURVEY-BRIEF.md`: an inventory organised by 23 areas, one entry per feature with what it does, its status and the URL it came from, plus sections on what is distinctive, architecture, relevance to us, overhyped claims, unverified claims and sources. |
| 3. Independent review | a `## 9. Review` section in sixteen survey files; three separate review files for Claude Code | Reviewers who did not write the surveys, working to `round1/REVIEW-BRIEF.md`, re-opened the cited URLs and asked for the specific fact. Verdicts: confirmed, confirmed-with-correction, not-supported, wrong, unreachable, status-wrong. Corrections were applied in place; unsupported claims moved to the Unverified section. |
| 4. Synthesis by feature area | ten files `round1/synth/01-*.md` to `10-*.md` | One agent per group of areas, working to `round1/SYNTH-BRIEF.md`, merged the seventeen surveys into 332 cross-harness features, each with one row per harness, an availability matrix, and sections on what is common, what only one harness has, ours against the field, corrections, and Round 2 questions. |

### The numbers

From `round1/synth/_parts/stats.json` and the review sections of the survey files.

- **2,393 entries** across the seventeen surveys. This is a mechanical count of `###` headings, which in several files includes the 23 area headings (the DeepSeek harness reviewer counts 100 headings of which about 23 are area headings). The reviewers' own estimates of feature entries add up to about 2,130.
- **1,561 URLs**, the sum of the distinct URLs counted per survey file. The count understates two files: Hermes Agent (5) cites docs paths relative to its docs root, and OpenHands (31) uses a `D:` shorthand for its docs root.
- **About 626 items checked in review**, which is about a quarter of the 2,393 headings and closer to 29% of the reviewers' own entry counts. Per file: Antigravity 27, Claude Code 161 items across three reviews (52, 47 and 62; review 3's 62 items cover about 52 entries), Claude Desktop 30, Cline 25, Codex 47, Cursor 28, DeepSeek harness 30, goose 29, Hermes Agent 27, Oh My Pi about 30, OpenClaw 28, OpenCode 31, OpenCodex 21, OpenHands 23, Pi 30, Warp 28, Zed/ACP 31.
- **332 features** in ten synthesis files: loop, tools, editing and shell 61; permissions, sandboxing and security 34; context and memory 26; planning, sub-agents and background work 42; sessions and git 29; extensibility 35; models and observability 25; UI, browser and collaboration 24; remote and distributed architecture 33; configuration, licence and other 23.

### The rules

- **Public sources only.** Official sites and docs, READMEs, changelogs, release notes, official blog posts, repository descriptions. **No source code was read in this round**, including for the open-source harnesses. Third-party articles were allowed only to find a primary source.
- **One entry per feature, with the URL actually opened.** No URL, no claim. Anything the agent believed but could not confirm went to an Unverified section.
- **Status labels** per entry: GA, beta, experimental, preview, deprecated, planned, unclear. A roadmap item is planned, never GA. Marketing statements without a mechanism are tagged `vendor-claim`.
- **Trust labels** carried into the synthesis for every row: `reviewed` (confirmed in review), `corrected` (changed in review; the corrected text is used), `unreviewed` (not checked), `unverified` (nobody could confirm it).
- **Absence is weak evidence.** "Nothing documented" means no page was found, never that the feature does not exist.
- **Ours** is described only from `round1/own-rizzy-harness.md`, which was read from branch `main` at commit `a268048` with `path:line` citations. Nothing was run or compiled for it.

### Judgement calls

- **File names.** Each survey uses a short slug (`codex.md`, `deepseek-harness.md`, `zed-acp.md` for Zed plus ACP in one file). The synthesis uses one fixed list of short names and one fixed column order, given in `round1/SYNTH-BRIEF.md`.
- **Features are merged, evidence is not.** Where six harnesses do something similar, there is one feature entry and six rows, each with its own mechanism, status, URL and check value. Differences in mechanism are the point and are kept in the rows and in a "Variants" paragraph.
- **The detail lives in the ten synthesis files**, not in this report. Each is between about 110 KB and 185 KB. One document holding all of them would be about 1.5 MB and unreadable. This report carries the conclusions and points to the feature entries by name and file.
- **Claude Code was surveyed differently.** Its survey was produced by thirteen sub-agents and is about 900 KB, roughly ten times the size of the others (most are 55 to 100 KB). It contains duplicate entries for one feature that sometimes disagree, and it was reviewed in three separate files whose correction lists override the survey text, rather than edited in place. The extra volume reflects the size of Claude Code's documentation, not a judgement that it matters ten times more; but it does mean Claude Code rows are more detailed than anyone else's, which biases comparisons toward "Claude Code documents X".
- **OpenCodex and Zed/ACP** are included where they have something real (a gateway's routing, a protocol's semantics), not for symmetry. Most counts in the synthesis use sixteen "real harnesses", excluding OpenCodex.

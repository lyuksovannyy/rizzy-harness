# AI coding harnesses: Round 1 research (public sources)

**Research date:** 2026-11-10. **Status:** Round 1 complete, awaiting the owner's review. Round 2 (source code) has not started.

**What this is.** A survey of what seventeen AI coding harnesses document today, consolidated into 332 cross-harness features and compared with our own harness, rizzy-harness. Everything here comes from public sources (sites, docs, READMEs, changelogs, release notes). No source code was read in this round.

**How the study is laid out.**

| File | Contents |
|---|---|
| `research/harnesses-round-1.md` (this file) | The report: summary, method, overview, the feature index, the availability matrices, findings, opportunities, limits, sources |
| [`harnesses-round-1-appendix.md`](harnesses-round-1-appendix.md) | Full lists: features only one harness has, ours against the field per area, corrections and unverified claims, common features, every distinctive and overhyped item, all Round 2 questions |
| [`round1/synth/`](round1/synth/) | Ten synthesis files holding the evidence: one entry per feature, one row per harness with its own source URL |
| [`round1/`](round1/) | The seventeen per-harness survey files with their review sections, our own inventory, the target list, and the three briefs the agents worked to |

**Judgement calls made without asking.** (1) The three versions of the brief named different files; this study uses `research/harnesses-round-1.md`. (2) Features are merged across harnesses, but every harness keeps its own row and its own source: evidence is grouped, never collapsed. (3) The detailed evidence is about 1.5 MB, so it stays in the ten synthesis files and this report indexes it instead of repeating it. (4) "OpenCodex" was surveyed as `lidge-jun/opencodex`, a provider gateway, with a short section on the alternative project; the owner should confirm which was meant. (5) Seven harnesses were added to the owner's ten, and several candidates were skipped; section 2 gives the reasons.

**How to read a claim.** A claim about another harness is only as good as its row in the synthesis files: each row carries a status label and a check value (`reviewed`, `corrected`, `unreviewed`, `unverified`). About a quarter of all entries were independently checked. "Nothing documented" means no page was found, not that a feature is absent.

## Contents

1. Executive summary
2. Research scope and methodology
3. Harness overview
4. Consolidated feature taxonomy
5. Feature availability matrix
6. Interesting or uncommon capabilities
7. Notable architectural and UX approaches
8. Potential opportunities for our project
9. Limitations and research gaps
10. Source references
11. Features only one harness has
12. Corrections and claims that could not be verified
13. What surprised us and what looks overhyped
14. Recommended focus for Round 2

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

## 3. Harness overview

Identities, licences and versions are from `round1/00-targets.md`, corrected where a survey or review found it wrong (noted in the table). "Entries" is the mechanical heading count from `stats.json` (it includes area headings in some files); "Checked" is the number of entries or claim groups a reviewer re-opened, from each survey's review section.

| Harness | Vendor or maintainer | What it is | Surfaces | Open or closed | Licence | Version surveyed (date) | Survey file | Entries | Checked |
|---|---|---|---|---|---|---|---|---|---|
| Codex | OpenAI | coding agent family | CLI (`codex exec`), IDE extension, ChatGPT desktop app (Codex mode), Codex Cloud, mobile, SDK, App Server, GitHub Action | mixed: CLI, SDK, App Server, Codex Security CLI/SDK open; IDE extension, cloud and desktop closed | Apache-2.0 (open parts) | `rust-v0.167.0` (2026-11-06) | `codex.md` | 161 | 47 |
| OpenCodex | `lidge-jun` (identity assumed, owner to confirm) | local provider gateway and proxy; **not a harness** | CLI `ocx`, local proxy, web dashboard, remote hub, experimental Remote Workspace | open | MIT | `v2.100.1` (2026-11-10) | `opencodex.md` | 36 | 21 |
| OpenCode | `anomalyco` organisation (formerly `sst`) | open coding agent with a background server and thin clients | TUI/CLI, desktop (beta), web mode, mobile, HTTP API, SDK, ACP | open | MIT | `v2.2.7` (2026-11-10); V1 line `v1.19.5` (2026-10-28) | `opencode.md` | 110 | 31 |
| Claude Code | Anthropic | agentic coding tool, one engine across surfaces | CLI, Desktop Code tab, VS Code, JetBrains, web cloud sessions, mobile, Remote Control, self-hosted runners (beta), Agent SDK | closed: public repo is "All rights reserved"; Agent SDK Python is MIT, TypeScript "All rights reserved" | proprietary | `v2.1.322` (2026-11-10) | `claude-code.md` | 806 | 161 items in three reviews |
| Claude Desktop | Anthropic | desktop app holding Chat, Cowork and the Code tab | macOS, Windows, Linux (beta); Dispatch from a phone | closed | proprietary | `v4.23118.1` (2026-11-09; the scout had it as unverified) | `claude-desktop.md` | 94 | 30 |
| DeepSeek harness | DeepSeek AI | "everything is a plugin" harness, developer preview | web UI, desktop (macOS, Windows), headless, ACP, SDK over stdio, Python SDK | open | MIT (first npm version listed BSD-3-Clause; not traced) | prerelease `dsh-v0.2.2-rc.1` (2026-11-04); no stable release (the scout's "no GitHub releases" was wrong) | `deepseek-harness.md` | 100 | 30 |
| Oh My Pi | can1357 / Stencil Labs | hard fork of Pi with a Rust core | TUI/CLI, RPC, SDK, ACP, collab | open | MIT | `v18.18.6` (2026-11-09) | `oh-my-pi.md` | 112 | about 30 |
| Pi | Earendil Inc. (created by Mario Zechner) | minimal, extensible harness with documented non-features | TUI, print and JSON modes, RPC, TypeScript SDK | open | MIT | `v1.5.2` (2026-11-10) | `pi.md` | 104 | 30 |
| Cursor | Cursor (acquired by SpaceX, 2026-08-14) | AI editor plus cloud and self-hosted agents | IDE, Agents Window, CLI `agent`, Cloud Agents, Self-Hosted Machines, iOS, Bugbot, SDKs, ACP | closed | proprietary | changelog 2026-11-06; CLI `v2026.10.22` | `cursor.md` | 95 | 28 |
| Antigravity | Google | agentic coding platform on one harness | App, CLI `agy`, IDE, extensions, Python SDK, Remote Agents, Remote Control | closed core; Python SDK open | proprietary; SDK Apache-2.0 | App `v2.31.0` (2026-11-09); CLI `1.5.5` (2026-11-10) | `antigravity.md` | 93 | 27 |
| OpenHands | OpenHands (formerly All-Hands-AI) | self-hosted web control centre (Agent Canvas) over pluggable backends | Canvas web UI, Agent Server, SDK, CLI ("no longer actively maintained") | open core: Canvas, SDK, Agent Server, CLI open; Cloud and Enterprise commercial | MIT per READMEs | `v1.38.0` (2026-11-10) | `openhands.md` | 89 | 23 |
| OpenClaw | OpenClaw (a 501(c)(3) foundation employs the core team) | personal agent: one Gateway with paired nodes | Gateway daemon, CLI, Control UI, macOS app, nodes, chat channels | open | MIT | `v2026.11.2` (2026-11-08) | `openclaw.md` | 112 | 28 |
| goose | Agentic AI Foundation / Linux Foundation (formerly Block) | Rust harness where everything is ACP | desktop, CLI, `goose serve`, ACP, GDK Rust crates | open | Apache-2.0 | `v1.59.0` (2026-11-10; the scout listed `v1.58.1`) | `goose.md` | 99 | 29 |
| Hermes Agent | Nous Research | general agent with memory, agent-written skills and a messaging gateway | CLI/TUI, desktop, gateway on about 28 platforms, API server, Docker and remote terminal backends | open | MIT | `v0.23.1` (2026-11-10) | `hermes-agent.md` | 126 | 27 |
| Zed/ACP | Zed Industries | editor agent, plus the Agent Client Protocol | Zed agent panel, external agents, remote development; ACP spec, registry and SDKs | open | GPL and Apache licence files at the root (copyleft: ideas only) | Zed `v1.27.0` (2026-11-04) | `zed-acp.md` | 47 | 31 |
| Warp | Warp (Denver Technologies) | terminal with an agent and a cloud agent platform | terminal, Warp Agent, `oz` CLI, cloud agents, Factories, self-hosted workers | client open since 2026-04-28; control plane and cloud services closed | AGPL-3.0 with MIT UI crates (ideas only) | changelog 2026.10.22 (`v0.2026.10.21.08.25`) | `warp.md` | 114 | 28 |
| Cline | Cline | IDE agent that grew a CLI, desktop, Kanban and SDK | VS Code and JetBrains extension, CLI/TUI, desktop with SSH remote, cloud agents, Kanban, SDK, ACP | open | Apache-2.0 | `v4.1.26` (2026-10-26) | `cline.md` | 95 | 25 |

### What each one is best understood as

**Codex.** A family, not a product. The open Rust CLI and its App Server protocol are the part worth reading; one JSON-RPC protocol carries the TUI, IDE, desktop and SDK, but OpenAI labels that App Server experimental and "not supported for production" (https://learn.chatgpt.com/docs/app-server.md). Its docs are unusually candid about the limits of its own sandbox (https://learn.chatgpt.com/docs/permissions.md).

**OpenCodex.** A provider gateway that lets Codex, Claude Code and other clients run on any model provider. It runs no agent loop of its own (a reading of its how-it-works and architecture pages, corrected in review: not a quoted claim). It matters for its routing, sidecars and remote hub, not as a harness. The owner must confirm that `lidge-jun/opencodex` is the project meant; `RyensX/OpenCodex` is the other candidate.

**OpenCode.** One background server per machine that owns sessions and permissions, with TUI, web and desktop as clients (https://opencode.ai/docs/troubleshooting/). Its durable sequence-numbered session log, separate from a volatile broadcast stream, is the closest public analogue to our fold-over-log UI (https://opencode.ai/docs/api/). It discontinued session sharing in v1.19.1 (https://opencode.ai/changelog/v1.19.1, corrected in review).

**Claude Code.** The most documented harness in the set and the one that ships most mechanisms first: auto-mode classifier, hook `defer`, credential masking, self-hosted runners, workflows. It is closed, its docs lag its changelog, and several headline features (agent view, background sessions) are research preview, not GA (corrected in review). Its self-hosted runner inverts "the node decides": the control plane sends each session's tool list and permission rules (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected in review).

**Claude Desktop.** Claude Code with a desktop shell, plus Cowork for non-coding work. What it adds is UX: diff comments, a browser pane, scheduled tasks, SSH sessions, Dispatch from a phone (https://code.claude.com/docs/en/desktop). Cowork runs either in a local VM or in the cloud; the getting-started article that says cloud only is stale (corrected in review).

**DeepSeek harness.** An open, preview-stage harness whose design notes are the best match to our event-log discipline: approval decisions and mode changes are log events, a failed audit append rejects the request (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval). Its own `SAFETY.md` says it has had no security audit (https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md).

**Oh My Pi.** Pi's maximalist fork: ten model roles, hash-anchored edits, compaction method chains, internal URL schemes, a Rust core. Its default approval mode is `yolo` (https://omp.sh/docs/approvals). Its README and docs disagree with each other often, so treat README claims as unverified.

**Pi.** The minimalist reference: no built-in permissions, no plan mode, no sub-agents, each a documented non-feature with a substitute (https://pi.dev/). It reversed its position on MCP and now ships it behind code mode and deferral (https://pi.dev/docs/latest/mcp).

**Cursor.** An editor company whose self-hosted workers are the clearest inversion of our design: the loop and inference stay in Cursor's cloud and the customer's machine only executes tools over one outbound connection (https://cursor.com/docs/cloud-agent/self-hosted). Closed; docs read through a summarising fetcher.

**Antigravity.** Google's closed platform, with a strong command-permission parser and plan and artifact review UX (https://antigravity.google/docs/permissions, https://antigravity.google/docs/plan). Its own pages call its browser "sandboxed" while describing a browser that drives the user's real Chrome profile (https://antigravity.google/docs/commands/browser).

**OpenHands.** The closest product shape to ours: a browser client over independent agent backends, each with its own secrets, plus per-conversation containers. Its open tier has no authentication by default and its FAQ calls it a single-user local tool (https://docs.openhands.dev/overview/faqs.md, corrected in review).

**OpenClaw.** The closest topology to ours: one Gateway, nodes that dial in and declare commands, two-layer pairing, exec approvals bound to argv, cwd, env and executable (https://docs.openclaw.ai/tools/exec-approvals). Authority sits on the Gateway, not the node. Its trust-model page is the most candid in the set (https://docs.openclaw.ai/gateway/security/trust-model).

**goose.** A Rust harness that made ACP its only protocol, including the only shipped ACP-over-HTTP/WebSocket transport (https://goose-docs.ai/docs/gdk/acp/index.md). Its survey's "nothing documented" claims are the least reliable in the set: the reviewer found the sandbox one wrong in part and asked for a second look at all of them.

**Hermes Agent.** A general agent with the most developed memory and skill management (write-approval staging, a curator with a hash ledger and rollback) and a credential-injecting egress proxy (https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy). Its docs describe a very wide surface; some features may be thin.

**Zed/ACP.** Two things: Zed's own agent (per-tool regex permissions, a scoped sandbox, a removed checkpoint feature) and ACP, the editor-to-agent protocol that puts file and terminal execution on the client and leaves policy storage to the agent (https://agentclientprotocol.com/protocol/v1/security.md). Copyleft: ideas only.

**Warp.** A terminal with an open client and a closed cloud platform that pushes resolved config to self-hosted workers (https://docs.warp.dev/platform/architecture/). Its code review panel and handoff snapshot are worth studying; its "open" covers the client only. AGPL: ideas only.

**Cline.** The open IDE agent with the reference approval and checkpoint UX, now with a CLI, desktop SSH remote and a Kanban. Its documented defaults contradict its own overview page (auto-approve on in the CLI, schedules default to yolo); see Cline in `round1/synth/_parts/overhyped.md`.

## 4. Consolidated feature taxonomy

### How the 332 features are organised

The surveys used 23 areas. The synthesis grouped them into ten files, each holding the evidence tables for its features:

| File in `round1/synth/` | Areas | Features |
|---|---|---|
| `01-loop-tools-editing-shell.md` | 1-4: agent loop, built-in tools, editing, shell | 61 |
| `02-permissions-sandbox-security.md` | 5, 21: permissions, sandboxing, security and privacy | 34 |
| `03-context-memory.md` | 6, 7: context management, memory and instructions | 26 |
| `04-planning-subagents-background.md` | 8-10: planning, sub-agents, background and scheduled work, plus model slots | 42 |
| `05-sessions-git.md` | 11, 12: sessions, checkpoints, git | 29 |
| `06-extensibility.md` | 13: MCP, skills, hooks, plugins, SDKs, protocols | 35 |
| `07-models-observability.md` | 14, 19: models and providers, usage, cost, logs, telemetry | 25 |
| `08-ui-browser-collaboration.md` | 15, 16, 18: UI surfaces, browser and computer use, teams | 24 |
| `09-remote-distributed-architecture.md` | 17: remote, headless, CI, distributed execution | 33 |
| `10-config-licence-distinctive.md` | 20, 22, 23: configuration, licence, everything else | 23 |

### How to read a feature entry

Each feature in a synthesis file has the same shape:

- **A name and "What it is"**: one to three vendor-neutral sentences.
- **Variants**: the differences in mechanism that matter (who decides, where it runs, the default, what it fails to cover). Similar names often hide different mechanisms; this paragraph is where that is stated.
- **One row per harness**, each with its own mechanism, status label, evidence URL and check value. Evidence is never merged: six harnesses with the same feature have six rows and six URLs.
- **An "ours" row**, taken only from `round1/own-rizzy-harness.md` (branch `main`, cited as `path:line`), or "not in the inventory".

Each file also has an availability matrix (`Y` documented and released, `B` beta, experimental, preview or partial, `P` planned or vendor-claim only, `-` nothing documented, `?` unverified), and sections on what is common, what only one harness has, ours against the field, corrections, and Round 2 questions. Those sections are excerpted later in this report.

### Check values

| Value | Meaning |
|---|---|
| `reviewed` | A reviewer re-opened the cited page and found the claim as written. |
| `corrected` | A reviewer found a detail wrong (a default, a status, a scope) and the corrected text is the one used. |
| `unreviewed` | Nobody checked it. Most rows are in this state; reviewers sampled about a quarter of all entries. |
| `unverified` | The survey or the review could not confirm it on a primary page, often because a page was missing, truncated or only seen as a search snippet. |

### Status labels

`GA`, `beta`, `experimental`, `preview`, `deprecated`, `planned`, `unclear`, as the vendor's page states them. Where a page gives no label, the row says so ("GA (no label)" or "unclear"). Status labels were the most common thing reviews corrected (for example Claude Code agent view from GA to research preview, OpenClaw cloud workers from GA to beta), so treat an unreviewed status as a guess. For ours: `works`, `partial`, `stub`, `docs-only`, `none`, and "branch only" for work that exists only on an unmerged branch.

### "Nothing documented" is not "absent"

A `-` or "nothing documented" means the survey found no page describing the feature. It does not mean the harness lacks it. This is the weakest kind of evidence in the study: at least one such claim was wrong (goose's sandbox), and another hid a removal (OpenCode's session sharing was discontinued, not undocumented). Read counts such as "12 of 16" as lower bounds.

The full list of features, with a one-line definition each, follows.

### Agent loop, tools, editing, shell (areas 1-4)

Evidence tables, one row per harness with its own source: [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md)


**Area 1. Agent loop and execution model**

- **Loop structure, turns and stop reasons**: how the harness defines one unit of work (turn, step, item) and how a run reports why it ended.
- **Where the loop runs relative to the tools**: which process owns the loop, which owns tool execution, and what the clients are.
- **Step, turn and budget limits**: hard caps on model calls, turns, spend or wall-clock time inside one run.
- **Stuck-loop and stalled-run detection**: automatic detection of a run that repeats itself or has stopped making progress.
- **Interrupting a running turn**: stopping the current model call and tools without losing the session.
- **Steering and message queueing while a turn runs**: accepting user input during a running turn, either injected into the turn (steer) or held for later (queue).
- **Parallel tool calls**: running several tool calls from one model response at the same time.
- **Goal and objective-continuation loops**: a persisted objective that keeps the agent working across turns until a completion check passes.
- **Time-driven re-submission loops**: re-running a prompt in the same session on a timer or after each turn, independent of an objective check.
- **Retry, backoff and stream-failure handling**: what the loop does when the provider fails or a stream stalls.
- **Multi-model turn strategies (handoff, mixture, forcing)**: using more than one model inside a single run or turn.
- **Event log as the loop's source of truth, one writer per session**: the session is an append-only log the model context and UI are derived from, with a defined writer.
- **Harness-injected context and message provenance**: content the harness adds to the conversation, and whether the model or SDK can tell it from user text.
- **Evaluator outside the main loop (asynchronous monitor, critic)**: a second process judging the run's safety or success without being the approver of each action.
- **Tool-call execution pipeline**: the ordered stages a tool call passes between the model's request and the result.
- **Deliberate turn-ending semantics (forced answer, silent finish, finish tools)**: ways a turn ends by design rather than by the model simply replying.

**Area 2. Built-in tools**

- **Built-in tool inventory (comparison)**: which tools ship with the agent, by category. Only Claude Code, OpenCode, Oh My Pi, Pi, Zed, OpenClaw, Cline and Hermes publish a tool list; Codex, Antigravity, OpenHands, goose, DeepSeek (generated catalogue, not default-mounted) and Warp give partial or no list.
- **Tool-set composition: defaults, presets and per-model gating**: how the set of tools actually offered to the model is chosen.
- **Deferred tool loading (tool search)**: keeping most tool schemas out of the prompt and letting the model look them up.
- **Model-written scripts that call tools (code mode, programmatic tool calling)**: the model writes a short program that calls other tools, so many calls cost one turn and only the script output re-enters the context.
- **Ask-the-user tool**: a tool call that pauses the loop to ask the user a structured question.
- **Todo and task-list tool**: a model-maintained task list shown to the user.
- **Web fetch and web search tools (with per-session caps)**: tools that read a URL or query the web.
- **Code-navigation tools (LSP, structure, AST, outline)**: tools that give the model symbols, definitions or structure instead of raw text search.
- **Tool result size limits and spill-to-file**: capping what a tool result adds to the context and keeping the rest retrievable.
- **Tool risk classification metadata**: tool-level metadata saying whether a call reads, writes or executes, used for approvals or concurrency.
- **Internal URL schemes inside file tools**: virtual paths (`pr://`, `conflict://`, `agent://`, `xd://`) that file-shaped tools read and write instead of dedicated tools.

**Area 3. Code editing and file operations**

- **Edit format: exact search/replace of a string**: the model sends the old text and the new text; the tool replaces an exact match.
- **Edit format: patch (apply_patch)**: the model emits a multi-file patch (add, update, delete) that the tool applies.
- **Edit format: hash-anchored lines (hashline)**: every line shown to the model carries a short content hash; edits cite the hash instead of reproducing old text; a changed file makes hashes mismatch and the edit is rejected.
- **Edit format: structural (AST) edits**: syntax-aware codemods instead of text replacement.
- **Whole-file write and create**: creating a file or replacing it entirely.
- **Model-routed choice of edit tool**: the harness picks which edit tool a session offers based on the model family.
- **Failed-edit handling, staleness and read-before-edit guards**: what the tool does when the old text is missing or ambiguous, when the file changed since the model read it, or when parallel edits race.
- **Encoding and line-ending safety**: avoiding silent corruption when a tool rewrites a file that is not plain UTF-8 or uses CRLF.
- **Post-edit diagnostics and formatters**: feeding type errors or formatter results back after a write.
- **Diff review and in-app file viewing or editing**: showing what the agent changed and letting the user accept, reject or edit.
- **Write confinement and protected paths for file tools**: limits on where the file tools may write, enforced in the tool rather than by an OS sandbox.
- **Read-tool capabilities and limits**: what the read tool returns (text, images, PDFs, notebooks, remote files) and how it bounds it.
- **File mentions and attachments**: attaching files, folders, URLs or images to a prompt from the UI.
- **Notebook support**: cell-level notebook reading or editing.
- **Git conflict resolution through virtual paths**: resolving merge conflicts by writing to session-local URLs rather than editing marker text.
- **Debug mode with log instrumentation**: a guided loop that inserts logging, has the user reproduce, reads logs, fixes, and removes the instrumentation.
- **Inline selection edits and edit prediction (editor-native)**: prompt-driven in-place transforms of a selection, and tab-style completions.
- **Per-session scratch directory**: a temp directory the agent may use without permission prompts.
- **Office-document deliverables**: producing spreadsheets, decks and documents rather than code files.

**Area 4. Terminal and shell integration**

- **Shell execution model (process per command or persistent shell)**: how one shell tool call maps to processes, and what state (cwd, env, aliases) survives between calls.
- **Command timeouts and auto-backgrounding**: how long a command may run and what happens at the limit.
- **Background processes**: starting a long-running command without blocking the loop, then reading, polling or killing it.
- **Agent-driven interactive terminal (PTY)**: the agent opens a real terminal, types into it and reads the screen, so it can drive REPLs, editors and debuggers.
- **User-facing terminal pane in the app**: an interactive terminal the user can use alongside the agent.
- **User shell escape (`!` commands)**: the user runs a command directly from the prompt without a model turn, optionally adding the output to context.
- **Shell output limits and spill**: the cap on command output the model sees and where the rest goes.
- **Command-output filtering and minimisation**: rewriting command output to cut noise before it reaches the model.
- **Environment given to spawned commands**: which environment variables a tool-spawned process inherits, and which markers it receives.
- **Resource limits for commands**: capping memory or CPU of tool-spawned processes.
- **Pluggable execution backends for shell and file tools**: choosing where commands and file operations run (local, container, SSH, remote worker) without changing the agent.
- **PowerShell and Windows shell support**: running the shell tool natively on Windows rather than through WSL.
- **Shell-rc and terminal-emulator integration**: hooking the agent into the user's own terminal or telling the terminal what the agent is doing.
- **Monitor: background command output as conversation events**: a tool that watches a command or socket and turns each output line into an event the model sees mid-conversation.
- **Hosting other CLI agents inside a terminal**: running a third-party agent CLI as a first-class thread or session inside the host app.

### Permissions, sandboxing, security (areas 5, 21)

Evidence tables, one row per harness with its own source: [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md)


**Area 5. Permissions, approvals and sandboxing**

- **F1. Approval modes and the default mode**: a named, session-wide setting that decides which tool calls run without asking, which ask a human, and which are refused. The default mode is what a new user gets without touching anything.
- **F2. Per-tool and per-command permission rules**: a rule language that allows, asks or denies specific tools, commands, paths, domains or MCP tools, with a defined precedence.
- **F3. Shell-command parsing: chains, wrappers and hidden-execution constructs**: how a command rule copes with `a && b`, pipes, subshells, `$(...)`, wrappers like `timeout`, and flags that run other programs (`git -c core.pager=...`).
- **F4. What an approval is bound to**: when a user clicks "allow", what exactly is remembered: the tool, a command prefix, the exact argv, the working directory, the executable's identity, and for how long (once, turn, session, forever).
- **F5. Model-based approvers and risk classifiers**: a second model (or the main model) decides whether an action runs, instead of or before a human.
- **F6. Hardcoded floors no mode can lift**: a fixed set of commands or actions that stay prompted or refused even in the most permissive mode.
- **F7. Protected paths inside writable areas**: paths that stay write-protected even where the agent may otherwise write, because writing them turns into code execution or policy change (git hooks, agent config, shell rc files).
- **F8. Workspace boundary for file tools**: the folder set the agent's built-in file tools may touch without asking, and what happens outside it.
- **F9. Administrator and managed policy layers**: a policy source that outranks the user's own settings, set by an organisation or machine administrator.
- **F10. Repository config cannot widen policy**: settings committed in a cloned repository (or created by the agent inside it) must not grant themselves more access.
- **F11. OS sandbox for agent shell commands**: kernel-enforced confinement (filesystem, network, sometimes process) of the commands the agent runs.
- **F12. Behaviour when the sandbox is unavailable**: what happens on a machine where the sandbox cannot start (old kernel, user namespaces blocked, unsupported OS).
- **F13. Sandbox network egress controls**: limiting where sandboxed commands may connect, usually through a host-side proxy and a domain allowlist.
- **F14. Model-requested widening and sandbox escape hatches**: a sanctioned way for the agent to ask to run one command with more access than the sandbox gives.
- **F15. Container, VM and remote-executor isolation**: running the agent's tools (or the whole agent) in a container, microVM or separate machine so the host is the boundary.
- **F16. Sub-agent permission inheritance**: whether a delegated agent can be more permissive than its parent.
- **F17. Unattended and headless approval behaviour**: what a run with nobody to click does when an action needs approval.
- **F18. Approvals delivered to another device or channel**: answering an approval prompt from a phone, web dashboard or chat app while execution stays on the host.
- **F19. Hooks and extensions in the permission path**: user or admin code that runs before a tool call and can allow, deny, ask or rewrite.
- **F20. Permission and approval decisions as logged events**: every policy change, approval request and decision recorded in an append-only log or audit stream.

**Area 21. Security and privacy**

- **F21. Child-process environment scrubbing**: removing secrets from the environment that the agent's commands inherit.
- **F22. Credential placeholders and injection at an egress proxy**: the agent's processes never hold the real secret; a proxy outside the agent's boundary adds it to outbound requests for allowed hosts.
- **F23. Secret redaction in output, logs and telemetry**: masking known secret values or secret-shaped strings before they reach the model, the transcript, logs or the vendor.
- **F24. Storage of the harness's own credentials**: where provider keys and login tokens live on disk.
- **F25. Prompt-injection defences for untrusted content**: measures that stop text from web pages, tool output, files or other users from steering the agent into harmful actions.
- **F26. Sub-agent and inter-agent output hygiene**: treating a sub-agent's or peer agent's report as data, not as instructions with user authority.
- **F27. SSRF and private-address guards on fetches**: refusing fetches to loopback, private ranges and cloud metadata endpoints, including after redirects and DNS rebinding.
- **F28. Project and workspace trust prompts**: before loading configuration, hooks, MCP servers or skills from a folder, the user confirms they trust it.
- **F29. Trust for extensions, plugins, hooks and marketplaces**: how the harness decides whether to run third-party code it did not write (plugins, MCP servers, hooks, skills).
- **F30. Authentication of the harness's own server and web UI**: how the harness's local or self-hosted HTTP/WebSocket server decides who may use it.
- **F31. Authentication of remote links (nodes, workers, remote control)**: how a worker, executor or remote-control client proves its identity to the central server, and the reverse.
- **F32. Telemetry defaults and opt-outs**: what the harness sends to its vendor by default, and how to stop it.
- **F33. Data retention and local data at rest**: what session data the harness keeps, where, for how long, encrypted or not, and how to delete it.
- **F34. Documented trust model and stated non-boundaries**: the vendor's own written statement of what its controls do not protect against.

### Context management and memory (areas 6, 7)

Evidence tables, one row per harness with its own source: [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md)


**Area 6: context management and compaction**

- **Automatic compaction (trigger, who compacts, what is kept)**: when the conversation nears the model window, older history is replaced by a model-written summary and the run continues.
- **Manual compaction and partial compaction**: a user command that compacts now, sometimes with a focus instruction or a partial range.
- **Staged, fallback and overflow-recovery compaction**: more than one compaction method, or a defined recovery when summarising fails or the provider rejects the request as too long.
- **Idle, background and incremental compaction**: compacting before the user needs it, to save cache-miss cost or latency.
- **Pruning or masking of old tool output**: shrinking or removing old tool results from the prompt without a summary call.
- **Large-output handling (caps, spill to file, retrieval)**: bounding what one tool result can put in the prompt and keeping the full text retrievable.
- **Prompt caching (layout, TTL, warming, diagnostics)**: keeping the request prefix byte-stable so the provider's prefix cache hits, and managing its lifetime.
- **Context usage display**: showing the user how full the window is, or what fills it.
- **Repository maps and code indexing**: giving the agent a ranked summary or searchable index of the repository so it need not discover files by search alone.
- **Context cost of tool and skill catalogs (lazy schemas, budgets)**: limiting what tool definitions, skill lists and rule files cost before the first prompt.
- **Hooks, plugins and templates that change compaction**: extension points around compaction (replace the summary, change the prompt, observe it, swap the whole context engine).
- **Side questions, recaps and fresh-context hand-offs**: ways to get an answer or a clean context without compacting the main thread.

**Area 7: memory and persistent instructions**

- **Instruction files: which file names are read**: plain-text project or user files loaded into the prompt every session.
- **Instruction scopes, precedence, imports and conditional loading**: how several instruction files combine, and when they load.
- **Instruction size limits and trust of loaded files**: caps on how much instruction text is injected and whether a repo's own files are trusted.
- **Generating and auditing instruction files (`/init`, prompt audit)**: commands that write or lint the instruction file.
- **Agent-written memory: storage format and what is injected**: notes the agent (or a background job) writes for later sessions.
- **Who approves an agent memory write**: whether the human or a policy gate sits between the agent proposing a memory and it entering the prompt.
- **Memory scopes (user, workspace, project, repository, team, agent)**: how far a memory reaches and who can see it.
- **Recall, memory back ends and embeddings**: how stored memory is found again.
- **Background consolidation and self-review of memory**: a non-interactive pass that extracts, merges or prunes memory outside the user turn.
- **Agent-written skills as memory (procedural memory)**: the agent turns lessons or procedures into reusable skill files instead of facts.
- **Memory content safety (untrusted text, secrets, injection)**: treating stored memory as potentially attacker-influenced input and keeping secrets out.
- **Memory and instruction opt-outs**: switches that turn memory or instruction loading off, per user, chat, workspace or organisation.
- **Memory import, export and portability**: moving memory or instruction setups between tools or accounts.
- **Searching past sessions as memory**: recall by searching stored conversation history instead of curated notes.

### Planning, sub-agents, background work (areas 8-10)

Evidence tables, one row per harness with its own source: [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md)


**Area 8. Planning, tasks and modes**

- **Plan mode (restricted planning phase)**: a session state in which the agent investigates and writes a plan but may not change the project until a human approves the plan.
- **Plan review and hand-off**: how the user inspects, edits, comments on and accepts a plan, and what context the implementation then starts with.
- **Model-maintained task / to-do list**: a tool the model writes to keep a visible checklist of steps and their status.
- **Goal / run-until-done loop**: a standing objective that keeps the agent taking turns until a check says the objective is met, a budget runs out, or a human stops it.
- **Read-only, ask and custom modes**: named modes other than plan that change what the agent may do or how it behaves (ask-only, chat-only, debug, profiles, presets).
- **Interview-first planning commands**: a command that makes the agent question the user (and the code) before planning.
- **Side questions (`/btw`, `/side`)**: ask something without polluting the main conversation.
- **Orchestration modes and trigger keywords**: a mode or a typed keyword that makes the main agent decompose the task and delegate, instead of the model deciding case by case.

**Area 9. Sub-agents and multi-agent orchestration**

- **Sub-agent delegation tool**: a model-callable tool that starts a child agent on a task and gives the parent its result.
- **Built-in agent roster**: the named agents a harness ships, what each is for, and where it may run.
- **Custom agent definition files**: user- or project-authored agent definitions (prompt, tools, model, limits).
- **Sub-agent context: fresh, forked or inherited**: what the child sees when it starts.
- **Sub-agent tools and permissions**: which tools a child gets and whose approval rules apply to it.
- **Sub-agent model and effort resolution**: how the child's model and effort are chosen.
- **Result return and output hygiene**: what comes back to the parent and how it is checked.
- **Background sub-agents and resumption**: children that run while the parent continues, and children that can be continued later.
- **Depth, concurrency and spend limits**: caps on nesting depth, parallel children and cost.
- **Isolation for sub-agents (worktrees, overlays, VMs)**: giving a writing child its own copy of the code so parallel edits do not collide.
- **Agent teams, swarms and shared boards**: several long-lived agents with names that coordinate through a shared task list, mailbox or board, rather than one parent with throwaway children.
- **Cross-session messaging and message buses**: agents or sessions sending messages to each other outside a parent/child call.
- **Script-orchestrated workflows**: the model writes a program (or a declarative recipe) that fans work out to many agents, so intermediate results stay out of the model's context.
- **Running other vendors' harnesses as sub-agents or runtimes**: driving Claude Code, Codex, Gemini CLI, OpenCode and others as children or as the agent runtime.
- **Coordinator agents that delegate and do not code**: a long-running top-level agent that routes work into separate worker sessions or threads and watches them.
- **Built-in parallel fan-out commands**: a shipped command that runs one task as many parallel agents (best-of-n, review fleets, split-change).
- **Advisor and watchdog models**: a second model that watches or is consulted during the main agent's work and injects guidance.
- **Agent-to-agent peering protocols**: calling an independently deployed agent as a peer over a protocol, not as a child.

**Model roles and slots for auxiliary work (cross-cutting; mostly Area 14)**

- **Auxiliary model slots**: named model assignments for work other than the main turn (titles, summaries and compaction, vision, goal judging, commit messages, planning, sub-agents, approval classifiers).
- **Fallback chains for the main model**: an ordered list of models (or accounts) to try when the primary fails.
- **Phase-based model switching**: changing model by phase of the task (plan with a strong model, execute with a cheaper one).

**Area 10. Background, long-running and scheduled work**

- **Background shell jobs**: long-running commands (dev servers, watchers) that run while the agent continues, with output the agent can poll.
- **Background agent sessions and supervisor daemons**: an agent session that runs without an attached UI, hosted by a long-lived local process so sessions outlive the client.
- **Agent dashboards and command centres**: one view of many running agents with status (working, needs input, done) and quick reply.
- **Auto-backgrounding of long tool calls and server-side tasks**: a long tool call stops blocking the turn and reports back later.
- **In-session schedules, loops and reminders**: timers that wake the **same** session later or repeatedly.
- **Standalone scheduled runs (cron automations)**: saved prompts that start a **fresh** agent run on a schedule, without a user present.
- **Event triggers and inbound webhooks**: external events (HTTP calls, GitHub, chat, tickets) that start an agent run.
- **Permission posture of unattended runs**: what an unattended scheduled or triggered run may do without a human.
- **Cloud background agents**: vendor-hosted runs that continue with the user's computer off (summary only; remote execution is covered by another group).
- **Durable and crash-surviving runs**: what survives a process crash or restart in the middle of a run.
- **Auto-continue after a usage limit**
- **Keep the machine awake**
- **Completion notifications**

### Sessions and git (areas 11, 12)

Evidence tables, one row per harness with its own source: [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md)


**Area 11: Sessions**

- **Session storage: format, location and shape**: where a conversation lives on disk (or server) and whether it is a structured event log, a transcript, a database, or a set of files.
- **Resume and continue**: reopening an earlier session and continuing it with its history, and what state comes back with it.
- **Fork: copy a session into a new session**: creating a second, independent session from a prefix (or all) of an existing one.
- **In-file session trees (branching without copying)**: the session log is a tree; rewinding and continuing creates a sibling branch in the same file instead of a new session.
- **Conversation rewind and edit-and-resubmit**: cutting the conversation back to an earlier message and continuing, possibly with an edited prompt, without necessarily touching files.
- **Export of a session**: writing a session out as a file for reading, archiving or analysis.
- **Session sharing by link**: giving someone else a view of a session via a URL.
- **Search across sessions and the session picker**: finding an earlier session by text, title, branch or PR.
- **Moving a session between machines or surfaces (handoff)**: continuing the same task on another host or surface (local, cloud, SSH host, messaging channel), with or without the git state.
- **Importing sessions from other harnesses**: reading another harness's saved sessions.
- **Archive, delete, retention and ephemeral sessions**: lifecycle controls for stored sessions.
- **Single writer, concurrency guard and crash recovery**: preventing two processes from appending to one session and repairing a session after a crash.
- **Pluggable external session storage**: letting the host replace local transcript files with its own store.
- **File checkpoints and undo: the mechanism**: automatic capture of file state around agent edits so a turn can be undone or a session rewound, separate from the user's own VCS.
- **Checkpoint coverage, retention, costs and removals**: what a checkpoint system does not restore, how long it keeps snapshots, and what it costs.
- **SDK file checkpointing and rewind**: a programmatic restore point per user message, usable from a host application.

**Area 12: Git and version control**

- **Worktrees as the isolation unit for parallel agents**: running a session or sub-agent in its own git worktree so parallel agents do not edit the same files.
- **Worktree setup and copying ignored files**: making a fresh worktree usable (env files, dependencies) without checking secrets in.
- **Worktree lifecycle: cleanup, snapshots before removal, caps**: what happens to worktrees when the session ends, is archived or goes stale; whether dirty work can be lost.
- **Git repository requirement and non-git folders**: whether the harness works (and what it loses) outside a git repository.
- **Commits and commit-message generation**: the harness creating commits, or generating a message for the user's commit.
- **Pull-request creation and PR checkout**: opening a PR from the harness, or checking one out as a session.
- **PR status tracking, auto-fix and auto-merge**: following a PR after it is opened: CI status, review comments, fixing and merging.
- **Local code review command**: a read-only reviewer run on the working tree, a branch diff or a commit from inside the harness.
- **Hosted PR review bots and CI actions**: an automated reviewer running on a hosting-platform event (PR opened, push, comment) and posting to the PR.
- **Cloud multi-agent deep review (Ultrareview)**: a fleet of reviewer agents in a cloud sandbox, each finding independently reproduced.
- **Diff views**: viewing what the agent changed, per file, per turn or per branch, with the ability to comment, revert or apply.
- **Attribution, signing and provenance in commits and PRs**: marking agent-made commits and PRs, and proving who made them.
- **Git host integrations and credential handling**: how the harness talks to GitHub/GitLab/others, and where git credentials live when work runs remotely.

### Extensibility (area 13)

Evidence tables, one row per harness with its own source: [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md)


**2.1 MCP as a client**

- **MCP client: transports and configuration scopes**: the harness connects to Model Context Protocol servers, local (stdio child process) or remote (HTTP), and offers their tools to the model. Configuration lives in one or more files whose scope (user, project, managed) decides who can add a server.
- **MCP remote authentication (OAuth and headers)**: signing the harness in to a remote MCP server, usually OAuth 2.0 with discovery, plus static or helper-generated headers.
- **Tool deferral and tool search**: instead of sending every tool schema each turn, the harness sends names (or nothing) and lets the model search for and load schemas on demand. The main answer to MCP context bloat.
- **MCP protocol features beyond tools**: the rest of MCP: prompts, resources, elicitation (server asks the user for input), roots, sampling (server asks the harness's model), `list_changed`, MCP Apps (`ui://` interactive UI), and the tasks extension for long jobs.
- **MCP limits, timeouts and connection lifecycle**: wall-clock and idle timeouts per call, startup waits, output caps, reconnect and retry rules.
- **MCP approval, trust and admin allow/deny lists**: how MCP tool calls are approved, how a project-supplied server is trusted before it starts, and how admins restrict servers.
- **Connector directories and MCP registries**: a browsable catalog of MCP servers or connectors that users can enable from inside the product.
- **MCP servers that push events into a session**: an MCP server that can inject messages (CI results, chat messages, alerts) into a running session rather than only answering calls.

**2.2 The harness as an MCP server**

- **Exposing the harness over MCP**: the harness runs as an MCP server so another client can use its tools or conversations.

**2.3 Skills**

- **Skill format and discovery**: a folder with `SKILL.md` (YAML frontmatter plus Markdown instructions) and optional scripts and references, discovered from user, project and bundled locations. The Agent Skills format (agentskills.io) is the shared base; `.agents/skills/` is the shared project path.
- **Skill loading, listing budget and invocation control**: how much of each skill sits in context (usually name plus description), how the body is loaded, and who may invoke it (user, model, both).
- **Skills that run shell commands while loading**: placeholders inside a skill or command that run a shell command before the prompt reaches the model, splicing the output in.
- **Agent-written skills and extensions**: the agent creates or edits its own skills, rules or extensions, turning experience into reusable procedures.
- **Skill registries and account sync**: a remote source of skills, either a public registry or skills synced from a user's account.

**2.4 Plugins and extensions (packaging and distribution)**

- **Plugin bundles: what a plugin can contain**: one installable unit that packages skills, commands, agents, hooks, MCP servers and sometimes code, themes or UI.
- **Plugin distribution and marketplaces**: where plugins come from (registry, git, npm, local folder), how a catalog is described and how updates arrive.
- **Plugin trust, isolation and supply-chain controls**: what stops an installed plugin, skill or extension from doing harm, and what the docs admit.
- **Managed (admin) controls over extensions**: settings that only an administrator source can set, restricting which hooks, plugins, skills and MCP servers may load.

**2.5 In-process middleware**

- **In-process extension API (code hooks into the loop)**: extension code loaded into the harness process that can observe, rewrite or answer engine events (prompts, tool calls, model requests, compaction) and register tools, commands and UI.

**2.6 Hooks**

- **Hook events and what a hook can decide**: user-configured handlers fired at lifecycle events (session start, prompt submit, before and after a tool, stop, compaction) that can block, rewrite, add context or force continuation.
- **Hook handler types**: what a hook actually runs: a command, an HTTP call, an MCP tool, a single LLM prompt, or a sub-agent.
- **Hook trust, precedence and failure behaviour**: whether a hook from a repo or plugin runs without review, how hooks from several sources combine, and what happens when a hook crashes or times out.

**2.7 Custom commands and prompt templates**

- **Custom slash commands and prompt templates**: user-defined `/name` commands that expand into a prompt, usually Markdown files with argument placeholders.

**2.8 Custom tools**

- **Custom tools defined in code**: tools defined by users or integrators as code (not via a separate MCP process) and offered to the model next to built-ins.

**2.9 Code mode**

- **Code mode (the model writes code that calls tools)**: instead of one tool call per turn, the model writes a script that calls tools, and only the script's output returns to context. Cuts schema cost and round trips.

**2.10 SDKs and embedding**

- **Embeddable SDK (agent as a library)**: a library that runs the harness's agent loop inside another program.
- **Programmatic control protocol (headless JSON streams, RPC, app-servers)**: a wire protocol a separate process uses to drive the agent: start turns, stream events, answer approvals.
- **OpenAI-compatible endpoint where the "model" is an agent**: an HTTP endpoint shaped like OpenAI's API that runs a full agent per request, so any OpenAI client can call the harness.
- **Structured output for programmatic callers**: the caller supplies a JSON Schema and the harness guarantees (or retries towards) a final answer that validates.

**2.11 Client protocols**

- **ACP: the harness as an ACP agent**: the Agent Client Protocol (JSON-RPC 2.0, modelled on LSP) lets an editor launch an agent as a subprocess over stdio. The agent runs the loop; the client does file and terminal I/O and shows permission prompts. v1 is stable; v2 is a draft (2026-07-20); stdio is the only standardised transport; remote Streamable HTTP/WebSocket is an RFD.
- **ACP: the harness as an ACP client hosting other agents**: the harness launches another vendor's agent over ACP and renders its thread, so one UI can drive Claude, Codex, Gemini and others.
- **Hosting other harnesses through their own interfaces (not ACP)**: running another vendor's agent as a backend through its own app-server, CLI or SDK, or wrapping it in a terminal.
- **Agent-to-Agent (A2A) protocol**: Linux Foundation protocol for agent-to-agent delegation (agent cards, JSON-RPC, push notifications); distinct from ACP.

**2.12 Reading other harnesses' configuration**

- **Reading other harnesses' files in place**: the harness reads instruction files, skills, MCP configs, hooks or plugin layouts that belong to another harness, without converting them.
- **One-shot import from another harness**: a command that copies another harness's setup (and sometimes chats) into this one once.

### Models and observability (areas 14, 19)

Evidence tables, one row per harness with its own source: [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md)


**Area 14: models, providers and inference configuration**

- **Provider support and bring-your-own-key**: which model providers a harness can talk to with a key the user supplies, and how that key is stored and resolved.
- **Cloud-platform providers and enterprise bring-your-own-cloud**: using models served from the customer's own AWS, Google Cloud or Azure account instead of the vendor's API.
- **Custom endpoints, wire formats and gateways as a provider**: pointing the harness at any HTTP endpoint that speaks a known wire protocol (Chat Completions, Responses, Messages), and how it copes with quirks.
- **Local models**: running or connecting to a model on the user's own hardware.
- **Using another harness or a subscription as a provider**: three different things share this name: (a) signing in with a consumer subscription (ChatGPT, Claude, Copilot, SuperGrok) so the harness bills against plan limits; (b) wrapping another harness or CLI as the model backend; (c) exposing your own subscription to other tools as a local proxy.
- **Model selection per session, per agent and per task**: choosing which model answers, at which scope (whole install, workspace, session, agent, sub-agent, single turn), and what happens on resume.
- **Reasoning effort and thinking controls**: a user-facing control for how much a model reasons before answering.
- **Speed tiers (fast mode)**: a paid, faster serving configuration of the same model.
- **Model fallback chains**: automatically retrying a request on a different model when the chosen one fails or is unavailable.
- **Provider retry, backoff and timeouts**: re-sending the same request to the same provider after a transient error, and how long to wait or give up.
- **Routing across providers and credential pools**: spreading requests over several providers, accounts or keys by rules, quota or cost.
- **Model roles for auxiliary work**: using a different, usually cheaper or stronger, model for side tasks (titles, compaction, summaries, classification, approval review, advice).
- **Vision and other modalities**: image input, image generation, and handling for models that lack vision.
- **Voice (dictation, text-to-speech, realtime)**: speech in or out of the agent. Three distinct mechanisms: dictation into the prompt, speech output, and realtime voice conversation.
- **Hosted model offerings (commercial services)**: model access sold by the harness vendor itself. **Everything in this section is a commercial service, not a harness feature.** Prices are as stated on vendor pages on 2026-11-10 and change often.
- **Model catalogue, capability discovery and pinning**: how the harness learns what each model can do (context, vision, tools, effort, price) and how users freeze or relabel models.
- **Model and provider governance (allowlists, locks)**: admin controls over which models, providers or effort levels users may use.
- **Prompt-cache controls and cache-aware behaviour**: settings and design choices that keep provider prompt caches warm (a cost lever).

**Area 19: observability (usage and cost, logs, tracing, debugging)**

- **Token and cost display**: showing the user how many tokens and how much money a turn, session or run used.
- **Budgets and limits**: caps that stop or warn on spend, tokens or time, set by the user or an admin, as distinct from plan quotas set by a vendor.
- **Usage dashboards and quota views**: an aggregated view across sessions, users or accounts, in the product or via API.
- **OpenTelemetry and other export**: sending traces, metrics or events to an external collector or analytics service.
- **Logs, transcripts and raw request logging**: durable records a developer can read after the fact: application logs, per-session transcripts, and raw provider requests.
- **Debugging and diagnostics tools**: commands that check installation, configuration and connectivity, or profile a running process.
- **Vendor telemetry and data-upload defaults**: what the harness itself sends to its vendor by default, and how to turn it off. Distinct from the user-configured export above. Included because a self-hosted harness's stance here is a visible product choice.

### UI, browser, collaboration (areas 15, 16, 18)

Evidence tables, one row per harness with its own source: [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md)


**Area 15. UI surfaces**

- **Terminal UI**: an interactive full-screen or scrollback terminal front end to the agent loop, with prompt editing, status line and transcript tools.
- **IDE integrations**: a panel or plugin inside an editor that hosts the agent and shares selection, open files and diagnostics.
- **Desktop apps**: a native or Electron shell around the agent, usually adding panes (diff, terminal, browser, files), notifications and multi-session management.
- **Web UIs (what the web UI can do)**: a browser client for sessions. The questions that matter for a web-first harness: which UI is open source and self-hostable, whether it has authentication, and which of sessions, diffs, approvals, file browsing, terminal and settings it covers.
- **Mobile**: a phone client, or phone access to a session that runs elsewhere, so you can start, steer and approve work away from the machine.
- **Voice**: dictation into the prompt, or a conversational voice loop.
- **Messaging-app channels**: using a chat platform (Slack, Telegram, Discord, Teams and so on) either as the agent's primary UI or as a trigger that starts a cloud task.
- **Approval presentation (how a permission request reaches a human)**: the prompt that asks a human to allow a tool call: what it shows, what answers exist, and where it can be answered from.
- **Diff presentation and review (per-file/hunk accept and reject, comments on diffs)**: showing what the agent changed and letting the human keep, revert or comment on it.
- **Plan review (artifacts and comments on plans)**: a plan is produced before editing and the human approves, edits or comments on it first.
- **Notifications and "waiting for you" signals**: telling the human that a run finished, errored or is blocked on an approval or question, especially when the tab or terminal is not focused.
- **Managing several agents at once**: one place to launch, watch and switch between multiple concurrent agent sessions.

**Area 16. Browser automation and computer use**

- **The agent's own browser (embedded or managed)**: a browser the harness starts and controls (headless or visible), separate from the user's day-to-day browser.
- **The user's real browser (extension, DevTools attach)**: driving the browser the user already uses, with its logged-in sessions, through an extension or the DevTools protocol.
- **Browser isolation, site policy and credential handling (what the docs say)**: statements about what a browser tool can reach and what limits apply: separate storage, per-site approval, origin lists, SSRF rules, credential filling.
- **Computer use (screen control)**: the agent sees screenshots and sends clicks and keystrokes to a desktop or app.

**Area 18. Collaboration, teams and enterprise administration**

- **Accounts, roles, organisations and workspaces**: more than one human using the same harness with separate identities, roles and scopes.
- **Single sign-on and provisioning**: login through the company identity provider, plus automatic account creation and removal (SCIM).
- **Managed configuration and policy distribution**: an admin sets rules that users and repositories cannot override, delivered by file, MDM, a server, or a self-hosted config service.
- **Shared team resources (rules, prompts, servers, plugins)**: things a team defines once and every member gets: instruction files, prompt libraries, MCP servers, skills, plugin marketplaces, environments.
- **Budgets and usage controls per team**: limits on spend or usage per user, group or organisation, and how they are enforced.
- **Audit logs**: a record of who did what that an admin can export, as opposed to the operator's own debug log.
- **Session sharing and live collaboration**: letting another person see, steer or continue a session.
- **Tier summary: which of the web UI and multi-user features are in the open or free tier**

### Remote execution and architecture (area 17)

Evidence tables, one row per harness with its own source: [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md)


**2.1 Headless and non-interactive modes**

- **One-shot headless run**: run one prompt (or a recipe) without an interactive UI, print the answer and exit, for scripts and CI.
- **Structured event stream and schema-checked output**: machine-readable output (JSON, NDJSON event stream) and an optional JSON Schema the final answer must satisfy.
- **Approval behaviour when nobody can answer**: what a headless or unattended run does when a tool call needs approval.
- **Config isolation for reproducible CI runs**: a way to run headless without picking up the user's or the repo's ambient configuration, so CI results do not depend on the machine.

**2.2 CI integrations**

- **Official CI action or pipeline integration**: a packaged way to run the agent inside a CI job (GitHub Actions, GitLab CI) triggered by events or mentions.
- **Machine credentials for CI and automation**: how a non-human run authenticates to the model and to the vendor's control plane.
- **Event-triggered remote runs (chat, tracker, git host, webhook)**: starting an agent run from an external event (Slack mention, Linear assignment, PR mention, signed webhook) rather than from the harness UI.

**2.3 Client protocols for remote control**

- **Programmatic control protocol (RPC, HTTP API, SDK transport)**: a documented protocol a separate process (UI, SDK, wrapper, remote client) uses to drive sessions: start, prompt, stream events, answer approvals, cancel.
- **ACP (Agent Client Protocol) support**: speaking the open editor-to-agent protocol, as an agent (served to editors) or as a client (hosting other agents).
- **Remote attach of a client to an agent server on another machine**: running the agent server on one machine (often a VM) and pointing a TUI, desktop or browser client at it.
- **Relay-based remote control of a session from web or phone**: a session keeps running on the user's machine while a browser or phone views and steers it through a hosted relay, without opening inbound ports.
- **Pairing and authentication of remote clients and execution hosts**: how a new device, client or worker proves who it is and gets a credential.
- **Exposure beyond loopback (bind, TLS, tunnels)**: how a server or gateway is made reachable from other machines, and what protects it when it is.
- **Reconnect, resume and takeover after link loss**: what happens to a running session when the link between control and execution side drops, or one side restarts.

**2.4 Distributed execution**

- **SSH remote execution**: using SSH to put work on another machine, either the whole agent (helper installed on the host) or only the tools.
- **Pluggable execution backend (tools run elsewhere than the loop)**: an interface inside the agent through which every file and shell operation goes, so the same loop can execute locally, in a container, over SSH or in a cloud sandbox.
- **Self-hosted workers for a hosted control plane**: the vendor (or a self-hosted gateway) keeps the control plane and UI; a customer-run worker process executes sessions on customer machines.
- **Work claim, lease and heartbeat**: how queued sessions get assigned to workers, and how the control side notices a dead worker.
- **Execution-side enrolment and per-dispatch credentials**: the long-lived credential a worker enrols with, and any shorter credential minted per session or dispatch so a leaked token is worth little.
- **Execution-side authority over policy**: whether the machine that runs tools has a local policy that the control side cannot widen.
- **Central credential broker or inference proxy**: one host holds provider credentials so execution machines or less-trusted clients can call models without holding long-lived keys.
- **Multi-node fleet under one control side**: one control side driving several execution machines, with per-machine selection.
- **Execution-side health, drain and metrics**: endpoints and flags an operator uses to know a worker is alive, take it out of rotation, and retire it cleanly.
- **Containerised deployment of the execution side**: shipping the worker or server as a container image or Kubernetes recipe.

**2.5 Cloud agents and their environments**

- **Vendor-hosted cloud agents**: agent sessions that run on vendor infrastructure in an isolated VM or container with a clone of the repo, and keep running when the user's machine is off.
- **Cloud environment definition and setup caching**: a saved description of the machine a cloud agent starts in (image, repos, setup script, env vars, network) and a cache of the prepared filesystem.
- **Cloud isolation and network egress**: the isolation unit for a cloud session and how its outbound network is limited.
- **Secrets in cloud environments**: how API keys and tokens reach a cloud run without leaking into the transcript, the repo or the model.
- **Credential injection outside the sandbox (egress proxy)**: the agent's processes never hold the real secret; a proxy on the boundary adds it to requests for allowed hosts.
- **Local and cloud hand-off**: moving a running conversation, with its working state, between the user's machine and a remote or cloud session.
- **Cloud agent control API and follow-ups**: starting, listing, steering and following up on remote runs from a CLI or REST API rather than the UI.
- **Idle pause and wake on demand**: releasing an idle remote sandbox or worker while keeping the session resumable, and bringing it back when a message arrives.
- **Warm pools and pre-warming**: keeping started-but-unassigned workers, processes or checkouts ready so a new session starts fast, without putting one user's data in the shared image.

### Configuration, licence, other (areas 20, 22, 23)

Evidence tables, one row per harness with its own source: [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md)


**Area 20: Configuration and customisation**

- **Config file formats and locations**: where a harness keeps its settings, in which format, and what else sits next to it (credentials, MCP, keybindings, prompts).
- **Precedence between user, project, managed and command-line settings**: which layer wins when the same key is set in several places.
- **Managed (admin) policy layer and how it is delivered**: a configuration source an administrator controls that users and repos cannot loosen, and how it reaches the machine.
- **Repo-supplied config must not widen policy (workspace trust for config)**: rules that stop a cloned repository from changing credentials, providers, hooks or permissions through its own config files.
- **Profiles, presets and recipes**: named bundles of settings that can be switched or shared.
- **Environment variables**: process-level settings that override or supply what files hold.
- **Themes**: user-changeable colour schemes.
- **Key bindings**: user-remappable keyboard shortcuts.
- **Output styles, personas and system-prompt overrides**: persistent changes to how the agent speaks or behaves, separate from per-project instructions.
- **Status lines and footers**: a configurable line of live session data in the UI.
- **Hot reload and invalid-file handling**: what happens when a config file changes at runtime or is malformed.
- **Programmatic config editing**: changing settings without hand-editing the file.
- **Reading other harnesses' configuration and instruction files**: loading `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, skills directories or MCP configs written for another tool, or importing their settings.
- **Diagnosis and recovery commands**: commands that check, repair or isolate a broken setup.
- **Feature flags and experimental toggles**: switches that gate unfinished behaviour, with maturity labels.

**Area 22: Licence, pricing and distribution**

- **Licence, open/closed split, install, pricing and governance (one table)**: the legal and commercial shape of each harness. Governance and ownership moves use the reviewed versions (Cursor acquisition confirmed; goose governance move corrected; Pi move confirmed; OpenCode V2/V1 confusion corrected; Warp licence split confirmed; Warp Factories relabelled Early Access).
- **Third-party use of vendor subscriptions and credentials**: terms that decide whether another harness may use a vendor's login, quota or gateway.
- **Self-update and release channels**: how the binary updates itself and whether stable and canary tracks exist.

**Area 23: Anything else or unusual**

- **Adjacent products around the harness**: separate products a vendor ships beside the harness, relevant mostly as scope warnings.
- **Artifacts and deliverables as a reviewed object**: agent output treated as a first-class thing to review, comment on and approve rather than a transcript.
- **Deep links**: a URL scheme that opens the app with a working directory or a prompt.
- **Agent-addressed docs and consent rules**: documentation or API behaviour written for an AI agent that reads the project, including limits on what the agent may do on the user's behalf.
- **Vendor-side feedback and one-off oddities**: small features that do not fit elsewhere.

## 5. Feature availability matrix

One matrix per group of areas, copied unchanged from the synthesis files. Rows are features; columns are the seventeen harnesses and ours.

Cell values: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned or a vendor claim only; `-` nothing documented (not the same as absent); `?` unverified. Each synthesis file repeats its own legend above its matrix, and a few add a value of their own.

The `Ours` column describes branch `main`. Section 8 says what the open pull request and the roster branch change.

### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

Legend: `Y` documented and released; `B` beta, experimental, preview or partial (every DeepSeek cell is `B` because the whole product is a developer preview); `P` planned or vendor-claim only; `-` nothing documented in the survey; `?` unverified or implied but unconfirmed. Cells come from the feature rows above, so a `-` means the survey found no page, not that the capability is absent. Area codes: A1 loop, A2 tools, A3 editing, A4 shell. Notes: Claude Desktop's Code tab shares Claude Code's engine, so its `-` cells mostly mean "nothing documented separately". OpenCodex is a gateway; its `B` cells are Remote Workspace, and its `Y` cells are the bounded web-search loop, the stall deadline and the synthetic search tool. Cursor's step-limit cell is `-` because its docs state there is no limit. Harnesses whose edit mechanism is undocumented (Pi, Antigravity, Cline `editor`, OpenClaw `edit`, goose, Cursor) are `-` in the edit-format rows.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A1 Loop structure, turns, stop reasons | Y | - | - | Y | Y | B | - | Y | - | - | Y | Y | B | Y | Y | Y | B | Y |
| A1 Loop placement relative to tools | B | B | Y | Y | Y | B | - | - | Y | ? | Y | Y | B | Y | Y | Y | Y | Y |
| A1 Step, turn and budget limits | Y | Y | Y | Y | - | - | - | - | - | - | - | Y | Y | Y | Y | - | Y | Y |
| A1 Stuck-loop and stalled-run detection | - | - | ? | Y | - | - | - | - | - | - | Y | Y | B | - | - | - | - | - |
| A1 Interrupting a running turn | Y | - | - | Y | - | B | Y | Y | - | - | B | Y | Y | Y | Y | - | B | B |
| A1 Steering and queueing while a turn runs | Y | - | Y | Y | - | B | Y | Y | Y | - | ? | Y | Y | Y | Y | Y | - | - |
| A1 Parallel tool calls | Y | - | - | Y | - | - | - | Y | - | - | B | - | - | Y | - | - | - | - |
| A1 Goal and objective-continuation loops | Y | - | B | Y | - | B | Y | - | B | Y | Y | B | - | Y | - | - | - | - |
| A1 Time-driven re-submission loops | - | - | - | Y | - | - | Y | - | - | - | - | - | - | Y | - | - | - | - |
| A1 Retry, backoff, stream-failure handling | Y | Y | - | Y | - | - | - | Y | - | - | - | Y | - | - | - | - | Y | - |
| A1 Multi-model turn strategies | - | - | - | - | - | - | Y | - | - | - | - | - | - | Y | - | - | - | Y |
| A1 Event log as source of truth, one writer | - | - | Y | Y | - | B | Y | Y | - | - | Y | Y | B | - | - | - | - | Y |
| A1 Injected context and message provenance | - | - | - | Y | - | B | - | - | - | - | - | - | - | - | - | - | - | B |
| A1 Evaluator outside the main loop | Y | - | - | - | - | - | - | - | - | - | B | - | - | - | - | - | - | - |
| A1 Tool-call execution pipeline | - | - | - | - | - | B | - | Y | - | - | Y | - | - | Y | - | - | - | Y |
| A1 Deliberate turn-ending semantics | - | - | Y | Y | - | B | - | Y | - | - | Y | Y | - | - | - | - | Y | - |
| A2 Built-in tool inventory published | B | - | Y | Y | - | B | Y | Y | Y | B | Y | Y | B | Y | Y | - | Y | Y |
| A2 Tool-set composition (defaults, presets, gating) | Y | - | Y | Y | - | B | Y | Y | - | - | Y | Y | Y | Y | - | - | Y | Y |
| A2 Deferred tool loading (tool search) | Y | - | - | Y | - | - | Y | Y | - | Y | - | Y | B | Y | - | - | - | - |
| A2 Model-written scripts that call tools | B | - | Y | - | - | B | B | B | - | - | - | - | B | Y | - | - | - | - |
| A2 Ask-the-user tool | B | - | Y | Y | - | B | Y | - | Y | - | - | Y | - | Y | Y | Y | Y | B |
| A2 Todo and task-list tool | Y | - | B | Y | - | B | Y | - | - | B | Y | - | B | Y | - | Y | - | - |
| A2 Web fetch and web search tools | Y | Y | Y | Y | - | B | Y | - | Y | - | - | Y | - | Y | Y | Y | Y | B |
| A2 Code-navigation tools (LSP, AST, outline) | - | - | - | Y | - | B | Y | - | - | - | - | - | Y | - | Y | - | - | Y |
| A2 Tool result size limits and spill | Y | - | Y | Y | - | B | - | Y | - | - | - | - | Y | Y | - | - | - | Y |
| A2 Internal URL schemes in file tools | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A2 Tool risk classification metadata | - | - | Y | Y | - | - | Y | - | - | - | Y | - | - | - | Y | - | - | Y |
| A3 Edit format: exact search/replace | - | - | Y | Y | - | B | - | - | - | - | Y | - | - | Y | Y | - | - | Y |
| A3 Edit format: patch (apply_patch) | Y | P | Y | - | - | - | - | - | - | - | ? | Y | - | - | - | - | Y | - |
| A3 Edit format: hash-anchored lines | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A3 Edit format: structural (AST) | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A3 Whole-file write and create | - | - | Y | Y | - | B | Y | Y | Y | - | Y | Y | - | Y | Y | - | - | Y |
| A3 Model-routed choice of edit tool | - | - | Y | B | - | - | - | - | - | - | - | - | - | - | - | - | Y | - |
| A3 Failed-edit handling and staleness guards | - | - | Y | Y | - | B | Y | B | - | - | B | - | - | B | - | - | - | B |
| A3 Encoding and line-ending safety | Y | - | - | Y | - | - | - | - | - | - | - | - | - | Y | - | - | - | - |
| A3 Post-edit diagnostics and formatters | - | - | Y | Y | - | - | Y | - | - | - | - | - | - | Y | - | B | - | - |
| A3 Diff review and in-app file view/edit | Y | - | - | Y | Y | - | - | - | Y | Y | Y | Y | - | - | Y | Y | - | B |
| A3 Write confinement and protected paths | Y | - | - | Y | - | - | - | - | Y | - | - | - | - | Y | - | - | - | Y |
| A3 Read-tool capabilities and limits | - | - | Y | Y | - | B | Y | - | - | - | - | - | - | Y | Y | - | - | Y |
| A3 File mentions and attachments | - | - | Y | - | Y | - | - | Y | - | Y | - | - | Y | Y | - | Y | Y | - |
| A3 Notebook support | - | - | - | Y | - | - | Y | - | - | B | - | - | - | - | - | - | Y | - |
| A3 Git conflict resolution via virtual paths | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A3 Debug mode with log instrumentation | - | - | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - |
| A3 Inline selection edits / edit prediction | - | - | - | - | - | - | - | - | - | - | - | - | - | - | Y | - | - | - |
| A3 Per-session scratch directory | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| A3 Office-document deliverables | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - |
| A4 Shell execution model | Y | - | Y | Y | - | B | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y |
| A4 Command timeouts and auto-backgrounding | - | - | Y | Y | - | B | - | - | - | - | - | - | - | - | - | - | - | Y |
| A4 Background processes | Y | - | Y | Y | - | B | Y | - | ? | Y | B | Y | - | Y | - | Y | B | - |
| A4 Agent-driven interactive terminal (PTY) | Y | - | - | - | - | B | Y | - | - | - | - | - | - | Y | Y | Y | - | - |
| A4 User-facing terminal pane in the app | Y | - | Y | - | Y | - | - | - | - | - | Y | Y | - | - | - | Y | - | - |
| A4 User shell escape (! commands) | Y | - | Y | Y | - | - | Y | Y | Y | - | - | - | - | Y | - | Y | - | - |
| A4 Shell output limits and spill | - | - | - | Y | - | B | - | Y | ? | - | - | - | - | Y | Y | - | - | Y |
| A4 Command-output filtering and minimisation | - | - | - | - | - | - | Y | - | - | - | - | B | - | - | - | - | - | Y |
| A4 Environment given to spawned commands | Y | - | - | B | - | B | - | Y | Y | - | - | - | Y | Y | - | - | - | Y |
| A4 Resource limits for commands | - | - | - | Y | - | - | - | - | - | - | - | - | - | Y | - | - | - | - |
| A4 Pluggable execution backends | - | B | - | - | - | B | - | Y | Y | - | Y | Y | - | Y | Y | - | - | B |
| A4 PowerShell and Windows shell support | - | - | - | B | - | B | Y | Y | - | - | - | - | Y | - | - | - | - | - |
| A4 Shell-rc and terminal-emulator integration | - | - | - | Y | - | - | - | Y | - | - | - | - | Y | - | - | Y | - | - |
| A4 Monitor: command output as events | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| A4 Hosting other CLI agents in a terminal | - | - | - | - | - | - | - | - | - | - | - | - | - | - | Y | Y | - | - |

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

Legend: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned or vendor-claim only (for Ours: exists only on an unmerged branch or in PLAN); `-` nothing documented; `?` unverified.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| F1 Approval modes | Y | - | Y | Y | Y | B | Y | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| F2 Per-tool/command rules | B | - | Y | Y | Y | - | Y | B | Y | Y | B | Y | Y | Y | Y | Y | Y | B |
| F3 Shell chain/construct parsing | B | - | Y | Y | - | - | Y | - | B | Y | - | Y | - | Y | Y | - | - | - |
| F4 Approval binding and scope | Y | - | Y | Y | - | B | - | - | - | Y | - | Y | Y | Y | Y | - | - | B |
| F5 Model-based approver | Y | - | - | Y | Y | B | - | - | Y | Y | Y | Y | B | Y | - | Y | Y | P |
| F6 Hardcoded floors | - | - | - | Y | - | - | Y | - | Y | - | - | - | - | Y | Y | Y | - | - |
| F7 Protected paths | Y | - | - | Y | - | - | - | - | Y | Y | - | - | - | - | B | - | - | B |
| F8 Workspace boundary (file tools) | Y | - | Y | Y | Y | - | - | - | Y | Y | - | Y | - | - | - | Y | Y | Y |
| F9 Admin/managed policy | Y | - | Y | Y | Y | - | - | - | Y | B | B | Y | Y | Y | - | Y | Y | Y |
| F10 Repo config cannot widen | Y | - | - | Y | - | B | - | Y | - | - | - | - | - | - | - | - | - | Y |
| F11 OS sandbox (shell) | Y | B | - | Y | Y | B | - | - | Y | Y | - | - | ? | - | B | - | ? | P |
| F12 Sandbox-unavailable behaviour | Y | - | - | Y | Y | B | - | - | Y | Y | - | - | - | - | B | - | - | P |
| F13 Sandbox network egress | B | - | - | Y | Y | - | - | - | Y | Y | - | - | - | Y | B | - | - | P |
| F14 Escape hatch / widening | Y | - | - | Y | - | B | - | - | - | Y | - | Y | - | - | B | - | - | - |
| F15 Container/VM isolation | Y | B | - | Y | Y | - | - | Y | Y | - | Y | Y | ? | Y | - | Y | - | - |
| F16 Sub-agent inheritance | Y | - | Y | Y | - | - | Y | - | - | Y | - | - | Y | - | - | - | Y | Y |
| F17 Unattended behaviour | Y | - | - | Y | - | B | Y | - | Y | - | - | Y | - | Y | - | - | Y | Y |
| F18 Remote approval delivery | Y | - | - | Y | Y | - | - | - | - | Y | - | Y | - | Y | - | - | Y | Y |
| F19 Hooks in permission path | Y | - | - | Y | - | - | - | Y | Y | Y | Y | - | - | - | - | - | Y | - |
| F20 Decisions as logged events | Y | - | - | Y | Y | B | - | - | - | - | - | - | ? | - | - | - | - | Y |
| F21 Env scrubbing for children | Y | - | - | Y | - | - | - | - | - | - | - | Y | - | Y | - | - | - | Y |
| F22 Credential placeholder/proxy | Y | Y | - | Y | Y | - | Y | Y | Y | - | - | B | - | Y | - | Y | - | - |
| F23 Secret redaction | Y | - | - | Y | - | - | B | - | - | - | Y | - | - | Y | - | Y | - | B |
| F24 Own credential storage | Y | - | - | Y | Y | B | Y | Y | - | Y | - | - | Y | Y | - | - | Y | Y |
| F25 Prompt-injection defences | Y | Y | - | Y | Y | B | - | - | - | - | Y | Y | ? | Y | Y | - | - | Y |
| F26 Inter-agent output hygiene | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | B |
| F27 SSRF guards | B | - | - | Y | - | B | - | - | - | - | - | - | - | Y | - | - | - | Y |
| F28 Project trust prompt | Y | - | - | Y | - | - | - | Y | Y | Y | - | - | - | - | Y | - | - | - |
| F29 Extension/plugin trust | Y | - | Y | Y | - | B | - | - | - | - | B | Y | Y | Y | - | Y | Y | - |
| F30 Own server/UI auth | B | Y | Y | Y | - | B | Y | - | - | - | Y | Y | ? | Y | - | - | B | Y |
| F31 Remote link auth | Y | B | - | Y | Y | Y | - | - | ? | Y | - | Y | B | B | P | Y | Y | Y |
| F32 Telemetry stated | Y | Y | - | Y | Y | B | Y | Y | - | Y | - | Y | Y | Y | Y | Y | Y | - |
| F33 Retention / data at rest stated | - | - | Y | Y | Y | - | Y | Y | Y | - | - | Y | - | - | Y | Y | - | Y |
| F34 Stated non-boundaries | Y | - | - | Y | - | B | Y | Y | Y | - | Y | Y | - | Y | Y | Y | Y | Y |

Notes on cells: Pi F1 `-` because it documents having no approval system. goose F11 `?` and F15 `?`: macOS sandbox seen only in search snippets; `--container` status unclear. goose F30 `?` because `goose serve` has no maturity label (row present, status unclear); I kept `?` to flag that rather than `Y`. Cline F11 `?`: `CLINE_SANDBOX` meaning unstated. Cursor F31 `?`: no release label for self-hosted workers. Zed/ACP F31 `P`: ACP remote transport is an RFD (Zed's own SSH remote development is not an agent-link auth mechanism here). Ours F11/F12/F13 `P`: Landlock code exists only on `integration/gate3`.

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Legend: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned or vendor-claim only; `-` nothing documented; `?` unverified. Where a harness is a gateway or protocol the cell reflects that role. Claude Desktop's Code tab runs the Claude Code binary; its column reflects only what its own survey documents.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Automatic compaction | Y | Y | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | B |
| Manual compaction | Y | B | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | - | Y | Y | Y | - |
| Staged, fallback, overflow recovery | - | - | B | B | - | B | Y | B | B | - | B | Y | B | Y | - | - | B | - |
| Idle or background compaction | - | - | B | Y | - | - | Y | - | - | - | - | - | - | B | - | - | - | - |
| Pruning or masking of old tool output | - | - | - | - | - | B | Y | B | - | - | - | Y | Y | Y | - | - | - | Y |
| Large-output handling | B | - | Y | Y | - | Y | Y | B | - | - | - | - | - | B | B | - | - | Y |
| Prompt caching design and controls | - | B | Y | Y | - | - | - | Y | - | - | - | Y | - | Y | - | B | B | Y |
| Context usage display | - | B | - | Y | Y | B | Y | - | - | Y | Y | - | Y | - | Y | - | Y | B |
| Repo map or code index | - | - | - | - | - | - | - | - | ? | ? | - | - | - | - | - | Y | - | Y |
| Lazy tool schemas, catalog budgets | Y | - | - | Y | - | - | - | B | - | Y | - | - | B | - | B | - | Y | - |
| Compaction hooks, plugins, templates | Y | - | Y | Y | - | - | - | Y | B | - | - | Y | Y | Y | B | - | - | - |
| Side questions, recaps, hand-offs | Y | - | Y | Y | - | - | Y | B | - | Y | - | - | - | - | B | Y | Y | - |
| Instruction files (names read) | Y | - | Y | Y | - | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Scopes, precedence, imports | Y | - | Y | Y | - | - | Y | Y | Y | Y | B | B | Y | Y | Y | Y | Y | B |
| Instruction size limits | Y | - | - | Y | - | - | - | - | B | Y | - | Y | B | Y | B | - | - | Y |
| `/init` and audit commands | Y | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | Y | B | - |
| Agent-written memory exists | Y | - | - | Y | Y | - | Y | - | B | - | Y | Y | ? | Y | - | B | B | Y |
| Human approval of memory writes | - | - | - | - | - | - | - | - | - | Y | - | - | - | Y | - | - | - | Y |
| Memory scopes beyond one store | B | - | - | Y | Y | - | Y | - | B | - | Y | B | - | B | - | Y | - | Y |
| Search/embedding recall, pluggable back ends | - | - | - | - | - | B | Y | - | B | - | - | Y | - | Y | - | B | - | Y |
| Background consolidation | Y | - | - | - | B | - | Y | - | - | - | - | Y | - | Y | - | B | - | - |
| Agent-written skills | - | - | - | - | - | - | B | - | - | Y | - | ? | - | Y | B | - | - | - |
| Memory content safety | Y | - | - | B | Y | - | - | - | B | - | Y | Y | - | Y | - | - | - | Y |
| Memory opt-outs | Y | - | Y | Y | Y | - | Y | Y | Y | - | Y | B | - | - | - | B | - | Y |
| Memory import/export | Y | - | - | B | ? | - | B | - | - | - | - | Y | - | Y | - | - | - | - |
| Search of past sessions | - | - | - | - | Y | - | - | - | - | - | - | - | - | Y | - | - | - | - |

Notes on cells: DeepSeek, Cursor memory rows use `B` for preview or unclear status; Hermes manual compaction is `-` because the entry read documents none; Claude Code memory content safety is `B` because only the outside-directory guard is documented; OpenCodex `B` on manual compaction is request forwarding for native passthrough only.

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

Legend: `Y` documented and released (GA); `B` beta, experimental, preview or partial; `P` planned or vendor claim only; `-` nothing documented (not "absent"); `?` unverified. "Ours" uses the inventory: `Y` works, `B` partial/stub, `-` none. Column abbreviations: CX Codex, OCX OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DS DeepSeek harness, OMP Oh My Pi, Pi, CU Cursor, AG Antigravity, OH OpenHands, OCL OpenClaw, GO goose, HE Hermes Agent, ZED Zed/ACP, WA Warp, CL Cline.

| Feature | CX | OCX | OC | CC | CD | DS | OMP | Pi | CU | AG | OH | OCL | GO | HE | ZED | WA | CL | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Plan mode | Y | - | Y | Y | Y | B | Y | - | Y | Y | - | - | - | - | - | Y | Y | - |
| Plan review / hand-off | - | - | - | Y | Y | B | Y | - | Y | Y | - | - | - | - | Y | Y | Y | - |
| Task / to-do tool | Y | - | B | Y | Y | B | Y | - | - | - | ? | ? | ? | Y | Y | Y | - | - |
| Goal / run-until-done | Y | - | ? | Y | - | B | Y | - | - | Y | Y | ? | ? | Y | - | - | - | - |
| Read-only / custom modes | Y | - | Y | Y | Y | B | Y | B | Y | Y | - | Y | Y | - | Y | Y | Y | Y |
| Interview-first planning | B | - | - | Y | - | B | Y | - | Y | Y | - | - | - | Y | - | - | Y | B |
| Side questions | Y | - | Y | Y | Y | - | Y | - | - | - | - | - | - | Y | - | - | - | - |
| Orchestration modes / keywords | - | - | - | Y | - | - | Y | - | - | Y | - | - | - | - | - | Y | B | Y |
| Sub-agent delegation tool | Y | B | Y | Y | Y | B | Y | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| Built-in named roster | Y | - | Y | Y | Y | - | Y | - | Y | Y | - | - | - | B | Y | B | - | Y |
| Custom agent files | Y | - | Y | Y | Y | B | Y | B | Y | Y | Y | Y | ? | Y | Y | B | - | Y |
| Forked sub-agent context | - | - | - | Y | - | - | - | - | - | - | - | Y | - | - | - | - | - | - |
| Child cannot widen permissions | Y | - | - | Y | - | ? | B | - | - | Y | ? | - | Y | Y | - | - | - | Y |
| Sub-agent model slot | Y | Y | Y | Y | - | B | Y | - | Y | Y | Y | Y | Y | Y | Y | Y | - | Y |
| Structured sub-agent output | - | - | - | Y | - | B | Y | - | - | - | - | - | - | Y | - | - | - | - |
| Sub-agent output scanning | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Background sub-agents | Y | - | Y | Y | - | B | Y | - | Y | Y | - | Y | - | - | - | Y | - | - |
| Resume a sub-agent | Y | - | Y | Y | - | B | Y | - | Y | Y | Y | - | Y | - | Y | Y | - | - |
| Depth limit documented | - | - | B | Y | - | B | Y | B | Y | Y | - | ? | Y | Y | - | Y | - | Y |
| Concurrency limit documented | Y | - | - | Y | - | B | Y | B | - | - | - | Y | Y | Y | - | - | - | Y |
| Spend / token cap on sub-agents | - | - | - | Y | - | B | Y | - | - | - | - | - | - | - | - | - | - | Y |
| Worktree / overlay isolation for children | - | - | - | Y | - | - | Y | - | Y | Y | Y | Y | - | Y | - | - | B | Y |
| Agent teams / swarms / boards | - | - | - | B | - | B | Y | - | - | B | - | ? | - | Y | - | B | B | - |
| Cross-session messaging / bus | - | - | - | Y | Y | B | Y | - | - | B | - | Y | - | Y | - | Y | B | - |
| Script-orchestrated workflows | - | - | - | Y | - | B | Y | - | - | - | ? | - | B | - | - | - | - | - |
| Other harnesses as sub-agents | - | Y | - | - | - | B | - | - | - | - | Y | Y | ? | - | Y | Y | B | B |
| Coordinator agents | - | - | - | B | B | - | Y | - | ? | Y | - | - | - | - | - | B | - | B |
| Parallel fan-out commands | - | - | - | Y | - | - | Y | - | Y | Y | - | - | - | - | - | - | - | B |
| Advisor / watchdog model | - | - | - | B | - | - | Y | - | - | - | B | B | - | Y | - | - | - | - |
| Agent-to-agent peering | - | - | - | - | - | - | - | - | ? | - | - | Y | - | Y | P | - | - | - |
| Title slot | - | - | Y | Y | Y | Y | - | - | - | - | - | - | - | Y | Y | - | - | - |
| Summary / compaction slot | - | - | Y | - | - | - | Y | - | - | - | Y | - | - | Y | Y | - | - | ? |
| Vision bridge slot | - | Y | - | - | - | - | Y | - | - | - | - | - | - | Y | - | - | - | - |
| Judge slot | - | - | ? | Y | - | - | - | - | - | - | Y | - | - | Y | - | - | - | - |
| Commit-message slot | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | Y | - | - | - |
| Per-slot fallback chains | - | Y | - | - | - | - | Y | - | - | - | - | - | - | Y | - | - | - | - |
| Main-model fallback chain | - | Y | - | Y | Y | - | Y | - | - | - | ? | Y | - | Y | - | Y | - | Y |
| Phase-based model switch | B | - | - | Y | - | - | Y | - | - | - | - | - | - | - | - | Y | Y | - |
| Background shell jobs | Y | - | Y | Y | Y | B | Y | - | - | Y | - | - | - | Y | - | - | - | - |
| Background sessions / daemon | Y | Y | Y | B | B | - | Y | - | - | - | - | Y | - | Y | Y | - | Y | B |
| Agent dashboard | Y | - | - | B | Y | - | Y | - | - | Y | - | Y | - | - | Y | Y | - | B |
| Auto-backgrounding long calls | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| In-session schedules / loops | Y | - | B | Y | - | B | Y | - | - | Y | - | Y | - | Y | - | - | - | - |
| Standalone scheduled runs | Y | - | B | B | Y | - | - | B | Y | Y | Y | Y | ? | Y | - | Y | Y | Y |
| Event triggers / webhooks | Y | - | - | B | - | B | Y | - | Y | Y | Y | Y | - | Y | - | Y | Y | Y |
| Unattended-run policy documented | Y | - | - | B | Y | B | - | - | Y | - | - | Y | - | Y | - | - | Y | Y |
| Cloud background agents | Y | - | - | Y | Y | - | - | - | Y | Y | Y | B | - | - | - | Y | B | - |
| Durable / crash-surviving runs | - | - | Y | B | Y | B | Y | B | - | - | - | Y | - | Y | - | - | - | Y |
| Auto-continue at usage limit | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Keep machine awake | B | - | - | - | Y | - | - | - | - | - | - | - | - | - | Y | - | - | - |
| Completion notifications | - | - | - | Y | B | - | - | - | - | - | - | Y | - | Y | Y | Y | - | ? |

Notes on cells:
- **Pi** `B` cells are example extensions or experimental pi-durable, not core.
- **Claude Desktop** inherits most Claude Code features through the Code tab. Only features its own survey documents are marked.
- **Ours "Interview-first planning" `B`** means the `ui:choices`/`ui:form` blocks.
- **Ours "Parallel fan-out" `B`** means chat-mode parallel answers.

---

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

Legend: `Y` documented and released; `B` beta/experimental/preview/partial (including documented only for one surface or one narrow case); `P` planned or vendor-claim only; `-` nothing documented; `?` unverified. "Removed" features are `-`. OpenCodex is a gateway, so most cells are `-`. For Ours, `Y`/`B` follow the inventory status (works/partial).

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek harness | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes Agent | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Session storage documented (format/location) | Y | - | Y | Y | - | B | Y | Y | B | B | Y | Y | Y | Y | B | B | Y | Y |
| Resume / continue | Y | - | - | Y | B | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| Fork to a new session | Y | - | Y | Y | - | B | Y | Y | Y | Y | Y | - | Y | Y | - | B | - | - |
| In-file session tree | - | - | - | - | - | - | Y | Y | - | - | - | - | - | - | - | - | - | - |
| Conversation rewind / edit-and-resubmit | Y | - | Y | Y | - | - | B | Y | Y | ? | Y | B | Y | Y | Y | - | Y | - |
| Export a session | Y | - | Y | Y | - | - | Y | Y | ? | - | Y | - | Y | Y | - | Y | Y | - |
| Share by link | - | - | - | Y | - | - | Y | Y | ? | - | Y | - | - | - | - | Y | - | - |
| Search across sessions / picker | - | - | - | Y | - | B | Y | - | - | Y | - | Y | B | B | Y | - | - | Y |
| Move session between machines/surfaces | Y | - | B | Y | Y | - | B | - | Y | - | - | - | - | B | - | Y | B | - |
| Import sessions from other harnesses | - | - | B | - | - | - | Y | - | - | - | - | - | - | Y | Y | - | Y | - |
| Archive / delete / retention / ephemeral | Y | - | - | Y | Y | B | B | B | Y | B | Y | Y | Y | Y | Y | Y | Y | Y |
| Single-writer / crash recovery | B | - | - | B | - | B | - | - | - | - | - | Y | - | Y | - | - | - | Y |
| Pluggable external session store | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| File checkpoints and undo | - | - | Y | Y | - | - | - | - | Y | ? | B | - | - | Y | - | - | Y | Y |
| Checkpoint limits / retention / removal documented | - | - | Y | Y | - | - | - | - | B | - | - | - | - | Y | Y | - | B | B |
| SDK file checkpointing | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Worktrees for parallel agents | Y | - | Y | Y | Y | B | Y | - | Y | Y | Y | Y | - | Y | Y | Y | Y | B |
| Worktree setup / ignored-file copy | Y | - | - | Y | Y | - | - | - | Y | - | - | Y | - | - | Y | - | B | B |
| Worktree lifecycle / cleanup | Y | - | - | Y | Y | - | B | - | Y | - | - | Y | - | Y | Y | - | - | B |
| Commit / commit-message generation | - | - | - | Y | - | - | Y | - | - | Y | B | - | - | - | Y | B | Y | - |
| PR creation / checkout | B | - | - | Y | - | - | Y | - | - | - | Y | ? | - | - | - | Y | B | - |
| PR status, auto-fix, auto-merge | - | - | - | Y | Y | - | - | - | B | - | - | - | - | B | - | - | - | - |
| Local code review command | Y | - | - | Y | Y | - | Y | - | - | - | - | - | ? | - | - | - | - | B |
| Hosted PR review bot / CI action | Y | - | - | Y | - | B | - | - | Y | - | Y | - | - | B | - | Y | B | - |
| Cloud multi-agent deep review | - | - | - | B | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Diff views | Y | - | B | B | Y | B | B | - | - | Y | B | B | - | - | B | Y | Y | B |
| Attribution / signing / provenance | - | - | - | Y | - | - | - | - | Y | - | - | - | - | - | - | - | - | B |
| Git host integrations / credential handling | Y | - | B | Y | - | B | Y | - | B | Y | Y | ? | B | - | - | Y | - | - |

Notes on cells: the Cursor `?` on export and sharing is the unopened "Shared Transcripts" page; the Antigravity `?` cells are the unverified `/rewind` behaviour; OpenClaw `?` for PR creation is the survey's "nothing found on pages read, CHANGELOG not opened"; goose `?` for review is the unexplained `goose review` flags. Cursor diff view is `-` only because the Agents Window and IDE diff UI were outside the survey's area 11-12 text.

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

Legend: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned, vendor claim only, or (for Ours) described in PLAN but not built; `-` nothing documented (or, for Codex's MCP server, removed); `?` unverified.

Columns: Cdx Codex, OCx OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DSH DeepSeek harness, OMP Oh My Pi, Pi, Cur Cursor, AG Antigravity, OH OpenHands, OCl OpenClaw, gse goose, Her Hermes Agent, Zed Zed/ACP, Wrp Warp, Cln Cline.

| Feature | Cdx | OCx | OC | CC | CD | DSH | OMP | Pi | Cur | AG | OH | OCl | gse | Her | Zed | Wrp | Cln | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MCP client | Y | - | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| MCP remote auth (OAuth) | Y | - | Y | Y | - | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Tool deferral / tool search | Y | - | Y | Y | - | - | Y | Y | - | Y | - | Y | B | Y | - | - | - | P |
| MCP beyond tools (prompts, resources, elicitation, Apps, tasks) | Y | - | Y | Y | Y | B | Y | Y | Y | - | - | Y | Y | Y | Y | - | - | - |
| MCP limits and lifecycle documented | Y | - | Y | Y | - | B | - | - | - | Y | - | - | Y | Y | - | Y | - | - |
| MCP approval, trust, allow/deny | Y | - | Y | Y | Y | - | Y | Y | Y | Y | Y | - | Y | Y | Y | Y | Y | - |
| Connector directory / MCP registry | Y | - | - | Y | Y | - | - | - | - | - | Y | - | Y | - | Y | - | Y | - |
| MCP push channels | - | - | - | B | - | - | - | - | - | - | - | - | - | B | - | - | - | - |
| Harness as MCP server | - | - | - | Y | - | - | - | - | - | - | - | Y | Y | Y | - | B | - | - |
| Skills (SKILL.md format and discovery) | Y | - | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | P |
| Skill loading budget and invocation control | Y | - | Y | Y | - | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | - | Y | - |
| Shell injection in skills or commands | - | - | Y | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Agent-written skills or extensions | - | - | - | Y | - | B | Y | - | - | Y | - | ? | - | Y | - | - | Y | - |
| Skill registries and account sync | Y | - | Y | Y | Y | - | - | Y | - | - | Y | Y | - | Y | Y | - | Y | - |
| Plugin bundles | Y | Y | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | - | Y | - |
| Plugin distribution / marketplaces | Y | - | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | - | Y | - |
| Plugin trust and supply-chain controls | Y | - | B | Y | Y | B | B | B | Y | B | B | Y | Y | Y | - | - | Y | - |
| Managed admin controls over extensions | Y | - | Y | Y | Y | - | - | - | Y | - | Y | Y | Y | - | - | - | Y | - |
| In-process extension API (middleware) | - | - | Y | Y | ? | B | Y | Y | - | - | B | Y | - | Y | - | - | Y | B |
| Hooks: events and decisions | Y | - | - | Y | ? | ? | Y | - | Y | Y | Y | Y | Y | Y | - | - | Y | - |
| Hook handler types beyond command | Y | - | - | Y | - | - | Y | - | Y | ? | Y | Y | - | Y | - | - | Y | - |
| Hook trust, precedence, failure documented | Y | - | Y | Y | - | - | Y | Y | Y | B | Y | Y | Y | Y | - | - | Y | - |
| Custom slash commands / templates | Y | - | Y | Y | - | - | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Custom tools in code | - | - | Y | Y | - | B | Y | Y | Y | Y | Y | Y | - | Y | - | - | Y | - |
| Code mode | B | - | Y | - | - | - | P | Y | - | - | - | - | Y | Y | - | - | - | - |
| Embeddable SDK | Y | - | Y | Y | - | B | Y | Y | Y | Y | Y | ? | Y | B | - | Y | Y | - |
| Programmatic control protocol | B | - | B | Y | - | B | Y | Y | Y | ? | Y | Y | Y | Y | - | Y | B | B |
| OpenAI-compatible agent endpoint | - | - | - | - | - | - | - | - | - | - | Y | Y | - | Y | - | - | - | - |
| Structured output for callers | - | - | - | Y | - | - | - | - | - | Y | - | - | Y | - | - | - | - | - |
| ACP agent (server) | - | - | Y | - | - | B | Y | - | Y | Y | - | Y | Y | Y | - | ? | Y | P |
| ACP client (hosting other agents) | - | - | - | - | - | B | - | - | - | - | Y | Y | Y | - | Y | - | - | B |
| Hosting other harnesses, non-ACP | - | Y | - | - | - | B | - | - | - | - | - | Y | Y | ? | Y | Y | B | B |
| A2A | - | - | - | - | - | - | - | - | - | - | - | - | - | Y | - | - | - | - |
| Reads other harnesses' files in place | B | - | Y | Y | - | B | Y | Y | Y | - | Y | Y | Y | - | Y | Y | Y | - |
| One-shot import from another harness | Y | - | - | Y | - | - | Y | - | - | Y | - | - | - | - | - | - | - | - |

Notes on cells: Codex "Harness as MCP server" is `-` because it was removed. Zed/ACP "ACP agent" is `-` because Zed is the client side; the protocol itself is described in that entry. Antigravity "Programmatic control protocol" is `?` because its headless NDJSON stream is mentioned only in the survey's section 5 (area 17), outside the lines read. OMP code mode is `P` because the tool bridge is a README claim. Ours `B` for middleware, ACP client and non-ACP hosting means only the `AgentBackend` trait exists; `B` for the control protocol means a REST/WS API with an admin-equivalent token and no spec.

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Legend: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned or vendor-claim only; `-` nothing documented; `?` unverified. Columns: Cdx = Codex, OCx = OpenCodex, OCd = OpenCode, CC = Claude Code, CD = Claude Desktop, DS = DeepSeek harness, OMP = Oh My Pi, Pi = Pi, Cur = Cursor, Ag = Antigravity, OH = OpenHands, OCl = OpenClaw, gs = goose, Hm = Hermes Agent, Zed = Zed/ACP, Wp = Warp, Cl = Cline.

Row notes: "Subscription" counts third-party subscription logins or a wrapped harness, not the vendor's own plan (Codex, Claude Code and Cursor use their own plans by design). "Hosted offering" is a commercial service. Commercial or Enterprise-only features are marked `Y` with the tier named in the feature entry.

| Feature | Cdx | OCx | OCd | CC | CD | DS | OMP | Pi | Cur | Ag | OH | OCl | gs | Hm | Zed | Wp | Cl | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Provider support and BYOK | Y | Y | Y | Y | Y | B | Y | Y | B | B | Y | Y | Y | Y | Y | B | Y | Y |
| Cloud-platform providers | Y | - | Y | Y | Y | - | Y | Y | B | B | Y | Y | Y | Y | Y | Y | Y | - |
| Custom endpoints and wire formats | Y | Y | Y | Y | Y | B | Y | Y | - | B | Y | Y | Y | Y | Y | Y | Y | Y |
| Local models | Y | Y | Y | - | - | - | Y | Y | - | B | Y | B | B | Y | Y | - | Y | Y |
| Subscription login or harness as provider | - | Y | Y | - | - | - | Y | Y | - | - | B | Y | Y | Y | Y | Y | Y | - |
| Model selection per session, agent, task | B | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| Reasoning effort controls | Y | Y | - | Y | Y | Y | Y | Y | - | Y | - | - | - | - | B | Y | Y | B |
| Speed tiers (fast mode) | Y | Y | - | Y | Y | - | - | - | - | B | - | - | - | - | - | - | - | - |
| Model fallback chains | - | Y | - | Y | B | - | Y | - | - | - | ? | Y | - | Y | - | Y | - | Y |
| Provider retry, backoff, timeouts | Y | Y | - | Y | - | - | B | Y | - | - | - | Y | B | B | - | - | - | - |
| Routing across providers, credential pools | - | Y | B | Y | - | - | Y | - | Y | - | B | Y | - | Y | - | Y | - | B |
| Model roles for auxiliary work | Y | Y | B | Y | - | - | Y | B | - | - | - | B | - | Y | Y | B | - | - |
| Vision and other modalities | B | Y | B | - | - | B | Y | Y | Y | - | ? | - | - | Y | Y | - | - | Y |
| Voice | B | B | ? | Y | Y | B | Y | - | Y | B | Y | Y | Y | Y | - | Y | Y | B |
| Hosted model offerings (commercial) | Y | - | Y | Y | Y | - | - | - | Y | Y | Y | - | - | Y | Y | Y | Y | - |
| Model catalogue and discovery | Y | Y | Y | Y | - | Y | Y | Y | - | - | - | - | B | - | - | - | B | Y |
| Model and provider governance | - | Y | B | Y | - | - | - | - | Y | B | - | Y | - | - | - | - | - | B |
| Prompt-cache controls | - | B | Y | Y | - | - | - | - | - | - | - | B | - | B | - | - | - | Y |
| Token and cost display | B | Y | Y | Y | B | ? | Y | Y | B | B | Y | Y | B | Y | Y | Y | Y | B |
| Budgets and limits | Y | B | Y | Y | - | - | B | - | B | B | Y | - | - | B | Y | B | B | B |
| Usage dashboards and quota views | B | Y | Y | Y | B | - | ? | - | Y | B | Y | Y | - | Y | Y | Y | - | - |
| OpenTelemetry and other export | Y | - | - | Y | Y | B | Y | B | B | - | Y | Y | Y | - | - | B | Y | - |
| Logs, transcripts, raw request logs | Y | Y | Y | Y | Y | B | Y | Y | Y | Y | - | Y | Y | Y | Y | Y | Y | Y |
| Debugging and diagnostics tools | Y | Y | Y | Y | - | B | Y | Y | - | B | - | Y | Y | Y | Y | Y | Y | Y |
| Vendor telemetry and upload defaults | Y | Y | ? | Y | Y | Y | B | B | B | Y | B | - | - | Y | Y | B | Y | - |

Cells worth a note: OpenCodex "Local models" and "Provider support" describe the gateway's upstream providers; "Model selection" is `-` because the gateway does not select per session (it routes; see routing). OpenHands "Fallback" is `?` because the SDK routing/fallback guide is only an index title. Warp "Local models" is `-` because the endpoint must be publicly reachable. Ours "Reasoning effort" is `B` because Anthropic models are forced to `effort=false`; "Voice" is `B` because input only.

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

Legend: `Y` documented and released; `B` beta, experimental, preview or partial (including "exists but the specific review/approval flow is not documented"); `P` planned or vendor-claim only; `-` nothing documented; `?` unverified (page not opened or skimmed only). Row 4 counts a hosted web UI as `Y` even when it is closed and paid; use the tier summary to see which are free. Rows 14 and 15 mean "documented browser statements"; a `-` means the survey found no page.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek harness | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes Agent | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 Terminal UI | Y | - | Y | Y | - | - | Y | Y | Y | Y | - | Y | Y | Y | - | Y | Y | - |
| 2 IDE integrations | Y | - | ? | Y | - | - | Y | - | Y | Y | - | - | Y | - | Y | - | Y | - |
| 3 Desktop apps | Y | B | B | - | Y | B | - | - | Y | Y | B | Y | Y | Y | Y | Y | Y | - |
| 4 Web UIs | - | B | Y | Y | B | B | B | - | Y | B | Y | Y | - | Y | - | Y | B | Y |
| 5 Mobile | Y | - | B | Y | Y | - | ? | - | Y | B | B | Y | - | - | - | B | - | B |
| 6 Voice | B | - | - | Y | Y | B | - | - | Y | Y | Y | Y | Y | Y | - | Y | Y | Y |
| 7 Messaging-app channels | Y | - | - | Y | B | - | - | - | Y | - | Y | Y | - | Y | - | Y | - | - |
| 8 Approval presentation | Y | - | Y | Y | Y | Y | Y | - | Y | Y | B | Y | Y | Y | Y | Y | Y | Y |
| 9 Diff review | Y | - | B | Y | Y | B | B | - | Y | Y | Y | Y | - | Y | Y | Y | Y | B |
| 10 Plan review | B | - | B | Y | Y | B | Y | - | Y | Y | - | - | - | - | B | Y | B | - |
| 11 Notifications / waiting signals | ? | - | B | Y | Y | - | - | - | Y | - | - | B | - | - | - | Y | - | Y |
| 12 Several agents at once | Y | - | B | B | Y | B | B | - | Y | Y | Y | Y | - | Y | Y | Y | B | B |
| 13 Agent's own browser | Y | - | Y | - | Y | B | Y | - | Y | Y | Y | Y | B | Y | - | Y | ? | - |
| 14 User's real browser | Y | - | P | Y | Y | - | Y | - | - | Y | - | Y | - | Y | - | - | - | - |
| 15 Browser isolation and site policy | Y | - | B | Y | Y | - | B | - | Y | Y | B | Y | - | Y | - | B | - | - |
| 16 Computer use | Y | - | - | B | B | B | Y | - | Y | - | - | Y | B | Y | - | Y | - | - |
| 17 Accounts, roles, orgs, workspaces | B | B | Y | Y | Y | - | - | - | Y | Y | Y | Y | - | Y | Y | Y | Y | Y |
| 18 SSO and provisioning | B | - | Y | Y | Y | - | - | - | Y | Y | Y | B | - | B | - | Y | Y | - |
| 19 Managed config / policy distribution | Y | - | Y | Y | Y | - | - | - | Y | B | B | B | B | B | - | Y | Y | B |
| 20 Shared team resources | B | - | B | Y | Y | - | - | - | B | B | B | B | B | Y | - | Y | Y | B |
| 21 Budgets and usage controls | - | - | Y | Y | B | - | - | - | Y | - | Y | - | - | - | Y | Y | B | - |
| 22 Audit logs | B | - | - | Y | B | B | - | - | Y | B | B | - | - | B | - | - | Y | B |
| 23 Session sharing / live collaboration | - | - | - | Y | Y | - | Y | B | Y | - | Y | Y | - | - | - | Y | - | B |

Cell notes: Claude Code row 12 is `B` because agent view is a research preview. Claude Code row 13 is `-` because Claude Code itself has no built-in browser (the pane belongs to Claude Desktop). Row 18 Zed/ACP is `-` (Zed Business has roles but SSO is not documented in the lines read). Row 1 goose is `Y` on the strength of a named CLI; the TUI itself is not described. OpenCode row 2 is `?` because its IDE pages were not opened.

---

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

Legend: `Y` documented and released; `B` beta, experimental, preview, partial, or released but deprecated; `P` planned or vendor-claim only; `-` nothing documented in the survey (weak evidence); `?` unverified. Column `Ours` uses the own inventory (`Y` works, `B` partial, `-` none). Short column names: CX Codex, OCX OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DS DeepSeek harness, OMP Oh My Pi, Pi Pi, Cur Cursor, AG Antigravity, OH OpenHands, OCl OpenClaw, gs goose, HA Hermes Agent, Zed Zed/ACP, Wp Warp, Cl Cline.

| Feature | CX | OCX | OC | CC | CD | DS | OMP | Pi | Cur | AG | OH | OCl | gs | HA | Zed | Wp | Cl | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| One-shot headless run | Y | - | Y | Y | - | B | Y | Y | Y | Y | B | - | Y | Y | - | Y | Y | - |
| Structured event stream / schema output | Y | - | Y | Y | - | - | Y | Y | Y | Y | B | - | Y | - | - | Y | Y | - |
| Approval behaviour with no human present | Y | - | - | Y | - | B | Y | Y | Y | Y | - | - | - | Y | - | Y | Y | Y |
| Config isolation for CI runs | Y | - | - | Y | - | - | - | - | - | - | - | - | Y | - | - | - | Y | - |
| Official CI action / pipeline | Y | - | B | Y | - | - | - | - | ? | - | ? | - | - | - | - | Y | Y | - |
| Machine credentials for CI | Y | - | - | Y | - | - | - | - | Y | B | - | - | - | - | - | Y | - | Y |
| Event-triggered remote runs | Y | - | - | B | - | B | - | - | Y | - | Y | Y | - | Y | - | Y | Y | Y |
| Programmatic control protocol | B | - | Y | Y | - | B | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| ACP support | - | - | Y | - | - | B | Y | - | B | Y | Y | Y | Y | Y | Y | ? | Y | - |
| Remote attach of client to agent server | B | - | Y | - | - | B | - | - | - | - | Y | Y | B | Y | - | - | B | Y |
| Relay-based remote control (web/phone) | Y | Y | Y | Y | Y | - | Y | B | Y | Y | - | B | B | Y | - | - | B | Y |
| Pairing / auth of remote clients and hosts | Y | Y | Y | B | B | B | - | - | Y | - | Y | Y | B | Y | - | Y | B | Y |
| Exposure beyond loopback | B | Y | Y | - | - | B | Y | - | - | - | Y | Y | B | Y | - | - | B | Y |
| Reconnect / resume after link loss | - | B | Y | Y | Y | B | - | B | B | - | - | B | Y | Y | Y | - | Y | Y |
| SSH remote execution | Y | ? | Y | - | Y | B | Y | B | - | B | - | Y | - | Y | Y | - | Y | - |
| Pluggable execution backend | ? | - | - | Y | - | B | - | Y | - | - | Y | Y | - | Y | Y | - | - | - |
| Self-hosted workers for hosted control plane | - | - | - | B | B | - | - | - | B | - | - | B | - | - | - | Y | - | Y |
| Work claim, lease, heartbeat | - | - | - | B | - | - | - | - | B | - | - | B | - | - | - | B | - | - |
| Per-session / per-dispatch worker credentials | - | - | - | B | - | - | - | - | B | - | - | B | - | - | - | Y | - | - |
| Execution-side authority over policy | Y | B | - | B | - | - | - | - | B | - | - | B | - | - | - | - | - | Y |
| Central credential broker / inference proxy | - | Y | - | Y | - | - | Y | - | - | - | - | B | - | B | - | Y | Y | - |
| Multi-node fleet under one control side | - | - | - | B | - | - | - | - | B | - | Y | Y | - | Y | - | Y | - | Y |
| Execution-side health, drain, metrics | B | Y | - | B | - | - | - | - | B | - | Y | - | - | - | - | Y | - | B |
| Containerised execution side | - | - | Y | B | - | - | - | - | - | - | Y | Y | - | Y | - | Y | - | - |
| Vendor-hosted cloud agents | Y | - | - | Y | Y | - | - | - | Y | Y | Y | - | - | - | - | Y | B | - |
| Cloud environment definition and caching | Y | - | - | Y | - | - | - | - | Y | Y | - | - | - | - | - | Y | - | - |
| Cloud isolation and egress control | Y | - | - | Y | - | - | - | - | Y | - | - | - | - | - | - | Y | - | - |
| Secrets in cloud environments | Y | - | - | Y | - | - | - | - | Y | Y | Y | - | - | - | - | Y | - | - |
| Credential injection outside the sandbox | Y | - | - | Y | - | - | - | Y | - | - | - | B | - | Y | - | - | - | - |
| Local and cloud hand-off | Y | - | - | Y | Y | - | - | - | Y | - | - | - | - | - | - | Y | B | - |
| Cloud agent control API / follow-ups | Y | - | - | Y | - | - | - | - | B | - | Y | - | - | - | - | Y | - | Y |
| Idle pause and wake on demand | - | - | - | Y | - | - | - | - | Y | - | B | - | - | Y | - | - | B | - |
| Warm pools and pre-warming | - | - | - | B | - | - | - | - | B | - | Y | B | - | - | - | Y | - | - |

Notes on the matrix. "Execution-side authority over policy": `Y`/`B` means the execution side can restrict what the control side asks for; OpenHands, Warp and Cline have entries in 2.4 but are `-` because their documented model puts the decision on the control side (or lets the client widen limits). "Self-hosted workers": ours is `Y` in the sense that the node is a self-hosted worker, but the control side (master) is self-hosted too, unlike every other row.

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

Legend: `Y` documented and released; `B` beta, experimental, preview (all DeepSeek harness cells are preview) or partial; `P` planned or vendor-claim only; `-` nothing documented in the lines read; `?` unverified. Columns: Cx Codex, OCx OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DS DeepSeek harness, OMP Oh My Pi, Pi Pi, Cu Cursor, AG Antigravity, OH OpenHands, Cl OpenClaw, Go goose, He Hermes Agent, Z Zed/ACP, W Warp, Cn Cline, Ours.

| Feature | Cx | OCx | OC | CC | CD | DS | OMP | Pi | Cu | AG | OH | Cl | Go | He | Z | W | Cn | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Config file formats and locations | Y | Y | Y | Y | B | B | Y | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y |
| Precedence user/project/managed/CLI | Y | B | Y | Y | - | B | Y | Y | B | B | - | - | Y | Y | - | B | B | Y |
| Managed (admin) layer and delivery | Y | - | Y | Y | Y | - | - | - | Y | - | - | - | Y | Y | - | Y | Y | Y |
| Repo-supplied config trust gating | Y | - | - | Y | B | - | B | Y | Y | Y | - | - | - | B | - | - | - | - |
| Profiles, presets, recipes | Y | - | - | - | - | B | Y | - | - | - | Y | - | Y | Y | - | Y | - | B |
| Environment variables | Y | Y | Y | Y | - | B | Y | Y | - | - | Y | Y | Y | Y | - | Y | - | Y |
| Themes | Y | - | Y | Y | - | - | Y | Y | - | Y | - | - | - | Y | - | Y | - | Y |
| Key bindings | Y | - | Y | Y | - | - | ? | Y | - | Y | - | - | - | - | Y | B | - | - |
| Output styles, personas, prompt overrides | Y | - | - | Y | - | B | - | Y | - | B | Y | Y | - | Y | - | - | B | B |
| Status lines and footers | Y | - | - | Y | - | - | Y | B | - | Y | - | - | - | - | - | Y | - | - |
| Hot reload and invalid-file handling | - | Y | - | Y | - | - | Y | Y | - | Y | - | Y | B | - | - | Y | - | B |
| Programmatic config editing | Y | Y | - | Y | - | B | Y | - | - | - | - | Y | - | - | - | Y | Y | Y |
| Reading other harnesses' config | Y | Y | B | Y | - | - | Y | - | Y | B | - | - | Y | Y | Y | Y | Y | - |
| Diagnosis and recovery commands | - | - | - | Y | - | B | - | - | - | - | - | Y | - | - | - | - | - | Y |
| Feature flags, experimental toggles | Y | - | B | Y | - | - | - | - | - | - | - | Y | - | - | - | - | - | - |
| Licence, open/closed, install, pricing table | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Third-party use terms for credentials | - | B | B | Y | - | - | - | - | - | Y | - | - | - | - | - | - | - | - |
| Self-update and release channels | Y | - | - | Y | B | - | Y | Y | - | Y | - | Y | Y | B | - | B | - | - |
| Adjacent products | Y | - | Y | Y | Y | B | Y | - | Y | - | B | B | B | B | - | Y | Y | - |
| Artifacts and deliverables as reviewed object | - | - | B | B | - | B | - | - | - | Y | - | - | - | - | - | - | - | B |
| Deep links | - | - | - | Y | - | - | - | - | - | - | - | - | Y | - | - | - | - | - |
| Agent-addressed docs and consent rules | - | Y | - | - | - | - | - | - | - | - | - | - | - | ? | - | - | - | - |

Notes on cells: Antigravity "Y" for themes, key bindings and status lines is from one TUI feature line; DSH is `B` throughout because the whole project is a developer preview; Pi footer is `B` because configurability is not stated. Claude Desktop "Y" for the managed layer is the 3P admin console; Claude Code's themes are `Y` but the survey also says plugin themes live under `experimental.*` manifest keys (see section 7).

## 6. Interesting or uncommon capabilities

A curated selection of 34 capabilities that are rare in the field or implemented in an unusual way, chosen for what they mean to a self-hosted master/node harness. The full list of distinctive items from every survey (179) is appended elsewhere in this report. Each row gives the mechanism as the vendor documents it, the status label, the check value, and the evidence URL. "Documented" means documented: nothing here was run.

### Approval binding and policy

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| OpenClaw | Approvals bind to the exact argv and cwd, the env when present, and a pinned executable path (plus a content hash when the executable is writable). The Gateway resolves every command-segment executable before review and re-checks before launch; a changed resolution denies. | Closes the "approve `git status`, then PATH points at another `git`" hole that every prefix rule leaves open. Our node already resolves paths component by component, so the binding is a natural extension. | GA, reviewed | https://docs.openclaw.ai/tools/exec-approvals |
| Codex | Command rules carry inline `match` / `not_match` examples that are checked when the rule file loads; `bash -lc` scripts are split with tree-sitter only when provably linear, otherwise judged as one opaque command. | Rule lists rot. Tests in the rule file make a bad rule fail at startup, not in production. | GA, reviewed | https://learn.chatgpt.com/docs/agent-configuration/rules.md |
| Antigravity | The command-permission parser turns off prefix matching for constructs that hide execution, such as `$(...)`, `git -c` and `tar --to-command`. | A concrete requirement list for any allow-list we write. | GA, reviewed | https://antigravity.google/docs/permissions |
| DeepSeek harness | The approval service writes `approval/asked` and `approval/decided` events; anything other than allowed-once denies; `never` is enforced before any answerer runs; if the audit append fails, the request is rejected. | "No unlogged decision" is exactly our one-writer event log rule, and the append-failure rule costs almost nothing to copy. | preview, reviewed | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval |
| Claude Code | A PreToolUse hook can return `defer`: the headless turn ends with the tool call pending, and the host resumes it after a human decides, with no timeout. The SDK's `canUseTool` answers out of band and redelivers after reconnect. | This is our case: the approver is on another device and may answer tomorrow. Our routines block a turn for up to 24 hours instead. | GA, unreviewed | https://code.claude.com/docs/en/hooks.md, https://code.claude.com/docs/en/agent-sdk/hooks.md |
| Claude Code | Auto mode: a separate classifier reviews actions after rules resolve; tool results are stripped from its input; broad code-exec allow rules are dropped while it is active; it falls back to prompting after 3 consecutive or 20 total blocks. The docs say it "does not guarantee safety". | The most injection-aware approver design in the set, and its safeguards (stripped inputs, circuit breaker) are what any model approver of ours must copy. | GA, reviewed | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/auto-mode-config.md |
| Claude Code | Cross-session messages have a receiver-side trust model: peer messages cannot answer permission prompts or justify config changes; `crossSessionInbound` can accept, hold or refuse; loop protection caps queued and repeated messages. | The only documented answer to "what may a message from another agent authorise". Needed before any agent-to-agent channel between nodes. | GA, reviewed | https://code.claude.com/docs/en/cross-session-messaging.md |
| Codex | Hooks are trusted by hash: a changed hook is skipped until it is reviewed again; admins can force managed-only hooks. | The right shape for node-side hooks: a repo that edits a reviewed hook gets no silent execution. | no GA label on the page, corrected | https://learn.chatgpt.com/docs/hooks.md |

### Sandbox, secrets and credentials

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| DeepSeek harness | Each sandbox backend reports `full` or `partial` enforcement (for example on older Landlock ABIs), and an unavailable backend fails with `SANDBOX_UNAVAILABLE` rather than silently passing commands through. | Our gate3 Landlock sandbox has exactly such a gap (pathname Unix sockets below Linux 7.1). Reporting it per node makes the gap visible to the operator. | preview, reviewed | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox |
| Codex | Protected subpaths inside writable roots: `.git`, `.agents`, `.codex`, `.aws` and the resolved gitdir stay read-only at the OS layer. | Stops an agent rewriting its own config or git hooks from the shell. Our file tools refuse `.git`, but `bash` does not; the gate3 sandbox could add this. | GA, reviewed | https://learn.chatgpt.com/docs/agent-approvals-security.md |
| Hermes Agent | iron-proxy: a host-side TLS-intercepting proxy (pinned, hash-verified binary) gives Docker and remote sandboxes opaque tokens in the normal provider variables and swaps in real keys only for allowlisted upstreams. It binds the Docker bridge gateway, never `0.0.0.0`. Stated limits include a stolen CA key, raw sockets, and Bedrock or Vertex keeping real credentials inside. | The one documented design that keeps provider keys on the execution machine but out of the agent's reach. That fits "the node decides" better than moving keys to the master. | GA (optional), corrected | https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy |
| OpenClaw | Cloud workers: provider keys stay on the Gateway and inference is proxied by model reference; each dispatch gets a new worker credential, stored only as a hash; rotation plus owner-epoch fencing rejects a stale worker. | The opposite answer to the same question: credentials and authority both on the control side. Also a direct model for fencing a node that reconnects with stale state. | beta, corrected (status was GA; "outbound-only" is not stated) | https://docs.openclaw.ai/gateway/cloud-workers/security-model |
| Claude Code | `CLAUDE_CODE_TOOL_MEMORY_LIMIT` puts every Bash, PowerShell and Monitor command of a session in one memory cgroup on Linux; if the cgroup cannot be created, commands run uncapped. | A node that runs model-written commands needs a resource cap as well as a filesystem cap. The silent fallback is the part not to copy. | GA, reviewed | https://code.claude.com/docs/en/tools-reference.md |
| Claude Code | Worktree hardening: neutralises repo-local filter drivers, `includeIf` and LFS custom transfers, and refuses commands where git cannot be shown to stay inside the worktree. | Treats `.git/config` as attacker-writable, as we do; our sub-agent worktrees still let git-lfs filters run (PLAN T19). | GA, unreviewed | https://code.claude.com/docs/en/worktrees.md |

### The control and execution link

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| Codex | The App Server's bounded queues answer `-32001` "Server overloaded" instead of buffering. | The only documented overload behaviour on a control protocol. Gate3 bounds our link channels; an explicit overload reply is the missing half. | experimental, reviewed | https://learn.chatgpt.com/docs/app-server.md |
| OpenCodex | Remote Workspace: when the executor is disconnected, file tools fail; they never fall back to the hub's filesystem. Read-only by default; write needs a sandbox probe. | A rule worth writing down for us: a session bound to an offline node must not run anywhere else. | experimental, corrected | https://opencodex.me/guides/remote-workspace/ |
| OpenClaw | Two-layer node pairing: the device identity is approved first, then the set of commands the node declares; a node command runs only if declared, approved, and in the Gateway's allowlist. | An admin UX we lack: the master shows what a node claims to offer before it is used. In OpenClaw the Gateway holds the final say; in ours the node would. | GA, corrected (pairing-store history dated) | https://docs.openclaw.ai/nodes/command-policy, https://docs.openclaw.ai/gateway/pairing |
| Claude Code | Self-hosted runners: the first session claimed locks a runner to that owner until it exits with a fresh disk; `--confine-repo-settings enforce` refuses sessions whose committed repo settings reach outside the workspace. | A one-owner lock bounds cross-user leakage on a shared node without per-session VMs; the repo-settings floor is our invariant applied to repo config. | beta, reviewed / corrected | https://code.claude.com/docs/en/self-hosted-environments.md, https://code.claude.com/docs/en/self-hosted-environments-deploy.md |
| goose | Roaming: peer-to-peer ACP over iroh (QUIC), no open ports, key cards accepted by both sides, revocation closes connections within seconds. The docs state that spawned OS processes survive revocation. | NAT traversal without a relay, and an honest note that applies to any revoke design we build. Not in released binaries. | experimental, reviewed | https://goose-docs.ai/docs/guides/roaming-agents.md |

### Session log, durability and restart

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| OpenClaw | Each admitted run records an active-writer claim before streaming; the commit transaction verifies the claim still matches, so a superseded run cannot commit stale data. | One writer per session enforced at the storage layer, not only by the actor. | GA, corrected (the claim is verified in the transaction; no state-dir lock on the cited page) | https://docs.openclaw.ai/concepts/agent-loop |
| DeepSeek harness | Per-session append-only log as checksummed concatenated zstd frames, atomic materialisation, per-batch fsync, torn-tail truncation on read. | The only design that addresses both log growth and crash-torn writes in an append-only log. Our log keeps every streaming delta forever (AUDIT D5). | preview, reviewed | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence |
| Hermes Agent | A crashed turn is marked `unknown` and never replayed; a delivery ledger stores replies before sending (3 attempts in 24 hours). | Matches our "restart closes the turn as interrupted", and adds the delivery side we will need if the master bridges chat platforms. | GA, corrected (documented on the messaging page) | https://hermes-agent.nousresearch.com/docs/user-guide/messaging |
| Pi | Session JSONL tree with `id` / `parentId`, in-file branching, and an append-only `context_edit` entry that changes future context without rewriting history. | Branching as a different leaf pointer over one append-only log: the cheapest fork design for our fold-over-log model. | GA, reviewed | https://pi.dev/docs/latest/session-format |
| OpenClaw | Managed worktrees snapshot to a git ref before removal and can be restored. | Prevents "deleted a dirty worktree"; cheap to copy for our sub-agent worktrees. | GA, reviewed | https://docs.openclaw.ai/concepts/managed-worktrees |
| DeepSeek harness | A timed `ask_user_question` whose answer can arrive as a later turn, even after a host restart. | A web user answers hours later; the question should survive the tool call and a node restart. | preview, unreviewed | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/user-questions |

### Context and compaction

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| Oh My Pi | Compaction is an ordered chain: provider-native, snapcompact (older history archived as images, needs a vision model), handoff, `shake` (strips bulk, no model call), soft summary. | The best fallback design in the set; `shake` gives a no-LLM last resort. | GA, corrected (idle compaction off by default) | https://omp.sh/docs/compaction |
| OpenClaw | `safeguard` mode audits each summary for structure, pending requests and identifiers, marks degraded results, and rejects invalid ones. | Cheap quality control that fits our `history_summarized` event. | GA, reviewed | https://docs.openclaw.ai/concepts/compaction |
| Hermes Agent | A failure ladder (60, 300, 900 s) ends in a deterministic fallback summary; the docs state that if the summary model's window is smaller than the main model's, middle turns are dropped without a summary. | A documented failure mode to test for the moment we add a summariser slot. | GA, reviewed | https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching |

### Memory

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| Hermes Agent | `memory.write_approval` and `skills.write_approval` (default off) stage agent writes for approve or reject; a curator marks agent-written skills stale at 14 days, archives at 30, with pin, lock, a hash ledger and rollback. | The only field example of both a human gate and lifecycle management for agent-written memory. | GA, reviewed | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/features/skills |
| Antigravity | `/learn` drafts a `learning_proposal.md` artifact and writes rule or skill files only after explicit approval. | The other approval-gated design, file-based and reviewable. | GA, reviewed | https://antigravity.google/docs/commands/learn |
| OpenClaw | Dreaming promotes memories only past score and recall thresholds and rejects a rewrite that drops more than 25% of prior entries. No human step; the project admits no time-based retention. | The most defensible no-approval design, and the project's own admission of what that costs. | GA, reviewed | https://docs.openclaw.ai/concepts/dreaming |

### Sub-agents, automation and model slots

| Harness | What it does and how | Why it matters for us | Status, check | Evidence |
|---|---|---|---|---|
| Claude Code | Sub-agent reports are rewritten if they imitate harness control tags; sub-agents cannot escalate past the parent's mode. | A cheap injection filter at the delegation boundary; our sub-agent and cross-node reports go straight to a parent. | GA, unreviewed | https://code.claude.com/docs/en/agent-sdk/subagents.md |
| DeepSeek harness | Model-written workflow scripts run under a `maxTotalAgents` cap that the script cannot see. | A budget the model cannot plan around. | preview, reviewed | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow |
| Hermes Agent | Script-only cron jobs, and `{"wakeAgent": false}` gating: a cheap check decides whether to pay for an agent run. | Directly applicable to our routines. | GA, reviewed | https://hermes-agent.nousresearch.com/docs/user-guide/features/cron |
| Hermes Agent | Auxiliary models per task (compression, vision, titles, approval and others), each with its own fallback chain beside the turn-scoped main chain; unset slots default to the main model. | The cleanest answer to "a summariser outage should not take titles down", and the shape our planned slots should take. | GA, unreviewed | https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers, https://hermes-agent.nousresearch.com/docs/integrations/providers |
| OpenCodex | Vision sidecar: describes images for text-only models, three at a time, eight per turn, cached. | Exactly the vision bridge the owner plans; its output is model-written text and must be treated as untrusted. | GA, reviewed | https://opencodex.me/guides/sidecars/ |

### Technically impressive, and why

This round read documentation, not code. "Impressive" below means the documented mechanism is specific, bounded and honest about its limits. Round 2 must confirm that the code matches.

- **Claude Code** documents the most mechanisms, and several are first of their kind: a classifier approver with stripped inputs and circuit breakers, hook `defer`, self-hosted runners with leases, single-use work orders (one runner each, 120-second spawn lease) and a session JWT with a public JWKS, receiver-side trust for cross-session messages, worktree hardening (https://code.claude.com/docs/en/self-hosted-environments-configuration.md). Against it: it is closed, its docs lag its changelog, its credential masking is partial and depends on experimental TLS termination (corrected in review), and headline features are research preview.
- **Codex** has the most complete sandbox story across macOS, Linux and Windows and says plainly where it falls short: DNS-rebinding protection is best effort, deny-read globs are a per-process snapshot, the proxy does not cover search or MCP (https://learn.chatgpt.com/docs/permissions.md). Its App Server is one protocol for every client with generated schemas and an overload error. Its rules with inline tests and hooks trusted by hash are small, correct ideas.
- **OpenClaw** has the most rigorous execution-path design among the open harnesses: argv-and-executable approval binding, a writer claim checked at commit, per-dispatch hashed credentials, owner-epoch fencing, two-layer pairing, and a trust model that says one Gateway is one trust boundary and that scopes are not tenant isolation (https://docs.openclaw.ai/gateway/security/trust-model). It also publishes a maturity scorecard with percentages.
- **Hermes Agent** stands out on memory and skill lifecycle and on the egress proxy, both with stated limits. Its breadth is also its risk: the reviewer notes hundreds of merged pull requests per patch release and many pages that are only listed.
- **DeepSeek harness** has the cleanest written log discipline (every policy and mode change is an event; anything the model sees must be in the log) and the most careful persistence format. It is a developer preview with no security audit, by its own statement.
- **Oh My Pi** is the most inventive (hash-anchored edits, snapcompact, TTSR stream rules, internal URL schemes), but its evidence is weaker: the hashline benchmark is the maintainer's own (https://stencil.so/blog/the-harness-problem, corrected in review), and its README and docs disagree on tool counts, roles and memory tools.

## 7. Notable architectural and UX approaches

Everything here is what public sources document. Where a mechanism is inferred rather than stated, the row says so. Terms: the **control side** is whatever holds the UI, accounts and queue (our master); the **execution side** is the machine where commands and file edits happen (our node); the **loop** is the model-call and tool-call cycle.

### Where the loop runs, and where the tools run

The full comparison, with credentials, policy, transport, pairing, data flow, restart behaviour and evidence for 31 documented designs, is the control-side versus execution-side table at the start of section 2 of `round1/synth/09-remote-distributed-architecture.md`. The summary:

| Shape | Designs | Who decides what may run |
|---|---|---|
| **Matches ours**: loop and tools on the machine that owns the code; the other side is a UI or relay | Codex SSH remote and remote TUI, Codex remote control, OpenCode server, Claude Code Remote Control, Claude Code self-hosted runner, Claude Desktop SSH and Dispatch, Antigravity Remote Control, OpenHands Canvas plus Agent Server, OpenClaw cloud workers in `worker-turn` mode, goose `serve` and Roaming, Warp self-hosted worker (loop placement inferred), Cline SSH remote, Zed remote development (where model calls originate is contradictory on its page) | Mostly the control side. See below. |
| **Inverted**: loop on the control side, tools on the worker | Cursor self-hosted workers, OpenCodex Remote Workspace, OpenClaw Gateway plus nodes, DeepSeek harness SSH backend, Pi pi-env, Hermes Agent terminal backends. ACP is a partial inversion (the agent loops; the client executes `fs/*` and `terminal/*`). | The control side, by construction. |
| **Neither**: loop and tools both at the vendor | Codex Cloud, Claude Code cloud sessions, Cursor cloud agents, Antigravity Remote Agents, Cline cloud agents, Warp hosted runs, OpenHands Cloud | The vendor. |

The rows that matter most to us:

- **Claude Code self-hosted runner** is the closest in shape and the furthest in authority. The runner dials out only, polls for work under a lease (about 60 seconds), and runs each session as a child process. But the control plane sends each session's tool list and permission rules with the work payload, plus model, connectors and hooks; the runner can only add a floor (image `managed-settings.json`, `--confine-repo-settings enforce`) (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected in review). Prompts, responses and tool results go to Anthropic, which stores the transcript.
- **Cursor self-hosted workers** invert our design: loop, planning and inference stay in Cursor's cloud; the worker executes edits, terminal commands and local MCP over one outbound HTTPS connection, and sends back file contents, terminal output, diffs and screenshots. The OS user is the boundary (https://cursor.com/docs/cloud-agent/self-hosted).
- **OpenClaw Gateway plus nodes** is the closest topology. Nodes dial the Gateway, sign a challenge with a device key, and declare commands; a command runs only if the node declared it, the operator approved it, and the Gateway's per-platform allowlist includes it. The node can withhold a command by not declaring it, but the gate is on the Gateway (https://docs.openclaw.ai/nodes/command-policy).
- **Warp self-hosted worker**: the control plane pushes the task, resolved config and scoped runtime credentials; the worker dials out (https://docs.warp.dev/platform/architecture/).
- **Ours**: the node runs loop and tools, holds the provider key, and its `node.toml` is the only authority; approval mode is `min(requested, node max)`; nothing over HTTP or the link writes it. No other row documents an execution-side file that outranks what the control side sends.

Outbound-only connections from the execution side are the norm (Claude Code runner and Remote Control, Cursor, Warp, OpenCodex executor, OpenClaw nodes, Antigravity, Hermes Relay), and ours matches. The designs where a client dials an agent server (Codex remote TUI, OpenCode, goose, OpenHands, DeepSeek harness) need the execution host to listen, and that is where the weakest authentication defaults sit: OpenHands has no authentication by default (https://docs.openhands.dev/overview/faqs.md), and Codex's non-loopback App Server listeners are unauthenticated by default "during rollout" (https://learn.chatgpt.com/docs/app-server.md).

### One protocol for every client

Several harnesses put every client on one protocol so that the UI is just another client:

- **Codex App Server**: JSON-RPC over stdio, WebSocket or a Unix socket, one `initialize` with capability negotiation and an `experimentalApi` gate, server-initiated approval requests, version-matched generated schemas, bounded queues that answer `-32001`. It carries the TUI, IDE, desktop and SDKs, and is labelled experimental (https://learn.chatgpt.com/docs/app-server.md).
- **OpenCode**: one background server per machine with an OpenAPI 3.1 HTTP API; TUI, web and desktop attach to it (https://opencode.ai/docs/api/).
- **goose**: everything is ACP, over stdio and over HTTP plus WebSocket (https://goose-docs.ai/docs/gdk/acp/index.md).
- **Warp**: all clients use the same control-plane APIs, with a published OpenAPI spec and SDKs (https://docs.warp.dev/platform/architecture/).
- **Cline hub**: clients register capabilities with a per-machine hub, which routes a diff to the IDE and a shell request to the CLI; no authentication is documented (https://docs.cline.bot/sdk/architecture/hub-spoke.md, corrected in review).

Ours already works this way (master REST and `/api/ws` for the UI, a versioned master-node protocol for nodes), but it has no published spec and the API token is admin-equivalent. See "Programmatic control protocol" in `round1/synth/09-remote-distributed-architecture.md`.

### Client protocols: ACP and who speaks it in which role

The Agent Client Protocol (ACP) is a JSON-RPC protocol between an editor (client) and an agent. Its design choices matter more than its adoption:

- The agent runs the loop, but `fs/*` and `terminal/*` calls are executed **by the client**, which must apply its own policy (https://agentclientprotocol.com/protocol/v1/terminals.md).
- Permission requests are advisory; the agent stores "always" decisions, and the protocol standardises only the prompt's option kinds (https://agentclientprotocol.com/protocol/v1/security.md).
- The standard transport is stdio. A remote transport (Streamable HTTP or WebSocket on one endpoint) is still an Active RFD (https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md).

| Role | Who, as documented |
|---|---|
| Agent (served to editors) | OpenCode, DeepSeek harness, Oh My Pi, Cursor, Antigravity, OpenHands CLI, goose, OpenClaw, Hermes Agent, Cline. Codex, Claude Code and Pi are adapter-only. Warp's support appears only in changelog wording (unverified). |
| Client (hosting other agents) | Zed (the reference client), OpenHands Agent Server (runs Claude Code, Codex, Gemini, Pi, OpenCode as backends, credentials resolved by secret name), goose ACP providers, OpenClaw `/acp spawn` |
| Remote transport shipped | goose only, ahead of the RFD |

Nobody in the client role documents how a hosted agent's approval requests reach a human (see "Other harnesses as sub-agents" in `round1/synth/04-planning-subagents-background.md`). That is the gap any ACP integration of ours would have to close, because ACP's advisory permission model and agent-side "always" store sit badly with "the node decides".

### Event logs versus transcripts

| Model | Who | What is documented |
|---|---|---|
| Event log as source of truth with a stated writer rule | OpenClaw, DeepSeek harness, ours | OpenClaw records an active-writer claim and verifies it in the commit transaction (https://docs.openclaw.ai/concepts/agent-loop, corrected). DeepSeek derives model context from an append-only typed log and requires anything the model sees to be logged (https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle). |
| Durable log separate from a volatile stream | OpenCode | A sequence-numbered per-session log readable with `follow=true`, beside a broadcast stream that drops slow consumers (https://opencode.ai/docs/api/). |
| Event-sourced, no writer rule stated | OpenHands | An append-only `EventLog` with a FIFO lock (https://docs.openhands.dev/sdk/arch/conversation.md). |
| Transcript file | Claude Code, Pi, Oh My Pi, Codex | Claude Code keeps an append-only JSONL transcript (https://code.claude.com/docs/en/sessions.md); its single-writer protection is advisory and two terminals can interleave (see "Session storage" in `round1/synth/05-sessions-git.md`). Pi and Oh My Pi keep a branching JSONL tree (https://pi.dev/docs/latest/session-format). |

Ours is in the first group and is the only one that combines a single writer on the execution side with a mirror to a self-hosted control side. What we lack is lifecycle: retention, compaction of streaming deltas, and crash-torn write handling (see "Where we are ahead" in section 8).

### Keeping credentials away from the execution side

Four patterns are documented (see "Credential placeholders and injection at an egress proxy" in `round1/synth/02-permissions-sandbox-security.md` and "Central credential broker or inference proxy" in `round1/synth/09-remote-distributed-architecture.md`):

1. **The key never enters the sandbox; a vendor proxy attaches it after the request leaves.** Codex Cloud network secrets (https://learn.chatgpt.com/docs/environments/cloud-environments.md), Claude Code cloud sessions (https://code.claude.com/docs/en/cloud-environments.md), Cursor runtime secrets (vendor statement, https://cursor.com/docs/cloud-agent/setup).
2. **A placeholder inside, substituted by a local proxy outside the sandbox.** Hermes iron-proxy (https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy, corrected), Pi's documented Docker Sandboxes pattern (https://pi.dev/docs/latest/containerization, unreviewed), Claude Code's sandbox `mask` mode, which needs experimental TLS termination, covers sandboxed Bash only, and is a different mechanism from its cloud secrets (https://code.claude.com/docs/en/sandboxing.md, corrected in review).
3. **Inference proxied by the control side, so workers never hold provider keys.** OpenClaw cloud workers (beta, https://docs.openclaw.ai/gateway/cloud-workers), Oh My Pi `auth-gateway` (https://omp.sh/docs/secrets), OpenCodex hub (https://opencodex.me/guides/remote-hub/), Warp Agent traffic (https://docs.warp.dev/platform/architecture/). Oh My Pi's `auth-broker` is weaker than its name suggests: it hands clients API keys as well as access tokens, and is loopback-only plain HTTP without TLS (corrected in review). Oh My Pi states that the proxy operator can read prompts and responses.
4. **The key sent per session, never written to the worker.** Cline SSH remote (https://docs.cline.bot/usage/cline-desktop-ssh.md).

Ours does none of these: each node holds its provider key in `config.toml` and calls the provider directly; tool children have provider key variables stripped from their environment.

### How policy is layered, and who can override whom

| Pattern | Who | Note |
|---|---|---|
| Admin file outranks everything on one machine | Codex `requirements.toml` and MDM, with an optional self-hosted `[config_server]`; omitted permission profiles are denied, including future built-ins (https://learn.chatgpt.com/docs/enterprise/managed-configuration.md) | The strongest admin design; single host, not a split. |
| Vendor console pushes managed settings | Claude Code (hourly; MDM every 30 minutes), with strict-value-wins keys a lower scope may tighten but not loosen (https://code.claude.com/docs/en/settings.md). Its own docs say server-managed settings are a client-side control, not a security boundary (https://code.claude.com/docs/en/server-managed-settings.md). | |
| A policy layer that "only tightens" | OpenCode `policies`: but a policy `allow` can lift an earlier policy deny, so it is not deny-only (https://opencode.ai/docs/policies/, corrected in review) | The caveat is the lesson. |
| Merge by decision kind | Cursor hooks: deny beats ask beats allow for permission fields regardless of source; for other fields the last response wins (https://cursor.com/docs/hooks, corrected: not "most restrictive wins") | |
| Policy that does not reach every surface | Cline remote config covers the IDE extensions, gives the CLI only rules and workflows, and Desktop nothing (https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md) | |
| Filesystem permissions only | Hermes `/etc/hermes` overrides user files per key; no signing, no MDM (https://hermes-agent.nousresearch.com/docs/user-guide/managed-scope) | |
| Repo config may not widen | Codex ignores provider, base URL, `notify`, `otel` and profile keys in project config with a warning (https://learn.chatgpt.com/docs/config-file/config-advanced.md); Claude Code lists keys project files cannot set (https://code.claude.com/docs/en/settings.md) | Oh My Pi loads everything from the repo with no trust prompt. |
| **Execution side outranks control side** | **Ours only**: `node.toml` on the node | The inverse direction from everyone above. |

### UX patterns

**Reviewing plans.** Antigravity drafts an Implementation Plan artifact; comments attach to the next message as a chip, and `/plan` always stops for approval (https://antigravity.google/docs/plan). Oh My Pi offers "execute in a fresh session with only the plan" (https://omp.sh/docs/plan). Warp keeps an editable plan with version history and share links (https://docs.warp.dev/agents/capabilities/planning/). How plan mode is enforced varies more than how it looks: runtime tool rules in OpenCode (https://opencode.ai/docs/agents/) and the Antigravity CLI, prompt guidance only in DeepSeek harness (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan), and in Claude Code not enforced at all in interactive terminals where bypass is available (https://code.claude.com/docs/en/permission-modes.md).

**Reviewing diffs.** The field's standard is a whole-change review pane with per-hunk revert and inline comments fed to the agent's next turn: Codex desktop (https://learn.chatgpt.com/docs/environments/local-environment.md, unreviewed), Claude Desktop (https://code.claude.com/docs/en/desktop), Antigravity with Uncommitted, Branch and Agent Edits views (https://antigravity.google/docs/review-changes), Warp, including comments sent to third-party CLI agents (https://docs.warp.dev/code/code-review/), Zed per hunk (https://zed.dev/docs/ai/agent-panel). Ours shows the old and new strings of one `edit` call.

**Approvals.** Answer options range from once only (DeepSeek harness) to once, session, pattern and always (OpenCode, Zed, Codex). Approvals can be answered from a phone (Codex, Claude Code Remote Control, Antigravity, OpenCode) or inside a chat channel with approver lists (OpenClaw `/approve <id>`, https://docs.openclaw.ai/tools/exec-approvals). DeepSeek deliberately omits tool arguments from approval requests, which its own reviewer flags as approving blind. Claude Code excludes the permission prompt from what mods can render (https://code.claude.com/docs/en/plugins/mods/interface.md).

**Running several agents.** Claude Code's agent view groups background sessions by state ("Needs input", "Ready for review") with a peek-and-reply panel, backed by a per-user supervisor daemon; research preview (https://code.claude.com/docs/en/agent-view.md, corrected). Cline and Hermes run a Kanban where each card is an agent in its own worktree (https://docs.cline.bot/usage/kanban.md, https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban). Antigravity, Cursor and Warp have command-centre windows.

**Notifications.** Claude Code pushes to the phone when a task ends or a decision is needed, suppressed while the terminal is focused (https://code.claude.com/docs/en/remote-control.md); Cursor uses iOS Live Activities for up to eight agents (https://cursor.com/docs/cloud-agent/mobile); Warp uses toasts, a mailbox and tab badges (https://docs.warp.dev/agents/capabilities/agent-notifications/). Ours has browser notifications in a secure context and an attention list, with no PWA or service worker, so nothing reaches a closed tab.

**Hand-off between devices and machines.** Codex Handoff moves a chat and its Git state between the local computer and a connected remote host (https://learn.chatgpt.com/docs/remote-connections.md, corrected: "to a worktree" is not stated). Warp's handoff snapshots tracked and untracked uncommitted changes for a local-to-cloud move (https://docs.warp.dev/platform/handoff/, corrected). Claude Code Remote Control has explicit takeover rules ("another connection took over") and a resume window of about four hours (https://code.claude.com/docs/en/remote-control.md). Claude Desktop's SSH sessions are not documented to survive the app quitting: the remote login is renewed only while Desktop is connected, and the docs advise against counting on overnight runs (https://code.claude.com/docs/en/desktop, corrected in review).

### The central tension: authority on the node means credentials on the node

Our design puts authority on the execution side. A node must therefore be able to act on its own policy without asking the master, and that includes calling the model: the node holds the provider key. The field has gone the other way. Where it splits control and execution, it centralises policy and credentials together:

- Claude Code's runner receives its tool list and permission rules from the control plane; Warp's worker receives resolved config and scoped credentials; OpenClaw's Gateway holds the command allowlist and, for cloud workers, the provider keys. Cline's SSH remote keeps approvals and provider settings on the control side and sends the key per session.
- In those designs a compromised worker leaks little: no long-lived model key, a short-lived or per-dispatch credential (Claude Code work orders, Cursor per-claim tokens, OpenClaw hashed per-dispatch credentials). In ours, a prompt-injected `bash` on `main` can read the node's `config.toml`, and one long-lived node key does everything after enrolment.
- But the centralised designs move the trust problem rather than solve it, and their vendors say so. Claude Code calls its server-managed settings "not a security boundary" (https://code.claude.com/docs/en/server-managed-settings.md). OpenClaw says one Gateway is one trust boundary and that operator scopes are not tenant isolation (https://docs.openclaw.ai/gateway/security/trust-model). A compromised or hostile control side can widen what every worker runs. In ours it cannot: that is what "the node decides" buys. `crates/master/tests/hostile_node.rs` covers the hostile-node direction; the hostile-master direction still has no test (see "Ours against the field" in `round1/synth/09-remote-distributed-architecture.md`).

What the surveys show for each side:

| Question | Evidence for node-held credentials (ours) | Evidence for control-side credentials (the field) |
|---|---|---|
| Who can widen policy | Only the node's own file; no surveyed design matches this. | The control side, in every documented split. |
| Blast radius of a compromised execution machine | Large on `main`: the key is readable by unconfined `bash`. Gate3's Landlock sandbox closes the read path for the shell (unmerged). | Small: workers hold short-lived or no credentials. |
| Blast radius of a compromised control side | Small: a hostile master cannot widen `node.toml`. | Large, and stated by the vendors themselves. |
| What the control side sees | Every event is mirrored to the master already, so an inference relay would add little exposure. | The control side sees all traffic (Oh My Pi says so for its gateway). |
| Availability | The node calls the provider directly. | Inference depends on the control side being up. |

There is a middle pattern that keeps the key on the node but out of the agent's reach: a node-local credential proxy outside the sandbox, as Hermes iron-proxy does for its container backends and as Pi documents for Docker Sandboxes. It is compatible with "the node decides". It is also heavier than it looks: Hermes lists a stolen CA key, raw sockets and cloud SigV4 as limits, and Claude Code's local equivalent needs experimental TLS termination and leaves the real file readable when its pattern does not match.

The question is not settled by this evidence. No surveyed harness combines execution-side authority with control-side credentials, so there is no field data on how that combination fails. The decision belongs to the owner, before Gate 3 work on the link, and should be made after the gate3 sandbox is merged and its residual gaps (pathname Unix sockets before Linux 7.1, UDP and DNS) are measured on real nodes.

## 8. Potential opportunities for our project

Two warnings before the list. First, `CLAUDE.md` puts a feature freeze in place until Gate 3 of the fix plan. Most items below are features and cost against that plan; a few are correctness fixes and are marked as such. Second, every "ours" statement in the synthesis describes branch `main`. Where pull request 4 (`integration/gate3`, open, CI green, awaiting an outside security review) or the local branch `feat/agent-roster` changes our position, it is stated per branch.

### Where we are ahead

Ahead on structure, not on feature count. Several of these leads exist only once gate3 merges.

| Lead | Evidence from the field | Branch |
|---|---|---|
| **The execution side is the authority.** `node.toml` is the only policy source; approval mode is `min(requested, node max)`; nothing over HTTP or the link writes it. | No surveyed design documents an execution-side file that outranks the control side. Claude Code's runner receives tool lists and permission rules from the control plane (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected); Warp pushes resolved config (https://docs.warp.dev/platform/architecture/); OpenClaw's Gateway allowlist is the gate (https://docs.openclaw.ai/nodes/command-policy). | `main` |
| **The owner owns the control side.** Every event mirrors to the master the owner runs. | Claude Code's runner and Remote Control store transcripts at Anthropic; Cursor's worker sends file contents and terminal output; Warp stores transcripts by default. See the control-side table in `round1/synth/09-remote-distributed-architecture.md`. | `main` |
| **Multi-user with private chats in the open tier.** Accounts, workspaces, roles, chats private to their owner. | OpenHands keeps RBAC and SAML in Cloud and Enterprise (https://docs.openhands.dev/openhands/usage/agent-canvas/agent-profiles.md); OpenClaw has multi-user in open code but says scopes are not tenant isolation (https://docs.openclaw.ai/gateway/operator-scopes). See "Accounts, roles" in `round1/synth/08-ui-browser-collaboration.md`. | `main` |
| **Children start from an empty environment** plus an allow-list; provider keys always stripped. | Codex passes `*KEY*`, `*SECRET*`, `*TOKEN*` by default (https://learn.chatgpt.com/docs/config-file/config-advanced.md); Claude Code's sandbox inherits secrets unless an opt-in scrub is set (https://code.claude.com/docs/en/env-vars.md). No other harness documents an empty-environment start. | `main` |
| **Prompt-injection containment on the web path.** `web_fetch` only fetches URLs that appeared in the user's words or tool output; SSRF-safe resolver re-checked per redirect; untrusted webhook turns lose memory and web tools; no model HTML. | No other surveyed harness documents URL provenance. Gate3 tightens the provenance set, makes sub-agents inherit it, and closes IPv6 transition-address gaps (AUDIT L11, L12). Caveat: this covers the chat runtime only; agentic sessions reach the network through `bash`. | `main`, tighter on gate3 |
| **Agent-written memory needs owner approval** before it becomes active. | Only Antigravity `/learn` (https://antigravity.google/docs/commands/learn) and Hermes, opt-in and off by default (https://hermes-agent.nousresearch.com/docs/user-guide/features/memory), gate writes. Claude Code, Codex, OpenHands and OpenClaw do not. On `main` project members can activate project facts (AUDIT S12); gate3 restricts activation to a manager, which closes S12. | lead complete on gate3 |
| **Shadow-repo checkpoints with hardened git** (cleared env, no system or global config, no hooks), working on non-git folders. | Only OpenCode, Hermes and Cline document shadow git; none documents comparable git hardening (nothing documented, not proof of absence). See "File checkpoints and undo" in `round1/synth/05-sessions-git.md`. Unmeasured at scale: see "Worth studying". | `main` |
| **Unattended runs fail safe.** Agentic routines cannot run in `auto`; the webhook entry has a hashed per-routine secret, idempotency keys and token buckets. | Cline schedules default to yolo; Cursor cloud agents never ask; DeepSeek's SDK pins full access (see F17 in `round1/synth/02-permissions-sandbox-security.md`). | `main` |
| **Command output and cache discipline.** Output filter with a retrievable full copy under per-session random ids; observation masking batched to protect the prompt cache. | Claude Code and DeepSeek spill to a file; no other harness documents cache-keeping batch masking. See "Pruning" and "Large-output handling" in `round1/synth/03-context-memory.md`. | `main` |
| **Fallback is visible.** Per-agent fallback chains with a `model_fallback` event; no switching mid-stream. | Claude Code's fallback is not shown in `/status` (https://code.claude.com/docs/en/model-config.md); OpenCodex fails over only before output (https://opencodex.me/guides/combos/). | `main` |
| **Fail-closed sandbox default.** `[sandbox] mode = required` refuses agentic sessions where Landlock is not enforced. | Claude Code silently runs unsandboxed by default (https://code.claude.com/docs/en/sandboxing.md); Zed on Linux falls back with a warning (https://zed.dev/docs/ai/sandboxing.md). | gate3 only |

### Table stakes we lack

Ranked by how much each matters for a self-hosted master/node harness that runs shell commands on other people's machines.

#### 1. A shell sandbox, on `main`

- **What.** Kernel confinement of agent shell commands. On `main`, `bash` runs as the node's OS user, can read the data directory and the provider key, and has no network control (AUDIT S1).
- **Who and how.** Seven of sixteen harnesses document one. Copy **Codex** for posture: fail closed when the policy cannot be enforced, protected subpaths (`.git`, agent config, resolved gitdir) read-only inside writable roots, and candid limits in the docs (https://learn.chatgpt.com/docs/sandboxing.md, https://learn.chatgpt.com/docs/agent-approvals-security.md). Copy **DeepSeek harness** for honesty: report `full` or `partial` enforcement per backend (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox). **Cursor** is the Landlock-plus-seccomp reference on Linux 6.2+ (https://cursor.com/docs/agent/security/run-modes).
- **What goes wrong.** Claude Code's default silently runs unconfined when the sandbox cannot start. Codex rejects Landlock for filesystem-restricted policies because it cannot isolate Unix sockets: that is exactly our residual.
- **Invariants.** Fits "the node decides": the mode lives in `node.toml`.
- **Dependency.** This is pull request 4. It adds Landlock confinement (Linux 6.2 or newer, optional TCP block), fails closed, and has known residuals: pathname Unix sockets such as the Docker socket or D-Bus are blocked only from Linux 7.1, and UDP and DNS are not covered. Merging it is Gate 3 work, not a feature.
- **Size.** Small to finish (the code exists and is under review). Small to add per-node `full`/`partial` reporting and OS-level protection for `.git/hooks` and `.git/config`.

#### 2. Context handling keyed to the real model window, with overflow recovery

- **What.** Our trigger is a characters-divided-by-four estimate against a fixed 24,000; a context-length error ends the turn; there is no `/compact`. On nodes with different models this is a correctness bug.
- **Who and how.** Fifteen of sixteen key compaction to the window or a reserve. Copy **Pi**: compact at window minus a 16,384-token reserve, keep about 20,000 tokens verbatim, and on an overflow error compact and retry once (https://pi.dev/docs/latest/compaction). **OpenCode** does the same with a 15% buffer (https://opencode.ai/docs/compaction/). Add **OpenClaw**'s summary audit (https://docs.openclaw.ai/concepts/compaction).
- **What goes wrong.** Hermes documents that a summary model with a smaller window than the main model silently drops middle turns (https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching). Claude Code stops after repeated compactions that refill the context, and notes that a gateway rewriting the too-long error breaks reactive compaction.
- **Invariants.** Compaction must be an event sequence written by the session actor (DeepSeek logs it as a locked start/end sequence and treats a start without an end as a crash orphan).
- **Dependency.** None. Arguably a fix, not a feature.
- **Size.** Medium.

#### 3. Provider retry and backoff

- **What.** A single 429 or 5xx either triggers a fallback model or ends the turn.
- **Who and how.** Copy **Pi**'s small, exact defaults: 3 retries, 2,000 ms base, 60,000 ms max, provider-level retries 0 so layers do not multiply (https://pi.dev/docs/latest/settings). For unattended routines, take **Claude Code**'s rules: never retry a spend-limit 429, stop when a gateway's `retry-after` exceeds 60 s, abort quiet streams with a watchdog (https://code.claude.com/docs/en/errors.md, unreviewed). Take **Hermes**' ordering: fall back after retries for 429 and 5xx, immediately for 401, 403 and 404.
- **What goes wrong.** Retry layers that multiply; retrying billing failures.
- **Invariants.** Node-side; none touched.
- **Dependency.** None. A fix.
- **Size.** Small.

#### 4. Web tools and memory proposals on agentic nodes

- **What.** `web_search`, `web_fetch` and `remember` are registered only in the chat runtime. An agentic Software Developer has no web search; a Researcher delegated on a node has no web tools; agentic sessions can read memory but never propose it. The README and the built-in agent definitions promise otherwise.
- **Who and how.** Eleven of sixteen harnesses ship web fetch and search. Our own chat-runtime implementation is the model to copy, because no other harness documents URL provenance.
- **What goes wrong.** Today agentic sessions reach the network only through `bash`, which bypasses provenance, the SSRF resolver and the untrusted-turn rules. Adding web tools without the sandbox's network block leaves both paths open.
- **Invariants.** "The master never runs tools": the web tools must run on the node, not be relayed to the master's chat runtime. That puts any search-provider key on the node as well, which feeds the credentials question below. The untrusted-webhook rules must carry over.
- **Dependency.** `feat/agent-roster` (the roster assumes these tools) and gate3 (tighter provenance, sub-agents inheriting the parent's allowed set, network block for `bash`).
- **Size.** Medium.

#### 5. Steering and queueing while a turn runs

- **What.** A prompt during a running turn is rejected with "a turn is already running". Twelve of sixteen accept it. For a web UI driving a remote node, cancel is the only recourse today.
- **Who and how.** Copy **OpenClaw**: accepted input is stored before the Gateway acknowledges it, with `steer` as the default mode, a debounce and a cap of 20 per session (https://docs.openclaw.ai/concepts/queue). Add **Claude Code**'s protocol details: per-message priority (`next`, `later`, `now`) and an interrupt receipt listing still-queued message ids, which a web client that loses its connection needs (https://code.claude.com/docs/en/agent-sdk/typescript.md, unreviewed).
- **What goes wrong.** Queue-by-default versus steer-by-default differs across harnesses; pick one and show it.
- **Invariants.** One writer per session: the master relays, the node's session actor appends the queued message as an event and decides the step boundary.
- **Dependency.** Gate3's per-session command lanes, so cancel and approvals are never stuck behind a slow command.
- **Size.** Medium.

#### 6. Repository instruction files and skills, read on the node

- **What.** Fourteen of sixteen read `AGENTS.md` or an equivalent; sixteen of sixteen read `SKILL.md` skills. We read neither; our project instructions live on the master, away from the code.
- **Who and how.** Read `AGENTS.md` and `.agents/skills/` (the only cross-vendor skill path) with progressive loading: name and description up front, body on demand. Copy **Codex**'s project-config key blocklist the moment any repo config is read (https://learn.chatgpt.com/docs/config-file/config-advanced.md) and **Pi**'s separate project trust gate (https://pi.dev/docs/latest/security).
- **What goes wrong.** Skill listings overflow the context (Codex and Claude Code both added budgets); skills that run shell at load time (OpenCode's run outside permissions); project skills trusted automatically; frontmatter that grants tools (Claude Code `allowed-tools`); headless modes that treat any folder as trusted (Claude Code `-p`). See "Skills" in `round1/synth/06-extensibility.md`.
- **Invariants.** The node reads them, consistent with "the node decides". Nothing a repository ships is trusted: no `!cmd`, no `allowed-tools`, no frontmatter hooks in the first version.
- **Dependency.** None, but it breaks the freeze.
- **Size.** Medium.

#### 7. A plan mode enforced by the node

- **What.** No read-only plan-then-approve phase. Ten of sixteen ship one.
- **Who and how.** Copy the enforcement of **OpenCode** (the `plan` agent's rules deny edits outside a plan directory, https://opencode.ai/docs/agents/) and the **Antigravity CLI** (`--mode=plan` allows only read and search tools, https://antigravity.google/docs/cli/modes). Record the mode as an event, as DeepSeek logs `plan/mode`. Copy the review UX of **Antigravity** (comments attach to the next message, https://antigravity.google/docs/plan) and **Oh My Pi**'s "execute in a fresh session with only the plan" (https://omp.sh/docs/plan).
- **What goes wrong.** DeepSeek's plan mode is prompt guidance that the sandbox ignores; Claude Code does not enforce it in interactive terminals where bypass is available (https://code.claude.com/docs/en/permission-modes.md); Cline's lets `python -c` write (https://docs.cline.bot/features/subagents.md).
- **Invariants.** A node-enforced tool and policy state, approved through the existing approval path. A plan mode that keeps an unconfined `bash` is not read-only.
- **Dependency.** Gate3 sandbox (read-only `bash` needs it); the roster (a Plan agent).
- **Size.** Medium.

#### 8. Whole-turn diff review with comments back to the agent

- **What.** We show the old and new strings of one `edit` call and a per-turn file list. The field's standard is a review pane with per-hunk revert and inline comments sent with the next message.
- **Who and how.** Copy **Claude Desktop** (comment on a line, sent with the next message, https://code.claude.com/docs/en/desktop) and **Antigravity** (Uncommitted, Branch and Agent Edits views, https://antigravity.google/docs/review-changes). The data already exists: the shadow repo holds the before and after of every turn.
- **What goes wrong.** Claude Code's web review lists only Claude's own edits, so formatter changes are invisible, and ignores repo diff drivers (https://code.claude.com/docs/en/claude-code-on-the-web.md); Antigravity's discard deletes untracked files immediately.
- **Invariants.** Diff content is file content leaving the node, like every other event; render as text, never HTML.
- **Dependency.** None.
- **Size.** Medium.

#### 9. Background processes and timeout-to-background

- **What.** An agent cannot run a dev server or watcher without blocking until the timeout kills it. Eleven of sixteen document background processes.
- **Who and how.** Copy **Claude Code**: a foreground command at its timeout moves to the background with a task id and output path instead of being killed, and `Monitor` turns background output into conversation events with a mandatory deadline (https://code.claude.com/docs/en/tools-reference.md). **Hermes** auto-backgrounds a trailing `&` and refuses `nohup` and `disown` (https://hermes-agent.nousresearch.com/docs/user-guide/features/tools, unreviewed).
- **What goes wrong.** Pi's answer is "use tmux", which only works where a user is present. goose documents that spawned processes survive revocation, and the same risk applies to any process that outlives its session.
- **Invariants.** Background children must stay inside the sandbox and die with the session; the harness process stays light.
- **Dependency.** Gate3 sandbox and cancel handling.
- **Size.** Medium to large.

#### 10. An MCP client on the node, with tool deferral from day one

- **What.** No MCP at all; sixteen of sixteen have it.
- **Who and how.** **Pi**'s per-server exposure modes (`codemode`, `deferred`, `direct`, `hidden`) are the systematic answer to context bloat (https://pi.dev/docs/latest/mcp); Pi argued against MCP for that reason, then shipped it. **Claude Code** tool search is the deferral reference (https://code.claude.com/docs/en/mcp.md).
- **What goes wrong.** Project-supplied servers launching processes in headless runs, servers running outside the sandbox and permission rules, egress controls that miss MCP (Codex's proxy), schemas providers reject, tool descriptions used as an injection channel. See "MCP" in `round1/synth/06-extensibility.md`.
- **Invariants.** Only `node.toml` may add a server: a stdio server is arbitrary code, so no HTTP or protocol path may add one. The approval binding must be per call, not per tool name per turn, before MCP tools exist.
- **Dependency.** Gate3; approval binding (below).
- **Size.** Large.

### Worth copying

Ranked. Each is small enough to adopt without a design debate.

1. **Bind approvals to argv, cwd, env and executable (OpenClaw).** Our "allow for this turn" keys on the tool name, so one click allows every later `bash` command of the turn (AUDIT L10). Gate3 honours it only after a tool's first approval, but does not add argument binding. OpenClaw binds to exact argv and cwd, the env when present, a pinned executable path and a content hash for writable executables, and re-checks before launch (https://docs.openclaw.ai/tools/exec-approvals). Fits node-side policy; record the binding in the approval event. Depends on gate3. Size: small to medium.
2. **A failed audit append rejects the approval (DeepSeek harness),** plus a decision-source field on each approval event (`config`, `hook`, `user_permanent`, `user_temporary`, as Claude Desktop's telemetry records, https://claude.com/docs/cowork/monitoring.md). Matches one writer per session exactly (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval). Size: small.
3. **Park unattended approvals instead of blocking (Claude Code `defer`).** A PreToolUse hook ends the headless turn with the call pending; the host resumes after a human decides, with no timeout (https://code.claude.com/docs/en/hooks.md, unreviewed; one deferred call per turn). Our routines block a turn for up to 24 hours. The pending call is a log event, so it survives a restart. Pair it with "auto inside a required sandbox" for agentic routines once gate3 lands: no competitor documents that combination with node authority. Size: small to medium.
4. **Scan sub-agent reports before the parent reads them (Claude Code).** Neutralise imitations of harness tags, mark reports as carrying no user authority (https://code.claude.com/docs/en/agent-sdk/subagents.md, unreviewed). Applies to sub-agent and future cross-node reports. Size: small.
5. **Fence stale nodes and forbid fallback (OpenClaw, OpenCodex).** A node that reconnects with a rotated credential or stale state is rejected (owner-epoch fencing, https://docs.openclaw.ai/gateway/cloud-workers/security-model); a session bound to an offline node never runs elsewhere (https://opencodex.me/guides/remote-workspace/). Both protect one writer per session. Size: small.
6. **Routine pre-checks and a catch-up rule.** Hermes' `wakeAgent: false` lets a script decide whether to pay for an agent run (https://hermes-agent.nousresearch.com/docs/user-guide/features/cron); Claude Desktop runs one catch-up for missed scheduled tasks within 7 days (https://code.claude.com/docs/en/desktop-scheduled-tasks, corrected for other details). Size: small.
7. **An explicit overload reply on the link (Codex).** Gate3 bounds the link (8 inbound messages per node, 64 outbound commands, 8 MiB frames); Codex's App Server answers `-32001` "Server overloaded" rather than stalling (https://learn.chatgpt.com/docs/app-server.md). Size: small, after gate3.
8. **Cost as a labelled estimate, budgets checked before a turn starts.** OpenCodex labels cost an API list-price equivalent, not billing, with coverage warnings (https://opencodex.me/guides/web-dashboard/, corrected; see "Token and cost display" in `round1/synth/07-models-observability.md`); OpenCode Console enforces at request start with a 429 (see "Budgets and limits" in the same file). Our `budget` frontmatter field is parsed and never read, which is worse than absent. Budgets only tighten, so the master enforcing them for chat does not conflict with "the node decides"; agentic budgets belong on the node. Size: medium.
9. **Secrets-aware session export (Hermes `--redact`, OpenCode `--sanitize`).** Ten of sixteen can export; we cannot. Owner-only export keeps chats private. Size: small.
10. **Log lifecycle.** Our log keeps every streaming delta forever (AUDIT D5). Claude Code sweeps after 30 days, OpenClaw has `pruneAfter` and `maxEntries`, and DeepSeek's checksummed zstd frames with torn-tail truncation handle growth and torn writes (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence). Before the first tagged release this can change the schema in place. Size: medium.

### Worth studying before deciding

1. **Where model credentials live.** Every node holds a long-lived provider key. Options: (a) keep node-held keys behind the gate3 sandbox (simplest, keeps "the node decides" whole); (b) a node-local credential proxy outside the sandbox (Hermes iron-proxy, https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy; Pi's Docker Sandboxes pattern, https://pi.dev/docs/latest/containerization), which keeps the key on the node but out of the agent's reach, at the cost of TLS interception; (c) inference relayed through the master (OpenClaw cloud workers, https://docs.openclaw.ai/gateway/cloud-workers), which removes keys from nodes but makes nodes depend on the master and adds a single point of failure. See the central tension in section 7. Decide before Gate 3 work on the link. Size: medium for (b), large for (c).
2. **Per-session credentials on the link.** After enrolment one long-lived Ed25519 node key does everything. Cursor mints per-claim session tokens (https://cursor.com/docs/cloud-agent/self-hosted/pool); OpenClaw stores per-dispatch credentials only as a hash; Claude Code's work orders register exactly one runner and expire with a 120-second spawn lease, and every session gets a short-lived JWT (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected). For us the risk is lower because the node dials and holds its own key; the gain is revoking one session without dropping the node. Size: medium.
3. **A model-based approver.** Twelve harnesses ship one and every vendor caveats it: Cursor "not a security boundary" (https://cursor.com/docs/agent/security/run-modes), Claude Code "does not guarantee safety", goose fails open. Only safe behind a sandbox. If built, copy the converged safeguards: strip tool results from the reviewer's input, fail closed on reviewer error, circuit-break to a human after a few denials (Codex: 3 consecutive or 10 of 50, https://learn.chatgpt.com/docs/sandboxing/auto-review.md), drop broad allow rules while active. Run it on the node, with the node's model, so no command context leaves for a third party (Cursor's runs on its backend). Size: medium.
4. **A master-proposed policy that a node may only tighten.** Admins of a fleet will want to distribute policy. Codex's `requirements.toml` with a self-hosted config server denies omitted profiles including future ones (https://learn.chatgpt.com/docs/enterprise/configuration-server.md); OpenCode's "only tightens" layer can still lift an earlier deny (https://opencode.ai/docs/policies/, corrected), which is the bug to avoid. The node must remain the authority: a proposal is applied only as `min(proposal, node.toml)`. Size: medium.
5. **ACP, in which role.** Hosting Claude Code or Codex through `AgentBackend` (client role) is the bigger product lever; serving ACP from a node (agent role) lets editors attach. ACP's remote transport is still an RFD, its permission requests are advisory and the agent stores "always" decisions (https://agentclientprotocol.com/protocol/v1/security.md), and no host documents how hosted agents' approvals reach a human. Design the approval mapping onto `node.toml` before building. Size: large.
6. **Checkpoints at scale.** Our shadow-repo checkpoints are a lead today, but Zed removed per-prompt Git checkpoints in 1.27.0 because they slowed or hung large repositories (https://zed.dev/releases/stable, corrected); Cline documents large-repo slowness; Hermes caps checkpoints at 500 MB total, 10 MB per file and skips above 50,000 files; Claude Code lists what it does not restore (Bash-only changes, sub-agent edits, symlinks). We have only a count cap (latest 10). Measure on a large repository before claiming the lead. Size: small to measure.
7. **Fork with a file baseline.** Twelve of sixteen fork sessions; OpenHands and Antigravity explicitly do not fork file state. A Pi-style parent pointer over our append-only log (https://pi.dev/docs/latest/session-format) plus a shadow-repo baseline would be a fork that carries consistent files, which no harness documents. Size: medium.
8. **Moving a session between nodes.** Codex Handoff moves a chat with its Git state (https://learn.chatgpt.com/docs/remote-connections.md, corrected); Warp snapshots tracked and untracked uncommitted changes (https://docs.warp.dev/platform/handoff/). Our single-owner rule and per-node `node.toml` make this a design decision, not a copy. Size: large.

### What we should not copy

- **Controls their own vendors say are not a security boundary, used as one.** Claude Code's server-managed settings and Bash rules (https://code.claude.com/docs/en/server-managed-settings.md, https://code.claude.com/docs/en/permissions.md); Cursor's Auto-review (https://cursor.com/docs/agent/security); Oh My Pi's approvals and sub-agent isolation (https://omp.sh/docs/approvals, https://omp.sh/docs/subagents); Cline's plan mode and `.clineignore` (https://docs.cline.bot/customization/clineignore.md); OpenClaw's operator scopes (https://docs.openclaw.ai/gateway/operator-scopes); OpenHands' security analyzers (https://docs.openhands.dev/sdk/guides/security.md); OpenCode's policies for plugin code (https://opencode.ai/docs/policies/); ACP's permission requests. These are UX. Containment comes from the sandbox.
- **Plan mode as prompt guidance** (DeepSeek harness, and Claude Code in interactive terminals with bypass available).
- **The acting model judging its own risk.** Cline's model-set `requires_approval` (https://docs.cline.bot/features/auto-approve.md), OpenHands' inline `security_risk` argument, Warp's "Agent decides".
- **Fail-open judges and hooks.** goose's adversary reviewer (https://goose-docs.ai/docs/guides/security/adversary-mode.md), Hermes' goal judge on error (https://hermes-agent.nousresearch.com/docs/user-guide/features/goals), Claude Code mods by default (corrected in review: the built-in guard loads only with managed settings or a Team or Enterprise login).
- **Permissive defaults.** Oh My Pi's `yolo` default; Cline schedules defaulting to yolo and its Telegram connector approving everything (https://docs.cline.bot/cli/connectors.md); Codex's token pass-through; Claude Code's silent sandbox fallback; Codex's unauthenticated non-loopback App Server during rollout.
- **Skipping approvals by backend.** Hermes container backends skip dangerous-command approval and turn checkpoints off (https://hermes-agent.nousresearch.com/docs/user-guide/security, corrected). Any such rule belongs in `node.toml`, explicit.
- **Approval requests without arguments** (DeepSeek harness): the approver is approving blind.
- **Shell that bypasses the permission flow,** such as OpenCode's command `!cmd` blocks (see "Custom slash commands" in `round1/synth/06-extensibility.md`).
- **Link sharing of chats.** OpenCode discontinued it in v1.19.1 (https://opencode.ai/changelog/v1.19.1, corrected), and "chats are private to their owner" argues against it. End-to-end sealed relays (Oh My Pi collab, https://omp.sh/docs/collab) conflict with a master that must read the log.
- **One shared "small model" key.** Claude Code's `ANTHROPIC_DEFAULT_HAIKU_MODEL` also moves the goal evaluator and the `haiku` alias (https://code.claude.com/docs/en/model-config.md).
- **The loop on the control side** (Cursor self-hosted workers). It would break "the master never runs tools" in spirit and send every file the agent reads to the control side.
- **In-process code plugins and unsandboxed marketplaces.** Oh My Pi `auto` updates let a publisher ship code without approval; DeepSeek's `http:` plugin indexes are tamperable. If we ever package, package declarative parts only.
- **Subscription logins.** Anthropic's terms do not let a third-party harness offer claude.ai login; the carve-out allows an end user to sign in to the unmodified `claude` binary (https://code.claude.com/docs/en/agent-sdk/overview.md, corrected in review). Antigravity forbids third-party tools on its consumer service (https://antigravity.google/docs/faq).
- **"Sandboxed" wording for a browser that drives a live profile** (Antigravity, https://antigravity.google/docs/commands/browser).

### The two things the owner is designing now

#### The built-in agent roster (`feat/agent-roster`)

How the field builds rosters (the full table is the "Built-in agent roster" entry in `round1/synth/04-planning-subagents-background.md`):

- **Small rosters, three to six agents, split by capability:** a read-only explorer, a planner and a general worker. Codex ships `default`, `worker`, `explorer` (https://learn.chatgpt.com/docs/agent-configuration/subagents.md); OpenCode `build`, `plan`, `general`, `explore` plus hidden `title`, `summary`, `compaction` agents that are really model slots (https://opencode.ai/docs/agents/); Claude Code Explore, Plan, general-purpose and a few helpers (https://code.claude.com/docs/en/sub-agents.md); Cursor Explore and Browser.
- **Built-ins are mostly sub-agent only.** Claude Code's and OpenCode's explore and general agents cannot be the main agent.
- **Seven harnesses ship no named roster** (DeepSeek harness, OpenHands, OpenClaw, goose, Hermes, Cline, Pi) and rely on custom definitions.
- **Read-only is enforced by tool rules, not names.** Claude Code's Explore has Write and Edit denied; OpenCode's `plan` denies edits outside its plan directory; Codex agent files cannot override sandbox mode, approval policy, provider or MCP servers; Claude Code plugin agents ignore `hooks`, `mcpServers` and `permissionMode`; Antigravity agents carry a `commandExecutionPolicy` defaulting to `sandbox` (https://antigravity.google/docs/subagents). The warning case is Cline: a plan mode that removes edit tools but keeps a shell lets `python -c` write.

What our roster gets right:

- **Server-side enforcement of where an agent may run** (`available_as`, refused by `Resolver::top_level`), rather than only hiding it in a picker. The field documents where built-ins run; none documents refusing a sub-agent-only agent at the server.
- **A stale or unknown agent id is an error** (`AgentGone`), not a silent switch to another agent.
- **Unknown frontmatter fields are rejected.** Claude Code silently ignores them and skips files with bad YAML.
- **A default agent per mode** (chat: Assistant; Agentic: Software Developer), refusing a tool-less agent as the agentic default.
- **Children inherit the parent's approval mode, with approvals inside the child.** That is the Codex and Antigravity model; Cline and Oh My Pi, where approving the spawn approves everything, are the ones not to copy.
- **A table-driven test guards each agent's tool list.**

What it gets wrong, bluntly:

- **Eight is at the high end** of a field that converges on three to six.
- **Smol is a model slot dressed as an agent.** No tools, no memory: OpenCode ships exactly that shape and hides it. If its job is titles and other cheap side tasks, make it a slot. If it is a cheap chat agent for users, say so in its description.
- **Senior Teamlead duplicates Software Developer.** Software Developer already delegates to Scout, Researcher and Reviewer; the Teamlead is the old `ultracode` orchestrator under a new name.
- **Researcher and Software Developer are broken on agentic nodes.** The web tools they rely on are not registered there (table stakes item 4). Until that is fixed, Researcher is chat-only and a Researcher delegated by Software Developer has no web tools.
- **No Plan agent,** and no plan mode for it to pair with. Claude Code and OpenCode pair the two.
- **"Read-only" must mean no unconfined shell.** The inventory says Scout and Reviewer are read-only; if either can run `bash` on `main`, it is not read-only in any sense the Cline evidence would accept. Make the guarantee "read-only tools, or `bash` inside a read-only sandbox".
- **Only Scout is documented as sub-agent only.** Decide for each of the eight; the field's built-ins are mostly sub-agent only.
- **No other harness ships a built-in Writer.** Not wrong, but it should earn its place.

#### Model slots and memory scopes

**What the field does with slots** (the slot tables are under "Auxiliary model slots" in `round1/synth/04-planning-subagents-background.md`):

| Design | Who | What it shows |
|---|---|---|
| One small or fast model for every background task | Claude Code: titles, `/goal` evaluator, agent-view summaries, set by one variable that also changes the `haiku` alias (https://code.claude.com/docs/en/model-config.md) | The documented coupling trap. |
| A slot per task, each with its own fallback chain, defaulting to the main model | Hermes: compression, vision, titles, goal judge, curator, approval (https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers, unreviewed) | The cleanest design: a summariser outage does not take titles down. |
| Per-feature model keys | Zed: `thread_summary_model`, `compaction_model`, `commit_message_model`, `subagent_model` (https://zed.dev/docs/ai/agent-settings.md) | Simple, no fallback documented. |
| A role table with aliases and auto-resolution | Oh My Pi: ten roles, `@role` aliases, per-role fallback chains; unassigned roles resolve from whatever is authenticated (https://omp.sh/docs/roles) | Complete, but "which model ran this" becomes non-obvious. |
| Hidden agents as slots | OpenCode `title`, `summary`, `compaction` (https://opencode.ai/docs/agents/) | Same shape as our Smol. |
| Sidecar for a missing capability | OpenCodex vision sidecar, capped per turn and cached (https://opencodex.me/guides/sidecars/) | The vision bridge, as a pattern. |

What that implies for the owner's three slots (title, vision bridge, summariser), each pointing at an agent's model chain:

- **One slot per task, not one shared default.** A shared "small" model is fine as the default value of several slots; it must not be one key that moves them all. The evidence for this is Claude Code's trap and Hermes' separate chains.
- **Default each slot to the main or agent model** (Hermes, Zed), not to whatever happens to be authenticated (Oh My Pi). Under "the node decides", which models a node may call must be explicit, and the slot must resolve to a model the node holds a key for.
- **Place slots where the work runs.** Titles for chat can run on the master's chat runtime. The summariser for agentic compaction and the vision bridge for an agentic turn must run on the node, because the loop runs there. Pointing a slot at an agent's chain reuses our existing per-agent fallback machinery.
- **The summariser's window must be at least the main model's.** Hermes requires this and documents the silent failure when it is not; Claude Code refuses to fall back to a smaller-context model during compaction. Validate it when the slot is configured.
- **Treat slot output as untrusted model text,** especially the vision bridge's descriptions.
- **First settle which model runs history summarisation today.** The inventory does not say.

**What the field does with memory scopes.** Nobody documents all four of our planned scopes (workspace, project, user facts, per-repository). Claude Code's auto memory is per git repository, machine-local and shared across worktrees (https://code.claude.com/docs/en/memory.md); Claude chat has global plus per-project memory, where project chats see project and non-project memory but never another project's, a one-way containment rule (https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context); Warp attaches personal, agent and team stores read-only or read-write (https://docs.warp.dev/agents/agent-memory/, preview); Hermes profile distributions carry config and skills but never memory or credentials. Hard size caps force curation: Hermes 2,200 and 1,375 characters, Claude Code 200 lines or 25 KB, ours 500 characters per fact and the top 6 recalled.

Implications for our design:

- **A per-repository scope is only well defined on the node,** which can key it by git identity. A master-side store cannot, unless the node reports an identity. Today `remember` is not registered on agentic nodes, so nothing could write that scope.
- **Per-repository memory crosses users.** On a node shared by several master users, memory written from one user's chat would be read in another's. "Chats are private to their owner" means a per-repository fact derived from a private chat needs the same sharing step as a workspace fact. This is an inference from our invariants, not a field finding.
- **Copy Claude chat's one-way containment** between project scopes and **Warp's read-only or read-write attachment** per scope.

**Does agent-written memory need a human approval step?** The field mostly says no: Claude Code, Codex, OpenHands, Claude chat and OpenClaw write without one. The failure evidence comes from those designs: Cursor warns that untrusted input can poison automation memory and that its SDK journal is shared across all sessions and users (https://cursor.com/docs/agent-sdk/reference/memory, unreviewed); OpenClaw admits promoted memories have no time-based retention and deletion has gaps (https://docs.openclaw.ai/concepts/dreaming); OpenHands and Hermes treat injected memory as untrusted text. Nothing in the surveys measures whether users abandon an approval step in practice. The inference that follows: an approval step is clearly justified for the shared scopes (project, workspace, per-repository), where one poisoned fact reaches other people; for a user's own private facts it could be optional, as Hermes makes it. Our current design (owner approval for everything, manager approval for project facts on gate3, sharing off by default outside the Personal workspace on `feat/agent-roster`) is stricter than the field and on the right side of that line. Keep the gate on shared scopes; relaxing it for private facts is a product choice, not a safety one.

## 9. Limitations and research gaps

### What this round cannot tell us

**Documentation is not code.** Round 1 read public pages only, including for the open-source harnesses. Every mechanism in this report is "as documented". Where a vendor says a check is fail-closed, an approval is bound to argv, or a log has one writer, nobody has confirmed that the code does it. Docs also lag releases: Claude Code's weekly digest stops at Week 37 while the changelog runs on, OpenHands' release notes stop three versions behind GitHub, and several READMEs describe older behaviour than their docs (see "The pattern across files and reviews" in `round1/synth/_parts/overhyped.md`).

**Pages were mostly read through a summarising fetch tool.** Most surveys say so in their identity section (Antigravity, Cursor, goose, OpenClaw, OpenCode, OpenHands, Pi, Hermes Agent). The tool returns a model-written summary, not the page. Reviewers worked around it by asking for specific facts and quotes, but wording, defaults and numbers in unreviewed rows passed through a summariser once and were never compared with the raw page.

**Only about a quarter of entries were checked.** About 626 items were re-opened in review, against 2,393 entry headings (about 2,130 feature entries by the reviewers' own counts). The rest are `unreviewed`. Reviewers judged the unchecked remainder broadly reliable, with errors in small details (a default, a version, a cap) rather than invented features, but the check rate differs a lot per file: OpenCodex had 21 of about 33 entries checked, Claude Code 161 items against about 805 entries.

**"Nothing documented" claims are the least reliable evidence.** They mean a survey found no page. At least one was wrong: goose's "no sandbox documented" was not supported, since search snippets describe an opt-in macOS `GOOSE_SANDBOX` whose page now returns 404 (corrected in review, still unverified). Another hid a removal: OpenCode's session sharing was not undocumented but discontinued in v1.19.1. The goose reviewer asked for a second look at every "nothing documented" in that survey. Every count of the form "N of 16" in this report is a lower bound.

**Status labels were sometimes inferred.** Status was the most common correction: Claude Code agent view and background sessions from GA to research preview, OpenClaw cloud workers from GA to beta, Warp Factories to Early Access, OpenHands CLI from "deprecated" to "no longer actively maintained", Codex hooks from GA to no label. Where a page has no label, surveys sometimes wrote GA. Treat an unreviewed status label as a guess.

**Duplicate entries in the Claude Code survey disagree.** Produced by thirteen sub-agents, the file has several entries for one feature that differ: two dates for when project `defaultMode: bypassPermissions` started being ignored (v2.1.257 versus Week 36), an auto-mode classifier entry stale relative to a later one, and three conflicting idle-compaction entries. The synthesis merged them and took the reviewed values, but unreviewed duplicates may still carry the wrong one.

**Some summary sections were not updated after review.** Reviews corrected feature entries in place but often left the survey's own distinctive and relevance sections untouched. Known stale text: Claude Code's sections 3 and 5 still say each session gets a single-use work order (it is one runner per work order, with a 120-second spawn lease); Claude Desktop's section 5 still says the SSH helper survives the app quitting (the claim was removed); OpenClaw's section 5 still says cloud workers use an outbound-only connection (not stated by the page). The synthesis used the corrected versions; anyone reading the survey files directly should not.

**Changelog-derived dates and numbers are weak.** Version and date claims taken from changelogs were only partly reachable by reviewers (Claude Code's changelog and model-config pages were read to their first 100K characters only). Examples that did not hold: Claude Code idle compaction "about 54 minutes", Codex "WSL1 dropped at 0.115" and "GPT-6.1 Sol default in 0.161.0", the Codex presets 8,000-byte limit. Zed's checkpoint-removal dates were corrected in review.

**Closed products can only ever be described from their docs.** Claude Code, Claude Desktop, Cursor and Antigravity are closed; Warp's services and the Codex IDE extension, cloud and desktop are closed. Round 2's source reading cannot cover them. For Claude Code a few defaults can be printed by running the CLI (`claude auto-mode defaults` and similar).

### Documentation that was never opened

The important gaps, per harness, from the surveys' unverified sections and the review omissions:

| Harness | Not opened, truncated or unreachable |
|---|---|
| Codex | No tools reference page exists, so the full built-in tool list is unknown; notifications, computer history and Record and Replay pages; managed configuration seen as headings only; remote executors and `codex exec-server` (no page); the relay behind remote control is undescribed. |
| OpenCodex | The image and video bridge guides and the full provider matrix; the GitHub releases page returned 502, so only two release notes were read. |
| OpenCode | SCIM and workspace pages; V1 IDE and V1 Enterprise pages; the V2 config API page returned 404. |
| Claude Code | Most provider and gateway entries (Bedrock, Vertex, Foundry, spend limits, OpenTelemetry) were outside review 2's sample; the changelog and model-config page were read only in part; which model runs compaction and WebFetch extraction is not stated on the pages reached. |
| Claude Desktop | The last 25,000 characters of the main desktop page; memory import and export; MCP tunnels and MCP Apps detail pages (titles only); session sidebar, worktree defaults, PR monitoring and diff review were not checked. |
| DeepSeek harness | The last 25,000 characters of the generated tool catalogue (so default tool sets are unconfirmed) and the config catalogue (so compaction thresholds and the default sandbox mode are unknown). Several subsystem pages exist only on GitHub. |
| Oh My Pi | No docs pages for stats, SSH or `eval`; themes and key bindings not read; no changelog page in the sitemap. |
| Pi | No tools reference page located (edit matching rules unknown); `pi-chat` not opened; example extensions read by name only. |
| Cursor | SSO, SCIM, OpenTelemetry export and analytics API pages; audit logs and spend limits seen as index titles; Shared Transcripts, Origin, Voice Chat and Design Mode pages. |
| Antigravity | The pricing page and precedence tables; relay host and encryption for Remote Control and Remote Agents are not described. |
| OpenHands | Routing, fallback and subscription guides (index only); how approvals are presented in Canvas; whether the Enterprise server code is public and under what licence. |
| OpenClaw | The CHANGELOG; the managed-worktrees page was truncated (30K characters unread); sub-agent depth defaults subpage; embedding and Gateway-client pages. |
| goose | The sub-agents page returned a 391-byte stub; the sandbox page returns 404; the Developer extension's tool list and edit mechanism. |
| Hermes Agent | Remote config, multi-gateway, voice and webhook pages; the credential-pool page; the tail of the hooks page; the MCP config reference. |
| Zed/ACP | The hosted model list and prices; several ACP RFDs (usage, compaction, custom endpoint, proxy chains) seen as titles only. |
| Warp | The last parts of several `llms-txt` dumps; the ACP support claim (changelog wording only); worker and SDK licences. |
| Cline | SSO and member pages; the enterprise API reference; OpenTelemetry events and environment pages; what the hub dashboard shows; no browser tool page found. |

### Where our own "ours" statements are thin

- **Everything describes `main`.** The inventory was read at commit `a268048`. Pull request 4 and `feat/agent-roster` appear only in its "unmerged branches" section and in the synthesis as "branch only". This report states the branch position where it matters, but the synthesis files do not always.
- **Some synthesis files read only part of the inventory.** Context and memory (`03`) read inventory sections 4 and 5 plus keyword hits, so its "not in the inventory" means "not found in what was read". Configuration and licence (`10`) read sections 17 and 18 plus greps, and therefore reports our project licence as undefined; it is MIT. Planning (`04`), UI (`08`), models (`07`) and sessions (`05`) read named sections only. Permissions (`02`), extensibility (`06`) and remote (`09`) read the whole file.
- **Some synthesis text is stale against the branches.** `03-context-memory.md` calls audit item S12 (project members activating project facts) open; pull request 4 closes it by letting only a manager activate a shared project fact.
- **The inventory itself was not executed.** "Works" means code and tests exist, not that anything was observed working. It does not state which model runs history summarisation, and it does not measure checkpoint cost on a large repository.

### What is solid and what is not

Solid enough to act on now:

- **Execution-side authority is unique to us.** Every documented split hands policy to the control side; this rests on reviewed rows for Claude Code, Cursor, Warp and OpenClaw and on our own code.
- **Our table-stakes gaps are real.** No shell sandbox on `main`, no overflow-aware compaction, no provider retry, no steering, no web tools on agentic nodes: these come from our own code, and the field counts behind them are large enough that a few wrong "nothing documented" cells do not change the conclusion.
- **The field's own vendors say permission layers are not a security boundary.** These are reviewed quotes from Claude Code, Cursor, Oh My Pi, Cline, OpenClaw, OpenHands and Pi.
- **The credential pattern is moving to "workers never hold the model key".** Several independent, mostly reviewed rows (OpenClaw corrected to beta, Hermes corrected, Codex, Warp, Claude Code cloud).

Do not rely on these until Round 2:

- Any exact default, threshold or version from an unreviewed row, especially numbers from changelogs.
- Any "nothing documented" cell, and any count built from them.
- How strong a documented enforcement is in practice: OpenClaw's argv binding and its time-of-check gap, Codex's Linux sandbox profile and its network default, OpenCode's shell scanner, Hermes' hardline matcher, Codex's hook-trust hash. All are open source and settleable.
- Oh My Pi README claims (hashline benefits, round-robin credentials, typed results) and other vendor claims without a mechanism.
- Anything about Claude Code, Cursor or Antigravity that their docs leave unstated: wire protocols, VM isolation technology, classifier rules, relay encryption.

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

## 11. Features only one harness has

The full lists, per group of areas and with the reason each is or is not interesting, are in appendix A of [`harnesses-round-1-appendix.md`](harnesses-round-1-appendix.md). Section 6 above is the curated selection.

## 12. Corrections and claims that could not be verified

Appendix C of [`harnesses-round-1-appendix.md`](harnesses-round-1-appendix.md) lists, per group of areas, what the reviews changed, where a vendor's own pages contradict each other, and what nobody could confirm. Each survey file also ends with its own review table (`round1/<harness>.md`, section 9), and Claude Code's three reviews are in `round1/claude-code.review-1.md` to `-3.md`.

## 13. What surprised us and what looks overhyped

### Genuine surprises

Things that contradict a reasonable prior, including reversals.

- **Pi shipped MCP after arguing against it.** Its author argued against MCP in 2025 on context-bloat grounds; MCP was added in 0.99.0 as a built-in extension, behind code mode and deferral, and the homepage still lists it among deliberate non-features (https://pi.dev/docs/latest/mcp, https://pi.dev/).
- **Zed removed its checkpoints.** "Restore Checkpoint" was removed in 1.27.0 (2026-11-04) because per-prompt Git snapshots slowed or hung large repositories; the Agent Panel page is silent about it (https://zed.dev/releases/stable, corrected in review). A shipped feature withdrawn on cost is rare evidence.
- **OpenCode discontinued session sharing** in v1.19.1 (15 October 2026); the survey first recorded it as merely undocumented (https://opencode.ai/changelog/v1.19.1, corrected in review).
- **Claude Code withdrew its task tools on newer models.** TaskCreate and its siblings are default only on older models and were withdrawn for Opus 4.8, Sonnet 5, Fable 5 and newer in 2.1.233 (https://code.claude.com/docs/en/tools-reference.md).
- **Claude Code's sub-agent nesting went from 5 levels to off to 3 within six weeks,** while its digest still says 5; a 200 cap on total spawns was added in 2.1.212 and removed in 2.1.224 (https://code.claude.com/docs/en/sub-agents.md).
- **goose removed background sub-agents and its orchestrator** in v1.55.0, then added resume in v1.56.0 (https://goose-docs.ai/docs/guides/context-engineering/subagents.md).
- **Codex removed its MCP server,** and the only replacement, the App Server, is labelled "experimental and isn't supported for production workloads" while the stable Python SDK is built on it (https://learn.chatgpt.com/docs/mcp-server.md, https://learn.chatgpt.com/docs/app-server.md).
- **Codex rejects Landlock** for filesystem-restricted policies because Landlock cannot isolate Unix sockets, the same residual our gate3 sandbox documents (https://learn.chatgpt.com/docs/sandboxing.md).
- **Codex passes secrets to child processes by default:** `ignore_default_excludes` defaults to true, so `*KEY*`, `*SECRET*` and `*TOKEN*` variables pass through (https://learn.chatgpt.com/docs/config-file/config-advanced.md). No surveyed harness documents starting children from an empty environment, as ours does.
- **Codex's App Server accepts unauthenticated non-loopback connections by default "during rollout"** (https://learn.chatgpt.com/docs/app-server.md).
- **Claude Code's plan mode is not enforced in interactive terminals where bypass is available;** Claude is only instructed not to edit (https://code.claude.com/docs/en/permission-modes.md).
- **Claude Code's credential masking is partial.** It needs experimental TLS termination, covers sandboxed Bash only, is blocked outright by macOS for masked files, and leaves the real file readable when its pattern does not match; it is a different mechanism from the cloud secret proxy (https://code.claude.com/docs/en/sandboxing.md, corrected in review).
- **Claude Code mods fail open by default,** and the built-in guard loads only with managed settings or a Team or Enterprise login, so API-key users get none and a user mod can approve past deny rules (https://code.claude.com/docs/en/plugins/mods/overview.md, corrected in review).
- **Anthropic's restriction on third-party claude.ai login has a carve-out:** an end user may sign in to the unmodified `claude` binary with their own subscription, including where a platform hosts Claude Code (https://code.claude.com/docs/en/agent-sdk/overview.md, corrected in review).
- **Oh My Pi's default approval mode is `yolo`** (https://omp.sh/docs/approvals), and its auth broker hands clients API keys, not only access tokens, over plain HTTP on loopback (https://omp.sh/docs/secrets, corrected in review).
- **Hermes' container backends skip dangerous-command approval and disable checkpoints** (https://hermes-agent.nousresearch.com/docs/user-guide/security, corrected).
- **Hermes documents that a smaller summary model silently drops middle turns** rather than summarising them (https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching).
- **OpenClaw's dreaming is on by default with no time-based retention,** by the project's own admission (https://docs.openclaw.ai/concepts/dreaming).
- **Human approval of agent-written memory is rare.** Only Antigravity `/learn` and Hermes (opt-in, off by default) gate writes; Claude Code, Codex, OpenHands, Claude chat and OpenClaw do not (see "Who approves an agent memory write" in `round1/synth/03-context-memory.md`).
- **Cursor's self-hosted workers keep the loop in Cursor's cloud** and send file contents, terminal output, diffs and screenshots back (https://cursor.com/docs/cloud-agent/self-hosted). "Self-hosted" means self-hosted execution, not a self-hosted agent.
- **No surveyed split design lets the execution side outrank the control side.** Even Claude Code's self-hosted runner receives its tool list and permission rules from the control plane (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected).

### Claims bigger in marketing than in the documented mechanism

Including controls whose own vendor says they are not a security boundary.

- **Antigravity's "sandboxed" browser** drives the user's real Chrome profile with live sign-ins over the DevTools MCP (https://antigravity.google/docs/commands/browser).
- **Cursor's Auto-review is marketed as a default and documented as not a security boundary;** run-mode guardrails are "best-effort" (https://cursor.com/docs/agent/security, https://cursor.com/docs/agent/security/run-modes). Its classifier runs on Cursor's backend, so command context leaves the machine.
- **Claude Code's own limits:** Bash rules are "not a security boundary", the sandbox is "not a complete isolation boundary", auto mode "does not guarantee safety", server-managed settings are not a security boundary (https://code.claude.com/docs/en/permissions.md, https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/server-managed-settings.md).
- **Claude Code vendor claims without mechanism:** fast mode "up to 2.5x faster", Ultrareview's "every finding independently reproduced", enterprise cost averages, and a communications kit saying the CLI talks directly to Anthropic's API, which is false for gateway and cloud-provider deployments (see Claude Code in `round1/synth/_parts/overhyped.md`).
- **Oh My Pi's hashline benchmark is the maintainer's own,** titled for 15 models while the body lists 16, and the docs' editing pages do not describe hashline at all (https://stencil.so/blog/the-harness-problem, corrected in review).
- **Oh My Pi's TTSR "zero standing context cost"** is not stated on the TTSR page (https://omp.sh/docs/ttsr, corrected); its "time-traveling" is abort-and-retry with partial output discarded (see Oh My Pi in `round1/synth/_parts/overhyped.md`). Its approvals and sub-agent isolation are each documented as "not a sandbox" (https://omp.sh/docs/approvals, https://omp.sh/docs/subagents).
- **Codex's mobile and remote "secure relay layer"** has no technical description (https://learn.chatgpt.com/docs/remote-connections.md).
- **Cline's "every action requires your explicit approval"** sits beside a CLI that defaults to auto-approve, schedules that default to yolo and an SDK that default-allows (see Cline in `round1/synth/_parts/overhyped.md`). Its plan mode is not a security boundary, since `python -c` can still write (https://docs.cline.bot/features/subagents.md), and `.clineignore` is "not a security boundary" while its token savings are a vendor claim (https://docs.cline.bot/customization/clineignore.md).
- **OpenClaw's operator scopes are a guardrail within one trust domain, not tenant isolation;** `sessionKey` is a routing selector, not authentication (https://docs.openclaw.ai/gateway/security/trust-model). Its prompt-injection page cites attack-success rates by model with no primary source.
- **OpenHands is "open source" without a licence named on the comparison page,** has no authentication by default, and its Helm chart is "an unauthenticated, single-tenant application" (https://docs.openhands.dev/overview/faqs.md, corrected). Its security analyzers are "not a sandbox replacement" (https://docs.openhands.dev/sdk/guides/security.md).
- **OpenCode's admin policies "only tighten",** yet a policy allow can lift an earlier policy deny, and they are not a sandbox for plugin code (https://opencode.ai/docs/policies/, corrected). Its product page's "100% free" is a vendor claim.
- **goose's adversary-mode reviewer fails open** on errors (https://goose-docs.ai/docs/guides/security/adversary-mode.md).
- **ACP's permission requests are advisory,** and ACP does not sandbox agents (https://agentclientprotocol.com/protocol/v1/security.md).
- **Warp's redaction is "a backstop, not a substitute"** (https://docs.warp.dev/platform/architecture/), and "open" covers the client only; SOC 2, zero data retention and "no code stored" are vendor claims.
- **DeepSeek harness: 278k stars for a repository created on 2026-08-13** says nothing about maturity; the project's own `SAFETY.md` says it has had no security audit and must not be treated as production-ready (https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md).
- **OpenCodex's "40+ providers" and "zero client patches"** are vendor claims; its Remote Workspace is presented as a feature but labelled an experimental foundation (see OpenCodex in `round1/synth/_parts/overhyped.md`).

## 14. Recommended focus for Round 2

Round 2 reads the source code of the open-source harnesses. The ten synthesis files list well over a hundred questions that only code can settle (collected in `round1/synth/_parts/round2.md`). These twenty are the ones whose answers change a decision we face now. Each names the repository, the feature, what exactly to find out, and why it matters. The number in brackets is the overall rank.

Licence note: Zed (GPL and Apache files) and Warp (AGPL client) may be read for ideas only. Copying code from any repository needs a permissive licence and an update to `THIRD_PARTY_NOTICES.md`.

### Sandbox design

- **[1] Codex, Linux sandbox** (https://github.com/openai/codex). Find the bubblewrap argument list and seccomp filter per sandbox mode; whether network is actually off by default under `workspace-write` (not confirmed in review); and how the Unix-socket isolation that disqualified Landlock is done. Why: our gate3 Landlock sandbox cannot block pathname Unix sockets (Docker socket, D-Bus) before Linux 7.1 and does not cover UDP or DNS. This decides whether we add a seccomp or namespace layer for those gaps or accept them with a per-node warning.
- **[8] DeepSeek harness, sandbox enforcement reporting** (https://github.com/deepseek-ai/deepseek-harness). Find how `partial` enforcement is decided per Landlock ABI, and the default sandbox mode and preset at startup. Why: it is the model for reporting `full` or `partial` per node, which would make our socket gap visible to the operator.

### Approval binding

- **[2] OpenClaw, exec approval binding** (https://github.com/openclaw/openclaw). Find how the executable content hash and the resolved-path re-check are implemented, the time-of-check to time-of-use window between check and launch, and how `/approve <id>` from a chat channel is bound to the approver list and the exact command so a replayed approval cannot cover a different call. Why: this is the binding we intend to copy to replace "per tool name per turn".
- **[13] OpenCode, shell scanner and "always"** (https://github.com/anomalyco/opencode). Find which parser runs per shell, how `cd` targets are inferred, why unparseable input is refused, and how "always" derives the saved patterns from a command. Why: if we add per-command rules, this is the converged design to match.
- **[19] Codex, execpolicy** (https://github.com/openai/codex). Find what the tree-sitter splitter treats as "plain linear", how an opaque invocation is matched (exact string or argv), and how `match`/`not_match` tests run at load. Why: rules with inline tests are on the "worth copying" list.

### Credentials on workers

- **[3] Hermes Agent, iron-proxy integration** (https://github.com/NousResearch/hermes-agent). Find how opaque tokens map to real keys, what happens to Hermes' own model calls, and why container backends skip dangerous-command approval in code. Why: a node-local credential proxy is the one option that keeps keys off the agent's reach without moving authority to the master.
- **[7] OpenClaw, cloud workers** (https://github.com/openclaw/openclaw). Find the worker route's method allowlist, how inference is proxied by model reference, how owner-epoch fencing rejects a stale worker, and the actual transport and direction of the worker connection (the docs do not say outbound-only). Why: it is the reference for the other option, credentials on the control side, and for fencing a node that returns with stale state.

### The control and execution link

- **[4] OpenClaw, where node command policy is enforced** (https://github.com/openclaw/openclaw). Find whether the check runs on the Gateway before `node.invoke`, on the node on receipt, or both. Why: OpenClaw is the closest topology to ours; if its node also enforces, it is the only peer with any execution-side authority, and its code shows how that is tested.
- **[11] Codex, App Server** (https://github.com/openai/codex). Find how WebSocket authentication is enforced before `initialize`, what the non-loopback "unauthenticated during rollout" default is in code, and the queue sizes behind `-32001`. Why: we need an explicit overload reply on the bounded gate3 link and a clean auth order for any future remote client.
- **[20] OpenCode, durable session log** (https://github.com/anomalyco/opencode). Find the contract behind `GET /api/experimental/session/{id}/log`: exclusive sequence, ordering, retention, and how it relates to the volatile event stream. Why: the closest public analogue to our fold-over-log UI, and we have no log lifecycle yet.

### Checkpoints at scale

- **[6] Zed, the removed checkpoint code** (https://github.com/zed-industries/zed). Find which git operations it ran, how often, on what repository sizes, and what made it stall. Why: our shadow-repo checkpoints have a count cap only; this predicts the failure we will hit.
- **[12] Hermes Agent and Cline, shadow git** (https://github.com/NousResearch/hermes-agent, https://github.com/cline/cline). For Hermes: the destructive-command detection list, the one-per-directory-per-turn rule, how nested repos become gitlinks, how user edits since the snapshot are detected and skipped, and how the size caps are enforced. For Cline: location, exclusion rules, size handling and the cost of a commit per tool use. Why: the two other shadow-git designs; their limits are the ones we lack.

### Compaction

- **[5] Pi, compaction and `context_edit`** (https://github.com/earendil-works/pi). Find how cut points and split-turn summaries are chosen, how compact-and-retry on overflow is implemented, and how `context_edit` entries are created and folded. Why: Pi is the model for our overflow-aware compaction, and `context_edit` maps onto a fold over our event log.
- **[16] Hermes Agent, compression** (https://github.com/NousResearch/hermes-agent). Find the real behaviour when the summary model's window is smaller than the main model's, whether compression is in place or creates a child session (the docs disagree), and whether a manual compress command exists. Why: the failure mode a summariser slot must guard against.

### Sub-agent context and isolation

- **[10] OpenHands, approvals for ACP-hosted agents** (https://github.com/OpenHands/OpenHands). Find how permission requests from Claude Code or Codex running as ACP backends reach the Canvas user, and what is auto-approved. Why: no host documents this, and it is the part we would have to design before hosting other harnesses through `AgentBackend` without breaking "the node decides".
- **[17] DeepSeek harness, sub-agent inheritance** (https://github.com/deepseek-ai/deepseek-harness). Find whether a sub-agent inherits the parent's sandbox mode and approval policy (the docs are silent), and how the hidden `maxTotalAgents` cap is enforced. Why: our children inherit the approval mode; we need to know how others stop a child from widening it.

### Model slots

- **[15] Hermes Agent, auxiliary models** (https://github.com/NousResearch/hermes-agent). Find which model each auxiliary task uses when unset, and whether auxiliary fallback chains share cooldown state with the primary chain. Compare Zed's `thread_summary_model` and `compaction_model` defaults (https://github.com/zed-industries/zed). Why: settles "default to the main model" and "one chain per slot" for our three planned slots.

### Memory

- **[14] Hermes Agent, memory write approval** (https://github.com/NousResearch/hermes-agent). Find how `write_approval` staging, the frozen snapshot and the injection scan are enforced, and how the curator lock blocks agent writes, including writes from the terminal tool. Why: the only open implementation of a human gate on agent-written memory; it tells us whether the gate can be bypassed through the shell.

### MCP and tool deferral

- **[18] Pi, code mode and tool search defaults** (https://github.com/earendil-works/pi). Find whether `builtin:codemode` and `builtin:tool-search` load by default in the CLI (three docs pages disagree), and which resources project trust gates in headless mode. Why: Pi's exposure modes are the systematic answer to MCP context cost, and its trust gate is the model for repo-shipped servers.

### Hooks

- **[9] Codex, hook trust by hash** (https://github.com/openai/codex). Find what exactly is hashed (command string, script bytes, plugin directory), where the trust record lives, and whether a plugin update re-triggers review. Why: node-side hooks trusted by hash are the shape we would adopt; the details decide whether a repository can swap a script under a reviewed hook.

### Closed: documentation only

These cannot be covered by reading source code. Round 2 can only re-read their docs or, where a CLI prints its own defaults, run it.

| Harness or part | Status |
|---|---|
| Claude Code | Closed; the public repository holds the changelog and plugins only. Some defaults (the read-only Bash allowlist, the environment scrub list, auto-mode rules) can be printed by running the CLI, for example `claude auto-mode defaults`. |
| Claude Agent SDK, TypeScript | "All rights reserved". The Python SDK is MIT but drives the closed binary. |
| Claude Desktop | Closed; no repository. |
| Cursor | Closed; the public repository has no licence and no code. |
| Antigravity | Closed core; the CLI repository is distribution only; the Python SDK is Apache-2.0 but depends on a compiled runtime binary. |
| Codex IDE extension, Codex Cloud, ChatGPT desktop app | Closed; the CLI, SDK and App Server are open. |
| Warp control plane, cloud agents, Factories backend | Not described as open anywhere; the client is AGPL (ideas only); worker and SDK licences were not checked. |
| OpenHands Cloud and Enterprise | Commercial; whether the Enterprise server code is public was not found. |

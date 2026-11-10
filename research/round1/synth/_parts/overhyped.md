Every item from section 6 of all 17 survey files, grouped by harness, followed by the pattern the reviews found. Items a review changed are marked **[review]**.

##### Codex
- `project_doc_max_bytes` is described two ways: combined size (AGENTS.md page) vs per file (advanced config page). https://learn.chatgpt.com/docs/agent-configuration/agents-md.md vs https://learn.chatgpt.com/docs/config-file/config-advanced.md
- `codex agents --no-daemon`: CLI reference says unsupported, 0.167.0 notes (2026-11-06) tell users to use it; the reference may lag. https://learn.chatgpt.com/docs/developer-commands.md vs https://learn.chatgpt.com/docs/changelog
- App-server is "experimental and isn't supported for production workloads" yet is the only replacement for the removed MCP server and under the stable Python SDK. https://learn.chatgpt.com/docs/mcp-server.md, https://learn.chatgpt.com/docs/codex-sdk.md
- GitLab review is "preview" on one page and "beta" on another. https://learn.chatgpt.com/docs/code-review.md, https://learn.chatgpt.com/docs/third-party/gitlab.md
- Code Mode is "under development and off by default" in the config reference while releases keep shipping user-facing Code Mode features. https://learn.chatgpt.com/docs/config-file/config-reference.md vs https://learn.chatgpt.com/docs/changelog
- The `untrusted` migration text contradicts itself (retired, yet managed `allowed_approval_policies` "must include `untrusted`"). https://learn.chatgpt.com/docs/agent-approvals-security.md
- Broken in-page anchors to `cloud#...` sections that do not exist in the Markdown (may render only in HTML; not checked). https://learn.chatgpt.com/docs/cloud.md
- Speed page still cites GPT-5.6 beside GPT-6 models (looks stale). https://learn.chatgpt.com/docs/agent-configuration/speed.md
- Docs index titled "Codex" but most pages are ChatGPT, Work, dots or Space. https://learn.chatgpt.com/docs/llms.txt
- README links to `developers.openai.com/codex`, now redirecting to the unified docs. https://raw.githubusercontent.com/openai/codex/main/README.md
- Marketing-level claim: the mobile/remote "secure relay layer" has no technical description. https://learn.chatgpt.com/docs/remote-connections.md
- Memories: "off by default" next to "Enabled memory v2 by default for fresh profiles" (refers to store version, not enablement).

##### OpenCodex
- "40+ providers", "zero client patches", sub-agent demos are vendor claims; the client page says no client has runtime-verified results **[review]**: the page does not make a blanket statement.
- README calls the desktop app beta; the desktop docs page does not.
- README lists Docker Compose; the remote-hub page says no official image is published.
- Remote Workspace is presented as a feature but is an "experimental foundation".
- 502 errors blocked the GitHub release list; only two release notes seen.

##### OpenCode
- Version confusion: docs say V2 current; GitHub "latest" flags v1.19.5; a Korean post (search snippet only) says "2.0" referred to Desktop v2. https://opencode.ai/changelog
- README (https://github.com/anomalyco/opencode) lists npm `@opencode/cli`, describes plan as asking before bash, calls desktop beta and has no client/server note: likely lags V2.
- The API page calls the whole surface experimental while SDK pages present it as the supported contract.
- Config page omits `experimental.schedules`/`experimental.artifacts` that the changelog introduces; `/goal` is in the changelog without a docs page.
- V2 docs omit sharing and GitHub integration; V1 pages stay live; sharing was discontinued in v1.19.1 **[review]**, https://opencode.ai/en/docs/share seen in a search result only.
- Product page: "One agent, every surface", "100% free to use", a "Degraded performance" banner: marketing. https://opencode.ai/product
- Console page does not map the three prices to plan names; the Go page does.

##### Claude Code
- Docs lag the changelog: weekly digest stops at Week 37 (2026-09-11), Week 38 returns 404; many 5.5 models, Mods, idle compaction, AGENTS.md support have no digest; `fast-mode.md` still says unavailable on Google Cloud a day after 2.1.322 added it on Vertex; status line JSON sample shows version 2.1.90; mods reference says "as of" 2.1.290/2.1.299 while citing 2.1.321; settings JSON schema lags; anchors in `server-managed-settings.md` renamed.
- Default-model advice contradicts the defaults: `how-claude-code-works.md` and `costs.md` say Sonnet handles most work, every plan defaults to Opus 5.5; `costs.md` thinking budgets that `model-config.md` says current models ignore. https://code.claude.com/docs/en/model-config.md
- Churn: subagent nesting went 5 levels, off, 3 within six weeks (digest says 5); `keybindingFlavor` lived 23 days, `taskOutputMaxChars` about 2 weeks; output styles deprecated then un-deprecated after 5 days; `/simplify` renamed `/code-review` and now names a different command; `/fork` meant three things.
- Status labels disagree: fullscreen "research preview" yet default since 2026-05-06; `claude agents` research preview on one page; plugin evals, monitors and themes presented as available but under `experimental.*` manifest keys; auto mode "available to all users on every provider" vs model- and provider-gated. **[review]** Review 2 confirmed agent view is research preview (the survey's "GA" entries were wrong).
- Pages disagree on mechanism: spend limits ("not a shared pool" vs `pooled_budget`); cross-session messaging across machines vs same machine; MCP startup timeout 30 s vs 5 s; MCP "Clear authentication" revoking upstream tokens; `self-hosted-environments-deploy` recommends `--capacity 1` but recipes use 4.
- Vendor claims without mechanism or numbers: fast mode "up to 2.5x faster"; Ultrareview "every finding independently reproduced" at $5-25; enterprise cost averages; GitLab "sandboxed execution" with no sandbox in the examples; computer use "flags prompt injection"; the communications kit says the CLI talks directly to Anthropic's API, false for gateway and cloud-provider deployments.
- The vendor admits limits: Bash rules "not a security boundary", sandbox "not a complete isolation boundary", auto mode "does not guarantee safety", policy mods bypassable by `--safe-mode` and worker crashes, server-managed settings not a security boundary.

##### Claude Desktop
- Where Cowork runs: getting-started article says cloud; Linux and 3P pages say a local VM. **[review]** Both modes exist; the getting-started article is stale or incomplete.
- Cowork scheduled tasks "run in the cloud" vs Desktop local tasks needing the app open. **[review]** Different products, not a contradiction; `/schedule` now fills in a scheduling request.
- Dispatch: Pro/Max/Ultra feature in Code docs, "limited beta, not for new users" in the support article.
- Cowork "generally available" Apr 9 2026 vs Sep 16 2026 gradual rollout; product page still says "being folded into Claude".
- Linux page lists Cowork in the beta; download page does not list Android; docs index says no dedicated artifacts, memory or research pages.
- Productivity claims on the Cowork page come from testimonials (vendor-claim).
- Changelog version numbers look non-monotonic (v4.29.0 then v4.4212.0).

##### DeepSeek harness
- `00-targets.md` said no GitHub releases; the API shows prereleases up to `dsh-v0.2.2-rc.1` **[review]** confirmed.
- Landing page marks agent teams, auto review, scheduled tasks and voice experimental; the docs site documents schedules without that label.
- Landing statements ("out of the box", "explore the upper limit of intelligence", Pomodoro plugin in 5 min 24 s) are marketing.
- The subsystems index links `ssh`, `mcp`, `agent-team`, `webhook` GitHub pages that the docs-site mirror lacks (404; content exists on GitHub) **[review]**.
- The tool catalogue lists alternate or opt-in tools without saying which are mounted by default.
- 278k stars for a repo created 2026-08-13 says nothing about maturity.
- npm first release listed BSD-3-Clause, later MIT; not traced.
- SDK page says the SDK profile pins `danger-full-access`; the web quickstart asks for approval (different profiles, easy to confuse).

##### Oh My Pi
- README tool count (31) and coreutil counts (46/58/67) disagree internally; README nine roles vs docs ten (`designer`).
- README says `skill://` exists; skills doc documents none. README lists `chillax`; magic-keywords page does not.
- docs/memory says no named memory tools; docs/tools and README list five.
- README "schema-validated typed results" for task vs subagents doc "no formal results schema".
- README claims round-robin credentials with session affinity; the providers doc does not describe it.
- Hashline benchmark numbers are the author's own; docs/editing does not mention hashline.
- "Time-traveling" is marketing: abort-and-retry with partial output discarded by default.
- Default approval mode is `yolo`; "IDE wired in" and 60+ providers are vendor claims.
- SDK requires Bun 1.3.14+ vs README install Bun >=1.4.4.
- docs/tools says `ast_grep` is off but README lists it as a headline tool.
- No docs page for stats, SSH, eval; themes and keybindings not read; sitemap has no changelog.

##### Pi
- Codemode default: CLI page says off, settings page says `on` and lists it loaded by default, MCP page says MCP tools default to `codemode`; needs a run to settle. https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/settings, https://pi.dev/docs/latest/mcp, https://pi.dev/docs/latest/codemode
- Homepage lists MCP among deliberate non-features while noting it is built in. https://pi.dev/
- npm scope: April 2026 post says `@earendil/pi`; README and docs use `@earendil-works/pi-coding-agent`.
- Homepage "15+ providers" vs a longer providers page.
- Non-feature rationale is from a 2025 post when Pi was a personal project; MCP was argued against there and later added.
- how-pi-works and quickstart pages do not mention the omitted features.
- pi-chat shows two licences on one page. https://github.com/earendil-works/pi-chat
- README package table omits `pi-env`, `codemode`, `mcp`, `schema`, `evals` that exist as directories.
- The 2025 "under 1,000 tokens" prompt claim is not repeated and may no longer hold (unverified).
- Terminal-Bench 2.0 benchmark claims are the author's own runs.

##### Cursor
- Worker limits: 200 per user and 1,000 per team (pool page) vs 10,000 each (self-hosted page).
- Cloud data retention: privacy page says encrypted copies only while running; cloud security page says VM snapshots kept 30 days and not deletable on demand.
- Memories: silent in agent overview and rules pages; removal documented only on forum posts that disagree (UI vs feature removed).
- Mobile: cloud-agent page suggests Android via PWA; mobile page says Android planned.
- GitHub integration page omits `@cursor` mentions that the cloud agent page lists.
- "Cursor never connects into your network" and "meets the security requirements of most teams" are vendor claims.
- Auto-review is "not a security boundary" but marketed as a default.
- BYOK and Max Mode pages are brief; many status labels absent.

##### Antigravity
- "Sandboxed" browser: docs say the subagent uses the real Chrome profile with live sign-ins.
- Hooks: plugins page says shell commands or prompt evaluations; hooks page documents only `command`.
- Effort: `max` in the CLI reference vs low/medium/high elsewhere.
- Plans page says no BYOK/custom endpoints, yet CLI settings list `modelProvider`/`customModelsConfig`/`GEMINI_API_KEY` and the SDK supports Gemini keys, Vertex and local servers.
- Slash command catalog calls itself complete but omits commands; `/context` page does not mention `/compact`; artifact review page mentions "Fast mode" though `/fast` was removed in v1.1.0.
- Overview says four surfaces, llms.txt five; migration page links to a missing settings page.
- "Co-trained", "highly optimized", "securely" are vendor claims.
- Auto mode "Preview" in v2.29.0 and earlier; sandbox "Preview" for business accounts **[review]**: the label could not be found.
- Enterprise "never used for training" is a vendor claim; the enterprise page defers to linked Cloud docs.

##### OpenHands
- Docs release notes end at 1.35.0 while GitHub is at 1.38.0.
- Key-features page describes the legacy GUI; the introduction page says GUI and CLI are deprecated/unmaintained.
- Enterprise lifecycle page contradicts itself on idle timeout leading to PAUSED vs MISSING.
- SDK security docs disagree on whether the LLM analyzer uses a separate LLM, and on the default confirmation policy.
- Condenser guide says no defaults documented; architecture page gives 120/4.
- Workspace architecture page lists five workspace types; guide index lists Apptainer and Cloud too.
- The enterprise-vs-oss page says "open source" without naming the licence; the FAQ says single local user while Canvas pages describe team VMs.
- V1 API pause/resume paths look truncated.
- Automation pages describe Cloud behaviour (fresh sandbox per run) that does not apply to a local backend.
- Slide-style claims ("composable", use-case pages) are vendor claims.

##### OpenClaw
- Home page and features list still say "heartbeat scheduling" while the heartbeat page says the monitor is retired.
- Workspace page lists `TOOLS.md`/`HEARTBEAT.md` in a summary but not in its file map.
- Tool Search is under "Experimental features" yet on by default and a built-in tool.
- Maturity scorecard shows Windows App/Node as M3 Beta with 19% quality, which the page's bands call Experimental.
- Why-OpenClaw is a self-written comparison with Hermes Agent (`vendor-claim`); its "single-process design relying on the OS" for Hermes is unverified.
- `telemetry off` and `DO_NOT_TRACK` still leave the daily check on.
- Docker page does not state the default bind; the security page says containers default to an exposed bind.
- The prompt-injection page's attack-success test by model (0.5% to 8.5%) has no primary source.
- Dreaming is default-on yet "promoted memories have no time-based retention limit".
- Docs move fast (anchors redirected, schema bumps; heartbeat migration needs schema 30).

##### goose
- README says 15+ providers; the providers page lists many more.
- Docs name the server `goosed` in places while the remote-server guide and CLI use `goose serve`; v1.53.0 notes a lean ACP-only binary.
- Multi-model page promises "dynamic, context-aware switching" but documents only `/model` and subagent models.
- In-session actions page says the CLI does not support model switching as a setting; the CLI reference lists `/model`.
- ACP providers page mentions a `copilot` binary not in its supported list.
- Prompt injection page does not state the default; config pages say off.
- Subagents are disabled outside autonomous mode, so delegation conflicts with approvals.
- Hook page: `SubagentStart/Stop` listed but "not emitted".
- Context relevance operation and context report (v1.58.0) have no doc page found.
- Release notes mention newer models (Opus 5.5, GPT-6) read only as release-note text.
- **[review]** The survey's "nothing documented" for sandboxing was wrong in part (a macOS desktop `GOOSE_SANDBOX` page appears in search results; the page 404s on fetch).

##### Hermes Agent
- README says session search uses LLM summarisation; memory and sessions pages say raw excerpts, no LLM.
- Architecture diagram says seven terminal backends, prose six, tools page eight including plugins.
- Delegation `max_iterations`: 250 (delegation page) vs 50 (agent-loop page).
- Compression lineage: in-place (compression page) vs "creates a child session" (sessions page).
- Tool Search called "opt-in" yet default `auto` behaves as on.
- Prompt-assembly page places the skills index in both the volatile and stable tiers.
- Docker network recipe mixes `dashboard` and `hermes-dashboard` names.
- "Skills improve during use" and MoA benchmark numbers are vendor claims.
- Release notes show hundreds of merged PRs per patch; the docs describe a very wide surface with many "listed" pages; some features may be thin.
- Termux and several packaging paths flagged prerelease or unsupported.

##### Zed/ACP
- Remote development page says the local machine talks to LLMs and also that the remote uses its own proxy.
- Instructions page lists nine project files, then says the Zed Agent loads `AGENTS.md`, `CLAUDE.md`, `copilot-instructions.md` **[review]**: first match wins in the listed order and `.rules` is still supported.
- Docs say sandboxing without any maturity label; release notes mention it only as a bug fix.
- Agent Panel docs say "no checkpoints" but 1.25.2 and 1.23.3 release notes still refer to them; 1.27.0 removed them.
- Tool-permissions page does not say how it applies to external agents.
- ACP home page says remote support is in progress; the agents page lists 51 agents, several third-party or adapter-based.
- ACP v2 overview page does not mark what is new versus v1.

##### Warp
- Naming churn: "Oz" agent renamed "Warp Agent" (2026.08.13) while Oz CLI, Oz web app and `oz-*` repos keep the name; CLI docs call `oz` "legacy" yet it is the documented CI route and the transitioning page says "current". **[review]** confirmed.
- SOC 2, ZDR and "no code stored" are vendor claims.
- Codebase Context page says no code is stored on Warp servers but does not say where embeddings are computed.
- "Open" covers the client only; the cloud platform is not described as open anywhere.
- Orchestration page does not label beta while Factories is Early Access **[review]** (status-wrong: relabelled from preview); parts of the CLI doc truncated.
- Admin Panel page says only the team creator is admin; enterprise pages describe Owner, Admin and Member roles.

##### Cline
- Overview says "Every action requires your explicit approval", but CLI defaults to auto-approve true, schedules default to yolo, SDK default-allows.
- YOLO enterprise control: page says all interfaces; remote-config matrix says no surface supports it.
- Plan mode "cannot run commands" vs subagent/scheduling pages saying it only blocks mutating commands and not `python -c` writes.
- Flag naming: `--auto-approve` vs `--yolo`; Plan/Act pages still mention todo lists though Focus Chain is deprecated.
- Checkpoint restore labels differ between the checkpoints page and the IDE page.
- Kanban overview mentions auto-commit/auto-PR; core-workflow says auto-PR not described; Kanban remote access has no auth.
- `.clineignore` "200k+ to under 50k tokens" is a vendor claim; not a security boundary and being deprecated.
- ClinePass "2-5x usage" is a vendor claim; no absolute limits.
- Hooks docs page is a stub; docs reference old tool names; Auto Compact and Memory Bank pages look legacy.
- Release page shows no year; Cloud sessions "rolling out gradually".

##### The pattern across files and reviews

Taken from the surveys' section 6 lists and the review tables:
1. **Docs lag releases and READMEs lag docs.** Seen for Codex (reference vs release notes), Claude Code (digest stops at Week 37, fast-mode page a day stale), OpenCode (README lists V1 package and behaviour), OpenHands (docs notes 1.35.0 vs 1.38.0), OpenCodex (README vs pages), OMP (README vs docs counts), Hermes (README says session search uses an LLM), Pi (homepage "15+ providers"), goose (README "15+ providers").
2. **Maturity and status labels are the most common error in the surveys themselves and in vendor pages.** Review corrections were mostly status labels: Claude Code agent view GA→research preview, OpenClaw cloud workers GA→beta, Warp Factories preview→Early Access, OpenHands CLI "deprecated"→"no longer actively maintained". Vendors disagree with themselves: Codex GitLab preview/beta and App-server "experimental" under a stable SDK, OpenClaw Tool Search experimental but on by default, Claude Code fullscreen research preview but default, Antigravity auto mode Preview.
3. **Defaults contradict across pages.** Pi codemode, Cline auto-approve versus "every action requires approval", OMP default `yolo`, goose prompt-injection default, Hermes delegation `max_iterations` 250 vs 50, Claude Code default-model advice, OpenHands default confirmation policy.
4. **Absence claims ("nothing documented") were occasionally wrong.** goose sandboxing; OpenCode "sharing undocumented" was actually discontinued; OpenClaw's hardened-baselines page 404 once. This is why the matrix treats `-` as weak.
5. **Review corrections to the surveys' own synthesis sections were more frequent than to feature entries.** In section 3 and relevance text: Claude Code's "each session gets a single-use work order" and grouping Remote Control under leases; the credential-masking summary missing its experimental dependency; OMP TTSR "zero standing context cost" not on the page; OpenClaw cloud workers "outbound-only"; goose ACP users; Cursor hooks "most restrictive wins"; Zed checkpoint "page note" that did not exist. Reviewers said the same of feature entries: errors were small (a default, a version, a cap), not invented features.
6. **Numeric and date claims from changelogs are the weakest category** (Claude Code idle compaction "about 54 minutes", 2.1.x dates; Codex 0.115/0.161.0; OpenCodex release-feed claims; Zed dates and 1.27.0).
7. **Vendor claims without mechanism cluster in marketing text:** speed-ups, "securely", "co-trained", SOC 2/ZDR, `.clineignore` token savings, ClinePass multipliers, benchmarks run by the author (Hashline, Terminal-Bench), star counts as maturity, "zero client patches", "40+ providers".
8. **Cross-page conflicts that reviewers resolved as different products or modes, not errors:** Cowork cloud vs local VM, Cowork cloud tasks vs Desktop local tasks, DSH SDK profile `danger-full-access` vs web profile asking for approval.
9. **Naming and renaming churn:** Warp Oz, Claude Code subagent depth and `/fork`, goose `goosed`, OpenClaw heartbeat, OpenCode V1/V2 and Zen/Console, Pi npm scope.

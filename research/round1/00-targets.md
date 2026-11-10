# Round 1, step 0: targets and primary sources

Research date: **2026-11-10**. Scope: identify each harness and where its primary sources live. No feature survey here.

## How to read this file

- **Repository figures** (stars, licence, default branch, releases, last commit) were read from the GitHub REST API on 2026-11-10 (`repos/<owner>/<name>`, `/releases`, `/commits`, `/license`). The cited repository URL is the source. Stars are rounded.
- **"Opened"** means the URL was fetched on 2026-11-10, returned HTTP 200 and its title or first heading matched. A subset was also read in detail; identity statements in this file come only from pages that were read. Every URL listed below was opened unless marked **(not opened)**.
- **Trap for later agents:** GitHub's "latest release" flag is wrong for several repositories here (OpenCode, Qwen Code, Warp). Read the releases list, not `/releases/latest`.
- **Assumptions I had to make:** (1) "OpenCodex" most plausibly means `lidge-jun/opencodex`; this is a judgement, not a fact, see that section. (2) "Codex" is surveyed as the whole OpenAI Codex family, including the parts now folded into the ChatGPT desktop app. (3) Closed products have no version tag to cite; I give the newest changelog entry and its date instead.
- **Hypotheses from the brief that were wrong or stale:**
  - "DeepSeek harness" is not a community project. DeepSeek ships an official open-source product literally named *DeepSeek Harness*.
  - Pi is no longer at `badlogic/pi-mono`; it moved to `earendil-works/pi` and is owned by Earendil Inc.
  - Codex documentation no longer lives at `developers.openai.com/codex`; it redirects into the unified ChatGPT docs, and the desktop surface is now the "ChatGPT desktop app".
  - Cursor is no longer an independent company (Anysphere); it was acquired by SpaceX.
  - Three candidates on the additions list are dead or frozen: Roo Code (archived), Continue (read-only), Aider (stalled). Gemini CLI is superseded by Antigravity CLI for consumers. Windsurf is now "Devin Desktop".

---

## 1. Codex (OpenAI)

- **Canonical name / vendor:** Codex, by OpenAI. A coding agent family: an open-source terminal agent plus closed IDE, desktop, cloud and mobile surfaces (https://learn.chatgpt.com/docs/open-source).
- **Surfaces today:**
  - Codex CLI, with non-interactive `codex exec` (https://learn.chatgpt.com/docs/codex/cli, https://learn.chatgpt.com/docs/non-interactive-mode).
  - Codex IDE extension (https://learn.chatgpt.com/docs/codex/ide).
  - ChatGPT desktop app for macOS, Windows and Linux, where the user chooses Chat, ChatGPT Work or Codex (https://learn.chatgpt.com/docs/app, https://learn.chatgpt.com/docs/use-chatgpt). This is the former standalone Codex app surface; the docs still say "Codex desktop app" in places (https://learn.chatgpt.com/docs/llms.txt, Codex Security workbench entry).
  - Codex Cloud, with cloud environments (https://learn.chatgpt.com/docs/cloud).
  - Codex on mobile, driving a connected computer; SSH remote connections from the desktop app (https://learn.chatgpt.com/docs/mobile, https://learn.chatgpt.com/docs/remote-connections).
  - Codex SDK and Codex App Server protocol for embedding (https://learn.chatgpt.com/docs/codex-sdk, https://learn.chatgpt.com/docs/app-server).
  - GitHub Action, PR review for GitHub and GitLab, Slack, Linear (https://learn.chatgpt.com/docs/github-action, https://learn.chatgpt.com/docs/llms.txt).
  - Codex Security (CLI, SDK, cloud, plugin) (https://learn.chatgpt.com/docs/llms.txt).
- **Open or closed:** mixed. Open: CLI, SDK, App Server (all in `openai/codex`), Codex Security CLI and SDK, skills, plugins, the universal cloud image. Closed: IDE extension and Codex cloud are stated as "not open source"; the desktop app is not listed among the open components (https://learn.chatgpt.com/docs/open-source).
- **Repository:** https://github.com/openai/codex. Rust (about 97% by bytes), Apache-2.0 (`LICENSE`), default branch `main`, about 134.7k stars, active. Latest stable release `rust-v0.167.0`, published 2026-11-06; newest prerelease `rust-v0.169.0-alpha.7`, 2026-11-09; last commit seen 2026-11-10 (https://github.com/openai/codex/releases). The repository does not accept external pull requests (https://learn.chatgpt.com/docs/open-source). Related: https://github.com/openai/codex-action (Apache-2.0), https://github.com/openai/codex-security (Apache-2.0, release `npm-v0.9.1`, 2026-11-10).
- **Primary sources (all opened):**
  - Docs root: https://learn.chatgpt.com/docs — machine index: https://learn.chatgpt.com/docs/llms.txt (every page has a `.md` twin)
  - CLI: https://learn.chatgpt.com/docs/codex/cli — IDE: https://learn.chatgpt.com/docs/codex/ide — desktop app: https://learn.chatgpt.com/docs/app
  - Sandbox: https://learn.chatgpt.com/docs/sandboxing — approvals and security: https://learn.chatgpt.com/docs/agent-approvals-security
  - Permission modes: https://learn.chatgpt.com/docs/permission-modes — permission profiles (beta): https://learn.chatgpt.com/docs/permissions — auto-review: https://learn.chatgpt.com/docs/sandboxing/auto-review — rules: https://learn.chatgpt.com/docs/agent-configuration/rules
  - AGENTS.md: https://learn.chatgpt.com/docs/agent-configuration/agents-md — memories: https://learn.chatgpt.com/docs/customization/memories
  - Subagents: https://learn.chatgpt.com/docs/agent-configuration/subagents
  - MCP: https://learn.chatgpt.com/docs/extend/mcp — skills and plugins: https://learn.chatgpt.com/docs/skills-and-plugins — hooks: https://learn.chatgpt.com/docs/hooks
  - Config reference: https://learn.chatgpt.com/docs/config-file/config-reference — models: https://learn.chatgpt.com/docs/models
  - Headless/CI: https://learn.chatgpt.com/docs/non-interactive-mode — GitHub Action: https://learn.chatgpt.com/docs/github-action
  - SDK: https://learn.chatgpt.com/docs/codex-sdk — App Server: https://learn.chatgpt.com/docs/app-server
  - Cloud: https://learn.chatgpt.com/docs/cloud — where a chat runs: https://learn.chatgpt.com/docs/environments/modes — remote: https://learn.chatgpt.com/docs/remote-connections — mobile: https://learn.chatgpt.com/docs/mobile
  - Changelog: https://learn.chatgpt.com/docs/changelog — weekly digest: https://learn.chatgpt.com/docs/whats-new — open-source map: https://learn.chatgpt.com/docs/open-source
  - README and releases: https://github.com/openai/codex , https://github.com/openai/codex/releases — product page: https://chatgpt.com/codex
  - https://openai.com/codex/ **(not opened: HTTP 403 to a script)**
- **Ambiguities:** `developers.openai.com/codex/*` now redirects to `learn.chatgpt.com/docs/*` (observed redirects for `/codex`, `/codex/cli`, `/codex/changelog`). Docs mix ChatGPT, "ChatGPT Work", "dots" and Codex; a survey agent must filter for Codex. The Codex MCP server was removed in favour of the app server (https://learn.chatgpt.com/docs/llms.txt, "Codex MCP server removal"). "Codex Cloud (Legacy)" pages coexist with the new cloud environments pages (same index).
- **Confidence:** high. Repository, docs and open/closed split all come from OpenAI's own pages.

---

## 2. OpenCodex

**No project of this name is a coding harness in the sense of the other targets.** Candidates found (GitHub repository search, 2026-11-10):

| Candidate | What it is | State |
|---|---|---|
| https://github.com/lidge-jun/opencodex | Local gateway/proxy that lets Codex, Claude Code, Claude Desktop, Grok Build and other clients run on any model provider; has a web dashboard on `localhost:10100` (README) | TypeScript, MIT, default branch `dev`, about 18.4k stars, created 2026-06-18, release `v2.100.1` on 2026-11-10, last commit 2026-11-10. Very active. |
| https://github.com/RyensX/OpenCodex | Middleware for the Codex/ChatGPT desktop app: operate Codex on a target machine from a phone or another computer through a browser (repository description) | JavaScript, AGPL-3.0, about 730 stars, release `2.1.0` and last commit 2026-09-13. Quiet for two months. |
| https://github.com/ymichael/open-codex | Fork of the old TypeScript Codex CLI adding other providers (fork of `openai/codex`) | Apache-2.0, about 2.4k stars, last commit 2025-05-03. Dead. |
| https://github.com/matonhp5108/Opencodex | "A free, open-source coding agent inside VS Code" (description) | MIT, 54 stars, last commit 2026-08-29. Negligible. |

- **Most plausible target:** `lidge-jun/opencodex`. Reasons: it is the only one with real adoption and daily releases, its name is used unqualified by other projects (for example `feiyulc/opencodex-account-importer`, seen in the same search), and the owner listed it next to Codex and OpenCode, the two clients it fronts.
- **What that means for the survey:** it is a companion, not an agent loop. It does not run tools. What is worth studying is its provider translation layer, web dashboard, "remote hub" deployment and sub-agent surface.
- **Surfaces:** CLI `ocx`, local proxy, web dashboard, background service (https://github.com/lidge-jun/opencodex README). npm package `@bitkyc08/opencodex` (README).
- **Primary sources (all opened):** https://opencodex.me/ , https://opencodex.me/getting-started/how-it-works/ , https://opencodex.me/reference/architecture/ , https://opencodex.me/guides/web-dashboard/ , https://opencodex.me/guides/remote-hub/ , https://opencodex.me/guides/sub-agent-surface/ , https://opencodex.me/reference/client-support/ , https://opencodex.me/reference/configuration/ , https://github.com/lidge-jun/opencodex , https://github.com/lidge-jun/opencodex/releases . Further pages are listed in https://opencodex.me/sitemap-0.xml (opened), including `/guides/codex-integration/`, `/guides/claude-code/`, `/guides/model-routing/`, `/guides/remote-workspace/`, `/reference/cli/` **(those five not opened)**.
- **Confidence:** medium-low that this is what the owner meant; high that the facts about each candidate are right. **The owner should confirm.** If the owner meant "remote web access to Codex", the target is `RyensX/OpenCodex` instead.

---

## 3. OpenCode

- **Canonical name / maintainer:** OpenCode, maintained in the `anomalyco` GitHub organisation (formerly `sst`; the company name behind it was not verified). "The open source coding agent" (https://github.com/anomalyco/opencode).
- **Surfaces today:** terminal UI and CLI; desktop app (beta) for macOS, Windows and Linux; mobile; SDK; extensions (https://opencode.ai/product, README). Web mode and ACP mode from the CLI (https://opencode.ai/docs/cli/web/, https://opencode.ai/docs/cli/acp/). HTTP API (https://opencode.ai/docs/api/). Plugins for server, TUI and GUI (https://opencode.ai/docs/llms.txt).
- **Open or closed:** open source. https://github.com/anomalyco/opencode — TypeScript (plus a little Rust), MIT (`LICENSE`), default branch **`dev`**, about 225.9k stars, active, last commit 2026-11-10. `sst/opencode` redirects here (GitHub API returns `anomalyco/opencode` for `sst/opencode`).
- **Releases:** newest is **`v2.2.7`, 2026-11-10**. GitHub's "latest" flag still points at `v1.19.5` (2026-10-28), so the V1 line is still being released in parallel (https://github.com/anomalyco/opencode/releases).
- **Primary sources (all opened):**
  - Docs root (V2): https://opencode.ai/docs/ — index: https://opencode.ai/docs/llms.txt
  - Tools: https://opencode.ai/docs/tools/ — permissions: https://opencode.ai/docs/permissions/ — policies: https://opencode.ai/docs/policies/
  - Agents and subagents: https://opencode.ai/docs/agents/ — instructions: https://opencode.ai/docs/instructions/
  - MCP: https://opencode.ai/docs/mcp-servers/ — skills: https://opencode.ai/docs/skills/ — plugins: https://opencode.ai/docs/plugins/
  - Config: https://opencode.ai/docs/config/ — models: https://opencode.ai/docs/models/
  - Compaction: https://opencode.ai/docs/compaction/ — snapshots: https://opencode.ai/docs/snapshots/
  - CLI: https://opencode.ai/docs/cli/ — web: https://opencode.ai/docs/cli/web/ — ACP: https://opencode.ai/docs/cli/acp/
  - SDK: https://opencode.ai/docs/build/sdk/ — API: https://opencode.ai/docs/api/
  - Migration: https://opencode.ai/docs/migrate-v1/ — changelog: https://opencode.ai/changelog — product: https://opencode.ai/product
  - V1 docs still served: https://opencode.ai/v1/docs/github/ , https://opencode.ai/v1/docs/server/
  - README: https://github.com/anomalyco/opencode
- **Ambiguities and collisions:**
  - Docs at `/docs` describe **V2**; older V1 pages (GitHub integration, server) only exist under `/v1/docs/` and the V2 index has no GitHub page. A survey must say which major version a feature belongs to.
  - The unrelated older Go project https://github.com/opencode-ai/opencode is archived (last commit 2025-09-18, release `v0.0.55`); its README says the project continued as Crush by its original author and the Charm team.
  - `opencode.ai/zen` now redirects to `opencode.ai/console/models` (observed redirect).
- **Confidence:** high.

---

## 4. Claude Code (Anthropic)

- **Canonical name / vendor:** Claude Code, by Anthropic. An agentic coding tool with one engine across terminal, desktop, IDE, web and mobile (https://code.claude.com/docs/en/platforms).
- **Surfaces today:** CLI; Desktop (the Code tab of the Claude Desktop app); VS Code; JetBrains; web (cloud sessions); mobile; plus Chrome, GitHub Actions, GitLab CI/CD, Code Review, Slack and Claude Tag integrations (https://code.claude.com/docs/en/platforms). Remote Control continues a local session from another device (https://code.claude.com/docs/en/remote-control). Self-hosted environments exist for cloud sessions (https://code.claude.com/docs/en/self-hosted-environments).
- **Claude Agent SDK:** a Python and TypeScript library that runs the Claude Code binary and exposes its tools, permissions, sessions and hooks to your own process. The docs contrast it with the CLI, the plain API client SDKs and Anthropic-hosted Managed Agents (https://code.claude.com/docs/en/agent-sdk/overview, https://platform.claude.com/docs/en/managed-agents/overview).
- **Open or closed:** closed. https://github.com/anthropics/claude-code is public but its `LICENSE.md` reads "All rights reserved"; it holds the changelog, plugins, examples and issue tracker, not the product source (root listing and licence file read via API). About 155.2k stars, default branch `main`, release `v2.1.322` on 2026-11-10, last commit 2026-11-10.
  - Agent SDK: https://github.com/anthropics/claude-agent-sdk-python is MIT (release `v0.2.180`, 2026-11-09). https://github.com/anthropics/claude-agent-sdk-typescript has the same "All rights reserved" licence (release `v0.3.322`, 2026-11-10).
  - https://github.com/anthropics/claude-code-action is MIT.
- **Primary sources (all opened):**
  - Docs root: https://code.claude.com/docs/en/overview — index: https://code.claude.com/docs/llms.txt (pages have `.md` twins)
  - How it works: https://code.claude.com/docs/en/how-claude-code-works — tools: https://code.claude.com/docs/en/tools-reference
  - Permissions: https://code.claude.com/docs/en/permissions — permission modes: https://code.claude.com/docs/en/permission-modes — sandbox: https://code.claude.com/docs/en/sandboxing
  - Memory: https://code.claude.com/docs/en/memory — checkpointing: https://code.claude.com/docs/en/checkpointing
  - Subagents: https://code.claude.com/docs/en/sub-agents — agent teams: https://code.claude.com/docs/en/agent-teams
  - MCP: https://code.claude.com/docs/en/mcp — skills: https://code.claude.com/docs/en/skills — hooks: https://code.claude.com/docs/en/hooks — plugins: https://code.claude.com/docs/en/plugins/overview
  - Settings: https://code.claude.com/docs/en/settings — models: https://code.claude.com/docs/en/model-config
  - Headless: https://code.claude.com/docs/en/headless — GitHub Actions: https://code.claude.com/docs/en/github-actions
  - Cloud: https://code.claude.com/docs/en/claude-code-on-the-web — remote control: https://code.claude.com/docs/en/remote-control — self-hosted environments: https://code.claude.com/docs/en/self-hosted-environments
  - Agent SDK: https://code.claude.com/docs/en/agent-sdk/overview — hosting it: https://code.claude.com/docs/en/agent-sdk/hosting
  - Changelog: https://code.claude.com/docs/en/changelog — what's new: https://code.claude.com/docs/en/whats-new/index — GitHub changelog: https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md
  - Product page: https://claude.com/product/claude-code
- **Ambiguities:** the docs index also lists newer areas a survey should not miss: workflows, channels, routines/scheduled tasks, mods, auto mode, advisor tool (https://code.claude.com/docs/llms.txt). "Claude Code SDK" is the old name of the Agent SDK **(rename not re-verified in this pass)**.
- **Confidence:** high.

---

## 5. Claude Desktop (Anthropic)

- **Canonical name / vendor:** Claude Desktop, Anthropic's desktop app for macOS, Windows and Linux (beta) (https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/desktop-linux).
- **What is in it:** three things share the app. Chat; **Cowork**, which brings Claude Code's agentic capabilities to non-coding knowledge work and handles Dispatch and longer agentic tasks; and the **Code tab**, which *is* Claude Code (https://code.claude.com/docs/en/desktop, https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork). Chat and Cowork are being merged into one experience (same support article).
- **Harness-relevant and specific to Desktop** (all from https://code.claude.com/docs/en/desktop): parallel sessions with Git isolation, pane layout with diff, browser preview, terminal and file editor, side chats, computer use, Dispatch from a phone, PR monitoring, connectors, sessions that run locally, in the cloud or over SSH, and scheduled tasks (https://code.claude.com/docs/en/desktop-scheduled-tasks).
- **Belongs to Claude Code proper, not Desktop:** the agent loop, tools, permissions, hooks, skills, plugins, MCP configuration and project memory, which the local surfaces share (https://code.claude.com/docs/en/platforms). Scripting and the Agent SDK are CLI-only (same page).
- **Extension mechanisms on the Desktop side:** connectors and plugins from the marketplace (https://claude.com/connectors redirects to https://claude.com/marketplace/connectors-plugins), local MCP servers (https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop), and Desktop Extensions for one-click MCP installs (https://www.anthropic.com/engineering/desktop-extensions).
- **Open or closed:** closed. No public repository. A version number for the desktop app was **not verified**.
- **Primary sources (all opened):** https://code.claude.com/docs/en/desktop , https://code.claude.com/docs/en/desktop-quickstart , https://code.claude.com/docs/en/desktop-linux , https://code.claude.com/docs/en/desktop-scheduled-tasks , https://code.claude.com/docs/en/platforms , https://claude.com/download , https://claude.com/product/cowork , https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork , https://support.claude.com/en/collections/16163169-claude-desktop , https://support.claude.com/en/articles/12138966-release-notes , https://www.anthropic.com/engineering/desktop-extensions , https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop , https://claude.com/marketplace/connectors-plugins , https://claude.com/docs/third-party/claude-desktop/overview (Claude Desktop on third-party providers).
- **Ambiguities:** "Claude Desktop" and "Claude Code Desktop" are used for the same app in the docs. Feature availability differs by plan (Dispatch and computer use need Pro, Max or Ultra per the platforms page). Survey it as "Claude Code plus a desktop shell plus Cowork", not as a separate harness.
- **Confidence:** high.

---

## 6. "DeepSeek harness" = DeepSeek Harness (`dsh`), official

- **Canonical name / vendor:** DeepSeek Harness, command `dsh`, by DeepSeek AI. An open-source agent harness built on an "everything is a plugin" architecture on top of the Cordis framework (https://github.com/deepseek-ai/deepseek-harness README).
- **Status:** developer preview with announced breaking changes; the project's own safety notice says it has had no security audit and must not be treated as production-ready (README and `SAFETY.md` in the repository).
- **Surfaces today:** Web UI started with `npx @deepseek-ai/dsh web`, default `http://127.0.0.1:3080` (README); desktop app for macOS (Apple silicon) and Windows x64 (https://deepseek.com/harness); headless one-shot mode, ACP over stdio, and an SDK mode over JSON-RPC stdio, all selected as "profiles" of the single `dsh` launcher (`apps/cli/README.md` in the repository); Python SDK (https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk); GitHub review sessions from webhooks (https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review); plugin markets (https://deepseek-harness.github.io/deepseek-harness/en/guide/plugin-market); reverse-proxy public deployment guide (https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments).
- **Open or closed:** open source. https://github.com/deepseek-ai/deepseek-harness — TypeScript, MIT (`LICENSE`), default branch **`master`**, about 278.1k stars, created 2026-08-13, active, last commit 2026-11-04. **No GitHub releases**; newest tag is `dsh-v0.2.2-rc.1` (commit dated 2026-11-04) and no non-prerelease tag was found among the most recent 100. The npm version was **not verified**.
- **Primary sources (all opened):**
  - Landing: https://deepseek.com/harness — docs root: https://deepseek-harness.github.io/deepseek-harness/ — index: https://deepseek-harness.github.io/deepseek-harness/llms.txt (English pages under `/en/`, `.md` twins exist)
  - Quickstart: https://deepseek-harness.github.io/deepseek-harness/en/guide/quickstart — models: https://deepseek-harness.github.io/deepseek-harness/en/guide/providers
  - Architecture: https://deepseek-harness.github.io/deepseek-harness/en/reference/ — agent lifecycle: https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle — tool pipeline: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-execution-pipeline
  - Tool catalogue: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog — config catalogue: https://deepseek-harness.github.io/deepseek-harness/en/reference/config-catalog
  - Approval: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval — permission presets: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/permission-presets — sandbox: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox
  - Subagents: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent — workflow: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow — skills: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/skills
  - Compaction: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction — persistence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence
  - HTTP server: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server — API gateway: https://deepseek-harness.github.io/deepseek-harness/en/reference/api-gateway
  - Memory (via third-party MCP): https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory — schedules: https://deepseek-harness.github.io/deepseek-harness/en/guide/schedule
  - Plugin development: https://deepseek-harness.github.io/deepseek-harness/en/develop/basic/ , https://deepseek-harness.github.io/deepseek-harness/en/develop/basic/tool
  - README, `SAFETY.md`, `apps/cli/README.md`: https://github.com/deepseek-ai/deepseek-harness
- **Ambiguities:** docs are Chinese-first with an English mirror. There is no changelog page in the docs index; release history must be read from git tags. Related official repository: https://github.com/deepseek-ai/dsh-superagent-prototype (MIT, created 2026-10-31). Large unofficial ecosystem (for example https://github.com/anywhere-labs/dsh-desktop, https://github.com/awesome-dsh-plugin/awesome-dsh-plugin) that must not be mistaken for the official product.
- **Community DeepSeek-first agents, in case the owner meant one of these instead:**
  - https://github.com/codewhale-hq/Codewhale — Rust terminal agent, MIT, about 41.2k stars, release `v0.10.2` (2026-10-24). This **is** the former `Hmbown/DeepSeek-TUI` (GitHub redirects the old name), now provider-agnostic.
  - https://github.com/esengine/DeepSeek-Reasonix — Go, MIT, about 36.1k stars, release `studio-v2.59.0` (2026-11-10).
  - https://github.com/lessweb/deepcode-cli — TypeScript, MIT, about 2.2k stars.
- **Confidence:** high. The product name matches the owner's words exactly and the repository is in DeepSeek's own organisation.

---

## 7. Oh My Pi (`omp`)

- **Canonical name / maintainer:** oh-my-pi, command `omp`, by can1357 / Stencil Labs. "A coding agent with the IDE wired in" (https://github.com/can1357/oh-my-pi README, https://omp.sh/).
- **Relationship to Pi:** the README calls it a "Fork of Pi" and "originally built on" Pi. On GitHub it is **not** a fork object (`fork: false`, no parent), so it does not track upstream through GitHub. It is a hard fork that has become its own distribution: its own npm scope (`@oh-my-pi/pi-coding-agent`), its own binary, its own version line (v18.x against Pi's v1.x) and a Rust core (`crates/pi-*`) that Pi does not have (repository listing; README says about 80k lines of Rust).
- **Surfaces today:** terminal UI and CLI; RPC mode; SDK; ACP; a collaboration feature ("collab"); GitHub integration (https://omp.sh/docs, https://omp.sh/docs/rpc, https://omp.sh/docs/sdk, https://omp.sh/docs/acp, https://omp.sh/docs/collab, https://omp.sh/docs/github).
- **Open or closed:** open source. https://github.com/can1357/oh-my-pi — TypeScript and Rust, MIT (`LICENSE`), default branch `main`, about 38.5k stars, created 2025-12-31, active. Release `v18.18.6` on 2026-11-09 (several releases per day); last commit 2026-11-10.
- **Primary sources (all opened):** https://omp.sh/ , https://omp.sh/docs , https://omp.sh/docs/tools , https://omp.sh/docs/approvals , https://omp.sh/docs/security , https://omp.sh/docs/memory , https://omp.sh/docs/subagents , https://omp.sh/docs/mcp , https://omp.sh/docs/plugins , https://omp.sh/docs/hooks , https://omp.sh/docs/skills , https://omp.sh/docs/settings , https://omp.sh/docs/providers , https://omp.sh/docs/cli , https://omp.sh/docs/rpc , https://omp.sh/docs/sdk , https://omp.sh/docs/compaction , https://omp.sh/docs/collab , https://omp.sh/docs/web , https://github.com/can1357/oh-my-pi , https://stencil.so/blog/the-harness-problem (the edit-format benchmark post; the README's `blog.can.ac` link redirects here). Full page list: https://omp.sh/sitemap.xml (opened).
- **Ambiguities:** `omp.sh/docs/security` is about security *scans*, not the agent's own security model; approvals are at `/docs/approvals`. No changelog page was found on omp.sh; use GitHub releases.
- **Confidence:** high.

---

## 8. Pi

- **Canonical name / maintainer:** Pi, by Earendil Inc. (created by Mario Zechner). "A minimal, extensible agent harness" (https://github.com/earendil-works/pi README, https://pi.dev/).
- **Ownership move:** Zechner announced on 2026-04-08 that he joined Earendil and took Pi with him (https://mariozechner.at/posts/2026-04-08-ive-sold-out/). `badlogic/pi-mono` now resolves to `earendil-works/pi` (GitHub API). The npm scope is `@earendil-works` (README).
- **Surfaces today:** interactive terminal UI; print and JSON modes; RPC mode; TypeScript SDK (README; https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/rpc, https://pi.dev/docs/latest/sdk). Slack/chat automation lives in a separate repository, `earendil-works/pi-chat` (README) **(not opened)**.
- **Deliberate omissions that a survey must record as design choices, not gaps:** no built-in sub-agents or plan mode, and **no built-in permission system**; the README tells users to containerise instead (README; https://pi.dev/docs/latest/security, https://pi.dev/docs/latest/containerization).
- **Open or closed:** open source. https://github.com/earendil-works/pi — TypeScript, MIT (`LICENSE`), default branch `main`, about 121.2k stars, created 2025-08-09, active. Release `v1.5.2` on 2026-11-10; last commit 2026-11-10. Monorepo packages include `pi-ai`, `pi-agent-core`, `pi-coding-agent`, `pi-tui`, `pi-durable`, `chord` (README).
- **Primary sources (all opened):** https://pi.dev/ , https://pi.dev/docs/latest , https://pi.dev/docs/latest/how-pi-works , https://pi.dev/docs/latest/extensions , https://pi.dev/docs/latest/skills , https://pi.dev/docs/latest/packages , https://pi.dev/docs/latest/mcp , https://pi.dev/docs/latest/security , https://pi.dev/docs/latest/containerization , https://pi.dev/docs/latest/compaction , https://pi.dev/docs/latest/sessions , https://pi.dev/docs/latest/settings , https://pi.dev/docs/latest/models , https://pi.dev/docs/latest/cli , https://pi.dev/docs/latest/rpc , https://pi.dev/docs/latest/sdk , https://pi.dev/docs/latest/codemode , https://pi.dev/changelog , https://github.com/earendil-works/pi , https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ (original design rationale), https://mariozechner.at/posts/2026-04-08-ive-sold-out/ , https://earendil.com/ .
- **Ambiguities:** many third-party articles still cite `badlogic/pi-mono` and the old npm name. Pi now has an MCP docs page although its author argued against MCP in 2025 (https://pi.dev/docs/latest/mcp exists; the change of position was not investigated). "Pi" also collides with Inflection's Pi assistant; ignore that.
- **Confidence:** high.

---

## 9. Cursor

- **Canonical name / vendor:** Cursor. The company (Anysphere) was acquired by SpaceX; Cursor's own blog announced completion on 2026-08-14 (https://cursor.com/blog/joining-spacex). Docs describe it as "a coding agent for building ambitious software" (https://cursor.com/docs).
- **Surfaces today:**
  - Desktop editor (IDE) and the **Agents Window**, an agent-first workspace covering local, cloud and remote-SSH agents (https://cursor.com/docs/agent/agents-window).
  - **Cursor CLI**, command `agent`, interactive and headless (https://cursor.com/docs/cli/overview, https://cursor.com/docs/cli/headless).
  - **Cloud Agents** in isolated VMs, started from desktop, web or mobile (https://cursor.com/docs/cloud-agent), with Automations (https://cursor.com/docs/cloud-agent/automations).
  - **Self-Hosted Machines:** Cursor keeps the agent loop and inference; a worker on the customer's machine performs file edits and commands (https://cursor.com/docs/cloud-agent/self-hosted, https://cursor.com/blog/self-hosted-machines). This is the inverse of rizzy-harness, where the node runs the loop.
  - Cursor for iOS (https://cursor.com/docs/cloud-agent/mobile); Bugbot PR review (https://cursor.com/docs/bugbot); TypeScript/Python SDKs and an Agent SDK (https://cursor.com/docs/sdk/typescript, https://cursor.com/docs/agent-sdk); ACP (https://cursor.com/docs/cli/acp); integrations for Slack, Teams, Jira, Linear, GitHub, GitLab, JetBrains, Xcode and others (https://cursor.com/llms.txt).
  - Newer products in the same docs: **Origin** (Git hosting) and **Grok Bot** (persistent "Bots" on a cloud computer) (https://cursor.com/docs/origin, https://cursor.com/docs/grok-bot).
- **Open or closed:** closed. https://github.com/cursor/cursor has no licence and no code releases (about 33.3k stars, last push 2026-05-12).
- **Latest version:** no single product version. Newest changelog entry 2026-11-06 (https://cursor.com/changelog); newest CLI release `v2026.10.22` (https://cursor.com/docs/cli/changelog).
- **Primary sources (all opened):**
  - Docs root: https://cursor.com/docs — index: https://cursor.com/llms.txt (pages have `.md` twins)
  - Agent: https://cursor.com/docs/agent/overview — Agents Window: https://cursor.com/docs/agent/agents-window — terminal tool: https://cursor.com/docs/agent/tools/terminal — plan mode: https://cursor.com/docs/agent/plan-mode
  - Security: https://cursor.com/docs/agent/security — run modes: https://cursor.com/docs/agent/security/run-modes — CLI permissions: https://cursor.com/docs/cli/reference/permissions
  - Rules: https://cursor.com/docs/rules — skills: https://cursor.com/docs/skills — subagents: https://cursor.com/docs/subagents — hooks: https://cursor.com/docs/hooks — MCP: https://cursor.com/docs/mcp — plugins: https://cursor.com/docs/plugins
  - Cloud: https://cursor.com/docs/cloud-agent — self-hosted: https://cursor.com/docs/cloud-agent/self-hosted — cloud security: https://cursor.com/docs/cloud-agent/security — automations: https://cursor.com/docs/cloud-agent/automations
  - CLI: https://cursor.com/docs/cli/overview — headless: https://cursor.com/docs/cli/headless — ACP: https://cursor.com/docs/cli/acp
  - Worktrees: https://cursor.com/docs/configuration/worktrees — models: https://cursor.com/docs/models-and-pricing
  - Changelogs: https://cursor.com/changelog , https://cursor.com/docs/release-notes , https://cursor.com/docs/cli/changelog — blog: https://cursor.com/blog
- **Ambiguities:** `docs.cursor.com` redirects to `cursor.com/docs` (observed). "Background agents" is the old name for Cloud Agents **(rename not re-verified)**. No memory page appears in the docs index; whether Cursor still has a "Memories" feature is **unverified**. The docs list xAI's Grok models and Grok Voice alongside other vendors' models (https://cursor.com/llms.txt, https://cursor.com/changelog).
- **Confidence:** high.

---

## 10. Antigravity (Google)

- **Canonical name / vendor:** Google Antigravity. An agentic coding platform with one agent harness shared by a desktop app, a CLI, an IDE, IDE extensions and an SDK (https://antigravity.google/docs/overview, https://antigravity.google/llms.txt).
- **Surfaces today:**
  - **Antigravity App** ("Antigravity 2.0"), a standalone desktop command centre evolved from the Agent Manager (https://antigravity.google/docs/overview).
  - **Antigravity CLI**, binary `agy`, written in Go, with headless and ACP modes (https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/, https://antigravity.google/docs/cli/headless).
  - **Antigravity IDE** (https://antigravity.google/llms.txt, product list).
  - **Extensions** for VS Code, JetBrains, Visual Studio, Zed and Xcode (https://antigravity.google/llms.txt; https://antigravity.google/docs/ide/extensions).
  - **Python SDK** (https://antigravity.google/product/antigravity-sdk).
  - **Remote Agents** in a Google-managed environment, and Remote Control (https://antigravity.google/docs/remote-agents, https://antigravity.google/docs/remote-control).
- **Open or closed:** closed core. https://github.com/google-antigravity/antigravity-cli is a distribution repository only (README, changelog, examples; no licence file; about 2.8k stars; release `1.5.5` on 2026-11-10). https://github.com/google-antigravity/antigravity-sdk-python is Apache-2.0 (about 4.0k stars, last commit 2026-10-28, no GitHub releases).
- **Latest version:** Antigravity App `v2.31.0`, 2026-11-09 (https://antigravity.google/docs/changelog); CLI `1.5.5`, 2026-11-10 (GitHub releases above).
- **Primary sources (all opened):**
  - Docs: https://antigravity.google/docs (redirects to `/docs/getting-started`) — overview: https://antigravity.google/docs/overview — index: https://antigravity.google/llms.txt (pages have `.md` twins)
  - Features: https://antigravity.google/docs/features — models: https://antigravity.google/docs/models
  - Permissions: https://antigravity.google/docs/permissions — terminal sandbox: https://antigravity.google/docs/sandbox — CLI execution modes: https://antigravity.google/docs/cli/modes
  - Subagents: https://antigravity.google/docs/subagents — teams: https://antigravity.google/docs/teamwork — swarm: https://antigravity.google/docs/swarm
  - MCP: https://antigravity.google/docs/mcp — skills: https://antigravity.google/docs/skills — rules: https://antigravity.google/docs/rules — plugins: https://antigravity.google/docs/plugins — hooks: https://antigravity.google/docs/hooks
  - Artifacts: https://antigravity.google/docs/artifacts — remote agents: https://antigravity.google/docs/remote-agents — remote control: https://antigravity.google/docs/remote-control
  - CLI: https://antigravity.google/docs/cli/reference — headless: https://antigravity.google/docs/cli/headless — migration from Gemini CLI: https://antigravity.google/docs/cli/gcli-migration
  - Extensions: https://antigravity.google/docs/ide/extensions — SDK: https://antigravity.google/product/antigravity-sdk
  - Changelog: https://antigravity.google/docs/changelog — releases: https://antigravity.google/releases — blog: https://antigravity.google/blog
  - Gemini CLI transition post: https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/
- **Ambiguities:** "Antigravity" is no longer just an IDE; the IDE is one of five surfaces. Gemini CLI was folded in: consumer access ended on 2026-06-18 (same Google post). The docs index has no memory page; how memory works is **unverified**. Whether Jules (https://jules.google/, opened, still live) is being merged into Antigravity is **unverified**.
- **Confidence:** high.

---

## Additions

Briefer records. "Why" is one line on what is distinctive or why to skip. Figures from the GitHub API on 2026-11-10; the repository URL is the source unless another is given.

### Verified and worth considering

| # | Harness | Vendor | Open? Repository, language, licence | Latest release (date) | Stars | Surfaces | Docs root (opened) | Why |
|---|---|---|---|---|---|---|---|---|
| A1 | **OpenHands** (now "Agent Canvas") | OpenHands (ex All-Hands-AI; old org name redirects) | Open. https://github.com/OpenHands/OpenHands , TypeScript, MIT, branch `main` | `v1.38.0` (2026-11-10) | 93.1k | Self-hosted web control centre; agent backends local, Docker, VM, Kubernetes, cloud; automations; SDK; can drive third-party agents over ACP (README) | https://docs.openhands.dev/ | Closest analogue to a master with remote workers and a web UI. |
| A2 | **OpenClaw** | OpenClaw | Open. https://github.com/openclaw/openclaw , TypeScript, MIT, `main` | `v2026.11.2` (2026-11-08) | 394.3k | One long-lived Gateway daemon; clients (CLI, web UI, macOS app) and **nodes** connect over WebSocket; nodes declare capabilities and are paired (https://docs.openclaw.ai/concepts/architecture, https://docs.openclaw.ai/nodes) | https://docs.openclaw.ai/ | Gateway-plus-nodes personal agent; pairing, exec approvals and sandboxing at huge scale. |
| A3 | **goose** | Agentic AI Foundation / Linux Foundation (ex Block; `block/goose` redirects) | Open. https://github.com/aaif-goose/goose , Rust, Apache-2.0, `main` | `v1.58.1` (2026-11-10) | 56.4k | Desktop app, CLI, API; ACP; "GDK" Rust crates to build agents (README, https://goose-docs.ai/llms.txt) | https://goose-docs.ai/ | The mature Rust harness; MCP-first extensions, recipes, permission modes, reusable crates. |
| A4 | **Hermes Agent** | Nous Research | Open. https://github.com/NousResearch/hermes-agent , Python, MIT, `main` | `v0.23.1` (2026-11-10) | 262.4k | CLI/TUI, desktop app, messaging gateway on 21+ platforms, Docker terminal backend (https://hermes-agent.nousresearch.com/docs/llms.txt) | https://hermes-agent.nousresearch.com/docs | Persistent memory and agent-written skills; egress proxy with credential injection; checkpoints. |
| A5 | **Zed agent** and the Agent Client Protocol | Zed Industries | Open. https://github.com/zed-industries/zed , Rust, `LICENSE-GPL` and `LICENSE-APACHE` at root (copyleft: ideas only), `main` | `v1.27.0` (2026-11-04) | 94.7k | Editor agent panel; external agents (Claude, Codex, OpenCode, Copilot, Cursor, Pi) via ACP registry (https://zed.dev/docs/ai/external-agents); SSH remote development | https://zed.dev/docs/ai/overview | Rust; ACP is the protocol most harnesses in this file now speak; per-tool permissions. |
| A6 | **Warp** | Warp (Denver Technologies) | Client open since 2026-04-28 (https://www.warp.dev/blog/warp-is-now-open-source). https://github.com/warpdotdev/warp , Rust, AGPL-3.0 with MIT UI crates, branch `master` | GitHub tags stop at `v0.2026.06.09` (dev) although commits continue to 2026-11-10; use https://docs.warp.dev/changelog | 66.0k | Terminal, built-in agent, Warp Agent CLI, cloud agents ("Oz", Automation Platform, Factories) (https://docs.warp.dev/llms.txt) | https://docs.warp.dev/ | Rust; cloud agent orchestration with an OpenAPI spec; copyleft, so ideas only. |
| A7 | **Cline** | Cline (`cline` GitHub organisation) | Open. https://github.com/cline/cline , TypeScript, Apache-2.0, `main` | `v4.1.26` (2026-10-26); also `cli-v3.0.74`, `desktop-v0.0.49` | 71.5k | IDE extension, CLI/TUI, desktop app with SSH remote, cloud agents, Kanban with remote access, SDK, ACP (https://docs.cline.bot/llms.txt) | https://docs.cline.bot/ | The reference approval and checkpoint UX among open IDE agents. |
| A8 | **Amp** | Amp (began at Sourcegraph; current corporate status **unverified**) | Closed. https://ampcode.com/ | No version; news at https://ampcode.com/chronicle | n/a | CLI, desktop, mobile, "Orbs", "Boxes" incl. self-hosted, "use your Mac as a runner", agent-to-agent, plugins (https://ampcode.com/docs nav) | https://ampcode.com/docs | Opinionated closed product with runners and server-side threads. |
| A9 | **GitHub Copilot CLI** and cloud agent | GitHub | Closed. https://github.com/github/copilot-cli is a release/issue repo with a proprietary licence (`LICENSE.md`) | `v1.0.109` (2026-11-10) | 11.4k | CLI; cloud agent on github.com and mobile | https://docs.github.com/en/copilot/how-tos/copilot-cli | Largest distribution; cloud agent tied to the Git host. |
| A10 | **jcode** | 1jehuang (individual) | Open. https://github.com/1jehuang/jcode , Rust, MIT, branch `master` | `v0.99.1` (2026-10-22) | 21.7k | TUI plus a server that coordinates agent "swarms" in one repo (README) | https://jcode.sh/ | Rust, claims about 3 MB per extra session; directly relevant to the 50 MB idle invariant. Claims unverified. |
| A11 | **T3 Code** | `pingdotgg` GitHub organisation | Open. https://github.com/pingdotgg/t3code , TypeScript, MIT | `v0.1.8` (2026-11-09) | 55.4k | Desktop/web/mobile "control plane" over other agents (https://t3.codes/) | https://t3.codes/ | A UI over other harnesses, not a harness; useful for web UX only. |
| A12 | **Grok Build** | xAI / SpaceX | Open. https://github.com/xai-org/grok-build , Rust, Apache-2.0 | No GitHub releases; last sync 2026-09-29 | 27.7k | Fullscreen TUI, headless, ACP (README) | https://x.ai/cli (title seen; body blocked by Cloudflare, **not read**) | Rust and permissive, but the public tree is a periodic mirror. |
| A13 | **Codewhale** (ex DeepSeek-TUI) | codewhale-hq | Open. https://github.com/codewhale-hq/Codewhale , Rust, MIT | `v0.10.2` (2026-10-24) | 41.2k | Terminal agent, 40+ providers, `/receipts` audit list (README) | https://codewhale.net/ | Rust, permissive; audit trail idea. |
| A14 | **Kilo Code** | Kilo | Open. https://github.com/Kilo-Org/kilocode , TypeScript, MIT | `v7.8.27` (2026-11-05) | 27.9k | VS Code, JetBrains, CLI; `main` frozen for features, work on `v2` (README) | https://kilo.ai/docs | Popular, but mid-rewrite and overlaps Cline. |
| A15 | **Crush** | Charm | Source-available. https://github.com/charmbracelet/crush , Go, **FSL-1.1-MIT** (`LICENSE.md`) | `v0.100.0` (2026-11-05) | 29.0k | TUI | https://github.com/charmbracelet/crush (README is the docs) | Successor of the old Go OpenCode; TUI polish. Licence is not permissive. |
| A16 | **Devin** incl. **Devin Desktop** (ex Windsurf) | Cognition (vendor name **unverified** in this pass) | Closed | n/a | n/a | Cloud agent; desktop "command centre" for local and cloud agents (https://devin.ai/desktop) | https://docs.devin.ai/ | `windsurf.com` and `docs.windsurf.com` now redirect to Devin Desktop (observed). |
| A17 | **Factory Droid** | Factory | Closed. https://github.com/Factory-AI/factory has no code licence | n/a | n/a | Droid CLI and platform | https://docs.factory.com/ (`docs.factory.ai` redirects) | Enterprise agent; nothing found that the picks above do not cover. |
| A18 | **Kiro** | AWS (vendor **unverified** in this pass) | Closed. https://github.com/kirodotdev/Kiro is an issue tracker, no licence | n/a; https://kiro.dev/changelog/ | 4.5k | IDE and CLI; specs, steering, hooks, local command sandbox (https://kiro.dev/docs/ nav) | https://kiro.dev/docs/ | Spec-driven workflow is its one distinctive idea. |

Top-pick source lists (all opened unless marked):

- **OpenHands:** https://docs.openhands.dev/llms.txt , https://docs.openhands.dev/openhands/usage/agent-canvas/overview , https://docs.openhands.dev/openhands/usage/agent-canvas/architecture , https://docs.openhands.dev/openhands/usage/agent-canvas/backends , https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/remote , https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/docker-execution , https://docs.openhands.dev/openhands/usage/agent-canvas/acp-agents , https://docs.openhands.dev/openhands/usage/automations/overview , https://docs.openhands.dev/overview/skills , https://docs.openhands.dev/overview/model-context-protocol , https://docs.openhands.dev/sdk , https://github.com/OpenHands/software-agent-sdk , https://github.com/OpenHands/OpenHands/releases . Note: docs release notes stop at Agent Canvas 1.35.0 while GitHub is at 1.38.0 (docs index vs releases), so docs lag slightly. Whether everything in the repository is MIT (for example an enterprise directory) is **unverified**.
- **OpenClaw:** https://docs.openclaw.ai/llms.txt , https://docs.openclaw.ai/concepts/architecture , https://docs.openclaw.ai/nodes , https://docs.openclaw.ai/tools/exec-approvals , https://docs.openclaw.ai/gateway/sandboxing , https://docs.openclaw.ai/gateway/security , https://docs.openclaw.ai/concepts/memory , https://docs.openclaw.ai/tools/subagents , https://docs.openclaw.ai/web/control-ui , https://github.com/openclaw/openclaw .
- **goose:** https://goose-docs.ai/llms.txt , https://goose-docs.ai/goose-docs-map.md , https://goose-docs.ai/docs/quickstart , https://goose-docs.ai/docs/guides/managing-tools/goose-permissions , https://goose-docs.ai/docs/getting-started/using-extensions , https://goose-docs.ai/docs/guides/recipes/recipe-reference , https://goose-docs.ai/docs/guides/context-engineering/using-skills , https://goose-docs.ai/docs/guides/goose-cli-commands , https://goose-docs.ai/docs/guides/acp-providers , https://goose-docs.ai/docs/gdk/ , https://github.com/aaif-goose/goose/releases , https://aaif.io/ . https://goose-docs.ai/docs/guides/subagents returned a 391-byte stub **(not usable; find the subagents page through the docs map)**.
- **Hermes Agent:** https://hermes-agent.nousresearch.com/docs/llms.txt , https://hermes-agent.nousresearch.com/docs/developer-guide/architecture , https://hermes-agent.nousresearch.com/docs/user-guide/security , https://hermes-agent.nousresearch.com/docs/user-guide/features/memory , https://hermes-agent.nousresearch.com/docs/user-guide/features/skills , https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation , https://hermes-agent.nousresearch.com/docs/user-guide/features/tools **(not opened)** , https://hermes-agent.nousresearch.com/docs/user-guide/checkpoints-and-rollback **(not opened)** , https://hermes-agent.nousresearch.com/docs/user-guide/egress **(not opened)** , https://github.com/NousResearch/hermes-agent .
- **Zed:** https://zed.dev/docs/ai/overview , https://zed.dev/docs/ai/agent-panel , https://zed.dev/docs/ai/external-agents , https://zed.dev/docs/ai/tool-permissions , https://zed.dev/docs/ai/agent-settings , https://zed.dev/docs/ai/mcp , https://zed.dev/docs/remote-development , https://zed.dev/releases/stable , https://agentclientprotocol.com/ . https://zed.dev/docs/ai/rules returns 404.
- **Warp:** https://docs.warp.dev/ , https://docs.warp.dev/llms.txt , https://docs.warp.dev/changelog , https://www.warp.dev/blog/warp-is-now-open-source , https://github.com/warpdotdev/warp , and the section indexes named in `llms.txt` — https://docs.warp.dev/_llms-txt/agents.txt , https://docs.warp.dev/_llms-txt/warp-agent-cli.txt , https://docs.warp.dev/_llms-txt/automation-platform.txt , https://docs.warp.dev/openapi.yaml **(these four not opened)**.
- **Cline:** https://docs.cline.bot/llms.txt , https://docs.cline.bot/cline-overview , https://docs.cline.bot/features/auto-approve , https://docs.cline.bot/core-workflows/checkpoints , https://docs.cline.bot/tools-reference/all-cline-tools , https://docs.cline.bot/usage/cloud-agents , https://docs.cline.bot/usage/kanban , https://docs.cline.bot/usage/cli-overview **(not opened)** , https://github.com/cline/cline/releases .

### Verified, recommend skipping

| Harness | State | Evidence |
|---|---|---|
| **Gemini CLI** | Superseded. Google moved free and Google AI Pro/Ultra users to Antigravity CLI on 2026-06-18; the repo remains for enterprise Code Assist licences and is "not optimized for Gemini 4+". Still released: `v0.67.0`, 2026-11-04; Apache-2.0; 108.8k stars. | https://github.com/google-gemini/gemini-cli README; https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/ ; docs https://geminicli.com/docs/ |
| **Roo Code** | Dead. Repository archived; last release `v3.54.0` and last commit 2026-05-15; Apache-2.0; 24.2k stars. `roocode.com` redirects to Roomote, a cloud agent "by the creators of" Roo Code. Community fork: Zoo Code (`v3.96.0`, 2026-11-07, 2.1k stars). | https://github.com/RooCodeInc/Roo-Code ; https://roomote.dev/ ; https://github.com/Zoo-Code-Org/Zoo-Code . The shutdown announcement itself was **not opened** (secondary reports only). |
| **Continue** | Frozen. README says the repository "is no longer actively maintained and is read-only"; "final 2.0.0 release" on 2026-06-19; last commit 2026-07-21. Not flagged archived in the API. Apache-2.0; 36.5k stars. | https://github.com/continuedev/continue ; https://docs.continue.dev/ |
| **Aider** | Stalled. Last release `v0.86.0` on 2025-08-09; last commit 2026-05-22; Apache-2.0; 50.1k stars. No shutdown notice found. Historically important (repo map, edit formats); read once for ideas, do not survey as a live competitor. | https://github.com/Aider-AI/aider ; https://aider.chat/HISTORY.html |
| **Windsurf** | Renamed/absorbed: the domain redirects to Devin Desktop. Survey under Devin if at all. | https://windsurf.com/ → https://devin.ai/desktop (observed) |
| **Qwen Code** | Live vendor CLI. Apache-2.0, 29.2k stars, `v0.27.1` (2026-11-09). Lineage from Gemini CLI is **unverified** here. Nothing distinctive identified. | https://github.com/QwenLM/qwen-code ; https://qwenlm.github.io/qwen-code-docs/en/users/overview |
| **Kimi Code, ZCode, Mistral Vibe, MiniMax Code** | Live vendor CLIs tied to one model family. Kimi Code: MIT, 8.2k, `2.5.3`. ZCode: Apache-2.0, 8.6k, `v3.17.2`. Mistral Vibe: Apache-2.0, 5.4k, `v3.1.2`. MiniMax Code: 2.2k (found in search, details **unverified**). | https://github.com/MoonshotAI/kimi-code ; https://github.com/zai-org/ZCode ; https://github.com/mistralai/mistral-vibe ; https://github.com/MiniMax-AI/minimax-code |
| **Junie, Auggie** | Closed, proprietary licences in their distribution repos. | https://github.com/JetBrains/junie ; https://github.com/augmentcode/auggie |
| **ForgeCode** | Rust, Apache-2.0, 7.6k stars, `v2.14.0` (2026-10-01); moved from `antinomyhq/forge`. Small; covered by goose/jcode. | https://github.com/tailcallhq/forgecode |
| **Every Code** (`just-every/code`) | Codex CLI fork, Apache-2.0, 4.0k stars, last release 2026-09-29. Only relevant as "what people add to Codex". | https://github.com/just-every/code |

### Ranking and recommended cut

Ranked for a self-hosted Rust harness with a web dashboard and a master/node split. Weight went to remote or distributed execution, web UI, sandboxing, approvals, sub-agents, memory and extensibility, then to licence (permissive code can be read closely) and to being alive.

1. **OpenHands / Agent Canvas** — the same product shape: self-hosted web control centre, pluggable execution backends, per-conversation containers, automations. MIT.
2. **OpenClaw** — the same topology: one gateway, paired nodes with declared capabilities, exec approvals, a web control UI. MIT. The most-used self-hosted agent, so its security complaints are the best free threat model available.
3. **goose** — the mature Rust harness with foundation governance; reusable agent crates; permission modes; MCP as the extension model. Apache-2.0.
4. **Hermes Agent** — strongest on memory and self-written skills, plus egress isolation and credential-injecting proxy. MIT.
5. **Zed agent + ACP** — Rust, per-tool permission design, and the protocol that would let rizzy-harness host or be hosted by the others. Copyleft: take ideas.
6. **Warp** — Rust, cloud agent orchestration with a published API; shows what "agents as a fleet" looks like in a product. Copyleft: take ideas.
7. **Cline** — approvals, checkpoints and plan/act UX that users of open IDE agents expect; now also remote and cloud. Apache-2.0.

**Recommended cut: survey 1 to 7 in full.** If the budget is five, drop Warp and Cline: Cursor (already a target) covers cloud fleets, and OpenCode and Claude Code cover approval UX.

Next tier, a half-size survey each if there is time: **Amp** (closed, but runners and self-hosted boxes), **GitHub Copilot cloud agent** (how the largest vendor sandboxes a cloud agent), **jcode** (Rust memory numbers against our 50 MB invariant).

Skip: everything in the "recommend skipping" table, plus T3 Code, Grok Build, Codewhale, Kilo, Crush, Devin, Factory Droid and Kiro unless a specific question comes up (T3 Code for web UX; Kiro for specs; Codewhale for its receipts log).

One overlap to settle before round 1 starts: OpenClaw and Hermes Agent are general personal agents first and coding agents second. rizzy-harness describes itself as "a self-hosted assistant and coding-agent harness", so they are in scope, but the owner named only coding tools. If the owner wants coding harnesses only, replace them with Amp and jcode.

---

## Summary table

| Name | Open/closed | Repository or product URL | Docs root | Latest version (date) | Confidence |
|---|---|---|---|---|---|
| Codex | Mixed: CLI, SDK, app server open (Apache-2.0); IDE extension, cloud, desktop closed | https://github.com/openai/codex | https://learn.chatgpt.com/docs | `rust-v0.167.0` (2026-11-06) | High |
| OpenCodex | Open (MIT); a gateway, not a harness | https://github.com/lidge-jun/opencodex | https://opencodex.me/ | `v2.100.1` (2026-11-10) | Medium-low on identity; owner to confirm |
| OpenCode | Open (MIT) | https://github.com/anomalyco/opencode | https://opencode.ai/docs/ | `v2.2.7` (2026-11-10); V1 line `v1.19.5` (2026-10-28) | High |
| Claude Code | Closed (public repo, all rights reserved) | https://github.com/anthropics/claude-code | https://code.claude.com/docs/en/overview | `v2.1.322` (2026-11-10) | High |
| Claude Agent SDK | Python MIT; TypeScript all rights reserved | https://github.com/anthropics/claude-agent-sdk-python | https://code.claude.com/docs/en/agent-sdk/overview | Python `v0.2.180` (2026-11-09); TS `v0.3.322` (2026-11-10) | High |
| Claude Desktop | Closed | https://claude.com/download | https://code.claude.com/docs/en/desktop | Version unverified | High |
| DeepSeek Harness (`dsh`) | Open (MIT), developer preview | https://github.com/deepseek-ai/deepseek-harness | https://deepseek-harness.github.io/deepseek-harness/ | tag `dsh-v0.2.2-rc.1` (2026-11-04); no stable release | High |
| Oh My Pi (`omp`) | Open (MIT); hard fork of Pi | https://github.com/can1357/oh-my-pi | https://omp.sh/docs | `v18.18.6` (2026-11-09) | High |
| Pi | Open (MIT); now Earendil Inc. | https://github.com/earendil-works/pi | https://pi.dev/docs/latest | `v1.5.2` (2026-11-10) | High |
| Cursor | Closed; owned by SpaceX since 2026-08-14 | https://cursor.com/ | https://cursor.com/docs | Changelog 2026-11-06; CLI `v2026.10.22` | High |
| Antigravity | Closed core; Python SDK Apache-2.0 | https://antigravity.google/ | https://antigravity.google/docs | App `v2.31.0` (2026-11-09); CLI `1.5.5` (2026-11-10) | High |
| OpenHands | Open (MIT) | https://github.com/OpenHands/OpenHands | https://docs.openhands.dev/ | `v1.38.0` (2026-11-10) | High |
| OpenClaw | Open (MIT) | https://github.com/openclaw/openclaw | https://docs.openclaw.ai/ | `v2026.11.2` (2026-11-08) | High |
| goose | Open (Apache-2.0) | https://github.com/aaif-goose/goose | https://goose-docs.ai/ | `v1.58.1` (2026-11-10) | High |
| Hermes Agent | Open (MIT) | https://github.com/NousResearch/hermes-agent | https://hermes-agent.nousresearch.com/docs | `v0.23.1` (2026-11-10) | High |
| Zed | Open (GPL and Apache files) | https://github.com/zed-industries/zed | https://zed.dev/docs/ai/overview | `v1.27.0` (2026-11-04) | High |
| Warp | Client open (AGPL-3.0, MIT UI crates) | https://github.com/warpdotdev/warp | https://docs.warp.dev/ | GitHub tags stale since 2026-06; see docs changelog | Medium (version unverified) |
| Cline | Open (Apache-2.0) | https://github.com/cline/cline | https://docs.cline.bot/ | `v4.1.26` (2026-10-26) | High |
| Amp | Closed | https://ampcode.com/ | https://ampcode.com/docs | No version; Chronicle feed | Medium (ownership unverified) |
| GitHub Copilot CLI | Closed | https://github.com/github/copilot-cli | https://docs.github.com/en/copilot/how-tos/copilot-cli | `v1.0.109` (2026-11-10) | High |
| jcode | Open (MIT) | https://github.com/1jehuang/jcode | https://jcode.sh/ | `v0.99.1` (2026-10-22) | Medium (performance claims unverified) |
| Gemini CLI | Open (Apache-2.0); superseded for consumers | https://github.com/google-gemini/gemini-cli | https://geminicli.com/docs/ | `v0.67.0` (2026-11-04) | High |
| Aider | Open (Apache-2.0); stalled | https://github.com/Aider-AI/aider | https://aider.chat/ | `v0.86.0` (2025-08-09) | High |
| Roo Code | Open (Apache-2.0); archived | https://github.com/RooCodeInc/Roo-Code | https://roocodeinc.github.io/Roo-Code/ | `v3.54.0` (2026-05-15), final | High |
| Continue | Open (Apache-2.0); read-only | https://github.com/continuedev/continue | https://docs.continue.dev/ | `v2.0.0` (2026-06-19), final | High |
| Kilo Code | Open (MIT) | https://github.com/Kilo-Org/kilocode | https://kilo.ai/docs | `v7.8.27` (2026-11-05) | High |
| Crush | Source-available (FSL-1.1-MIT) | https://github.com/charmbracelet/crush | README | `v0.100.0` (2026-11-05) | High |
| Codewhale | Open (MIT) | https://github.com/codewhale-hq/Codewhale | https://codewhale.net/ | `v0.10.2` (2026-10-24) | High |
| Grok Build | Open mirror (Apache-2.0) | https://github.com/xai-org/grok-build | https://x.ai/cli (not read) | No tags; last sync 2026-09-29 | Medium |
| Qwen Code | Open (Apache-2.0) | https://github.com/QwenLM/qwen-code | https://qwenlm.github.io/qwen-code-docs/en/users/overview | `v0.27.1` (2026-11-09) | High |
| Devin / Devin Desktop (ex Windsurf) | Closed | https://devin.ai/desktop | https://docs.devin.ai/ | Unverified | Medium |
| Factory Droid | Closed | https://factory.ai (not opened) | https://docs.factory.com/ | Unverified | Medium |
| Kiro | Closed | https://kiro.dev (root not opened) | https://kiro.dev/docs/ | Unverified | Medium |

## Not resolved

- Which "OpenCodex" the owner means. Needs the owner.
- Desktop app version numbers for Claude Desktop and the ChatGPT desktop app.
- Whether Cursor and Antigravity still expose a user-visible memory feature; neither docs index has a memory page.
- The npm version of `@deepseek-ai/dsh` (no npm client on this machine) and whether any stable `dsh` release exists.
- Amp's corporate ownership, Cognition as vendor of Devin Desktop and AWS as vendor of Kiro were not confirmed from a primary page in this pass.
- Roo Code's shutdown announcement was seen only through secondary sites; the archive flag and the redirect are primary.

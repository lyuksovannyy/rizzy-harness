# Claude Desktop - Round 1 public-surface survey

Surveyed 2026-11-10. Public docs only; no source code read. The target is not a separate harness: Chat, Cowork and the Code tab (Claude Code with a desktop shell) share one app. Claude Code core behaviour (loop, tools, hooks, skills, CLAUDE.md) is surveyed elsewhere; this file records what the desktop app adds.

Abbreviations: CD = Claude Desktop app. "Chat" = conversations in claude.ai / desktop / mobile. "Code tab" = Claude Code inside CD. "3P" = CD routed to Bedrock, Foundry, Google Cloud Agent Platform or a gateway.

## 1. Identity

- Name: Claude Desktop (docs also say "Claude Code Desktop" for the Code tab). Vendor: Anthropic. Closed source, no public repository, proprietary.
- Surfaces inside the app: Chat, Cowork, Code tab, built-in browser pane, Dispatch (phone to desktop). Related but separate: claude.ai/code (web cloud sessions), iOS/Android app, Claude in Chrome extension.
- Platforms: macOS (universal DMG), Windows x64 and arm64 (installer, Microsoft Store, MSIX), Linux beta on Ubuntu 22.04+/Debian 12+ x86_64/arm64 only, via Anthropic apt repo (https://claude.com/download, https://code.claude.com/docs/en/desktop-linux).
- Version surveyed: newest changelog entry v4.23118.1, 2026-11-09, bundling Claude Code 2.1.321 (https://claude.com/docs/cowork/changelog.md). 00-targets.md said the version was unverified; this is the correct source. Note the changelog uses an odd numbering (v4.29.0 on 2026-10-27 followed by v4.4212.0 on 2026-10-29).
- Plans: Chat on every plan including Free; Claude Code and Cowork need Pro, Max, Ultra, Team or Enterprise (https://claude.com/download).

## 2. Features by area

### Area 1 - Agent loop and execution model

### Same engine as Claude Code
- What: The Code tab runs the same Claude Code engine as the CLI; config, project memory and MCP servers are shared across the local surfaces. Cowork reuses Claude Code's agentic architecture for non-coding work, without a terminal.
- Status: GA
- Evidence: https://code.claude.com/docs/en/platforms (Platforms and integrations); https://claude.com/docs/cowork/overview.md (Overview)
- Notes: Desktop sets some env vars itself (for example API_TIMEOUT_MS 900000) and ignores settings-file `env` for local sessions; use the local environment editor instead (https://code.claude.com/docs/en/desktop). Desktop's Cowork skills/plugins come from claude.ai, not `~/.claude`.

### Three session locations: local, cloud, SSH (plus WSL)
- What: A Code tab session runs on your machine (local), on Anthropic-managed cloud VMs (continues after the app closes), over SSH on a remote machine, or in WSL 2 on Windows. Picked in the row under the prompt box; model and permission mode can change mid-session.
- Status: GA (SSH and WSL documented as shipped; Windows SSH remote and `sshConfigs` carry beta notes in changelog)
- Evidence: https://code.claude.com/docs/en/desktop (Session types); https://claude.com/docs/cowork/changelog.md
- Notes: Plugins are unavailable in WSL sessions; plugins installed in Desktop are not available in cloud sessions.

### Area 2 - Built-in tools

### Code tab tool set
- What: Nothing desktop-specific is documented beyond Claude Code's own tools. Desktop adds routing: connector first, then Bash, then Chrome, then iOS Simulator, then computer use (broadest and slowest last).
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Computer use)
- Notes: Full tool list belongs to the Claude Code survey. Cowork-specific tools beyond file/Office-document handling, browser and computer use: nothing documented in the pages opened.

### Cowork file and Office deliverables
- What: Cowork reads and writes local files directly and produces spreadsheets with formulas, presentations and formatted documents; slash commands `/docx /pdf /pptx /xlsx` appear in its menus; new-chat Output picker can create Google Docs/Sheets/Slides.
- Status: GA
- Evidence: https://claude.com/docs/cowork/overview.md; https://claude.com/docs/cowork/changelog.md (v2.16120.0, 2026-09-29)
- Notes: `vendor-claim` on "polished" output quality.

### Area 3 - Code editing and file operations

### File pane editor
- What: Local sessions can edit files in the working directory inside the app (auto-save default on; Override/Discard on external changes). Cloud, SSH and WSL sessions are view-only. Some files (`.mcp.json`, `.claude/settings.json`) are view-only.
- Status: GA (since v1.2581.0 per version table)
- Evidence: https://code.claude.com/docs/en/desktop (Terminal and files)

### Right-click path actions and external editors
- What: Right-click a file path to attach it, open in VS Code, Cursor or Zed, reveal in Finder/Explorer, or copy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop

### Cowork project file handling limits
- What: Claude reads individual files up to 50 MB in a Cowork project; dragging files copies them into the project's first folder without overwriting (numbered suffix up to 19); macro-capable Office files prompt each open.
- Status: GA
- Evidence: https://claude.com/docs/cowork/guide/projects.md
- Notes: (verified in review)

### Area 4 - Terminal and shell integration

### Integrated terminal pane
- What: Ctrl+` toggles a terminal sharing the session's environment; available in local and SSH sessions, with limits on Windows remote machines.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Terminal and files)

### Area 5 - Permissions, approvals and sandboxing

### Permission modes in the desktop UI
- What: Manual (default), Accept edits, Plan, Auto (background classifier checks each action; needs a recent model), Bypass permissions. Mode remembered per folder for local sessions; cloud sessions offer Accept edits, Plan, Auto only. Shift+Tab does not cycle modes in Desktop and `dontAsk` is CLI-only.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Permission modes; Differences from the CLI)
- Notes: Pro/Max/Ultra enable Bypass in Settings; Team/Enterprise controlled by org policy (`permissions.disableBypassPermissionsMode`, `disableAutoMode`). (partly verified in review: mode list, Auto model requirement, Shift+Tab, dontAsk; per-folder memory not checked)

### Cowork approval levels
- What: Manual (ask before actions), Auto (a reviewer checks each action and blocks unsafe ones), Skip (nothing checks; page advises only for fully trusted tasks). Permanent deletion needs explicit permission.
- Status: GA
- Evidence: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork (Security and controls)
- Notes: Changelog v4.29.0 says "skip-all-approvals" follows the org's Cowork setting. (verified in review)

### Cowork VM sandbox and folder scoping
- What: Cowork shell commands run in a hardened local VM (Linux beta: QEMU+KVM, needs `/dev/kvm` and `/dev/vhost-vsock`; Windows uses OS virtualization). Access is scoped to folders the user attaches, network egress follows an allowlist. Resolved paths are checked so symlinks and `..` can't escape. Only the user, not the agent, may attach a network-drive path. Admins (3P) can set an `allowedWorkspaceFolders` allowlist with per-folder rw/ro.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop-linux (Cowork requirements); https://claude.com/docs/third-party/claude-desktop/overview.md (Security posture, Architecture table: "Code execution sandbox: Local VM"); https://claude.com/docs/third-party/claude-desktop/local-access.md
- Notes: SOURCES CONFLICT on where Cowork runs, see section 6. Local VM on Linux (QEMU/KVM, /dev/kvm, /dev/vhost-vsock), the 3P architecture table, symlink and `..` resolved-path checks, user-only network-drive attach (stated for Windows mapped drives) and the read-only-in-file-tools-only caveat verified in review; cloud Cowork also exists. (corrected in review: Cowork has both local-VM and cloud modes) Read-only folders are enforced for Claude's file tools only in Code sessions, not shell commands (local-access page).

### Code tab sandbox
- What: Shell confinement in the Code tab uses Claude Code's sandbox where supported; on Windows devices and hosts lacking sandbox dependencies the folder allowlist does not confine shell commands. `blockReadsOutsideWorkingDirectories` only turns such reads into prompts there.
- Status: GA, partial platform coverage
- Evidence: https://claude.com/docs/third-party/claude-desktop/local-access.md (Note)
- Notes: (verified in review)

### Chat file-creation sandbox egress
- What: Claude's file-creation/code-execution sandbox in Chat has a network egress setting for Team and Enterprise owners: off (pre-installed packages only), package managers only, package managers plus owner-specified domains, or all domains minus Anthropic's legal blocklist. Defaults: Team = package managers only; new Enterprise orgs = off; Free/Pro/Max/Ultra have network enabled for approved package sources. MCP integrations can still reach outside regardless.
- Status: GA
- Evidence: https://support.claude.com/en/articles/12111783-create-and-edit-files-with-claude (opened in review)
- Notes: Domain-allowlist option and the non-Team/Enterprise default were missing from the first draft. (corrected in review: added the fourth egress option and the Free/Pro/Max/Ultra default)

### Area 6 - Context management and compaction

### Context card, /compact, /context
- What: Desktop shows a Context card with usage and supports `/compact` and `/context`. Cloud sessions set their own auto-compact threshold.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Sessions); https://code.claude.com/docs/en/claude-code-on-the-web (Manage context)
- Notes: Teleported cloud sessions that were compacted bring only the summary, not earlier messages.

### Area 7 - Memory and persistent instructions

### Chat memory (topic-based)
- What: Claude stores individual topics as you chat (or when told to remember); on by default for Free, Pro, Max, Ultra; Team/Enterprise owner controls. Each project has its own memory; project chats see project memory plus non-project memory, not other projects'. Pause, reset, per-topic edit/delete under Settings; per-chat off switch only before first message; sensitive topics excluded unless opted in; some data classes never saved. Memory is included in data exports and shared with cloud Cowork only (not local Cowork).
- Status: GA
- Evidence: https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context (via WebFetch; opened); https://support.claude.com/en/articles/12138966-release-notes (Jul 10 2026 "Updated memory", Aug 25 2026 "Memory in Cowork and editable topics")
- Notes: Replaced an earlier daily-summary design (Jul 10 2026). Turning memory off org-wide deletes all members' memory. (verified in review)

### Chat search over past conversations
- What: Claude searches earlier non-project chats and chats within one project (not across projects); searches show as tool calls with links to the source chats; can be turned off in Settings > Your Claude. Paid plans.
- Status: GA
- Evidence: https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context

### Incognito chats and memory import/export
- What: Incognito chats are excluded from history, memory and search. Memory can be imported from other assistants and exported (web and desktop, all plans).
- Status: GA
- Evidence: https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context; https://support.claude.com/en/articles/12123587-import-and-export-your-memory-from-claude (title seen in search result only, not opened)
- Notes: Import/export article `unverified` (not opened).

### Cowork projects with project memory
- What: A Cowork project holds local folders, instructions, links, linked claude.ai projects and a project-scoped memory store; lives on the computer only, not synced or shareable. Dispatch reads the project description to choose a project for a task. claude.ai projects (account-side) are separate and shareable on Team/Enterprise.
- Status: GA
- Evidence: https://claude.com/docs/cowork/guide/projects.md
- Notes: Contrast with Memory page statement that local Cowork doesn't use account memory; project memory is a separate store. (verified in review)

### Monthly recap
- What: Settings > Reflect shows topics, most active day/peak hour and work observations; requires memory on.
- Status: beta
- Evidence: https://support.claude.com/en/articles/12138966-release-notes (Jul 9 2026)
- Notes: (verified in review)

### Project instruction files in the Code tab
- What: CLAUDE.md and `.claude/` config are shared with the CLI (project memory shared across local surfaces). Some settings keys (`companyAnnouncements`, `fileSuggestion`, `respectGitignore`, `prUrlTemplate`, others) have no visible effect in the Code tab.
- Status: GA
- Evidence: https://code.claude.com/docs/en/platforms; https://code.claude.com/docs/en/desktop (Enterprise configuration)

### Area 8 - Planning, tasks and modes

### Plan mode, plan pane and tasks pane
- What: Plan mode explores and proposes without editing source; desktop adds dedicated plan, tasks and subagent panes and a plan box at end of turns (changelog v2.42304.0, not shown in 3P). Side chat (Cmd/Ctrl+; or `/btw`) asks a read-only question without touching the main thread and isn't saved to disk. View modes Normal/Thinking/Verbose (Ctrl+O).
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Workspace and panes); https://claude.com/docs/cowork/changelog.md

### Suggested tasks
- What: Claude can propose follow-up tasks with Start, Do it here, Options, Start all actions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Cross-session features)

### Area 9 - Sub-agents and multi-agent orchestration

### Cowork sub-agent coordination
- What: Cowork divides complex work into parallel workstreams via sub-agents; plugins can bundle agents (loaded in Cowork and Code, ignored in Chat).
- Status: GA
- Evidence: https://claude.com/docs/cowork/overview.md; https://claude.com/docs/plugins/platform-support.md

### Cross-session work inside the app
- What: Claude in a desktop session can list, read, message, rename and archive other desktop sessions (local, SSH, WSL only; not cloud or CLI). Archive needs approval except in Auto/Bypass. Admin key `crossSessionInbound`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Cross-session features, Managed settings)

### Projects (cloud coordinator conversation)
- What: A Project is one ongoing conversation where Claude starts a thread per task, each usually a cloud session, running in parallel and sharing repositories, instructions and memory; a thread can be moved to your own computer through Remote Control. Appears in the Code tab sidebar and claude.ai/code.
- Status: beta (public beta, rolling out; Pro/Max/Ultra only, not Team/Enterprise)
- Evidence: https://code.claude.com/docs/en/claude-projects (note block; fuller text read via saved output)
- Notes: Distinct from claude.ai chat projects and Cowork projects (three things named "project"). (verified in review)

### Area 10 - Background, long-running and scheduled work

### Local scheduled tasks (Desktop)
- What: Desktop starts a fresh session on a schedule while the app is open and the computer awake. Options: Hourly, Daily, Weekdays, Weekly, Manual, Custom cron (local time, minimum once per hour in forms; Claude itself can set 1-minute intervals and one-off runs). Deterministic few-minute start offset; skipped when asleep; on wake one catch-up run for the latest missed slot within 7 days. Per-task permission mode and model; optional isolated worktree; "Always allowed" panel to review/revoke approvals; prompt stored at `~/.claude/scheduled-tasks/<name>/SKILL.md`. "Keep computer awake" setting. Needs Desktop 1.1.5368+.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop-scheduled-tasks
- Notes: `/schedule` was removed from the Cowork slash menu in v3.6556.1; the changelog says typing it now fills in a request to schedule the task, like the Schedule button, so scheduling is not removed. (corrected in review: it is not a settings key, and the command still produces a schedule request) (verified in review: catch-up, 7 days, jitter, Always allowed, 1.1.5368)

### Cloud scheduled tasks (formerly routines)
- What: Run in the cloud when the computer is off; can also be triggered by API call or GitHub events; minimum interval 1 hour; Auto mode permission by default; fresh clone, no local files. Listed on the same Scheduled page in Desktop.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop-scheduled-tasks (comparison table); https://code.claude.com/docs/en/routines (linked, not opened)
- Notes: The product was renamed; app may show "Routines". (verified in review)

### Cowork scheduled tasks
- What: `/schedule` in a task or "Scheduled" in the sidebar manages recurring/on-demand Cowork tasks. Cowork article says they run in the cloud.
- Status: GA
- Evidence: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork
- Notes: The `/schedule` menu change does not conflict (typing it still requests a schedule). The Cowork article's statement is confirmed ("Scheduled tasks run in the cloud, with no device online"); the Desktop page describes a different product (Code-tab local tasks). A changelog fix also mentions a task "moved to the cloud". (corrected in review: conflict reduced to two different products) (verified in review)

### Dispatch
- What: One persistent Cowork conversation you message from the phone; it runs on your desktop (computer awake, app open), uses local files/connectors/plugins/apps, and can spawn Code sessions that appear in the sidebar with push notifications. Pairing: install both apps, open Cowork, select Dispatch, enable file access and keep-awake. Single continuous thread only. Spawned computer-use approvals last 30 minutes.
- Status: beta (support article: limited beta, not available to new users, existing users keep it)
- Evidence: https://support.claude.com/en/articles/13947068 (opened); https://code.claude.com/docs/en/desktop (Dispatch); https://code.claude.com/docs/en/mobile
- Notes: Pro/Max/Ultra only, not Team/Enterprise (code.claude.com/docs/en/desktop). Docs on code.claude.com present it without the "not available to new users" caveat. The 30-minute approval window is stated on the code.claude.com desktop page, not in the support article (which points to a separate computer-use page). (corrected in review: attribution of the 30-minute figure) (verified in review: limited beta, not available to new users, desktop must be awake)

### Cloud sessions
- What: Sessions on Anthropic-managed VMs (or a self-hosted environment) keep running after you close the laptop; started from claude.ai/code, mobile, desktop environment selector, `claude --cloud` or scheduled/triggered runs. No separate VM charge, counts against plan limits. GitHub credentials stay server-side behind a proxy; network access levels (default Trusted) set per cloud environment.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web
- Notes: Pro, Max, Ultra, Team; Enterprise with premium or Chat+Claude Code seats. Not available with ZDR, HIPAA config or CMEK.

### Area 11 - Sessions: persistence, resume, fork, checkpoints, undo

### Session sidebar, archive, auto-archive
- What: Sessions list with group, sort and filter; archive; auto-archive after the PR merges or closes (cloud and running sessions excluded); worktrees removed about 30 days after archive (uncommitted changes can be lost). OS notification when a background session finishes.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop
- Notes: Checkpoint/rewind in the desktop UI: nothing documented on the pages opened (cloud page mentions "Rewind to here" for self-hosted runners).

### Move work between surfaces
- What: "Open in" sends a local desktop session to the cloud; `claude --teleport` pulls a cloud session into the terminal (needs clean git state, same repo and account; terminal copy diverges from the cloud one); a CLI session can be moved into the app with `/desktop`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Continue elsewhere); https://code.claude.com/docs/en/claude-code-on-the-web; https://claude.com/download
- Notes: CLI-to-cloud handoff is one-way from the CLI; Desktop can push a local session to the cloud.

### Area 12 - Git and version control

### Parallel sessions with worktrees
- What: Toggle Worktree before a session; stored in `<project>/.claude/worktrees/` by default (configurable; SSH has its own folder), `.worktreeinclude` copies gitignored files, branch prefix configurable. Git `post-checkout` hooks no longer run on worktree creation (v4.16336.0).
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Session types, Worktrees); https://claude.com/docs/cowork/changelog.md

### Diff view, inline comments, code review card
- What: Click line counts on the Changes card to open the diff; hover a line and `+` to comment, send comments with the next message. `/code-review` produces a Code review card with "Walk through in diff" and "Apply fixes" (local, SSH, WSL only). Cloud diff view compares to base branch or any branch, computed from raw blob content.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Diff review); https://code.claude.com/docs/en/claude-code-on-the-web (Review changes)

### PR monitoring, auto-fix, auto-merge
- What: With `gh` authenticated, Desktop can auto-fix CI failures and review comments from trusted authors, and enable GitHub auto-merge (squash unless a merge queue); merge-when-ready disabled in SSH and WSL. Cloud auto-fix needs the Claude GitHub App and posts replies under your GitHub account labelled as Claude Code.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (PR monitoring); https://code.claude.com/docs/en/claude-code-on-the-web (Auto-fix pull requests)
- Notes: Docs warn that replies can trigger `issue_comment` automation.

### Area 13 - Extensibility

### Connectors (remote MCP) and directory
- What: Connectors are MCP servers added through a UI (+ > Add connectors; manage under Customize). The marketplace page lists 1,104 connectors and 530 plugins with filters for Web, Desktop extension and Interactive types; labels Verified, Community, Custom. Cowork loads connectors, skills and plugins enabled on the claude.ai account, synced at session start.
- Status: GA
- Evidence: https://claude.com/marketplace/connectors-plugins (counts as of fetch, page possibly truncated); https://claude.com/docs/connectors/verification.md (listed in llms.txt, not opened); https://claude.com/docs/cowork/overview.md
- Notes: Counts `vendor-claim`-adjacent and may change. (verified in review)

### Desktop Extensions (.mcpb)
- What: ZIP package with `manifest.json` bundling a local MCP server (Node, Python or binary; Node runtime bundled in the app). `user_config` builds a settings UI; fields marked sensitive go to the OS keychain. Install by opening the file or dragging into Settings; directory extensions auto-update, private ones don't. Format renamed from `.dxt`; spec versioned 0.1 and open source. CLI `@anthropic-ai/mcpb` has `init` and `pack`.
- Status: GA
- Evidence: https://www.anthropic.com/engineering/desktop-extensions; https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop
- Notes: Old `isDxt*` managed keys now unrecognised (changelog v2.37337.1), replaced by `isDesktopExtensionEnabled` etc. The support article documents no manual JSON config path for local MCP servers. (verified in review)

### Plugins across Chat, Cowork, Code
- What: Plugins bundle skills, commands, agents, hooks, connectors. Component support differs: agents and hooks ignored in Chat; local MCP servers load only when Cowork runs on the computer; `bin/` executables can't be installed in Chat or Cowork; LSP, output styles, themes and settings are Code-only. Installs via Customize > Plugins attach to the account and also sync into Claude Code at next session start; CLI installs stay on the machine. Upload a zip (200 MB limit, 5,000 files), up to 25 self-added marketplaces. Admins mark plugins Not available / Available / Installed by default / Required; org sync from GitHub or GitLab. Claude can suggest plugins with an Add card in cloud sessions.
- Status: GA
- Evidence: https://claude.com/docs/plugins/platform-support.md; https://claude.com/docs/plugins/overview.md (listed, not opened); https://code.claude.com/docs/en/claude-code-on-the-web (Plugin suggestions)
- Notes: A "create plugin with Claude" flow exists in claude.ai/desktop per docs index (https://claude.com/docs/plugins/create-with-claude.md, not opened). (verified in review)

### MCP Apps and interactive connectors
- What: Connectors can ship `ui://` resources rendered as side-panel or full-screen apps with app tools, `sendMessage`, file download and framed sites; mobile can render interactive apps (Mar 25 2026).
- Status: GA (from docs index and release notes)
- Evidence: https://claude.com/docs/llms.txt (MCP Apps section, titles only); https://support.claude.com/en/articles/12138966-release-notes
- Notes: Detail pages not opened.

### MCP tunnels
- What: A deployable stack (Helm or Docker Compose, cloudflared) that lets Claude reach MCP servers inside a private network without inbound ports; OAuth for internal auth servers.
- Status: unclear (title and description only)
- Evidence: https://claude.com/docs/llms.txt (MCP tunnels section)
- Notes: Pages not opened; put in Unverified.

### Skills
- What: Skills live in `~/.claude/skills/` for the Code tab, plus skills synced from claude.ai in local and cloud sessions; Cowork reads only the account-synced set.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Connectors, skills, plugins); https://claude.com/docs/cowork/overview.md

### Mods and hooks
- What: Hooks and plugin-bundled "mods" exist in Claude Code; directory lists mods for Claude Code only. Desktop-specific extension beyond that: nothing documented.
- Status: unclear
- Evidence: https://claude.com/docs/plugins/platform-support.md

### Area 14 - Models, providers and inference

### Claude Desktop on 3P
- What: A deployment mode that routes all inference through Google Cloud Agent Platform, Amazon Bedrock, Microsoft Foundry, an LLM gateway, or the Anthropic API directly. The web app is bundled in the desktop binary, conversation history is stored on the user's disk, identity is a local device identity. Chat, Cowork and Code all work. Config via MDM, a bootstrap HTTPS endpoint, or the admin console. Feature releases lag standard CD.
- Status: GA (some keys beta, e.g. `sshConfigs`, `skillScanningEnabled`)
- Evidence: https://claude.com/docs/third-party/claude-desktop/overview.md
- Notes: Computer use, Claude in Chrome (except console-managed orgs), cloud sessions, `--cloud`, Remote Control are not available on 3P; the overview page itself does not say this, but https://claude.com/docs/third-party/claude-desktop/feature-matrix.md, https://code.claude.com/docs/en/computer-use and https://code.claude.com/docs/en/remote-control do. The overview carries no GA label; "GA" is an inference. Scheduled tasks, Memory and SSH remote sessions are available on 3P. (verified in review; corrected in review: sources and status basis)

### Model and effort controls
- What: Model picker per session; effort and fast mode for Code sessions; Remote Control lets a connected device change model and effort. Fast mode on Opus 4.8 is deprecated, removal 2026-11-30. Auto mode requires Opus 4.6+, Sonnet 4.6+, Haiku 5.5 or a Fable model.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop; https://claude.com/docs/cowork/changelog.md (v4.16336.0); https://code.claude.com/docs/en/remote-control
- Notes: Research uses Opus 5.5 with Opus 4.8 fallback (verified in review at https://support.claude.com/en/articles/11088861-use-research-on-claude).

### Area 15 - UI surfaces

### Pane workspace
- What: Draggable panes: chat, diff, browser, terminal, file, plan, tasks, subagent, iOS Simulator (macOS); popout windows, split view, minimize to small view (v4.0.0+), session rail with Environment (incl. Remote Control switch), Changes, Context, background tasks. Quick Entry global hotkey (X11 and Wayland portal on Linux). Dictation on mac/Windows, not Linux. Chats can open in a new window (v2.37337.1).
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop; https://code.claude.com/docs/en/desktop-linux; https://claude.com/docs/cowork/changelog.md
- Notes: (partly verified in review: Linux Quick Entry and dictation limits only)

### Unified Claude (Chat + Cowork merged)
- What: Chat and Cowork share one message box (select Cowork or Chat); Cowork capabilities available from any chat; artifacts, projects, connectors and skills carry over; gradual rollout for Pro and Max on web, desktop, mobile. For 3P, `desktopHome` controls it and an unset value reads as `standard` (Unified) after 2026-11-10.
- Status: beta / rolling out
- Evidence: https://support.claude.com/en/articles/12138966-release-notes (Sep 16 2026); https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork; https://claude.com/docs/cowork/changelog.md (v4.4212.0)
- Notes: (partly verified in review: Sep 16 2026 rollout note only)

### Artifacts, Design, Slides, Docs
- What: Artifacts can be created in any conversation including Claude Code; Claude Design, Slides and Docs artifact types are on every plan including Free (beta on Enterprise). Individual artifacts can be shared; Cowork sessions cannot. Live artifacts created before 2026-08-19 are desktop-only. Edit drafts in place by highlighting text (Jun 12 2026).
- Status: GA (Enterprise beta)
- Evidence: https://support.claude.com/en/articles/12138966-release-notes; https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork (Limitations)
- Notes: A general artifacts docs page does not exist in claude.com docs index.

### Mobile app and cross-device
- What: iOS and Android app is a client for Code sessions (cloud, Projects, Remote Control, Dispatch) in a Code tab; photos/files attached on phone reach the local session; push notifications when Remote Control active. The download page lists iOS and a QR code; Android is on the Code docs page but not on the download page. Mobile can reach Maps and iMessage connectors.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mobile; https://claude.com/download

### Claude Duet
- What: A separate app where Claude acts as a proactive agent; research preview for Pro and Max since 2026-10-21; can search past chats if allowed.
- Status: preview
- Evidence: https://support.claude.com/en/articles/12138966-release-notes
- Notes: Only a one-line release note opened; mechanism `unverified`. (existence and date verified in review; mechanism still unverified)

### Area 16 - Browser automation and computer use

### Built-in browser pane
- What: Tabbed browser pane (Cmd/Ctrl+Shift+B) with a clean profile separate from the user's browser; Claude starts dev servers, takes screenshots, inspects the DOM and clicks to verify changes ("Auto-verify"). Per-site Allow once / Always allow / Deny for external sites; Keep cookies, Clear browsing data; per-site Local network access (v4.4212.0). Cowork's version can import logins from Chrome/Edge/Firefox (mac) or Firefox (Win/Linux beta). Anthropic's site safety check blocks or flags risky sites; admin domain allow/block lists share the Claude in Chrome policy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Preview and browser); https://claude.com/docs/third-party/claude-desktop/browser.md; https://support.claude.com/en/articles/16607400-use-the-built-in-browser-in-claude-cowork (opened in review: supports login import, safety checks, owner control only)
- Notes: Docs also describe sign-in popups including Google OAuth. Per-site choices documented on the 3P page are Allow once and Always allow (no Deny seen); cookies persist in Cowork until Clear browsing data, but Code sessions clear them at app quit unless Keep cookies is set to Shared or Per session. The separate-storage claim, site safety check and admin domain lists (shared with Claude in Chrome policy) are verified. (corrected in review: Deny choice not found; cookie behaviour differs by session type)

### `.claude/launch.json` dev-server config
- What: JSON listing named dev servers (runtime, args, cwd, port, autoPort, env, url, `autoVerify`), used by the preview/Dev servers menu. `url` supports HTTPS, `*.localhost` and attaching to an already-running server.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Preview)

### iOS Simulator pane
- What: macOS-only pane for driving the iOS Simulator instead of screen control; supports key presses and rotation (v3.6556.1). Managed key `disableMobileSimulatorTools`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop; https://code.claude.com/docs/en/computer-use (see "Test a simulator flow"); https://claude.com/docs/cowork/changelog.md

### Computer use
- What: Off by default; Settings > This computer. macOS (needs Accessibility and Screen Recording, macOS 14+) and Windows; not Linux. App tiers fixed: View only (browsers, trading), Click only (terminals, IDEs), Full control (rest). Approvals last the session (30 min for Dispatch-spawned). One session at a time holds a lock (CLI doc); other apps are hidden while Claude works; Esc aborts; screenshots downscaled. A denied-apps list exists in Desktop only.
- Status: preview (research preview)
- Evidence: https://code.claude.com/docs/en/desktop (Computer use); https://code.claude.com/docs/en/computer-use
- Notes: Pro, Max, Ultra only; not Team/Enterprise; not with 3P. Release note dates it Mar 23 2026 for Cowork and Code. (verified in review)

### Claude in Chrome extension
- What: Browser extension that drives your real Chrome/Edge (other Chromium browsers detected) with your signed-in sessions; connects to Claude Code or CD through a native messaging host and a bridge at `bridge.claudeusercontent.com`; opens tabs in a tab group; pauses on login/CAPTCHA; can record GIFs, upload files (10 MB limit; refuses credential-named files), read console. Site permissions inherited from the extension; project settings cannot enable it. Needs claude.ai login (not API key, not 3P). Not supported in WSL.
- Status: GA (extension 1.0.36+ for Claude Code integration)
- Evidence: https://code.claude.com/docs/en/chrome
- Notes: Extension must be signed into the same org as the app. Shortcuts and scheduled tasks saved in the extension are lost on logout. (verified in review)

### Area 17 - Remote, cloud, headless, CI

### Remote Control
- What: Connects claude.ai/code or the mobile app to a Claude Code session running on your machine (CLI, Desktop `/remote-control`, VS Code). Execution and filesystem stay local; web/mobile are a window onto it; messages sync across devices; Desktop has a "Connect new sessions" setting and `remoteControlAtStartup`. Server mode supports `--spawn worktree`, `--capacity` default 32. Team/Enterprise need an Owner toggle; Trusted Devices option (beta, off by default; Jun 25 2026). Not with API keys, a custom base URL or a Claude apps gateway.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control (read via saved output; plan and flag lines read); https://support.claude.com/en/articles/12138966-release-notes (Jun 25 2026)
- Notes: Capacity default 32, `--spawn worktree`, plan list, Owner toggle and API-key/base-URL/gateway restrictions verified in review. (corrected in review: Trusted Devices is labelled beta)

### SSH sessions
- What: Desktop connects through `~/.ssh/config` (own OpenSSH or built-in client), installs a helper and Claude Code under `~/.claude/remote/` (allow about 1 GB; folder cannot be on a noexec mount). Forwarding options (agent, X11, tunnels) disabled by Desktop; admin keys `sshConfigs`, `sshHostCommands` (command that creates a machine), `sshHostAllowlist` (jump hosts too), `disableSshSavedPasswords`; deep link `claude://code/new?ssh_host=...`. Windows remote hosts supported from v2.31226.0 with restrictions.
- Status: GA (some beta flags)
- Evidence: https://code.claude.com/docs/en/desktop (SSH; read at offset 100000)
- Notes: The remote app login is renewed only while Desktop is connected, so overnight runs can lose sign-in and stop; the docs say not to count on overnight tasks. (corrected in review: removed the claim that tasks survive sleep, network loss and app quit, which the pages read do not support; forwarding options, admin keys, deep link, Windows host version verified)

### Self-hosted environments for cloud sessions
- What: Team/Enterprise can route cloud sessions to runners in their own infrastructure; isolation is the deployer's responsibility.
- Status: GA
- Evidence: https://code.claude.com/docs/en/platforms; https://code.claude.com/docs/en/claude-code-on-the-web (Security and isolation)
- Notes: Runner pages not opened.

### Headless / SDK
- What: Scripting and the Agent SDK are CLI-only; Desktop has no headless mode.
- Status: GA (statement)
- Evidence: https://code.claude.com/docs/en/platforms

### Area 18 - Collaboration, teams and enterprise administration

### Admin console controls and managed settings
- What: Toggles for Desktop access, cloud sessions, Remote Control; under HIPAA, Desktop is off by default and cloud/Remote Control cannot be enabled. Managed keys include disableAutoMode, bypass restriction, browser keys, SSH keys, `disableDesktopLocalSessions`. Delivery: macOS `com.anthropic.claudefordesktop` domain, Windows `SOFTWARE\Policies\Claude`, Linux `/etc/claude-desktop/managed-settings.json` (root-owned). `forceLoginOrgUUID` restricts sign-in. Deployment via DMG/MDM, MSIX, PKG. SSO on Team/Enterprise. Cloud sessions receive only server-managed settings; Cowork sessions don't fetch console settings.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Enterprise configuration); https://claude.com/download
- Notes: A broken managed-settings file blocks local Code sessions (v3.1623.0).

### Org plugin and extension governance
- What: Owners set per-plugin availability, sync plugin repos, allow/deny directory extensions, upload private extensions, use private plugin marketplaces; system policy overrides in-app lists.
- Status: GA
- Evidence: https://claude.com/docs/plugins/platform-support.md; https://www.anthropic.com/engineering/desktop-extensions (Enterprise controls); https://claude.com/product/cowork

### Shared sessions and team features
- What: Cloud sessions can be shared (Team: Private/Team visibility with repo-access verification; Pro/Max: Private/Public). Cowork sessions cannot be shared. Enterprise gets project spend tags (beta, Nov 6 2026), Analytics API, OpenTelemetry. Claude Tag and Slack integrations exist for team entry points.
- Status: GA (spend tags beta)
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web (Share sessions); https://support.claude.com/en/articles/12138966-release-notes; https://claude.com/product/cowork
- Notes: Claude Tag only seen in docs index and platforms page.

### Claude Desktop for Government
- What: Separate administrator documentation for tenants, organizations, seat tiers, SCIM, compliance API and tenant restrictions via proxy header.
- Status: unclear
- Evidence: https://claude.com/docs/llms.txt (Claude for Government admin section, titles only)
- Notes: Not opened beyond titles.

### Area 19 - Observability

### Cowork OpenTelemetry export
- What: Team/Enterprise admins set OTLP endpoint, protocol, headers and resource attributes under Organization settings > Data and privacy. The device exports events: `user_prompt`, `assistant_response`, `tool_result`, `api_request` (cost, tokens, cache), `api_error`, `tool_decision`, correlated by `prompt.id`. Content capture is controlled per device by `otlpContentCapture` (metadata only by default); `workspace.host_paths` and `user.email` always present. Does not cover Code tab sessions. Needs CD 1.1.4173+. Traces on 3P (beta).
- Status: GA
- Evidence: https://claude.com/docs/cowork/monitoring.md
- Notes: Cost values are approximations. Older versions sent content by default on first-party deployments. (verified in review)

### Usage and transcripts
- What: Enterprise Compliance API can retrieve Cowork session transcripts; Settings > Usage card and Storage section appear in 3P builds; 3P telemetry doc lists network paths.
- Status: GA
- Evidence: https://claude.com/docs/cowork/monitoring.md; https://claude.com/docs/cowork/changelog.md; https://claude.com/docs/third-party/claude-desktop/overview.md
- Notes: Code tab OTel: nothing documented on the pages opened (Claude Code's own telemetry is covered elsewhere).

### Area 20 - Configuration and customisation

### Settings surfaces
- What: Settings > Code covers prompt suggestions, browser tools, bypass toggle, worktree location, branch prefix, auto-archive, Remote Control connect. Saved local environments hold env vars and folders. `permissions.defaultMode` in settings. 3P has an in-app configuration window and a documented key reference with a configuration changelog.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop; https://claude.com/docs/third-party/claude-desktop/overview.md; https://claude.com/docs/cowork/changelog.md

### Area 21 - Security and privacy

### Data handling
- What: Local and SSH sessions send data to the configured model provider; cloud sessions to Anthropic's API. Cloud: isolated VMs, network levels, credentials kept outside the VM. Telemetry in 3P: crash reports and analytics scrubbed, disableable; only `downloads.claude.ai` is required egress when other Anthropic services are disabled. Sign-in stored in the OS keyring on Linux. ZDR and HIPAA orgs cannot use cloud sessions; HIPAA turns Desktop and Claude in Chrome off by default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/desktop (Enterprise configuration); https://claude.com/docs/third-party/claude-desktop/overview.md; https://code.claude.com/docs/en/claude-code-on-the-web; https://code.claude.com/docs/en/desktop-linux
- Notes: Deleted Cowork tasks leave history at once and are purged from backend storage within 30 days.

### Prompt-injection safeguards
- What: Computer use flags injection from on-screen content; browser has per-site approval and safety classifiers in every mode; Chrome integration pauses at logins; Cowork carries a documented risk warning.
- Status: GA
- Evidence: https://code.claude.com/docs/en/computer-use; https://code.claude.com/docs/en/desktop (Browser); https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork

### Area 22 - Licence, pricing and distribution

### Pricing (marketing page)
- What: Pro $17/month annual or $20 monthly, Max $100/month, Ultra $200/month, Team $20/seat/month (2-150 seats), Enterprise custom.
- Status: GA
- Evidence: https://claude.com/product/cowork
- Notes: Cowork product page, `vendor-claim` for savings; Cowork uses limits faster than Chat.

### Distribution
- What: DMG, MSIX, PKG, Microsoft Store and a signed apt repository (fingerprint published; no self-update on Linux, apt delivers updates). macOS 14+ for computer use (not re-checked in review).
- Status: GA (Linux beta)
- Evidence: https://code.claude.com/docs/en/desktop-linux; https://claude.com/download; https://claude.com/docs/cowork/changelog.md
- Notes: (partly verified in review: Linux apt repo and no self-update; Windows build number not found)

### Area 23 - Anything else

### Chat-side assistant features seen
- What: Research (multi-step web/connector research; paid plans; needs web search on; works on web, desktop and mobile, but on mobile it searches the web only and skips connectors; cannot use local MCP servers; runs on Opus 5.5 with Opus 4.8 fallback), chat search, memory, incognito, file creation, interactive apps on mobile, edit-in-place of drafts, monthly recap, Personal Finance (US, Pro/Max/Ultra, beta, Nov 5 2026).
- Status: mix GA / beta
- Evidence: https://support.claude.com/en/articles/12138966-release-notes; https://support.claude.com/en/articles/11088861-use-research-on-claude (opened in review)
- Notes: Research article was not opened by the author. (corrected in review: "mobile web only" was wrong; the page says Research works on web, Claude Desktop and mobile)

### Claude Science
- What: A separate AI workbench for research (beta, Mac/Windows/Linux), with cloud storage, literature access, artifacts with versioning and comments.
- Status: beta
- Evidence: https://claude.com/download; https://claude.com/docs/llms.txt (Claude Science titles)
- Notes: Out of scope; noted for completeness.

## 3. Distinctive or unusual

- Three agent surfaces on one engine, with moving boundaries (Chat/Cowork merge, Dispatch spawning Code sessions): rare to see one binary host a general assistant, a knowledge-work agent and a coding agent.
- Dispatch: phone-to-desktop single persistent thread that decides whether to spawn a coding session; includes a 30-minute computer-use approval window.
- Browser pane with dev-server verification via `launch.json` and a separate clean profile; Anthropic-run site safety list.
- Scheduled tasks stored as `SKILL.md` on disk with deterministic jitter, one catch-up run, and a per-task "Always allowed" revocation list.
- Cowork VM using QEMU/KVM on Linux and a folder-attach trust model where only the user can attach network drives.
- Admin delivery of the same product to a customer's own inference provider (3P), with bootstrap-server config delivery.
- Three different things called "project" (claude.ai, Cowork, cloud Projects coordinator).
- Cross-session tools letting the agent manage other desktop sessions, with admin switch.
- Accessible per-tool decision source in telemetry (`config`, `hook`, `user_permanent`, `user_temporary`, ...).

## 4. Architecture and UX notes

- Desktop is a shell over the Claude Code binary for the Code tab; the loop runs wherever the session location is (local process, SSH helper in `~/.claude/remote/`, Anthropic cloud VM).
- Cowork: sessions can run on the device in a hosted VM, or in the cloud (see conflict); the desktop must be open for tasks that need local files or the browser.
- Cloud, Remote Control and mobile all rely on Anthropic-hosted session/relay infrastructure (`bridge.claudeusercontent.com` appears in network requirements; IP allowlists must route it).
- 3P: web app bundled locally, history on local disk, VM workspace bundle and CLI binary downloaded from `downloads.claude.ai`.
- Chrome integration uses native messaging to the CLI/app and a bridge host; project settings cannot turn it on.
- Plugins and skills for Chat/Cowork sync via the account; Claude Code reads machine files; a synced plugin appears in Claude Code next session.

## 5. Relevance to a self-hosted master/node harness

- Session location selector (local/cloud/SSH/WSL) and the SSH helper that survives app exit map directly onto master/node: the "node" is a durable helper, the UI only re-attaches (Area 17 SSH, Area 1).
- Remote Control keeps execution local and uses a relay for the UI; self-hosting means the master can be that relay under the owner's control (Remote Control entry).
- Scheduled-task semantics worth copying: one catch-up run within 7 days, skip reasons visible in history, deterministic jitter, per-task permission mode and revocable allow list (Local scheduled tasks).
- Admin policy delivered to the device with distinct key names and "server-managed vs file" precedence confusion is a documented pain point; a node's `node.toml` authority is simpler (Admin console controls; "Cowork sessions don't fetch console settings").
- Per-site browser approval plus org domain lists and safety classifier before acting (Built-in browser pane) fits a node-side browser tool.
- Memory split (global topics, per-project, no cross-project, per-chat off switch, incognito) is a concrete model for the assistant side (Chat memory).
- Plugin component matrix per surface shows the cost of one package format on several runtimes; plan a single component set per node (Plugins across Chat, Cowork, Code).
- Folder attach as the permission primitive, with resolved-path checks and agent-cannot-attach rule (Cowork VM sandbox and folder scoping).
- Telemetry event schema with decision source and prompt id correlation is a good model for event log fields (Cowork OpenTelemetry export).
- Mobile as a thin client for Remote Control/cloud/Dispatch, with push notifications at "needs decision" (Mobile app).

## 6. Overhyped, stale or contradictory

- Where Cowork runs: the Cowork getting-started article says sessions run in the cloud in an isolated environment, while the Linux and 3P pages say Cowork's commands run in a local VM hosted by the app. Reviewer: two modes exist. The release notes (Jul 7 2026, Claude Cowork on web and mobile) say sessions can run remotely (beta), the OTel page has a "Cowork and Cowork in the cloud" item, and a changelog fix covers tasks falling back to the cloud when an admin turned off the local VM. The getting-started article states cloud-only without qualification, so it is stale or incomplete.
- Cowork scheduled tasks "run in the cloud" per the Cowork article, while Desktop scheduled-task docs describe local runs needing the app open. Reviewer: these are different products (Cowork cloud tasks vs Code-tab local tasks), so this is not a contradiction. The 2026-10-22 changelog entry says `/schedule` now fills in a scheduling request rather than being removed.
- Dispatch: Code docs list it as a Pro/Max/Ultra feature; its support article says limited beta and not available to new users.
- Cowork "generally available" Apr 9 2026 vs. Sep 16 2026 "comes to every conversation" gradual rollout; the product page still says Cowork is "being folded into Claude".
- The Linux page lists Cowork as part of the beta; the download page says Android isn't listed; docs index says no dedicated artifacts, memory or research pages exist on claude.com/docs.
- Productivity claims on the Cowork page come from customer testimonials: `vendor-claim`.
- Changelog version numbering is non-monotonic in appearance (v4.29.0 then v4.4212.0); the desktop page itself notes some version-gated features.

## 7. Unverified

- Chat web search internals, citations, and Research mechanism: the Research article was opened in review (plans, web search requirement, models); mechanism and citations are still not documented there.
- File-creation sandbox details (limits, file types): the article was opened in review for egress only; limits and file types were not extracted.
- Built-in browser in Cowork user guide: opened in review; it does not mention a clean profile, per-site choices, Keep cookies, the Cmd/Ctrl+Shift+B shortcut or domain lists (the 3P browser page covers the first four items except the shortcut; shortcut appears only in changelog text).
- Windows minimum build "10 19041 (Oct 2026)" in Distribution: the download page does not state it; not found in the pages checked (moved here in review).
- Memory import/export article (12123587) was not opened by the author or reviewer.
- Unified Claude Enterprise setup: https://support.claude.com/en/articles/17444462-set-up-and-manage-the-new-unified-claude-experience-for-your-enterprise-organization.
- Dispatch guide on claude.com: https://claude.com/docs/cowork/guide/dispatch.md.
- Cowork security architecture overview (Trust Center, requires access).
- MCP tunnels, MCP Apps, connectors verification, Claude Tag, Claude for Government pages: titles only.
- Desktop page Dispatch and computer-use sections beyond the part read (the page text was truncated at 200,000 of 225,263 chars; the last 25k chars were not read, nor the first 100k read in the same detail as a human would).
- Remote Control transport and security details (relay protocol, token lifetime): only plan/flag/limits lines read; open the remote-control page security section.
- Claude Duet mechanism; Personal Finance; Claude Design details.
- Cowork VM technology on macOS and Windows (named hypervisor): not stated in pages opened except Linux (QEMU/KVM).

## 8. Sources

Opened (WebFetch) 2026-11-10:
- https://code.claude.com/docs/en/desktop - Use Claude Code Desktop: main feature inventory (partially read, offsets 0-200000).
- https://code.claude.com/docs/en/desktop-scheduled-tasks - Schedule recurring tasks.
- https://code.claude.com/docs/en/desktop-linux - Linux beta, Cowork VM requirements.
- https://code.claude.com/docs/en/platforms - surfaces, plans, away-from-terminal options.
- https://code.claude.com/docs/en/computer-use - CLI computer use and desktop differences.
- https://code.claude.com/docs/en/chrome - Claude in Chrome integration.
- https://code.claude.com/docs/en/remote-control - Remote Control (long output, read via grep of saved file).
- https://code.claude.com/docs/en/claude-code-on-the-web - cloud sessions.
- https://code.claude.com/docs/en/claude-projects - Projects (only note and intro read).
- https://code.claude.com/docs/en/mobile - mobile client.
- https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork - Cowork overview, plans, security.
- https://support.claude.com/en/articles/12138966-release-notes - app release notes Mar-Nov 2026.
- https://support.claude.com/en/articles/13947068 - Dispatch.
- https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop - extensions install and controls.
- https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context - chat search, memory, incognito.
- https://support.claude.com/en/collections/16163169-claude-desktop - collection landing (no titles).
- https://www.anthropic.com/engineering/desktop-extensions - Desktop Extensions.
- https://claude.com/download - platforms and install.
- https://claude.com/product/cowork - Cowork product page.
- https://claude.com/marketplace/connectors-plugins - directory counts.
- https://claude.com/docs/llms.txt - claude.com docs index.
- https://claude.com/docs/cowork/overview.md, https://claude.com/docs/cowork/changelog.md, https://claude.com/docs/cowork/monitoring.md, https://claude.com/docs/cowork/guide/projects.md
- https://claude.com/docs/plugins/platform-support.md
- https://claude.com/docs/third-party/claude-desktop/overview.md, .../local-access.md, .../browser.md
- https://code.claude.com/docs/llms.txt - fetch returned 404.

Search results only (pages not opened; used to find primary pages): support.claude.com articles 12111783 (file creation), 11088861 (Research), 16607400 (built-in browser), 12123587 (memory import/export), 15520349, 17444462, 17631495 (Duet).

## 9. Review

Reviewed 2026-11-10 by an independent reviewer, public pages only. Entries checked: 30 of about 71 (counting entries opened in whole or in part; many were partly verified, marked as such). Pages opened included the Cowork getting-started article, the Dispatch article, the desktop, scheduled-tasks, Linux, remote-control, computer-use and chrome pages on code.claude.com, the 3P overview, local-access, browser, feature-matrix pages, the Cowork changelog, monitoring, projects and plugin pages, and the support articles on release notes, memory, Research, file creation and the built-in browser.

| Entry | Verdict | Reason |
|---|---|---|
| Version surveyed (v4.23118.1, 2026-11-09, Claude Code 2.1.321; v4.29.0 then v4.4212.0) | confirmed | Changelog table matches |
| Cowork approval levels | confirmed | Manual/Auto/Skip wording matches the Cowork article |
| Cowork VM sandbox and folder scoping | confirmed-with-correction | Both local-VM and cloud Cowork exist; claim of one location was wrong |
| Cowork scheduled tasks | confirmed-with-correction | Cloud claim holds; `/schedule` was not removed, it now fills in a schedule request |
| Local scheduled tasks | confirmed-with-correction | All numbers verified; "settings key `/schedule`" was wrong |
| Cloud scheduled tasks | confirmed | 1 hour minimum, Auto mode default |
| Dispatch | confirmed-with-correction | Limited beta confirmed; 30-minute figure is from the code.claude.com desktop page, not the support article |
| Remote Control | confirmed-with-correction | Capacity 32, worktree, plans verified; Trusted Devices is beta |
| SSH sessions | confirmed-with-correction | Helper path, 1 GB, admin keys, deep link verified; "survive app quit" unsupported and contradicted |
| Claude Desktop on 3P | confirmed-with-correction | Providers and architecture verified; unavailability list came from other pages; no GA label |
| Chat file-creation egress | confirmed-with-correction | Page opened; a fourth option and non-Team defaults added |
| Research (in chat-side features) | wrong in part | "mobile web only" is wrong; it works on web, desktop, mobile |
| Built-in browser pane | confirmed-with-correction | Support article does not cover per-site choices; Deny not found; cookie rules differ by session type |
| Plugins across Chat, Cowork, Code | confirmed | Matrix, 200 MB, 5,000 files, 25 marketplaces, four admin states |
| Connectors directory counts | confirmed | 1,104 and 530 on the page |
| Desktop Extensions (.mcpb) | confirmed | Blog statements match |
| Cowork OpenTelemetry export | confirmed | Event names, 1.1.4173, content capture, Code tab excluded |
| Cowork project file limits | confirmed | 50 MB, numbered copies up to 19, macro prompts |
| Cowork projects with project memory | confirmed | Local only, Dispatch reads description |
| Chat memory | confirmed | Defaults, per-chat switch timing, cloud Cowork only, export |
| Monthly recap, Duet, Personal Finance, spend tags | confirmed | Release notes dates and labels match |
| Cloud Projects coordinator | confirmed | Public beta, Pro/Max/Ultra, not Team/Enterprise |
| Computer use | confirmed | Plans, tiers, lock, denied-apps list, no Linux; macOS 14 not re-checked |
| Claude in Chrome | confirmed | 1.0.36, 10 MB, credential names, WSL, bridge host |
| Permission modes | confirmed | Mode list, Auto model list, Shift+Tab, dontAsk |
| Code tab sandbox | confirmed | local-access note matches |
| Linux platform and Cowork requirements | confirmed | QEMU/KVM, vhost-vsock, beta, no self-update |
| Distribution (Windows build 10 19041) | not-supported | Moved to Unverified |

Borrowed features: none found. The Dispatch 30-minute window and `/schedule` details were attached to the wrong source rather than borrowed.

Notable omissions (each from a page I opened):
- Dynamic workflows run in Desktop (https://code.claude.com/docs/en/workflows.md, referenced from the desktop and remote-control pages).
- Claude apps gateway sign-in (https://code.claude.com/docs/en/claude-apps-gateway), which disables Remote Control.
- `claude --desktop` and `/desktop` / `/resume` hand-off between CLI and Desktop (https://code.claude.com/docs/en/desktop).
- Saved local and SSH environments and session details panel (same page).
- Claude Desktop 3P bootstrap server and admin console, skill scanning, telemetry and data-storage pages (https://claude.com/docs/third-party/claude-desktop/bootstrap.md, .../admin-console.md).
- Trusted Devices for Remote Control as a standalone control (beta).
- Agent teams: stated as CLI-only, not in Desktop (desktop page).
- Compliance API access to Cowork transcripts is mentioned; the Claude Security and Claude Tag pages are not summarised.

Trust judgement: the verified core (scheduling, Remote Control, SSH admin keys, plugins, OTel, memory, release-note items, computer use, Chrome) is accurate to the word and number. The errors were attribution and overstated reconciliation, not invention. The unchecked remainder (Desktop pane details, session sidebar, worktree defaults, PR monitoring, diff review, mobile, admin key lists) is probably reliable because it comes from the same fetched desktop page, but treat specific version numbers and defaults there as unconfirmed. The file is usable with the corrections above.

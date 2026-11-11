# Synthesis 04: planning, sub-agents, background work (and model slots)

Round 1 cross-harness synthesis, 2026-11-10. Synthesis only: every fact below comes from the round 1 survey files and their reviews; nothing was browsed.

## 1. Scope

Areas covered: **Area 8** (planning, tasks and modes), **Area 9** (sub-agents and multi-agent orchestration), **Area 10** (background, long-running and scheduled work). As the task asked, this file also covers **model roles and slots for auxiliary work** (titles, summaries, vision, judges, commit messages) and their fallback chains. Those entries come from each survey's Area 14 and from the model details inside the Area 1, 8, 9 and 10 entries.

Files and line ranges read (all in `research/round1/`):

| File | Lines read |
|---|---|
| `codex.md` | 54-64, 108-147, 171-175, 340-345, 364-369, 427-504, 824-856, 986-1196 (sections 3-9) |
| `opencodex.md` | 36-43, 67-130, 195-328 |
| `opencode.md` | 36-58, 89-101, 138-141, 213-279, 458-610 (plus grep for model keys) |
| `claude-code.md` | 156-167, 234-239, 327-331, 339-344, 357-362, 480-509, 946-951, 1117-1121, 1373-1845 (Areas 8-10 in full), 1884-1889, 2017-2046, 2113-2118, 2621-2626, 2800-2805, 3000-3095, 3269-3273, 3479-3483, 3604-3627, 4846-4851, 5202-5340 (sections 3-7) |
| `claude-code.review-1.md`, `-2.md`, `-3.md` | whole files, filtered for these areas |
| `claude-desktop.md` | 146-207, 224-239, 290-295, 469-607 |
| `deepseek-harness.md` | 57-62, 201-317, 363-370, 533-677 |
| `oh-my-pi.md` | 26-52, 133-138, 190-266, 297-400, 549-663 |
| `pi.md` | 199-252, 418, 561-697 |
| `cursor.md` | 163-240, 318-335, 456-642 |
| `antigravity.md` | 38-42, 194-270, 375-379, 406-412, 494-618 |
| `openhands.md` | 1-60, 145-216, 294-298, 459-592 |
| `openclaw.md` | 16-36, 122-127, 216-311, 340-345, 390-403, 488-493, 596-788 |
| `goose.md` | 87-90, 204-263, 388-400, 518-677 |
| `hermes-agent.md` | 1-8, 40-60, 287-364, 391-394, 493-505, 679-820 |
| `zed-acp.md` | 15-30, 101-163, 244-255, 316-330, 355-484 |
| `warp.md` | 57-71, 120-128, 194-246, 320-360, 546-670 |
| `cline.md` | 23-28, 84-110, 174-243, 476-629 |
| `own-rizzy-harness.md` | 14-28, 104-209, 315-404 |

Nothing was unreadable. `claude-code.md` was read only by line range, as the brief says.

Two surveys cite in a short form. **OpenHands** writes `D:` for `https://docs.openhands.dev`, and **Hermes Agent** gives paths under `https://hermes-agent.nousresearch.com/docs/`. Both files define their prefix, so this file expands those citations to full URLs using that definition. No other URL was changed.

Claude Code status labels follow the review corrections. Agent view, `--bg`, the supervisor daemon and attach/logs/stop are all research preview, not GA (review-2, correction 1). Cloud scheduled tasks are research preview, have a 1-hour minimum interval, and were formerly called routines (review-2, correction 5).

---

## 2. Features

### Area 8. Planning, tasks and modes

#### Plan mode (restricted planning phase)
What it is: a session state in which the agent investigates and writes a plan but may not change the project until a human approves the plan.
Variants: harnesses differ mostly in **how the restriction is enforced**:
- **Permission rules or tool removal, enforced by the runtime:** OpenCode (the plan agent's rules deny edits except under `~/.opencode/plan`), Antigravity CLI (`--mode=plan` allows only read/search tools), OMP (read-only turns).
- **Prompt guidance only:** DeepSeek says outright that the sandbox and approvals do not read plan state. Cline's plan mode is a tool gate plus a shell blocklist in sub-agents and schedules, so `python -c` can still write.
- **Mixed:** Claude Code blocks edits but runs exploratory shell commands, with a classifier or a prompt deciding which. In an interactive terminal where bypass is available, the blocks are not enforced at all; Claude is only told not to edit.

They also differ in what plan mode changes besides permissions: the model (OMP's `plan` role, Claude's `opusplan`, Cline's per-mode model, Warp's planning model) and the effort (Codex `plan_mode_reasoning_effort`). Pi and goose deliberately ship no plan mode.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/plan [prompt]` switches the chat to plan mode; `plan_mode_reasoning_effort` overrides effort; unavailable while Codex is working; the app-server exposes "collaboration mode" presets. No enforcement mechanism is described. | GA (collaboration modes API experimental) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | `plan` is a primary agent. Its permission rules deny edits to normal project files (allowed only under `~/.opencode/plan`) and allow questions. Switched with Tab or `/agents`. The README (V1-era) says plan asks before bash. | GA | https://opencode.ai/docs/agents/, https://opencode.ai/docs/permissions/, https://github.com/anomalyco/opencode | reviewed |
| Claude Code | A permission mode, entered with `--permission-mode plan`, `/plan` or Shift+Tab. Claude reads and runs exploratory shell commands but source edits are blocked until approval through `ExitPlanMode`. When auto mode is available (`useAutoModeDuringPlan`, default on) a classifier reviews shell commands; otherwise non-read-only commands prompt. **In interactive terminal sessions where bypass is available the blocks are not enforced** (Claude is only instructed); headless, the SDK and VS Code chat keep them. Plan files go to `~/.claude/plans/`. `planModeInstructions` (TS SDK) replaces the workflow body but keeps the enforcement preamble. Ultraplan (cloud planning) was removed in 2.1.222. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/common-workflows.md, https://code.claude.com/docs/en/agent-sdk/typescript.md | corrected (review-1: plan mode also runs classifier-approved commands when auto mode is available) |
| Claude Desktop | Same engine as Claude Code, plus dedicated plan, tasks and subagent panes and a plan box at the end of turns. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Per-agent state logged as a `plan/mode` event. While active, a guidance section is added to each request. This is **soft guidance only: the sandbox and approvals do not read plan state**. `exit_plan_mode` is always registered (the tool list never changes) and needs a markdown plan that starts with a heading. Commands `/plan`, `/plan <message>`, `/plan off`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan | unreviewed |
| Oh My Pi | `/plan` (Alt+Shift+P) runs read-only turns on the `plan` role model and stores the draft as a session artifact. Headless `--plan-yolo`. Exclusive with goal and vibe modes. | GA | https://omp.sh/docs/plan | unreviewed |
| Pi | Documented non-feature. Plans belong in files such as PLAN.md; where restriction is needed, limit the tool set. An example `plan-mode/` extension ships. | GA (deliberate omission) | https://pi.dev/, https://mariozechner.at/posts/2025-11-30-pi-coding-agent/, https://pi.dev/docs/latest/extensions | corrected (review: the plan-mode reason also includes visibility and avoiding approval of many commands) |
| Cursor | Plan mode asks clarifying questions, researches, and writes an editable plan before any code. It may suggest itself on complex prompts. Plans save to the home directory by default. "Build in Parallel" runs independent steps at once. CLI modes are Agent, Plan and Ask. | GA | https://cursor.com/docs/agent/plan-mode, https://cursor.com/docs/cli/overview, https://cursor.com/help/ai-features/multi-agent | unreviewed |
| Antigravity | `/plan` explores read-only, asks structured questions and drafts an Implementation Plan artifact. The agent **always** stops for approval after `/plan`, whatever the review policy. CLI `--mode=plan` restricts to read-only search/view tools until approval. | GA | https://antigravity.google/docs/plan, https://antigravity.google/docs/implementation-plan, https://antigravity.google/docs/cli/modes | unreviewed |
| OpenHands | Nothing documented (only a `task_tracker` tool is named). | nothing documented | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| OpenClaw | Nothing documented as a dedicated plan mode; the session permission mode `read-only` covers read-only work. | nothing documented | https://docs.openclaw.ai/gateway/permission-modes | unreviewed |
| goose | Nothing documented as a plan mode. "Chat Only" mode is analysis without actions. | nothing documented | https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md | unreviewed |
| Hermes Agent | Nothing documented. The CLI page does not describe a plan mode. | nothing documented | https://hermes-agent.nousresearch.com/docs/user-guide/cli | unreviewed |
| Zed/ACP | Zed Agent: nothing documented (the read-only `Ask` profile is the closest). ACP: session modes (used, for example, for plan-mode exit through a permission request) are deprecated in favour of session config options with a `mode` category. | Zed: nothing documented; ACP modes deprecated | https://zed.dev/docs/ai/agent-profiles.md, https://agentclientprotocol.com/protocol/v1/session-modes.md, https://agentclientprotocol.com/protocol/v1/session-config-options.md | unreviewed |
| Warp | `/plan` or natural language produces an editable plan in a rich-text editor. A separate planning model can be set in the profile. In the CLI, `/plan <task>` or Ctrl+Shift+P. Enforcement is not described. | GA | https://docs.warp.dev/agents/capabilities/planning/, https://docs.warp.dev/agents/cli/agent-conversations/ | unreviewed |
| Cline | Plan reads and searches; Act can edit and run commands. History carries over and a separate model per mode is optional. The Plan & Act page says plan cannot run commands. The sub-agent and scheduling pages say that in those contexts plan removes edit tools and blocks only commonly mutating commands, so `python -c` can still write: **not a security boundary**. | GA | https://docs.cline.bot/core-workflows/plan-and-act.md, https://docs.cline.bot/features/subagents.md, https://docs.cline.bot/cli/scheduling.md | reviewed |
| rizzy-harness (ours) | None. The closest is the `ask_all` approval mode and the read-only Scout/Reviewer agents; `ui:progress` is only a display card. | none | `crates/core/src/ui/progress.rs` (inventory §10, §c) | from own inventory |

#### Plan review and hand-off
What it is: how the user inspects, edits, comments on and accepts a plan, and what context the implementation then starts with.
Variants:
- **Comment model:** inline comments on a plan document (Claude Code in VS Code, Antigravity's pending comment chips, Warp's versioned editor), annotate/delete in a full-screen review (OMP), or approve / request changes / dismiss (DeepSeek).
- **Hand-off:** OMP offers four choices (execute in a fresh session with only the plan, compact then implement, keep full context, or refine). Claude Code offers clear-context-on-accept plus a choice of the next permission mode. Cline's `/deep-planning` opens a new task.
- **Approval policy:** Antigravity makes it a setting (`artifactReviewPolicy`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Nothing documented beyond `/plan`. | nothing documented | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| OpenCode | Nothing documented beyond switching agents. | nothing documented | https://opencode.ai/docs/agents/ | unreviewed |
| Claude Code | Options are "Yes, and use auto mode" (or auto-accept edits, or bypass), "Yes, manually approve edits" and "No, keep planning". Ctrl+G edits the plan in an editor. Clearing context on accept is optional. Accepting names the session after the plan. In VS Code the plan opens as a Markdown document with inline comments. Resuming a session that ended in plan mode restores plan mode. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/vs-code.md, https://code.claude.com/docs/en/sessions.md | reviewed |
| Claude Desktop | Plan pane and plan box at the end of turns. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | `exit_plan_mode` presents the plan for review; approval, requested changes and dismissal are distinct outcomes. `ask_user_question` has a `plan-review` intent that changes presentation only. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/user-questions | unreviewed |
| Oh My Pi | Full-screen Plan Review with annotate, delete and edit-in-$EDITOR. Approval options: execute in a fresh session with only the plan, compact then implement, keep full context (blocked above 95% of the window), refine, or save and quit. The implementation role is chosen at approval. | GA | https://omp.sh/docs/plan | unreviewed |
| Pi | Non-feature; the gallery lists a third-party plan-review package (`@plannotator/pi-extension`). | GA (deliberate omission) | https://pi.dev/packages | unreviewed |
| Cursor | Editable plan; the user clicks to build; "Save to workspace" moves the plan into the repo. | GA | https://cursor.com/docs/agent/plan-mode | unreviewed |
| Antigravity | Comments attach to the next message as a chip and Proceed sends them; the agent revises and re-requests review, or implements. `artifactReviewPolicy`: `asks-for-review`, `agent-decides` or `always-proceed`; even always-proceed does not skip `/plan` approval. | GA | https://antigravity.google/docs/implementation-plan, https://antigravity.google/docs/artifact-review, https://antigravity.google/docs/settings | unreviewed |
| OpenHands | Nothing documented. | nothing documented | — | — |
| OpenClaw | Nothing documented. | nothing documented | — | — |
| goose | Nothing documented. | nothing documented | — | — |
| Hermes Agent | Nothing documented. | nothing documented | — | — |
| Zed/ACP | ACP `plan` updates carry entries (content, priority, status); each update replaces the whole plan. There is no approval step in the protocol. | GA | https://agentclientprotocol.com/protocol/v1/agent-plan.md | unreviewed |
| Warp | Version history (each agent update is a restorable version). You can run all of the plan or one section. Plans sync to a Plans folder in Warp Drive, export to Markdown, and can be shared by link. | GA | https://docs.warp.dev/agents/capabilities/planning/ | unreviewed |
| Cline | `/deep-planning` writes `implementation_plan.md`, then creates a new task with trackable steps. | GA | https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| rizzy-harness (ours) | None. | none | inventory §c "Plan mode" | from own inventory |

#### Model-maintained task / to-do list
What it is: a tool the model writes to keep a visible checklist of steps and their status.
Variants: a whole-list replace (DeepSeek `todo_write`, ACP plan updates) versus per-item create/update tools (Claude Code `TaskCreate`/`TaskUpdate`). Some harnesses gate the tool by model: Claude Code turns it off by default for newer models. Some deprecated it: Cline's Focus Chain, and Pi refuses it on principle because to-do lists confuse models. Claude Code persists the list across compaction and can share one list between sessions.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `update_plan` is the local plan tool (hook-matchable). | GA | https://learn.chatgpt.com/docs/hooks.md | unreviewed |
| OpenCode | `todowrite`, opt-in through `experimental.todo`. | experimental | https://opencode.ai/docs/config/ | unreviewed |
| Claude Code | `TaskCreate`/`TaskGet`/`TaskUpdate`/`TaskList` (or `TodoWrite`); lifecycle pending → in_progress → completed or deleted. **On by default only for Opus 4.7, Sonnet 4.6, Haiku 4.5 and earlier**; newer models need opt-in. Ctrl+T shows up to five tasks. Tasks persist across compaction. `CLAUDE_CODE_TASK_LIST_ID` shares a list across sessions. | GA | https://code.claude.com/docs/en/interactive-mode.md, https://code.claude.com/docs/en/agent-sdk/todo-tracking.md | reviewed |
| Claude Desktop | Tasks pane. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | `todo_write` replaces the whole checklist (pending/in_progress/completed). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | `todo` keeps a checklist (blocked/active/complete); the README says it tracks phases. | GA | https://omp.sh/docs/tools | unreviewed |
| Pi | Documented non-feature (use TODO.md); a `todo.ts` example and a third-party package exist. | GA (deliberate omission) | https://pi.dev/, https://pi.dev/docs/latest/extensions | corrected (review: the stated reason is that to-do lists confuse models) |
| Cursor | Nothing documented in the survey. | nothing documented | — | — |
| Antigravity | No separate task-list artifact documented; the Implementation Plan holds the task breakdown. | nothing documented | https://antigravity.google/docs/plan | unreviewed |
| OpenHands | A `task_tracker` tool exists (named in tool annotations). | unclear | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| OpenClaw | `get_goal`/`create_goal`/`update_goal` tools; the bundled Workboard plugin tracks cards (see agent teams). | unclear | https://docs.openclaw.ai/tools | unreviewed |
| goose | A Todo extension tracks tasks. | unclear | https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md | unreviewed |
| Hermes Agent | `todo_list` keeps a session task list with nested subtasks. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Zed/ACP | Protocol `plan` entries with priority and status; whole-plan replace. | GA | https://agentclientprotocol.com/protocol/v1/agent-plan.md | unreviewed |
| Warp | Task lists: the agent breaks requests into tasks, each with status current/completed/not started/cancelled; a chip opens the list. | GA | https://docs.warp.dev/agents/capabilities/task-lists/ | unreviewed |
| Cline | The Focus Chain to-do list is deprecated with no replacement; some pages still mention to-do lists. | deprecated | https://docs.cline.bot/resources/deprecations.md | reviewed |
| rizzy-harness (ours) | None. The project kanban board is user-edited; agents cannot write it. | none | inventory §2 "Missing tools", §c | from own inventory |

#### Goal / run-until-done loop
What it is: a standing objective that keeps the agent taking turns until a check says the objective is met, a budget runs out, or a human stops it.
Variants: **who decides "done"** is the key difference.
- **A separate judge model:** Claude Code (the small/fast model with three verdicts, no tools), OpenHands (judge LLM, round cap 10, invalid JSON counts as not complete), Hermes (auxiliary judge that sees only the last ~4 KB and **fails open to continue**), and OpenCode (a "reviewer", mechanism undocumented).
- **The model's own declaration, enforced by a stop hook that demands evidence:** Antigravity; the survey notes there is no independent check.
- **Deterministic shell checks:** goose recipe `retry.checks`, Hermes quality gates, Claude Code Stop hooks.
- **Budgets:** tokens and time in Codex and OMP, round caps in DeepSeek, OpenHands and Hermes. Antigravity removed its cap.
- **Persistence:** Codex and DeepSeek persist goal state (DeepSeek with compare-and-set revisions and a human-authorised resume); OMP restores paused after a cold restart.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/goal <objective>` (at most 4,000 chars) is a persisted thread goal with `tokenBudget`, `tokensUsed` and `timeUsedSeconds`; pause, resume, edit, clear; `features.goals` means "persisted goals and automatic continuation"; `goals.max_goal_token_budget` caps the budget. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/long-running-work.md | reviewed |
| OpenCode | `/goal <objective>` keeps working until "a reviewer" confirms; pause/resume/clear. Only in the changelog; how the reviewer works is undocumented. | unclear | https://opencode.ai/changelog | reviewed (changelog only) |
| Claude Code | `/goal <condition>` (at most 4,000 chars). After each turn the **small fast model** gets the condition plus the conversation and returns not yet met / met / impossible with a reason; no tools. Built as a session-scoped prompt Stop hook, so workspace trust applies and `disableAllHooks` turns it off. Four errors clear the goal; transient errors retry 3 times then pause. Check-ins back off from 30 minutes while background work runs. Resume restores the goal but resets its counters. `claude -p "/goal …"` runs it headless. Separately, a `Stop` hook with exit 2 blocks ending the turn (overridden after 8 consecutive blocks). | GA | https://code.claude.com/docs/en/goal.md, https://code.claude.com/docs/en/best-practices.md | unreviewed |
| Claude Desktop | Same engine (not separately documented in the survey). | — | — | — |
| DeepSeek harness | An event-sourced goal with objective, phase (active/paused/blocked/complete), a revision for compare-and-set edits, and `maxGoalRounds`. `resume` needs a human; blocked carries a machine-routable code. Activation is process-local and never persisted. A separate experimental `ralph` tool runs a fresh-agent loop with `maxRounds`. | preview (goal), experimental (ralph) | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/goal, https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed (goal), reviewed (ralph) |
| Oh My Pi | Goal mode: one durable objective with automatic continuation until verified, paused, dropped or token budget hit. It survives compaction and is restored as paused after a cold restart. `/guided-goal` interviews first. Completion only after checking each deliverable against repo state. Continuation is automatic only in interactive mode by default. | GA | https://omp.sh/docs/goal | reviewed |
| Pi | Nothing documented. | nothing documented | — | — |
| Cursor | Nothing documented. | nothing documented | — | — |
| Antigravity | `/goal` installs a stop hook that blocks ending the turn until each deliverable is mapped to evidence. One goal per conversation; **no iteration cap** (removed in v1.0.14); waits for running sub-agents and background tasks. Completion is the model's own declaration. | GA | https://antigravity.google/docs/commands/goal | reviewed |
| OpenHands | `/goal`: a separate judge LLM audits the transcript and returns a score, a complete flag and "missing" text that is fed back; default cap 10 rounds; invalid judge JSON counts as not complete. Separately, the experimental Critic scores success and, with iterative refinement, sends the agent back below 0.6 (up to 3 retries). | GA (goal SDK); experimental (critic) | https://docs.openhands.dev/sdk/guides/convo-goal.md, https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md, https://docs.openhands.dev/openhands/usage/agent-canvas/critic.md | reviewed (goal), unreviewed (critic) |
| OpenClaw | Goal tools `get_goal`/`create_goal`/`update_goal`; the Workboard dispatch loop runs cards; workers' "proof" statuses are self-reports. | unclear | https://docs.openclaw.ai/cli/workboard, https://docs.openclaw.ai/tools | unreviewed |
| goose | Recipe `retry`: `checks` are shell commands that must exit 0 (`timeout_seconds` default 300) plus an `on_failure` command (default timeout 600). After a failed check goose resets the message history and restarts. | unclear | https://goose-docs.ai/docs/guides/recipes/recipe-reference.md | reviewed |
| Hermes Agent | `/goal`: after each turn an auxiliary judge model (it sees the goal and the last ~4 KB) returns done/continue/blocked/wait. Default budget 20 continuation turns, then auto-pause. Optional completion contract and shell quality gates (default 3 retries, 5-minute timeout). **Judge errors fail open to continue.** Continuations are plain user messages, so the prompt cache holds. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/goals | reviewed |
| Zed/ACP | Nothing documented. | nothing documented | — | — |
| Warp | Nothing documented. | nothing documented | — | — |
| Cline | Nothing documented. | nothing documented | — | — |
| rizzy-harness (ours) | None. The loop stops at a hard 50 model calls per turn. | none | `crates/agent-native/src/lib.rs:33`, `:650` | from own inventory |

#### Read-only, ask and custom modes
What it is: named modes other than plan that change what the agent may do or how it behaves (ask-only, chat-only, debug, profiles, presets).
Variants: tool-availability profiles (Zed profiles, Cursor custom modes backed by skills, Pi tool loadouts) versus named prompt presets (Codex numbered presets, DeepSeek tool presets) versus behaviour modes (Cursor Debug, Antigravity Planning/Fast).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Ten numbered preset slots (`/presets`, `tui.presets.<0-9>`) hold reusable prompts or slash commands. Custom prompts (`~/.codex/prompts/`) are deprecated in favour of skills. | GA (presets; 8,000-byte limit unverified); deprecated (custom prompts) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/custom-prompts.md | unverified (8,000-byte limit moved to Unverified in review) |
| OpenCode | Primary agents `build` and `plan`; `default_agent`. | GA | https://opencode.ai/docs/agents/ | reviewed |
| Claude Code | Permission modes (Manual, formerly "default", accept-edits, plan, auto, bypass), covered by another group. `/effort ultracode` makes Claude plan a workflow for every substantive task. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/workflows.md | reviewed |
| Claude Desktop | View modes Normal/Thinking/Verbose; permission modes in the UI. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Agent presets `minimal`, `standard`, `cordis`, `ptc` (and `code`) select which tools mount; `agentPreset` can pick a composition for a child agent. No page lists preset contents. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | unreviewed |
| Oh My Pi | Vibe mode (director plus workers, see coordinators), `/fast` priority serving, `/force <tool>`, magic keywords (`ultrathink`, `orchestrate`, `workflowz`). | GA | https://omp.sh/docs/modes, https://omp.sh/docs/magic-keywords | reviewed |
| Pi | Example extensions `preset.ts` and `tools.ts`; `setActiveTools` and `prepareLoadout`. | GA (examples) | https://pi.dev/docs/latest/extensions | unreviewed |
| Cursor | Agent (default), Plan, Ask (read-only), Debug (`/debug`), Custom Modes that a skill can back; Shift+Tab switches. | GA | https://cursor.com/docs/cli/overview, https://cursor.com/docs/cli/reference/slash-commands, https://cursor.com/docs/skills | unreviewed |
| Antigravity | Planning vs Fast mode on the App page; the CLI removed `/planning` and `/fast` in v1.1.0 (possible docs lag). | GA | https://antigravity.google/docs/artifact-review | unreviewed |
| OpenHands | Nothing documented. | nothing documented | — | — |
| OpenClaw | Session permission modes `read-only`, `guarded`, `workspace` (Auto-review), `full`. | GA | https://docs.openclaw.ai/gateway/permission-modes | reviewed |
| goose | Autonomous (default), Manual Approval, Smart Approval, Chat Only (no extensions or file changes). | GA | https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md | reviewed |
| Hermes Agent | Nothing documented as modes (approval modes belong to another area). | nothing documented | — | — |
| Zed/ACP | Profiles Write, Ask (read-only), Minimal (no project tools); custom profiles with per-tool booleans, MCP toggles and a `default_model`. Profiles control tool availability only. | GA | https://zed.dev/docs/ai/agent-profiles.md | unreviewed |
| Warp | Agent profiles (base model, planning model, autonomy, permissions). | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | unreviewed |
| Cline | Plan and Act. | GA | https://docs.cline.bot/core-workflows/plan-and-act.md | reviewed |
| rizzy-harness (ours) | Home-bar modes Search / Ask / Agentic; approval modes. | works | `web/src/lib/modes.ts:4-5` | from own inventory |

#### Interview-first planning commands
What it is: a command that makes the agent question the user (and the code) before planning.
Variants: Antigravity `/grill-me` (one question at a time with a recommended answer; it explores the code instead of asking answerable questions), OMP `/guided-goal`, Cline `/deep-planning` (silent investigation first), Cursor plan mode's clarifying questions, and DeepSeek's timed questions whose answers can arrive after a restart.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `tool/requestUserInput`: 1 to 3 short questions with an optional "other". | experimental | https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| Claude Code | `AskUserQuestion` (area 2; removed from sub-agents). | GA | https://code.claude.com/docs/en/sub-agents.md | reviewed |
| DeepSeek harness | `ask_user_question` takes batches with options, multi-select and "Other". Timed questions return `pending` and **a late answer arrives as a new user message, even after a host restart**. Child agents cannot ask. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/user-questions | unreviewed |
| Oh My Pi | `/guided-goal` interviews before goal mode. | GA | https://omp.sh/docs/goal | reviewed |
| Cursor | Plan mode asks clarifying questions first. | GA | https://cursor.com/docs/agent/plan-mode | unreviewed |
| Antigravity | `/grill-me`: Socratic interview, one question at a time with a recommended answer; combinable as `/plan /grill-me <task>`. | GA | https://antigravity.google/docs/commands/grill-me | unreviewed |
| Hermes Agent | `clarify`: 1 to 5 questions; on messaging platforms the replies are numbers. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Cline | `/deep-planning` step 2: clarifying questions after a silent investigation. | GA | https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| Others | OpenCode `question` tool, goose, OpenHands, OpenClaw, Zed, Warp, Pi: not part of this area in the surveys. | — | — | — |
| rizzy-harness (ours) | No ask tool. Questions are `ui:choices`/`ui:form` blocks in message text, answers checked against the log; sub-agents are capped to display-only blocks. | partial | `crates/node/src/session.rs:345-379`, `crates/core/src/ui/mod.rs:132-145` | from own inventory |

#### Side questions (`/btw`, `/side`)
What it is: ask something without polluting the main conversation.
Variants:
- **No tools, not saved:** Claude Code `/btw` runs concurrently with the main turn and can fork into a background sub-agent; Claude Desktop's side chat is the same idea.
- **An ephemeral forked chat:** Codex `/side`.
- **A background agent:** OMP `/tan`, Hermes `/bg`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/side`: an ephemeral fork with its own transcript; the parent's status stays visible. | GA | https://learn.chatgpt.com/docs/developer-commands.md | reviewed |
| OpenCode | `/btw <question>`: not added to the conversation. | GA | https://opencode.ai/docs/cli/tui/ | unreviewed |
| Claude Code | `/btw`: answers from context, no tools, runs concurrently with the main turn, not added to history; `f` forks it into a background sub-agent with full tools. | GA | https://code.claude.com/docs/en/interactive-mode.md | reviewed (review-1) |
| Claude Desktop | Side chat (Cmd/Ctrl+; or `/btw`): read-only, not saved to disk. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Oh My Pi | `/btw` (side question, not saved); `/tan` (tangential work in a background agent). | GA | https://omp.sh/docs/slash | reviewed |
| Hermes Agent | `/bg` runs a task in a separate session and posts results back; `/btw` is listed but not described. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging | unreviewed |
| Others | Nothing documented in these areas. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Orchestration modes and trigger keywords
What it is: a mode or a typed keyword that makes the main agent decompose the task and delegate, instead of the model deciding case by case.
Variants: one-turn keywords (OMP `orchestrate`/`workflowz`, Claude `ultracode`), a persistent mode (OMP vibe: a read-only director plus `fast`/`good` workers; Claude `/effort ultracode`), or a command that proposes a breakdown the user must approve (Warp `/orchestrate`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | The `ultracode` keyword, or `/effort ultracode` (Claude plans a workflow for every substantive task; also bypasses the 20-sub-agent concurrency cap). The keyword is ignored in `-p`, unstamped SDK input, scheduled prompts, webhooks and PR comments. | GA | https://code.claude.com/docs/en/workflows.md | reviewed |
| Oh My Pi | Magic keywords `orchestrate` (decompose, delegate, verify) and `workflowz` (deterministic multi-agent workflow), each toggleable. Vibe mode: the session becomes a read-oriented director coordinating persistent `fast` (small model) and `good` (strong model) workers on one working tree. | GA | https://omp.sh/docs/magic-keywords, https://omp.sh/docs/vibe | reviewed |
| Warp | `/orchestrate`, or `/plan` with an orchestration config block, proposes a breakdown; children start only after user approval (or "Run orchestrated agents: Always allow"). | GA (status is the survey's inference) | https://docs.warp.dev/platform/orchestration/, https://docs.warp.dev/agents/cli/cloud-and-orchestration/ | corrected |
| Cline | `/team` prefix creates a coordinator plus teammates (see teams). | GA/limited | https://docs.cline.bot/cli/agent-teams.md | unreviewed |
| Antigravity | `/boost` (orchestrator, worktree-isolated sub-agents, synthesis) and `/teamwork` (see teams). | GA (paid) | https://antigravity.google/docs/boost | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | The `ultracode` orchestrator agent on `main` (`delegates: coder, scout, writer`; 4 sub-agents / 200k tokens), replaced on `feat/agent-roster` by "Senior Teamlead". | works | `crates/agents/src/lib.rs:50-56`, `crates/agents/builtin/*.md` | from own inventory |

---

### Area 9. Sub-agents and multi-agent orchestration

#### Sub-agent delegation tool
What it is: a model-callable tool that starts a child agent on a task and gives the parent its result.
Variants:
- **Name and shape:** Codex `spawn_agent`/`send_input`/`resume_agent`/`wait_agent`/`close_agent`, OpenCode `subagent`, Claude `Agent` (formerly `Task`), OMP `task`, OpenHands `task`, Hermes `delegate_task` (batch `tasks`), OpenClaw `sessions_spawn`, Zed and Cline `spawn_agent`, Antigravity `invoke_subagent`/`define_subagent`, goose `delegate`, our `delegate`.
- **Sync vs async:** OpenHands is synchronous (parallelism comes from the tool concurrency limit), Claude and OMP background by default, goose sequential by default.
- **Where approval lives inside the child:** see the tools and permissions entry.
- **Deliberate refusal:** Pi ships no sub-agents; it spawns itself through bash so output stays visible.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Spawns parallel agents when asked or when AGENTS.md or a skill requests it, waits and consolidates. Tools `spawn_agent`, `send_input`, `resume_agent`, `wait_agent`, `close_agent`. `/subagents` switches threads; the IDE has a background-agent panel. Docs warn about token use and parallel write-heavy work. | GA (on by default) | https://learn.chatgpt.com/docs/agent-configuration/subagents.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed (workflows), reviewed (tools via custom agents entry) |
| OpenCodex | Runs no sub-agents itself. It rewrites Codex's model catalog (`multi_agent_version` v1/base/v2) to choose the collaboration surface and which models (up to five) Codex offers for `spawn_agent`. | GA | https://opencodex.me/guides/sub-agent-surface/ | reviewed |
| OpenCode | The `subagent` tool starts a child session (foreground or `background`, resumable by `sessionID`). The parent's `subagent` permission controls which agents can be launched. | GA | https://opencode.ai/docs/tools/, https://opencode.ai/docs/agents/ | reviewed |
| Claude Code | The `Agent` tool (renamed from `Task` in v2.1.63; `Task(...)` still works). Claude delegates broad searches to Explore and forks side tasks when fork mode is on. Invocation: natural language, `@agent-<name>`, or `claude --agent` for the whole session. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/best-practices.md | reviewed |
| Claude Desktop | Cowork divides work into parallel workstreams through sub-agents; plugins can bundle agents (loaded in Cowork and Code). | GA | https://claude.com/docs/cowork/overview.md, https://claude.com/docs/plugins/platform-support.md | unreviewed |
| DeepSeek harness | The `subagent` tool gives a child a durable child session with optional `cwd` and optional model. Running children take steering at the next step boundary; idle ones start a turn; absent ones cold-resume. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | reviewed |
| Oh My Pi | The `task` tool: the main session decides the fan-out and writes each assignment. Background by default (`async.enabled`). Setting "Prefer Task Delegation": default/preferred/always. | GA | https://omp.sh/docs/subagents | reviewed |
| Pi | Documented non-feature. An example extension runs each sub-agent as a separate `pi` process (modes single, parallel with at most 8 tasks and 4 concurrent, and chain with `{previous}`). The experimental pi-durable has child tasks. | GA (omission; example) / experimental | https://pi.dev/, https://github.com/earendil-works/pi/tree/main/packages/coding-agent/examples/extensions/subagent, https://github.com/earendil-works/pi/blob/main/packages/durable/README.md | reviewed (non-feature), corrected (pi-durable) |
| Cursor | The Task tool; parallel through several Task calls; cloud sub-agents get their own VM and branch. | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | Sub-agents start with a clean context in their own conversation and run async in parallel; `define_subagent` at runtime. States Running, Idle (auto-wakes), Killed. SDK: `enable_subagents=True` for self-cloning, or a static `SubagentConfig`. | GA | https://antigravity.google/docs/subagents, https://antigravity.google/docs/sdk/subagents | reviewed |
| OpenHands | The `task` tool (prompt, optional `subagent_type`, `resume`). A TaskManager runs a fresh sub-agent conversation to completion. **Synchronous**; parallelism comes from the parent's `tool_concurrency_limit` (default 1). DelegateTool is "retired". Canvas: `launch_child_conversation` on a local or Cloud target. | GA | https://docs.openhands.dev/sdk/guides/task-tool-set.md, https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | corrected (wording on DelegateTool) |
| OpenClaw | `sessions_spawn` starts background runs, each in its own session, announcing results to the requester; `sessions_yield` ends the turn to await results; hidden children vs `visible: true` interactive sessions. | GA | https://docs.openclaw.ai/tools/subagents, https://docs.openclaw.ai/concepts/session-tool | reviewed |
| goose | `delegate` (Summon extension), on request or on goose's own judgement, **only in autonomous mode**. Sequential by default; parallel when the prompt says so. | GA | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | `delegate_task` spawns isolated children; batch through `tasks`, optional `output_schema` and images; control actions list, steer, stop. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Zed/ACP | Zed: `spawn_agent` starts a sub-agent with its own context window and the parent's tools. ACP: sub-agent sessions are only a draft RFD (observable, not promptable, cancel only, behind `unstable_subagents`). | GA (Zed, inferred); planned (ACP) | https://zed.dev/docs/ai/tools.md, https://agentclientprotocol.com/rfds/subagents.md | reviewed |
| Warp | Orchestration: a parent plus children, each an independent run with its own transcript and usage. | GA (inferred) | https://docs.warp.dev/platform/orchestration/ | corrected |
| Cline | `spawn_agent`: its own instructions and context window, parallel, nesting allowed, no MCP tools, on by default. **CLI and Desktop only, not VS Code/JetBrains.** | GA | https://docs.cline.bot/features/subagents.md | reviewed |
| rizzy-harness (ours) | `delegate(agent, task)` is synthesised by the loop for agents with `delegates`. It starts a child session (`parent_id`) on the same node with the same approval mode and returns the final answer. Several `delegate` calls in one step run concurrently (`join_all`). | works | `crates/agent-native/src/lib.rs:167-216`, `:233-317`, `:796-801`, `crates/node/src/session.rs:654-763` | from own inventory |

#### Built-in agent roster
What it is: the named agents a harness ships, what each is for, and where it may run.
Variants: most harnesses ship a small roster split into a **read-only explorer**, a **planner**, and a **general worker** (Codex, OpenCode, Claude Code, Cursor). OMP ships a role-tagged roster tied to model roles (`@smol`, `@slow`). OpenCode ships **hidden maintenance agents** (title, summary, compaction) that are really model slots. Several harnesses ship no named built-ins at all (DeepSeek, OpenHands, OpenClaw, goose, Hermes, Cline, Pi) and rely on custom definitions or one generic sub-agent.

The full comparison is the **roster table** below; this entry's per-harness rows point to it.

**Roster table.** "Where" says whether the agent can be the main/primary agent or is a sub-agent only. "Model" gives the documented default tier; "inherit" means it uses the parent's or main model.

| Harness | Agent | What it is for | Where | Read-only | Model (default) | Evidence | Check |
|---|---|---|---|---|---|---|---|
| Codex | `default` | purpose not described in the survey | sub-agent role | not stated | `[agents]` default sub-agent model and effort | https://learn.chatgpt.com/docs/agent-configuration/subagents.md | reviewed |
| Codex | `worker` | not described in the survey (name implies execution) | sub-agent role | not stated | as above | same | reviewed |
| Codex | `explorer` | not described in the survey (name implies exploration) | sub-agent role | not stated | as above | same | reviewed |
| OpenCode | `build` | default full-access agent | primary | no | user's model | https://opencode.ai/docs/agents/ | reviewed |
| OpenCode | `plan` | explore without editing project files | primary | edits denied except under `~/.opencode/plan` | user's model | https://opencode.ai/docs/agents/, https://opencode.ai/docs/permissions/ | reviewed |
| OpenCode | `general` | research and multi-step work; cannot launch sub-agents | sub-agent | no | inherits the parent's if unset | https://opencode.ai/docs/agents/ | reviewed |
| OpenCode | `explore` | read-only exploration | sub-agent | yes | inherits the parent's if unset | same | reviewed |
| OpenCode | `compaction`, `title`, `summary` | hidden maintenance agents, not selectable; `title`/`summary` deny all permissions | system | yes (deny all) | `agents.title.model` replaces the old `small_model` | https://opencode.ai/docs/agents/, https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | Explore | broad search; Claude passes quick / medium / very thorough; no CLAUDE.md or git status; one-shot (cannot be resumed) | sub-agent | yes (Write/Edit denied) | main model, capped at Opus (the `opus` alias when the main model is Fable) | https://code.claude.com/docs/en/sub-agents.md | reviewed |
| Claude Code | Plan | research during plan mode; same startup limits as Explore | sub-agent | yes | not stated | same | reviewed |
| Claude Code | general-purpose | all sub-agent tools; the default when the Agent call names no type | sub-agent | no | inherit | same | reviewed |
| Claude Code | `claude` | catch-all; default agent for dispatched background sessions | sub-agent and background session | no | inherit | same | reviewed |
| Claude Code | statusline-setup | configure the status line | sub-agent | not stated | Sonnet | same | reviewed |
| Claude Code | claude-code-guide | answer questions about Claude Code | sub-agent | not stated | Haiku | same | reviewed |
| Claude Code | fork (`subagent_type: "fork"`) | a copy of the conversation sharing the parent's cache; not a named agent but a spawn mode | sub-agent | no | main model | https://code.claude.com/docs/en/agent-sdk/subagents.md | reviewed |
| Claude Desktop | same as Claude Code (Code tab); Cowork sub-agents are unnamed | — | — | — | — | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | no named agents; tool presets `minimal`/`standard`/`cordis`/`ptc` can be given to a child (`agentPreset`) | — | — | — | optional model selection, off by default | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | reviewed |
| Oh My Pi | `scout` | read-only exploration | sub-agent (task) | yes | `@smol` | https://omp.sh/docs/subagents, https://omp.sh/docs/agents-and-roles | reviewed |
| Oh My Pi | `designer` | design work | sub-agent | not stated | `@designer` | same | reviewed |
| Oh My Pi | `reviewer` | review | sub-agent | not stated | `@slow` | same | reviewed |
| Oh My Pi | `security-reviewer` | security review | sub-agent | yes | no role: session model fallback | same | reviewed |
| Oh My Pi | `librarian` | research on external libraries | sub-agent | not stated | `@smol` | same | reviewed |
| Oh My Pi | `task` | general worker | sub-agent | no | `@task` | same | reviewed |
| Oh My Pi | `sonic` | mechanical edits | sub-agent | no | `@smol` | same | reviewed |
| Oh My Pi | vibe workers `fast`, `good` | persistent workers under the vibe director | workers | no | small model / strong model | https://omp.sh/docs/vibe | reviewed |
| Pi | none (deliberate) | — | — | — | the example extension inherits the parent's model | https://pi.dev/ | reviewed |
| Cursor | Explore | parallel searches | sub-agent | not stated | "faster model" | https://cursor.com/docs/subagents | reviewed |
| Cursor | Browser | browser work; filters noisy DOM output | sub-agent | not stated | not stated | https://cursor.com/docs/subagents, https://cursor.com/docs/agent/tools/browser | reviewed |
| Antigravity | `browser` | hidden built-in that drives the user's own Chrome over the DevTools MCP | sub-agent (hidden) | no | not stated | https://antigravity.google/docs/commands/browser | reviewed |
| Antigravity | `/teamwork` roles: Sentinel, Orchestrator, Explorers, Workers, Critic, Challenger, Auditor, Success Auditor | roles in a team run; Explorers read-only; Workers own files exclusively; the Critic is adversarial | team members | Explorers yes | not stated | https://antigravity.google/docs/teamwork | corrected |
| Antigravity | `/swarm` `@verifier`, `@monitor` | the verifier (about one per 2 to 4 workers) sends problems back to the author; the monitor stays idle | swarm members | not stated | not stated | https://antigravity.google/docs/swarm | reviewed |
| OpenHands | no named built-ins documented (`subagent_type` is optional) | — | — | — | — | https://docs.openhands.dev/sdk/guides/task-tool-set.md | corrected |
| OpenClaw | no named roster; one built-in runtime `openclaw` | — | — | — | `agents.defaults.subagents.model` | https://docs.openclaw.ai/tools/subagents | reviewed |
| goose | none documented | — | — | — | `GOOSE_SUBAGENT_MODEL` | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | leaf vs `orchestrator` role in delegation | an orchestrator may delegate (depth 2); leaves cannot | sub-agent roles | no | per-child override | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Zed/ACP | profiles Write, Ask, Minimal | main-agent tool profiles, not agents | primary | Ask yes; Minimal has no project tools | profile `default_model` | https://zed.dev/docs/ai/agent-profiles.md | unreviewed |
| Warp | Factories: foreman plus default triage, planning, implementation and review agents | the foreman talks to people and coordinates; never merges | cloud team | not stated | configurable per agent | https://docs.warp.dev/factories/ | corrected (status Early Access) |
| Cline | none named; Plan/Act modes and a generic `spawn_agent` | — | — | — | — | https://docs.cline.bot/features/subagents.md | reviewed |
| rizzy-harness (ours, `main`) | `general` | all tools | primary | no | per-agent model | `crates/agents/src/lib.rs:50-56` | from own inventory |
| rizzy-harness (ours, `main`) | `coder`, `writer` | coding, writing | primary and delegate target | no | per-agent | `crates/agents/builtin/*.md` | from own inventory |
| rizzy-harness (ours, `main`) | `scout` | read-only | primary and delegate target | yes | per-agent | same | from own inventory |
| rizzy-harness (ours, `main`) | `ultracode` | orchestrator; delegates to coder, scout, writer; 4 sub-agents / 200k tokens | primary | no | per-agent | same | from own inventory |
| rizzy-harness (ours, `feat/agent-roster`, unmerged) | Assistant (`general`, alias `ai`), Smol, Researcher, Software Developer (`coder`), Senior Teamlead, Writer, Scout, Reviewer | Assistant: web plus `remember`, no files; Smol: no tools, no memory; Researcher: web; Software Developer: all file/shell plus web and `remember`, delegates to scout/researcher/reviewer; Teamlead: orchestrator (4 / 200k); Scout and Reviewer: read-only | Scout is sub-agent only (server-enforced through `available_as`); the others are not stated | Scout, Reviewer yes | per-agent | `crates/agents/builtin/*.md` (branch), `crates/master/src/resolver.rs:214` (branch) | from own inventory |

Per-harness rows for this feature: see the roster table above (one or more rows per harness, each with its own evidence).

#### Custom agent definition files
What it is: user- or project-authored agent definitions (prompt, tools, model, limits).
Variants:
- **Format:** Markdown with YAML frontmatter is the norm (OpenCode, Claude Code, OMP, Cursor, Antigravity, OpenHands, goose, Pi's example, ours). Codex alone uses TOML; OpenCode also takes JSONC.
- **Cross-reading other harnesses:** Cursor reads `.claude/agents/` and `.codex/agents/`; goose reads `.claude/agents/`; OMP deliberately does not.
- **Unknown fields:** Claude Code silently ignores them; we reject them.
- **Widening permissions:** Codex agents cannot override sandbox, approval, provider or MCP; Claude plugin agents ignore `hooks`/`mcpServers`/`permissionMode`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | TOML in `~/.codex/agents/` or `.codex/agents/`. Required: `name`, `description`, `developer_instructions`. Optional: model, effort, summary, verbosity. Can disable `features.shell_tool`, apps, plugins, memories, `request_permissions_tool` and skills. **Cannot override `sandbox_mode`, `approval_policy`, `model_provider`, `mcp_servers`.** `agents.<name>.config_file` points at a config layer. | GA | https://learn.chatgpt.com/docs/agent-configuration/subagents.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed |
| OpenCode | Markdown in `.opencode/agents/` or `~/.config/opencode/agents/` (nested paths become ids) or JSONC under `agents`. Options: description, model (`provider/model#variant`), system, hidden, color, disabled, request, permissions, steps, `default_agent`. | GA | https://opencode.ai/docs/agents/, https://opencode.ai/docs/migrate-v1/ | reviewed |
| Claude Code | Markdown with YAML; only `name` and `description` are required. Priority: managed, `--agents` JSON, project `.claude/agents/` (walking up), user, plugin. Fields: `tools`, `disallowedTools`, `model`, `permissionMode`, `maxTurns`, `autoCompactWindow`, `skills` (up to 32), `mcpServers`, `hooks`, `memory`, `background`, `omitClaudeMd`, `effort`, `isolation: worktree`, `color`, `initialPrompt`, `experimental.cacheTtl`. **Unknown fields are silently ignored**; files with bad YAML are skipped silently. Plugin agents ignore `hooks`, `mcpServers`, `permissionMode`. Hot reload. SDK `AgentDefinition` is the programmatic form. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/agent-sdk/subagents.md | reviewed |
| Claude Desktop | Plugins can bundle agents (Cowork and Code; ignored in Chat). | GA | https://claude.com/docs/plugins/platform-support.md | unreviewed |
| DeepSeek harness | `agentPreset` picks a composition for a child; no definition file format is described. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | unreviewed |
| Oh My Pi | Markdown in `.omp/agents/` or `~/.omp/agent/agents/`. Fields: name, description, tools, model (concrete, `@role` or ordered list), thinking-level, spawns, autoload-skills, read-summarize, output (JSON Schema), blocking, prewalk, advisor. Project beats user beats bundled; **`.claude/agents` is not scanned**; `omp agents unpack` exports the bundled agents. | GA | https://omp.sh/docs/subagent-authoring, https://omp.sh/docs/agents-and-roles | unreviewed |
| Pi | The example extension uses Markdown in `~/.pi/agent/agents/` or `.pi/agents/` (project scope is opt-in). | GA (example) | https://github.com/earendil-works/pi/tree/main/packages/coding-agent/examples/extensions/subagent | unreviewed |
| Cursor | Markdown with `name`, `description`, `model` (default `inherit`), `readonly`, `is_background`, read from `.cursor/agents/`, **`.claude/agents/`, `.codex/agents/`** (project and home). | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | Markdown in `.agents/agents/`, `~/.gemini/config/agents/` or plugins. Fields: `name`, `description`, `tools`, `mainAgent`, `subagent`, `model` (`inherit|flash|pro`), `commandExecutionPolicy` (`off|auto|eager|sandbox`, default `sandbox`), `mcpServers`, `skills`, `plugins`. A misspelled tool name may hang the sub-agent. | GA | https://antigravity.google/docs/subagents | reviewed |
| OpenHands | Markdown in `.agents/agents/` or `.openhands/agents/`. Fields: name, description, tools, model (profile or `inherit`), skills, `max_iteration_per_run`, color, `mcp_servers`, hooks, `permission_mode` (always/never/confirm_risky; omitted means the parent's), `profile_store_dir`. | GA | https://docs.openhands.dev/sdk/guides/agent-file-based.md | unreviewed |
| OpenClaw | Multiple isolated agents per Gateway, each with workspace, `agentDir` and its own SQLite store; bindings route by channel or peer. No sub-agent file format is described. | GA | https://docs.openclaw.ai/concepts/multi-agent | unreviewed |
| goose | Markdown (`name` required, `description`, `model`) in `~/.agents/agents/` or `<project>/.agents/agents/`; also reads `.goose/agents/` and **`.claude/agents/`**. Cannot define servers or be scheduled. Invoked by `@name`, delegation or load. | unclear | https://goose-docs.ai/docs/guides/context-engineering/custom-agents.md | unreviewed |
| Hermes Agent | Profiles: each is a separate Hermes home (config, `.env`, SOUL, memories, skills, cron). Profiles do not sandbox. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/profiles | unreviewed |
| Zed/ACP | Custom profiles under `agent.profiles` (tools, MCP, `default_model`). No sub-agent definitions. | GA | https://zed.dev/docs/ai/agent-profiles.md | unreviewed |
| Warp | Factories: custom agents, with models and harnesses configurable; definitions can live as files in a customer repo, reviewed by PR. | preview (Early Access) | https://docs.warp.dev/factories/ | corrected |
| Cline | Nothing documented as agent files. | nothing documented | — | — |
| rizzy-harness (ours) | Markdown with YAML: `name`, `description`, `tools: [..]|all`, `model`, `effort`, `budget` (stub), `ui`, `delegates`, `max_subagents`, `max_subagent_tokens`, `memory`, `project_memory`, `available_as`; **unknown fields rejected**. Files in `<data>/agents/<id>.md` (a built-in's id replaces it); admins can create agents in the UI. Branch adds `aliases`, `category`. | works | `crates/agents/src/lib.rs:1-33`, `:74-103`, `crates/master/src/agent_edits.rs:1-12` | from own inventory |

#### Sub-agent context: fresh, forked or inherited
What it is: what the child sees when it starts.
Variants:
- **Fresh with a written assignment:** Codex, OpenCode, OMP, Cursor, Antigravity, OpenHands, Hermes, goose sub-recipes, ours.
- **Fresh plus project instructions and an environment snapshot:** Claude Code non-fork sub-agents load the CLAUDE.md hierarchy and git status but never history.
- **Forked:** full history and a shared prompt cache (Claude Code forks and `/subtask`, OpenClaw `context: "fork"`).
- **Durable child sessions** that can be resumed or steered later (DeepSeek, OpenCode `sessionID`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Not described beyond threads per sub-agent. | — | https://learn.chatgpt.com/docs/agent-configuration/subagents.md | unreviewed |
| OpenCode | Sub-agents start with fresh context. | GA | https://opencode.ai/docs/agents/ | reviewed |
| Claude Code | Non-fork: its own system prompt plus environment, the delegation message, the full CLAUDE.md/AGENTS.md hierarchy, a git status snapshot, preloaded skills, and a sibling roster if it has SendMessage. **Never inherited:** history, invoked skills, files read, output style, auto memory. `omitClaudeMd` drops user/project CLAUDE.md. Forks inherit the full conversation, system prompt, tools and model and share the parent's prompt cache. | GA | https://code.claude.com/docs/en/sub-agents.md | reviewed |
| DeepSeek harness | A durable child session; cold-resume of absent children. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | reviewed |
| Oh My Pi | Workers get no parent conversation; the main session writes each assignment. | GA | https://omp.sh/docs/subagents | reviewed |
| Cursor | Each runs in its own clean context. | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | Clean context in its own conversation. | GA | https://antigravity.google/docs/subagents | reviewed |
| OpenHands | A fresh sub-agent conversation; saved and resumable. | GA | https://docs.openhands.dev/sdk/guides/task-tool-set.md | corrected |
| OpenClaw | Its own session by default; `context: "fork"` shares the parent transcript. | GA | https://docs.openclaw.ai/tools/subagents | reviewed |
| goose | Isolated sub-agents; sub-recipes run in their own session with no shared history. | GA / experimental | https://goose-docs.ai/docs/guides/context-engineering/subagents.md, https://goose-docs.ai/docs/guides/recipes/subrecipes.md | reviewed |
| Hermes Agent | A fresh conversation with only `goal`, `context` and workspace context files. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Zed/ACP | Its own context window. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| Warp | An independent run with its own transcript. | GA | https://docs.warp.dev/platform/orchestration/ | corrected |
| Cline | Its own instructions and context window. | GA | https://docs.cline.bot/features/subagents.md | reviewed |
| Pi | The example extension runs a separate process with its own context; chain mode passes `{previous}`. | GA (example) | https://github.com/earendil-works/pi/tree/main/packages/coding-agent/examples/extensions/subagent | unreviewed |
| rizzy-harness (ours) | A child session with the task text; not documented further in the inventory. On `integration/gate3`, sub-agents inherit the parent's web-fetch provenance set. | works | `crates/agent-native/src/lib.rs:233-317`, `crates/web/src/lib.rs:39-72` (branch) | from own inventory |

#### Sub-agent tools and permissions
What it is: which tools a child gets and whose approval rules apply to it.
Variants: this is the security-relevant split.
- **Child inherits the parent's approval settings and cannot widen them; approvals surface to the human:** Codex, Antigravity (approvals bubble up), Claude Code (auto mode reviews at spawn, per action and at completion).
- **Approving the spawn approves everything the child does:** Cline (approval settings do not apply inside), OMP (sub-agents run tier decisions as yolo but still respect deny entries).
- **Child uses only its own permissions:** OpenCode; the parent's `subagent` permission decides only who may be launched.
- **Hard tool blocklists:** Hermes (no `clarify`, `memory`, `send_message`, `cronjob`), goose (no spawning, extension changes or schedules), Cline (no MCP), Claude (no AskUserQuestion, plan tools, ScheduleWakeup and more).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Inherits the parent turn's sandbox and approval settings, including live `/permissions` and `--yolo` changes. Approvals from inactive threads appear in the CLI overlay with the source label. In non-interactive runs a needed approval fails back to the parent. | GA | https://learn.chatgpt.com/docs/agent-configuration/subagents.md | reviewed |
| OpenCode | The parent's `subagent` permission controls which agents can launch; **the child uses its own permissions, not its parent's**. Agent rules append after global rules. | GA | https://opencode.ai/docs/agents/, https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | Sub-agents inherit built-in and MCP tools, then two filters apply. **Removed from every sub-agent:** `Agent` at the depth limit, AskUserQuestion, EndConversation, Enter/ExitPlanMode, ScheduleWakeup, WaitForMcpServers, Workflow. **Background sub-agents** keep a fixed built-in subset. So one definition can resolve to different tool sets in foreground and background, with no error. Auto mode's classifier checks at spawn, per action (frontmatter `permissionMode` ignored) and on the final report. Inline MCP servers and hooks from project agents load only after that folder is trusted. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/permission-modes.md | reviewed |
| DeepSeek harness | Permission inheritance is not described; child agents cannot ask the user or schedule. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent, https://deepseek-harness.github.io/deepseek-harness/en/guide/schedule | reviewed |
| Oh My Pi | Sub-agents run tier decisions as yolo (approving the parent's `task` call is the boundary) but inherit the `tools.approval` map, where deny blocks and prompt rejects. Workers cannot pause for approval. | GA | https://omp.sh/docs/approvals, https://omp.sh/docs/subagents | reviewed |
| Cursor | `readonly` frontmatter flag. | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | A `tools` allowlist plus per-agent `commandExecutionPolicy` (default `sandbox`). Inherits the parent's allowed command prefixes, file scopes and sandbox; **approvals bubble up**. SDK static sub-agent tools must also be listed in the parent. | GA | https://antigravity.google/docs/subagents, https://antigravity.google/docs/sdk/subagents | reviewed |
| OpenHands | `permission_mode` per agent file (omitted means the parent's). The docs state no filesystem sandboxing between parent and child. | GA | https://docs.openhands.dev/sdk/guides/agent-file-based.md, https://docs.openhands.dev/sdk/guides/task-tool-set.md | corrected |
| OpenClaw | No session or message tools by default; per-agent sandbox and tool policy; cross-agent session access is on by default (`tools.agentToAgent`, visibility `all`). | GA | https://docs.openclaw.ai/tools/subagents, https://docs.openclaw.ai/concepts/multi-agent | reviewed |
| goose | Inherits the parent's extensions by default and can be restricted; cannot spawn sub-agents, change extensions or manage schedules; disabled outside autonomous mode. | GA | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | Inherits toolsets; cannot use `clarify`, `memory`, `send_message`, `cronjob`; leaves cannot delegate. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Zed/ACP | Same tools as the parent. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| Warp | The CLI shows an approval card before children start; the "Run orchestrated agents" permission can be Always allow or Never. | GA | https://docs.warp.dev/agents/cli/cloud-and-orchestration/ | unreviewed |
| Cline | No MCP tools. **Approving the `spawn_agent` call approves everything the sub-agent does;** normal approval settings do not apply inside. | GA | https://docs.cline.bot/features/subagents.md | reviewed |
| rizzy-harness (ours) | The child runs with the **same approval mode** as the parent; approval happens inside the child. The agent `tools:` list narrows what is offered, and node policy still decides execution. Sub-agent replies are capped to display-level `ui:` blocks. | works | `crates/node/src/session.rs:654-763`, `crates/agent-native/src/lib.rs:176`, `crates/core/src/ui/mod.rs:132-145` | from own inventory |

#### Sub-agent model and effort resolution
What it is: how the child's model and effort are chosen.
Variants: an ordered resolution chain (Claude Code: per-call parameter, then frontmatter, then env var, then main model; plus a FORCE override and allowlist substitution), role aliases (OMP `@role`, Antigravity `flash|pro`), a single global sub-agent model setting (Zed `agent.subagent_model`, goose `GOOSE_SUBAGENT_MODEL`, OpenClaw `agents.defaults.subagents.model`, Codex `[agents]`), or plain inherit-by-default (OpenCode, Cursor, Pi example). Fallback for sub-agents: Claude applies its fallback chain to sub-agents; OpenCodex gives spawned workers a three-step fallback order.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `[agents]` sets the default sub-agent model and effort and `max_concurrent_threads_per_session`; a custom agent can set model and effort. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed |
| OpenCodex | Worker fallback order: requested model, then the per-model chain, then the global chain. The "preferred worker model" is guidance injected for the main agent, not a router. In v2, a ChatGPT-native parent handing a task to a routed child fails with `unreadable_encrypted_agent_task`. | GA | https://opencodex.me/guides/sub-agent-surface/ | reviewed |
| OpenCode | `model` in the definition; a sub-agent with no model inherits the parent's. | GA | https://opencode.ai/docs/agents/ | reviewed |
| Claude Code | Order: (1) per-call `model` (aliases only), (2) frontmatter `model` (`inherit` = main), (3) `CLAUDE_CODE_SUBAGENT_MODEL`, (4) main model. `CLAUDE_CODE_SUBAGENT_MODEL_FORCE=1` applies one model to all sub-agents, teammates and workflow agents. `availableModels` blocks are substituted with a warning. Sub-agents inherit thinking on/off; no per-sub-agent thinking. Effort: env, then per-call, then frontmatter, then session (only if same model). The fallback chain applies to sub-agents. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/model-config.md | reviewed |
| DeepSeek harness | Optional model selection, off by default (per the tool catalogue). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | reviewed |
| Oh My Pi | The agent's model is concrete, an `@role` alias or an ordered list; agent → role → model chain; `task.agentModelOverrides`. | GA | https://omp.sh/docs/agents-and-roles, https://omp.sh/docs/subagent-authoring | reviewed |
| Cursor | `model` (default `inherit`); on legacy request-based plans without Max Mode, sub-agents use Composer regardless. | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | `model: inherit|flash|pro`. | GA | https://antigravity.google/docs/subagents | reviewed |
| OpenHands | `model`: an LLM profile name or `inherit`. | GA | https://docs.openhands.dev/sdk/guides/agent-file-based.md | unreviewed |
| OpenClaw | `agents.defaults.subagents.model`. | GA | https://docs.openclaw.ai/tools/subagents | reviewed |
| goose | `GOOSE_SUBAGENT_PROVIDER` / `GOOSE_SUBAGENT_MODEL`. | GA | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | Per-child model/provider overrides. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Zed/ACP | `agent.subagent_model` (provider, model, effort, `enable_thinking` default false, `speed`); unset means inherit. | GA | https://zed.dev/docs/ai/agent-settings.md | reviewed |
| Warp | Children may use other harnesses and models. | GA | https://docs.warp.dev/platform/orchestration/ | corrected |
| Cline | Not documented for sub-agents. | nothing documented | — | — |
| rizzy-harness (ours) | Per-agent `model` and `effort` in the definition; per-agent fallbacks in settings. Whether a delegate uses its own agent's model is not stated in the inventory. | works | `crates/agents/src/lib.rs:1-33`, `crates/master/src/agent_edits.rs:1-12` | from own inventory |

#### Result return and output hygiene
What it is: what comes back to the parent and how it is checked.
Variants:
- **What returns:** only the final summary in most harnesses. goose lets the caller choose full or summary.
- **Structured output:** a JSON Schema (OMP `output`, Hermes `output_schema`, DeepSeek structured value plus a bounded diagnostic).
- **Partial results:** returned on turn caps (Claude `maxTurns`; OMP hard partial at 1.5× the soft budget).
- **Scanning before the parent reads the report:** Claude Code rewrites text that imitates harness control tags, prepends warnings, and has the auto-mode classifier review the final report.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Only the final result returns. The output is marked partial at `maxTurns` and can be resumed. Background results arrive as an automated completion notification in a later turn. The auto-mode classifier reviews the final report, and flagged reports get a security warning. The harness scans the final message for instruction-shaped patterns and prepends a `[harness: …]` marker (v2.1.210+; a review-2 omission). | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/agent-sdk/subagents.md | reviewed |
| DeepSeek harness | Output, an optional structured value, a diagnostic (at most 4096 bytes, which must exclude tool inputs, file contents and credentials), and a stop reason. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | reviewed |
| Oh My Pi | Structured results only through the agent's `output` JSON Schema (the README's "schema-validated typed results" is broader than the docs). | GA | https://omp.sh/docs/subagent-authoring | reviewed |
| Cursor | Only a final summary; background output is written to `~/.cursor/subagents/`. | GA | https://cursor.com/docs/subagents | reviewed |
| OpenHands | Status and text. | GA | https://docs.openhands.dev/sdk/guides/task-tool-set.md | corrected |
| OpenClaw | Results are announced to the requester. | GA | https://docs.openclaw.ai/tools/subagents | reviewed |
| goose | Return mode full or summary; failed or timed-out sub-agents return nothing. | GA | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | Only the final summary; optional `output_schema`; stall monitoring with a budget warning and force-finalize. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Cline | Only the final reply. | GA | https://docs.cline.bot/features/subagents.md | reviewed |
| Others | Codex consolidates results; OpenCode notifies the parent; Antigravity, Zed and Warp are not described. | — | — | — |
| rizzy-harness (ours) | The final answer; sub-agent answers cite in the parent's numbering. | works | `crates/agent-native/src/lib.rs:310-314` | from own inventory |

#### Background sub-agents and resumption
What it is: children that run while the parent continues, and children that can be continued later.
Variants: background by default (Claude Code since 2.1.198, OMP `async.enabled`, OpenClaw), opt-in (OpenCode `background: true`, Cursor `is_background`, Claude frontmatter), or synchronous (OpenHands, ours). Resume by id: OpenCode `sessionID`, OpenHands `resume`, Cursor by id, DeepSeek cold-resume, Claude (not Explore/Plan), goose since v1.56. goose removed "background subagents and orchestrator" in v1.55.0.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Archiving or deleting a thread cascades to its spawned descendants; `SubagentStart`/`SubagentStop` hooks. | GA | https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/hooks.md | unreviewed |
| OpenCode | `background: true` returns immediately and notifies the parent; `sessionID` continues a prior child. | GA | https://opencode.ai/docs/tools/ | reviewed |
| Claude Code | Sub-agents start in the background by default. While fork mode is on (the default from v2.1.302), Claude cannot wait for them interactively; `CLAUDE_CODE_FORK_SUBAGENT=0` lets it wait. Resume needs the `agentId`; Explore and Plan are one-shot. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/agent-sdk/subagents.md | reviewed |
| DeepSeek harness | A running child takes steering at the next step; an idle one starts a turn; an absent one cold-resumes. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | reviewed |
| Oh My Pi | Async by default; the `wait` tool collects results; idle workers are parked after 7 minutes and can be revived from the Agent Hub; isolated workers cannot be revived. | GA | https://omp.sh/docs/subagents | reviewed |
| Cursor | Foreground blocks the parent; background writes to `~/.cursor/subagents/` and resumes by id. | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | Async; the Idle state auto-wakes. | GA | https://antigravity.google/docs/subagents | reviewed |
| OpenHands | Synchronous; resume by prior task id. | GA | https://docs.openhands.dev/sdk/guides/task-tool-set.md | corrected |
| OpenClaw | Background runs; `sessions_yield`. | GA | https://docs.openclaw.ai/tools/subagents | reviewed |
| goose | Sequential by default; resume added in v1.56.0 and resume of completed delegates in v1.59.0; v1.55.0 removed background sub-agents. | GA | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Zed/ACP | Sub-agent threads persist (release notes fix restore hangs). | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| Warp | A child in a terminal state can still receive follow-up messages. | GA | https://docs.warp.dev/platform/orchestration/ | corrected |
| Others | Hermes (steer/stop), Cline (parallel): resumption not described. | — | — | — |
| rizzy-harness (ours) | Synchronous within the parent's turn; no background sub-agents, no resume. | none | inventory §7 "Autonomous background agents" | from own inventory |

#### Depth, concurrency and spend limits
What it is: caps on nesting depth, parallel children and cost.
Variants: the spread is wide. Depth ranges from 1 (OpenCode, Cursor, Warp, goose, Hermes, ours) through 2 (OMP), 3 (Claude) and 10 (Antigravity) to unlimited (Cline, OpenHands: none documented). Concurrency defaults to 5 (goose), 8 (OpenClaw), 10 (Hermes), 20 (Claude) or 32 (OMP). Budget caps are rare: Claude `maxBudgetUsd`, OMP request budget, ours count plus tokens per turn.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `max_concurrent_threads_per_session` (no default documented). | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed |
| OpenCode | Nesting depth one by default (`experimental.subagent_depth`). | GA (flag experimental) | https://opencode.ai/docs/config/ | reviewed |
| Claude Code | Depth 3 layers below main (`CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`; 1 disables nesting); 20 concurrent (`CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`; spawn 21 fails with "do not retry"; ultracode exempt; `/subtask` forks can exceed it); no cap on total spawns (a 200 cap was added in 2.1.212 and removed in 2.1.224); SDK `maxBudgetUsd`; stall watchdog. Workflows: 16 concurrent (1-256), 1,000 agents per run. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/agent-sdk/subagents.md, https://code.claude.com/docs/en/workflows.md | reviewed |
| DeepSeek harness | `maxDepth`; workflows have a per-run `maxTotalAgents` that the script cannot see or change; background jobs at most 10 per owner session. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow | reviewed |
| Oh My Pi | `task.maxConcurrency` 32, `maxRecursionDepth` 2, `softRequestBudget` 200 (hard partial at 1.5×), `maxRuntimeMs` 0 (none). | GA | https://omp.sh/docs/subagents | reviewed |
| Pi | Example extension: at most 8 parallel tasks, 4 concurrent. | GA (example) | https://github.com/earendil-works/pi/tree/main/packages/coding-agent/examples/extensions/subagent | unreviewed |
| Cursor | One level of nesting since 2.5 (main plus direct sub-agents may launch). | GA | https://cursor.com/docs/subagents | reviewed |
| Antigravity | Depth at most 10, "strictly enforced"; no documented cap on concurrency, timeout or budget. | GA | https://antigravity.google/docs/subagents | reviewed |
| OpenHands | Docs state no nesting depth, count or timeout limits. | GA | https://docs.openhands.dev/sdk/guides/task-tool-set.md | corrected |
| OpenClaw | Depth configurable (numbers on an unopened subpage); default concurrency 8 per spawning session. Group threads: at most 4 rounds and 32 participant turns; bot-loop guard 20 events per 60 s. | GA | https://docs.openclaw.ai/concepts/queue, https://docs.openclaw.ai/channels/bot-loop-protection | reviewed |
| goose | 5 concurrent (`GOOSE_MAX_CONCURRENT_SUBAGENTS`), 5-minute timeout, 1000 max turns, no recursion. | GA | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | 10 concurrent children, depth 1 (2 for `orchestrator`), 250 iterations (the agent-loop page says 50: a contradiction). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | reviewed |
| Zed/ACP | No numeric limits documented. | — | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Warp | Exactly one level deep; no limits on children or concurrency documented. | GA | https://docs.warp.dev/platform/orchestration/ | corrected |
| Cline | No documented caps on count, depth or timeout. | GA | https://docs.cline.bot/features/subagents.md | reviewed |
| rizzy-harness (ours) | Depth 1 (children have no delegates). Per turn: sub-agent count (default 4) and combined tokens (default 200,000), refused with a cap message; tokens are checked between steps, so a step can overshoot. | works | `crates/core/src/agent.rs:114-130`, `crates/agent-native/src/lib.rs:750-775` | from own inventory |

#### Isolation for sub-agents (worktrees, overlays, VMs)
What it is: giving a writing child its own copy of the code so parallel edits do not collide.
Variants:
- **Git worktree per child:** Claude Code `isolation: worktree` (with four enforcement checks), Antigravity `branch`, Hermes opt-in, Cursor best-of-n, Cline Kanban, ours.
- **Copy-on-write clone or overlay, merged back as a patch:** OMP (reflinks, overlayfs, projfs; patch apply). Ours also merges back as a patch, but from a worktree.
- **Cloud VM per child:** Cursor cloud sub-agents.
- **Shared tree by design:** OMP vibe workers, Claude agent teams ("partition files instead"), Antigravity `share`.

Claude and we both refuse repos whose git config defines filter drivers. Claude also neutralises LFS, `includeIf` and replace objects.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Managed worktree tools (behind a feature); scheduled tasks may run in a background worktree. Per-sub-agent isolation is not described. | GA (behind a feature) | https://learn.chatgpt.com/docs/changelog | unreviewed |
| OpenCode | First-class worktree tools and API with pluggable strategies; `opencode run -w`. Per-sub-agent use is not described. | GA | https://opencode.ai/docs/config/ | unreviewed |
| Claude Code | `isolation: worktree` (frontmatter, or per call for forks) gives each sub-agent a temporary worktree on `baseRef` (the default branch unless `head`). It is removed if unchanged; a changed one is swept after `cleanupPeriodDays`; `git worktree lock` is held while the agent runs. Enforcement: edits and Bash cwd into the main checkout are blocked, git redirects are blocked, unverifiable commands are refused; non-git writes (`cp`) are not tracked. Filter drivers and LFS custom transfers are refused. Agent teams have **no** worktree isolation. | GA | https://code.claude.com/docs/en/worktrees.md, https://code.claude.com/docs/en/agent-teams.md | reviewed |
| DeepSeek harness | `create_worktree` switches the session into a new worktree; per-child use is not described. | experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | `task.isolation.mode` (default none; auto picks APFS/btrfs/zfs reflinks, overlayfs, projfs or rcopy). Changes are applied to the parent as patches. Needs git; unavailable in plan mode; **not a security sandbox**; isolated workers cannot be revived. | GA | https://omp.sh/docs/subagents | unreviewed |
| Cursor | Cloud sub-agents get their own VM and branch; `/best-of-n` gives each model its own worktree. | GA | https://cursor.com/docs/subagents, https://cursor.com/docs/configuration/worktrees | reviewed |
| Antigravity | Workspace modes `inherit`, `branch` (an isolated git worktree removed on kill), `share`; `/boost` uses ephemeral isolated worktrees. | GA | https://antigravity.google/docs/subagents, https://antigravity.google/docs/boost | reviewed |
| OpenHands | Canvas local children run in an isolated worktree or share the parent's workspace. The SDK task tool has no filesystem sandboxing between parent and child. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md, https://docs.openhands.dev/sdk/guides/task-tool-set.md | unreviewed |
| OpenClaw | Managed worktrees per agent task, snapshotted to `refs/openclaw/snapshots/<id>` before removal (kept 30 days). | GA | https://docs.openclaw.ai/concepts/managed-worktrees | reviewed |
| Hermes Agent | `delegation.worktree_isolation` (default false): a worktree and branch `hermes-subagent/subagent-<id>` per child. Local backend git repos only; **otherwise the workspace is silently shared**. | GA, opt-in | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation, https://hermes-agent.nousresearch.com/docs/user-guide/git-worktrees | unreviewed |
| Warp | Docs advise one worktree and branch per agent (advice deferred to a separate guide). | — | https://docs.warp.dev/platform/orchestration/ | corrected (not on the cited pages) |
| Cline | Kanban cards run in ephemeral worktrees, with gitignored dirs (e.g. `node_modules`) **symlinked from the main repo**, so edits there hit the main copy. | preview | https://docs.cline.bot/usage/kanban.md | unreviewed |
| Zed/ACP | Threads can run in separate worktrees (title-bar picker); archiving saves the worktree state. | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| rizzy-harness (ours) | A sub-agent that can write/edit/bash in a git repo gets its own worktree seeded with the working state. Changes are applied back as a patch; conflicts are reported, not applied. Refused if the repo's config defines filter commands. Residual: git-lfs filters still run (PLAN T19). | works | `crates/node/src/session.rs:765-856`, `crates/node/src/git.rs:861-960` | from own inventory |

#### Agent teams, swarms and shared boards
What it is: several long-lived agents with names that coordinate through a shared task list, mailbox or board, rather than one parent with throwaway children.
Variants:
- **Coordination medium:**
  - per-agent mailbox files plus a shared task list with dependencies and file locks (Claude Code teams);
  - a durable peer inbox plus a task DAG with compare-and-set updates (DeepSeek);
  - one public message board with no private messages (Antigravity `/swarm`);
  - a SQLite kanban with an OS process per worker and a dispatcher that reclaims crashed claims (Hermes);
  - a board, mailbox and mission log (Cline);
  - the Workboard dispatch loop (OpenClaw).
- **Maturity:** almost all are experimental or preview, and their own docs say a single agent is often cheaper (Cline) or that the lead sometimes does the work itself (Claude).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Off by default (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`); interactive only. A lead spawns teammates, each a full Claude Code instance with its own context. Messaging by name through JSON mailbox files under `~/.claude/teams/{team}/inboxes/`; no broadcast. A shared task list with three states and dependencies, claimed under file locks. Display in-process or in tmux/iTerm2 split panes. Hooks `TeammateIdle`, `TaskCreated`, `TaskCompleted`. Limits: no resume or rewind of in-process teammates, one team per session, no nesting, fixed lead, **no worktree isolation**, no hard size cap (3 to 5 suggested). With teams on, a named sub-agent launches as a teammate without confirmation. Teammates start in the lead's mode, including bypass (review-2 omission). | experimental | https://code.claude.com/docs/en/agent-teams.md | reviewed |
| Claude Desktop | Agent teams are CLI-only, not in Desktop (review omission). | — | https://code.claude.com/docs/en/desktop | reviewed |
| DeepSeek harness | A Lead plus named durable teammates with direct peer inbox delivery and a shared task DAG (`blockedBy`, compare-and-set claim/release/reassign). `write_scopes` are **advisory path prefixes, not locks**. Off by default in new sessions. | experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog, https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/agent-team.md | corrected |
| Oh My Pi | Vibe workers share one working tree (see coordinators). | GA | https://omp.sh/docs/vibe | reviewed |
| Antigravity | `/swarm`: a coordinator creates named workers plus `@verifier` and `@monitor`; they coordinate **only through one shared board** (`@name/@group/@all`). Quota exhaustion pauses a member with no retry; killed workers cannot be revived; no merge process. `/teamwork`: roles and three artifacts over "multiple days", a fresh successor orchestrator between milestones, integrity modes (`benchmark` forbids mock passes). | preview (swarm, Pro/Ultra); GA paid (teamwork) | https://antigravity.google/docs/swarm, https://antigravity.google/docs/teamwork | reviewed (swarm), corrected (teamwork) |
| OpenClaw | The Workboard plugin: cards in nine statuses; a dispatch loop starts sub-agent workers (3 per pass by default, one per owner); "proof" statuses are self-reports. | unclear | https://docs.openclaw.ai/cli/workboard | unreviewed |
| Hermes Agent | Kanban: a durable `kanban.db` shared across profiles. Each worker is a separate OS process spawned by a dispatcher in the gateway (about 60 s tick). It reclaims crashed claims, auto-blocks after 2 spawn failures, and treats a clean exit without completion as `protocol_violation`. Eight states, parent/child dependencies, worktree workspaces, PR completion contracts. REST routes are unauthenticated behind localhost. Bot Mode rooms of 2 to 6 profile bots, hop limit 4. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban, https://hermes-agent.nousresearch.com/docs/user-guide/bot-mode | reviewed |
| Warp | Factories: a team of cloud agents attached to repos (foreman plus triage, planning, implementation, review), with human checkpoints on spec, code and product; never merges. | preview (Early Access) | https://docs.warp.dev/factories/ | corrected |
| Cline | `/team` (or the model's own decision): a coordinator plus teammates sharing a task board, mailbox and mission log in `~/.cline/data/db/teams.db`; resume with `cline --id`; not with `--yolo`; SDK/CLI/Kanban only. Kanban board: each card runs a CLI agent (Cline, Claude Code, Codex, OpenCode) with dependency chains. | GA/limited (teams); preview (Kanban) | https://docs.cline.bot/cli/agent-teams.md, https://docs.cline.bot/usage/kanban.md | unreviewed |
| Others | Codex, OpenCode, Cursor, goose, Zed, Pi, OpenHands: nothing documented. | — | — | — |
| rizzy-harness (ours) | None. Projects have kanban boards, but they are user-edited and agents cannot write them. | none | inventory §c "Todo/task tool" | from own inventory |

#### Cross-session messaging and message buses
What it is: agents or sessions sending messages to each other outside a parent/child call.
Variants:
- **Transport:**
  - Claude Code: a per-session Unix socket or named pipe on the same machine; cross-machine messages go through Anthropic.
  - Warp: a server-backed durable inbox per agent, across harnesses and locations.
  - OpenClaw: group threads with coordinator limits and a bot-loop guard.
  - DeepSeek and Claude teams: inbox files.
- **Trust:** only Claude Code documents a receiver-side model. Peer messages cannot answer permission prompts or justify config changes; `crossSessionInbound` (accept/hold/refuse) holds messages by default when the receiver bypasses permissions; loop protection caps queued and repeated messages.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `ListAgents` / `SendMessage` reach sub-agents, teammates, other local sessions, cloud sessions and Remote Control sessions. Text only, never history or files. Receiver trust rules plus `crossSessionInbound` and `dialogExpiry` (5 min). The inbox socket is restricted to the OS user, with a per-session token. `isolatePeerMachines` requires approval for anything leaving the machine. Loop protection: about 1M characters per message, 50 queued, repeats dropped. `notify_when_idle` sends one idle/exit notice. | GA | https://code.claude.com/docs/en/cross-session-messaging.md | reviewed |
| Claude Desktop | A desktop session can list, read, message, rename and archive other desktop sessions (local, SSH, WSL; not cloud or CLI); archive needs approval except in Auto/Bypass; admin key `crossSessionInbound`. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Team peer inboxes. | experimental | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/agent-team.md | corrected |
| Oh My Pi | `wait` collects background results and peer messages; the Agent Hub can steer running workers. | GA | https://omp.sh/docs/subagents | reviewed |
| Antigravity | The swarm message board. | preview | https://antigravity.google/docs/swarm | reviewed |
| OpenClaw | Cross-agent session access on by default (`tools.agentToAgent`); group-thread limits; bot-to-bot loop guard. | GA | https://docs.openclaw.ai/concepts/multi-agent, https://docs.openclaw.ai/channels/bot-loop-protection | unreviewed |
| Hermes Agent | Bot Mode rooms; A2A (see peering). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/bot-mode | unreviewed |
| Warp | A durable, server-backed message bus with per-agent inboxes across harnesses and locations; lifecycle events avoid polling. | GA | https://docs.warp.dev/platform/orchestration/multi-agent-runs/ | reviewed (bus) |
| Cline | Team mailbox. | GA/limited | https://docs.cline.bot/cli/agent-teams.md | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Script-orchestrated workflows
What it is: the model writes a program (or a declarative recipe) that fans work out to many agents, so intermediate results stay out of the model's context.
Variants:
- **Model-written JavaScript in a sandbox with an agent/pipeline/parallel API:** Claude Code (no filesystem; nondeterministic APIs throw so runs replay by prompt order) and DeepSeek (a near-identical API that runs in a Node PTC process under the caller's file policy, with a hidden `maxTotalAgents`).
- **Declarative sub-recipes:** goose.
- **A keyword:** OMP `workflowz`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `agent(prompt,{schema,label,stallMs})`, `pipeline`, `parallel`, `phase`, `log`, `args`. No filesystem, shell or `import()`; `Date.now()` and `Math.random()` throw so replays stay deterministic. Schema retries 5 times. 16 concurrent by default (1-256), 4,096 items per call, 1,000 agents per run; agents are held for a shared cache prefix (5 s stagger). Approval depends on mode. Size guideline is advice only. Saved under `.claude/workflows/`. | GA (Pro opt-in) | https://code.claude.com/docs/en/workflows.md | reviewed |
| DeepSeek harness | The `workflow` tool takes a JavaScript script with JSON `meta` (`agent`, `pipeline`, `parallel`, `phase`, `log`). It runs in a Node PTC process under the calling session's file policy. `maxTotalAgents` per run cannot be changed by the script. No overall deadline. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow | reviewed |
| Oh My Pi | The `workflowz` keyword: a deterministic multi-agent workflow (needs sub-agents and a persistent-evaluation capability). | GA | https://omp.sh/docs/magic-keywords | reviewed |
| goose | Recipe `sub_recipes` become tools; each runs in its own session; no nesting. | experimental | https://goose-docs.ai/docs/guides/recipes/subrecipes.md | reviewed |
| OpenHands | "Dynamic Agent Workflows" and "Iterative Refinement" SDK guides exist (index only). | unclear | https://docs.openhands.dev/llms.txt | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Running other vendors' harnesses as sub-agents or runtimes
What it is: driving Claude Code, Codex, Gemini CLI, OpenCode and others as children or as the agent runtime.
Variants:
- **Through ACP over stdio:** OpenHands (Canvas spawns the CLI as a subprocess), OpenClaw (`/acp spawn` via acpx), goose (ACP providers that map goose's permission modes onto theirs), Zed (external agents).
- **Through their own SDK or app-server:** DeepSeek's Codex/Claude Code backends (one execution handle each; follow-ups rejected), OpenClaw's bundled `codex` runtime.
- **As cloud children with their own provider auth:** Warp `--harness claude|codex`.
- **As terminal processes in a board:** Cline Kanban, and Warp hosting third-party CLIs.
- **By rewriting the client's model catalog:** OpenCodex.

A recurring documented gap: approvals for hosted agents are undocumented (OpenHands) or limited.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Controls Codex's sub-agent surface and the models offered for `spawn_agent`. Cross-provider delegation demos are README claims. | GA (demos vendor-claim) | https://opencodex.me/guides/sub-agent-surface/ | reviewed |
| DeepSeek harness | Sub-agent backends: Codex, Claude Code, ACP and the DSH SDK, each a single execution handle (follow-ups rejected); separate npm bundles. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent, https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | reviewed |
| OpenHands | Canvas runs Claude Code, Codex, Gemini CLI, Pi, OpenCode or any stdio ACP server as the agent. The Agent Server spawns the CLI as a subprocess and relays turns; credentials are resolved server-side by secret name at spawn. Approvals for ACP agents are undocumented. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/acp-agents.md | reviewed |
| OpenClaw | `/acp spawn` launches Codex, Claude Code, Gemini CLI, OpenCode, Cursor and Droid through acpx. The runtime layer also runs `codex` (app-server) and CLI backends (`claude-cli`). Once a harness starts, failures are not retried on another runtime. | GA/partial | https://docs.openclaw.ai/cli/acp, https://docs.openclaw.ai/concepts/agent-runtimes | unreviewed |
| goose | ACP providers drive Claude/Codex/Pi/Amp while mapping goose's permission modes onto theirs; external sub-agents (e.g. Codex as an MCP server) configured under `subagent`. | unclear (external sub-agents) | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Zed/ACP | External Agents (an ACP process with its own auth and config) and Terminal Threads (native CLIs in a terminal) run beside the Zed Agent; the ACP list has 51 agents, many through adapters. | GA | https://zed.dev/docs/ai/agents.md, https://agentclientprotocol.com/get-started/agents.md | reviewed |
| Warp | Orchestration children can use another harness (Claude Code, Codex). `oz agent run-cloud --harness oz|claude|codex`: Claude Code and Codex call their providers directly using auth secrets. The terminal hosts third-party CLI agents with notifications. | GA | https://docs.warp.dev/platform/orchestration/, https://docs.warp.dev/agents/cli/oz-cli/ | reviewed |
| Cline | Kanban cards run Cline CLI, Claude Code, Codex or OpenCode in their own worktree and terminal. | preview | https://docs.cline.bot/usage/kanban.md | unreviewed |
| Others | Codex, OpenCode, Claude Code, OMP, Pi, Cursor, Antigravity, Hermes: nothing documented in these areas (Hermes lists a "Codex app-server runtime" page, unopened). | — | — | — |
| rizzy-harness (ours) | The `AgentBackend` trait allows external harnesses, but only `NativeBackend` exists. | partial | `crates/agent-native/src/lib.rs:537-541` | from own inventory |

#### Coordinator agents that delegate and do not code
What it is: a long-running top-level agent that routes work into separate worker sessions or threads and watches them.
Variants: cloud coordinators with per-thread branches and PR auto-fix (Claude Code/Desktop Projects, Cursor Projects), a foreman over a cloud team (Warp Factories), a local read-oriented director (OMP vibe), a project orchestrator with successors (Antigravity `/teamwork`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code / Claude Desktop | Projects: one coordinator conversation answers quickly or starts/reuses threads (cloud sessions with their own context and branch) that open PRs and auto-fix. A thread can run on the user's machine through Remote Control. Standing context: project memory and instructions (16,000 chars). Defaults: Opus everywhere, high effort for threads, low for the coordinator; threads in auto mode. 500 new threads per day. | public beta | https://code.claude.com/docs/en/claude-projects.md, https://code.claude.com/docs/en/claude-projects | reviewed |
| Oh My Pi | Vibe mode: a read-oriented director coordinating `fast` and `good` workers; the director must verify; no start/fork/handoff while active. | GA | https://omp.sh/docs/vibe | reviewed |
| Cursor | A Project in the Agents Window: a coordinator agent plans and delegates to cloud agents without writing code; subscriptions (Slack channel, schedule). | unclear | https://cursor.com/docs/agent/projects | unreviewed |
| Antigravity | `/teamwork` Project Orchestrator; handoff to a fresh successor between milestones. | GA (paid) | https://antigravity.google/docs/teamwork | corrected |
| Warp | Factories foreman. | preview | https://docs.warp.dev/factories/ | corrected |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | `ultracode` / Senior Teamlead is an orchestrator agent within one chat turn; not a long-running coordinator. | partial | `crates/agents/builtin/*.md` | from own inventory |

#### Built-in parallel fan-out commands
What it is: a shipped command that runs one task as many parallel agents (best-of-n, review fleets, split-change).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/batch` splits one change into 5 to 30 worktree-isolated sub-agents. `/code-review ultra` runs a cloud fleet of reviewer agents ($5 to $25 after 3 free runs; Claude never starts it on its own). `/deep-research` is the bundled workflow. | GA (`/batch`); research preview (ultrareview) | https://code.claude.com/docs/en/agents.md, https://code.claude.com/docs/en/ultrareview.md | reviewed |
| Oh My Pi | `/review` with 1 to 16 parallel reviewer slices; `/cleanse` fixes diagnostics with parallel workers. | GA | https://omp.sh/docs/review, https://omp.sh/docs/slash | reviewed |
| Cursor | `/multitask` (async sub-agents in parallel); `/best-of-n` (one task across several models, each in its own worktree; comparing does not merge). | GA | https://cursor.com/help/ai-features/multi-agent, https://cursor.com/docs/configuration/worktrees | unreviewed |
| Antigravity | `/boost`: orchestrator, then sub-agents in ephemeral worktrees, then synthesis with iteration on failures. | GA (paid) | https://antigravity.google/docs/boost | unreviewed |
| Others | Nothing documented in these areas. | — | — | — |
| rizzy-harness (ours) | Parallel answers: the same prompt to up to 4 models, no tools, user picks one (chat only, not agentic). | works | `crates/agent-native/src/lib.rs:930-1030` | from own inventory |

#### Advisor and watchdog models
What it is: a second model that watches or is consulted during the main agent's work and injects guidance.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | A server-side advisor tool that Claude calls when it chooses. The advisor sees the full conversation; it must rank at or above the main model, otherwise it is not attached (silently, if the API refuses the pairing). Sub-agents inherit it. Its read is never cached. Anthropic API only. | experimental | https://code.claude.com/docs/en/advisor.md | reviewed |
| Oh My Pi | An `advisor` role model watches each turn in the background with read/grep/glob. It injects nit/concern/blocker notes; blockers can interrupt; `advisor.immuneTurns` 3. `WATCHDOG.yml` defines a roster of specialist advisors, which can be granted edit/bash. | GA | https://omp.sh/docs/advisor | reviewed |
| OpenClaw | An advisor model reviews work and can send one correction; a Decision model for "decision assistance"; both off by default. | experimental | https://docs.openclaw.ai/concepts/experimental-features | unreviewed |
| OpenHands | The Critic model scores success (see goal loops). | experimental | https://docs.openhands.dev/openhands/usage/agent-canvas/critic.md | unreviewed |
| Hermes Agent | Mixture of Agents presets: reference models run first without tools and an aggregator writes the response. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mixture-of-agents | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Agent-to-agent peering protocols
What it is: calling an independently deployed agent as a peer over a protocol, not as a child.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Cursor | Agent SDK peers reached through MCP tools `ask`, `check` and `call_tool`; routing is one-way and declared in the caller's instructions. | unclear | https://cursor.com/docs/agent-sdk/guides/agent-to-agent | unreviewed |
| Hermes Agent | A2A v1.0 plugin both ways: agent card, JSON-RPC on port 9900, SSE, HMAC-signed push; localhost-only without a token; per-peer tokens, 60/min rate limit, audit log. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging/a2a | unreviewed |
| OpenClaw | The delegate architecture: a named agent with its own organisational identity working under standing orders, in three tiers. | GA (guidance) | https://docs.openclaw.ai/concepts/delegate-architecture | unreviewed |
| Zed/ACP | Sub-agent sessions RFD (draft). | planned | https://agentclientprotocol.com/rfds/subagents.md | reviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

---

### Model roles and slots for auxiliary work (cross-cutting; mostly Area 14)

#### Auxiliary model slots
What it is: named model assignments for work other than the main turn (titles, summaries and compaction, vision, goal judging, commit messages, planning, sub-agents, approval classifiers).
Variants:
- **A role table with aliases and per-role fallback chains:** OMP has ten roles, `@role` aliases, ordered alternatives, and auto-resolution from authenticated models.
- **Per-task auxiliary settings, each with its own fallback chain:** Hermes.
- **Per-feature model keys:** Zed.
- **Hidden agents that act as slots:** OpenCode `title`, `summary` and `compaction`.
- **One "small/fast" slot shared by every background task:** Claude Code. Setting `ANTHROPIC_DEFAULT_HAIKU_MODEL` changes the goal evaluator, titles **and** the `haiku` alias at once.
- **Sidecars that replace a capability for weaker models:** OpenCodex's vision and web-search sidecars.
- **Default when a slot is unset:** usually the main model (Hermes, Zed sub-agent, OpenCode sub-agent, Claude row summaries). OMP auto-resolves from authenticated models but the advisor never inherits the default. Claude on Bedrock/Vertex defaults the small slot to Sonnet.

**Slot table (by harness).**

| Harness | Named slots or roles | Default when unset | Slot-specific fallback | Evidence | Check |
|---|---|---|---|---|---|
| Codex | `review_model` (code review); `[agents]` default sub-agent model and effort; `plan_mode_reasoning_effort` (effort only). | main model | not documented | https://learn.chatgpt.com/docs/code-review.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCodex | Vision sidecar: describes images for text-only models (default backend Anthropic if an OAuth account is usable, else OpenAI; 3 at a time, 8 per turn, cached). Web-search sidecar backend model (OpenAI, Anthropic, xAI, Gemini, Exa; up to 3 searches per turn). Sub-agent worker model. | as stated | worker: requested, then per-model chain, then global chain; combos fail over before output | https://opencodex.me/guides/sidecars/, https://opencodex.me/guides/sub-agent-surface/, https://opencodex.me/guides/combos/ | reviewed |
| OpenCode | Hidden agents `title`, `summary`, `compaction`; `agents.title.model` (replaces V1 `small_model`); per-agent model. | sub-agents inherit the parent's | not documented | https://opencode.ai/docs/agents/ | reviewed |
| Claude Code | **Small/fast (Haiku-class) background model:** session titles, `/goal` evaluator, agent-view row summaries and session names. Set with `ANTHROPIC_DEFAULT_HAIKU_MODEL` (which also changes the `haiku` alias). **Advisor model** (`advisorModel`). **Sub-agent model** (`CLAUDE_CODE_SUBAGENT_MODEL`, FORCE). **Workflow sub-agent model** (`CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL`, changelog only). **`opusplan`** (Opus in plan mode, Sonnet for execution). WebFetch extraction runs "a separate model call" (model unstated). | Small slot: Haiku on the Anthropic API; **Sonnet on Bedrock/Vertex**; the primary model on Foundry; the main model through an Anthropic-format gateway. Row summaries fall back to the main model if no Haiku-class model is configured. | Session fallback chain (at most 3 models, turn-scoped, also applies to sub-agents); content-classifier fallback | https://code.claude.com/docs/en/goal.md, https://code.claude.com/docs/en/sessions.md, https://code.claude.com/docs/en/agent-view.md, https://code.claude.com/docs/en/amazon-bedrock.md, https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/advisor.md, https://code.claude.com/docs/en/tools-reference.md | unreviewed (small model), reviewed (advisor, fallback, sub-agent) |
| Claude Desktop | Per-session model picker; per-task model on local scheduled tasks; Projects defaults (Opus; coordinator low effort, threads high); Research uses Opus 5.5 with an Opus 4.8 fallback. | — | Research: Opus 4.8 | https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/desktop-scheduled-tasks | reviewed |
| DeepSeek harness | Optional sub-agent model selection (off by default); titles are generated by an LLM from the first prompt (model not stated). | — | not documented | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-client | unreviewed |
| Oh My Pi | **Ten roles:** default, smol, slow, vision, plan, designer, commit, tiny, task, advisor (the README says nine). Selectors take ordered alternatives, `@role` aliases and thinking suffixes. `omp commit` uses the `commit` role while its agent uses `smol`. `compactionModel` in `models.yml`. Prewalk switches to `smol`. | Unassigned roles auto-resolve from authenticated models; **advisor does not inherit default** | `retry.fallbackChains` keyed by role, exact model, `provider/*` or prefix; `fallbackRevertPolicy`; opt-in usage-aware fallback | https://omp.sh/docs/roles, https://omp.sh/docs/agents-and-roles, https://omp.sh/docs/commit, https://omp.sh/docs/custom-models | reviewed |
| Pi | None. `/scoped-models` and `--models` set the cycle list. | — | — | https://pi.dev/docs/latest/cli | unreviewed |
| Cursor | Explore uses a "faster model"; sub-agent `model` (default inherit); Auto router (Cost/Balance/Intelligence; classifier router on Teams/Enterprise). | inherit | not documented | https://cursor.com/docs/subagents, https://cursor.com/docs/cursor-router | reviewed (sub-agents) |
| Antigravity | Sub-agent `model: inherit|flash|pro`; Automations model tier Flash Lite / Flash (default) / Pro. | inherit; Flash for automations | not documented | https://antigravity.google/docs/subagents, https://antigravity.google/docs/features | reviewed (sub-agents) |
| OpenHands | LLM profiles; a separate judge LLM for `/goal`; Critic model (default hosted critic "currently" free); LLM summarising condenser (model unstated); Model Routing and Task-Based Model Routing guides (index only). | — | SDK documents fallback (page unopened) | https://docs.openhands.dev/sdk/guides/convo-goal.md, https://docs.openhands.dev/openhands/usage/agent-canvas/critic.md, https://docs.openhands.dev/sdk/arch/condenser.md | reviewed (goal) |
| OpenClaw | `agents.defaults.subagents.model`; advisor and decision models (experimental); `agents.defaults.model.fallbacks`. | — | Auth-profile rotation first, then the fallback chain; a user-chosen model is strict | https://docs.openclaw.ai/tools/subagents, https://docs.openclaw.ai/concepts/experimental-features, https://docs.openclaw.ai/concepts/model-failover | unreviewed (fallback) |
| goose | `GOOSE_SUBAGENT_PROVIDER/_MODEL`; recipe `settings.goose_model`. **No lead/worker and no fallback documented.** | — | none documented (docs point to Tetrate/OpenRouter) | https://goose-docs.ai/docs/guides/multi-model/index.md | unreviewed |
| Hermes Agent | **Auxiliary models per task:** compression, vision, summaries, goal judge, curator, background review (memory/skills; "cheaper model with an input-token cap"), smart approval; per-child delegation model; Mixture-of-Agents aggregator. The compression auxiliary model must be at least as large as the main model (section 5). | **main model** | **Separate fallback chains for auxiliary tasks** (compression, vision, titles, approval) besides the turn-scoped primary chain | https://hermes-agent.nousresearch.com/docs/integrations/providers, https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers, https://hermes-agent.nousresearch.com/docs/user-guide/features/memory | unreviewed |
| Zed/ACP | Per-feature models: `inline_assistant_model`, `commit_message_model` (+ `commit_message_instructions`; uses the 3 latest commits as style examples), `thread_summary_model`, `compaction_model`, `subagent_model`. | `subagent_model` inherits; others not stated | not documented | https://zed.dev/docs/ai/agent-settings.md, https://zed.dev/docs/ai/llm-providers.md | reviewed (subagent), unreviewed (others) |
| Warp | Profile base model, also used for planning unless a separate planning model is set; custom model routers (YAML, by complexity or natural-language rules); Auto models. | base model | "Fallback routing switches to a comparable model during outages" | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/, https://docs.warp.dev/agents/inference/custom-routers/, https://docs.warp.dev/agents/inference/model-choice/ | unreviewed |
| Cline | Optional separate model per Plan and Act mode. | same model | not documented | https://docs.cline.bot/core-workflows/plan-and-act.md | reviewed |
| rizzy-harness (ours) | Per-agent model and per-agent ordered fallbacks; `[agent_models]` in `config.toml`. History summarisation exists, but the inventory does not say which model runs it. No title, vision-bridge or judge slot in the inventory. The `feat/agent-roster` branch adds a "Smol" **agent** (no tools, no memory), not a slot. | main/agent model | Per-agent chain: tried when unavailable or when it fails before output with 404/408/429/5xx/connect; not mid-stream; recorded as `model_fallback` | `crates/agent-native/src/lib.rs:503-534`, `:640-701`, `:570-600`, `crates/master/src/config.rs:1-420` | from own inventory |

**Slot table (by slot).** Y means documented and released, B beta/experimental, - nothing documented. Harnesses with nothing in any slot are left out (Pi, Cline except planning, goose except sub-agent).

| Slot | Codex | OpenCodex | OpenCode | Claude Code | DeepSeek | OMP | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Title / session name | - | - | Y | Y (small/fast) | Y (model unstated) | - | - | - | - | - | - | Y (aux, titles chain) | Y (`thread_summary_model`) | - | - | - |
| Summary / compaction | - | - | Y | - (review: unstated) | - | Y (`compactionModel`) | - | - | Y (condenser LLM) | - | - | Y | Y (`compaction_model`) | - | - | unstated |
| Vision bridge for text-only models | - | Y | - | - | - | Y (`vision` role) | - | - | - | - | - | Y (aux vision) | - | - | - | - |
| Goal / completion judge | - | - | ? (reviewer undocumented) | Y (small/fast) | - | - | - | - | Y (judge LLM), B (critic) | - | - | Y | - | - | - | - |
| Commit message | - | - | - | - | - | Y (`commit`) | - | - | - | - | - | - | Y | - | - | - |
| Planning | effort only | - | - | Y (`opusplan`) | - | Y (`plan`) | - | - | - | - | - | - | - | Y | Y (per mode) | - |
| Sub-agent default | Y | Y | Y | Y | B | Y (`task`, `smol`) | Y | Y | Y | Y | Y | Y | Y | Y | - | per agent |
| Approval / safety classifier | other area | - | - | other area | - | - | - | - | - | other area | - | Y (aux approval) | - | - | - | - |
| Advisor / watcher | - | - | - | B | - | Y | - | - | - | B | - | - | - | - | - | - |
| Code review | Y (`review_model`) | - | - | - | - | Y (`slow` via reviewer) | - | - | - | - | - | - | - | - | - | - |
| Status-line / row summaries | - | - | - | B (research preview) | - | - | - | - | - | - | - | - | - | - | - | - |
| Background memory / skill review | - | - | - | - | - | - | - | - | - | - | - | Y (aux review, curator) | - | - | - | - |
| Per-slot fallback chain | - | Y | - | - (session chain only) | - | Y | - | - | - | - | - | Y | - | - | - | - |

#### Fallback chains for the main model
What it is: an ordered list of models (or accounts) to try when the primary fails.
Variants:
- **Scope:** turn-only (Claude Code, Hermes), sticky for up to 30 minutes after a rate-limit fallback (OpenClaw), reverting on cooldown expiry (OMP).
- **Triggers:** Claude Code never falls back on auth, billing, rate-limit or request-size errors. Hermes falls back on 429/5xx (after retries) and immediately on 401/403/404. Ours: 404/408/429/5xx/connect before any output. OpenCodex combos fail over only before output.
- **Order:** credential pools are rotated before switching models in OpenClaw, OpenCodex and Hermes.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Combos: failover (default), weighted round-robin, random, least-used, reset-window, `jev`. Hop errors cool the target. Tool calls stay on the issuing target for the turn. Streaming fails over only before output. | GA | https://opencodex.me/guides/combos/ | reviewed |
| Claude Code | `--fallback-model a,b` / `fallbackModel`: at most 3, current turn only, never on auth/billing/rate-limit/size/policy errors. Will not fall back to a smaller-context model during compaction. Covers sub-agents. Not shown in `/status`. | GA | https://code.claude.com/docs/en/model-config.md | reviewed |
| Oh My Pi | `retry.fallbackChains` (by role, model, provider or prefix); `fallbackRevertPolicy` cooldown-expiry; usage-aware fallback (off by default). | GA | https://omp.sh/docs/roles | reviewed |
| OpenClaw | Rotate auth profiles, then walk `agents.defaults.model.fallbacks`. Backoff 30 s, 1 m, 5 m. A user-chosen model is strict. A working fallback is preferred for 30 minutes after a rate limit. | GA | https://docs.openclaw.ai/concepts/model-failover | unreviewed |
| Hermes Agent | Turn-scoped `fallback_providers`; credential pools first; separate auxiliary chains. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers | unreviewed |
| Warp | Fallback routing during outages. | GA | https://docs.warp.dev/agents/inference/model-choice/ | unreviewed |
| goose | None documented (docs recommend Tetrate/OpenRouter). | nothing documented | https://goose-docs.ai/docs/guides/multi-model/index.md | unreviewed |
| Others | Covered by the models group. | — | — | — |
| rizzy-harness (ours) | Per-agent ordered fallbacks before any output; `model_fallback` event; no retry/backoff. | works | `crates/agent-native/src/lib.rs:503-534`, `:640-701`, `:693-701` | from own inventory |

#### Phase-based model switching
What it is: changing model by phase of the task (plan with a strong model, execute with a cheaper one).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `plan_mode_reasoning_effort` (effort, not model). | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| Claude Code | `opusplan`: `opus` in plan mode, `sonnet` for execution, each phase with its family's window. A Haiku session upgrades to Sonnet in plan mode. | GA | https://code.claude.com/docs/en/model-config.md | unreviewed |
| Oh My Pi | The `plan` role for planning, with the implementation role chosen at approval. Prewalk: a strong model investigates, writes todos and makes the first edit, then the session switches once to a cheaper model (default `smol`). | GA | https://omp.sh/docs/plan, https://omp.sh/docs/prewalk | reviewed |
| Warp | Separate planning model in the profile. | GA | https://docs.warp.dev/agents/capabilities/planning/ | unreviewed |
| Cline | Optional separate model per Plan/Act mode. | GA | https://docs.cline.bot/core-workflows/plan-and-act.md | reviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None (model is per agent or per message). | none | `crates/master/src/resolver.rs:1-10` | from own inventory |

---

### Area 10. Background, long-running and scheduled work

#### Background shell jobs
What it is: long-running commands (dev servers, watchers) that run while the agent continues, with output the agent can poll.
Variants: an output file read with ordinary tools (Claude Code), a bounded ring buffer with a consuming cursor (DeepSeek: 256 KiB live, 16 KiB settled, 10 per owner), resumable process polling (Codex `write_stdin`), or a line stream pushed into the conversation (Claude `Monitor`, Antigravity sidecars). Pi refuses on principle (use tmux).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Background terminals: `/ps` lists them with recent output; `/stop` stops all; the API can list, clean and terminate (experimental). Unified exec: `write_stdin` polls a running command. | GA (CLI); experimental (API) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCode | Background shells with live output and a stop control (desktop activity pill). | GA | https://opencode.ai/changelog, https://opencode.ai/docs/tools/ | unreviewed |
| Claude Code | `run_in_background`; `/tasks`; a foreground command that hits its timeout is moved to the background instead of killed. Unattended sessions have a time limit (30 min default, up to 2 h). Killed above 5 GB of output; reaped under memory pressure after 30 minutes idle. `Monitor` streams command stdout lines or WebSocket frames into the conversation (5 min default, 30 min max), and private or metadata addresses are denied. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/interactive-mode.md | reviewed (review-1, background time limit) |
| DeepSeek harness | A job registry with statuses and a bounded output ring per job; the model reads with a consuming cursor. Process-local: **live jobs are not restored after restart**, and a settled record can be lost in a crash. Job ids are predictable; the owner session is the boundary. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/jobs | reviewed |
| Oh My Pi | `bash` can background; `/jobs`; `/pause`; supervised processes (area 4). | GA | https://omp.sh/docs/slash | unreviewed |
| Pi | Documented non-feature (process tracking and cleanup complexity; use tmux). | GA (omission) | https://pi.dev/, https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ | reviewed |
| Antigravity | Sidecars: App-managed background processes with restart policy `always|on-failure|never` (default `always`), off by default. | GA (inferred) | https://antigravity.google/docs/sidecars | reviewed |
| Hermes Agent | `process_manage` tool (area 2). | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Others | Covered in Area 4 by another group. | — | — | — |
| rizzy-harness (ours) | None (bash runs per tool call; no background jobs). | none | inventory §7, §c | from own inventory |

#### Background agent sessions and supervisor daemons
What it is: an agent session that runs without an attached UI, hosted by a long-lived local process so sessions outlive the client.
Variants:
- **A per-user supervisor that hosts each session as its own process:** Claude Code reaps idle processes after about an hour, resumes them from disk, restarts crashed ones, and migrates after auto-update.
- **A per-machine hub that owns "spoke" workers:** Cline.
- **One background server per machine that all clients attach to:** OpenCode, Codex's daemon.
- **A gateway:** Hermes, OpenClaw.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | The CLI can attach to a managed local background app-server that stays running between sessions; `/daemon`; `--no-daemon`; host-local Unix socket. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/environments/network-file-systems.md | unreviewed |
| OpenCode | One background server per machine owns sessions; the TUI, desktop and web attach. | GA | https://opencode.ai/docs/troubleshooting/ | reviewed |
| Claude Code | `claude --bg "task"` / `--bg --exec 'cmd'` (a PTY shell job with no model). `claude attach|logs|stop|respawn|rm`; `rm` refuses over worktrees with unpushed commits. A per-user supervisor daemon hosts each background session as its own process. Idle, unattached sessions are stopped after about 1 h and resumed from disk; crashed ones are restarted. If the supervisor dies, sessions stop unless a new one connects within about 1 minute. Two processes cannot write one transcript. `/background` detaches. Dispatched sessions move into a worktree before editing, commit and push on finish, and open a draft PR when the task calls for it; never push to main. | research preview | https://code.claude.com/docs/en/agent-view.md, https://code.claude.com/docs/en/cli-reference.md | corrected (status was GA) |
| Claude Desktop | Dispatch: one persistent Cowork conversation messaged from the phone, running on the desktop and able to spawn Code sessions (limited beta; not for new users). | beta | https://support.claude.com/en/articles/13947068, https://code.claude.com/docs/en/desktop | corrected |
| OpenCodex | `ocx service` (launchd/systemd/Task Scheduler) for the proxy; no scheduled agent work. | GA | https://github.com/lidge-jun/opencodex | reviewed |
| Oh My Pi | `/tan` runs tangential work in a background agent. | GA | https://omp.sh/docs/slash | reviewed |
| OpenClaw | One Gateway daemon (systemd/LaunchAgent/Scheduled Task) owns sessions, cron and approvals. | GA | https://docs.openclaw.ai/concepts/session | unreviewed |
| Hermes Agent | `/bg` runs a task in a separate session and posts back; the gateway owns sessions, queue, cron and the kanban dispatcher. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging | unreviewed |
| Zed/ACP | Threads keep running while you work in other threads; no headless agent. | GA | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Cline | A per-machine hub daemon (`127.0.0.1:25463`) coordinates; spoke workers run the loop and are owned by the daemon, so sessions outlive clients. `-z/--zen` runs a task in the background hub. No hub authentication documented. | GA (hub); unclear (zen) | https://docs.cline.bot/sdk/architecture/hub-spoke.md, https://docs.cline.bot/cli/cli-reference.md | corrected |
| Others | Nothing documented in these areas. | — | — | — |
| rizzy-harness (ours) | No in-chat background sessions. The node runs sessions independently of the UI, and the link replays by `Resume{after}`, so a chat continues if the browser leaves (inventory §6). | partial | `crates/node/src/lib.rs:323-330`, inventory §7 | from own inventory |

#### Agent dashboards and command centres
What it is: one view of many running agents with status (working, needs input, done) and quick reply.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex agents` / `/agents`: chats on a shared local or remote server, grouped by repo, pinned, unread, sorted by recency. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Claude Code | Agent view (`claude agents`): groups Pinned / Ready for review / Needs input / Working / Completed; row summaries by a small model; peek and reply; PR status badges; `--json` is the supported machine interface. VS Code agent map (`/tasks`) shows the sub-agent tree with tokens. | research preview (agent view); GA (VS Code map) | https://code.claude.com/docs/en/agent-view.md, https://code.claude.com/docs/en/vs-code.md | corrected (intro version not on the page) |
| Oh My Pi | Agent Hub (Alt+A): roster and tree with status, model, tokens, cost, lineage; steer, follow up, revive, kill; persisted across resume. | GA | https://omp.sh/docs/subagents | unreviewed |
| Antigravity | The App as a "command centre" (Agent Manager) for multiple agents across projects; Local or New Worktree mode per project. | GA | https://antigravity.google/docs/overview, https://antigravity.google/docs/projects | unreviewed |
| Zed/ACP | Threads Sidebar grouped by project (Zed Agent, external agents, terminal threads). | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Warp | Agent Management Panel and notifications, including for Claude Code, Codex and OpenCode. | GA | https://docs.warp.dev/agents/cli/agent-conversations/ | unreviewed |
| OpenClaw | Control UI Subagents and background-process panels. | GA | https://docs.openclaw.ai/tools/subagents | reviewed |
| Others | Nothing documented in these areas. | — | — | — |
| rizzy-harness (ours) | Web sidebar of chats and routines with run history; no live multi-agent board. | partial | `web/src/components/Sidebar.tsx` | from own inventory |

#### Auto-backgrounding of long tool calls and server-side tasks
What it is: a long tool call stops blocking the turn and reports back later.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | An MCP call running longer than 2 minutes becomes a background task (`CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS`; it does not survive exit). The MCP tasks extension (`resultType: "task"`, polling `tasks/get`): remote tasks survive exit and are re-followed on resume. | GA; beta (MCP tasks) | https://code.claude.com/docs/en/mcp.md | reviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### In-session schedules, loops and reminders
What it is: timers that wake the **same** session later or repeatedly.
Variants:
- **Cron tools the model calls:** Claude `CronCreate` (session-scoped, 7-day expiry, jitter), Antigravity `schedule` (one-shot or cron, optional daemon), DeepSeek reminders (persisted; deliver only the latest missed occurrence; at-least-once; the host must run).
- **Self-paced re-runs:** Claude `/loop` with `ScheduleWakeup` (1 min to 1 h), Hermes `/loop` (1 to 15 min back-off), OMP `/loop` (prompt / compact / reset modes).
- **Heartbeats:** Codex heartbeat tasks inside a chat, Hermes `/heartbeat` (idle-only, restored after restart). OpenClaw retired its heartbeat into an ordinary automation.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Scheduled tasks can run inside an existing chat ("heartbeat"). | GA | https://learn.chatgpt.com/docs/automations.md, https://learn.chatgpt.com/docs/glossary.md | reviewed |
| OpenCode | `experimental.schedules` runs scheduled prompts under `opencode serve`, firing once across several server processes; API `/api/experimental/schedule` scoped to a session. | experimental | https://opencode.ai/changelog, https://opencode.ai/docs/api/ | reviewed |
| Claude Code | `/loop` with an interval (converted to cron), with a prompt only (self-paced via `ScheduleWakeup`, 1 min to 1 h; a fallback wakeup about 20 minutes later if neither), or bare (maintenance prompt; `.claude/loop.md` overrides). `CronCreate`/`CronList`/`CronDelete`: 50 per session, fire between turns, jitter up to half the period (cap 30 min), **recurring tasks expire after 7 days**, missed fires are not caught up. Unexpired crons are restored on resume; self-paced loops are not. The cloud `/loop` timer dies with the container. | GA | https://code.claude.com/docs/en/scheduled-tasks.md, https://code.claude.com/docs/en/tools-reference.md | reviewed |
| DeepSeek harness | The model creates reminders that reopen the original session and queue a follow-up. Relative, absolute, interval (≥60 s), daily, weekly or cron with IANA zone. Persisted. On restart, an overdue one-shot or **only the latest missed recurrence** is delivered. Exactly-once not guaranteed. **The host must be running.** Sub-agents cannot schedule. | preview (landing page: experimental) | https://deepseek-harness.github.io/deepseek-harness/en/guide/schedule, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/schedule | corrected |
| Oh My Pi | `/loop`: unbounded, N times or until a deadline; modes `prompt`, `compact`, `reset`. In-session only. | GA | https://omp.sh/docs/modes | reviewed |
| Antigravity | The `schedule` tool: one-shot timers (with an optional cancel condition) or five-field cron (`MaxIterations`, `IsDaemon`); fires a high-priority notification into the same conversation; non-daemon schedules die with the task. Where timers run is not stated. | GA | https://antigravity.google/docs/commands/schedule | unreviewed |
| OpenClaw | Standing intents: owner-created instructions that fire when a user turn matches keywords (model-free full-text prefilter; 24 h cooldown, 3 fires, 90-day expiry). The heartbeat was retired into a "Proactive check" automation. | GA | https://docs.openclaw.ai/concepts/standing-intents, https://docs.openclaw.ai/gateway/heartbeat | unreviewed |
| Hermes Agent | `/loop` (fixed or self-paced, 1 min floor backing off to 15 min while replies are unchanged; `LOOP_COMPLETE`, `--times`, `--until` judged by the goal judge; `loops.max_ticks` 100). `/heartbeat every <interval>` (60 s minimum, idle-only, coalesced, restored after gateway restart). Both fire only while the backend lives. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/loops, https://hermes-agent.nousresearch.com/docs/user-guide/features/heartbeat | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None inside a chat (routines create new run chats). | none | inventory §7 | from own inventory |

#### Standalone scheduled runs (cron automations)
What it is: saved prompts that start a **fresh** agent run on a schedule, without a user present.
Variants:
- **Where they run:**
  - locally, so the app must be open: Codex desktop, Claude Desktop local, goose, OpenClaw Gateway, Hermes gateway, Cline hub, DeepSeek;
  - in the vendor cloud: Claude cloud scheduled tasks, Cursor, Warp, Antigravity, OpenHands Cloud;
  - on the master: ours.
- **Missed runs:** caught up once (Claude Desktop, within 7 days), never (Claude session crons), retried once (OpenClaw interrupted agent-turn jobs), or by "missed-run rule" (ours).
- **Minimum interval:** 1 hour (Claude cloud), 1 minute (Desktop when Claude sets it), none stated elsewhere.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Standalone tasks (each run opens a new chat; findings go to a Scheduled inbox) or heartbeat tasks. On the desktop a task runs in the local project or a background worktree, needs the app running, and **runs unattended with the default sandbox settings**. No CLI management UI. | GA | https://learn.chatgpt.com/docs/automations.md | reviewed |
| OpenCode | `experimental.schedules` (session-scoped). | experimental | https://opencode.ai/changelog | reviewed |
| Claude Code | Cloud scheduled tasks (formerly routines): a saved prompt, repos, environment, connectors, model and effort run as a full cloud session. Triggers are schedule (**at least hourly**, cron in UTC unless `CRON_TZ`), API `/fire` (token shown once) and GitHub events. Fresh clone; pushes to `claude/` branches; starts in auto mode. Hourly caps: 100 scheduled runs per account. Run outcomes reflect only how the session ended. | research preview | https://code.claude.com/docs/en/routines.md | corrected (rename; 1-hour minimum; 72-hour clause unverified) |
| Claude Desktop | Local scheduled tasks while the app is open and the computer awake: Hourly to Custom cron (local time; Claude itself can set 1-minute and one-off). Deterministic offset; **one catch-up run for the latest missed slot within 7 days**. Per-task permission mode and model; optional worktree; an "Always allowed" panel to revoke approvals. Prompt stored at `~/.claude/scheduled-tasks/<name>/SKILL.md`. Cowork scheduled tasks run in the cloud. | GA | https://code.claude.com/docs/en/desktop-scheduled-tasks, https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork | corrected |
| DeepSeek harness | Only session reminders (above); "a fresh session per run is not supported". | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/schedule | corrected |
| Oh My Pi | Nothing documented. | nothing documented | https://omp.sh/docs | unreviewed |
| Pi | Nothing documented in core; pi-durable has sleeping tasks woken by a timer or host alarm. | unclear / experimental | https://github.com/earendil-works/pi/blob/main/packages/durable/README.md | reviewed |
| Cursor | Automations: cloud agents on cron (may start late, never early), webhook, GitHub/GitLab/Bitbucket, Slack, Linear, PagerDuty, Sentry. PR creation and computer use on by default. Billed to the creator or a service account. Security Agents and Rollouts run on the same platform. | GA (no label) | https://cursor.com/docs/cloud-agent/automations, https://cursor.com/docs/security-agents, https://cursor.com/docs/rollouts | unreviewed |
| Antigravity | Automations (App): scheduled conversations, a fresh conversation per run, model tier Flash Lite/Flash/Pro, start offset up to 10% of the interval (cap 15 min). Renamed from Scheduled Tasks. | GA | https://antigravity.google/docs/features, https://antigravity.google/docs/changelog | unreviewed |
| OpenHands | An Automation Server stores schedules and triggers, tracks runs and dispatches conversations to the Agent Server. Types: prompt, prompt plus plugins, script bundle. Cron with timezone. Runs `SKIPPED` when the concurrency limit is reached. Fresh sandbox per run (Cloud). Export/import, per-run USD cost, service-account runs, Git sync of `automation.yaml`. | GA | https://docs.openhands.dev/openhands/usage/automations/overview.md, https://docs.openhands.dev/openhands/usage/agent-canvas/managing-automations.md, https://docs.openhands.dev/openhands/usage/agent-canvas/git-sync.md | unreviewed |
| OpenClaw | A scheduler inside the Gateway; jobs and history in SQLite. One-shot jobs auto-delete after delivery. Payloads: agent turns, commands, scripts. Watchdogs: 60 min agent, 10 min command, 5 min script. The agent can propose promoting a repeated request into a job, test-run it, and delete it on failure. Delivery to a channel, a webhook or nowhere. The Gateway must be running. | GA | https://docs.openclaw.ai/automation/cron-jobs, https://docs.openclaw.ai/automation/cron-jobs/how-it-works | reviewed |
| goose | `goose schedule add|list|remove|sessions|run-now` runs a recipe on cron; desktop Scheduler view; `goose serve --enable-scheduler`. | unclear | https://goose-docs.ai/docs/guides/goose-cli-commands.md, https://goose-docs.ai/docs/guides/recipes/session-recipes.md | unreviewed |
| Hermes Agent | Cron jobs are agent runs (fresh agent, attached skills): one-shot, interval, phrases, cron, ISO, per-job timezone. `jobs.json` plus `executions.db`; 60 s tick in the gateway. `context_from` chains outputs; `continuity` feeds a job its prior output. Prompts are scanned at creation; cron runs cannot create jobs. Script-only jobs (`--no-agent`) and `{"wakeAgent": false}` gating. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/cron | reviewed |
| Zed/ACP | Nothing documented. | nothing documented | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Warp | Scheduled agents: cloud agents on cron with a fixed prompt or skill, a fresh session per run, `oz schedule …`, run as the creator. Timezone, limits and overlap not stated. | GA (Oz CLI "legacy") | https://docs.warp.dev/platform/triggers/scheduled-agents/ | unreviewed |
| Cline | `cline schedule create --cron --prompt --workspace` (local time); `--mode act|plan|yolo` (**default yolo**), `--timeout`, `--max-parallel` (default 1); needs the hub; history, stats, export/import. | GA | https://docs.cline.bot/cli/scheduling.md | reviewed |
| rizzy-harness (ours) | Routines on the master: cron (5 fields, time zones), interval or one-shot; up to 8 schedules or exactly one webhook per routine; run-now; missed-run rule; per-user/server concurrency and daily caps; run history with tokens and status; run chats auto-pruned. | works | `crates/master/src/routines/schedule.rs:1-12`, `runner.rs:1-12`, `spec.rs:1-35` | from own inventory |

#### Event triggers and inbound webhooks
What it is: external events (HTTP calls, GitHub, chat, tickets) that start an agent run.
Variants:
- **Authentication:** a per-task bearer token shown once (Claude `/fire`, ours stored as SHA-256), HMAC of the body (OpenHands `X-Signature-256`, DeepSeek GitHub overlay), a dedicated token that rejects query-string tokens (OpenClaw).
- **Idempotency:** OpenClaw `Idempotency-Key`, ours. DeepSeek explicitly does none.
- **Payload trust:** Claude wraps `/fire` text as an untrusted `<routine-fire-payload>` that the prompt must opt in to act on. DeepSeek labels PR fields untrusted. OpenClaw wraps content, but wrapping does not remove tool access. Ours strips memory and web tools for the whole chat.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `@codex review` on GitHub; Slack `@ChatGPT` delegates to a cloud task; Linear assignment creates a cloud chat; the GitHub Action with an `allow-users`/`allow-bots` gate. | GA | https://learn.chatgpt.com/docs/third-party/github.md, https://learn.chatgpt.com/docs/third-party/slack.md, https://learn.chatgpt.com/docs/third-party/linear.md, https://learn.chatgpt.com/docs/github-action.md | reviewed (action, PR review) |
| Claude Code | Cloud scheduled-task API and GitHub triggers. The saved prompt is the assignment and cannot serve as consent; `/fire` text is wrapped as untrusted. `claude-code-action` (trigger phrase, or automation mode on any event including cron; no shell until granted). Channels: MCP servers push events into a running session (research preview). | research preview (triggers, channels); GA (action) | https://code.claude.com/docs/en/routines.md, https://code.claude.com/docs/en/channels-reference.md | corrected (routines), unreviewed (action) |
| DeepSeek harness | An opt-in signed GitHub webhook (`POST /github` on `127.0.0.1:3081`): a PR moving from draft to ready creates a read-only review session; PR fields are labelled untrusted. **No queue, retry or dedup**; 202 means accepted, not matched. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review, https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/webhook.md | reviewed |
| Oh My Pi | `/green` watches and repairs CI until the branch is green; `github` tool run-watch. | GA | https://omp.sh/docs/slash, https://omp.sh/docs/github | unreviewed |
| Cursor | Automations triggers: webhook (private endpoint plus API key), Git hosts, Slack (public channels only), Linear, PagerDuty, Sentry; `@cursor` on GitHub/Bitbucket. | GA | https://cursor.com/docs/cloud-agent/automations, https://cursor.com/docs/cloud-agent | unreviewed |
| Antigravity | Sidecars can be cron sources (builtin `schedule`) and use the `agentapi` CLI (`new-conversation`, `send-message`) to drive conversations. | GA (inferred) | https://antigravity.google/docs/sidecars | reviewed |
| OpenHands | Built-in sources (GitHub Enterprise, Azure DevOps, Jira DC, Bitbucket DC); custom webhooks via `POST /api/automation/v1/webhooks` with HMAC-SHA256 (`X-Signature-256`); JMESPath filters; Canvas must be publicly reachable, which exposes the whole agent server. | GA | https://docs.openhands.dev/openhands/usage/automations/event-automations.md | reviewed |
| OpenClaw | Off by default. `POST /hooks/wake`, `POST /hooks/agent` (isolated, `Idempotency-Key`, optional `waitForCompletion`), `POST /hooks/<name>` mappings with trusted JS/TS transforms. A dedicated token; query-string tokens rejected; 429 on auth failures; not exactly-once. Gmail Pub/Sub, IMAP and MCP Events pages (titles only). | GA | https://docs.openclaw.ai/automation/cron-jobs/webhooks | unreviewed |
| Hermes Agent | GitHub/GitLab and other events trigger runs through the gateway; MCP event subscriptions use signed dynamic routes (experimental). The webhook page was not opened. | GA; experimental (MCP events) | https://hermes-agent.nousresearch.com/docs/llms.txt, https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | unreviewed |
| Warp | Triggers: schedules, CLI, API/SDK, integrations, GitHub mentions, GitHub Actions, custom webhooks, Handoff. | GA | https://docs.warp.dev/platform/triggers/ | unreviewed |
| Cline | Connectors (Telegram, Slack, Discord, Google Chat, WhatsApp, Linear): each message creates or continues a session; a hook script can allow/deny actors. **Telegram without `--allowed-user-id` or a hook auto-approves all messages.** | GA (CLI) | https://docs.cline.bot/cli/connectors.md | reviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | `POST /api/hooks/<trigger id>`: per-routine bearer secret (shown once, rotatable, stored as SHA-256), idempotency key, 64 KiB body, token buckets that unknown ids cannot exhaust. Triggers a routine, not an arbitrary session. Webhook chats get no memory, no `web_*`, no `remember`, a banner, and a sticky untrusted flag. | works | `crates/master/src/http/hooks.rs:1-60`, `crates/master/src/routines/buckets.rs:1-10`, `crates/master/src/lib.rs:257-297` | from own inventory |

#### Permission posture of unattended runs
What it is: what an unattended scheduled or triggered run may do without a human.
Variants: from "everything" to "nothing without a click":
- **Self-approve by default:** Cline scheduled runs approve their own calls in every mode, default yolo.
- **Auto mode classifier:** Claude cloud tasks stop only on ask rules or repeated blocks.
- **Default sandbox:** Codex desktop scheduled tasks.
- **Per-task mode with a revocable allow list:** Claude Desktop.
- **Headless default-deny for dangerous commands:** Hermes.
- **Standing grants bound to job, command, cwd and env:** OpenClaw.
- **Human click required for every command:** ours.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Desktop scheduled tasks run **unattended with the default sandbox settings**. | GA | https://learn.chatgpt.com/docs/automations.md | reviewed |
| Claude Code | Cloud tasks start in auto mode and stop only on ask-rule matches or repeated-block thresholds. A run in auto mode can publish an artifact without asking. Tasks act as the owning user. | research preview | https://code.claude.com/docs/en/routines.md | corrected |
| Claude Desktop | Per-task permission mode; an "Always allowed" panel to review and revoke approvals. | GA | https://code.claude.com/docs/en/desktop-scheduled-tasks | corrected |
| DeepSeek harness | GitHub review sessions use the `read-only` permission preset. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| OpenClaw | Cron grants: a standing grant bound to job, command, cwd and env (`grantExpiryDays` unset by default). | GA | https://docs.openclaw.ai/automation/cron-jobs | reviewed |
| Hermes Agent | Headless defaults deny (`cron_mode`, `unattended_mode`); cron runs cannot create jobs. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/cron | reviewed |
| Cursor | Automations: PR creation and computer use on by default. | GA | https://cursor.com/docs/cloud-agent/automations | unreviewed |
| Cline | Scheduled runs **approve their own tool calls in every mode** and default to `yolo`. | GA | https://docs.cline.bot/cli/scheduling.md | reviewed |
| Others | Not documented. | — | — | — |
| rizzy-harness (ours) | Agentic routines only in `ask`/`ask_all`: a command waits for a human click (cancelled after 24 h); `auto` is rejected. So unattended scheduled agents can only do read-only work. | works | `crates/master/src/routines/spec.rs:131-141`, `runner.rs:95` | from own inventory |

#### Cloud background agents
What it is: vendor-hosted runs that continue with the user's computer off (summary only; remote execution is covered by another group).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Codex Cloud: a per-task workspace that keeps running while the computer sleeps; `codex cloud exec` / `list` (with `attempt_total`). | GA | https://learn.chatgpt.com/docs/cloud.md, https://learn.chatgpt.com/docs/developer-commands.md | reviewed |
| Claude Code / Desktop | Cloud sessions on Anthropic VMs or a self-hosted environment; `claude --cloud`; Projects threads. | GA | https://code.claude.com/docs/en/claude-code-on-the-web | unreviewed |
| Cursor | Cloud Agents in isolated VMs, pushing to a branch and PR (formerly Background Agents). | GA (paid) | https://cursor.com/docs/cloud-agent | reviewed |
| Antigravity | Remote Agents in a Google-managed environment, persisting when the machine is offline (Turbo preset by default; no plan mode). | GA rolling out | https://antigravity.google/docs/remote-agents | unreviewed |
| OpenClaw | Cloud workers (Crabbox): `worker-turn` runs the loop on the box with inference proxied by the Gateway. | beta | https://docs.openclaw.ai/gateway/cloud-workers | corrected |
| Warp | Cloud runs: trigger, task record, config resolution, provisioning, loop, outputs, teardown; maximum runtime. | GA | https://docs.warp.dev/platform/architecture/ | unreviewed |
| Cline | Cloud Agents: per-session sandbox on a `cline/` branch, a PR at the end; pauses after 15 minutes idle; 10 concurrent, 50 per day; no triggers. | preview | https://docs.cline.bot/usage/cloud-agents.md | reviewed |
| OpenHands | Cloud/Enterprise sandboxes with an idle pause and a 12-hour continuous-run cap. | GA (Enterprise) | https://docs.openhands.dev/enterprise/conversations-and-sandboxes.md | unreviewed |
| rizzy-harness (ours) | Not applicable (self-hosted); remote nodes play this role. | — | inventory §11 | from own inventory |

#### Durable and crash-surviving runs
What it is: what survives a process crash or restart in the middle of a run.
Variants:
- **Replay from recorded results:** Claude workflows (cached agent results are reused until the first changed prompt), pi-durable (commit before display, checkpoint per step, restart limit 5).
- **Resume input after restart:** OpenClaw injects one resume prompt so the agent checks external state.
- **Never replay a crashed turn:** Hermes marks it `unknown` and blocks until discarded; ours closes it as `interrupted`.
- **Partial restore:** OpenClaw restores completed cron results without rerunning and retries interrupted agent-turn jobs once.
- **Not durable:** DeepSeek jobs (process-local), Claude self-paced loops and monitors.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Supervisor: crashed processes restart; background shells and sub-agents carry across process restarts; monitors stop. Workflows: completed agents return cached results on relaunch until the first changed prompt; stalled agents restart up to 5 times; a usage limit pauses and auto-continues within 24 h; `claude --resume` plus relaunch restores a run from saved results. Cloud sessions store workflow results with the conversation. | research preview (supervisor); GA (workflows) | https://code.claude.com/docs/en/agent-view.md, https://code.claude.com/docs/en/workflows.md | corrected (supervisor status), reviewed (workflows) |
| DeepSeek harness | Reminders persist and deliver the latest missed occurrence; **background jobs are not restored**; goal activation is never persisted. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/jobs, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/goal | reviewed |
| Oh My Pi | Goal restored as paused after a cold restart; the Agent Hub persists across resume. | GA | https://omp.sh/docs/goal | reviewed |
| Pi | pi-durable: transcripts, turns, tool calls and state committed before display; per-step checkpoints; resume after crash with a restart limit (default 5); background-owned work survives the parent's abort. | experimental | https://github.com/earendil-works/pi/blob/main/packages/durable/README.md | corrected |
| OpenClaw | Graceful stop marks running root sessions, and startup injects one resume input; crashes can still resume eligible sessions. Cron: completed results are restored without rerun; interrupted time-scheduled agent-turn jobs get one retry; command, script and event jobs are not replayed. Durable writer claim checked at commit. | GA | https://docs.openclaw.ai/concepts/session, https://docs.openclaw.ai/automation/cron-jobs/how-it-works, https://docs.openclaw.ai/concepts/agent-loop | reviewed (cron), corrected (writer claim) |
| Hermes Agent | A gateway crash mid-turn marks the turn `unknown`; it is not replayed, and the session blocks until discarded. Heartbeat and goal state are restored on `/resume`. Kanban reclaims crashed claims. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban, https://hermes-agent.nousresearch.com/docs/user-guide/features/goals | reviewed |
| OpenCode | Schedules fire once across multiple server processes; durable per-session event log. | experimental / GA | https://opencode.ai/changelog, https://opencode.ai/docs/api/ | reviewed |
| Claude Desktop | Local scheduled tasks catch up one missed run within 7 days. | GA | https://code.claude.com/docs/en/desktop-scheduled-tasks | corrected |
| Others | Not documented in these areas. | — | — | — |
| rizzy-harness (ours) | A restart mid-turn closes the turn as `interrupted` and denies pending approvals; the backend runs in its own task so a panic ends the turn, not the node; routines have a missed-run rule. | works | `crates/node/src/session.rs:154-191`, `:415-449`, `crates/master/src/routines/schedule.rs:1-12` | from own inventory |

#### Auto-continue after a usage limit
| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Interactive subscription sessions wait in place and, at reset, prompt Claude to resume (at most three automatic continuations). It handles laptop sleep. Not offered in background, `-p`, API-key or cloud-provider sessions. `/config` can disable it but not enable it, because enabling grants unattended execution. | GA | https://code.claude.com/docs/en/interactive-mode.md | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Keep the machine awake
| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `features.prevent_idle_sleep` while a turn runs. | experimental | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| Claude Desktop | "Keep computer awake" setting for scheduled tasks. | GA | https://code.claude.com/docs/en/desktop-scheduled-tasks | corrected |
| Zed/ACP | `agent.prevent_idle_sleep` while a thread generates; notification and sound when done. | GA | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Completion notifications
| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Mobile push while Remote Control is active (long task finished, decision needed, or asked); agent-view `Notification` hook types `agent_needs_input`/`agent_completed`. | GA | https://code.claude.com/docs/en/remote-control.md, https://code.claude.com/docs/en/agent-view.md | unreviewed |
| Claude Desktop | Dispatch-spawned sessions push notifications. | beta | https://code.claude.com/docs/en/desktop | corrected |
| Zed/ACP | Desktop notification or sound when a background thread finishes. | GA | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Warp | Notifications (complete, request, error) including for third-party CLIs. | GA | https://docs.warp.dev/agents/cli/agent-conversations/ | unreviewed |
| OpenClaw / Hermes | Cron delivery to chat channels or home channels (`[SILENT]` suppresses in Hermes). | GA | https://docs.openclaw.ai/automation/cron-jobs, https://hermes-agent.nousresearch.com/docs/user-guide/features/cron | reviewed |
| Others | Nothing documented. | — | — | — |
| rizzy-harness (ours) | Not in the inventory. | not in the inventory | — | from own inventory |

---

## 3. Availability matrix

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

## 4. Common across the ecosystem

There are 16 real harnesses (all except OpenCodex, the gateway). A feature counts as common when at least 8 of them document it (`Y` or `B`).

| Feature | Count (of 16) | Notes |
|---|---|---|
| Sub-agent delegation tool | 15 | all except Pi (example extension only) |
| Custom agent definition files | 14 (+1 unverified) | goose unverified; Cline nothing documented |
| Read-only / custom modes | 14 | nothing documented: OpenHands, Hermes |
| Sub-agent model slot | 13 | not documented: Claude Desktop (inherits the engine), Pi, Cline |
| Standalone scheduled runs | 12 (+1 unverified) | nothing documented: DeepSeek (reminders only), OMP, Zed; goose unverified |
| Resume a sub-agent | 11 | |
| Event triggers / webhooks | 11 | |
| Plan mode | 10 | nothing documented: OpenHands, OpenClaw, goose, Hermes, Zed Agent; Pi refuses it |
| Built-in named roster | 10 | the shapes differ sharply; see the roster table |
| Depth limit documented | 10 (+1 unverified) | from 1 to 10 levels, or none documented |
| Plan review / hand-off | 9 | |
| Task / to-do tool | 9 (+3 unverified) | |
| Background sub-agents | 9 | |
| Cross-session messaging / bus | 9 | |
| Background sessions / supervisor | 9 | |
| Cloud background agents | 9 | |
| Interview-first planning | 8 | |
| Concurrency limit documented | 8 | |
| Worktree / overlay isolation for sub-agents | 8 | |
| Background shell jobs | 8 | |
| Agent dashboard | 8 | |
| In-session schedules / loops | 8 | |
| Unattended-run permission policy documented | 8 | |
| Durable / crash-surviving runs | 8 | |
| Goal / run-until-done | 7 (+3 unverified) | just under the bar; counting the unverified rows would make it 10 |

Counts come from the availability matrix (`Y` or `B`, OpenCodex excluded).

What this says: by late 2026, **delegation with a per-agent model, custom Markdown agent files, a plan mode, cron-style scheduled runs and webhook triggers are table stakes**. Teams, swarms, script workflows, advisors and auxiliary slots are not, yet.

---

## 5. Only one harness has it

| Feature | Harness | Why it is interesting (or not) |
|---|---|---|
| Sub-agent output scanning before the parent reads it | Claude Code | A cheap prompt-injection layer at the delegation boundary. **Directly relevant to us**, because sub-agent and cross-node reports go to a parent. |
| Workflows with deterministic replay (`Date.now()`/`Math.random()` throw) | Claude Code | Lets a crashed fan-out resume from cached results by prompt order. A strong idea for durable orchestration; DeepSeek's workflow API copies the shape without the replay claim. |
| Cross-session inbound trust model (`crossSessionInbound`, peer messages cannot answer approvals) | Claude Code | The only documented answer to "what may a message from another agent authorise". Needed before any agent-to-agent bus. |
| Auto-backgrounding long MCP calls and MCP tasks | Claude Code | Useful only once there is MCP; we have none. |
| Auto-continue after a usage limit | Claude Code | Subscription-specific; `/config` can disable it but not enable it, because enabling grants unattended execution. That one-way rule is worth copying. |
| Timed user questions answered after a restart | DeepSeek harness | A question survives the tool call and a host restart. Good for a web UI where the user answers hours later. |
| Goal objects with compare-and-set revision and human-authorised resume | DeepSeek harness | Matches an event-log design: the goal is a fold over events. |
| Hidden `maxTotalAgents` that the script cannot see | DeepSeek harness | A cap the model cannot plan around. Simple and correct. |
| Ten model roles with `@role` aliases, per-role fallback chains and auto-resolution | Oh My Pi | The most complete slot design. Its auto-resolution makes "which model ran this" non-obvious, so it is a weakness for auditability. |
| Prewalk (strong model starts, one switch to a cheaper model) | Oh My Pi | A cheap cost lever; one switch, logged. |
| Copy-on-write isolation backends with patch merge | Oh My Pi | Faster than worktrees on reflink filesystems; the docs say it is not a sandbox. |
| Vision sidecar for text-only models | OpenCodex (gateway) | Exactly the "vision bridge" slot the reader wants. Hermes's auxiliary vision model is the in-harness equivalent. |
| Message-board-only swarm (no private messages) | Antigravity | Forces visibility of all coordination. Interesting for auditability; preview. |
| Sidecars plus the `agentapi` CLI | Antigravity | Lets local scripts create or message conversations: an event source without a webhook. |
| Integrity modes (`benchmark` forbids mock passes) | Antigravity | A rare explicit guard against an agent faking success. |
| Standing intents (keyword-triggered, model-free prefilter) | OpenClaw | Reminders that fire on content rather than time, at zero model cost. |
| Restart resume input ("check files, processes, external actions") | OpenClaw | An honest handling of side effects across a crash. |
| Kanban with an OS process per worker, a reclaiming dispatcher and `protocol_violation` | Hermes Agent | A real crash model for multi-agent boards; its threat model is single-host only. |
| Separate fallback chains per auxiliary task | Hermes Agent | The cleanest answer to "a summariser outage should not take down titles". |
| Script-only cron with `wakeAgent: false` gating | Hermes Agent | A model-free pre-check before paying for an agent run. Directly applicable to routines. |
| Durable server-backed inbox per agent across harnesses | Warp | Cross-harness orchestration; the control plane sees all traffic. |
| One-catch-up rule for missed scheduled runs (within 7 days) | Claude Desktop | A clear and testable missed-run policy. |

---

## 6. Ours against the field

Ranked by how much each matters for a self-hosted master/node harness.

1. **Unattended routines are the strongest part and also the most hobbled.**
   - **Ahead:** our webhook entry point (hashed per-routine secret, idempotency key, token buckets, sticky untrusted flag that strips memory and web tools; see *Event triggers*) is ahead of DeepSeek (no dedup), Cline (Telegram approves everything) and OpenHands (exposing the backend exposes the agent server). It is on par with Claude Code's fire-payload wrapping and OpenClaw's webhooks.
   - **Hobbled:** agentic routines cannot run without a human click (*Permission posture of unattended runs*). Codex runs scheduled tasks under its default sandbox and Claude under an auto-mode classifier. Once the `integration/gate3` Landlock sandbox lands, "auto inside a required sandbox, node-side" is the defensible middle that no competitor documents with node authority.
   - **Missing:** a model-free pre-check (Hermes `wakeAgent`) and a stated catch-up rule (Claude Desktop: one run within 7 days) beyond our "missed-run rule".

2. **We have no plan mode, and the field shows exactly how not to build one.**
   - DeepSeek's plan mode is prompt guidance that the sandbox ignores. Claude Code does not enforce plan blocks when bypass is available. Cline's plan mode lets `python -c` write. (*Plan mode*)
   - For us, plan mode must be a **node-enforced tool and policy state**, as OpenCode and the Antigravity CLI do it, recorded as an event (as DeepSeek logs `plan/mode`), with approval through the existing approval path.
   - Review UX worth taking: OMP's "execute in a fresh session with only the plan" and Antigravity's comment-on-artifact. (*Plan review*)

3. **Delegation is solid but shallow and synchronous.**
   - **Ahead:** worktree-per-writing-child with patch-back and a filter-driver refusal matches Claude Code and OMP (*Isolation*). Token plus count caps per turn are rarer than depth caps (only Claude `maxBudgetUsd` and OMP request budgets compare).
   - **Lacking:**
     - background children (9 harnesses document them) and resumable children (11);
     - structured output schemas (Claude, OMP, Hermes, DeepSeek);
     - a delegation-boundary output scan (only Claude);
     - a sub-agent "cannot do" list. Hermes and goose block memory, scheduling and messaging from children; we do not, though `remember` is chat-only anyway.
   - **Our approval model:** the child runs with the parent's approval mode, approvals inside the child. That is the Codex/Antigravity model, and it is the right one; Cline and OMP, where approving the spawn approves everything, are what not to copy.

4. **Model slots: we have per-agent model and fallback, and nothing else.** (*Auxiliary model slots*)
   - The reader's plan (small model for titles, vision bridge, summariser) is validated by the field. Hermes, OMP, Zed and OpenCode all have it. Design lessons:
     - **One slot per task, not one shared "small" model.** Claude's single `ANTHROPIC_DEFAULT_HAIKU_MODEL` also moves the goal judge and the `haiku` alias: a documented coupling trap.
     - **Default each slot to the main model** (Hermes, Zed) rather than auto-resolving from whatever is authenticated (OMP). With "the node decides", which model a node may call must be explicit.
     - **Give each slot its own fallback chain** (Hermes, OMP); our per-agent chain machinery can be reused.
     - **The vision bridge is a sidecar pattern** (OpenCodex: describe, cache, cap per turn). Its output is model-written text and must be treated as untrusted.
     - **Which model runs our history summarisation is not stated in the inventory.** Settle that before adding a summariser slot.

5. **Roster (`feat/agent-roster`).**
   - The field converges on 3 to 6 built-ins with a read-only explorer, a planner and a general worker (*Built-in agent roster*). Our branch's eight is on the high side, and three are worth attacking:
     - **"Smol"** (no tools, no memory) is a model slot dressed as an agent. OpenCode's title/summary agents show that shape and hide them. If its job is cheap side tasks, make it a slot. If it is a user-facing cheap chat agent, say so.
     - **"Senior Teamlead"** duplicates the delegation role that "Software Developer" already has (it delegates to scout/researcher/reviewer).
     - **"Researcher"** has web tools that agentic nodes do not register (inventory "Still true"), so it is chat-only.
   - **No harness ships a built-in "Writer".** Not wrong, but a reason to check it earns its place.
   - **Missing compared with the field:** a **Plan** agent paired with a plan mode (Claude, OpenCode).

6. **No goal loop and no task tool.**
   - Common now (*Goal / run-until-done*, *Task / to-do*). The judge design matters more than the loop. Hermes fails **open** on judge errors and Antigravity trusts self-declaration. OpenHands (invalid judge output counts as not done) and goose (shell checks that must exit 0) are the safer patterns.
   - Our hard 50-step cap would bound a goal loop only per turn, not across turns. A goal needs its own budget (Codex tokens and time, OMP tokens).

7. **Cross-harness sub-agents.**
   - `AgentBackend` exists with one implementation (*Other harnesses as sub-agents*). OpenHands, OpenClaw, goose and Zed prove ACP-over-stdio works. Every one of them leaves **approval handling for the hosted agent undocumented**: the gap we would have to close to keep "the node decides".

8. **Teams, swarms and script workflows: skip for now.**
   - Nearly all are experimental or preview, and their own docs admit costs (Cline says a single agent is often more efficient; Claude says the lead sometimes does the work itself).
   - Under the feature freeze these are the easiest to defer. The one idea worth keeping on file is Claude's deterministic replay of workflow results, because it maps onto our event log.

9. **Durability.** Closing an interrupted turn and denying pending approvals is on par with Hermes's `unknown` turn and safer than replaying. What we lack is OpenClaw's restart "resume input" and any durable background work: we have no background work, so nothing to make durable yet.

---

## 7. Corrections, contradictions and unverified claims

**Changed by review (these areas):**
- **Claude Code.** Background sessions, the supervisor CLI, agent view, `--bg`, the daemon and attach/logs/stop are **research preview**, not GA (review-2 corrections 1 and 2). The agent-view introduction version (2.1.139) is not on agent-view.md (correction 3). Cloud scheduled tasks are the product formerly called routines, with a **1-hour minimum interval**, and the 30/hour per-task cap is one shared count (correction 5). "Skipped up to 72 hours then turns off" moved to Unverified (review-3). Plan mode also runs classifier-approved commands when auto mode is available (review-1 correction 1).
- **Claude Desktop.** `/schedule` was not removed (typing it requests a schedule); Cowork versus local scheduled tasks are two products, not a contradiction; Dispatch's 30-minute approval window comes from the desktop page, not the support article.
- **OpenHands.** DelegateTool is "retired", not "replaced".
- **DeepSeek harness.** The agent-team page exists on GitHub, not on the docs site; `write_scopes` are advisory; reminders "do not guarantee exactly-once" (the page does not say "at-least-once").
- **Antigravity.** `/teamwork` horizon is "multiple days".
- **Warp.** Factories are "Early Access". Orchestration "wake on message" and "one worktree per agent" are not on the cited pages; orchestration has no status label.
- **OpenClaw.** Cloud workers are beta, and "outbound-only" is an inference. The writer claim is verified in the commit transaction, not written in it.
- **Pi.** Background-owned work survives the parent's abort in pi-durable; the to-do non-feature reason is "confuse models".
- **Cline.** Hub roles are per session participant.
- **Codex.** The presets 8,000-byte limit moved to Unverified.

**Vendor pages that disagree:**
- **Claude Code** sub-agent nesting went 5 → 0 → 3 within six weeks (the digest still says 5). `/fork` has meant three things, and today depends on whether agent view is on. Cross-session messaging works on "any of your machines" (changelog) or the same machine (digest).
- **Hermes** delegation `max_iterations` is 250 on the delegation page and 50 on the agent-loop page.
- **Cline** says plan mode "cannot execute commands" on one page and only blocks mutating commands in sub-agents and schedules on others. Kanban auto-PR is described on one page and not on another.
- **OpenCode** README (plan asks before bash) disagrees with the V2 permissions page. `/goal` and schedules exist only in the changelog.
- **OMP** README "schema-validated typed results" disagrees with the docs, where schemas come only through `output`. The README lists 9 roles; the docs list 10.
- **goose** says sub-agents are disabled outside autonomous mode, which conflicts with using approvals and delegation together. v1.55.0 removed "background subagents and orchestrator".
- **Antigravity**'s App page still says "Fast mode" after the CLI removed `/fast`.
- **DeepSeek** schedules are labelled experimental on the landing page and unlabelled in the guide.
- **OpenClaw**'s home page still advertises "heartbeat scheduling" after the heartbeat was retired.

**Nobody could confirm:**
- what Codex's built-ins `default`/`worker`/`explorer` actually do, and the default of `max_concurrent_threads_per_session`;
- how OpenCode's `/goal` reviewer works;
- whether Antigravity timers survive closing the app;
- OpenClaw's sub-agent depth defaults (subpage unopened);
- Hermes webhook details (page unopened);
- Warp scheduled-agent timezone and overlap behaviour;
- Cline hub behaviour after the hub stops, and zen mode;
- the model behind Claude Code's compaction (review-1 omission 3) and WebFetch extraction;
- the model behind DeepSeek's title generation;
- OpenHands' routing and fallback guides (index only);
- the Claude Code changelog-only variables `CLAUDE_CODE_SUBAGENT_EFFORT` and `CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL`.

---

## 8. Questions for Round 2 (source code)

- **Codex (CLI, open):**
  - What are the prompts and tool sets of the built-in agents `default`, `worker` and `explorer`?
  - Is plan mode enforced by tool removal, by the sandbox, or by prompt?
  - What is the default for `max_concurrent_threads_per_session`?
  - How does goal "automatic continuation" decide to continue, and what stops it besides the token budget?
- **OpenCode:**
  - Does the `plan` agent's rule set actually deny shell, or only edits?
  - How does the `/goal` reviewer judge, and with which model?
  - How do `experimental.schedules` "fire once across multiple server processes" (lock, lease, or DB row)?
  - What model does the hidden `title` agent default to when `agents.title.model` is unset?
- **DeepSeek harness:**
  - What is the default `maxDepth` and `maxGoalRounds`?
  - Does a sub-agent inherit the parent's sandbox mode and approval policy (the docs are silent)?
  - How are reminder deliveries made idempotent on restart?
  - Which model generates titles?
- **Oh My Pi:**
  - What is the exact role auto-resolution algorithm when `modelRoles` is empty?
  - How does `task.isolation` choose a backend and apply patches on conflict?
  - Is plan mode read-only by tool removal?
  - How is the advisor's "blocker interrupt" implemented?
- **Pi:**
  - In the example sub-agent extension and pi-durable, how is a child's tool set and cwd constrained?
  - Does pi-durable's restart limit count per task or per run?
- **OpenHands:**
  - Does the task tool enforce any depth or count limit in code?
  - How are approvals from ACP-hosted agents (Claude Code, Codex) surfaced or answered?
  - Which LLM runs the goal judge by default?
- **OpenClaw:**
  - What are the sub-agent depth defaults and the tool-loss list?
  - How is "one retry for interrupted agent-turn jobs" recorded so it cannot repeat?
  - Is the cloud worker connection outbound-only in code?
- **goose:**
  - Do sub-agents inherit the parent's provider or model when `GOOSE_SUBAGENT_MODEL` is unset?
  - Where does the scheduler store jobs, and what happens to a run missed while `goose serve` is down?
- **Hermes Agent:**
  - Which model does each auxiliary task use when unset, and do the auxiliary fallback chains share cooldown state with the primary chain?
  - Does the goal judge's fail-open path have any cap?
  - How does the kanban dispatcher reclaim a crashed claim without double-running a worker?
- **Zed:**
  - Does `spawn_agent` enforce any depth or concurrency limit?
  - Do `thread_summary_model` and `compaction_model` default to the main model?
  - What tools does a sub-agent get when the parent's profile restricts them?
- **Warp (open client):**
  - Is orchestration (child launch, inbox) implemented client-side or only on the closed control plane?
  - Where is the planning-model switch applied?
- **Cline:**
  - Does `spawn_agent` really have no depth, count or timeout cap?
  - Is plan mode's shell restriction a blocklist in code?
  - Does a schedule created while the hub is stopped fire later, and with what approval mode?
- **Zed/ACP protocol:** In the sub-agents RFD's reference implementation (if one exists), can a child session's permission requests reach the client?

## 9. Review (reviewer 2: Areas 9-15, lines 1432-3506)

Date: 2026-11-10. Reviewer worked only on public docs at code.claude.com. `claude-code.md` was NOT edited (see the task note: three reviewers share the file).

Checked: 47 entries of about 190 in lines 1432-3506 (entries counted individually, including several in one cluster). Verdicts: 43 confirmed, 3 confirmed-with-correction, 1 status-wrong, 0 wrong, 0 not-supported, 0 unreachable. Two details inside confirmed entries could not be checked because the fetch was truncated (listed below).

Pages opened (all fetched in full or in the relevant part): sub-agents.md, agent-teams.md, agent-view.md, checkpointing.md, sessions.md, workflows.md, scheduled-tasks.md, routines.md, cross-session-messaging.md, hooks.md (offsets 0, 100000, 200000), skills.md, mcp.md (first 100,000 chars), plugins/mods/overview.md, mods/admin.md, mods/events.md, mods/reference.md (limits table), plugins/marketplace-reference.md, worktrees.md, model-config.md (first 100,000 chars), advisor.md, fast-mode.md, agent-sdk/subagents.md, self-hosted-environments.md.

### Entry table

| Entry (line) | Verdict | Reason |
|---|---|---|
| Default subagent delegation behavior (1434) | confirmed | background default and fork mode on by default stated on sub-agents.md |
| Cross-session messaging (1445) | confirmed | v2.1.224 / v2.1.234 floors, text-only, @name typeahead v2.1.232 match |
| Cross-session inbound trust model (1451) | confirmed | accept/hold/refuse, 100 held, 5 min dialogExpiry, own-child rules all on page |
| Cross-session transport: inbox socket (1457) | confirmed | `/tmp/cc-socks-<uid>`, 30 s idle close, token auth Windows-required |
| Cross-machine and cloud messaging (1463) | confirmed | isolatePeerMachines, Remote Control path, offline behaviour |
| notify_when_idle (1469) | confirmed | v2.1.236, 12 h drop, main conversation only |
| Messaging limits and loop protection (1475) | confirmed | ~1M chars, 50 queued, rate-limit per sender |
| /fork copies a session (1481) | confirmed | research preview, own worktree; fork subagent when agent view off |
| Subagent definition format and scopes (1487) | confirmed | priority table 1-5 identical; plugin agents ignore hooks/mcpServers/permissionMode |
| Subagent frontmatter fields (1493) | confirmed | field list matches (incl. `experimental`) |
| Built-in subagents (1517) | confirmed | Explore on opus alias when main is Fable via subscription/Console/gateway; statusline-setup Sonnet, claude-code-guide Haiku |
| Subagent model resolution (1559) | confirmed | order 1-4 and "before v2.1.251 env var came first" quoted on page |
| Per-subagent and teammate model selection (1643) | confirmed | duplicate of 1559, consistent |
| Foreground vs background, fork mode (1571) | confirmed | background by default; fork mode on by default; CLAUDE_CODE_FORK_SUBAGENT=0 |
| Nesting depth and concurrency limits (1583) | confirmed | 20 concurrent, 3 layers, `Concurrent subagent limit reached`, ultracode exempt |
| Subagent depth, concurrency and spend caps (1661) | confirmed | SDK page: 3 / 20 / maxBudgetUsd, `Budget limit reached`, `error_max_budget_usd` |
| Programmatic subagents (1655) | confirmed | AgentDefinition fields, async_launched, Agent vs Task naming, Opus 5 preset line |
| Agent teams (1599) | confirmed | experimental, env flag, mailbox path, 3 task states, file locking, `session-`+8 chars; named subagent becomes teammate unasked |
| Agent teams display modes (1605) | confirmed | in-process default, tmux/iTerm2, 30 s idle hide, model order, teammateDefaultModel removed v2.1.234 |
| Agent teams limitations (1611) | confirmed | all nine limits on page; 3-5 teammates advice |
| Dynamic workflows (1626) | confirmed | 16 concurrent (1-256), 4,096 items, 1,000 agents, 5 s stagger, 5 schema retries |
| Workflow triggering and size guidance (1632) | confirmed | ultracode, per-mode approval table, small/medium/large, 25-agent warning |
| Workflow resume, pause, stall (1773) | confirmed | replay order, 5 restarts / 6 attempts, usage-limit pause conditions |
| Background sessions and supervisor CLI (1708) | status-wrong | marked GA; agent-view.md says research preview (see corrections) |
| Automatic backgrounding of long MCP calls (1720) | confirmed | two minutes, CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS, 0 = off |
| MCP tasks (1725) | confirmed | public beta, v2.1.304+, terminal/SDK/Actions only |
| Agent view (1737) | confirmed-with-correction | research preview confirmed; "Introduced v2.1.139" is not on agent-view.md (see below) |
| Supervisor and process lifecycle (1743) | confirmed | ~1 h idle stop, ~1 min supervisor reconnect |
| Dispatch from the shell and shell jobs (1749) | confirmed | --bg rejected with -p, shell output in memory only, cleaned ~5 min after exit |
| Session-scoped scheduling /loop and cron (1779) | confirmed | 50 tasks, 8-char IDs, 25,000-byte loop.md, 1 min-1 h self-pace, ~20 min fallback |
| Cron jitter, expiry, restore (1785) | confirmed | half period capped 30 min, `*/5` at 4m45s, 90 s early one-shots, 7-day expiry |
| Routine limits and run outcomes (1791) | confirmed-with-correction | caps correct; product is now "scheduled tasks, formerly routines"; minimum interval 1 hour (see below) |
| Session storage as JSONL (1848) | confirmed | path rule, 200-char truncate + hash, cleanupPeriodDays 30, env vars |
| Resume entry points (1854) | confirmed | table matches; -p/SDK/loop excluded from picker |
| What a resumed session restores (1860) | confirmed | --mcp-config, --settings, --plugin-dir, --fallback-model, --add-dir not restored |
| Permission mode on resume (1866) | confirmed | bypassPermissions never restored; plan-mode -p conditions match |
| Fork and branch a session (1890) | confirmed | /branch vs --fork-session table matches |
| Checkpoints and rewind (1902) | confirmed | 100 checkpoints, 30-day sweep, six menu actions, double Esc |
| What checkpoints do not track (1908) | confirmed | Bash, subagent edits (only foreground context:fork restored), symlinks/hard links |
| Worktree base branch / LFS neutralisation (2035) | confirmed | fresh default, 24 h fetch, 5 s cap, filter drivers skipped, GIT_NO_REPLACE_OBJECTS |
| Subagent worktrees cleanup/locking (2041) | confirmed | removed if unchanged, sweep by cleanupPeriodDays, git worktree lock |
| Hooks: model and configuration (2200) | confirmed-with-correction | page lists 33 events, entry says 31 (see below) |
| Hook exit-code semantics (2224) / Exit 2 per event (2230) | confirmed | exit 2 table identical; other exit = non-blocking |
| additionalContext cap, timeouts, onFailure (2236, 2254) | confirmed | 10,000 chars + 2,000 preview; 600/30/60 defaults; v2.1.295 |
| Stop hook (2392), SessionEnd (2446), defer (2338), PermissionDenied (2350) | confirmed | 8-continuation cap + CLAUDE_CODE_STOP_HOOK_BLOCK_CAP; 1.5 s SessionEnd; tool_deferred; retry:true |
| Skills format/locations/frontmatter/fork/allowed-tools/synced (2458-2518) | confirmed | agentskills.io, enterprise > personal > project, 1,536-char cap, context: fork "not a fork of the conversation", grant clears next message, synced v2.1.273 |
| MCP scopes and precedence (2548), transports (2536), tasks (1725) | confirmed | local > project > user > plugin > connector, whole entry, SSE deprecated, ws via JSON only |
| Plugin sources / marketplace sources (2673, 2679) | confirmed | six plugin source types, command source limits, `npm` marketplace source "not yet implemented", reserved names |
| Mods: in-process model (2746), event catalogue (2752), API limits (2758), built-in mods (2794) | confirmed | see "Mods" section below |
| Claude Mods (2931) | confirmed | v2.1.287 on-by-default matches overview.md; no preview label on any mods page |
| Model flags (2983), default model and effort (2989) | confirmed | Opus 5.5 default, medium effort, 5 selection layers in the stated order |
| Fallback model chains (3060) | confirmed | max three after dedupe, current turn only, flag beats setting |
| Automatic content-based fallback (3066) | confirmed | Fable/Opus 5.5 bio -> Opus 5, cyber -> Opus 4.8; Sonnet 5.5 cyber -> Sonnet 5, bio refuses |
| Advisor tool (3000) | confirmed | experimental, Anthropic API only, rank table, flag hidden from --help |
| Fast mode (3096) | confirmed | prices, 2.5x, Opus 4.8 deprecation 2026-10-30 / removal 2026-11-30, research preview for Opus 5/4.8 |
| Model allowlists and locks (3186) | confirmed | deniedModels and availableModelsMatch need v2.1.283 (page), managed-only |
| Self-hosted runners status (1803, 1809) | confirmed | public beta, Team/Enterprise, off by default, outbound-only |

### Mods: what the docs say it is (the survey's most surprising claim)

The claim is real and accurately described. plugins/mods/overview.md: a mod is "a plugin" whose JS/TS hook functions run inside Claude Code; events.md calls the handlers "a middleware chain" with `next(e)`; a hook can observe, rewrite or answer (answer = return without calling `next`, so Claude Code's own behaviour does not run). Status: no preview/experimental label anywhere; "on by default" in terminal v2.1.287+ and Desktop v2.1.286+ (admin.md says v2.1.286+). Limits confirmed on reference.md: 10 s per hook (50 ms for `prompt.edit`), `$.process.run` 30 s default / 10 min max, 4 MiB for fs/http/store, newest 4,096 messages. Not sandboxed (overview.md: "Mods aren't sandboxed"); a mod can approve tool calls ahead of the user's prompt.

What the survey under-states (not wrong, but a reader needs it):
- Failure is fail-open by default: a hook that throws or times out is skipped and the tool call goes ahead, unless the author adds a `.catch` handler (events.md "Handle a hook that fails"). The survey entries at 2746-2762 do not say this.
- The guard: a built-in `cc-plugin-sec-default` mod loads first only if the machine has managed settings or `managed-mcp.json`, or the session uses a Team/Enterprise `/login`. API-key users with no managed settings get no guard. With the guard, a user's mod cannot approve what a deny rule, an ask rule or a blocking PreToolUse hook stops; but a mod can still read `.env` through `$.fs.read` and run processes with `$.process.run` regardless of Read deny rules (admin.md).
- Admin controls missing from the survey: `allowManagedModsOnly` (option on the guard, set under `pluginConfigs`), `allowModsToOverrideDenyRules`, `prependPlugins`/`appendPlugins`, and the order (guard and org mods, then user mods, then appendPlugins, then other built-ins).
- Hook-order detail: managed-settings PreToolUse hooks run before any mod's `tool.call`; other settings/plugin PreToolUse hooks run after the last mod.

### Duplicate or overlapping entries in the range

- Cross-session messaging: 1445 (+ 4 sub-entries) vs 1696. Consistent, but 1696 Notes says the changelog wording "on any of your machines" differs from "same machine" in the digest; the page confirms same-machine needs no Anthropic servers and cross-machine needs Remote Control, so the changelog wording is loose, not the survey.
- Dynamic workflows: 1626 and 1685 (plus 1632). No contradiction.
- Subagent model: 1559 vs 1643 (same order, both correct). Depth/concurrency: 1583, 1661, 1667 (same numbers).
- Background sessions / agent view: 1708 (GA), 1737 (research preview), 1743 (research preview), 1749 (research preview), 1826 (research preview at launch, "the rest is GA"). These contradict each other on status. The docs say research preview for the whole of agent view, the supervisor, `claude --bg`, attach/logs/stop. See corrections.
- Fork: `/fork` at 1481 and 1890 consistent; 1577 says `/subtask` is the in-session fork, consistent with agent-view.md ("When agent view is turned off, /fork starts a forked subagent").

### Corrections to apply

1. Heading "### Background sessions and supervisor CLI (agent view)" (line 1708): `Status: GA` is wrong. Change to `Status: research preview`. Page: "Agent view is in research preview. The interface and keyboard shortcuts may change." URL: https://code.claude.com/docs/en/agent-view.md. Add to Notes: `(corrected in review: status was GA)`.
2. Heading "### Background sessions and agent view" (line 1826): the Status line "agent view was labeled research preview at launch; the rest is GA" should read "research preview (agent view, `--bg`, daemon, attach/logs/stop all documented as part of agent view)". Same URL. The agent-view.md page has no "GA" graduation notice.
3. Heading "### Agent view (`claude agents`)" (line 1741): Notes say "Introduced v2.1.139". agent-view.md does not give an introduction version (it only gives versions for individual features). Reword to "Introduced v2.1.139 per the changelog/digest (not stated on agent-view.md)". Could not open the 2.1.139 changelog line (the changelog page is 1.2M chars and the fetch was truncated).
4. Heading "### Hooks: model and configuration" (line 2200): "31 events total" should be "33 events". List on https://code.claude.com/docs/en/hooks.md (Hook lifecycle table): SessionStart, Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest, PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch, Notification, MessageDisplay, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, CwdChanged, DirectoryAdded, FileChanged, WorktreeCreate, WorktreeRemove, PreCompact, PostCompact, PreModelSwitch, PostModelSwitch, Elicitation, ElicitationResult, SessionEnd. Note: the entries in the survey cover all of these, so only the count is off.
5. Heading "### Routine limits and run outcomes" (line 1791) and "### Loops, routines, scheduled tasks" (line 1836): the page https://code.claude.com/docs/en/routines.md is now titled "Automate work with scheduled tasks" and says "A cloud scheduled task, formerly a routine". Add that rename. Also add: minimum schedule interval is one hour (cron expressions that run more often than hourly are rejected); the 30/hour per-task cap is one shared count for Run now, API fires and re-running a one-off task (the entry lists "30 Run-now plus API fires per task", which is right but incomplete). Status "research preview" is confirmed ("Scheduled tasks are in research preview").

### Move to Unverified

None. Nothing in the range was unsupported. Two details inside confirmed entries could not be tested because the fetched text was cut off; leave as is but they are not verified by this review:
- "alwaysLoad ... capped at a 5-second connect timeout" and the 2,048/4,096 description caps (entries at 2588 and 2593): not in the first 100,000 chars of mcp.md (the search found only `MCP_TIMEOUT` 30 s). Self-hosted-environments pages mention a 5 s wait for alwaysLoad servers, which agrees.
- "Skill listing budget 1% of context window" (2482): only the 1,536-char cap was seen on skills.md.

### Borrowed-feature findings

None found. Where similarly named features exist, the survey states the vendor's own mechanism correctly: `context: fork` on skills is noted as "not a fork of the conversation" (skills.md says so); `/fork` vs `/branch` vs `/subtask` are separated correctly; "checkpoints" are correctly described as file-edit snapshots, not git.

### Notable omissions (not in the survey, or only in passing)

1. Mods admin and guard behaviour: https://code.claude.com/docs/en/plugins/mods/admin.md (allowManagedModsOnly, prependPlugins, conditions under which the guard loads).
2. Mods fail-open and `.catch` rule: https://code.claude.com/docs/en/plugins/mods/events.md#handle-a-hook-that-fails
3. Desktop scheduled tasks (third scheduling option, minimum interval 1 minute when Claude sets it): https://code.claude.com/docs/en/desktop-scheduled-tasks.md
4. Mods interface / UI drawing and test kit pages: https://code.claude.com/docs/en/plugins/mods/interface.md
5. Managing plugins for an organisation (allowlists, audit, update policy): https://code.claude.com/docs/en/plugins/org.md
6. `claude agents` dispatch defaults and restricted mode (`--restricted`, v2.1.248): https://code.claude.com/docs/en/agent-view.md
7. Prompt caching page (TTL buckets, why subagent/teammate caches default to 5 min): https://code.claude.com/docs/en/prompt-caching.md
8. Subagent output scanning (harness scans final subagent message for instruction-shaped patterns and prepends a `[harness: ...]` marker, v2.1.210+): https://code.claude.com/docs/en/agent-sdk/subagents.md (What subagents inherit). The survey's context-isolation entry (1530) does not mention it; it matters for a master/node design that passes sub-agent output to a parent.
9. Plan-approval auto-approval in teams: a teammate that finishes planning is approved automatically unless the user changed its mode (agent-teams.md "Have teammates plan before implementing"). Not in the survey's team entries.
10. Teammate permission mode: new teammates start in the lead's mode including bypassPermissions; no per-teammate mode at spawn (agent-teams.md "Permissions"). The survey's limitation list gives it only as "No per-teammate permission modes" in the page but not in the survey entry at 1611.

### Overall trust judgement for the unchecked remainder

High for this range. Every one of the 47 checked entries matched its source, including numbers, versions, defaults and status labels; the only errors were a status label (GA vs research preview), an event count (31 vs 33), a missing product rename, and an introduction version that the cited page does not state. The author's habit of quoting caps and env vars paid off. Weaker spots: the status field on agent-view-related entries (assume research preview unless the page says otherwise); changelog-derived entries (e.g. "Subagent defaults and limits (fast-moving)" at 1667, "Claude Mods" at 2931) cite a 1.2M-char page that this review could only partly read, so version/date claims there remain unchecked; treat them as likely right but not verified. The file is usable, but its mods and hooks entries should get the fail-open and guard caveats above before anyone relies on them for a security design.

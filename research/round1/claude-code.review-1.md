## 9. Review (reviewer 1: Areas 1-8, lines 119-1431)

Date: 2026-11-10. Entries checked: 52 of about 330 entries in lines 119-1431 (file total 805). Checked by fetching the cited code.claude.com pages and asking for the specific fact (permission-modes, sandboxing, auto-mode-config, permissions, memory, context-window, prompt-caching, model-config (first 100K chars only), checkpointing, interactive-mode, tools-reference, sandbox-environments, agent-sdk/permissions, agent-sdk/todo-tracking, agent-sdk/typescript (partly), whats-new/2026-w32, changelog (first 100K chars only)). The file was not edited.

Verdict counts: confirmed 46; confirmed-with-correction 3; not-supported (details only) 3; wrong 0; status-wrong 0; unreachable 0.

### Checked entries

| Entry (line of heading) | Verdict | Reason |
|---|---|---|
| Built-in tool inventory / Glob and Grep absent by default (315) | confirmed | tools-reference: Glob/Grep left out on macOS/Linux/WSL, default on Windows |
| Read tool (309) | confirmed | 25,000 tokens, 256 KB, 100 MB notebook |
| WebFetch (327), WebSearch (333) | confirmed | 100,000 chars, eight backend searches, no combined allow/block lists |
| Monitor tool (339) | confirmed | 5 min default, 30 max, 10 in -p |
| Bash timeouts and output limits (474) | confirmed | 2 min / 10 min, 5 GB kill, 30,000 chars, 150,000 ceiling |
| Background Bash time limit (480) | confirmed | 30 min default, 2 h max |
| Tool memory cap via cgroup (486) | confirmed | CLAUDE_CODE_TOOL_MEMORY_LIMIT, Linux/WSL |
| Rule evaluation order (564) | confirmed | deny, ask, allow; specificity does not reorder |
| Bash rule matching (576), limits (582) | confirmed | separators, wrappers, "not a security boundary" |
| Read-only command set (588) | confirmed | incl. ps env check, UNC prompt |
| Symlink handling (610), network-path read check (615) | confirmed | v2.1.268, v2.1.292 |
| Hooks and mods in the permission pipeline (632) | confirmed | mod can approve past deny only without managed settings / Team login |
| Working directories (638) | confirmed | blockWrites v2.1.314, blockReads v2.1.257 |
| Permissions vs sandbox interplay (644) | confirmed | autoAllowBashIfSandboxed default true; plan mode skips |
| Permission modes (six) (649) | confirmed-with-correction | plan mode is described by the docs as "reads, plus classifier-approved commands when auto mode is available"; the entry says only "research and plan only, edits blocked". Add the classifier detail |
| Actions no mode auto-approves (655) | confirmed | |
| Starting-mode selection (660) | confirmed | v2.1.283 default auto; v2.1.228/233 floors; project auto/bypass ignored |
| Mode switching (666) | confirmed | bypass flag requirement, "switch to auto" prompt not independently checked for v2.1.247 |
| Auto mode availability and model gating (672) | confirmed | model list matches the page, including Bedrock/Foundry list |
| Auto mode decision order (678), default block/allow lists (684) | confirmed | tool results stripped from the classifier; default allow of pushes to any branch of the working repo |
| Repeated-block thresholds (695) | confirmed | 3 consecutive / 20 total, not configurable |
| bypassPermissions safeguards (710) | confirmed | root/sudo refusal, --restricted v2.1.248 |
| Protected paths (715) | confirmed | directory and file lists match |
| Critical-path rm check (721), per-mode countdown (727) | confirmed | two-minute countdown, v2.1.281, EVEN_MORE_DANGEROUSLY v2.1.311 |
| autoMode.environment (738), where autoMode is read (744), classifier rule lists (750), human checkpoints (756) | confirmed | project settings excluded; allow can override org soft_deny; `git -C` evades ask rule |
| /auto-mode-setup (761), claude auto-mode CLI and Recently blocked (767, 772) | confirmed | versions and reads match |
| OS sandbox mechanism and defaults (777) | confirmed | off by default; Seatbelt / bwrap+socat; WSL1 and native Windows unsupported |
| What runs outside the sandbox (783) | confirmed | |
| Sandbox modes (789), strict mode (795) | confirmed | |
| Network isolation and proxy (824), per-command domains (830) | confirmed | v2.1.271, strictAllowlist v2.1.219 |
| Credential protection (836) | confirmed | tlsTerminate labelled experimental; masking v2.1.221 |
| Documented sandbox limitations (847) | confirmed | wording matches "not a complete isolation boundary" |
| Sandbox runtime srt (864), isolation options compared (858) | confirmed | "beta research preview" |
| Permission lockdown / SDK evaluation order / SDK modes (998, 1016, 1028) | confirmed | six steps; v2.1.302 default mode; root refusal |
| Subagent permission-mode inheritance (935, 1034) | confirmed (SDK page) | bypass never applied from subagent v2.1.267 |
| Auto-compaction and window (1135), per-model window (1206) | confirmed | 967K Haiku 5.5, /autocompact, cap at model window; the 100K-1M range, v2.1.288 and CLAUDE_CODE_MAX_CONTEXT_TOKENS could not be reached (page truncated) |
| What survives compaction (1153) | confirmed | five files, 5,000 tokens per skill, 25,000 total, v2.1.304 |
| Prompt cache layout, invalidate, keep (1159, 1165, 1171) | confirmed | |
| Prompt cache TTL selection (1177) | confirmed | v2.1.242, precedence order |
| Idle compaction, first entry (1147) | confirmed-with-correction | 200K+ tokens and the one-hour TTL are confirmed (changelog 2.1.314, prompt-caching.md), but the exact idle time, "machine awake", and the 100,000 floor are not on the pages reached |
| Idle compaction, second entry (1230) | not-supported (details) | "about 54 minutes", "Terminal: 2.1.299, Oct 14, 2026", "/config row 2.1.302" not found; only the 2.1.314 VS Code/Desktop line was seen (Oct 30, 2026) |
| Prompt cache controls (1239) | confirmed-with-correction | docs say promptCacheTtl needs v2.1.242; entry says 2.1.243 |
| CLAUDE.md scopes (1266), imports (1272), rules (1278), managed and excludes (1284) | confirmed | paths, 4 MiB, 40,000 chars, four hops, 1,000 patterns / 4 MiB |
| AGENTS.md (1290) | confirmed | v2.1.277; default mode `claude-md-or-agents-md`. `.claude/AGENTS.md` not found in the text read |
| Additional directories (1296) | confirmed | CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1 |
| Auto memory (1302) | confirmed | four types, 200 lines or 25 KB, modified timestamp, off in self-hosted |
| Memory management commands (1308) | confirmed | prompt-audit v2.1.283 |
| Task list display (1375) | confirmed | Ctrl+T, five tasks, CLAUDE_CODE_TASK_LIST_ID |
| Plan mode entry / gating (1381, 1393), SDK todo tools (1410), planModeInstructions (1416) | confirmed | task tools default only on Opus 4.7, Sonnet 4.6, Haiku 4.5 and earlier (v2.1.268+) |
| Auto mode milestones (1058) | confirmed | Aug 14, 2026 default on Pro/Max/Team (whats-new/2026-w32); Ultraplan removed same week. Per-version items not individually checked |
| Programmatic sandbox settings (1046) | not-supported (detail) | "failIfUnavailable default true" in the SDK not found; CLI docs imply default off. Could not reach the typescript.md sandbox table |
| /btw (1117) | confirmed | no tools, f forks, v2.1.314 subagent transcript |

### Corrections to apply
1. `### Permission modes (six) and what each auto-approves`, line 649: in the `plan` description add that, when auto mode is available, classifier-approved commands also run (docs table: "Reads, plus classifier-approved commands when auto mode is available"). URL: https://code.claude.com/docs/en/permission-modes.md
2. `### Prompt cache controls and diagnostics`, line 1239: change "2.1.243" to "2.1.242" for `promptCacheTtl`/`subagentPromptCacheTtl`, or note that the docs page says v2.1.242 while the entry's source (changelog) was not reached. URL: https://code.claude.com/docs/en/prompt-caching.md ("Choose the TTL yourself": "require Claude Code v2.1.242 or later")
3. `### Idle compaction`, line 1147: keep "200,000 tokens", "one-hour cache TTL", "idleCompaction: false (v2.1.290+)" (confirmed); mark "about an hour idle", "machine awake" and "floor 100,000" as unverified. URL: https://code.claude.com/docs/en/prompt-caching.md (Compacting the conversation)

### Move to Unverified
- `### Idle compaction`, line 1230: "about 54 minutes", "Terminal 2.1.299, Oct 14, 2026", "/config row 2.1.302". Searched changelog page (first 100K chars) and prompt-caching.md. Settle by reading the changelog entries 2.1.299 and 2.1.302 and the `idleCompaction` entry in settings-reference.md.
- `### Programmatic sandbox settings (SDK)`, line 1046: "failIfUnavailable (default true)". Searched agent-sdk/typescript.md (partly). Settle by reading the Sandbox Configuration table. Note the CLI entry at line 777 says the default is off (silent fallback).
- `### CLAUDE.md scopes` / AGENTS.md (line 1290): the claim that `.claude/AGENTS.md` is read was not found in memory.md.

### Duplicate or contradictory entries in this range
- Idle compaction appears three times (1123, 1147, 1230) with different idle times ("about 200K sits idle", "about an hour idle", "about 54 minutes") and different version stories (2.1.290, 2.1.299, 2.1.314). Consolidate into one.
- Compaction window appears twice (1135, 1206) and subagent inheritance twice (935, 1034); these agree with each other.
- Auto mode classifier engine: line 705 says server-side review is "rolling out" with v2.1.271-282 thresholds and covers cloud providers from v2.1.278; line 1062 says the local classifier is the default on Bedrock/Vertex/Foundry "for now (2.1.273)" and server-side by default on direct API with telemetry off (2.1.282). Different dates; the later entry is stale relative to 705.
- Project-file handling of `defaultMode`: line 1000 says auto and bypassPermissions are ignored from project files "since v2.1.257"; line 1098 dates the bypass ignore to Week 36. Not checked against the changelog; one date is wrong.
- Auto-mode default: line 661 (v2.1.283, all interactive sessions) versus line 1061 (Aug 14, 2026 for Pro/Max/Team). w32 supports the Aug 14 date for those plans; the 283 date was confirmed by permission-modes.md. They describe different stages, not a conflict, but should be cross-referenced.
- promptCacheTtl version: 2.1.242 (line 1178) versus 2.1.243 (line 1241).

### Borrowed features
None found. The auto memory, CLAUDE.md, sandbox and permission entries use this vendor's own mechanisms and quote the docs closely.

### Notable omissions (my range)
1. Dev container reference configuration with default-deny firewall: https://code.claude.com/docs/en/devcontainer.md (only 2 mentions in the file).
2. Enforce sandboxing with managed settings (admin-required sandbox, `allowManagedDomainsOnly`): https://code.claude.com/docs/en/sandboxing.md
3. Compaction uses the session model's cache prefix, fallback chains apply to compaction, and the classifier gets over-long conversations compacted; no entry states which model summarises: https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/prompt-caching.md
4. Checkpointing limits (100 most recent checkpoints, bash-only changes not tracked, subagent edits not restored, symlinks skipped, about 30-day retention): https://code.claude.com/docs/en/checkpointing.md (the file has Area 11 entries for this at lines 1902-1974; not checked here).
5. No documented clearing of old tool results as a context-management step was found on the pages read; the file does not claim one.

### Overall trust
High for the entries checked: Area 5, 6 (except idle compaction), 7 and 8 matched the cited pages closely, including numbers and version gates. Risk is concentrated in changelog-derived "milestone" entries (1058-1107, 1230-1256), which cite changelog and what's-new pages that I could only partly reach; treat their version numbers and dates as likely but unconfirmed. The unchecked remainder of Areas 1-4 (sampled: tools, Bash) looked reliable.

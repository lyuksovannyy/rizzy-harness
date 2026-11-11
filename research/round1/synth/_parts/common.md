### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

Counted over the 16 real harnesses (OpenCodex excluded as a gateway; Claude Desktop and Zed/ACP included), as documented-and-released `Y` plus partial `B`, and at least 8 of 16. Counts come from the matrix and carry its caveat: `-` means nothing documented.

| Feature | Y + B of 16 | Note |
|---|---|---|
| Shell execution model | 15 (13 + 2) | Universal; the difference is process per command versus persistent shell. |
| Built-in tool inventory published | 14 (10 + 4) | Codex, Antigravity, OpenHands, goose, DeepSeek and Warp give partial or no list. |
| Loop placement relative to tools | 13 (10 + 3) | See the entry: opposite answers to our "node runs the loop". |
| Loop structure, turns and stop reasons | 12 (9 + 3) | |
| Steering and queueing while a turn runs | 12 (11 + 1) | Default queue versus default steer differs. Ours has none. |
| Interrupting a running turn | 11 (8 + 3) | |
| Tool-set composition (defaults, presets, gating) | 11 (10 + 1) | |
| Ask-the-user tool | 11 (9 + 2) | Ours uses `ui:` blocks. |
| Web fetch and web search tools | 11 (10 + 1) | |
| Background processes | 11 (8 + 3) | Pi has it as a deliberate non-feature. Ours has none. |
| Todo and task-list tool | 10 (6 + 4) | Claude Code withdrew it for newer models. |
| Goal and objective-continuation loops | 10 (6 + 4) | Every one of them was added recently; mechanisms differ (who judges completion). |
| Whole-file write and create | 10 (9 + 1) | |
| Diff review and in-app file view or edit | 9 (9 + 0) | |
| Step, turn and budget limits | 8 (8 + 0) | Two of the eight are only a protocol stop reason (Zed/ACP) and a mistake limit (Cline). |
| Event log as source of truth with one writer | 8 (6 + 2) | |
| Deferred tool loading | 8 (7 + 1) | On by default in Claude Code, Hermes, OpenClaw, Antigravity. |
| File mentions and attachments | 8 (8 + 0) | |
| User shell escape (`!`) | 8 (8 + 0) | |

Edit formats are not in this list because six harnesses do not document how their edit tool matches text. Among those that do, exact search/replace appears in 6 (Claude Code, OpenCode, DeepSeek, OpenHands, Hermes, Zed), patch format in 4 (Codex, OpenCode, OpenClaw, Cline), and hash-anchored lines in 1.

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

"Real harnesses" here are the sixteen excluding OpenCodex (a gateway). Half is eight. Counts use `Y` or `B` in the matrix.

| Feature | Count (of 16) | Comment |
|---|---|---|
| F1 Approval modes | 15 | Everyone but Pi, which documents not having one. Defaults split three ways (F1 Variants); ours is prompt-by-default, the conservative end. |
| F2 Per-tool/command rules | 15 | Universal (several partial). Ours has only whole-tool `denied_tools`. |
| F32 Telemetry stated | 13 | Defaults vary widely: on by default without content (Claude Code, Cline, Pi install telemetry, OpenClaw version check), session-log upload on by default (DeepSeek), opt-in (goose). |
| F34 Stated non-boundaries | 12 | The norm is to say in writing that rule lists and approvals are not a boundary. |
| F9 Admin/managed policy | 12 | Mostly org-level push from a vendor console, MDM or system file. |
| F5 Model-based approver | 12 | The fastest-moving area. Codex, Claude Code, Cursor and OpenClaw ship a separate reviewer; Codex, Claude Code and OpenClaw add a circuit breaker back to a human. |
| F24 Own credential storage | 10 | Keyring with plaintext fallback is the norm. |
| F29 Extension/plugin trust | 10 | Everyone who has plugins says they are unsandboxed. |
| F31 Remote link auth | 10 (Cursor `?`, Zed/ACP `P` not counted) | Dominant pattern: outbound-only connection from worker to control plane with short-lived credentials. |
| F3 Shell parsing | 9 | Every harness that parses converges on the same rule: split plain chains, refuse or exact-match anything with substitution. |
| F8 Workspace boundary | 9 | Usually enforced only for built-in file tools, not the shell. |
| F15 Container/VM isolation | 9 (goose `?` not counted) | Mostly vendor cloud VMs or documented recipes. |
| F22 Credential placeholder/proxy | 9 | The strongest-growing secret pattern; ours has none. |
| F30 Own server/UI auth | 9 (goose `?` not counted) | Ranges from strong (ours, DeepSeek launch token, OpenCode password) to none (OpenHands default, Cline hub and Kanban). |
| F33 Retention stated | 9 | |
| F17 Unattended behaviour | 8 | Deny-and-continue is converging as the safe default; Cline and Cursor cloud are the exceptions. |
| F25 Prompt-injection defences | 8 (goose `?` not counted) | Mostly content marking and pattern scans; nobody claims to solve it. |

Just under half (seven each), listed because they matter for the comparison: F11 OS sandbox for the shell (Codex, Claude Code, Claude Desktop, DeepSeek, Cursor, Antigravity, Zed; goose and Cline unverified), F16 sub-agent inheritance, F18 remote approval delivery, F19 hooks in the permission path. Seatbelt on macOS is used by every harness with a macOS sandbox; Linux splits between bubblewrap (Codex, Claude Code, Zed, Antigravity) and Landlock plus seccomp (Cursor; DeepSeek offers both).

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Denominator: 16 real harnesses (OpenCodex is a gateway and excluded); Zed/ACP counted as one. Counts are `Y` or `B` cells above.

| Capability | Count | Who |
|---|---|---|
| Automatic summarising compaction | 16 of 16 | all except OpenCodex (which proxies it) |
| Manual compaction command | 14 of 16 | not Hermes (nothing documented), not ours |
| Instruction files with `AGENTS.md` or an equivalent project file | 14 of 16 | not DeepSeek harness (unclear), not Claude Desktop (own survey), not ours |
| Scopes, precedence or conditional loading of instruction files | 14 of 16 | same set (ours is partial) |
| Reads other vendors' instruction names | at least 9 of 16 | OMP, Pi (`CLAUDE.md`), Hermes, Zed, goose, Cline, Warp (via `/init`), OpenHands, Cursor (`.claude`/`.codex` agents), Claude Code (`AGENTS.md`) |
| Compaction extension points (hooks, plugin, template, model choice) | 9 of 16 | Codex, OpenCode, Claude Code, Pi, Cursor (observe only), OpenClaw, goose, Hermes, Zed (model choice only) |
| Agent-written memory of some kind documented | 11 of 16 | Codex, Claude Code, Claude Desktop, OMP, Cursor, OpenHands, OpenClaw, Hermes, Warp, Cline (convention), goose (unclear) |
| Context-usage display | 9 of 16 (counting ours as partial: 10) | Claude Code, Claude Desktop, OMP, Antigravity, OpenHands, goose, Zed, Cline, DeepSeek harness (API only) |
| Memory off by default or easily disabled | majority of the 11 above | Codex, OMP, OpenHands off by default; Claude Code, Claude Desktop, Cursor, Warp have toggles |

Skills directories (`SKILL.md`) are read in many of these harnesses but belong to another area; here they matter only as agent-written memory.

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

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

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

"Documented" here means `Y` or `B` in the matrix. Denominator is the 16 real harnesses (OpenCodex excluded; Zed/ACP counted as one). Half = 8.

| Feature | Count of 16 | Notes |
|---|---|---|
| Resume / continue | 15 | Only OpenCode lacks it in the survey ("nothing documented", not absence). |
| Archive / delete / retention / ephemeral | 15 | Mostly basic list/delete; the interesting differences are retention defaults. |
| Session storage documented | 13 | Strong families: JSONL (Codex, Claude Code, Oh My Pi, Pi, DeepSeek), SQLite (OpenCode, OpenClaw, goose, Hermes, Cline), file-per-event (OpenHands). |
| Worktrees for parallel agents | 14 | Everyone except Pi and goose (and Ours as user feature). Differences are in who gets one and how cleanup works. |
| Fork to a new session | 12 | Several (Antigravity, OpenHands) explicitly do not fork file state. |
| Diff views | 12 | Mix of full review panes (Warp, Antigravity, Claude Desktop, Cline) and API-only (OpenCode, OpenHands). |
| Conversation rewind / edit-and-resubmit | 11 | Counting Oh My Pi and OpenClaw as `B`. |
| Git host integrations / credential handling | 10 | |
| Export a session | 10 | Format and redaction differ widely. |
| Move session between machines/surfaces | 9 | Codex, Claude Code, Claude Desktop, Cursor, Warp are the full ones. |
| Search across sessions / picker | 8 | Exactly half. |
| Hosted PR review bot / CI action | 8 | Exactly half; 5 are GA-style (Codex, Claude Code, Cursor, OpenHands, Warp). |
| Worktree lifecycle / cleanup | 8 | Exactly half, GA in most. |

Below half but notable: file checkpoints (6 of 16 including OpenHands `undo_edit` as `B`; GA in OpenCode, Claude Code, Cursor, Hermes (off by default), Cline), commit-message generation (7), worktree setup/ignored-file copy (7), share by link (5), PR creation (5-6), local review command (4).

The notable negative pattern: 10 of 16 harnesses document no file-level undo at all (Codex, DeepSeek, Oh My Pi, Pi, Claude Desktop, Antigravity (unverified), OpenClaw, goose, Warp, Zed after removal), and the ones that do split between shadow git (OpenCode, Hermes, Cline, Ours) and non-git file snapshots (Claude Code, Cursor).

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

Real harnesses counted: 16 (OpenCodex excluded as a gateway; Zed counted). Threshold: 8. Counts include `Y` and `B`.

| Feature | Count | Who lacks it (nothing documented) |
|---|---|---|
| MCP client | 16/16 | none |
| Skills in SKILL.md format | 16/16 | none |
| Plugin bundles | 15/16 | Warp (own agent) |
| Plugin distribution / marketplaces | 15/16 | Warp |
| MCP remote auth (OAuth) | 14/16 | Claude Desktop (not described), DeepSeek harness |
| MCP approval, trust, allow/deny | 14/16 | DeepSeek harness, OpenClaw |
| Custom slash commands / templates | 14/16 | Claude Desktop, DeepSeek harness (Cursor partial) |
| Programmatic control protocol | 13/16 (+Antigravity unverified) | Claude Desktop, Zed |
| Reads other harnesses' files in place | 13/16 | Claude Desktop, Antigravity, Hermes Agent |
| Embeddable SDK | 13/16 (OpenClaw unverified) | Claude Desktop, Zed |
| Plugin trust and supply-chain controls | 14/16 (6 of them only partial: OpenCode, DSH, OMP, Pi, Antigravity, OpenHands) | Zed, Warp |
| MCP beyond tools | 12/16 | Antigravity, OpenHands, Warp, Cline |
| Skill loading budget and invocation control | 14/16 | Claude Desktop, Warp |
| Hook trust, precedence, failure documented | 12/16 | Claude Desktop, DeepSeek harness, Zed, Warp |
| Custom tools in code | 11/16 | Codex, Claude Desktop, goose, Zed, Warp |
| Hooks (config-declared) | 10/16 (+2 unverified) | OpenCode and Pi (use in-process code instead), Zed, Warp |
| Skill registries and account sync | 10/16 | DeepSeek harness, OMP, Cursor, Antigravity, goose, Warp |
| Tool deferral / tool search | 9/16 | Claude Desktop, DeepSeek harness, Cursor, OpenHands, Zed, Warp, Cline |
| ACP agent (server) | 9/16 (+Warp unverified) | Codex, Claude Code, Pi (all adapter-only), Claude Desktop, OpenHands, Zed (client) |
| In-process extension API | 9/16 | Codex, Cursor, Antigravity, goose, Zed, Warp |
| Managed admin controls over extensions | 9/16 | DeepSeek harness, OMP, Pi, Antigravity, Hermes, Zed, Warp |
| Hook handler types beyond command | 8/16 (+Antigravity unverified) | OpenCode, Claude Desktop, DSH, Pi, goose, Zed, Warp |
| MCP limits and lifecycle documented | 8/16 | Claude Desktop, OMP, Pi, Cursor, OpenHands, OpenClaw, Zed, Cline |

Two observations the counts hide:
- The skill format is the one true standard (agentskills.io plus `.agents/skills/`), but frontmatter extensions, precedence and trust gates differ per harness, so a skill is portable as text, not as behaviour.
- "Plugins" in 15 harnesses means two different things: declarative bundles of skills, hooks and MCP config (Claude Code shape, copied by Codex, Cursor, OpenHands, Antigravity, OMP fallback) and code packages that run in-process (Pi, OMP, OpenCode, OpenClaw, Hermes, DSH, Cline). Only the first is safely portable.

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Counted over the 16 real harnesses (the 17 minus OpenCodex, a gateway; Zed/ACP counted as a harness), `Y` or `B` cells only, threshold 8 of 16.

| Feature | Count (of 16) | Note |
|---|---|---|
| Provider support and BYOK | 16 | Universal. Breadth and credential handling vary. |
| Model selection per session, agent, task | 16 | Universal; the scope ladder differs. |
| Cloud-platform providers (Bedrock, Vertex, Azure) | 15 | Absent only from DeepSeek harness. Ours is the gap. |
| Custom endpoints and wire formats | 15 | Cursor is the exception. Only Codex and Claude Code constrain the wire format hard. |
| Token and cost display | 15 | DeepSeek harness unverified. Cost in currency is a minority within this. |
| Logs, transcripts, raw request logs | 15 | Raw provider request logs are rare (goose documents them). |
| Voice | 13 | Mostly input dictation; realtime and TTS in OpenClaw, Hermes, Oh My Pi, Cursor, Antigravity. |
| Debugging and diagnostics tools | 13 | `doctor` is the common shape. |
| Vendor telemetry and upload defaults | 13 | Defaults differ more than the existence of telemetry. |
| OpenTelemetry and other export | 12 | Claude Code, Codex, OpenHands, goose, OpenClaw, Oh My Pi lead; four more are partial or enterprise-only. |
| Local models | 11 | |
| Hosted model offerings (commercial) | 11 | Vendor services, not harness features. |
| Budgets and limits | 11 | Mostly vendor plan quotas; user-set USD budgets are Claude Code, OpenCode Console, OpenHands Enterprise, Zed. |
| Usage dashboards and quota views | 11 | |
| Reasoning effort controls | 10 | |
| Subscription login or harness as provider | 10 | Legal limits documented only by Anthropic, Antigravity and OpenCodex. |
| Model roles for auxiliary work | 9 | |
| Vision and other modalities | 8 | |
| Routing across providers, credential pools | 8 | |
| Model catalogue and discovery | 8 | |

Fewer than half: provider retry (7), model fallback chains (6), governance (5), speed tiers (4), prompt-cache controls (4).

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

Counts are of Y and B together out of the 16 harnesses (OpenCodex and ours excluded); "Y only" in brackets. Half is 8.

- Approval presentation in a UI: 15 of 16 (Y 14). Everyone except Pi, which has no permission system by design.
- Several agents or sessions at once: 14 of 16 (Y 9). Only Pi and goose have nothing documented.
- Diff presentation and review: 14 of 16 (Y 11). Missing: Pi and goose.
- Desktop app: 13 of 16 (Y 10).
- Terminal UI: 12 of 16 (Y 12); voice: 12 of 16 (Y 10); the agent's own browser: 12 of 16 (Y 10).
- Web UI: 12 of 16 (Y 7). Five of those are hosted and closed (Claude Code, Claude Desktop, Cursor, Antigravity, Warp); the open, self-served ones are OpenCode, OpenHands, OpenClaw, Hermes, DeepSeek (preview), Cline (Kanban preview) and Oh My Pi (collab viewer only).
- Accounts, roles or organisations: 12 of 16 (Y 11), but only OpenClaw and Hermes have them in open code; the rest keep them in a paid or hosted tier.
- Managed configuration or policy distribution: 12 of 16 (Y 7); shared team resources: 12 of 16 (Y 5).
- Plan review: 11 of 16 (Y 6); SSO and provisioning: 11 of 16 (Y 8); browser isolation or site policy: 11 of 16 (Y 7); computer use: 10 of 16 (Y 6).
- Mobile: 9 of 16 (Y 5); audit logs: 9 of 16 (Y 3).
- Exactly half (8): IDE integrations (Y 8), messaging-app channels (Y 7, B 1), budgets and usage controls (Y 6, B 2), session sharing or live collaboration (Y 7, B 1).
- Below half: notifications and waiting signals (6 of 16, Y 4); the user's real browser (7 of 16, all Y).

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

Counted over the 17 surveyed harnesses (Y or B); "half" means 9 or more.

- **Programmatic control protocol: 15 of 17.** Every harness except OpenCodex (a gateway) and Claude Desktop documents one. The split is stdio-to-child (Claude Code SDK, Pi, DeepSeek harness `sdk`, ACP) versus long-lived server with HTTP/WebSocket (Codex, OpenCode, OpenHands, OpenClaw, goose, Hermes Agent, Cline hub). See 2.3.
- **One-shot headless run: 13 of 17.** Missing only from OpenCodex, Claude Desktop (by statement), OpenClaw (not in the survey) and Zed/ACP; OpenHands' is unmaintained. Ours has none. See 2.1.
- **Relay-based remote control from web or phone: 13 of 17.** The vendor relay is the commercial norm (Claude Code, Codex, Antigravity, Cursor, OpenCode OpenTunnel); none of them is self-hostable except as a pattern. See 2.3.
- **Pairing and authentication of remote clients and hosts: 13 of 17.** Single-use short-lived codes are the shared idiom (OpenCode, OpenCodex, OpenClaw, Codex). See 2.3.
- **Reconnect and resume semantics: 12 of 17**, with very different strength (from Claude Code's takeover rules to DeepSeek harness's "SSH loss is final"). See 2.3.
- **ACP support: 11 of 17** (12 if Warp's changelog mention is real). Nearly all are stdio only; goose alone ships ACP over HTTP/WebSocket. See 2.3.
- **Structured event stream: 11 of 17.** See 2.1.
- **SSH remote execution: 11 of 17** (12 if OpenCodex's release-note SSH setup is real). Split evenly between "whole agent on the remote" and "tools only on the remote". See 2.4.
- **Approval behaviour with no human present: 10 of 17**, with opposite defaults (fail closed versus approve everything). See 2.1.
- **Exposure beyond loopback documented: 10 of 17.** Loopback by default is now the norm among servers; the exceptions are Codex's unauthenticated non-loopback rollout default and container images that bind wide (OpenClaw, OpenHands Docker mode). See 2.3.
- **Event-triggered remote runs: 9 of 17.** See 2.2.

Just under half: vendor-hosted cloud agents (8), remote attach of a client to an agent server (8).

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

Count is out of the 17 listed names (half is nine). `Y` only; `B` counted in brackets.

| Feature | Harnesses documenting it | Count |
|---|---|---|
| Licence, open/closed, install and pricing information | all 17 | 17 |
| Config file formats and locations | Codex, OpenCodex, OpenCode, Claude Code, OMP, Pi, Cursor, Antigravity, OpenClaw, goose, Hermes, Zed, Warp, Cline (+ Claude Desktop, DSH, OpenHands as `B`) | 14 (+3) |
| Environment variables as an override/input layer | Codex, OpenCodex, OpenCode, Claude Code, OMP, Pi, OpenHands, OpenClaw, goose, Hermes, Warp (+ DSH) | 11 (+1) |
| Reading other harnesses' config or instruction files | Claude Code, Codex, OpenCodex, OMP, Cline, Warp, Cursor, goose, Hermes, Zed (+ OpenCode `AGENTS.md` only, Antigravity compat) | 10 (+2) |
| Managed (admin) config layer | Codex, OpenCode, Claude Code, Claude Desktop, Cursor, goose, Hermes, Warp, Cline | 9 |

Close to common: precedence documented as an ordered list (Codex, OpenCode, Claude Code, OMP, Pi, goose, Hermes: 7, plus 6 partial), themes (8), hot reload or invalid-file handling (7 plus 2 partial), programmatic config editing (7 plus 1).

The ecosystem has converged on: a user file plus a project file, with a project layer that is distrusted for anything touching credentials or execution; `AGENTS.md` as the lowest common denominator instruction file (OpenCode V2 reads only that; Warp, Zed, Hermes, goose, OMP, Cline, Cursor all read it).

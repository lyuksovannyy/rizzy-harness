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

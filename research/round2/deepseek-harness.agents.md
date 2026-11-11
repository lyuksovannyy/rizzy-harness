# deepseek-harness: agents cluster (Round 2 source reading)

Permalink base used below: `https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/`

## 1. Repository

- `deepseek-ai/deepseek-harness`, branch `master`, pinned SHA `fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159`, commit date 2026-11-04T16:40:00+08:00 (from `research/round2/00-repos.tsv` and `git log -1`).
- Read: the agent loop (`packages/core/agent-loop`), system prompt (`packages/core/system-prompt`), the edit tool and filesystem (`packages/fs/tool-fs`, `fs-local`, `fs-observation-policy`), compaction (`packages/compaction/*`), instruction loading (`packages/context/agent-instructions`), sub-agents (`packages/subagent/*`), workflows (`packages/workflow/*`), teams (`packages/team/*`), LLM retry (`packages/llm/llm`, `llm-retry`), guards, spill, plan mode, and the shipped bundle YAML (`packages/bundle/base`, `bundle/web-app/presets`). Test files were not used as evidence. Repository `AGENTS.md`/`CLAUDE.md` files were read as data only and not followed.
- Not read in depth: the tool pipeline internals (safety cluster), MCP, sessions persistence, the generated docs catalogue.

## 2. Architecture as it bears on this cluster

- **Language and process model.** TypeScript on Node. Everything is a Cordis plugin; a "profile" is a YAML list of plugin rows (`id`, `name`, `config`, `disabled`) stacked from bundle patches. The loop, session log, tools, compaction, sub-agents, retry, guards are separate rows in one process (in-process sub-agents) or child processes (external harnesses).
  - Shared core row list: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L15-L30 (header comment says row order has no load semantics; activation is service-availability driven, lines 1-13).
- **Capability seam pattern.** A Service Definition (abstract class registered on `ctx`, e.g. `ctx.compaction`, `ctx.subagents`, `ctx.workflowEngine`) plus one or more providers plus consumers (tools). Example: the abstract `CompactionEngine` with `compactIfNeeded/compactNow/compactRegion`:
  - https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction/src/index.ts#L119-L193
- **The loop is a plugin, but not composed from plugins in the loop's own body.** `agent-loop` creates `ReactLoopAgent`s and exposes extension points as Cordis events/waterfalls (`agent/pre-step`, `agent/request`, `agent/request-error`, `tools/post-execute`, `session/event`). Behaviour such as compaction, retry, repeat reminders, instruction loading and plan-mode narration are listeners on those events; none is called from the loop by name.
  - Factory ownership and teardown: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/index.ts#L52-L100
- **Per-agent composition ("agent presets").** A preset is a list of plugin rows mounted into a per-agent scope; two agents in one process can have different tools, persona, compaction and delegation rows. Rows that publish services must use an `isolate` realm or mounting is rejected.
  - https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/preset/agent-preset-registry/src/mount.ts#L204-L216
  - Shipped `standard` preset: persona, instructions, tools, plan mode, compaction, delegation, workflow groups: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/presets/standard.patch.yml#L8-L130
  - The `minimal` preset replaces the persona with `complete: true` (the sole prompt section) and turns off read/write/edit: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/presets/minimal.patch.yml#L10-L23
- **Event-sourced log is the context source.** Anything model-visible is appended as a session event; `surface` nodes are the replayable message list (compaction rewrites the surface with `replace` ops, it does not delete log events).

## 3. Findings

### System prompt assembly and tool descriptions

#### Prompt = ordered named sections with central order slots, per-scope shadowing, strict interpolation
- What the code does: `SystemPrompt` keeps named sections, each with a numeric `order` taken from a central table (`SECTION_ORDERS`: identity -1000, persona prefix 0, plan 500, team 600, per-tool sections 1000-3000, MCP 3100, persona suffix 10200). Sections sort by order then by code-unit name. A scoped (per-agent) registration with the same name shadows the global one; a duplicate in one layer throws. `{{variable}}` references are strict (unknown variable throws). A "complete" section replaces all others (used by the `minimal` persona). Tool sections return `''` when their tool is not visible in the scope, so the prompt tracks the tool set.
- Trace:
  - Order table: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/system-prompt/src/index.ts#L136-L172
  - Assemble (variables, scoped shadowing, sort, complete-section restore): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/system-prompt/src/index.ts#L620-L697
  - Identity line is the only harness-owned text in the base: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/system-prompt/src/index.ts#L453-L471
  - Per-tool section that disappears with the tool (the `edit` section returns `''` when the tool is not in scope): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/edit.ts#L76-L83
- Edge cases and failure handling: more than one complete section makes assembly throw (inside the same `assemble` function, the check precedes section resolution); assembly errors fail the step.
- Round 1: `confirmed` (prompt assembled from ordered sections and tool schemas); `new (not documented)` for the central order table and per-scope shadowing.
- For rizzy-harness: a single order table with named slots plus "section returns empty when its tool is absent" is cheap and keeps the prompt consistent across modes. Copy it.
- Confidence: read the definition (checked in review).

#### Dynamic facts are not in the system prompt; they ride as a user message so the prompt stays cache-stable
- What the code does: `CONTEXT_ORDERS` (working directory, sandbox policy, approval policy, sub-agent delegation, process-specific paths) are a separate set of "contexts". The loop re-renders them into one `runtime-context` user message at each step but logs a new message only when the rendered text differs from the retained snapshot (`RuntimeContextProjection.project` returns nothing when equal), and emits a "cleared" notice when none apply. The system prompt itself is only re-committed when its rendered text, the surface, or the tool set changes (and, per route capability, as a later `system` message in history rather than a replacement).
- Trace:
  - Context order table (comment: process-specific paths stay out of the cached prompt): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/system-prompt/src/index.ts#L177-L184
  - Step: refresh contexts, project the system prompt, append `system/message` and `user/message` events: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L441-L478
  - Runtime-context ownership and the "cleared" text: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/runtime-context.ts#L1-L21
  - Tool-set change detection against the logged request header: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L319-L324
- Edge cases and failure handling: a plugin may suppress runtime context (`suppressRuntimeContext`); the `minimal` preset does (`includeRuntimeContext: false`). Required contexts survive suppression (`refreshContext`, lines 575-600 of system-prompt/src/index.ts, same file as above).
- Round 1: `new (not documented)`.
- For rizzy-harness: this is the best idea in the area. Put cwd, sandbox mode and approval policy in a per-step user-role message, not the system prompt, so the cached prefix survives mode switches. The cost is one extra logged message per step that changes.
- Confidence: traced end to end (checked in review) (corrected in review: the snapshot is refreshed every step but appended only when its text changes, [runtime-context.ts L148-L169](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/runtime-context.ts#L148-L169); whether older snapshots are dropped from the surface was not traced).

#### The same prompt discipline is used by the plan-mode and team sections (state in log, text appears conditionally)
- What the code does: plan guidance and team policy are sections whose text is `''` unless the session's logged state is active. The plan-mode text itself is deployment config (YAML), not code.
- Trace:
  - Plan section conditional on session state: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/plan/plan-mode/src/index.ts#L197-L205
  - Plan text lives in the base bundle for the headless, ACP and SDK profiles: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L336-L350 ; the default web profile disables the host-level row and mounts `plan-mode` per agent preset with its own copy of the text: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/presets/standard.patch.yml#L54-L68
  - Team policy conditional on Team mode: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/team/tool-agent-team/src/index.ts#L112-L117
- Round 1: `confirmed` in outline.
- For rizzy-harness: keep prompt text in config, not code, so deployments can edit it.
- Confidence: read the definition (corrected in review: the plan text is in the preset files for the web profile, not only the base bundle).

### Edit tools and formats

#### `edit` is literal search and replace: no fuzzy matching, CRLF-normalised, unique match required
- What the code does: `edit(file_path, old_string, new_string, replace_all?)`. Input checks: non-blank path, non-empty `old_string`, `old_string !== new_string`. In the filesystem provider the file is read as UTF-8 (binary or non-UTF-8 rejected), CRLF is collapsed to LF for matching, `old_string` is counted by literal occurrence, zero matches throws `FS_EDIT_NOT_FOUND`, more than one throws `FS_AMBIGUOUS_EDIT` unless `replace_all`. The edited text is written back with the file's detected line-ending style (majority CRLF vs LF) via an atomic write under a per-target lock. There is no whitespace-tolerant or fuzzy fallback, no quote normalisation.
- Trace:
  - Tool definition, argument checks, intent waterfall then `ctx.fs.editText`: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/edit.ts#L47-L57 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/edit.ts#L119-L148
  - Matching and ambiguity error: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/fsio.ts#L862-L882
  - Counting and line-ending helpers: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/fsio.ts#L731-L763
  - Provider `editText` under lock, then match, restore line endings, atomic write: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/index.ts#L250-L277
- Edge cases and failure handling: errors are thrown (no partial write); a sandbox denial is mapped to a `[sandbox: ...]` marker. Success output is the Claude-style sentence "The file X has been updated successfully." with a diff card computed from before/after (`presentationMeta`, edit.ts lines 99-118).
- Round 1: `confirmed` ("edit replaces literal text, optional replace_all"); `new (not documented)` for the CRLF handling and the "no fuzzy fallback".
- For rizzy-harness: the unique-match rule plus a count in the error is enough; the lack of any fuzzy step means models must re-read on mismatch. Consider adding a "closest match" hint to the error (not present here).
- Confidence: traced end to end (checked in review).

#### Read-before-edit guard exists as a plugin but is DISABLED in the shipped base bundle
- What the code does: the freshness guard is the optional `fs-observation-policy` plugin. It listens to `fs/edit-intent`, `fs/write-intent`, `fs/delete-intent`, `fs/observed`, keeps a WeakMap keyed by agent session then target, and returns `{ version }` for the provider's compare-and-set, or throws `FS_NOT_OBSERVED` / `FS_NOT_FOUND`. The version token is `dev:ino:size:mtimeNs:ctimeNs`. In `bundle/base` the row is `disabled: true`, and no other bundle YAML I searched (`packages/bundle/*`) re-enables it. With no listener, `fs/edit-intent` yields `undefined` and the edit is unconditional (still serialised by the per-target lock). Yet the `edit` prompt section still tells the model "Read a file before editing it unless you just created or edited it in this session."
- Trace:
  - Disabled row: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L292-L294
  - Guard logic: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-observation-policy/src/index.ts#L78-L103 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-observation-policy/src/index.ts#L115-L141
  - Version token: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/fsio.ts#L75-L78
  - Single-slot default `() => undefined` (no policy means no guard): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/edit.ts#L131-L140
  - Prompt asks for reads regardless: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/edit.ts#L76-L83
  - Model-facing remedy text: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/error.ts#L21-L42
- Edge cases and failure handling: when enabled, a stale version is checked inside the per-target lock before matching, so a stale read reports `FS_STALE_VERSION` rather than a misleading not-found (fs-local index.ts lines 254-262, cited above). State is per session object, so a resumed session starts with nothing observed. Direct tool calls with no agent cannot satisfy the guard.
- Round 1: `corrected`. Round 1 says the policy "is optional; without it writes and edits are unconditional" (true) and implies a read-before-edit product behaviour; in the shipped base bundle it is off, so I found no shipped default that enforces it. Caveat: I searched `packages/bundle`, `packages/preset` and `apps` YAML only; a user profile or desktop overlay could enable it.
- For rizzy-harness: copy the design (version token + CAS inside the write lock, guard as an event slot), but enable it by default. Prompt-only "read first" is not enforcement.
- Confidence: traced end to end for the mechanism; the "no shipped bundle enables it" claim rests on a negative search (checked in review).

#### Other edit-adjacent tools: `apply_patch` exists but is off in the shipped bundles; no post-edit LSP/format step found
- What the code does: `tool-fs` registers `write`, `edit`, and optionally `apply_patch` (config `applyPatch`, default false; set `false` explicitly in base and the `standard` preset). A parser/applier exists (`fs-patch`) and `patchText` on the provider uses the same per-target lock and version guard. I found no formatter, linter or diagnostics call after an edit in the edit tool or provider; the `lsp` tool is a separate on-demand navigation tool.
- Trace:
  - Config default and registration: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/index.ts#L50-L55 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/index.ts#L96-L102
  - Base bundle turns it off: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L296-L300
  - `patchText` under the same lock and guard: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/index.ts#L280-L290
- Round 1: `confirmed` (Round 1 lists `str_replace_editor` and `edit`; I did not locate `str_replace_editor` in `tool-fs` and did not search for it elsewhere).
- For rizzy-harness: if we want post-edit diagnostics, nothing here to copy.
- Confidence: read the definition; "no post-edit diagnostics" is a negative search of tool-fs and fs-local.

### Compaction

#### Trigger: token pressure before each step, against a per-model threshold from the model's own context window
- What the code does: a listener on `agent/pre-step` calls `compactIfNeeded(agent, 'pressure')` every step. It prices the latest logged request header with the token meter, resolves the model's context window via `llm.resolveModelInfo`, and compacts when `totalTokens >= thresholdTokens`, where `threshold = floor(min(contextWindow * thresholdRatio, contextWindow - reservedCompletion - headroomTokens))`. Defaults: `thresholdRatio` 0.8, `headroomTokens` 65,536, summary `maxTokens` = headroom, `retainRatio` 0.16 of the message budget, 1 extra compaction retry, 1 overflow retry, `auto` true. Per-model overrides come from `modelPolicies`. Missing context window for the routed model throws a config error that is warned once per model and the turn continues uncompacted.
- Trace:
  - Defaults and resolution: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/config.ts#L20-L23 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/config.ts#L68-L110
  - Threshold arithmetic: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/config.ts#L191-L205
  - Pre-step listener (failure is warned, step continues): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/index.ts#L157-L175
  - Pressure path with retries: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/index.ts#L316-L371
- Edge cases and failure handling: if still above threshold after `compactionRetries + 1` attempts it throws; the pre-step listener swallows that and logs "step compaction failed ...; continuing the turn", so the request is sent anyway and may then hit a context-length error (next finding). Pricing uses route-specific token counts for retention and the shrink check but a fixed heuristic for shadow-price accounting (region.ts lines 376-381).
- Round 1: `confirmed` in outline (Round 1 asked for the numeric thresholds: 0.8 ratio, 64Ki headroom, 0.16 retain).
- For rizzy-harness: copy "threshold = min(ratio x window, window - reserved output - headroom)"; the "reserve output tokens" term is the one most harnesses miss.
- Confidence: traced end to end (checked in review).

#### Which model runs it: the conversation's own last routed model, reusing the exact prefix, unless overridden
- What the code does: the summariser replays the conversation's own system prompt, tool schemas and the shadowed messages, then appends the compaction instruction as the FINAL user message (not a separate summariser system prompt), so the provider prefix cache is reused. Target model: configured `summarizationProvider/Model`, else the last logged request route, else the agent's creation options. Output must be text only (images rejected), non-empty, and not truncated at `maxTokens` (a `max-tokens` finish is an error).
- Trace:
  - Instruction text (eight fixed Markdown sections, "do not call any tool"): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/summarizer.ts#L32-L67
  - Target selection and the call: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/summarizer.ts#L120-L181
  - Fail-closed finish handling: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/summarizer.ts#L196-L221
  - Summariser input built from the shadowed span plus the system head and logged tools: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L544-L563
- Edge cases and failure handling: a prior `<compacted-summary>` block is told to be merged, not copied. Note the summary request still passes the full tool schemas while telling the model not to call tools; a tool call in the reply is dropped by `summaryText` (text blocks only).
- Round 1: `new (not documented)`.
- For rizzy-harness: copy the cache-preserving trick (instruction as last user message over the same prefix); it makes compaction cheap on providers with prefix caching.
- Confidence: traced end to end.

#### What is kept: a token-budgeted tail, system head, and a framed summary replacing everything between
- What the code does: `selectCompactableRange` walks the surface from the newest node backwards accumulating route-priced tokens until `retainTokens` (default 16% of the message budget) is reached; that tail stays verbatim. It then moves the cut earlier until the boundary does not split a tool-call/result pair. The system head (surface node 0) is excluded from the range. Everything from the first non-system node to the cut is replaced by ONE user message framed with a preamble and `<compacted-summary>...</compacted-summary>`. Nothing else (file contents, skills, instruction files) is re-injected after compaction in this code path.
- Trace:
  - Range selection: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L117-L155
  - Framing: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/summarizer.ts#L69-L71 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/summarizer.ts#L188-L194
  - Commit as `compaction/summary` event plus a `user/message` with a `replace` surface op citing the shadowed seqs: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L471-L522
- Edge cases and failure handling: the summary is rejected unless its framed token estimate is strictly smaller than the span's route-priced size (region.ts lines 417-422). A single oversized retained unit cannot be repaired by surface compaction (doc comment, compaction/src/index.ts lines 124-135).
- Round 1: `confirmed` (the replacement-by-summary-node model); `new (not documented)` for the verbatim-tail sizing and the pair-balanced cut.
- For rizzy-harness: log the raw summarizer output and the shadowed seq list as events (this does) so the UI can show "what was compacted" and compaction is replayable.
- Confidence: traced end to end.

#### The "locked start and end sequence": a durable `compaction/start` marker is the lock, `compaction/end` always closes it, a crash orphan is cleared by `session/end-seed`
- What the code does: `compactSurfaceRegion` appends `compaction/start {compactionId, turn}`, then prepares, summarises (with a recovery waterfall), checks stability, appends `compaction/summary` and the replacing `user/message`, then `compaction/end`. Any failure before the commit body appends `compaction/end` with an `error` field, so a failed attempt leaves a start/end pair and the surface unchanged. Lock check: the latest start with no later end is "unmatched"; it only blocks if there is no later `session/end-seed` event, so a crash that left an orphan start is released when the session is next seeded. Manual compaction requires an idle agent (runs via `runMaintenance`) and flushes the session store before resolving.
- Trace:
  - Transaction: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L173-L276
  - Lock check and the `end-seed` release: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L308-L320 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L566-L600
  - Stability: automatic compaction demands the whole surface be unchanged during summarisation; manual only the selected span: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L431-L468
  - Manual entry: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/index.ts#L407-L459
  - Typed failure classes (`busy`, `cancelled`, `changed`, `summary`, `commit`, `persistence`): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction/src/index.ts#L34-L64
- Edge cases and failure handling: if the failure happens while closing (commit stage), the error is classed `commit`; if the closing append itself fails the original error is replaced by the close error. Failed attempts "remain visible in the log" (abstract class doc). If the summary body committed but `compaction/end` failed, the surface is already replaced but the lock stays open until `end-seed`: this window is real but I did not trace the downstream consequence (a second compaction would be refused as `busy`).
- Round 1: `confirmed` ("logged as a locked start and end sequence"); the crash-orphan question is answered: `session/end-seed` releases it.
- For rizzy-harness: our one-writer log can do the same. Persist a start marker before the model call, always append an end marker (with error), and define what event clears an orphan after node restart.
- Confidence: traced end to end (checked in review).

#### Recovery from a context-length error, model switch, and model-free pruning of old tool output
- What the code does: three more triggers feed the same engine. (1) On `agent/request-error` with code `CONTEXT_WINDOW_EXCEEDED`, up to `maxOverflowRetries` (default 1) per idle period: prune tool results, then compact ignoring the threshold and retention (`selectCompactableRange(..., 0)`), then return `{kind:'retry'}`. If the summary step fails after a prune already changed the surface, it still retries. (2) On `agent/request` with a different provider/model than the last header, it prices the history against the new model's window and compacts first; failure aborts the switch with `MODEL_SWITCH_COMPACTION_FAILED` and keeps the previous model. (3) Once pressure qualifies, a model-free pass `pruneSession` runs first: any tool result over 8,192 code points (config `thresholdChars`) is replaced by head 4,096 + marker `[... tool result middle pruned ...]` + tail 1,024, as a `replace` op preceded by a `compaction/prune` shadow-price event. The full output is not stored for pruned results (unless the spill plugin already saved it).
- Trace:
  - Overflow listener: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/index.ts#L213-L257
  - Model switch: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/index.ts#L181-L203
  - Overflow trigger body: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/index.ts#L292-L313
  - Pruner: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-tool-result-pruner/src/index.ts#L83-L122 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-tool-result-pruner/src/index.ts#L136-L182
  - Marker text and shipped budgets: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-tool-result-pruner/src/config.ts#L7-L7 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L431-L438
  - Image over-budget: oldest images replaced by placeholders and the request retried: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-image-offload/src/index.ts#L27-L40
- Edge cases and failure handling: overflow retries are reset when the agent goes idle or an assistant message lands. If compaction fails and the signal is not aborted with no surface progress, the original request error is preserved (`next()`).
- Round 1: `confirmed` for compaction; `new (not documented)` for the model-switch guard and the prune pass.
- For rizzy-harness: the "compact before honoring a model switch, otherwise refuse the switch" rule avoids a hard failure on the first request to a smaller-context model.
- Confidence: traced end to end.

#### Tool output size: spill to a file with a head/tail preview at 12,500 tokens
- What the code does: a `tools/post-execute` listener (prepended) counts tokens of a result; above `maxInlineTokens` (base: 12,500) it saves the full text through the spill store, keeps head and tail halves of the budget joined by `[...]`, and appends a notice with the stored locator and a retrieval hint. `read` results are exempt. Any failure keeps the content inline (warn only).
- Trace:
  - Config and exemption: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/spill/spill-policy/src/index.ts#L33-L35 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/spill/spill-policy/src/index.ts#L95-L131
  - Shipped value: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L419-L425
  - Notice text: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/spill/spill-policy/src/notice.ts#L22-L28
- Edge cases and failure handling: on any error it logs and keeps the full inline content, so a full disk or missing spill store means an unbounded result enters the context (the compaction pruner is then the next line of defence).
- Round 1: `confirmed` (Round 1 asked for the `maxInlineTokens` value: 12,500).
- For rizzy-harness: fail-open on spill error is the wrong default for a safety-bounded context; prefer truncating inline without the file.
- Confidence: read the definition (checked in review).

### Memory and instruction files

#### Instruction files: AGENTS.md and CLAUDE.md from user-global dirs and root-to-cwd, delivered as a user message, 64 KiB budget
- What the code does: candidates are `AGENTS.md`, `CLAUDE.md` and local variants `AGENTS.local.md`, `CLAUDE.local.md` (names only, no path separators). Sources in precedence order: user-global `AGENTS.md` in the DSH home, then the shared agents home; then for each directory from the project root (found by a `.git` marker) down to the cwd, all candidates present. Files with identical trimmed content collapse. Per-file read cap `maxSourceBytes` (1 MiB; larger files are silently skipped); render budget `maxBytes` (base: 65,536) per chain; truncation or omission is announced in-text ("Workspace instruction budget N bytes: ..."). The result is NOT part of the system prompt: it is a user message folded into the first request's claimed batch. Later, `read`, `write` and `edit` tool successes in a subdirectory project nested instruction files, and changed or removed files, into the inbox for the next step.
- Trace:
  - Candidate names and config schema: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/config.ts#L12-L13 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/config.ts#L22-L49
  - Discovery order: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/files.ts#L336-L363
  - Bounded read (oversize file returns undefined): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/files.ts#L387-L414
  - Budget notice: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/render.ts#L303-L312
  - Pre-step injection and touch-triggered nested projection: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/index.ts#L330-L356 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/index.ts#L358-L375
  - Shipped budget: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L307-L310
- Edge cases and failure handling: a file that vanishes between stat and read is skipped (read error swallowed); a source TODO admits there is no aggregate bound on bytes read across many files (files.ts line 394 area: `TODO(total-instruction-read-bound)`). I did not trace what a hostile repository AGENTS.md can do beyond being delivered as user-role text; there is no trust prompt in this path.
- Round 1: `new (not documented)` for the nested-on-touch behaviour and the two user-global roots.
- For rizzy-harness: copy "deliver as a user-role message and re-project when the model touches a subdirectory"; add a trust gate for repo-supplied files.
- Confidence: traced end to end for baseline; read the definition for the touch path (checked in review).

#### Model-written memory: not in the code
- What the code does: nothing. I searched `packages/**/*.ts` (excluding tests) for `memory_write`, `memories`, `auto-memory`, `MEMORY.md` and found no match; the base bundle has no memory row. The nearest mechanisms are instruction files (human-written), the `skill` tool, session search (opt-in, experimental), and `session-reference`.
- Round 1: `not in code` (Round 1 notes no memory feature either; the area is empty).
- For rizzy-harness: nothing to copy; memory is an open design choice for us.
- Confidence: inferred from a negative search (checked in review).

### Sub-agents and orchestration

#### Delegation is asynchronous: the tool returns a name immediately and the result arrives later as a runtime notice in the parent's inbox
- What the code does: `subagent` (spawn) and `subagent_fork` are two instances of one tool plugin over a provider. `execute` starts a managed activation with `delivery: 'parent'` and returns `{kind:'activation', name}` (rendered "started subagent X"). The tool is marked concurrency-safe so several can start in one model message. When the child settles, the manager builds a `subagent-settled` notice (distinct source kind from model-authored messages) and delivers it to the parent: queued if the parent is idle (waking a new turn), steered into the next step if busy. The tool description tells the model it gets the "completion notice" automatically and that `send_message` can steer or continue the child. The prompt section instructs: start independent delegations together, end the turn instead of polling, collect results before the final answer, split write work into disjoint paths.
- Trace:
  - Tool execute and activation: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/tool-subagent/src/index.ts#L273-L338
  - Prompt section text: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/tool-subagent/src/index.ts#L363-L378
  - Settlement delivery to the parent: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/manager.ts#L1109-L1127
  - Why settled notices are a distinct source kind: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/continuation-messages.ts#L22-L40
  - Base rows for both tools and providers: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L363-L403
- Edge cases and failure handling: delivery during parent teardown uses `parent.inject` (no wake); an undeliverable notice is logged as a warning and dropped. The child's final answer is its last assistant output; cleanup failure changes the `subagent/end` event but not the captured result (manager.ts lines 1085-1107).
- Round 1: `confirmed` in outline; `new (not documented)` for "result arrives as a distinct runtime message, parent need not wait".
- For rizzy-harness: copy the non-blocking start plus "result is a notice that wakes the parent". It removes parent-side polling and keeps cancellation simple.
- Confidence: traced end to end.

#### Spawn vs fork; the child inherits model route, not conversation, unless forked; fork prefix is the last completed turn
- What the code does: the `spawn` provider starts a child agent on the same Cordis context with its own session and zero parent context (`inheritsParentContext = false`); it supports preset selection, `agentOptions`, `outputSchema` and a depth cap. The `fork` provider seeds the child session with the parent's event log up to and including the last `turn/end` (the in-flight tool-call turn is excluded because it is unbalanced); it cannot pick a preset (inherits the parent's composition) and the tool omits model arguments "for KV Cache reuse". The seed is captured once at creation and becomes part of the child's own durable log. The child's provider/model/reasoning effort default to the parent's latest routed request; changing route without effort clears the parent's effort.
- Trace:
  - Spawn provider: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-spawn-in-process/src/index.ts#L38-L59
  - Fork provider and prefix: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-fork-in-process/src/index.ts#L38-L79
  - Child option inheritance: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L68-L119
  - Child session metadata (parent link, `isSeeded`, depth persisted): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L135-L154
  - Tool wording changes with `inheritsParentContext`: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/tool-subagent/src/index.ts#L116-L140
- Edge cases and failure handling: a fork before any completed turn starts fresh. Delegation depth is the max of the persisted session header and the runtime option, so a resumed parent cannot reset its depth (child-agent.ts lines 49-58).
- Round 1: `confirmed` (spawn vs fork, `agentPreset` independent composition).
- For rizzy-harness: forking by seeding the child's log with a prefix of the parent's is natural for an event-sourced design; the "end at last turn/end" rule is the necessary balance condition.
- Confidence: traced end to end (checked in review).

#### Limits: default depth 1, 8 concurrently active children per tree; no worktree or container isolation; shared working directory
- What the code does: `maxDepth` (default 1, volatile config) is checked at every start against the calling agent's depth; a child at the cap also receives a runtime-context line "Delegation depth ... remaining levels 0. Do not call subagent". `maxActiveSubagents` (default 8) is a per-root-agent `ActivationPool`: the 9th start throws `ACTIVATION_LIMIT_REACHED` with advice to wait or do the work directly. Children share the parent's workspace directory (optional `cwd` argument); the prompt warns that concurrent writes are not conflict-checked. A `create_worktree` tool exists only in the experimental package and is not part of base. Children are not isolated by container or VM.
- Trace:
  - Config defaults: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/index.ts#L195-L198
  - Pool: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/activation.ts#L19-L34 and reservation site https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/manager.ts#L707-L720
  - Depth check: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L49-L58
  - Depth hint in context: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/tool-subagent/src/index.ts#L379-L393
  - Worktree tool uses `git worktree add --no-checkout -b` (experimental): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/worktree/src/index.ts#L194-L194
- Edge cases and failure handling: external providers (codex, claude-code) use `maxDepth: 'provider-managed'` in their patches, i.e. no parent cap on what the external child does.
- Round 1: `new (not documented)` for the defaults. Answers the Round 2 question "default maxDepth": 1.
- For rizzy-harness: a depth cap of 1 and a small fixed fan-out are cheap and safe; also consider that no isolation means two writers on one tree, which we should avoid by default on nodes.
- Confidence: traced end to end (checked in review).

#### Permissions of an in-process child: sandbox override inherited, approval pinned to `never`, so approvals auto-reject
- What the code does: at the delegation boundary (synchronously, before the first await) the parent's explicit sandbox-mode override and its `auto` or `danger-full-access` preset identity are captured; the approval policy is set to `never` whenever the approval service is composed. These are appended to the CHILD's own log as `source: 'delegation'` events so its effective policy is reconstructable from its log. Only the parent's session-level override is copied: deployment defaults and one-shot grants are not. The child's runtime context says its scope cannot be widened, operations needing approval are rejected, and it should report the limitation to the parent instead of retrying. A child cannot be elevated by the parent after start.
- Trace:
  - Capture and pin: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L249-L256
  - Append onto the child log: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L267-L280
  - Child-facing statement: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L171-L175
- Edge cases and failure handling: if the approval service is not composed the pin is `undefined` (no approval event). With a parent at `danger-full-access` the child keeps that preset plus `never`, i.e. a full-access child. The `auto` preset's child inherits "auto" under `never`; how the Auto review integration treats that I did not trace (it lives under `packages/experimental/auto-review`, not read).
- Round 1: answers the Round 2 question "does a sub-agent inherit sandbox mode and approval policy": sandbox override yes (explicit override only), approval no (pinned `never`).
- For rizzy-harness: copy "child approval policy is `never`, child's policy is written into the child's log at creation". A background child cannot ask a human, so reject is the only sound default.
- Confidence: traced end to end (checked in review).

#### External harnesses as sub-agents: Codex over `codex app-server`, Claude Code over the Agent SDK, any agent over ACP, each a fixed-permission one-shot process
- What the code does: three out-of-process providers register on the same `ctx.subagents` seam, advertise no start capabilities (no `agentOptions`, preset, `outputSchema`, depth), start fresh (no parent conversation) and accept text-only prompts. They are NOT in the base bundle (base test asserts the rows are absent); each ships a `cordis.patch.yml` adding a provider row plus a delegation tool (`subagent_codex`, `subagent_claude_code`) with `maxDepth: provider-managed`.
  - **Codex**: launches the package-local `@openai/codex` wrapper as `node <wrapper> app-server --stdio` (path from the dependency manifest, independent of `PATH`) in the delegating session's cwd. Initialises, starts an ephemeral thread, sends the task, and reads the result. Permission is a provider-instance setting, independent of the parent: `never` (default: `approvalPolicy: never`, no sandbox field, so Codex's own default), `approve-for-me` (`on-request`, reviewer `auto_review`, `workspace-write`) or `dangerously-bypass-approvals-and-sandbox` (`never` + `danger-full-access`). Any server request for command or file approval is answered `cancel`/`decline`; permission grants get `{}`; user-input requests get empty answers; MCP elicitations are declined; each is recorded as a diagnostic.
  - **Claude Code**: uses `@anthropic-ai/claude-agent-sdk` with `persistSession: false`, `AskUserQuestion` disallowed, and a custom spawn routed through the shared subprocess service. Default permission mode `dontAsk`; allowed modes `dontAsk`, `acceptEdits`, `auto`, `plan`, `bypassPermissions`. `canUseTool` always denies ("cannot request human approval") except under `bypassPermissions`, where `allowDangerouslySkipPermissions` is set.
  - **ACP (generic)**: spawns a configured `command args` over stdio, `initialize` with EMPTY client capabilities (no fs, no terminal), `session/new` with `mcpServers: []`, `session/prompt` with text blocks; only `agent_message_chunk` text is kept; permission requests are auto-answered by a fixed policy `reject` (default; reply `cancelled`) or `allow` (first `allow_once`/`allow_always` option). stderr is inherited to the parent's stderr.
  - All three: environment is the parent's env with ambient credentials and `DSH_*` stripped (`scrubbedParentEnv` seam) plus explicit per-provider `env`; process lifetime is owned by the subprocess service with a grace period and escalation.
- Trace:
  - Not in base: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/tests/base.spec.ts#L42-L52 (test, used only to confirm the shipped composition) and the add-on patch https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/cordis.patch.yml#L1-L9
  - Codex argv: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/run.ts#L50-L53 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/run.ts#L136-L136
  - Codex modes and default: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/run.ts#L58-L69 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/wire.ts#L32-L44
  - Codex unattended answers: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/wire.ts#L574-L625 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/wire.ts#L59-L68
  - Claude Code modes and options: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-claude-code/src/run.ts#L43-L56 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-claude-code/src/run.ts#L316-L365
  - ACP provider config and `reject` default: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-acp/src/index.ts#L24-L64
  - ACP no capabilities, no MCP servers, output capture and permission answers: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-acp/src/run.ts#L415-L451 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-acp/src/run.ts#L483-L493
  - ACP spawn and env: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-acp/src/run.ts#L344-L356
- Edge cases and failure handling: a failed or exited child process surfaces as a stop reason `error` with a fixed-format safe diagnostic ("Product subagent failure (product: Codex; stage: ...)"), never a rejected result promise; startup failure reaps the private process before rejecting. Results are the child's final assistant text only; intermediate tool traffic stays in the child. Because the permission mode is fixed in provider config, an external child's authority is unrelated to the parent's current preset: a parent in read-only can delegate to a Codex child configured `dangerously-bypass-approvals-and-sandbox` (the sandbox/approval of the parent does not propagate to the external child). For the ACP child the harness cannot enforce anything beyond cwd and the permission reply; the child runs its own tools unsandboxed by dsh.
- Round 1: `confirmed` that external harnesses can be sub-agents (Round 1 lists ACP/SDK surfaces); `new (not documented)` for the per-provider permission model and unattended-answer behaviour.
- For rizzy-harness: copy the unattended-answer table (decline every approval, empty user input, decline elicitation) and fixed per-provider permission config. Avoid the parent-independence: derive the external child's strictest mode from the parent's node policy so a delegation cannot widen it.
- Confidence: traced end to end for Codex, Claude Code and ACP start and permission paths; the Claude Code result collection was not read in full (checked in review).

### Workflow scripts

#### `workflow` tool: a model-written JavaScript body run in a `vm` context inside the sandboxed PTC process, with hooks `agent`, `parallel`, `pipeline`, `phase`, `log`
- What the code does: the model supplies `{script, meta, args, run_in_background}`. The host parses the body (`new vm.Script` wrapped in an async IIFE; a leading `export const meta` is rejected since `meta` rides as data) before anything runs. The guest side (executed through the shared PTC runtime under the calling session's file policy) builds a fresh `vm` context whose globals are only the five hooks plus `args`; `runInContext` has a synchronous timeout (`syncTimeoutMs`, default 5000) but the overall run has no deadline (`timeoutMs: null`) and is cancellable. `agent(prompt, {schema, label, phase, provider, model})` runs a child to completion; with a schema the result is the validated object, else final text; a failed child returns `null`. The tool description is the script-authoring contract. A rule from the source: misuse (bad args, unknown options, tripped caps) ends the whole script rather than returning `null`.
- Trace:
  - Tool description with the hooks: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/tool-workflow/src/index.ts#L158-L166
  - Host parse and engine config: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/index.ts#L52-L66 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/index.ts#L104-L111
  - VM context and hooks: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/runtime.ts#L83-L109
  - Guest entry: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/guest.ts#L66-L66
  - Host runs the guest through the PTC runtime with the parent's sandbox policy and refuses to run if the runtime cannot enforce it: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/host.ts#L283-L300
- Edge cases and failure handling: result must be plain JSON (`RESULT_UNSERIALIZABLE` otherwise); a dropped hook promise cannot crash the process (`contain`). Infrastructure faults in a child (provider failure) are FATAL for the script, a child that merely fails resolves `null`.
- Round 1: `confirmed` (model-written scripts under a `maxTotalAgents` cap).
- For rizzy-harness: `vm` is not a security boundary; the safety here comes from running the guest inside the sandboxed PTC process with no fs/network/timers. If we copy the feature, run it on the node inside our sandbox.
- Confidence: traced end to end (checked in review).

#### Hard cap: 1000 children per run (engine ceiling), concurrency `min(16, cores - 2)`, the cap is not a script-visible value and not a tool argument
- What the code does: `maxTotalAgents` default 1000 per engine; a request may lower it (never exceed the ceiling), but the ordinary workflow tool does not pass it (it builds the request from `{script, meta, args}`; the doc says specialised consumers may lower it and "the script cannot observe or replace either policy"). `maxConcurrentAgents` 0 means `min(16, max(1, cores - 2))`; slots are acquired FIFO. When `started >= maxTotalAgents` the `agent()` call throws `AGENT_CAP` with a message naming the cap, which ends the whole script. `maxItemsPerCall` 4096 bounds `parallel`/`pipeline` inputs.
- Trace:
  - Config defaults: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/index.ts#L104-L111
  - Resolution and concurrency: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/index.ts#L83-L96 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/index.ts#L138-L148
  - Enforcement site: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/runtime.ts#L143-L174
  - The host starts each child as `delivery: 'caller'` through the same `startActivation`: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/host.ts#L200-L220
- Edge cases and failure handling: the cap is enforced in the GUEST (the same process as the model's script); the host counts starts (`this.started`) but I found no host-side cap check in `startChild`. A script that escaped the `vm` realm could bypass the cap; the remaining host-side limit is the subagent `ActivationPool`. Interaction I infer but did not run: workflow children go through the same pool as other children (`maxActiveSubagents` default 8), while workflow concurrency defaults to up to 16, so the 9th simultaneous child on a 18+ core machine would hit `ACTIVATION_LIMIT_REACHED`, surfacing as an `AGENT_START` error that ends the script. No workflow doc mentions this (grep of workflow docs for the limit found nothing).
- Round 1: `confirmed` ("cap the script cannot see": the value is not passed to the script, though the cap error text reveals it); `new (not documented)`: default 1000, concurrency formula.
- For rizzy-harness: a hard cap held outside the script is right; enforce it on the node side (not in guest code) and size it against the sub-agent pool.
- Confidence: traced end to end for the cap and defaults; the pool interaction is inferred (checked in review).

### Agent teams

#### A "team" is a shared task board plus ordinary sub-agents; mode is a per-root-session flag, off by default
- What the code does: `agent-team` stores tasks in the root session's log (event journal and projection) and exposes `team_task_create/list/get/update`. Task updates are compare-and-set on a `revision` number (`claim`, `release`, `edit`, `set_dependencies`, `complete`, `reopen`, `reassign`, `delete`), with dependency graph validation (missing, duplicate, cycle), `blockedBy`, advisory `write_scopes`, max 256 tasks, 16,384-char descriptions. Teammates ARE sub-agents (the config rejects the old `maxMembers`/`maxMessageBytes` keys with "teammates are subagents, so subagent limits and inbox delivery apply"); messaging is the sub-agent `send_message`. Mode is toggled per session by `team/mode` event from the root only; `defaultEnabled` (default false) auto-enables fresh non-sub-agent roots. Tools and the `team:policy` prompt section appear only when enabled.
- Trace:
  - Service, config, mode switch: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/team/agent-team/src/index.ts#L46-L112
  - Tool CAS and enabled gating: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/team/tool-agent-team/src/index.ts#L193-L231
  - Task creation limits: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/team/agent-team/src/task-board.ts#L55-L90
  - Base rows (mounted, not disabled): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L366-L372
- Edge cases and failure handling: an independently resumed child has no active root and sees Team mode as off (tool-agent-team lines 100-110). `write_scopes` are advisory only: nothing enforces disjoint writes.
- Round 1: `corrected`. Round 1 says the team tools are "disabled in the shipped base bundle"; the base bundle mounts both rows enabled. What is off by default is Team mode per session (`defaultEnabled: false`, so tools are filtered out via `when`). I did not check whether the web profile surfaces a toggle (a `ui-agent-team` row exists in `bundle/web-app/cordis.patch.yml` line 393).
- For rizzy-harness: a CAS-revision task board on top of existing sub-agents is a small, auditable design; advisory write scopes are the weak part, a node could enforce them through its path policy.
- Confidence: read the definition (checked in review).

### Model configuration, retry and fallback

#### Model selection: per-agent route in the log; default `deepseek-official/deepseek-flash`; no fallback chain
- What the code does: the default selection comes from the `agent-default-model` row (`provider: deepseek-official`, `model: deepseek-flash`) and a saved Settings selection. The loop reads `agent.model.current()`; a step with no provider/model throws. A listener on `agent/request` may propose a different route but only before step preparation fixes it (later change throws "use agent.model.select()"). The route actually used is recorded in the logged request header, and compaction, sub-agents, titles and spill read that header. Sub-agent model selection is optional (`modelSelection: true`, restricted to the parent's route unless the session policy allows more) and a discovery tool `list_subagent_models` exists in the web preset. I found no fallback-model chain: a failed request is retried on the SAME route or the turn ends with an error. Session titles use the logged request route unless `provider`+`model` are configured, answering the Round 1 title question.
- Trace:
  - Default model row: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L88-L94
  - Loop reads and fixes the selection: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L283-L317 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L612-L640
  - Title route from the logged header: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-title-llm/src/index.ts#L183-L195
  - Sub-agent route inheritance: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L68-L119
- Edge cases and failure handling: `llm-pi-ai` registers extra multi-provider routes dormant until a settings section supplies profiles (base bundle lines 128-136).
- Round 1: `not in code` for fallback chains (Round 1 did not claim one); title question answered.
- For rizzy-harness: no fallback chain to copy.
- Confidence: read the definition; "no fallback chain" is a negative search (`fallback` appears only in unrelated image/catalog code in `packages/llm`) (checked in review).

#### Retry: per-provider policy, exponential backoff with jitter, `Retry-After` honoured, retries logged as events
- What the code does: the adapter exposes a resolved retry policy per provider; the optional `llm-retry` plugin executes it as an `agent/request-error` listener. Default policy (`normal`): 5 retries, initial delay 500 ms, max delay 10,000 ms, jitter ratio 0.1, retryable codes `EMPTY_RESPONSE`, `RATE_LIMIT`, `SERVER`, `TIMEOUT`, `TRANSPORT`. Delay = min(initial x 2^(n-1), max) times jitter in [0.9, 1.1], capped at max. If the provider supplies a `retryAfterMs` larger than `maxDelayMs`, `normal` mode gives up immediately (fails the step); a smaller one is used as the delay. An `always` mode retries every failure forever (recovery by downstream listeners first). Each retry appends `llm/retry` then `llm/retry-started`; the attempt count is a projection reset at `step/start` and `turn/end`, so it is per step. Cancellation during the delay aborts.
- Trace:
  - Defaults: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/llm/llm/src/retry-policy.ts#L14-L24 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/llm/llm/src/retry-policy.ts#L148-L160
  - Delay and decision: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/llm/llm-retry/src/index.ts#L59-L64 and https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/llm/llm-retry/src/index.ts#L187-L257
  - Loop side: a failed stream is settled as an `assistant/attempt` event, then the `agent/request-error` waterfall decides retry: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L541-L564
  - Policy is a volatile provider config: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/llm/llm-deepseek/src/config.ts#L110-L110
- Edge cases and failure handling: mid-stream failures that already produced content are settled durably (partial assistant output with `interrupted: true` only on abort; otherwise the attempt is recorded without a message); abort during streaming keeps interrupted blocks (agent.ts lines 499-540). A retry after a partial stream restarts the request.
- Round 1: `confirmed` in outline; `new (not documented)` for the numbers and the `Retry-After` rule.
- For rizzy-harness: log each retry as an event with the delay (this does); treat a long `Retry-After` as a failure rather than blocking a worker.
- Confidence: traced end to end (checked in review).

### Stuck-loop detection, output truncation, plan-mode enforcement

#### Repeat-call reminder: three escalating nudges at 3, 5, 8 identical consecutive calls; never blocks
- What the code does: a `tools/post-execute` listener keys each call by tool name plus key-sorted canonical JSON of arguments; consecutive identical keys increment a per-agent chain. At counts in `thresholds` (default 3, 5, 8) it attaches a user-role notice to the result (`additionalContexts`): a gentle sentence at the first threshold, a detailed one (tool, count, 500-char argument preview, "do not call this tool with these exact arguments again") at later ones. It observes and enriches only; it never blocks or kills the call. A genuine user message in a step resets the chain. A different tool or arguments also resets it. It is in the base bundle.
- Trace:
  - Config: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/guard/repeat-tool-reminder/src/index.ts#L50-L56
  - Reminder texts: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/guard/repeat-tool-reminder/src/index.ts#L70-L86
  - Observe and reset: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/guard/repeat-tool-reminder/src/index.ts#L194-L240
  - Base row: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L455-L460
- Edge cases and failure handling: only exact-argument repeats on consecutive calls are detected; alternating two calls (A, B, A, B) is not. There is no step or turn cap in the loop that I found (turn loop runs until the model stops calling tools or the signal aborts: agent.ts lines 344-401); the only budget-like controls are the compaction thresholds, the tool timeout policy, the sub-agent and workflow caps.
- Round 1: `new (not documented)`. Also: no maximum steps per turn found.
- For rizzy-harness: copy the reminder (cheap), add an actual step budget and an alternate-pattern detector.
- Confidence: traced end to end for the reminder; "no step cap" is a negative reading of the loop (checked in review).

#### Plan mode is advisory prompt guidance; sandbox and approvals do not change with it
- What the code does: plan mode is logged state (`plan/mode` events) that turns on the `plan:policy` prompt section and discloses the `exit_plan_mode` tool. The module header states that sandbox mode and approval policy "enforce restrictions independently and do not read or write plan state". The prompt text itself says the tool catalog stays unchanged across modes for request-cache stability and tells the model not to use mutation tools. `exit_plan_mode` requires a markdown plan starting with `# `, asks the user through the user-questions channel (Approve / Keep planning), and fails if no review channel exists (the prompt then says to stay in plan mode). A `/plan` command toggles state; changes apply at the next accepted pre-step.
- Trace:
  - Contract in the module doc: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/plan/plan-mode/src/index.ts#L1-L22
  - Exit tool and review: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/plan/plan-mode/src/index.ts#L280-L330
  - Prompt text (base bundle; the web presets carry a near-identical copy at https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/presets/standard.patch.yml#L54-L68): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L339-L350
- Edge cases and failure handling: because plan mode does not set the sandbox to read-only, a model (or prompt-injected content) that ignores the prompt can still write files under the session's normal permission; the only barrier is the ordinary approval/sandbox.
- Round 1: `new (not documented)` (Round 1 lists `exit_plan_mode` but not that it is advisory).
- For rizzy-harness: do not copy the advisory model. Tie plan mode to a read-only sandbox/approval overlay on the node, so the mode cannot be bypassed by the model.
- Confidence: read the definition (corrected in review: added the web-preset copy of the plan text; advisory-only conclusion confirmed by search of plan-mode for pre-execute, guard, sandbox and deny).

### Loop mechanics that bear on this cluster

#### One step: pre-step waterfall, request attempt loop, tool batch, with structured crash recovery of tool results
- What the code does: `turn()` appends `turn/start`, then repeatedly calls `preStep` (claim inbox messages, resolve the tool set, assemble the prompt, run the `agent/pre-step` waterfall which may rewrite messages or reject), appends `step/start`, runs `step()`, and appends `step/end`. A `reject` decision ends the turn as `blocked`. `step()` loops over request attempts (so retry and overflow-recovery re-enter without a new step): it re-prepares the request, appends the prompt/user events, streams, settles the assistant message durably, then executes the tool calls (max parallel default 10, concurrency per tool via `isConcurrencySafe`). If a step throws after tool calls were logged without results, `ToolCallRecovery` appends the missing `tool/result` events so the log stays balanced. Turn end reasons: completed, aborted (with the cancel cause), blocked, error, max-tokens (sticky).
- Trace:
  - Turn driver: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L327-L433
  - Pre-step: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L283-L317
  - Step and tool dispatch: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L584-L610
  - Parallelism default: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/constants.ts#L5-L6
  - Durability before each request and top-level dispatch: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L25-L36
- Edge cases and failure handling: a request-level failure is first recorded as an `assistant/attempt` event; the `agent/request-error` waterfall then decides `retry` or the failure becomes an `LlmError` and the turn ends with `error`. Cancellation records interrupted assistant content if any was produced.
- Round 1: `confirmed` (steps, turn end reasons) and answers "pre-step interception ... at a step boundary".
- For rizzy-harness: the "recovery appends missing tool results" rule is important for a one-writer log: never leave a dangling tool call in the log.
- Confidence: traced end to end.

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Everything is a Cordis plugin; loop, tools, providers composed as plugin rows | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L15-L30 |
| Compaction is logged as a locked start/end sequence | confirmed (plus: an orphan start is released by `session/end-seed`; failure appends an `end` with `error`) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L173-L276 |
| Filesystem freshness policy is optional; edit unconditional without it | confirmed, but the policy row is `disabled: true` in the shipped base | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L292-L294 |
| `edit` replaces literal text with optional `replace_all` | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/fsio.ts#L862-L882 |
| Workflow scripts run under a `maxTotalAgents` cap the script cannot see | confirmed (default 1000; enforced in guest code, error text reveals it) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/workflow/workflow-ptc/src/runtime.ts#L143-L174 |
| Agent-team tools are experimental, "disabled in the shipped base bundle" | corrected: rows are mounted in base; Team mode per session is off by default | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L366-L372 |
| Tool results over a threshold spill to a file (`maxInlineTokens`) | confirmed; value 12,500; fails open | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/spill/spill-policy/src/index.ts#L95-L131 |
| `exit_plan_mode` / plan mode (tool exists) | confirmed; advisory, no sandbox change | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/plan/plan-mode/src/index.ts#L1-L22 |
| Pre-step hook can intercept or alter a step; authoritative | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L296-L312 |
| Preset can select an independent composition for a child | confirmed (spawn only; fork cannot) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-fork-in-process/src/index.ts#L60-L66 |
| Sub-agent inherits sandbox and approval (docs silent) | answered: explicit sandbox override inherited; approval pinned to `never` | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L249-L256 |
| Default `maxDepth` | answered: 1 (and 8 active children) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/index.ts#L195-L198 |
| `maxGoalRounds`, reminder idempotency | not examined (goal and schedule are outside the topics read) | n/a |

## 5. In the code but not in the documentation

- Dynamic facts as a per-step runtime-context user message, separate from the cache-stable system prompt (section 3).
- Compaction instruction delivered as the final user message over the conversation's exact prefix for prefix-cache reuse; `session/end-seed` as the orphan-lock release; compaction before a model switch with a hard refusal on failure.
- Read-before-edit guard shipped disabled while the prompt still asks for it.
- Retry numbers and the "long `Retry-After` fails fast" rule; per-step retry counter.
- Child approval pinned to `never` and delegation policy events written into the child's log; external children run with provider-fixed permission modes unrelated to the parent.
- Workflow concurrency formula and a probable (inferred) clash with the 8-child pool.
- Spill fails open.
- Instruction files nested on touch, with both AGENTS.md and CLAUDE.md read and content-deduplicated.

## 6. Could not find or could not settle

- Whether workflow concurrency above 8 actually trips `ACTIVATION_LIMIT_REACHED`: needs a run (not allowed) or a test reading `workflow-ptc` plus `subagent` together; I did not read the tests for it.
- Whether any shipped profile (desktop, ACP, SDK, headless) enables `fs-observation-policy`: I searched `packages/bundle`, `packages/preset`, `apps` YAML only.
- Where `str_replace_editor` (Round 1) lives; not in `tool-fs` and not searched elsewhere.
- Auto review (`packages/experimental/auto-review`): prompt, model, failure behaviour not read (safety cluster).
- `maxGoalRounds` and reminder idempotency (goal, schedule): not read.
- The Claude Code provider's result collection and `claudeSpawnSpec` details were only partly read.
- What stops a hostile repository AGENTS.md: the instruction loader has no trust gate in the code I read; a trust prompt could exist elsewhere (the sessions/safety clusters).
- Memory: none found; a negative search cannot prove absence outside `packages/`.

## 7. Citation check

`research/round2/deepseek-harness.agents.md: 157 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Checked 21 of 27 findings plus the shipped web-app presets. Verdicts: 18 supported, 1 absence confirmed, 2 partly (dynamic facts in runtime-context; the plan and team prompt-section finding, whose plan-text citation was incomplete), 0 wrong, 0 not supported. The plan-mode advisory finding also got its citation widened because the default web profile takes the text from the preset files, not only the base bundle. Absence claims: no model-written memory, no step cap, no fallback chain (search of `packages/` for memory terms and step-cap names found nothing; absence confirmed). Not checked: apply_patch, summariser model choice, what-is-kept details, recovery and model-switch details, async delegation notices, one-step loop mechanics.

| Finding | Verdict | Reason |
|---|---|---|
| Prompt sections and order table | supported | `SECTION_ORDERS` matches the listed values |
| Dynamic facts as user message | partly | refreshed per step but appended only when text changes |
| Plan and team prompt sections | partly (citation) | web profile uses preset-level plan text |
| `edit` literal replace | supported | `applyLiteralEdit` and `editText` under lock |
| Read-before-edit disabled | supported | `disabled: true` in base; no other bundle mentions it; policy code matches |
| Compaction trigger and numbers | supported | 0.8, 65,536, 0.16, retries 1/1, formula |
| Compaction start/end markers | supported | start before summarise, end appended in every path, flush after close |
| Spill at 12,500 | supported | `read` exempt, failure keeps inline |
| Instruction files | supported | 64 KiB per chain, 1 MiB per file, TODO on aggregate bound |
| No model-written memory | absence confirmed | no memory tool names in packages |
| Spawn vs fork | supported | fork prefix to last `turn/end` |
| Depth 1, 8 children | supported | config defaults and pool |
| Child approval pinned `never` | supported | `captureDelegatedPolicyOverrides` |
| External harness permissions | supported | Codex param table, Claude Code `canUseTool` denies |
| Workflow tool | supported | guest runs under PTC; host refuses if policy not enforceable |
| Workflow cap 1000 | supported | enforced in guest `agent()`, host `startChild` has no cap |
| Team board | supported | base rows mounted, `defaultEnabled` false |
| Model selection | supported | default row; no fallback chain found |
| Retry | supported | numbers and `Retry-After` rule match |
| Repeat reminder | supported | thresholds 3/5/8, observe-and-enrich |
| Plan mode advisory | supported | no pre-execute listener or guard in plan-mode |

Overall: the cited code matched in 19 of 21 checks (18 supported plus the confirmed absence), and the two corrections are about presentation and citation, not substance. The unchecked remainder is mostly lower-stakes mechanics; trust it as moderately high, but re-check any numbers before copying them.

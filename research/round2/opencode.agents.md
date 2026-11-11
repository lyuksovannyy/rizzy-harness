# OpenCode: Round 2 source investigation, cluster "agents"

## 1. Repository

- `anomalyco/opencode`, branch `dev`, pinned SHA `05d347d75054bf27778750ad5f2cd30336e81d8f`, commit date 2026-11-10T20:28:23Z (from `research/round2/00-repos.tsv`).
- Read: `packages/core/src` (the engine: `agent.ts`, `plugin/{agent,plan,optimize,identity,goal,warming,tool-input-repair}.ts`, `session/{compaction,context,model-request,title,generate,subagent-*,instructions,instruction-state,history,recent-text}.ts`, `session/runner/*`, `tool/plugin/{edit,patch,read,subagent}.ts`, `tool-output.ts`, `model*.ts`, `instruction*.ts`, `config/plugin/{agent,instruction,compaction}.ts`), `packages/util/src/patch.ts`, `packages/ai/src/{provider-error.ts,schema/errors.ts}`, `packages/schema/src/agent.ts`. Docs were only used to find my way.
- Not read in depth (other clusters): permission evaluation internals, the shell scanner, sandboxing, MCP, Code Mode, snapshots, server and clients.

## 2. Architecture as it bears on this cluster

- **Which code is current.** The repository is a TypeScript monorepo (Bun, Effect). There is no `packages/opencode` V1 engine in this clone. The engine is `packages/core` (V2). V1 survives only as migration code: `packages/core/src/v1/config/*` (old config shapes translated by `ConfigMigrateV1`) and `database/v1-migration.bun.ts`. A V1-era permission key `doom_loop` exists only in the V1 config validator.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/v1/config/permission.ts#L27-L38 : the only mention of `doom_loop` in the tree is a V1 config validator, not a runtime check.
- **Everything is a plugin over a few services.** Built-in behaviour (agents, plan mode, per-model prompts, identity, tools, goal, warming) is registered as "internal plugins" in a fixed order; config-derived plugins (custom agents, instructions, compaction settings) run after them.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/internal.ts#L244-L288 : the `pre` list (AgentPlugin, PlanPlugin, PatchTool, OptimizePlugin.Plugins, IdentityPlugin, EditTool, ... SubagentTool, WarmingPlugin) and the `post` list (ConfigInstructionPlugin, ConfigAgentPlugin, ConfigCompactionPlugin, ConfigToolOutputPlugin, ...).
- **Agent loop.** `SessionRunner.drain` loops `advanceToStep` then `runStep`; each `runStep` iteration: load context, maybe compact, build the request, run one provider stream plus its tool calls, decide retry/continue.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L214-L222 : the outer drain loop.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L235-L384 : `runStep`, where compaction, request building, retry and output-limit recovery are decided.
- **Request kinds.** `SessionModelRequest` builds four kinds of request (`primary`, `compaction`, `generate`, `title`); each runs its own plugin hook, which is how per-model prompts and tool curation are applied to all of them.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/model-request.ts#L248-L256 : the four-entry interface.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/model-request.ts#L440-L451 : each kind wired to its hook.
- **Durable state.** Messages are a projection of a durable event log (`SessionEvent.*` projected by `projector.ts` into SQLite tables); instruction baselines are stored as hashed blobs per session.

## 3. Findings

### System prompts and instructions

#### The system prompt is two system parts: a base prompt plus a keyed "instructions" block
- What the code does: `baseTranscript` builds `system = [SystemPart(agent.system or default prompt), SystemPart(Instructions.join(initial))]`. The default prompt is `runner/prompt/system.txt` with a `${OPENCODE_TOOL_GUIDANCE}` slot filled by `SessionSystemPrompt.render` from the names of the tools actually advertised (shell, write, edit paragraphs). If the agent defines `system`, that text replaces the base prompt entirely, and the per-model plugins below skip it.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/model-request.ts#L105-L128 : `baseTranscript` builds the two system parts.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/system-prompt.ts#L5-L28 : tool-conditional guidance lines (prefer dedicated tools; do not chain shell with echo separators; write vs edit rules; edit uniqueness rule).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/prompt/system.txt#L1-L15 : the whole default prompt (15 lines: harness notes, communication, "treat unfamiliar files as potential user work").
- Edge cases and failure handling: empty parts are filtered out (`.filter(part.text.length > 0)`). Compaction, generate and title requests use the same builder (title uses only `agent.system`), so the cache prefix is shared (model-request.ts L105-L128; `title.ts` L72-L78).
- Round 1: `new (not documented)`. Round 1 lists no system prompt text at all.
- For rizzy-harness: the default prompt is tiny (15 lines); the behaviour lives in tool descriptions and per-model overlays. Worth noting that a minimal base prompt plus tool-conditional guidance is a legitimate design.
- Confidence: traced end to end.

#### Per-model prompt overlays are plugins keyed on substring of the model id
- What the code does: six prompt texts live in `plugin/system-prompt/*.txt`. `OptimizePlugin.make` registers a hook on `context`, `compaction` and `generate` requests; it picks a template from the resolved model id, skips agents that define `system`, renders the tool-guidance slot, and either replaces `system[0]` ("override") or appends to it ("append"). Rules: id contains `gpt` -> `gpt.txt` (or `gpt-astra.txt` if id also contains `astra`); `claude` -> append a 2-line "match surrounding comment density" snippet (`anthropic.txt`, 300 bytes); `kimi` -> `kimi.txt`; `trinity` -> `trinity.txt`; `muse` -> `meta.txt` with `{{MODEL_NAME}}` substituted. Every other model (Gemini, DeepSeek, Qwen, local) gets the generic `system.txt`. The `gpt.txt` variant adds "commentary channel" guidance, a "do not spawn subagents unless asked" rule and destructive-action rules.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/optimize.ts#L16-L30 : OpenAI and Anthropic selectors (`includes("gpt")`, `includes("claude")`, append mode).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/optimize.ts#L51-L62 : Kimi, Trinity, Muse selectors and the `Plugins` list.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/optimize.ts#L78-L104 : `make`: hook on three request kinds, skip when the agent has its own `system`, override vs append, errors swallowed (`Effect.catch(() => Effect.void)`).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/system-prompt/gpt.txt#L29-L39 : delegation, destructive-action and autonomy paragraphs in the GPT variant.
- Edge cases and failure handling: matching is plain substring on the catalog `id`, so any host serving a model whose id contains "gpt" (for example an OSS model through a gateway) gets the GPT prompt; the code has no provider check. The tool-curation twins (`OpenAIToolsPlugin`, `AnthropicToolsPlugin`, which delete `grep` and `glob`) are defined but intentionally not in `Plugins` (comment at optimize.ts L32-L33).
- Round 1: `new (not documented)`.
- For rizzy-harness: a model-family overlay mechanism is cheap, but id-substring matching misfires on gateways; match on a model "family" field plus provider.
- Confidence: traced end to end (checked in review).

#### A "Your Model" identity block and verbosity default are injected by hooks
- What the code does: `IdentityPlugin` inserts a third system part `# Your Model - Name / Provider ID / Model ID` at index 1 for context, compaction and generate requests. `VerbosityPlugin` sets `textVerbosity = "low"` for OpenAI GPT-5.x/6 models on direct OpenAI/Azure/gateway packages when neither the model nor variant configures it.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/identity.ts#L10-L31 : identity text and the `splice(1, 0, ...)`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/verbosity.ts#L22-L58 : verbosity default and the model-id test.
- Edge cases and failure handling: both hooks swallow errors, so a catalog failure drops the block silently (identity.ts L26).
- Round 1: `new (not documented)`.
- For rizzy-harness: putting the model identity right after the base prompt keeps it constant across a session (cache-safe) but changes on a model switch.
- Confidence: read the definition.

#### Instructions are keyed sources with a frozen baseline and chronological diffs, not text pasted into the prompt
- What the code does: every contextual item (Code Mode catalog, MCP guidance, references, skill list, AGENTS.md files, date and environment, API-attached entries) is an `Instructions.Source` with a key, a JSON-codec value, and renderers for `initial`, `changed`, `removed`. Per session, the runner reads all sources, hashes each canonical-JSON value, and publishes a durable `session.instructions.updated` event holding only the hash delta and the already-rendered update text (frozen so it never needs re-rendering). The first observation is the baseline and is rendered into `system[1]` at every request; later changes become a `system` message appended to history at the moment of change ("The instructions from X changed. Here's the diff"). Source order is "most to least shared across sessions": Code Mode, MCP, references, skills, AGENTS.md, date and environment, API entries.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L159-L175 : the ordered `Instructions.combine([...])` with the cache-prefix comment.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/instructions/index.ts#L143-L160 : `diff` hashes values and returns a delta plus blobs; an `Unavailable` source keeps its stored value, and blocks only the first observation (`InitializationBlocked`).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L49-L89 : `observe`, `commit` (text frozen into the event, blobs inserted in the commit hook) and `renderUpdateText`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/message-updater.ts#L154-L166 : the update event becomes a `system` message in history.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L225-L232 : `prepareContext` runs `InstructionState.prepare` before every step.
- Edge cases and failure handling: a source that errors becomes `unavailable` (the stored value stands); a never-initialised session with an unavailable source refuses to start the run (`Instructions.InitializationBlocked` is in `RunError`, runner/index.ts L12-L19). A revert that removes an update clears the fold (`InstructionState.rewind`, instruction-state.ts L142-L158).
- Round 1: `confirmed` ("keyed session instructions that do not break the prompt cache"), `new (not documented)` for the mechanism: hashed blobs, frozen update text, epoch baseline.
- For rizzy-harness: strong design for cache stability and for UI display ("instructions updated: ..." notices), at the cost of three SQL tables and a delta protocol. A simpler form: store the rendered baseline and append diffs as events.
- Confidence: traced end to end.

#### Instruction baselines are re-based at compaction ("epoch")
- What the code does: when a compaction ends, the projector calls `InstructionState.advanceEpoch`, which copies `current_values` into `initial_values`. The next request renders a new `system[1]` baseline containing the latest values, so updates made before the compaction vanish into the baseline and the old update notices (which sat before the checkpoint) are gone from history.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/projector.ts#L1119-L1122 : `Compaction.Ended` projection calls `advanceEpoch`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L126-L140 : `advanceEpoch` sets `initial_values = current_values`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L159-L170 : `initial` renders the epoch baseline "shown at the start of every model request".
- Edge cases and failure handling: the summary prompt tells the summarizer not to restate AGENTS.md or environment because "the next agent receives current versions of all of these separately" (compaction.ts L216), which is only true because of this re-basing.
- Round 1: `new (not documented)`.
- For rizzy-harness: copy the principle "compaction summarises conversation only; instructions are re-supplied from live sources".
- Confidence: traced end to end.

#### AGENTS.md discovery: global file, ancestor walk to the project root, plus `instructions` globs and URLs
- What the code does: candidates are `<global config>/AGENTS.md`, then every `AGENTS.md` found walking up from the location to the project root (or to `$HOME` when the location is under home), then entries from the config `instructions` array (absolute paths, `~/`, globs searched upward, or http(s) URLs fetched once at config load with a 5 s timeout, last good copy kept on failure). Order of rendering: global, project (nearest-to-farthest as returned by `fs.up`), configured. Only the filename `AGENTS.md` is looked up (no CLAUDE.md here). Each file is rendered as `Instructions from: <path>\n<content>`. Files are watched; changes are debounced 100 ms and arrive as diffs. No size limit appears in this path.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/instruction.ts#L36-L58 : stop directory, global file, candidate list.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/instruction.ts#L84-L94 : project walk; any unreadable file marks the whole project source `unavailable`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/instruction.ts#L151-L179 : URL fetch (5 s, keeps last good, never fails the set).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/instruction-discovery.ts#L123-L150 : render and diff rendering (diff vs full replacement, whichever is shorter).
- Edge cases and failure handling: a file missing mid-save is "transient" and makes the source `unavailable` (the stored value stands); an access-denied file is simply skipped (config/plugin/instruction.ts L64-L76). A hostile remote instruction URL is fetched with the user's HTTP client and rendered verbatim into the system block; the only guard is that it must be listed in config.
- Round 1: `confirmed` (global, upward discovery, `instructions` array with 5 s URL fetch); `corrected` ("V2 reads only AGENTS.md" is true for this loader, but see the `.claude`/`.agents` directories below for other config artefacts).
- For rizzy-harness: URL-sourced instruction text is persistent prompt injection surface; require an explicit trust step for remote sources.
- Confidence: traced end to end.

#### Nested AGENTS.md are injected after a `read`, with a ledger kept in the history itself
- What the code does: after a successful `read` of a file or directory inside the project (not external), the tool walks upward from the file's dirname to (excluding) the location root, collects `AGENTS.md` files not already covered by the root-level walk, and calls `SessionInstructions.load`. That publishes a durable `synthetic` message "Instructions from: <path>..." whose metadata is `{instruction:{paths}}`. The "already injected" check scans the synthetic messages in the model-visible history (latest completed compaction onwards), so after compaction or revert drops those messages the files are re-discovered and re-injected on the next read.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/read.ts#L129-L154 : discovery after a read; failures never fail the read.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instructions.ts#L40-L81 : in-flight claim set, `previouslyInjected`, publish of the synthetic message.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instructions.ts#L100-L113 : the ledger is the metadata of synthetic messages in history (`SessionHistory.synthetics`).
- Edge cases and failure handling: nested files are read once and not watched, so edits mid-session are not reflected (no source key, unlike root-level files). Read order, not path nearness, decides injection order.
- Round 1: `confirmed` ("not re-injected while in history; nested edits not detected mid-session"); the re-injection-after-compaction mechanism is `new (not documented)`.
- For rizzy-harness: a "ledger = messages that carry metadata" design avoids a separate table and self-heals on compaction/revert.
- Confidence: traced end to end.

#### Date, environment, skills and MCP are instruction sources too
- What the code does: `core/date` renders "Today's date: ..." (and "is now: ..." on change); `core/environment` renders cwd, project root, git yes/no, platform and a preferred tmp dir. The skill list is a keyed `<available_skills>` block (id, name, description; sorted; skills the merged permission rules deny are filtered out; `autoinvoke === false` skills omitted), with list-style diffs ("New skills are available...").
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/instructions/builtins.ts#L22-L53 : date and environment sources.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/skill/instructions.ts#L21-L46 : skill list render and update texts.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/skill/instructions.ts#L62-L80 : permission filtering and sorting. No budget or truncation on the skill list in this file.
- Edge cases and failure handling: the date changes daily, and its change becomes a mid-conversation system message, not a system-prompt edit, so the cached prefix survives midnight.
- Round 1: `new (not documented)`.
- For rizzy-harness: the date-as-delta trick is cheap to copy.
- Confidence: read the definition.

### Edit tools

#### `edit` matches in three tiers: exact, then Unicode-punctuation-normalised, then line-wise ignoring trailing whitespace
- What the code does: `matchView` works on an LF view of the file (CRLF stripped, offsets mapped back). Tier 1: exact `indexOf` occurrences of `oldString`. If none, tier 2: occurrences found after `normalizeForMatch` (curly quotes to straight, en/em dashes and minus to hyphen, NBSP and exotic spaces to space; a one-to-one mapping so offsets stay valid). If still none, tier 3: whole-line match where each line is compared after `trimEnd()` plus the same normalisation, with non-overlapping candidates. There is no indentation-insensitive, fuzzy, Levenshtein or block-anchor tier. A replacement keeps the file's line endings via an LCS alignment of matched region lines to replacement lines (`withRegionEndings`).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L49-L64 : `normalizeForMatch` and `findOccurrences`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L66-L102 : `findLineOccurrences` (trailing-whitespace tier).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L168-L188 : `matchView` with the three-tier selection (`exact`, `unicode`, `trailing`).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L112-L162 : line-ending preservation and `alignLines`.
- Edge cases and failure handling: when `oldString` and `newString` differ only in `\r`, matching is on exact bytes (so models can change line endings); identical strings and empty `oldString` are rejected before reading (edit.ts L280-L289). Leading-indentation differences are NOT tolerated: a wrong-indent `oldString` fails.
- Round 1: `corrected`. Round 1 said "exact-match replace" with "closest lines" on error; the code adds the Unicode and trailing-whitespace tiers (and the tool description tells the model so).
- For rizzy-harness: this is a conservative, safe fallback ladder (never changes semantics of code, only invisible characters). Adding indentation tolerance is where other harnesses had corruption bugs; copying this ladder as-is is defensible.
- Confidence: traced end to end (checked in review).

#### A failed edit returns a "closest lines" hint voted by whitespace-insensitive line matches, and only if `read` is allowed
- What the code does: on zero matches the tool first checks `permission.allows(read ...)` (never prompts); if reading is already permitted it computes `missHint`: if `newString` (>=20 chars) is already present, the hint says the edit may already be applied; otherwise lines vote for the start offset using whitespace-collapsed, normalised keys (only lines with a letter or digit count); at least half of the "distinct" search lines must agree; it prints up to 12 lines (200 chars each) from the first to the last matched line, numbered like `read` output. Multiple matches without `replaceAll` fail with the count; a no-op result fails with a line-ending-specific message.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L190-L241 : `missHint`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L322-L346 : miss, multi-match and unchanged branches, and the read-permission gate for the hint.
- Edge cases and failure handling: the hint is only the matched span, so padding `oldString` with unrelated lines cannot be used to exfiltrate neighbouring content the model should not see (comment at edit.ts L196-L198); the `read` check protects `.env`-style denied files.
- Round 1: `confirmed` ("edit errors show closest lines"); this answers the Round 2 question "how closest lines are computed, and whether any fuzzy matching exists": a vote on whitespace-insensitive line equality, no fuzzy string distance.
- For rizzy-harness: copy the permission-gated hint; it is a small leak-proof way to make retries succeed without a second `read`.
- Confidence: traced end to end (checked in review).

#### No staleness check on edit; per-path in-process lock; formatter runs after; no LSP diagnostics
- What the code does: `edit` reads the file at execution time, applies the replacement in memory, asks for `edit` permission (with a diff preview), then writes; there is no comparison against what the model last `read` (no mtime or hash), and no "must read first" rule. A process-local keyed mutex serialises edits on the same resolved path, but the comment says external writes can still race. After writing, a formatter matched by file extension runs (`stdin/stdout ignored`, first formatter exiting 0 wins), then the BOM is re-synced, and the diff returned to the UI is computed against the formatted file. LSP notification and diagnostics are explicit `TODO`s.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L291-L321 : read, match, preview, authorize.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L347-L361 : commit, formatted re-read, per-path `withLock`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-mutation.ts#L68-L83 : keyed mutex, "process-local ... external writes can still race".
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-write.ts#L83-L90 : `commit` writes then formats and syncs the BOM.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/formatter.ts#L47-L81 : formatter loop (extension match, `$FILE` substitution, failure logged and next formatter tried).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-mutation.ts#L133-L141 : TODOs for LSP, snapshots and multi-file rollback.
- Edge cases and failure handling: because matching is on current content, a file changed since the model's `read` is safe whenever `oldString` still matches uniquely; it is unsafe when the model's `oldString` happens to match a different, newly introduced location. A formatter failure is logged and does not fail the edit.
- Round 1: `new (not documented)`; Round 1 listed formatters but not the absence of staleness checks or LSP diagnostics in V2.
- For rizzy-harness: a hash or mtime of the last `read` stored per session would be a real improvement over this; cheap to do on a node that owns the files.
- Confidence: traced end to end (checked in review).

#### `patch` (apply_patch format) replaces edit/write only for GPT-family models, with a four-tier line matcher
- What the code does: a hook on context/compaction/generate requests deletes `edit` and `write` from the model's tool list when the model id contains `gpt-` and not `oss` or `gpt-4`; otherwise it deletes `patch`. `patch` takes `*** Begin Patch` envelopes (Add/Update/Delete/Move). `Patch.derive` locates each hunk's old lines with `seek`, trying four comparators in order: exact, `trimEnd`, `trim`, trim plus Unicode normalisation; a failed hunk throws "Failed to find expected lines in <path>:\n<lines>". A heredoc wrapper (`cat <<EOF ... EOF`) is stripped.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/patch.ts#L288-L301 : model gating and tool deletion.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/util/src/patch.ts#L311-L342 : `computeReplacements` (including the trailing-blank-line retry and error messages).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/util/src/patch.ts#L344-L381 : `seek` and the four comparators, `stripHeredoc`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/patch.txt#L1-L33 : the patch tool description.
- Edge cases and failure handling: all hunks are prepared (read and matched) first, one `edit` permission request covers all targets, then files are written sequentially. A write failure mid-way returns "...Completed before failure: a, b": partial application is possible and acknowledged (`file-mutation.ts` L139-L140: "edits are sequential and report partial application"). The `patch` tool's `trim` tier ignores leading indentation, unlike `edit`.
- Round 1: `confirmed` ("exposed only for supported GPT models").
- For rizzy-harness: the per-family choice of edit tool (patch for GPT, string-replace for others) is worth copying if GPT models are supported; the `trim` tier is the risky one.
- Confidence: traced end to end (checked in review).

#### Tool-call input repair and alias before execution
- What the code does: an `execute.before` hook coerces model input to the tool's schema when unambiguous (stringified objects/arrays, numeric and boolean strings, `null` for optional fields, closed-object extras dropped; max depth 6), and maps the name `bash` to `shell` when no `bash` tool exists, appending "There is no "bash" tool, so this ran as "shell"" to the result.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/tool-input-repair.ts#L8-L24 : the repair rules and alias table.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/tool-input-repair.ts#L31-L69 : hooks before and after execution.
- Edge cases and failure handling: ambiguous input is left alone and fails normal schema validation (the error goes back to the model as a tool result).
- Round 1: `new (not documented)`.
- For rizzy-harness: cheap robustness win against weaker models; keep the "tell the model what ran" note.
- Confidence: read the definition.

### Compaction

#### Trigger: before every step, when estimated prompt size reaches `window - max(15% of window, 16k)`
- What the code does: at the top of each `runStep` iteration the runner calls `estimatePrompt` and then `compaction.compact({reason:"auto"})`. `compact` returns `skipped` unless auto is on and `due()`: estimate (provider-measured tokens of the last assistant response plus a 4-chars-per-token estimate of what came after) >= `calculateCeiling`. The ceiling is `window - buffer`; `buffer` is a token count or a "20%" string, ignored if it would leave no room; default is `max(15% of window, 16000)` where the 16k floor applies only if the window is at least 32k. The window is `min(limit.input, limit.context)` (or whichever exists); an unknown window makes the ceiling infinite and automatic compaction never fires (only a provider overflow can trigger).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L249-L258 : estimate then `compact({reason:"auto"})`, then `precompute`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L301-L315 : `compact` gate (`auto` off, `due`).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L440-L449 : `due` (not right after a compaction; native window needs a measured response first).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1163-L1186 : `calculateCeiling`, `summaryRoom`, `inputWindow`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1132-L1156 : `estimatePrompt` (measured anchor plus estimated tail; images 1,500 and PDFs 2,000 tokens flat at L1269-L1274).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/util/token.ts#L1-L5 : the estimator is `length / 4`.
- Edge cases and failure handling: a failed automatic compaction fails the step (`StepFailedError`, llm.ts L253). With `auto: false`, nothing compacts, not even on overflow. `compact` records usage for every attempt, including failed or interrupted ones (compaction.ts L892-L906).
- Round 1: `confirmed` (15% buffer, min 16k, percent or token count); `corrected` for precision: the 16k floor needs a window of at least 32k.
- For rizzy-harness: "last measured prompt tokens + estimate of the tail" is the right trigger; use it.
- Confidence: traced end to end (checked in review).

#### The summary is written by the same model, same system prompt and tools, with the summary prompt appended as the last user message (cache reuse by construction)
- What the code does: `selectModel` uses the hidden `compaction` agent only to pick a model (`agent.model` resolved through the normal resolver, else the session's model); the request is `baseTranscript` of the primary agent (same system parts, same tool definitions, same history) plus a final user message with the prompt. So the prefix equals the previous request and hits the provider prompt cache. If the reply lacks every template heading, one nudge message is added ("did not fill in the required summary template"); a second miss fails the compaction. `maxTokens` is fitted to the room left; a `finish: length` or content-filter result is a failure.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L277-L299 : `selectModel` (compaction agent model else session model).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L451-L470 : the design comment: "Keeping the normal request's prefix intact lets the summary reuse its prompt cache".
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L497-L522 : `send`: prompt appended, headings check, one nudge, failure messages.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L853-L870 : `prepare` builds the request via `requests.compaction` with the context's own agent, tools and system.
- Edge cases and failure handling: the `compaction` hook (plugin API) may return a ready-made summary, in which case no model call is made (`fromHook`, compaction.ts L872-L885).
- Round 1: `confirmed` ("summary request reuses the normal request's prompt cache"; "`agents.compaction.model`").
- For rizzy-harness: build the summary request as `history + one extra user message` rather than a fresh prompt; it is both cheaper and keeps tool-result context.
- Confidence: traced end to end.

#### The compaction prompt and template
- What the code does: two prompts (`buildPrompt(update)`): first compaction ("You MUST summarize the conversation above into a structured summary that will be given to another agent to resume the work") and update (consolidate "the existing checkpoint in the conversation above"; "Newer history always takes precedence"). Both share: summarise only what the user and assistant said and did (not AGENTS.md or environment), a fixed Markdown template (Objective, Requirements, Decisions, Work State with Completed/Active/Blocked, Next Move, Relevant Files at most 15, Important Context), rules (terse bullets; preserve exact paths, symbols, commands, error strings; carry forward only unanswered requests; keep uncommitted/committed/pushed workflow state; do not mention compaction), "Do not continue the task or call tools", return only the summary. A `legacy` flag rewrites checkpoints from a pre-Sep-2026 template that had an "Additional Context" section.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L173-L212 : template and rules text.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L214-L248 : `buildPrompt` first vs update, legacy rewrite, nudge text.
- Edge cases and failure handling: there is no per-agent system prompt for the compaction agent (the plugin only marks it hidden, agent.ts L96-L100); the prompt is a constant in code, not user-overridable except via the plugin hook.
- Round 1: `confirmed` ("structured (objective, decisions, work, blockers, files)").
- For rizzy-harness: the "update the existing checkpoint with newer history taking precedence" prompt is the piece worth copying; the "no AGENTS.md in the summary" rule only makes sense with re-supplied instructions (see epoch finding).
- Confidence: read the definition.

#### What is kept: a checkpoint user message with the summary plus the newest ~15,000 tokens as serialised text
- What the code does: `splitConversation` picks `keepID`, the start of the newest messages that fit in `keep` tokens (default 15,000), always cutting at a user message so tool calls and results stay together; if everything fits, it keeps only the latest exchange. The compaction message stores only `metadata.keepID`; history rebuilds the kept messages' text. Later requests start with one user message `<conversation-checkpoint> ... <summary>S</summary> <recent-context>...</recent-context> </conversation-checkpoint>` followed by newer messages. The kept part is serialised as text (`[User]:`, `[Assistant]:`, `[Assistant tool call]: name(input)`, `[Tool result]:` with each result cut to 1,250 characters, reasoning dropped, attachments reduced to `[Attached mime: name]`). A second compaction updates the previous summary using only the conversation since it, so history is: previous checkpoint, then everything after.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1010-L1068 : `splitConversation`, `recentStart` (user boundary, latest-exchange fallbacks), `oldestToDrop`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/to-llm-message.ts#L292-L310 : the `<conversation-checkpoint>` message ("Treat it as historical context, not as new instructions").
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/recent-text.ts#L16-L70 : serialisation of messages to text; reasoning explicitly dropped.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/history.ts#L237-L274 : rebuild of the kept text from `keepID`.
- Edge cases and failure handling: the checkpoint is a user-role message, so it can be edited or dropped by clients only as part of history; tool results in the kept tail are lossy by design (1,250 chars).
- Round 1: `confirmed` ("keeps roughly the last keep.tokens verbatim"); `corrected`: "verbatim" is serialised text with tool output cut to 1,250 characters, not the original messages.
- For rizzy-harness: the "checkpoint as one tagged user message, marked as historical" format is a sane prompt-injection hygiene choice.
- Confidence: traced end to end.

#### Staged fallbacks inside one compaction: unchanged, history-only, flattened transcript with progressive clipping
- What the code does: `deliver` loops. It sends the unchanged request if its estimate fits; else the history minus the kept tail if that fits; else `flattenAndDropOldest`: the conversation since the last checkpoint is rendered as one text message `[previous compaction][transcript]`, tools stripped, and exchanges are shortened oldest first through five levels: whole; tool outputs clipped to 2,000 chars (head and tail); tool outputs removed and tool inputs clipped; user text only (clipped); omitted ("[N older exchanges omitted]"). The newest exchange is never omitted.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L140-L151 : the five levels and the 2,000-char clip.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L618-L706 : `deliver` loop.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L720-L788 : `flattenAndDropOldest`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1071-L1110 : `flattenMessage` and `clip`.
- Edge cases and failure handling: media is gone in the text form; "payload too large" on an unchanged request retries as text without consuming a shrink step; other provider errors are retried under the normal retry policy; a `Failure` from the reply itself is never retried (compaction.ts L683-L706). If nothing fits it fails with "cannot be reduced further without losing the latest exchange or checkpoint".
- Round 1: `new (not documented)`.
- For rizzy-harness: copy the ladder (oldest-first, level-by-level clipping, never drop the newest exchange); it is the part most harnesses get wrong.
- Confidence: traced end to end (checked in review).

#### Recovery from a context-length error: compact once at 85% of the rejected size, then 70, 50, 35
- What the code does: if the provider rejects the main request as context overflow before any output started, `recoverOverflow` calls `compact({reason:"overflow"})` once per step (`Compacted` outcome sets `recoverOverflow = false`), and the step restarts. Inside `deliver`, each further "too long" rejection shrinks the next attempt to 85%, 70%, 50%, then 35% of the first rejected request's size; a rejection after that gives up. Overflow is recognised by `isContextOverflowFailure` (a classification on the provider error, built from message patterns with exclusions for rate-limit wording).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L186-L197 : `isContextOverflowFailure` + `recoverOverflow` before output started returns `Compacted`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L316-L322 : `recoverOverflow` closure (once per step).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L126-L127 : `SHRINK_STEPS = [0.85, 0.7, 0.5, 0.35]`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/ai/src/provider-error.ts#L60-L80 : overflow patterns, exclusions and `isContextOverflowFailure`.
- Edge cases and failure handling: an overflow that arrives after output started is not recovered; it surfaces as the step's error. Overflow is excluded from the generic retry path (step.ts L232-L234).
- Round 1: `confirmed` ("overflow errors trigger compact-and-retry once"); the progressive shrink schedule is `new (not documented)`.
- For rizzy-harness: keep the "estimates can be wrong, so react to a real rejection with a smaller target" loop.
- Confidence: traced end to end (checked in review).

#### Background ("precomputed") summary: in memory, one per session, invalidated by five conditions, lost on restart
- What the code does: only when the experimental flag is on and `auto` is on. When the estimate is within `PRECOMPUTE_LEAD` (60,000 tokens, or half the cycle's room on small windows) of the ceiling, the runner forks a summary of everything before the latest response (so the request reads that prefix from cache) and holds the fiber in an in-process `Map` keyed by session. When compaction becomes due, the held summary is awaited and swapped in with everything since it kept verbatim, provided the tail is at most 40% of the budget. It is discarded if the first message changed, the instruction baseline text changed, the model or variant changed, `keepID` is gone, or the tail is too long; no new background summary starts after two failures, when the latest response is older than the cache lifetime minus 30 s, or after a model switch until the new model has a response. An overflow never uses it.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L351-L407 : `precompute` conditions and the fork.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L414-L438 : `usePrecomputed`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L971-L976 : `heldIndex` (first message, instruction text, model, variant, keepID).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L265-L265 : `precomputed = new Map(...)`: process memory only.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L326-L333 : a final precompute when the session is about to go idle.
- Edge cases and failure handling: a restart loses the held summary (it is a fiber in a Map), and the unused background run's tokens are billed (usage recorded into the compaction message when used, compaction.ts L433-L437). Round 1 asked how it is invalidated and what is lost on restart: see above.
- Round 1: `confirmed` ("pre-writes the summary ahead of the threshold ... held summaries lost on restart; unused ones cost tokens").
- For rizzy-harness: an optimisation that depends on exact cache semantics; skip until the plain path is solid.
- Confidence: traced end to end.

#### Native (provider-side) compaction is a per-model setting
- What the code does: if the resolved model's `compaction.type` is `native`, the summary agent is not used. If the route supports the "trigger" mechanism, a compaction trigger is sent and the result is `[recent user messages within keep] + checkpoint`; else if it supports an "endpoint" mechanism, the provider returns a replacement window. The window is stored as `native.messages` and replayed only on the same model (history selection keeps native windows only for the target model).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L565-L603 : `compactNatively`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model-resolver.ts#L211-L223 : rejects `native` on routes that cannot compact, at model construction.
- Edge cases and failure handling: an encrypted native window estimates as nothing locally, so `due` waits for a measured response (compaction.ts L446-L447).
- Round 1: `confirmed` (native vs summary).
- For rizzy-harness: skip; provider-specific and opaque.
- Confidence: read the definition.

#### There is no pruning of old tool output in V2; tool output is bounded once, at call time
- What the code does: each tool result passes `ToolOutput.truncate` before it is stored: over 2,000 lines or 50 KiB, the result is cut to the first lines that fit and a marker `[showing lines 1-N of M; full output saved to <path>]` is appended; the full text goes to `<data>/tool-output/tool_<id>` and files older than 7 days are cleaned. Limits are configurable (`tool_output.max_lines/max_bytes`). Nothing later clears or elides old tool results in the live history; only compaction does. Searches for `prune`/`elide` in `packages/core/src` found nothing in the engine.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool-output.ts#L13-L15 : limits and 7-day retention.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool-output.ts#L69-L120 : `truncate`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L145-L152 : the result goes through `toolOutput.truncate` right after execution.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/model-request.ts#L216-L244 : the only other old-content reduction, `boundImages`: images over 25 MiB total are replaced oldest-first down to 15 MiB with a fixed text, using hysteresis so the replaced prefix (and the cache) changes only every ~10 MiB.
- Edge cases and failure handling: the model is told the path of the full output but must use `read` with offsets to see it; the saved file lives under the app data dir (a read of it is an external-directory read, governed by permissions).
- Round 1: `corrected`/`confirmed`: Round 1 said "V1 had tail-turn and prune behaviour; those keys are ignored in V2" (confirmed: no prune), and that tool output retention is configurable (confirmed). Round 1 did not mention `boundImages`.
- For rizzy-harness: a stricter "never rewrite old history" stance is good for caching; the cost is that a long session of large tool outputs relies entirely on compaction. Consider a one-time "clear tool outputs older than N" step as a first compaction stage.
- Confidence: traced end to end (checked in review).

### Built-in agents and custom agents

#### An agent is data: id, name, model ref, request overlay, system, mode, hidden, steps, ordered permission rules
- What the code does: `Agent.Info` has `mode` (`subagent|primary|all`), `hidden`, optional `model`, `system`, `steps`, `permissions` (a list of `{action, resource, effect}`), `request` (headers/body/settings). A fresh agent starts with `[* allow, read *.env ask, read *.env.* ask, read *.env.example allow]`. Permission evaluation is last matching rule wins; no match means `ask`. The default agent is `config.default_agent` if selectable, else `build`, else the first selectable agent (selectable = not `subagent` and not hidden).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L23-L54 : `Agent.Info` and the default rules.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L94-L108 : `evaluate` (`findLast`, default ask) and `merge` (concatenation).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/agent.ts#L53-L64 : `selectedDefault`.
- Edge cases and failure handling: agent permissions plus session permissions are concatenated (`Permission.merge(agent.permissions, session.permissions)`, context.ts L146), so session-level rules come last and win.
- Round 1: `confirmed` ("agent = system prompt + model + permissions + display info").
- For rizzy-harness: ordered last-match-wins lists are simple to reason about and merge by concatenation; copy.
- Confidence: traced end to end (checked in review).

#### The built-in roster: build, plan (primary); general, explore (subagent); compaction, title, summary (hidden)
- What the code does: `AgentPlugin` edits the agent registry: `build` ("Build", primary, `question: allow`); `general` (subagent, `question: deny`, `subagent: deny`); `explore` (subagent, own system prompt "file search specialist... MUST NOT create, modify, delete... Shell commands MUST be read-only", rules: deny `*`, then allow shell, grep, glob, webfetch, websearch, read (with `.env` asks), `subagent: deny`, `external_directory: allow`); `compaction` (primary, hidden, no system, no rules); `title` (primary, hidden, system = title-naming prompt, deny `*`); `summary` (primary, hidden, system = "2-3 sentence PR-description summary", deny `*`). The roster is first-party defaults; configuration then edits or removes them.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L55-L118 : the transform defining all six.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L7-L50 : the explore, title and summary prompts.
- Edge cases and failure handling: `explore`'s read-only rule is enforced by its permission list (deny all, allow a few) and also by prompt; because `shell` is allowed, the "read-only shell" part is prompt-only; the shell rule is `allow` for any command and no scanner-level read-only constraint is applied in this file. `general` is described as "general-purpose" but has the full default rule set (`* allow`), minus `question` and `subagent`.
- Round 1: `confirmed` (build, plan, general, explore; hidden compaction, title, summary; general cannot launch subagents). `new (not documented)`: the exact prompts and that `explore` allows `external_directory` and arbitrary shell.
- For rizzy-harness: Explore as "deny all + allow list" is a clean pattern; but allowing unrestricted shell in a "read-only" agent is only prompt-enforced. If a read-only agent matters, give it a read-only shell or no shell.
- Confidence: traced end to end (checked in review).

#### Where the hidden `summary` agent is used: no consumer found
- What the code does: `summary` is defined (plugin/agent.ts L110-L116), but no code path in `packages/core/src`, `server/src`, `cli/src` or `tui/src` selects it; searches for `ID.make("summary")` and `agent: "summary"` returned only the definition. `title` is used by `SessionTitle`; `compaction` only for its optional `model`.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L110-L116 : the definition.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L99-L101 : `selectTitle` gets the `title` agent (the only hidden agent selected by id in the engine besides compaction at compaction.ts L283).
- Edge cases and failure handling: users may still configure `agents.summary.model`, but nothing reads it as far as I could find.
- Round 1: `corrected`: Round 1 lists `summary` as a working hidden maintenance agent; in this commit it looks like a V1 carry-over with no caller. (Settle by searching clients outside `packages/core` more widely; I searched core, server, cli, tui.)
- For rizzy-harness: do not build a `summary` agent without a consumer.
- Confidence: read the definition (absence of a consumer is inferred from searches) (checked in review).

#### Plan mode is permissions plus reminders, and it does not deny shell
- What the code does: `PlanPlugin` sets plan permissions to `question allow`, `edit *` deny, `edit <home>/.opencode/plan/*` allow. Shell is not denied: the plan agent inherits the default `* allow`, so it can run `shell` and write files that way. Enforcement layers: (1) rules; (2) an `execute.after` hook that rewrites the generic "blocked" error for edit/write/patch into "Cannot use X to modify files outside the Plan directory"; (3) a `<system-reminder>` "You are in Plan mode..." injected as a durable synthetic message when the session starts as plan or the agent switches to plan, and a "NO LONGER in Plan mode" reminder on leaving; (4) a per-request `context` hook that re-injects the reminder just before the last user message if compaction or revert removed it. The reminder text asks the model not to modify files "or ask a subagent to do so" and to tell the user to switch agents to implement.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 : plan agent rules (edit deny plus plan-dir allow, no shell rule).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L44-L53 : error rewrite.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L14-L25 : reminder texts.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L55-L92 : reconciliation per request and on agent events.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L39-L52 : the inherited default `* allow`.
- Edge cases and failure handling: a plan agent can mutate files via `shell` (`sed -i`, redirects) or by launching `general`, a subagent with its own rules (the subagent permission is `*: allow` by default; the child inherits session-level rules only), so plan mode is advisory for shell and subagents. The plan dir is outside the project, so plan files are not in the repo.
- Round 1: `confirmed` for "edit denied except under ~/.opencode/plan" and the README-vs-V2 doc discrepancy resolves as: V2 plan neither denies nor asks for bash; it relies on the default `allow`. Answer to Round 2 question: "plan's rule set only denies edits".
- For rizzy-harness: if plan mode must be real, deny the shell (or run it in a read-only sandbox) and `subagent` for plan; reminders and permissions alone leak.
- Confidence: traced end to end (checked in review).

#### Custom agents load from `agent(s)/` and `mode(s)/` markdown or from the `agents` config object; global permissions are appended to every agent
- What the code does: for each config directory entry, `{agent,agents}/**/*.md` and `{mode,modes}/*.md` are scanned (mode files are forced `primary`). The relative path minus the directory and `.md` becomes the id (nested paths allowed). Frontmatter is decoded as the V2 `ConfigAgent.Info` with the body as `system`; frontmatter with unknown keys and none of the V2-only keys (`permissions`, `request`, `system`, `disabled`) is decoded as a V1 agent and migrated; an invalid file is logged and skipped. Config directories (from `config/discovery.ts`): the system dir (`/etc/opencode` etc.), the global config dir, and walking up from the working directory each `.opencode`, `.claude` and `.agents` directory, plus global `~/.claude` and `~/.agents`. In the transform, config-level `permissions` are pushed onto every existing agent; per-agent entries then update fields (model, request, system, description, mode, hidden, color, steps) and append their own permissions; `disabled: true` removes the agent.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L22-L35 : source patterns and V2-only key list.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L148-L196 : markdown decode, legacy migration, error handling.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L64-L108 : the transform (permissions pushed to all agents, field updates, disable).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/discovery.ts#L82-L82 : candidate directory names `.claude`, `.agents`, `.opencode`.
- Edge cases and failure handling: a repo-controlled `.opencode/agents/x.md` can define an agent with an arbitrary `system` and `permissions: [* allow]`; the file is trusted like project config (no trust prompt in this path); project rules are appended after the builtin ones, so a project agent file's rules win over defaults. Hot reload is wired through `ConfigEntryObserver`.
- Round 1: `confirmed` (markdown in `.opencode/agents/` or global; nested paths become IDs; options listed); `new (not documented)`: `.claude` and `.agents` directories are also scanned, and `mode` directories.
- For rizzy-harness: project-supplied agent files are prompt and permission injection; gate them behind project trust.
- Confidence: traced end to end.

### Sub-agents

#### The `subagent` tool: creates a child session with fresh context, inherited location and session permissions
- What the code does: the tool (named `subagent`, not `task`) takes `agent`, `description`, `prompt`, optional `model`, `sessionID` (follow-up on an existing child), `background`. It resolves the agent (must not be `primary`), asserts the parent's `subagent` permission for that agent id, resolves an optional model override (must be a chat model, optionally `#variant`), then calls `sessions.create({parentID, title: description, agent, model})`. Model for a new child: explicit override, else the agent's configured model, else the parent session's model. `Session.create` copies the parent's location, metadata and `permissions` (session-level rules) to the child. The child's first prompt is `"You are a subagent spawned by another session.\n" + prompt`; none of the parent's history is passed ("New child sessions start with fresh context"). The child's tools, instructions (AGENTS.md etc.), system prompt and permissions come from the same machinery as any session: the child agent's rules plus inherited session rules.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L30-L62 : input schema and description.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L140-L152 : agent resolution, `primary` rejected, `subagent` permission assertion.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L168-L218 : model choice, follow-up semantics, child creation, first prompt.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L288-L316 : `create` inherits location, metadata and permissions from the parent.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L142-L146 : the child's permissions are `merge(agent.permissions, session.permissions)`.
- Edge cases and failure handling: a follow-up with a `sessionID` must be a direct child of the current session (else "is not a child of the current session"); changing agent on a follow-up switches the child's agent and, if given, its model. An interrupted foreground call interrupts and cancels the child (subagent.ts L234-L240).
- Round 1: `confirmed` (fresh context, inherits parent's model when none, parent's `subagent` permission controls which can launch); `corrected` ("the child uses its own permissions"): the child uses its agent's rules plus the parent session's session-level rules (for example approvals saved on the session).
- For rizzy-harness: "session permissions inherited by children, agent rules per child" is the right composition: a child can never be broader than the user's session-level restrictions.
- Confidence: traced end to end (checked in review).

#### Result return: last completed assistant text, wrapped in `<subagent>`; background mode delivers a synthetic message to the parent
- What the code does: foreground: `jobs.block` waits for the child run; the job's output is the text parts of the most recent completed, non-error assistant message among the child's last 20 messages (joined by blank lines), or "Subagent completed without a text response." The parent's tool result is `<subagent sessionID=".." state="completed">...</subagent>`. Failure or cancellation returns a tool failure that still includes the child `sessionID`. Background: returns immediately with instructions ("DO NOT sleep, poll...", "call subagent again with this sessionID to send follow-ups"), and when the job ends a `<subagent ... state="completed|error|cancelled">` synthetic message is admitted to the parent session, waking it. A foreground call can be backgrounded by the user mid-run (`result.type === "backgrounded"`).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L220-L266 : start job, background or block, result shapes.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/subagent-job.ts#L47-L64 : job `run`: resume the child, read last 20 messages, take the completed assistant text.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/subagent-completion.ts#L9-L46 : `text` extraction and `deliver` as a synthetic message with metadata.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L416-L439 : after a crash, a running `subagent` tool call is failed as "Tool execution interrupted ... (sessionID: ...)" so the model can continue the child.
- Edge cases and failure handling: only the text of the last message returns; intermediate findings are lost unless the child wrote them into that message. The child's session persists and is resumable by id (also by the user in the UI).
- Round 1: `confirmed` (background returns immediately and notifies; `sessionID` continues a child).
- For rizzy-harness: copy "child is a normal session, linked by `parentID`; only the final text returns; the child stays inspectable and resumable".
- Confidence: traced end to end.

#### Depth and concurrency: depth limit 1 by default, `general` and `explore` explicitly cannot spawn; no concurrency cap found
- What the code does: the tool counts ancestors of the calling session; if `depth >= experimental.subagent_depth` (default 1) it fails with "Subagent depth limit reached". With the default, only top-level sessions can spawn. Separately, `general` and `explore` carry `subagent: deny` rules, so even raising the depth limit leaves them unable to spawn; a custom agent can. The list of "Available subagents" in the tool description is built per request from agents with mode not `primary`, not hidden, and not denied for the calling agent by `Permission.evaluate("subagent", id, ...)`. The GPT prompt adds "Do not spawn subagents unless the user or applicable AGENTS.md/skill instructions explicitly ask". No cap on concurrent subagents appears in this file or in the step code: each tool call in a step is forked as its own fiber, so parallel `subagent` calls run in parallel.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L123-L139 : ancestor walk and depth check (`?? 1`).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L282-L309 : per-request tool description with the filtered agent list.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L143-L157 : each provider `tool-call` event forks its own fiber.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L63-L94 : `subagent: deny` on `general` and `explore`.
- Edge cases and failure handling: the depth key lives under `experimental` in config (and `subagent_depth` as a top-level key is rejected in `normalize.ts` L45). A stale parent chain (missing session) fails the call with "Parent session not found".
- Round 1: `confirmed` (depth default one, experimental flag, `general` cannot launch subagents).
- For rizzy-harness: add an explicit concurrency limit; OpenCode relies on the model's discretion.
- Confidence: traced end to end for depth; concurrency absence is inferred from searches (checked in review).

### Model roles, small model, retry

#### Which model a session uses: session model, then agent model, then the configured default, then the first text model
- What the code does: `SessionRunnerModel.select` uses `session.model` if present (fails with `ModelUnavailableError` if it is not available); otherwise the selected agent's `model` if available; otherwise `ModelResolver.fallback()`: the catalog default model if enabled, else the first available text model. A plugin hook `experimental.model.resolve` may swap the model within the same provider only. There is no per-agent fallback chain and no "if this model errors use that one" mechanism in the loop.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/model.ts#L81-L121 : `select` and `resolve` with the hook.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model-resolver.ts#L523-L527 : `fallback`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model.ts#L301-L306 : `default`.
- Edge cases and failure handling: no code path retries a failed request on a different model (search for fallback chains in core, schema, plugin, ai found only a ChatGPT catalog list).
- Round 1: `confirmed` (`provider/model#variant`); Round 1's synthesis claims of "per-agent fallback chains with a `model_fallback` event" are not true of OpenCode (that sentence names Claude Code and others); OpenCode has none.
- For rizzy-harness: if model failover is wanted, it must be built; OpenCode is not a precedent.
- Confidence: traced end to end for selection; "no fallback" is inferred from searches.

#### The small model: no global `small_model`; titles pick a per-provider "small" by family, with a low-reasoning variant
- What the code does: `selectTitle` takes the `title` agent; if it has a `model`, that is used (variant: configured, else a minimal-reasoning variant `none|minimal|low`). Otherwise it takes the session's primary model's provider and calls `Model.small(providerID)`: among that provider's active text models, the first of the families `gpt-luna`, `gemini-flash-lite`, `gemini-flash`, `claude-haiku` in that order. If the provider has none, the primary model itself is used at a lower-reasoning variant if one exists. If the chosen title model fails (an `AI.Error` is swallowed), a second attempt runs on the primary model. Title input is the first user message (<=2,000 chars), or for an already titled session the original request plus recent turns (8,000 char budget). The title is truncated to 100 chars and written only if the session's event sequence has not moved (optimistic commit). The other background roles do not use a small model: compaction uses the session model unless `agents.compaction.model`, and the goal reviewer and cache warming use the session's current model via `session.generate`.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L99-L134 : `selectTitle`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L200-L213 : minimal/lower variant rules.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model.ts#L307-L318 : `small` with the four families.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/title.ts#L114-L163 : prompt building, two-attempt fallback, truncation, sequence check.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/generate.ts#L43-L77 : `generate` uses the session's own model and history.
- Edge cases and failure handling: a title failure is silent (the session keeps its fallback title); titles are generated only for root sessions (`!selected.session.parentID`, llm.ts L200-L203) and in a forked fiber so they never delay the run.
- Round 1: `confirmed` ("`small_model` replaced by `agents.title.model`"); answers the Round 2 question "what model does the hidden title agent default to when `agents.title.model` is unset": a same-provider small model by the family list above, else the primary at lower reasoning.
- For rizzy-harness: "small model" as a per-provider family list is brittle (it hard-codes names); a configured slot with a fallback to the session model at low reasoning is simpler.
- Confidence: traced end to end (checked in review).

#### Retry and backoff: general schedule (10 retries, 2 s doubling to 10 s, jittered, honouring retry-after up to 15 min), separate one-hour budget for "no network"
- What the code does: on a provider error that is retryable and not a context overflow, `runStep` consults a per-step `ModelRetry.policy` stateful decider. General schedule: exponential from 2 s capped at 10 s per gap, at most 10 retries (about 84 s total), jittered; a `retry-after` from rate-limit or provider-internal errors raises the delay (capped at 15 minutes). Failures whose Node error code is `ENOTFOUND`, `EAI_AGAIN`, `ENETUNREACH`, `EHOSTUNREACH` or `ENETDOWN` use a separate schedule (2 s doubling to 30 s per gap, for up to one hour after the first) that does not spend the general allowance. At most 3 retries after transport timeouts. A plugin `retry` hook can veto or change the delay. A retry before any output started re-runs the same assistant message; after output started only interrupted streams (incomplete stream or read failure) retry, with a fresh assistant message. A `RetryScheduled` event with the wake-up time is published so UIs can show a countdown.
- What is retryable (`AIError.isRetryable`): rate limit, provider-internal (5xx), transport failures where the request was not rejected (accepted deliveries only for read failures), incomplete streams, and unknown provider errors; not retryable: authentication, quota exceeded, content policy, invalid request, unsupported operation, timeout reason. The response header `x-should-retry: true|false` overrides.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model-retry.ts#L11-L36 : retry-after cap and general schedule.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model-retry.ts#L38-L89 : unreachable-network schedule, timeout cap, `policy`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/ai/src/schema/errors.ts#L197-L230 : `isRetryable` by error class and the `x-should-retry` override.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L225-L248 : retry decision; before and after output started.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/retry.ts#L32-L78 : plugin `retry` hook, `RetryScheduled` event, sleep until `at`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model-retry.ts#L104-L120 : one-shot calls retry only if the provider never ran (429, 503/529, not-sent transport), at most 3.
- Edge cases and failure handling: transport failures flagged `retry-full` or `rotate-and-retry-full` (a WebSocket continuation that cannot be resumed) first fall back to a full request via `RecoverFull` (step.ts L225-L231). Non-retryable failures end the step with the error shown.
- Round 1: `new (not documented)` ("Nothing documented; settle by reading the provider layer").
- For rizzy-harness: the split between "no network" (patient, one hour) and "provider unhappy" (bounded, about 84 s) is a good UX detail; the retry-after cap is a defence against hostile or buggy headers.
- Confidence: traced end to end (checked in review).

#### Per-agent step limit: tools stay in the request, `toolChoice` is `none`, and a synthetic "maximum steps" message is appended
- What the code does: if `agent.steps` is set and the current step number is at least it, the request gets an extra assistant message `CRITICAL - MAXIMUM STEPS REACHED ... Respond with text only` and `toolChoice: "none"`; tool definitions are kept "to preserve the provider's cached prefix". If a model calls a tool anyway, execution returns "Tools are disabled after the maximum agent steps". Without `steps` there is no step limit.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L259-L281 : `stepLimitReached`, appended message, `toolChoice`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/max-steps.ts#L1-L16 : the prompt text.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L116-L118 : tool execution refused when `toolChoice` is `none`.
- Edge cases and failure handling: step numbering resets to 1 when new user input is promoted (llm.ts L204), so the cap is per user turn.
- Round 1: `corrected`: Round 1 said "on the final step tools are removed"; the code keeps them in the request and disables calling them.
- For rizzy-harness: copy the cache-preserving form (keep schemas, set `tool_choice: none`, reject calls).
- Confidence: traced end to end (checked in review).

#### Output-limit handling and sizing
- What the code does: the output token limit is the catalog limit (fallback 32,000, ceiling 256,000) fitted to the room left in the window (`context * 0.99 - measured - estimated * 1.15`), never below 1,024. If a response ends with `length`, the runner publishes a synthetic nudge ("Your last response hit the output token limit ... Break the remaining work into smaller pieces") and continues; three consecutive `length` responses fail the step ("still truncated after two output token limit continuations").
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/model-request.ts#L49-L103 : constants and `outputLimit`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L343-L373 : `OutputLimit` branch.
- Edge cases and failure handling: the continuation message is a `synthetic` event, visible in history.
- Round 1: `new (not documented)`.
- For rizzy-harness: simple and effective; copy the "three length stops in a row = fail" cutoff.
- Confidence: traced end to end.

#### Stuck-loop detection: there is none in the step loop; the only repetition guard is the goal reviewer's stall counter
- What the code does: nothing in `runStep` detects repeated identical tool calls (searches for doom, identical or repeated tool calls found only the V1 `doom_loop` config key). The `/goal` plugin has `MAX_ROUNDS = 50` and `MAX_STALLS = 3`: a round counts as a stall if the reviewer's `continue` reason equals the previous one or the model used no tool since the last continuation; three stalls pause the goal ("Not making progress"). It also pauses on server restart, after 30 minutes waiting on background work, and when the reviewer's own request overflows twice (compacting once in between).
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal.ts#L15-L19 : limits.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal.ts#L140-L203 : the `experimental.idle` hook (`rounds`, `stalls`, verdict handling, overflow compaction).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal.ts#L89-L102 : `usedTools`.
- Edge cases and failure handling: without `/goal`, a model looping on the same failing call runs until the user stops it, or until `agent.steps` (unset by default) is reached.
- Round 1: `confirmed` ("what `doom_loop` was in V1 and whether V2 has any repetition guard": V1 key only; V2 has none outside goal).
- For rizzy-harness: a cheap guard (same tool+input three times in a row) is missing here; add it.
- Confidence: traced end to end for goal; absence elsewhere is inferred from searches (checked in review).

#### The `/goal` reviewer is the session's own model asked for one line of JSON, in the same cache prefix
- What the code does: when the session would go idle, a plugin hook calls `session.generate` (same agent, tools, instructions, model and history as the main loop, so the prefix is cached) with `judge.txt`: "Stop acting as the assistant. You are now the reviewer ... Decide whether the goal has been met, using only evidence in the conversation ... The assistant claiming the goal is met is not evidence ... Reply with exactly one line of JSON" with verdicts `done|continue|blocked|paused|wait`. The objective is wrapped as `<untrusted_objective>`. Unusable output is retried once, then the goal pauses. `continue` injects a synthetic continuation message and the loop proceeds.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal.ts#L104-L136 : `judge` calling `ctx.session.generate`, one retry.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal/judge.txt#L1-L20 : the reviewer prompt.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/generate.ts#L43-L77 : `generate` reuses selection, history and the session model.
- Edge cases and failure handling: the reviewer is the same model that did the work (no independent judge), and `generate` retries only buffered calls with `ModelRetry.retry`.
- Round 1: `confirmed`/`new`: answers "how the `/goal` reviewer works and with which model": the session's own model, same agent and history.
- For rizzy-harness: a same-model reviewer shares the model's blind spots; if added, allow a distinct reviewer model.
- Confidence: traced end to end.

#### Cache warming: a keep-alive `generate` ping at 90% of the cache TTL, off by default, stops on a cache miss
- What the code does: when `warming` is configured (top level or per-model `cache.warming`), a hook after each request of a root session (child sessions are skipped) starts a loop that sends `session.generate` with "This is a keep-alive request. Do not perform any work... Reply with exactly: OK" every `0.9 * cache.ttl` (4 minutes if no ttl), for up to 30 minutes after the last real request. If a ping reports more cache-write than cache-read tokens, warming stops. If the ttl is longer than the window, no ping is needed.
- Trace:
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/warming.ts#L15-L20 : defaults (prompt, "auto", 30 minutes).
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/warming.ts#L36-L80 : `decide`: interval 0.9 x ttl, 4 minute fallback, `CacheOutlivesWindow`.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/warming.ts#L111-L129 : the ping and the miss check.
  - https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/warming.ts#L133-L137 : subagent sessions skipped.
- Edge cases and failure handling: a failed ping is logged and the loop continues; the ping runs the same hooks as real requests and is recognised by its prompt text (comment at warming.ts L142).
- Round 1: `confirmed` ("at 90% of cache TTL, or every four minutes; stops 30 minutes after the last real request; not for subagent sessions"); the stop-on-miss rule is `new (not documented)`.
- For rizzy-harness: skip; costs real tokens and depends on provider cache timing.
- Confidence: read the definition.

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| V2 is current; V1 remnants only | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/v1/config/permission.ts#L27-L38 (V1 shapes live under `v1/` migration code) |
| Step limit: "on the final step tools are removed and the model must answer in text" | corrected: tools stay in the request, `toolChoice: none` plus a synthetic message, calls refused | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L259-L281 |
| `edit` is exact-match; errors show closest lines | corrected: three tiers (exact, Unicode-normalised, trailing-whitespace lines); hint is a line vote | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L168-L188 |
| `patch` exposed only for supported GPT models (others edit/write) | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/patch.ts#L288-L298 |
| Compaction automatic at window minus buffer (default 15%, min 16k) | confirmed (16k floor only for windows of at least 32k) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1163-L1171 |
| Keeps roughly last `keep.tokens` (15,000) verbatim | corrected: serialised text, tool results cut to 1,250 chars | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/recent-text.ts#L6-L85 |
| Later compactions update the summary using only newer conversation | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L214-L245 |
| Summary request reuses the normal request's prompt cache | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L451-L470 |
| Overflow errors trigger compact-and-retry once | confirmed (plus shrink schedule 85/70/50/35%) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L126-L127 |
| Summary model overridable via `agents.compaction.model` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L277-L299 |
| `settings.compaction.type = native` vs `summary` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L565-L603 |
| Background compaction pre-writes the summary; lost on restart | confirmed (in-memory `Map`) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L265-L265 |
| V1 tail-turn and prune behaviour ignored in V2 | confirmed: no pruning in V2 | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool-output.ts#L69-L120 |
| Nested AGENTS.md load on read, not re-injected while in history | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instructions.ts#L100-L113 |
| `instructions` entries: files, globs, `~/`, URLs with 5 s timeout | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/instruction.ts#L98-L118 and L151-L179 |
| Keyed session instructions do not alter the system prompt | corrected: they do alter the instruction block at baseline; changes become mid-conversation system messages, and compaction re-bases | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L126-L140 |
| `plan` denies edits except under the plan dir; README says asks before bash | corrected: plan does not deny or ask for shell (inherits `* allow`) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 |
| `general` cannot launch subagents; `explore` read-only | confirmed (explore read-only only for edit tools; shell allowed) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L63-L94 |
| Hidden agents `compaction`, `title`, `summary` | corrected: `summary` has no caller found | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L96-L116 |
| Subagent starts with fresh context, inherits parent's model if none | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L184-L218 |
| Child uses its own permissions | corrected: agent rules plus inherited parent session rules | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314 |
| Subagent depth default one | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L135-L139 |
| Agent markdown in `.opencode/agents/` or `~/.config/opencode/agents/`, nested paths become IDs | confirmed, extended: also `.claude`, `.agents`, `mode(s)/` and the system dir | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L22-L27 |
| `small_model` replaced by `agents.title.model` | corrected: with no title model, a per-provider family list (`gpt-luna`, `gemini-flash-lite`, `gemini-flash`, `claude-haiku`) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model.ts#L307-L318 |
| Cache warming at 90% of TTL or 4 min, 30 min window, not for subagents | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/warming.ts#L36-L80 |
| `/goal` objective loop with a reviewer | confirmed (same model, JSON verdict, stall counter) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal.ts#L104-L136 |

## 5. In the code but not in the documentation

- Per-model system prompt overlays (GPT, Astra, Kimi, Trinity, Muse, a Claude comment-density snippet) keyed on substrings of the model id: plugin/optimize.ts L16-L62.
- Identity block ("# Your Model") and the default `textVerbosity: low` for some OpenAI models: plugin/identity.ts, plugin/verbosity.ts.
- The hashed, delta-based instruction system with an epoch re-base at compaction.
- The permission-gated "closest lines" hint and the Unicode and trailing-whitespace tiers of `edit`; four-tier matcher of `patch`.
- Tool-input repair and the `bash` to `shell` alias.
- The compaction fallback ladder (unchanged, history-only, flattened transcript with five clip levels) and the 85/70/50/35% shrink schedule.
- `boundImages`: oldest-first image replacement with hysteresis (25 MiB trigger, 15 MiB target).
- Output-limit sizing and the "three `length` stops fail" rule.
- Retry schedule details: 2 s doubling to 10 s, 10 retries, one-hour "no network" budget, retry-after cap 15 min, `x-should-retry` override, plugin `retry` hook.
- Plan mode reminders re-injected per request after compaction or revert.
- `.claude/` and `.agents/` directories feed the agent loader (and `mode(s)/` files become primary agents).
- Request headers: every model request carries `x-opencode-session-id`, `x-opencode-parent-session-id` for children, a session-affinity header and a prompt-cache key derived from the session id (model-request.ts L313-L328).

## 6. Could not find or could not settle

- **A consumer of the `summary` agent.** Searched `packages/core/src`, `server/src`, `cli/src`, `tui/src` for `ID.make("summary")` and `agent: "summary"`. Settle by searching `packages/app`, `desktop` and `sdk` for a `summary` agent call, or by running a session with `agents.summary.model` set and watching requests.
- **A cap on concurrent subagents.** None found in the tool or step code; a scheduler elsewhere (`job.ts`) could apply one. I did not read `job.ts` or the `Job` service.
- **Size limits on AGENTS.md and instruction URLs.** No limit seen in `config/plugin/instruction.ts` or `instruction-discovery.ts`; a provider-side limit may exist. `InstructionEntry` (API-attached context) does have `MaxValueBytes` (instruction-entry.ts L18).
- **Permission rule evaluation for `external_directory` and shell for `explore`** (safety cluster); I read only `Permission.evaluate`.
- **How a user-selected session model interacts with an agent's `model` on agent switch.** `select` prefers `session.model`; whether switching agents rewrites `session.model` is in the session-switch code I did not read (`Session.switchAgent`).
- **Whether the `Environment` abstraction (remote workspaces) changes how `edit` reads and writes files.** `edit` goes through `environment.files`, so a remote environment would put the edit logic on the master side; not traced (sessions cluster).

## 7. Citation check

`python3 research/round2/check_citations.py research/round2/opencode.agents.md`:

/run/media/lebron/PROJ/Rust/rizzy-harness/research/round2/opencode.agents.md: 170 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit

## 8. Review

Checked 19 of the 38 findings. No finding was wrong or overstated in what was read; none needed a text correction. Two caveats are noted in the table.

| Finding | Verdict | Reason |
|---|---|---|
| `edit` three-tier matching | supported | `edit.ts` `matchView`: exact, then Unicode-normalised, then trailing-whitespace lines; no indentation tier |
| Failed edit "closest lines" hint | supported | read permission gated with `permission.allows`, as claimed (`edit.ts` L322-L346) |
| No staleness check on edit | supported | `edit.ts` has no mtime, hash or read-first check (searched `tool/plugin/*.ts`, `file-write.ts`, `file-access.ts`, `file-mutation.ts`); lock is process-local per `file-mutation.ts` L68-L83 and the TODOs are at L133-L141 |
| `patch` for GPT-family only | supported | `patch.ts` L288-L301: `gpt-`, not `oss`, not `gpt-4` |
| Compaction trigger and ceiling | supported | `compaction.ts` `calculateCeiling`: `RESERVE_SHARE` 0.15, `RESERVE_MIN` 16,000 only when window >= 32,000; unknown window gives infinity |
| Staged fallbacks, five levels | supported | `deliver` and `flattenAndDropOldest`, `TRANSCRIPT_LEVELS`; clip is 2,000 characters total (1,000 head, 1,000 tail); newest exchange never omitted |
| Overflow shrink 85/70/50/35 | supported | `SHRINK_STEPS` and the `deliver` loop |
| No pruning of old tool output; limits | supported | `tool-output.ts` 2,000 lines, 50 KiB, 7 days; `boundImages` 25 MiB trigger, 15 MiB target |
| Agent as data, default rules | supported | `schema/src/agent.ts`, `agent.ts` `selectedDefault` |
| Built-in roster, `explore` shell allowed | supported | `plugin/agent.ts` L55-L118 |
| `summary` agent has no consumer | supported (absence within searched dirs) | `ID.make("summary")` and `agent: "summary"` appear only at the definition in core, server, cli, tui |
| Plan mode does not deny shell | supported | `plan.ts` L33-L53 |
| `subagent` tool, inherited session permissions | supported | `session.ts` L311-L314; this agrees with the corrected safety file and contradicts the original safety text |
| Depth limit 1, no concurrency cap | partly checked | depth check at `subagent.ts` L123-L139 supported; no cap found in `job.ts` either (only a completed-job list limit of 25), consistent with the author's inference |
| Small model families | supported | `model.ts` L307-L318 |
| Retry schedule | supported | `model-retry.ts`: 10 retries, 2 s to 10 s, about 84 s; unreachable codes, 1 hour, 30 s gap; 3 timeout retries; retry-after capped at 15 minutes |
| Step limit with `toolChoice: none` | supported | `llm.ts` L259-L281, `step.ts` L116-L118 |
| Stuck-loop detection absent, goal limits | supported | `goal.ts` `MAX_ROUNDS` 50, `MAX_STALLS` 3, wait limit 30 minutes |
| Per-model prompt overlays | supported | `optimize.ts` substring rules for gpt, claude (append), kimi, trinity, muse |

Overall judgement: the checked findings, which include the plan, explore, step-limit, edit, patch, small-model and subagent-inheritance corrections and the numeric claims in the compaction, retry and output-bounding findings, were accurate. The unchecked remainder is mostly descriptive (instruction sources, AGENTS.md discovery, warming, title generation, native compaction, hidden-agent prompts) and has the same author care; trust it as a good guide but re-read the code before copying a specific number.

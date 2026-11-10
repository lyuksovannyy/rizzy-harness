# OpenClaw, cluster "agents": source-code findings

## 1. Repository

- Repository: `openclaw/openclaw`, branch `main`, pinned commit `8bac830dcb8ff99055ba71558dd369a989cc7881`, commit date 2026-11-10T20:27:14Z (from `research/round2/00-repos.tsv`). Clone at `.claude/research-repos/openclaw`, read only.
- Language: TypeScript on Node. No code was run, built or imported.
- What I read: `packages/agent-core/src` (the loop and its steering queue), `src/agents/**` (system prompt, compaction hooks, embedded runner, sub-agents, model fallback, auth profiles, tool-loop detection, edit and patch tools), `src/auto-reply/reply/**` (the inbound queue), `src/channels/message/ingress-queue*.ts`, `src/config/sessions/**` (the writer claim and pending inputs), `src/routing/resolve-route.ts`, `src/skills/**`, `extensions/memory-core`, `extensions/skill-workshop`, `extensions/anthropic/config-defaults.ts`. The repository is about 1 GB and `src/agents` alone has more than 2,000 files; this report covers the paths named above, not the whole tree. Round 1 file: `research/round1/openclaw.md`.

## 2. Architecture as it bears on this cluster

- **The loop is OpenClaw's own code, adapted from Pi.** The agent loop lives in the private workspace package `packages/agent-core` (`agent-loop.ts`, `agent.ts`, `pending-message-queue.ts`). `THIRD_PARTY_NOTICES.md` says "Portions of OpenClaw were adapted from Pi / pi-mono" (MIT, Mario Zechner); only `@earendil-works/pi-tui` is a runtime dependency. Round 1 said "page does not name an underlying library": the code answers it.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/THIRD_PARTY_NOTICES.md#L18-L26 : the attribution and the licence.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L167-L190 : `runLoop`, the inner loop, which takes a steering getter and a follow-up getter in its config.
- **Embedded runner on top of it.** `src/agents/embedded-agent-runner/` wraps the core loop with preparation, tools, compaction, overflow recovery and model fallback; `src/agents/sessions/` holds the `AgentSession` (steer, follow-up, auto-compaction). Plugin "harnesses" (Codex app-server, others) are alternatives selected by `harness/selection-decision.ts`; the built-in OpenClaw harness is not in the plugin candidate list.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/harness/selection-decision.ts#L102-L123 : an explicit runtime of `openclaw` returns the built-in harness; an explicit plugin runtime is looked up and "fails closed" unless the plugin declares OpenClaw as a lossless fallback.
- **Process model for this cluster.** One gateway process owns sessions, the loop and the tools (there is no separate "master and worker" for the embedded runtime). State is SQLite (per-agent session store, shared state DB). Concurrency is by named lanes with widths (`main`, `subagent`, `cron`).
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server-lanes.ts#L25-L36 : lane widths: `main = resolveAgentMaxConcurrent`, `subagent = resolveSubagentMaxConcurrent`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/agent-limits.ts#L5-L24 : defaults: main lane `max(8, cpus*4)`, sub-agent lane 8, children per agent 5, spawn depth 5.

## 3. Findings

### System prompt assembly and tool descriptions

#### The system prompt is built in code, per run, with a byte-stable cached prefix
- What the code does: `buildAgentSystemPrompt` composes the prompt from string builders (no template files). The part before `SYSTEM_PROMPT_CACHE_BOUNDARY` (identity line, execution-bias and "promised work" sections, runtime context, directory roles, workspace files as "Project Context") is built under a cache keyed by `sha256` of the inputs that may change it, held in a 64-entry process map. Everything channel-, tool-, skill- and time-dependent is appended after the boundary so the prefix stays identical across sessions. A trailing relocatable "Runtime" line can move behind tools.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt.ts#L570-L611 : cache key, `cacheStablePromptPrefix`, the boundary marker.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt.ts#L100-L121 : the 64-entry LRU for stable prefixes.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt.ts#L866-L885 : provider dynamic suffix, "Runtime" section, relocatable boundary.
- Edge cases and failure handling: three modes. `none` returns a one-line prompt ("You are a personal assistant running inside OpenClaw.") and no skills; `minimal` is used for sub-agent and cron sessions and also whenever the tool allowlist is restricted; `full` otherwise.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt.ts#L346-L354 : `none` early return.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-prompt-helpers.ts#L105-L107 : sub-agent and cron keys get `minimal`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-system-prompt-prepare.ts#L250-L255 : `none` for raw model runs; restricted `toolsAllow` forces `minimal`.
- Round 1: `confirmed` (modes `full`, `minimal`, `none`; stable content above a cache boundary). New detail: the per-turn date/time claim was not traced.
- For rizzy-harness: copy the idea of a hashed, cached stable prefix with everything volatile below a marker; it is what makes provider prompt caching hold across sessions.
- Confidence: read the definition.

#### Workspace files are injected as "Project Context" with per-file and total caps, and AGENTS.md keeps a policy digest when cut
- What the code does: the injectable set is exactly `AGENTS.md, SOUL.md, IDENTITY.md, USER.md, BOOTSTRAP.md, MEMORY.md`, in that prompt order. Defaults: 20,000 characters per file, 60,000 total, `USER.md` capped at 4,000. A cut file keeps 75% head and 25% tail. `AGENTS.md` is cut differently: 45% head, 15% tail, and 35% of the budget is a "Policy digest" of lines matching words like must, never, do not, security, secret, credential, test, commit, push, PR (and some CJK prohibition words), so rules deep in the file survive truncation.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace-bootstrap-policy.ts#L30-L37 : the six file names.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap-limits.ts#L1-L3 : 20,000 per file.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap.ts#L87-L101 : 60,000 total, 4,000 for `USER.md`, 75/25 ratios, AGENTS digest ratios and patterns.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap.ts#L207-L262 : `trimAgentsBootstrapContent`: head, "[truncated, read AGENTS.md for full content]", policy digest, tail, then shrink tail, head, digest in that order until it fits.
- Edge cases and failure handling: sub-agent sessions get only `AGENTS.md`; cron sessions get `AGENTS, SOUL, IDENTITY, USER`; `MEMORY.md` is dropped for sub-agent, cron, group and channel chats (a privacy filter), so long-term memory does not leak into shared rooms.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace.ts#L859-L866 : the two allowlists.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace.ts#L896-L917 : `filterBootstrapFilesForSession` including the MEMORY.md filter.
- Round 1: `corrected`. Round 1 listed `TOOLS.md` and `HEARTBEAT.md` as injected and flagged the conflict; the code's list has neither (`DEFAULT_TOOLS_FILENAME` exists but is not in `WORKSPACE_BOOTSTRAP_FILENAMES`). The caps (20,000, 60,000, 4,000) are confirmed. The AGENTS policy digest is not in Round 1.
- For rizzy-harness: a head-plus-tail cut on an instruction file silently loses the middle where the "never" rules often are; the digest idea is cheap and worth copying. The memory privacy filter (no long-term memory in group or sub-agent prompts) is a rule to adopt.
- Confidence: traced end to end for the file list, caps, trimming and filters. (checked in review)

#### Tool descriptions: one-line summaries in the prompt, schemas elsewhere, policy-removed tools are not shown
- What the code does: the prompt's Tooling section is built from a hard-coded map of one-line summaries ("Read files", "Exact file edits", "Spawn subagent; clean context: context=\"isolated\"; transcript: context=\"fork\"", ...), restricted to the tools actually available in this run. A "Deferred Tool Schemas" directory is appended when tool search or code mode defers schemas. The full JSON schema and long description of each tool travel in the provider's tool list, not in prose.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt-tool-list.ts#L20-L72 : the summary map.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt-tool-list.ts#L118-L124 : deferred schema directory.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt.ts#L361-L373 : `visibleTools` and `availableTools` are built from the tools passed for this run.
- Round 1: `confirmed` (tools removed by policy are not sent to the model; Tool Search exists). The `edit` tool's own description is a single sentence (see the edit finding).
- For rizzy-harness: keep per-tool prose short and in the schema; keep the prompt's tool list derived from the final authorised set so the model never sees tools it cannot call.
- Confidence: read the definition.

#### Skills appear as a bounded catalog; bodies are read on demand
- What the code does: the Skills section says "Scan <available_skills> for a matching workflow", "Several: most specific", "Up-front max one", and tells the model to read the chosen skill (via `skills_read` or the `read` tool on the listed location). The catalog is capped at 150 skills and 18,000 characters, sorted by name unless order is preserved, and falls back to a compact form with shorter descriptions to fit.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt-skills.ts#L40-L66 : the instruction text.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/skill-prompt-limits.ts#L10-L40 : the two caps and the sort.
- Edge cases and failure handling: when the model has no `read` or `skills_read` tool the section is omitted entirely (`canAccess`).
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/system-prompt-skills.ts#L17-L23 : `canAccess`.
- Round 1: `confirmed` (compact list, body on demand, `skills_search`/`skills_read`). Caps 150 and 18,000 are new.
- For rizzy-harness: cap the catalog by count and by characters and degrade to names plus short descriptions rather than dropping skills.
- Confidence: read the definition. (checked in review)

### Skills loading and gating

#### Six source tiers, low to high, with a project `.agents/skills` below the workspace `skills`
- What the code does: roots are assembled low to high: `extra` (config `skills.load.extraDirs` plus plugin roots), `bundled`, `workshop` (per-agent learned skills dir declared by a plugin), `managed` (`~/.openclaw/skills`), `personal` (`~/.agents/skills`, only when the state dir is the default), then `workspace` (`<workspace>/.agents/skills`, then `<workspace>/skills`). Higher tiers win name collisions. Discovery never walks ancestors into another repository.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/workspace-skill-sources.ts#L134-L186 : the tier order as pushed.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/workspace-skill-roots.ts#L57-L70 : workspace directories and the no-ancestor-walk rule.
- Round 1: `confirmed` (order workspace, project `.agents/skills`, personal, managed, workshop, bundled, extra is the same list read top-down). The `personal` tier is conditional on the default state dir, which Round 1 did not say.
- For rizzy-harness: an explicit tier enum with a documented precedence beats ad hoc directory scanning.
- Confidence: read the definition.

#### Gating: enabled flag, OS, required binaries, env and config paths, evaluated at snapshot time
- What the code does: `shouldIncludeSkill` rejects a skill when its config says `enabled: false`, when its secret owner is unavailable, when it is a bundled skill outside an allowlist, or when `evaluateRuntimeEligibility` fails on `metadata.os`, `always`, `requires.bins`/`anyBins`, required env, or required config paths. Binary presence is probed once per snapshot for only the binaries that selected skills ask for; for a remote node it uses the remote's reported bins.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/config.ts#L105-L141 : `shouldIncludeSkill`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/config.ts#L143-L192 : `prepareSkillBinaryProbe` collects only needed binaries.
- Round 1: `confirmed` (`metadata.openclaw` `requires.bins/env/config`, `os`).
- For rizzy-harness: probing only the binaries that skills require avoids a PATH scan per turn.
- Confidence: read the definition.

#### The "workshop" tier is the Skill Workshop plugin: agents write skills themselves, default on, no human approval
- What the code does: the bundled `skill-workshop` plugin is `enabledByDefault`, declares `agentSkillsDir: "workshop-skills"` (the workshop tier) and a `skill_workshop` tool. Its autonomous mode defaults to `auto`: after a session accumulates 10 model iterations and goes idle for 30 s, a background review may save or update skills; learned skills unused for 30 days are archived. Each change is versioned and undoable; `propose` instead saves drafts; `off` disables. Reviews are blocked for sessions whose key contains `acp`, `cron`, `hook` or `subagent`.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/skill-workshop/openclaw.plugin.json#L6-L24 : `enabledByDefault`, `agentSkillsDir`, help text for the three modes and the 30-day archive.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/skill-workshop/src/config.ts#L26-L32 : `mode` defaults to `"auto"`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/skill-workshop/src/experience-review-scheduler.ts#L16-L26 : iteration threshold 10, idle 30 s, blocked session segments.
- Edge cases and failure handling: the 30-day archive and the undo are stated in the manifest help text; I did not trace the archive job.
- Round 1: `new (not documented)`. Round 1 asked what the workshop tier is and who writes it (question 10): the model writes it, by default, in the background, and a person reviews only after the fact.
- For rizzy-harness: agent-written skills are the same class of risk as agent-written memory. If we copy it, make `propose` the default and keep every write versioned.
- Confidence: read the definition (manifest and scheduler constants); the review prompt and writer were not read. (checked in review)

### Edit tools and formats

#### `edit`: exact match first, then a lookup-only fuzzy match, uniqueness required, read at edit time
- What the code does: the tool takes `path` and an array of `{oldText, newText}`. All texts are normalised to LF. Each `oldText` is searched exactly; on a miss, both file and `oldText` are normalised (NFKC, trailing whitespace trimmed per line, curly quotes, dashes and unusual spaces folded) and searched again. The match must be unique (error: "Found N occurrences... provide more context") and edits must not overlap. Fuzzy matching only locates the span; the replacement is spliced into the original bytes, and an ambiguous Unicode boundary is an error. Line endings and a BOM are restored.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/edit-diff.ts#L167-L183 : the folding and normalisation functions.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/edit-diff.ts#L418-L514 : `applyEdits`: exact, fuzzy, uniqueness, boundary mapping, overlap check.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/edit-diff.ts#L516-L565 : `prepareFileEdit`: no-op detection, LF restoration, diff receipt.
- Edge cases and failure handling: on a miss the error carries up to three similar-line candidates (score at least 0.45) and the current file contents (bounded) so the model can retry without another read. There is no "file changed since you read it" check; the file is read fresh inside the per-file mutation queue, edited, written, and then re-read to verify that the persisted bytes equal the intended content. If the write throws but the content verifies, the call is reported as success.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/edit-diff.ts#L287-L291 : candidate limits and minimum score.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/edit.ts#L117-L126 : mismatch hint appends current contents.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/edit.ts#L150-L234 : queue, read, plan, write, verify, recovery.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/tools/file-mutation-queue.ts#L1-L12 : per-real-file serialisation, parallel across files.
- Round 1: `new (not documented)`. Round 1 listed the `edit` mechanism as undocumented.
- For rizzy-harness: copy the unique-match rule, the fuzzy-as-lookup-only rule and the post-write verification; consider adding a content-hash precondition from the last read, which OpenClaw lacks (a concurrent editor between read and write inside one call is the only window it closes).
- Confidence: traced end to end.

#### `apply_patch` uses the OpenAI-style envelope and defaults to workspace-only
- What the code does: the patch text must start with `*** Begin Patch` and end with `*** End Patch`; hunks are `*** Add File:`, `*** Delete File:`, `*** Update File:` (with `@@` context markers). Application goes through guarded file operations (alias-safe paths, per-file queue). `workspaceOnly` defaults to true when sandboxed; the session permission policy sets it for every mode except `full`.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/apply-patch-parse.ts#L8-L16 : the markers.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/apply-patch.ts#L98-L102 : `workspaceOnly` default.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L55-L66 : `applyPatchWorkspaceOnly: workspaceOnly` where `workspaceOnly = mode !== "full"`.
- Round 1: `new (not documented)`.
- For rizzy-harness: a second edit format exists for models trained on the patch envelope; we need only one unless we target those models.
- Confidence: read the definition.

### Compaction and context pruning

#### Auto-compaction trigger: provider-reported usage after each response, threshold = min(window - reserve, 75% of window)
- What the code does: after each assistant message, `AgentSession` takes the provider's usage as the context size (or an estimate from the last successful usage when the response was an error) and calls `shouldCompact`. `shouldCompact` is true when tokens exceed `min(contextWindow - reserveTokens, floor(contextWindow * 0.75))`. The effective reserve is `max(configured, 20,000)` capped at 25% of the window, so small models do not compact at token one. A separate overflow trigger fires when the provider rejects the prompt.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction-threshold.ts#L1-L14 : the threshold formula.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction.ts#L208-L222 : `shouldCompact`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/agent-session-compaction.ts#L638-L695 : overflow path (retry after compaction) and threshold path (no retry).
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-settings.ts#L12-L56 : reserve floor 20,000 and the cap.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-compaction-constants.ts#L1-L16 : the 25% ratio and the 3-attempt overflow cap.
- Edge cases and failure handling: `keepRecentTokens` defaults to 20,000 and the session setting `reserveTokens` to 16,384 before the 20,000 floor applies. Compaction can be switched off in settings.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/settings-manager.ts#L127-L141 : `enabled` default true, 16,384 and 20,000 defaults.
- Round 1: `confirmed` ("smaller of 75% of window or window minus reserve", `keepRecentTokens` 20k). The 20,000 reserve floor with a 25% cap is new.
- For rizzy-harness: using the provider's reported usage (not a local estimate) as the trigger removes a class of tokenizer mismatch; keep a local estimate only for the error case.
- Confidence: traced end to end. (checked in review)

#### The summary is produced by the session's own model by default; a different model is allowed unless the session is model-locked
- What the code does: summarisation is one model request (retried up to 3 times with 500 ms to 5 s backoff, never on caller abort, timeout or refusal). Input has tool-result `details` and runtime-context entries stripped before it reaches the model ("SECURITY" comment). Output budget is 0.8 of the reserve (0.5 for a split-turn prefix). The prompts are structured checkpoint templates: Goal, Constraints, Progress (Done, In Progress, Blocked), Key Decisions, Next Steps, Critical Context, plus an "update" variant that must preserve everything in `<previous-summary>`. A system prompt tells the summariser to attribute facts by `sender={...}` and not to continue the conversation. `agents.defaults.compaction.model` overrides the model, but not for a session whose model selection is locked (a locked session's creating model owns every transcript read).
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/compaction.ts#L70-L125 : retry, abort rules, sanitisation.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction.ts#L225-L296 : the two prompt templates and the output cap.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/summarization-prompts.ts#L1-L12 : the summariser system prompt.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/compaction-runtime-context.ts#L100-L135 : model override and the lock rule.
- Edge cases and failure handling: when `compaction.model` is unset and fallbacks are configured, compaction itself is run through the model-fallback chain.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/compact.ts#L424-L440 : the condition choosing a direct run versus the fallback-wrapped run.
- Round 1: `confirmed` (separate summarizer `model`, ~40k input cap not re-verified, `maxSummaryChars` not re-verified). The "a lock blocks the override" rule is new.
- For rizzy-harness: stripping tool-result `details` and runtime context before summarising is a cheap injection and size defence worth copying.
- Confidence: read the definition; the 40,000-token input cap was not located.

#### `safeguard` is the default mode; its quality guard is on by default in code although the schema comment says off
- What the code does: if no compaction mode is set the config default is `safeguard`. The safeguard hook runs before compaction: it keeps the last 3 user and assistant turns verbatim (max 12, 600 characters per message, 160 for tool results), generates the summary with required headings (`## Decisions`, `## Open TODOs`, `## Constraints/Rules`, `## Pending user asks`, `## Exact identifiers`), audits it, and regenerates up to 1 retry (max 3) when the audit fails; if still bad it writes a structured fallback and marks it degraded. With no real conversation to summarise it writes a boundary summary instead of cancelling, so compaction does not re-trigger in a loop.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/defaults.ts#L478-L495 : default mode `safeguard`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/extensions.ts#L28-L47 : hook installed with `qualityGuardEnabled: qualityGuardCfg?.enabled ?? true`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-hooks/compaction-safeguard-quality.ts#L19-L42 : required sections and retry bounds.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-hooks/compaction-safeguard.ts#L79-L82 : recent-turn preservation constants.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-hooks/compaction-safeguard.ts#L495-L510 : the no-real-conversation boundary.
- Edge cases and failure handling: documentation and code disagree: the Zod comment says the guard is off by default, the help text and the wiring say on.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/zod-schema.agent-defaults-base.ts#L157-L163 : "Default: false".
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/schema.help.agents.ts#L163-L166 : "Safeguard mode enables this by default".
- Round 1: `confirmed` (default `safeguard`, audits structure, pending requests and identifiers, degraded marking). New: the audit costs an extra model call per retry, and the schema comment is wrong.
- For rizzy-harness: the "boundary summary when there is nothing to summarise" rule prevents a compaction loop that a naive trigger will hit; copy it.
- Confidence: read the definition; the audit body was not traced past its constants. (checked in review)

#### Context-overflow recovery: up to 3 compactions, then one tool-result truncation pass, else a typed user error
- What the code does: on a provider overflow error (or a pre-flight budget prediction), recovery first refuses when fixed context (system prompt plus tools) alone exceeds the window or the pending message alone does; stops without compacting when the error is a provider request-size ceiling; then compacts (max 3 attempts); then, once per run, truncates oversized tool results in the transcript; otherwise surfaces a message that distinguishes "compaction failed or timed out" from "overflow".
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L149-L173 : the two "cannot fit by compaction" exits.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L245-L257 : provider size ceiling means no compaction.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L279-L296 : compaction attempts bounded by `MAX_OVERFLOW_COMPACTION_ATTEMPTS`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L406-L433 : the one-time tool-result truncation.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L435-L458 : final classification and user text.
- Edge cases and failure handling: a parked code-mode run cannot follow a session rotated by compaction, so the mid-turn continuation is withheld (lines 259-263, 300-311 of the same file).
- Round 1: `confirmed` ("on overflow error, compact and retry"). New: the order, the size-ceiling exit and the two pre-exits.
- For rizzy-harness: a recovery ladder with explicit "this cannot fit, do not loop" exits is the correct shape; copy the pre-exits.
- Confidence: traced end to end for the ladder.

#### Memory flush before compaction exists: a silent, append-only turn near the threshold
- What the code does: before a user turn, the runner computes `flushThreshold = compactionThreshold - softThresholdTokens` (soft default 4,000, halved-window-capped). If projected tokens exceed it, or the transcript file is 2 MiB or larger, and no flush has happened for the current compaction, it runs a hidden turn with trigger `memory`. That turn's prompt and system prompt tell the model to store durable notes only in `memory/YYYY-MM-DD.md`, treat `MEMORY.md`, `DREAMS.md`, `SOUL.md`, `AGENTS.md` as read-only, never create timestamped variants, and reply with the silent token if nothing is worth saving. The `write` tool in that run is wrapped so it can only append to that one path.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/flush-plan.ts#L13-L38 : defaults and both prompts.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/flush-plan.ts#L48-L91 : the plan, including capping the soft threshold.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/agent-runner-memory.ts#L580-L650 : once-per-compaction guard, threshold, 2 MiB force rule, decision.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.read.ts#L601-L640 : the append-only write wrapper restricted to one path.
- Edge cases and failure handling: if tool policy removes `write` the flush logs a warning and the model may still report success, so memory is lost without a record; a missing provider persistence tool is a pre-inference skip.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.memory-flush.ts#L30-L57 : the warning and its reasoning.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.memory-flush.ts#L58-L66 : `MemoryFlushToolsUnavailableError`.
- Round 1: `confirmed` ("a silent pre-compaction turn saves notes"). New: it is triggered by tokens or by transcript size, not by compaction itself, and has a hard path restriction.
- For rizzy-harness: the append-only, single-path tool wrapper is the safe way to let a hidden turn write; the silent-failure window is the thing to avoid.
- Confidence: traced end to end.

#### Pruning is a stable projection with a cache TTL, soft trim then hard clear, off by default except for Anthropic auth
- What the code does: `contextPruning.mode: "cache-ttl"` installs a context transform that, once the provider cache TTL (default 5 minutes, parsed from `ttl`) has passed since the last cache touch, projects old tool results (never within the last three assistant turns or before the first user message): results over 4,000 characters keep the first and last 1,500 with a notice; if estimated usage is at least 50% of the window (4 characters per token) and at least 50,000 eligible characters exist, whole results are replaced by a placeholder until usage drops below 50%. Projections are frozen, replayed byte-identically, and recorded in a hidden transcript marker so a restart rebuilds the same prefix. Direct Anthropic API-key calls skip client-side rounds and use server-side clearing. The Anthropic provider sets the mode to `cache-ttl` with `ttl: "1h"` only when the user left the mode unset and Anthropic auth is detected.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/tool-result-truncation.ts#L62-L89 : settings: 5 minute default, hard clear default on, placeholder.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/tool-result-truncation.ts#L152-L173 : soft trim, 4,000 threshold, 1,500 and 1,500.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/tool-result-truncation.ts#L214-L281 : TTL gate, last-three-assistant cutoff, 30% and 50% window rules, 50,000 hard-clear floor, frozen projection.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-setup.ts#L288-L330 : wiring; `pruneNewRounds` false under server-side clearing.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/anthropic/config-defaults.ts#L205-L222 : the Anthropic default seeding.
- Edge cases and failure handling: pruning mutates only the prompt projection; transcript entries are not rewritten. Below 30% of the window nothing is pruned at all.
- Round 1: `confirmed` (soft trim 1500/1500 of over 4000, hard clear near 50%, protects last three assistant turns, Anthropic seeds 1h). Note the "hard clear" also needs 50,000 eligible characters, not stated in Round 1.
- For rizzy-harness: pruning that is frozen and recorded is the only kind that preserves a provider cache prefix; if we prune at all, store the projection.
- Confidence: read the definition and the wiring.

#### Live tool-result size caps scale with the context window
- What the code does: a single tool result is capped at `min(30% of window in tokens x 4 characters, hard cap)` where the hard cap is 16,000 characters below a 100,000-token window, 32,000 from 100,000, 64,000 from 200,000. Oversized results are truncated with a notice that tells the model to narrow arguments.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-result-limits.ts#L6-L40 : the caps.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/tool-result-truncation.ts#L40-L50 : the compact "narrow args" suffix.
- Round 1: `new (not documented)`.
- For rizzy-harness: scale per-result caps with the window instead of a fixed constant.
- Confidence: read the definition. (checked in review)

### Memory and instruction files

#### Memory search is hybrid (vector 0.7, keyword 0.3) with MMR and temporal decay; the weights are hard-coded
- What the code does: the `memory_search` configuration builder always sets hybrid retrieval with vector weight 0.7, text weight 0.3, a 4x candidate multiplier, MMR re-ranking with lambda 0.7, and temporal decay with a 30-day half-life. Only `maxResults`, `minScore`, sources and extra paths come from config; the hybrid block is not read from config.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/memory-search.ts#L145-L173 : the constants.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/memory/hybrid.ts#L52-L80 : keyword scores are rescaled against the best vector score so the text weight does not only reward window membership.
- Round 1: `new (not documented)` (Round 1 said "hybrid vector plus keyword", SQLite default).
- For rizzy-harness: rank fusion with decay and MMR is a known good default for a memory folder; hard-coding is fine until users ask.
- Confidence: read the definition.

#### Dreaming: a nightly job promotes recalled snippets into `MEMORY.md` through a model, with no human step
- What the code does: a cron job (default `0 3 * * *`) scores short-term memory entries. An entry is a candidate only if it was recalled at least 3 times, by at least 3 distinct user queries, and its weighted score is at least 0.75. The score is `0.24*frequency + 0.30*relevance + 0.15*diversity + 0.15*recency + 0.10*consolidation + 0.06*conceptual` plus a phase boost; recency has a 14-day half-life. Selected candidates go to a consolidation sub-agent that returns add, merge or supersede operations; the result is validated, and a plan that would remove more than 25% of the prior entries is rejected, in which case entries are appended instead. `MEMORY.md` has a 10,000-character budget; the oldest generated sections are dropped to fit, also within the 25% loss bound.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/memory-host-sdk/dreaming.ts#L22-L42 : cron default, 0.75, 3, 3, 14 days, 160-token snippet cap, 0.25 loss fraction.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/short-term-promotion.ts#L28-L40 : the six weights.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/short-term-promotion.ts#L100-L160 : candidate filters and score.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/dreaming-consolidation.ts#L387-L460 : sub-agent call, per-project grouping, "append-only fallback" on missing output, rejection or error.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/dreaming-consolidation.ts#L275-L295 : the 25% guard counts removed prior entries over current entries.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/memory-budget.ts#L17-L17 : `DEFAULT_MEMORY_FILE_MAX_CHARS = 10_000`.
- Edge cases and failure handling: the guard is on prior-entry removal only; added entries are unbounded except by the 10,000-character budget and its compaction. Synthetic recall keys cannot satisfy the "distinct queries" rule, only interactive user queries (`userQueryHashes`).
- Round 1: `confirmed` for the 25% guard and the default 03:00 schedule. The thresholds Round 1 could not find are now known: `minScore` 0.75, `minRecallCount` 3, `minUniqueQueries` 3 (question 9). Round 1's "no human step" is confirmed.
- For rizzy-harness: promotion by repeated, diverse recall is a defensible filter against one-off noise; if we let a model rewrite memory, count removed entries and refuse above a bound, as here.
- Confidence: traced end to end for thresholds and guard; the consolidation prompt text was not read. (checked in review)

#### Who writes memory: the model's `write` tool, the flush turn, the dreaming job; nothing asks a person
- What the code does: `MEMORY.md` and `memory/*.md` are ordinary workspace files, so the model can edit them with `write` or `edit` in any full run. The flush turn is restricted to appending to today's file; dreaming writes `MEMORY.md` through the locked, budgeted path. There is a privacy filter on reads (no `MEMORY.md` in group, channel, cron or sub-agent prompts) but no approval step on writes.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace.ts#L896-L917 : the read-side filter.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/short-term-promotion-apply.ts#L380-L400 : the locked write with consolidation.
- Round 1: `confirmed` ("project says memory guides behaviour but enforces no policy"; no human step in dreaming).
- For rizzy-harness: decide explicitly whether memory writes need approval; OpenClaw's answer is no, with a read-side privacy filter as the only guard.
- Confidence: read the definition.

### Sub-agents and orchestration

#### Spawn path: one function creates a child session, then launches the child through the gateway `agent` call
- What the code does: `spawnSubagentDirect` resolves the request, checks admission (depth, children), plans the child (cwd, workspace, model, sandbox), creates an initial child session, optionally forks the parent transcript, optionally binds a thread, builds a system prompt and task message, materialises attachments, and launches the child run through the gateway. A requested `worktree` is handed to session creation. Failures after session creation clean the provisional session up.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn.ts#L73-L122 : entry and resolved request.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn.ts#L156-L275 : child plan, initial session, context preparation.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn.ts#L311-L337 : completion mode and envelope.
- Edge cases and failure handling: a structured-output schema appends an instruction to call `structured_output` until one payload is accepted, with at most one retry.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn.ts#L70-L71 : that instruction.
- Round 1: `confirmed` (spawned via `sessions_spawn`, own session, announces back, `context: "fork"`).
- For rizzy-harness: spawning is a session creation plus a normal run, not a special runtime; that keeps the child's events in the same log shape.
- Confidence: read the definition (about 650 lines of the 715-line function were skimmed, not every helper).

#### Caps: depth 5, active children 5 per spawner, sub-agent lane width 8, all enforced at admission
- What the code does: admission rejects a spawn when `callerDepth >= maxSpawnDepth` (default 5), when active children for the requesting session reach `maxChildrenPerAgent` (default 5), or, in collector mode, when group limits are reached. The child session is stamped with `spawnDepth`, a `subagentRole` (`orchestrator` or `leaf`) and a control scope; a child at the last allowed depth is a leaf with no spawn tools. Separately, each spawning session gets its own `subagent:<owner>` command lane of width 8 (corrected in review; the original said a gateway-wide lane).
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/agent-limits.ts#L19-L31 : the defaults and `isSubagentSpawnDepthAllowed`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/child-admission.ts#L84-L109 : the rejections.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/spawn-plan.ts#L296-L350 : depth lookup, caps, child session patch.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server-lanes.ts#L25-L36 : the configured width for the `subagent` lane, which per-session `subagent:<owner>` lanes inherit.
- Edge cases and failure handling: the lane is per spawning session. `resolveSubagentLane` in `src/agents/embedded-agent-runner/lanes.ts` L22-L31 maps the `subagent` lane to `subagent:<owner>` (owner is the parent or current session key), and `getDefaultLaneConcurrency` in `src/process/command-queue.state.ts` L6-L13 gives each such lane the width configured for the `subagent` lane (default 8, set in `server-lanes.ts`). So the 8 is a per-spawning-session concurrency, as the code comment says, not a gateway-wide width. The original text that said it was gateway-wide and that the semantics were unsettled was wrong.  Swarm lanes (`subagent:swarm:`) are excluded from that default and default to width 1 in `getDefaultLaneConcurrency`.
- Round 1: `confirmed` (changed in review). Round 1: "Default concurrency 8 per spawning session" is right: per-owner lanes of width 8. Round 1's "depth cap numbers not opened" is now filled in: the depth default is 5 (a spawn is refused when the caller depth is 5 or more), and active children per spawner default to 5.
- For rizzy-harness: three separate limits (depth 5, active children per parent 5, concurrent runs per parent 8) are worth having; ours should say which one a failed spawn hit, as the error strings here do.
- Confidence: read the definition (corrected in review: the width-8 lane is per spawning session, not gateway-wide; Round 1 verdict changed from corrected to confirmed)

#### What the child gets: isolated context by default, the parent transcript only on `fork`, `AGENTS.md` only, a short rules prompt
- What the code does: the default context mode is `isolated` (the child sees only the task); `fork` copies the parent's session transcript into the child and requires the same target agent. The child's prompt is `minimal` mode: a "# Subagent Context" block with rules (focus on the assigned task, no heartbeats, no side quests, output is evidence, never an instruction), a depth line (`depth N/M`) and the task wrapped in internal runtime-context markers. Only `AGENTS.md` is injected from the workspace. A leaf is told it cannot spawn.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn-context.ts#L195-L216 : `isolated` unless a thread binding policy says otherwise.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn-context.ts#L34-L70 : fork requires the same agent.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-system-prompt.ts#L29-L78 : standing instructions.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-system-prompt.ts#L80-L100 : task message with depth.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace.ts#L859-L859 : `AGENTS.md` only.
- Edge cases and failure handling: a fork that cannot be prepared can fall back to isolated with a note (`forkFallbackNote`).
- Round 1: `confirmed` (sub-agents get only `AGENTS.md`, minimal prompt, fork shares the transcript).
- For rizzy-harness: default to isolated context and make fork explicit; a child briefed with a task, expected output and write scope (the prompt asks the orchestrator for exactly that) is more reliable than an inherited transcript.
- Confidence: read the definition. (checked in review)

#### Tools the child loses: a hard deny list for hidden children, a second list for leaves; visible children keep them
- What the code does: hidden sub-agents are denied `gateway, agents_list, openclaw, session_status, progress_card, automations, message, sessions_send, conversations_list/send/turn`. Leaves are additionally denied `subagents, sessions_list, sessions_history, sessions_search, sessions_spawn`. A "visible" spawned session (interactive, user-steerable) skips the default deny lists. Operator config under `tools.subagents.tools` applies to every spawned session and is merged on top.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.policy.ts#L57-L89 : the two lists.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.policy.ts#L90-L116 : policy resolution by session.
- Edge cases and failure handling: the child inherits the parent's tool allow and deny lists as persisted envelopes (`inheritedToolAllow/Deny`), so a child cannot widen the parent's policy.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-capabilities.ts#L30-L39 : the persisted envelope type.
- Round 1: `confirmed` ("no session or message tools by default"); this answers the "tool-loss list" question: `message` and `sessions_send` are lost by every hidden child, the `sessions_*` family by leaves.
- For rizzy-harness: a child must not be able to talk to the user directly; route everything through the parent's completion path, as the deny list here does.
- Confidence: read the definition. (checked in review)

#### Child model: per-call override, then the agent's `subagents.model`, then the defaults, then the parent's model
- What the code does: resolution order is the call's `model`, then `agents.entries.<id>.subagents.model`, then `agents.defaults.subagents.model`, then (only if no parent model was inherited) the target agent's primary; otherwise the parent's resolved model is inherited; last resort is the default model. A trailing `@profile` pins an auth profile.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-selection-config.ts#L30-L46 : configured selection order.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-selection.ts#L174-L209 : override, inherited, default.
- Round 1: `confirmed` (`agents.defaults.subagents.model`); inheritance of the parent's model when unset was not in Round 1.
- For rizzy-harness: say plainly that an unset sub-agent model inherits the parent's.
- Confidence: read the definition.

#### Results come back as a steered completion event, or as a private turn on the requester; announce is retried with an idempotency key
- What the code does: a child's final result becomes an internal "task completion" event (source, child session key, status, result as data). If the requester has a running turn, the event is injected there as a steer (`steeringMode: "all"`, wait for the transcript commit, deadline `announceTimeoutMs` default 120 s). If the steer is rejected the code falls through to a direct gateway `agent` turn on the requester session with `inputProvenance` `inter_session` and the same idempotency key. Four completion modes exist: `announce` (event to requester), `collector` (no event; the requester collects by run id), `quiet` (nothing), `thread-direct` (final reply goes to a bound thread). A private completion is never steered into a public turn, because that turn's delivery policy would expose child output. The model is told to "use its result to reply in your normal assistant voice", the result is wrapped as data, and `sessions_yield` ends the parent's turn as a clean handoff so it waits for the event.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/announce/subagent-announce-direct-delivery.ts#L267-L335 : steer first; accepted, indeterminate (never replayed).
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/announce/subagent-announce-direct-delivery.ts#L377-L400 : private completion on its own serialised turn.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/internal-events.ts#L223-L238 : the instruction text and data wrapping.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-system-prompt.ts#L16-L27 : the four completion notes.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/announce/subagent-announce-delivery-retry.ts#L9-L20 : 120 s default.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-sessions-yield.ts#L32-L36 : yield as a handoff.
- Edge cases and failure handling: an `indeterminate` steer (input consumed, no confirmed commit) is returned as `ambiguous` and never replayed; permanent delivery errors (chat not found, bot blocked) are classified and not retried; a dead-lettered handoff is a permanent failure.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/announce/subagent-announce-delivery-retry.ts#L41-L52 : the permanent-error patterns.
- Round 1: `confirmed` (announce results to the requester, `sessions_yield`). The steer-then-direct fallback and the "never replay an ambiguous steer" rule are new.
- For rizzy-harness: for a parent that is mid-turn, deliver the child result at the next checkpoint; for an idle parent, start a turn with a stable idempotency key; mark child output as data.
- Confidence: traced end to end for the steer path; the retry scheduler was not traced.

### Multi-agent routing

#### Bindings are matched by tier, most specific first; an ambiguous unbound message throws
- What the code does: `resolveAgentRoute` tries, in order: exact peer, the parent peer (for threads), peer wildcard, guild plus roles, guild, team, account, channel. The first tier with a matching binding picks the agent. The session key is built from the agent, channel, account, peer and the `dmScope`/`groupScope` (optionally overridden per binding), with identity links joining one person across channels. If nothing matches, it uses the configured default or a legacy sole agent; with several agents and no default it throws `AgentSelectionRequiredError` with a hint to add a channel-wide binding.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/routing/resolve-route.ts#L578-L608 : the tier order and the default.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-scope-config.ts#L178-L188 : `resolveSoleAgentId` throws when ambiguous.
- Edge cases and failure handling: the thrown error is surfaced to the caller; the message is not silently routed to any agent.
- Round 1: `confirmed` ("unbound messages are not sent to an arbitrary agent"; routing by channel, account, peer, guild/team with specificity ordering).
- For rizzy-harness: a fixed tier list in one function is easy to test; make "no match and several agents" an error, not a default.
- Confidence: traced end to end.

#### Cross-agent access is open by default: session visibility `all` and agent-to-agent enabled
- What the code does: `tools.sessions.visibility` falls back to `"all"` when unset; `tools.agentToAgent.enabled` is true unless explicitly `false`, and an empty or missing allow list means unrestricted; a cross-agent `sessions_send` is refused only when the policy says so. Sandboxed sessions default to seeing only the spawned tree.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/session-visibility.ts#L132-L155 : visibility default `all`; sandbox clamp `spawned`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/session-visibility.ts#L209-L222 : policy enabled by default, empty list unrestricted.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tools/sessions-send-executor.ts#L148-L166 : the refusal strings.
- Round 1: `confirmed` ("cross-agent session access is on by default"; "default session visibility is `all`").
- For rizzy-harness: our rule that chats are private to their owner is the opposite of this default; do not copy it.
- Confidence: read the definition.

### Model selection and fail-over

#### Candidate chain: requested model, configured fallbacks, then the configured primary; an explicit (even empty) override replaces the list
- What the code does: the chain starts with the requested model, appends each configured fallback (not filtered by the allowlist, since fallbacks are explicit intent), and, when no `fallbacksOverride` is given, appends the configured primary as a last candidate. A defined `fallbacksOverride`, including `[]`, replaces the fallbacks and suppresses the primary append; that is how a "strict" selection (a user `/model` choice or an agent primary without fallbacks) fails instead of masking. Plugins can return an override from a model-resolve hook.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-candidates.ts#L364-L410 : requested, fallbacks, primary.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-runner.ts#L105-L110 : `fallbacksOverride` semantics ("even empty").
- Edge cases and failure handling: I did not trace which callers pass `[]` for a user selection; the docs say it (`docs/concepts/model-failover.md`, user session override section) and the mechanism is consistent with the code above.
- Round 1: `confirmed` in mechanism ("a user-chosen model is strict"); the call site was not traced.
- For rizzy-harness: "explicit empty list means no fallback" distinguishes "unset" from "none"; use an option type, not a default value.
- Confidence: read the definition; the strict-call-site not traced.

#### Auth profiles: ordering, per-failure cooldown, and what the cooldown is for each reason
- What the code does: within a provider, profiles are ordered by an explicit order if set (hard preference), otherwise by type score (OAuth 0, token 1, API key 2), OAuth expiry state, then least-recently-used (round robin within a tier); profiles in cooldown sort last by soonest expiry; a user-pinned or auto-pinned profile is moved to the front. Cooldown is computed per failure reason:
  - `rate_limit`: no local counter; only the provider's retry-after deadline, never shortened.
  - other transient reasons: error count 1 gives 30 s, 2 gives 1 min, 3 or more gives 5 min, counters reset after 24 h without failure or after the cooldown expired.
  - `billing`: exponential from 10 min doubling to a 24 h cap, in a separate `disabledUntil` window.
  - `auth_permanent`: 10 min doubling to a 1 h cap.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/order.ts#L190-L286 : the ordering.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/order.ts#L293-L312 : type, expiry and `lastUsed` sort.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L18-L46 : 30 s, 1 min, 5 min and the billing and auth_permanent policies.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L80-L108 : rate limits follow provider timing only.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L110-L183 : counters, window reset, which window is written.
- Edge cases and failure handling: cooldown state is persisted with the auth profile store (SQLite), so it survives restarts and is shared across sessions of the agent; retries inside an active window cannot push recovery later.
- Round 1: `corrected`. "billing failures disable a credential 10 minutes" is the first step only: the disable window doubles up to 24 hours. "Rate-limit cooldown follows provider headers" is confirmed and is the only rule for rate limits (no 30 s step). Also new: `auth_permanent` has its own 10 min to 1 h ladder.
- For rizzy-harness: separate "temporarily unavailable" from "disabled until a human fixes it" and key the backoff on the failure reason, not one counter; copy that.
- Confidence: read the definition. (checked in review)

#### The fallback loop: skip repeated auth failures, probe the primary during cooldown, suspend the session when nothing is usable
- What the code does: for each candidate, the runner resolves the profile order and checks whether any profile is out of cooldown. If none is, `resolveCooldownDecision` chooses: skip all models of that provider on `auth` or `auth_permanent`; attempt a probe of the primary (at most once per 30 s per provider and agent dir, and, when fallbacks exist, only within 2 minutes of the soonest cooldown expiry); permit a transient probe for fallbacks on rate limit, overloaded, timeout, empty response and similar; otherwise `suspend_session`. A fallback that failed with an auth error earlier in the same session is skipped with a re-auth hint. Observers of fallback steps cannot change the outcome (their exceptions are swallowed). Quota suspension resumes by default after 30 minutes.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-runner.ts#L157-L200 : chain, sticky, auth store, rate-limit ordering.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-runner.ts#L370-L440 : auth-failure skip within a session; cooldown decision.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-cooldown.ts#L8-L95 : probe throttle (30 s, 2 min margin) and the decision rules.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/failover-policy.ts#L1-L39 : which reasons count as transient.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-suspension.ts#L17-L17 : 30 minute default.
- Edge cases and failure handling: the probe throttle map is in process memory (max 256 keys); a restart resets it. Per-run attempt budgets are tracked separately in `run/retry-budget.ts`; per-attempt LLM retry lives in `src/llm/utils/retry.ts`, which I did not read.
- Round 1: `confirmed` (rotate profiles, then fallbacks; cooldown).
- For rizzy-harness: a half-open probe of the primary during its cooldown, rate-limited, prevents both hammering and permanent demotion.
- Confidence: read the definition; the remainder of the loop body (lines 440 to 745) was skimmed, not traced.

#### "Prefer the working fallback" is in-memory, per session, 10 minutes idle and 30 minutes hard
- What the code does: after a rate-limit fallback wins, the session remembers requested model and fallback model in a process-global map (max 1,024 sessions). The next run tries that fallback first if the requested model is unchanged, the last use was under 10 minutes ago and the stick began under 30 minutes ago; taking the entry clears it unless the new winner re-records it.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-sticky.ts#L12-L66 : constants, the map, `takeStickyRateLimitFallback`.
- Edge cases and failure handling: lost on gateway restart; an aborted sticky run drops stickiness.
- Round 1: `corrected`. Round 1: "the next turns prefer the working fallback for up to 30 min" and asked how it is stored: it is memory only, with a 10-minute idle window as well as the 30-minute cap.
- For rizzy-harness: stickiness is a cache-warmth optimisation; keep it in memory and bounded.
- Confidence: read the definition. (checked in review)

### Steering and queueing

#### Queue modes: steer (default), followup, collect, interrupt; defaults 500 ms, cap 20, drop `summarize`
- What the code does: the active-run policy resolves in this order: inline `/queue` mode, the session's persisted mode, the per-channel config, global config, then `steer`. Debounce, cap and drop policy resolve similarly, defaulting to 500 ms, 20 and `summarize`. With mode `steer`, both "steer" and "followup" are enabled: the message is first offered to the active run; on rejection it is queued as a follow-up. `interrupt` aborts the active run and waits for it to settle before running the new message. Overflow with `summarize` drops the oldest entries and keeps short summaries as a synthetic follow-up; each dropped source is completed with a durable "dropped" receipt.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/settings.ts#L10-L41 : resolution order and defaults.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/state.ts#L25-L27 : 500, 20, `summarize`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/get-reply-run-admission.ts#L484-L522 : `shouldSteer`, `shouldFollowup`, the run-now path.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue-policy.ts#L4-L18 : `resolveActiveRunQueueAction`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/get-reply-run-queue.ts#L9-L22 : interrupt waits for the writer to settle.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/state.ts#L94-L135 : the cap trims summarised entries and completes each with `queue-cap-old`.
- Edge cases and failure handling: the docs say channels with durable ingress treat `collect` as `followup` (`docs/concepts/queue.md` line 52); I did not find that rule in code. Messages from another person are refused for steering unless `allowCollaborativeSteering` is set (`refuseCollaborativeSteering`).
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/lifecycle.ts#L187-L209 : the collaborative-steering refusal.
- Round 1: `confirmed` (steer default, 500 ms, cap 20, summarize).
- For rizzy-harness: model "steer with fallback to queue" as one policy; and make drop of an accepted message produce an explicit receipt.
- Confidence: traced end to end for the policy; the debounce timer was not read.

#### Steering injection into a live run: a FIFO queue drained at fixed checkpoints, with in-flight custody
- What the code does: `AgentSession.steer` enqueues a message injection, expands skill and prompt templates, builds a user message, and calls `queueSteer`, which tracks it as a queued user message and hands it to the core agent's steering queue. The core loop drains that queue at checkpoints: before the first turn, after each turn's tool batch, between sequential tool calls (after the first call has started), after a tool batch to override a stop hook, and again after the follow-up drain so an accepted steer cannot be stranded by `agent_end`. In `one-at-a-time` mode (the core default) one message is injected per checkpoint; in `all` mode (used by the gateway steer) every queued message is. Drained messages sit in an in-flight list until `message_end` commits them; a failed run restores them ahead of later input; one already submitted to the provider cannot be cancelled.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/agent-session-prompting.ts#L495-L563 : `steer`, with its doc comment ("Delivered before the next unstarted tool launch or model call. Running tools continue; suppressed calls receive paired synthetic results").
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/agent-session-prompting.ts#L607-L618 : `queueSteer`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent.ts#L215-L216 : both queues default to `one-at-a-time`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/pending-message-queue.ts#L53-L131 : `peek`, `drain` with the "submitted" fence, `commit`, `restore`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/pending-message-queue.ts#L133-L154 : `cancelFirst` cannot withdraw provider-admitted input.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L250-L272 : the loop injects pending messages at the top of each turn.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L465-L502 : checkpoint after a turn and the final recheck after follow-ups.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L626-L634 : checkpoint before the next sequential tool call; a non-empty result releases the skipped calls.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L871-L903 : unstarted tail calls get synthetic results with reason `steering`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/types.ts#L292-L298 : `toolExecution` defaults to `parallel`.
- Edge cases and failure handling: in parallel batches (the default) steering cannot skip anything; only sequential batches (a tool can force sequential mode) skip the tail after the first call starts. The gateway refuses to inject into a run that has shown no progress for longer than the stale threshold, unless it is waiting on a human or runtime-owned wait, and it caps acceptance with a transcript-commit wait and timeout.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/runs.message-injection.ts#L49-L76 : stale-run refusal.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-queue-message.ts#L226-L248 : waits for `message_end` of the same identity or rejects when the session settles or hands off.
- Round 1: `confirmed` ("`steer` injects into the active run"). The round2 question on how `steer` is injected is answered above; Round 1 did not say that it only takes effect at checkpoints and never aborts running tools.
- For rizzy-harness: the in-flight custody list (drained, not yet committed, restored on failure) is what makes steering safe when a run fails; copy the idea. It is plain in-memory arrays in `PendingMessageQueue` (`packages/agent-core/src/pending-message-queue.ts`), so it does not survive a process crash; durability of accepted input comes from the pending-input row described in a later finding, not from this queue. Note that default parallel tool batches make steering coarser than the docs imply.
- Confidence: traced end to end. (checked in review; corrected in review: the original "safe under crashes" wording overstated it, the queue is in memory and only covers a failed run, not a process crash)

#### Where accepted input is stored before it is acknowledged: a durable ingress table for channels, a staged pending-input row for agent turns
- What the code does: two stores.
  - Channel messages: `enqueueChannelIngressInDatabase` inserts rows into the `channel_ingress_events` SQLite table (unique on queue name and event id, so a redelivery is a no-op), as `pending`, or directly as `claimed` by an idle-claim shortcut when no pending or claimed row exists (decided in the same writer transaction). The drain claims a row with a token, owner and lease, refreshes it, and settles it as completed, released, failed or dead-lettered at adoption time. The transport ACK is awaited after the claim is owned and before dispatch starts its watchdog, so the row exists before the platform is told the message is accepted.
  - Gateway `agent` turns: `recorder.stageApproved` stages the user turn into the per-agent SQLite pending-input table "without changing the active transcript or scheduling execution"; if staging fails the run errors with "agent turn was not durably admitted".
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L145-L258 : the insert with `doNothing` on conflict and the idle-claim shortcut.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L260-L293 : claim with token, owner, time.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L330-L360 : recovery of a stale claim back to `pending`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-drain.ts#L484-L499 : the transport ACK is awaited after the claim is owned.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.pending-inputs.ts#L233-L236 : `stageSessionPendingInput` contract.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/agent-turn/agent-run-user-turn.ts#L402-L426 : the staging call and the failure.
- Edge cases and failure handling: a corrupt payload row is tombstoned instead of blocking the lane; a claim whose owner is lost goes back to `pending` without counting as a delivery failure; overflow drops under `old`/`new` are recorded as terminal outcomes so durable ingress does not replay the discarded message.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L330-L360 : stale claim recovery (same lines as above, `status: "pending"`, "Losing a claim owner is not a delivery failure").
- Round 1: `confirmed` ("accepted input is stored before the Gateway acks"); the storage is a SQLite table for channels and a staged row for agent turns. I did not trace the `chat.send` (web UI) path separately.
- For rizzy-harness: insert-before-ack with a unique event id gives idempotent redelivery; copy the claim token plus lease plus tombstone-on-corrupt design for any durable inbox.
- Confidence: traced for the channel table and the `agent` RPC; not for `chat.send`. (checked in review)

#### The "writer claim": `activeWriterRunId` on the session entry, compared by every terminal write
- What the code does: a run is adopted with `session.actor.adoptRun`, which patches `activeWriterRunId` (with an expected lifecycle state) onto the session entry. Terminal accounting captures `{sessionId, lifecycleRevision, writerRunId}` at the start and, inside the session actor's authorise step, throws "Terminal accounting session changed" if any differs. `completeTurn` throws `SessionActorStaleStateError` when the entry's writer is no longer the bookkeeping writer. `writer-revoke` clears the claim (and optionally marks the run killed) conditionally on the run id.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-actor-phase.ts#L172-L177 : `adoptRun` sets the claim.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-actor-phase.ts#L217-L226 : `completeTurn` stale-writer check.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/agent-runner-completion.ts#L36-L68 : the expected tuple and the authorise check.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-entry-patch-operation.ts#L242-L257 : `writer-revoke`.
- Edge cases and failure handling: the original text said the append transaction was not shown to re-check the claim. It does: `runTranscriptWriteSnapshotSync` re-reads the session row inside the write transaction (`readSessionEntryRow`) and refuses with `session-rebound` unless `transcriptWriteScopeIsCurrent` holds, which compares `activeWriterRunId` with the scope's `expectedWriterRunId` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L104-L130 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97). The check is skipped when the scope carries no expected writer (`expectedWriterRunId === undefined`), and the fence is attached from the run's async context, so protection depends on the write happening inside that context. The sessions-cluster file traces this in more detail.
- Round 1: `confirmed` with caveats: the claim is a field on the session entry, set by a compare-and-swap on session id and lifecycle revision (not on the previous writer), and re-checked inside the append transaction for fenced writes, plus by the actor at terminal commit points. It is not a state-directory lock.
- For rizzy-harness: our "one writer per session, contiguous seq" invariant is stronger if the append statement itself carries the expected writer id (OpenClaw does this for fenced writes); make the unfenced path impossible rather than optional.
- Confidence: read the definition (corrected in review: the append transaction does re-check the writer claim, for fenced writes; the earlier "not settled" and the Round 1 verdict "corrected" were wrong)

### Stuck-loop detection, output truncation and plan-mode

#### Tool-loop detection: rolling detector off by default; a repeated-error breaker is always on
- What the code does: there are two mechanisms. (1) A rolling-history detector over the last 30 calls: warning at 10 identical calls with the same outcome, critical block at 20 (10 for `wait`), global circuit breaker at 30 identical no-progress outcomes, ping-pong detection (warning 10, critical 20 with no-progress evidence), argument-churn warning, and an "unknown tool repeated 10 times" block. It runs only when `tools.loopDetection.enabled` is `true`. (2) `observeRepeatedToolError` counts identical error results for the same action; at the third identical failure it records a block for that action and the loop ends the run with a typed message. Mechanism (2) runs in the embedded runner independently of the flag. The post-compaction loop guard is on unless `loopDetection.enabled` is false.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L56-L59 : history 30, thresholds 10, 20, 30.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L548-L685 : `detectToolCallLoop` with every detector and message.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L450-L497 : `observeRepeatedToolError`, third identical failure.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L105-L108 : the rolling checks require `enabled === true`.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/schema.help.runtime.ts#L458-L459 : "Omit to keep rolling detection off and post-compaction protection on".
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L409-L430 : `terminateRun` ends the run with the loop-guard message.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/tool-loop-recovery.ts#L40-L92 : the observer wiring and the fail-closed `loop_admission_failure`.
- Edge cases and failure handling: if the admission check itself throws, the call is blocked ("loop safety checks failed"), i.e. it fails closed. The exec outcome hash ignores volatile text so a changing timestamp does not hide a loop.
- Round 1: `corrected`. Round 1 gave no thresholds (question: loop-detection thresholds). They are 10, 20, 30 over 30 calls, and rolling detection is off unless enabled; only the three-identical-errors breaker is always on.
- For rizzy-harness: default-on exact-repeat-error breaker is cheap and safe; a heuristic detector that can block good work should be opt-in as here. Hash outcomes, not only arguments.
- Confidence: traced end to end for the thresholds and gating. (checked in review)

#### Timeouts: 48 hours per attempt, 120 s model idle
- What the code does: the default per-run timeout is 48 hours; the model-stream idle timeout default is 120 s for cloud models.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/timeout.ts#L13-L14 : 48 hours.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/llm-idle-timeout.ts#L34-L36 : 120 s.
- Round 1: `confirmed` (48h, 120 s). The self-hosted 300 s value was not read.
- For rizzy-harness: an idle timeout on the stream matters more than a total timeout.
- Confidence: read the definition. (checked in review)

#### Plan mode: no separate plan mode; "read-only" is a session permission mode
- What the code does: I searched for plan mode, `planMode` and `plan_mode` under `src/agents` and `src/auto-reply` and found none. The closest thing is the session permission mode `read-only`, which sets `readOnly: true` in the core tool policy and maps exec to `deny`; the modes are `read-only`, `guarded`, `workspace`, `full`, and a retained mode is a ceiling that can only be tightened.
- Trace:
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L13-L18 : mode to exec-mode map.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L55-L66 : `readOnly` and `workspaceOnly` flags.
  - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L22-L48 : `intersectSessionPermissionModes` keeps the stricter mode.
- Round 1: `confirmed` ("a dedicated plan mode: nothing documented").
- For rizzy-harness: plan mode is a prompt plus a read-only tool set; OpenClaw has only the second half.
- Confidence: read the definition; absence from a text search only. (checked in review)

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Default runtime `openclaw` embeds an agent loop; "page does not name an underlying library" | corrected: loop is OpenClaw's `packages/agent-core`, adapted from Pi | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/THIRD_PARTY_NOTICES.md#L18-L26 |
| Runtime selection: model policy, provider policy, `auto` plugin claim, fallback `openclaw`; failures not retried on another runtime | partly confirmed: explicit plugin runtimes fail closed unless they declare an OpenClaw fallback; the order was not fully traced | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/harness/selection-decision.ts#L102-L123 |
| Prompt modes `full`, `minimal` (sub-agents), `none` | confirmed (minimal also for cron and any restricted tool allowlist) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-system-prompt-prepare.ts#L250-L255 |
| Bootstrap files capped 20,000 each and 60,000 total | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap.ts#L87-L90 |
| `USER.md` capped at 4,000 chars | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap.ts#L87-L90 |
| Workspace files include `TOOLS.md` and `HEARTBEAT.md` (page summary) | refuted (not in the injected set) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace-bootstrap-policy.ts#L30-L37 |
| Sub-agents get only `AGENTS.md` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace.ts#L859-L859 |
| Auto-compaction at the smaller of 75% of window or window minus reserve | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction-threshold.ts#L1-L14 |
| `keepRecentTokens` 20k | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/settings-manager.ts#L139-L141 |
| Default mode `safeguard` audits structure, pending requests, identifiers; marks degraded | confirmed (quality guard on in code; schema comment says off) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/extensions.ts#L28-L47 |
| Compaction on overflow error: compact and retry | confirmed, with a 3-attempt cap, a truncation step and pre-exits | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L279-L296 |
| A silent pre-compaction turn saves notes | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/flush-plan.ts#L22-L38 |
| Pruning: soft-trim keeps first/last 1500 of >4000; hard-clear near 50%; protects last three assistant turns | confirmed (hard clear also needs 50,000 eligible chars) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/tool-result-truncation.ts#L222-L281 |
| Pruning off by default except Anthropic auth seeds `cache-ttl` 1h | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/anthropic/config-defaults.ts#L205-L222 |
| Dreaming: `minScore`, `minRecallCount`, `minUniqueQueries` thresholds (numbers not on page) | confirmed and filled in: 0.75, 3, 3 | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/memory-host-sdk/dreaming.ts#L37-L39 |
| A rewrite that drops more than 25% of prior entries is rejected | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/dreaming-consolidation.ts#L288-L293 |
| Dreaming runs as a cron job, default 03:00 daily | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/memory-host-sdk/dreaming.ts#L22-L22 |
| `memory_search` hybrid vector plus keyword | confirmed (0.7 / 0.3, MMR, 30-day decay, hard-coded) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/memory-search.ts#L162-L170 |
| Skill load precedence workspace, project `.agents/skills`, personal, managed, workshop, bundled, extra | confirmed (personal only on the default state dir) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/workspace-skill-sources.ts#L134-L186 |
| Skill gating by `metadata.openclaw` `requires.bins/env/config`, `os` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/config.ts#L105-L141 |
| Sub-agent default concurrency 8 per spawning session | confirmed (changed in review): per-owner lanes `subagent:<owner>` each of width 8; per-spawner children cap is 5; depth cap is 5 | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/process/command-queue.state.ts#L6-L13 |
| Sub-agents have no session or message tools by default | confirmed (hidden children lose `message`, `sessions_send`; leaves lose `sessions_*`) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.policy.ts#L57-L89 |
| Sub-agent model via `agents.defaults.subagents.model` | confirmed (plus agent-level and inheritance) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-selection-config.ts#L30-L46 |
| Unbound messages are not sent to an arbitrary agent | confirmed (throws when several agents and no default) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-scope-config.ts#L178-L188 |
| Cross-agent session access on by default; visibility default `all` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/session-visibility.ts#L132-L138 |
| Rate-limit cooldown follows provider headers; other errors 30 s, 1 min, 5 min cap | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L18-L28 |
| Billing failures disable a credential 10 minutes | corrected: 10 min is the base; doubles to a 24 h cap | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L35-L46 |
| Profiles ordered: per-agent override, `auth.order`, configured, stored; OAuth before keys; least-recently-used | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/order.ts#L293-L312 |
| After a rate-limit fallback the next turns prefer the fallback for up to 30 min | corrected: in-memory, 10 min idle and 30 min cap, lost on restart | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-sticky.ts#L12-L14 |
| Queue: steer default, 500 ms debounce, cap 20, overflow summarize | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/state.ts#L25-L27 |
| Accepted input is stored before the Gateway acks | confirmed (SQLite `channel_ingress_events`; staged pending input for `agent` turns) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L145-L258 |
| Each admitted run records an active-writer claim before streaming; the commit transaction verifies it | confirmed with caveats (changed in review): claim is set by `adoptRun`; the append transaction re-reads the row and compares `activeWriterRunId` for fenced writes (unfenced writes are not checked) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-actor-phase.ts#L172-L177 |
| Loop-detection thresholds (not opened) | corrected: 30-call history; 10 warn, 20 critical, 30 breaker; off unless enabled; repeated-error breaker at 3 always on | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L56-L59 |
| Default per-attempt timeout 48 h; model idle 120 s | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/timeout.ts#L13-L14 |
| No dedicated plan mode | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L13-L18 |

## 5. In the code but not in the documentation

- AGENTS.md truncation keeps a "policy digest" of rule-like lines, so rules past the head survive a cut (`embedded-agent-helpers/bootstrap.ts`, finding above).
- Stable-prefix prompt cache keyed by `sha256` of the inputs, 64 entries (`system-prompt.ts`).
- Compaction reserve floor of 20,000 tokens with a 25% cap, so small-window models do not compact constantly (`agent-settings.ts`).
- Compaction writes a boundary summary when there is nothing real to summarise, to stop a re-trigger loop (`compaction-safeguard.ts`).
- Summariser input drops tool-result `details` and runtime-context entries as a stated security rule (`compaction.ts`).
- Memory flush has a second trigger: transcript file at least 2 MiB, and the flush's `write` tool is wrapped to append to one path (`agent-runner-memory.ts`, `agent-tools.read.ts`).
- Cache-TTL pruning projections are frozen, replayed byte-identically and recorded in a transcript marker (`tool-result-truncation.ts`).
- Per-result live tool-output caps scaled by context window: 16,000, 32,000, 64,000 characters, at most 30% of the window (`tool-result-limits.ts`).
- Fuzzy edit matching folds Unicode (NFKC, quotes, dashes, spaces) and refuses ambiguous boundaries; failed edits return the current file contents (`edit-diff.ts`, `edit.ts`).
- Auth-profile `auth_permanent` backoff ladder (10 min to 1 h) and the 24 h billing cap; probe throttle for the primary during cooldown (`usage-failure-state.ts`, `model-fallback-cooldown.ts`).
- Subagent depth default 5 and active-children default 5 (`agent-limits.ts`).
- Four sub-agent completion modes: `announce`, `collector`, `quiet`, `thread-direct` (`subagent-system-prompt.ts`).
- Steering never replays an "indeterminate" input; a private completion is never steered into a public turn (`subagent-announce-direct-delivery.ts`).
- Skill Workshop is enabled by default and autonomous (`skill-workshop/openclaw.plugin.json`).
- `MEMORY.md` is withheld from group, channel, cron and sub-agent prompts (`workspace.ts`).
- The schema comment for the compaction quality guard ("Default: false") contradicts the wiring (default true).

## 6. Could not find or could not settle

- **The 40,000-token summariser input cap and `maxSummaryChars` 16,000 (Round 1).** I found `MAX_COMPACTION_SUMMARY_CHARS` referenced in `compaction-safeguard.ts` but did not locate its value or the serializer budget (`sessions/compaction/request-budget.ts`). Reading those two files would settle it.
- **Whether the transcript append transaction itself re-checks `activeWriterRunId`.** I read the actor commands and the terminal accounting checks, not the append worker SQL (`session-actor-append.worker.ts`).
- **Sub-agent lane width versus per-session concurrency.** Settled in review: per spawning session (see the caps finding).
- **Which callers pass `fallbacksOverride: []` for a user model selection.** The mechanism is in `model-fallback-candidates.ts`; the call sites for `/model` were not traced.
- **The `chat.send` (Control UI) path for storing input before the ack.** Only channel ingress and the gateway `agent` RPC were traced.
- **Collect and debounce timers.** I read the policy, not the timer implementation in `queue/drain.ts`.
- **Context engine plugin slot and `legacy` default.** I saw the `legacy` registration (`context-engine/legacy.ts`) but did not read how `ownsCompaction` changes the compaction path.
- **Round 1 question on workshop skills' prompt and writer.** The manifest and scheduler were read; the review prompt (`experience-review-prompt.ts`) and the writer (`library.ts`) were not.
- **Per-attempt LLM retry and the remaining body of `model-fallback-runner.ts` (lines 440 to 745).** Skimmed, not traced.
- **Other vendors' agents (Codex app-server, ACP).** The Codex harness plugin and `acp-spawn*.ts` exist; they belong to the sessions cluster and were not read.

## 7. Citation check

`python3 research/round2/check_citations.py research/round2/openclaw.agents.md`

`research/round2/openclaw.agents.md: 206 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Checked 18 of the 38 findings against the pinned clone. Verdicts: 15 supported (one of them an absence confirmed), 3 partly (corrected in the text above), 0 not supported, 0 wrong in the sense of invented behaviour; one Round 1 verdict was reversed (sub-agent concurrency).

| Finding | Verdict | Reason |
|---|---|---|
| Workspace files as Project Context | supported | file list has no `TOOLS.md` or `HEARTBEAT.md`; 20,000 / 60,000 / 4,000 caps and 75/25 and 45/35 ratios confirmed; the Round 1 refutation stands |
| Skills catalog caps | supported | 150 skills and 18,000 chars in `skill-prompt-limits.ts` |
| Skill Workshop | supported | `enabledByDefault`, mode defaults to `auto`, 10 iterations and 30 s idle |
| Auto-compaction trigger | supported | threshold formula `min(window - reserve, floor(0.75 * window))` |
| `safeguard` default | supported | `applyCompactionDefaults` runs in `materialize.ts` after `applyAgentDefaults`, so the default holds; `resolveEffectiveCompactionMode` alone returns `default` when unset |
| Live tool-result caps | supported | 30% share, 4 chars per token, 16k/32k/64k hard caps |
| Dreaming | supported | 0.75, 3, 3, 14 days, 160 tokens, 0.25 loss fraction, `0 3 * * *` |
| Caps: depth, children, lane width | partly | depth 5 and children 5 confirmed; the "gateway-wide lane" claim is wrong: lanes are `subagent:<owner>`, width 8 each, so Round 1's "8 per spawning session" was right; fixed |
| What the child gets | supported | `isolated` unless a thread-binding policy says otherwise; `fork` explicit |
| Tools the child loses | supported | `SUBAGENT_TOOL_DENY_ALWAYS` and `SUBAGENT_TOOL_DENY_LEAF` lists match |
| Auth profiles cooldowns | supported | 30 s, 1 min, 5 min; billing 10 min to 24 h; auth_permanent 10 min to 1 h |
| Sticky fallback | supported | 10 min idle, 30 min cap, 1,024 sessions, in-memory |
| Steering injection | partly | queue and checkpoints confirmed; the queue is in memory, so "safe under crashes" was overstated; fixed |
| Accepted input stored before ack | supported | `stageApproved` throws "not durably admitted"; the transport ACK is awaited after the claim is owned |
| Writer claim | partly | the claim exists, but the finding said the append transaction was not shown to re-check it, and it does (`session-accessor.sqlite-transcript-write-snapshot.ts`); Round 1's claim is confirmed for fenced writes; fixed |
| Tool-loop detection | supported | `enabled === true` gate, 30/10/20/30, repeated-error breaker at 3 wired through `createToolLoopBatchAdmission` |
| Timeouts | supported | 48 h and 120 s constants (`SELF_HOSTED` is 300 s) |
| Plan mode | absence confirmed | searched `plan mode`, `planMode`, `EnterPlanMode` under `src` and `packages`: only a tool-name projection list and unrelated "runtime plan" identifiers |

Questions in the review request that land in this file:

- Steering as a queue stored before acknowledgement: the core steering queue (`PendingMessageQueue`) is in-memory arrays. What is stored before acknowledgement is the accepted input (channel ingress row, or the pending-input row staged by `stageApproved`), not the steering queue itself. The finding that says "stored before ack" is supported for those two stores; steering durability is not claimed by the code.
- Sub-agent caps: depth 5 (refused when caller depth is 5 or more), 5 active children per spawner, 8 concurrent runs per spawning session (the original file mislabelled this one as gateway-wide).
- Writer claim: see the corrected finding; the sessions file has the full trace.

Not checked: system prompt assembly, tool descriptions, skill tiers and gating, `edit` and `apply_patch`, summary generation, overflow recovery, memory flush, pruning, memory search, who writes memory, spawn path, child model, result announcement, routing, cross-agent access, model fallback chain and loop, queue modes. Overall: no invented behaviour was found in the checked set; the three errors were scope or settling errors (a concurrency lane misread, an in-memory queue described as crash-safe, a transaction check missed). The unchecked remainder should be trusted for constants and structure, and re-checked wherever it says a lane, queue or claim is global, durable or unsettled. The citation check was re-run after editing.

# Synthesis 03: context management and compaction (area 6), memory and persistent instructions (area 7)

Research date 2026-11-10. Synthesis only: every fact below comes from the survey and review files in `research/round1/`; nothing was browsed. "Nothing documented" means the survey found no page, not that the feature is absent.

## 1. Scope

Areas covered: 6 (context management and compaction) and 7 (memory and persistent instructions), across the seventeen surveyed harnesses plus our own inventory.

Read, by file:

| File | What was read |
|---|---|
| `codex.md` | Areas 6 and 7 (346-443); sections 3, 6, 7 and the review (986-1300, skimmed for these areas) |
| `opencodex.md` | Areas 6 and 7 (58-69); keyword pass over sections 3, 6, 7, 9 |
| `opencode.md` | 179-223; keyword pass over 53-56 (cache warming), 458-640, line 19, 309 |
| `claude-code.md` | 1109-1345 only (area 6 and 7 entries, duplicates merged); lines 1303-1306 for auto memory; keyword grep for the rest. The other ~900 KB was not read |
| `claude-code.review-1.md` | lines 46-95 (corrections, Unverified, duplicates, omissions); keyword grep of reviews 2 and 3 |
| `claude-desktop.md` | 102-140; keyword pass over 469-620 |
| `deepseek-harness.md` | 161-200; keyword pass over 533-700 |
| `oh-my-pi.md` | 154-219; keyword pass over 549-700 |
| `pi.md` | 153-212; keyword pass over 561-700 |
| `cursor.md` | 109-165; keyword pass over 456-700 |
| `antigravity.md` | 138-175; keyword pass over 494-700 |
| `openhands.md` | 123-160; keyword pass over 459-700 |
| `openclaw.md` | 170-235; keyword pass over 596-760 |
| `goose.md` | 140-205; keyword pass over 518-700 |
| `hermes-agent.md` | 217-290; keyword pass over 400-760 |
| `zed-acp.md` | 75-110; keyword pass over 355-520 |
| `warp.md` | 161-195; keyword pass over 546-700 |
| `cline.md` | 125-198; keyword pass over 476-700 |
| `own-rizzy-harness.md` | sections 4 and 5 (77-103); keyword grep for skills and instruction files elsewhere |

Not read: Claude Code survey outside area 6/7 (so tool-result handling elsewhere in that file may be missed); the Distinctive/Unverified sections of the surveys were read only by keyword (compact, memory, AGENTS, cache, skill, index), not line by line. Sections of `own-rizzy-harness.md` other than 4, 5 and keyword hits were not read, so "not in the inventory" below means "not found in what was read". Hermes URLs in the survey are relative to `https://hermes-agent.nousresearch.com/docs/`; they are expanded here with that base. OpenHands URLs use the survey's `D:` shorthand, expanded as `https://docs.openhands.dev`.

Check-value convention: `reviewed` = entry says verified in review (a few partly); `corrected` = review corrected the text and the corrected text is used; `unreviewed` = not checked; `unverified` = survey or review could not confirm.

## 2. Features

### Area 6: context management and compaction

#### Automatic compaction (trigger, who compacts, what is kept)
What it is: when the conversation nears the model window, older history is replaced by a model-written summary and the run continues.
Variants: trigger is a percent of window (Zed 90%, goose 80% capped at 225k, OpenClaw 75%, Hermes 50%), a reserve (OpenCode 15% buffer, Pi 16,384 tokens, OMP larger of 16,384 or 15%), an event count (OpenHands 120 events), or "at the limit" (Claude Code). The summariser is the session model (Claude Code, which reuses its cache prefix), a separately configured model (OpenCode, Zed, OpenClaw, Hermes auxiliary), or provider-side (Codex `remote_compaction`, OpenCode `native`, OpenClaw native endpoints, OpenCodex forwarding). What is kept: a verbatim tail (OpenCode 15k, Pi and OMP 20k, Hermes lean tail 2.5% of window), a protected head (OpenHands first 4 events, Hermes first 3 messages), or only the last user request (Cursor). Logged how: Pi, OpenHands, DeepSeek harness, Hermes keep the original history and add a compaction record; Cline and Claude Code replace history.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `model_auto_compact_token_limit` (scope `total` or `body_after_prefix`) and `model_post_turn_compact_threshold_percent` trigger it; prompt overridable (`compact_prompt`); providers can declare `remote_compaction = v2` for server-side compaction; `PreCompact`/`PostCompact` hooks. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/hooks.md | unreviewed (area entry carries no review tag) |
| OpenCodex | Gateway, not a harness. Native passthrough forwards `/v1/responses/compact` upstream; for routed models the proxy acts as a tool-free summariser and adds a checkpoint note to system instructions. Per-provider context caps (default value 350,000, only lowers known windows); upstream 413 becomes `context_length_exceeded`. | GA | https://opencodex.me/getting-started/how-it-works/, https://opencodex.me/reference/adapters/, https://opencodex.me/guides/model-routing/ | unreviewed (the 32 MiB compaction ceiling and the 350,000 default were confirmed in review) |
| OpenCode | Compacts when the window minus a buffer (default 15%, min 16k) is reached; summarises everything, keeps about the last `keep.tokens` (15,000) verbatim; later compactions update the summary from newer turns only; summary request reuses the main prompt cache; one compact-and-retry on overflow; summary model overridable (`agents.compaction.model`); `settings.compaction.type` `native` (provider opaque checkpoint) or `summary`. | GA | https://opencode.ai/docs/compaction/ | reviewed |
| Claude Code | Compacts at the model limit by default (200K for some models, about 967K on native-1M models, cloud sessions earlier); window settable per model (`/autocompact`, `autoCompactWindow`, `--autocompact`, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, range 100K-1M); stops after a few attempts with a "thrashing" error if one output keeps refilling context. Summary request inherits the session's thinking setting. Which model summarises is not stated on the pages reached (review). | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/settings-reference.md | reviewed |
| Claude Desktop | Code tab is the Claude Code binary; Desktop shows a Context card with `/compact` and `/context`; cloud sessions set their own threshold; a compacted cloud session teleported to local brings only the summary. | GA | https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/claude-code-on-the-web | unreviewed |
| DeepSeek harness | Triggers: pressure before a step, provider-confirmed overflow, manual `compactNow`, explicit range. Log sequence start, summary, replacement user message, end; one compaction per session at a time. Numeric thresholds not documented on the page. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction | unreviewed |
| Oh My Pi | On by default; post-response, mid-turn, after overflow, optionally idle. Threshold by tokens/percent or reserve (larger of 16,384 or 15%); `keepRecentTokens` 20000 is a target for summarising compaction. Models may declare `contextPromotionTarget` (switch to a larger model on overflow before compacting). | GA | https://omp.sh/docs/compaction, https://omp.sh/docs/custom-models | corrected |
| Pi | Compacts when context exceeds window minus `reserveTokens` (16384); keeps about `keepRecentTokens` (20000); cuts never land on tool results; thinking blocks dropped and tool results truncated to 2000 chars when summarising; summary has fixed sections (goal, constraints, progress, decisions, next steps, critical context, file lists); per-model overrides. Original entries stay in the session tree. | GA | https://pi.dev/docs/latest/compaction, https://pi.dev/docs/latest/settings | reviewed (defaults; summary format not re-checked) |
| Cursor | CLI sessions that hit the context limit summarise and continue; the latest user request is kept verbatim; on failure the earlier conversation is kept. No dedicated compaction page. Context window size selectable on some cloud/subagent models. | GA (no label) | https://cursor.com/docs/cli/changelog, https://cursor.com/docs/cloud-agent | reviewed (partly: summarise-and-continue and verbatim request) |
| Antigravity | Older turns are summarised automatically into checkpoint summaries; algorithm and thresholds not documented (an SDK local-models config mentions a `compaction_config`). | GA | https://antigravity.google/docs/cli/commands/context, https://antigravity.google/docs/changelog | unreviewed |
| OpenHands | `LLMSummarizingCondenser` fires when history exceeds `max_size` (120 events), keeps the first 4, summarises the middle toward `max_size//2`, emits a `Condensation` event; the log stays intact and the view filters it. Agent raises a `CondensationRequest` on a context-window error. | GA | https://docs.openhands.dev/sdk/arch/condenser.md, https://docs.openhands.dev/sdk/guides/context-condenser.md | reviewed |
| OpenClaw | On by default at the smaller of 75% of window or window minus reserve, or on overflow (compact and retry). One summarisation request capped near 40k input tokens; pairs kept together; invalid summaries rejected; separate summariser `model`, `maxSummaryChars` 16k, `keepRecentTokens` 20k; native endpoints for OpenAI Responses/xAI and server-side compaction for Anthropic. | GA | https://docs.openclaw.ai/concepts/compaction | reviewed |
| goose | Triggers at min(0.8 x window, 225,000 tokens) (`GOOSE_AUTO_COMPACT_THRESHOLD`, `GOOSE_AUTO_COMPACT_TOKEN_LIMIT`); 0.0 disables; full history stays visible, only the compacted version is active; prompt template `compaction.md` is editable. | GA | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md, https://goose-docs.ai/docs/guides/context-engineering/prompt-templates.md | reviewed |
| Hermes Agent | Primary compressor inside the tool loop at 50% of context (floored at 0.75 for windows under 512K); a gateway "hygiene" pass at 85% is a safety net. Middle summarised once by an auxiliary model, re-compressions update the previous summary; default `lean` tail keeps 2.5% of the window plus identifier log, anchor index, verbatim user messages and a `session_search` pointer. If the summary model's window is smaller than the main model's, middle turns are dropped without a summary. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching | reviewed |
| Zed/ACP | `agent.auto_compact` on by default, `threshold` default 90% (percent, tokens, or negative tokens-remaining); `agent.compaction_model` chooses another model; a "Context Compacted" entry shows the summary; banner suggests "New From Summary" if the window is under 80k. ACP carries compaction as a session update; session compaction is also an RFD (title only). | GA | https://zed.dev/docs/ai/agent-panel, https://zed.dev/docs/ai/agent-settings.md | reviewed |
| Warp | Automatic summarisation when the context limit is exceeded; admin toggle for provider-level "prompt summarization caching". No thresholds documented. | GA | https://docs.warp.dev/agents/local-agents/interacting-with-agents/, https://docs.warp.dev/agents/cli/agent-conversations/ | unreviewed |
| Cline | Near the limit it summarises the whole conversation, replaces history, continues; shown as a tool call; reuses the prompt cache. No threshold documented; the page is not in the llms.txt index (possibly legacy). CLI `--compaction agentic|basic|off` (default agentic; `basic` vs `agentic` semantics undocumented). | GA | https://docs.cline.bot/features/auto-compact, https://docs.cline.bot/cli/cli-reference.md | unreviewed for Auto Compact; reviewed for the CLI flag |
| rizzy-harness (ours) | When estimated history exceeds `[chat] summarize_after_tokens` (default 24000), all but the last 2 turns are summarised once (<=300 words) into a `history_summarized` event and reused. The trigger is a char/4 estimate against a fixed number, not the model's real window or measured usage. | works (partial) | `crates/agent-native/src/lib.rs:76-98`, `:570-600`, `:1036-1078`, `crates/agent-native/src/provider.rs:135-155` | from own inventory |

#### Manual compaction and partial compaction
What it is: a user command that compacts now, sometimes with a focus instruction or a partial range.
Variants: focus text (Codex `/compact` after confirmation, Claude Code, Pi, OpenClaw, Warp, Cline), partial ranges (Claude Code "Summarize from/up to here"; DeepSeek harness explicit range), fork-and-compact (Warp), method choice (OMP), an API route (OpenCode, Codex `thread/compact/start`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/compact` summarises the visible chat after a confirmation; `thread/compact/start` over the app-server API. 0.164.0 fixed failed compaction leaving partial summaries. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/changelog | unreviewed |
| OpenCodex | `/v1/responses/compact` forwarded for native passthrough; the proxy summarises for routed models. | GA | https://opencodex.me/getting-started/how-it-works/ | unreviewed |
| OpenCode | `POST /api/session/{id}/compact`. | GA | https://opencode.ai/docs/compaction/ | reviewed |
| Claude Code | `/compact [focus]`; a "Compact Instructions" section in CLAUDE.md steers it; `/rewind` offers "Summarize from here" and "Summarize up to here"; a PreCompact hook can block it or supply instructions (blocking a compaction triggered by an API overflow makes the request fail); `/recap` writes a display-only summary. | GA | https://code.claude.com/docs/en/context-window.md, https://code.claude.com/docs/en/how-claude-code-works.md, https://code.claude.com/docs/en/prompt-caching.md | reviewed (area 6 entries confirmed except idle compaction) |
| Claude Desktop | `/compact` and `/context` in the Code tab. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | `compactNow` and explicit range; compaction costs an extra model request. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction | unreviewed |
| Oh My Pi | `/compact [soft|remote|snapcompact]`, `/shake`, `/handoff` (compact in place with a continuation brief). | GA | https://omp.sh/docs/compaction, https://omp.sh/docs/handoff | unreviewed |
| Pi | `/compact [instructions]`. | GA | https://pi.dev/docs/latest/compaction | reviewed |
| Cursor | CLI `/summarize` (alias `/compress`). | GA | https://cursor.com/docs/cli/reference/slash-commands | reviewed (partly) |
| Antigravity | `/compact` (app v2.24.0, CLI 1.4.4-1.4.6 changelog); the `/context` page does not mention it. `/rewind` rolls back to an earlier checkpoint. | GA | https://antigravity.google/docs/changelog, https://antigravity.google/docs/cli/commands/context | unreviewed |
| OpenHands | Canvas "Compact context" button and an API "Condense Conversation". | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | reviewed |
| OpenClaw | `/compact [focus]`. | GA | https://docs.openclaw.ai/concepts/compaction | reviewed |
| goose | `/compact` in CLI, "Compact now" in desktop; `/summarize` is a deprecated alias. | GA | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md | reviewed |
| Hermes Agent | Nothing documented for a manual compress command in the entries read. | unclear | https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching | unreviewed |
| Zed/ACP | `/compact`. | GA | https://zed.dev/docs/ai/agent-panel | reviewed |
| Warp | `/compact [instructions]` in the CLI; `/fork-and-compact`. | GA | https://docs.warp.dev/agents/capabilities/slash-commands/ | unreviewed |
| Cline | `/smol` (alias `/compact`) compresses in place; `/newtask` starts a fresh context with a packaged hand-off (plan, work done, files, next steps). | GA | https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| rizzy-harness (ours) | None: no `/compact`. | none | `crates/agent-native/src/lib.rs:702-708` | from own inventory |

#### Staged, fallback and overflow-recovery compaction
What it is: more than one compaction method, or a defined recovery when summarising fails or the provider rejects the request as too long.
Variants: an ordered chain of methods (OMP), a failure ladder with a deterministic last resort (Hermes), an audit of the summary (OpenClaw), prune-then-summarise (DeepSeek harness, Hermes), switch to a bigger model first (OMP), reactive compact-and-retry once (OpenCode, Pi, OpenClaw, OpenHands, DeepSeek harness), and rule-based truncation when summarising is unsupported (Cline, goose).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Nothing documented beyond fixes for failed compaction (0.164.0). | unclear | https://learn.chatgpt.com/docs/changelog | unreviewed |
| OpenCode | One compact-and-retry on overflow; per-model choice between provider-native and summary compaction. | GA | https://opencode.ai/docs/compaction/ | reviewed |
| Claude Code | Anti-thrash stop after a few attempts; switching to a smaller-context model summarises first with the previous model (2.1.301); fallback chains apply to compaction (review omission note); a gateway that rewrites the too-long error breaks reactive compaction. | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/changelog.md | reviewed for the first; changelog items unreviewed |
| DeepSeek harness | Optional tool-result pruning runs first, then range selection preserving call/result pairs (can compact early steps of one oversized turn); recovery retries only if the surface advanced; a `start` without `end` is an orphaned crash lock. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction | unreviewed |
| Oh My Pi | Default `methodOrder`: remote (provider-native), snapcompact (older history archived as images, no summarisation call, needs a vision model), handoff (structured summary), shake (strip bulk, no summary), soft (model-written summary). Pre-step prunes superseded reads and useless output. | GA | https://omp.sh/docs/compaction | corrected |
| Pi | Overflow error triggers compact and retry once; an oversize single turn is cut internally and gets two merged summaries. | GA | https://pi.dev/docs/latest/compaction | reviewed |
| Cursor | On compaction failure the earlier conversation is kept. | GA | https://cursor.com/docs/cli/changelog | reviewed (partly) |
| OpenClaw | `safeguard` mode audits the summary (structure, pending requests, identifiers) and marks degraded results; invalid summaries rejected; a failing plugin context engine is quarantined and falls back to `legacy`. | GA | https://docs.openclaw.ai/concepts/compaction, https://docs.openclaw.ai/concepts/context-engine | reviewed (compaction); unreviewed (engine) |
| goose | Context-limit strategy: summarise, truncate oldest, clear, or prompt (CLI only); `GOOSE_CONTEXT_STRATEGY`; desktop and headless always summarise. | GA | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md | reviewed |
| Hermes Agent | Failure ladder 60/300/900 s then a deterministic fallback summary; tool-result pruning (over 200 chars, no LLM) runs before summarising. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching | reviewed |
| Cline | With models lacking support, falls back to rule-based truncation; checkpoints can restore the pre-summary state. | GA | https://docs.cline.bot/features/auto-compact | unreviewed |
| rizzy-harness (ours) | None: a provider context-length error simply errors the turn; summarisation is skipped when kept turns alone are large. | none | `crates/agent-native/src/lib.rs:702-708` | from own inventory |

#### Idle, background and incremental compaction
What it is: compacting before the user needs it, to save cache-miss cost or latency.
Variants: Claude Code compacts after idle just before the 1-hour cache expires; OpenCode pre-writes the summary ahead of the threshold; OMP has an idle trigger (off by default); Hermes micro-compaction summarises one exchange per turn into a rolling marker.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | On the one-hour cache, a conversation of at least 200,000 tokens (`CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS`) that sits idle is compacted before the cache expires; `idleCompaction: false` (v2.1.290+) disables. The survey had three conflicting entries; review confirmed 200K, the one-hour TTL and the setting, and moved "about 54 minutes", "about an hour idle", "machine awake" and the 100,000 floor to Unverified. | GA | https://code.claude.com/docs/en/prompt-caching.md, https://code.claude.com/docs/en/settings-reference.md | corrected |
| OpenCode | Background compaction pre-writes the summary ahead of the threshold and swaps it in; held summaries are lost on restart; unused ones cost tokens. | experimental | https://opencode.ai/docs/compaction/, https://opencode.ai/docs/config/ | reviewed |
| Oh My Pi | Idle compaction is OFF by default (`idleEnabled` false; 200000 tokens / 300 s when on); async pre-preparation exists. | GA | https://omp.sh/docs/compaction | corrected |
| Hermes Agent | Micro-compaction: after each turn, summarises the oldest unsummarised exchange into a rolling marker (user messages never summarised; a defrag pass re-summarises the summary). Warns each pass invalidates the cached prefix. | opt-in, default off | https://hermes-agent.nousresearch.com/docs/developer-guide/micro-compaction | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Pruning or masking of old tool output
What it is: shrinking or removing old tool results from the prompt without a summary call.
Variants: masking by age with a one-line stand-in, done in batches to keep the cache valid (ours); soft-trim then hard-clear after cache TTL (OpenClaw); elide to a recoverable artifact (OMP `/shake elide`); summarise older tool outputs in the background (goose); prune before summarising (Hermes, DeepSeek harness); delegate to server-side clearing (OpenClaw on direct Anthropic calls).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | No documented clearing of old tool results as a context step was found on the pages read (review). Images and PDFs are pruned in batches when per-request limits are hit. | nothing documented (tool results) | https://code.claude.com/docs/en/prompt-caching.md | reviewed |
| DeepSeek harness | Optional tool-result pruning runs before range selection. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction | unreviewed |
| Oh My Pi | Pre-step prunes superseded reads and useless output; `/shake` modes `elide` (heavy output replaced, saved as `artifact://` refs), `images`, `thinking`; `shake` needs no LLM. | GA | https://omp.sh/docs/compaction | corrected |
| Pi | Append-only `context_edit` session entry changes future model context for an earlier entry without modifying it; extensions have `context` and `before_provider_request` hooks. How users create `context_edit` is not described. | GA | https://pi.dev/docs/latest/session-format, https://pi.dev/docs/latest/extensions | unreviewed |
| OpenClaw | Session pruning: after cache TTL, soft-trim large results (keep first/last 1500 chars of results over 4000), hard-clear at roughly 50% usage; last three assistant turns and everything before the first user message are protected. Off by default except Anthropic auth seeds `cache-ttl` 1h. | GA | https://docs.openclaw.ai/concepts/session-pruning | unreviewed |
| goose | Older tool-call outputs are summarised in the background, recent ones kept in full (`GOOSE_TOOL_CALL_CUTOFF`). | GA (no label) | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md | unreviewed |
| Hermes Agent | Old tool results over 200 chars pruned with no LLM call as the first compression step. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching | reviewed |
| OpenCode | V1 had prune behaviour; those keys are ignored in V2. | nothing documented (V2) | https://opencode.ai/docs/compaction/ | reviewed |
| rizzy-harness (ours) | Tool outputs older than the newest `keep..2*keep` (default keep 8, at least 400 chars) are replaced by a one-line note, in batches so the prompt cache stays valid. | works | `crates/agent-native/src/mask.rs:15-60`, `crates/core/src/savings.rs:48-67` | from own inventory |

#### Large-output handling (caps, spill to file, retrieval)
What it is: bounding what one tool result can put in the prompt and keeping the full text retrievable.
Variants: spill to a file with a locator and a read/grep hint (DeepSeek harness, Claude Code for MCP, OpenCode); head-and-tail truncation with a store-and-retrieve tool (ours); tool-side truncation with no spill (Pi); offload only hook output (Codex).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | If a hook's `additionalContext` exceeds `additionalContextLimit` (default 2,500 tokens) the full text goes to disk and a preview is sent; ordinary MCP tool schemas are compacted above a byte threshold (5,000). | GA | https://learn.chatgpt.com/docs/hooks.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed (hook offload) |
| OpenCode | `tool_output.max_lines/max_bytes`; shell and webfetch shorten large output and keep the full text in a managed file. | GA | https://opencode.ai/docs/config/, https://opencode.ai/docs/tools/ | unreviewed |
| Claude Code | MCP output warning above 10,000 tokens; default limit 25,000 (`MAX_MCP_OUTPUT_TOKENS`); results over the limit or over 50,000 chars are saved to the session `tool-results` dir and replaced by a path message; per-tool ceiling up to 500,000 chars; long error text keeps first and last 5,000 chars; images saved with the path given to the model. | GA | https://code.claude.com/docs/en/mcp.md | unreviewed (area 6 entry; sampled review did not list it) |
| DeepSeek harness | Spill policy replaces text or image results over `maxInlineTokens` with head, tail and a locator; per-session hashed directory, 0700/0600 permissions, exclusive creation against planted symlinks; on save failure the inline result is kept. No retention policy defined; inherited locators are not copied on fork. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/spill | unreviewed |
| Oh My Pi | `/shake elide` saves heavy output as `artifact://` references (see pruning). | GA | https://omp.sh/docs/compaction | unreviewed |
| Pi | Tools are expected to truncate; documented built-in limit 50KB or 2000 lines with `truncateHead`/`truncateTail` helpers. No spill documented in the entry. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| Hermes Agent | `web_extract` truncates with the full text saved to disk. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed (surfaced in a keyword pass) |
| Zed/ACP | ACP `terminal/create` takes an `outputByteLimit` and returns a truncated flag. | GA (protocol) | https://agentclientprotocol.com/get-started/architecture.md | reviewed (terminal entry) |
| rizzy-harness (ours) | Shell output is filtered (ANSI, progress lines, build noise, passing tests collapsed to a count, repeats collapsed); full text kept (2 MB, per-session random ids) and retrievable with the `tool_output` tool. Model sees at most 30,000 chars per call (head+tail); live stream per call at most 1 MiB; web page at most 20,000 chars; read window 400/2000 lines. | works | `crates/tools/src/filter.rs:1-40`, `crates/tools/src/outputs.rs:15-25`, `crates/tools/src/bash.rs:206-232`, `crates/tools/src/lib.rs:37`, `crates/web/src/lib.rs:32-35` | from own inventory |

#### Prompt caching (layout, TTL, warming, diagnostics)
What it is: keeping the request prefix byte-stable so the provider's prefix cache hits, and managing its lifetime.
Variants: Claude Code documents the layout, what invalidates and keeps the cache, TTL selection and diagnostics; OpenCode and Pi add keep-alive warming (OpenCode off by default, Pi default `idle`); OpenClaw puts a cache boundary in the prompt with date/time in a per-turn message; Hermes uses stable/context/volatile tiers and up to 4 breakpoints; ours appends per-turn context after history.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Quota-aware account selection with thread affinity keeps prompt caches warm; the docs call out a ToS risk. | GA | https://opencodex.me/getting-started/how-it-works/ | unreviewed |
| OpenCode | Cache warming: keep-alive requests at 90% of cache TTL or every four minutes, stop 30 minutes after the last real request, not for subagent sessions, costs real tokens. Summary requests reuse the main cache. Plugin keyed session instructions avoid invalidating the prompt. | GA, off by default (warming); experimental/new (keyed instructions) | https://opencode.ai/docs/warming/, https://opencode.ai/changelog | reviewed (warming) |
| Claude Code | Order: system prompt + tool definitions, then project context (CLAUDE.md, auto memory, unscoped rules), then conversation; plan mode, skills, output style and file-change notices are appended to keep the prefix. Separate caches per model, fast mode and, on most models, effort. TTL: 1h for the main conversation only on a subscription within plan usage, 5m otherwise; `promptCacheTtl`/`subagentPromptCacheTtl` (v2.1.242, corrected from 2.1.243). Cache is per machine and directory; gateway behaviour with `cache_control` documented. `/cost` shows hit ratio and likely miss cause. | GA | https://code.claude.com/docs/en/prompt-caching.md | corrected |
| Pi | `cacheWarming` (default `idle`, global only), `showCacheMissNotices` (default false), `short`/`long` cache lifetimes per model. Mechanism of idle warming not described. | GA | https://pi.dev/docs/latest/settings, https://pi.dev/changelog | reviewed |
| OpenClaw | Stable content above a cache boundary; date/time in a per-turn message so day rollover does not bust the prefix; bootstrap files capped 20,000 chars each and 60,000 total. | GA | https://docs.openclaw.ai/concepts/system-prompt | unreviewed |
| Hermes Agent | Stable, context and volatile tiers; for Claude a `system_and_3` strategy with up to 4 breakpoints (`5m`, `1h`, `auto`); memory writes mid-session reach the prompt only next session; skill changes travel as user messages. The prompt-assembly page places the skills index in two tiers (inconsistent). | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching, https://hermes-agent.nousresearch.com/docs/developer-guide/prompt-assembly | unreviewed |
| Cline | Auto-compact reuses the prompt cache. | GA | https://docs.cline.bot/features/auto-compact | unreviewed |
| Warp | Admin toggle: provider-level prompt summarisation caching. | GA | https://docs.warp.dev/agents/capabilities/slash-commands/ | unreviewed |
| rizzy-harness (ours) | Anthropic `cache_control` breakpoints on the system message and newest message; per-turn context (memory, project brief) appended after the history; OpenAI-style providers cache themselves. On by default. | works | `crates/agent-native/src/llm.rs:102-142`, `crates/agent-native/src/lib.rs:600-608` | from own inventory |

#### Context usage display
What it is: showing the user how full the window is, or what fills it.
Variants: a per-category grid (Claude Code, Antigravity), a token budget by source (OMP), a meter with a button (OpenHands), colour indicators (goose), a token-usage readout (Zed, Cline TUI).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/context [all]` shows a coloured grid by category with suggestions and an over-limit warning (adds an estimate for messages since the last response, so it can read higher than the status line); status-line `used_percentage` measures against the full window, not the compaction threshold. | GA | https://code.claude.com/docs/en/commands.md, https://code.claude.com/docs/en/context-window.md | reviewed |
| Claude Desktop | A Context card with usage. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | `measure()` returns a token snapshot (baseline from last usage or estimated, per-node prices, delta); no UI display or currency cost documented. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/token-meter | unreviewed |
| Oh My Pi | `/context` shows the token budget by source. | GA | https://omp.sh/docs/compaction | unreviewed |
| Antigravity | `/context` shows a grid by category (user, agent, tool calls, system prompt, customizations, MCP tools, free), tracked knowledge/artifact/skill files and a compaction count. | GA | https://antigravity.google/docs/cli/commands/context | unreviewed |
| OpenHands | Canvas context meter and "Compact context" button. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | reviewed |
| goose | Colour usage indicators in desktop and CLI; `GOOSE_CONTEXT_LIMIT` overrides the assumed window (displayed usage only per the page); a context window report/viewer in v1.58.0 notes is undocumented. | GA (override); unclear (report) | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md, https://github.com/aaif-goose/goose/releases | unreviewed |
| Zed/ACP | Token usage shown near the profile selector. | GA | https://zed.dev/docs/ai/agent-panel | reviewed |
| Cline | TUI status area shows model, context usage and cost. | GA | https://docs.cline.bot/usage/tui.md | unreviewed |
| OpenCodex | Real token usage display and a usage page that separates estimates from billing (gateway-level). | GA | https://opencodex.me/guides/web-dashboard/ | unreviewed |
| rizzy-harness (ours) | Total in/out/cache tokens per chat as a tooltip; no live context-fullness indicator against the model window (PLAN section 0 promised a token meter). | partial | `web/src/components/Conversation.tsx:293-297` | from own inventory |

#### Repository maps and code indexing
What it is: giving the agent a ranked summary or searchable index of the repository so it need not discover files by search alone.
Variants: embeddings index (Warp); tree-sitter plus PageRank map injected in the prompt (ours); Cursor's indexing is not documented on pages read.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Warp | Embeddings index of Git-tracked files, built on first open, periodically, at the start of each conversation and on manual sync; agents do not use it until indexing finishes; worktrees indexed separately; Free plan up to 5,000 files per repo; honours `.gitignore`, `.warpindexingignore`, `.cursorignore`, `.cursorindexingignore`, `.codeiumignore`; remote repos indexed on the remote machine. "No code stored on Warp servers" is a vendor claim; where embeddings are computed is not stated. | GA | https://docs.warp.dev/agents/capabilities/codebase-context/ | reviewed |
| Cursor | Indexing, embeddings and `.cursorindexingignore` are not covered on pages opened. | unverified | https://cursor.com/help/customization/context | unverified |
| Antigravity | A `/codesearch` command page appears in the docs index but was not opened. | unverified | https://antigravity.google/llms.txt | unverified |
| rizzy-harness (ours) | tree-sitter tags + PageRank map of definitions (Rust, Python, JS, TS, Go), token budget 256-8192, cached 1 h per folder, injected into the system prompt. Off by default. | works (opt-in) | `crates/codemap/src/lib.rs:1-14`, `:176-179`, `crates/agent-native/src/lib.rs:884-909` | from own inventory |

#### Context cost of tool and skill catalogs (lazy schemas, budgets)
What it is: limiting what tool definitions, skill lists and rule files cost before the first prompt.
Variants: deferred tool schemas loaded on demand (Claude Code, goose, Antigravity MCP), percent-of-window budget for the skills list (Codex), per-file and aggregate byte/token caps on rules (Antigravity), progressive three-level skill loading (Cline).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Skills list uses at most 2% of the window (8,000 chars if the window is unknown; `skills.max_context_tokens` cap 10,000); descriptions shortened, then skills omitted with a warning. MCP schemas compacted above 5,000 bytes. | GA | https://learn.chatgpt.com/docs/build-skills.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed (skills budget) |
| Claude Code | Tool search on by default: only tool names and server instructions load; definitions fetched through a `ToolSearch` tool; `ENABLE_TOOL_SEARCH` auto mode switches at 10% of the window. Docs say selection accuracy degrades beyond 30-50 loaded tools. | GA | https://code.claude.com/docs/en/mcp.md, https://code.claude.com/docs/en/agent-sdk/tool-search.md | unreviewed |
| Antigravity | Rules: 24,000 bytes per file and 20,000 tokens aggregate for global/always-on rules; larger files become pointers read on demand; MCP tool schemas lazy-loaded by default. | GA | https://antigravity.google/docs/rules, https://antigravity.google/docs/mcp | unreviewed |
| goose | `GOOSE_TOOL_DEFERRAL=lookup` publishes MCP tool names up front and enables schemas via `enable_tools`; off unless set. Code Mode exposes three meta-tools. | unclear | https://goose-docs.ai/docs/guides/config-files.md | corrected |
| Cline | Skills: metadata always loaded (about 100 tokens each), body on trigger (under 5k tokens), resources on demand. | GA | https://docs.cline.bot/customization/skills.md | reviewed |
| Zed/ACP | Skills catalog (name + description) capped at 50KB. | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory (no skills; tool schema handling not covered in sections read). | none | not in the inventory | from own inventory |

#### Hooks, plugins and templates that change compaction
What it is: extension points around compaction (replace the summary, change the prompt, observe it, swap the whole context engine).
Variants: full engine replacement (OpenClaw, Hermes), summary replacement hook (OpenCode, Pi), prompt template (goose, Codex), observe-only hook (Cursor).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `compact_prompt` / `experimental_compact_prompt_file`; `PreCompact`/`PostCompact` hooks. | GA | https://learn.chatgpt.com/docs/hooks.md | unreviewed |
| OpenCode | `compaction` session hook can modify or replace checkpoint summaries. | GA | https://opencode.ai/docs/build/plugins/server/ | unreviewed |
| Claude Code | PreCompact hook (block or add summary instructions), TS `PostCompact`; "Compact Instructions" in CLAUDE.md; re-run SessionStart hooks with matcher `compact`. | GA | https://code.claude.com/docs/en/context-window.md, https://code.claude.com/docs/en/agent-sdk/agent-loop.md | reviewed |
| Pi | `session_before_compact` lets extensions cancel or replace summaries. | GA | https://pi.dev/docs/latest/compaction | reviewed (defaults) |
| Cursor | `preCompact` hook, observation only. | GA (no label) | https://cursor.com/docs/hooks | reviewed (partly) |
| OpenClaw | Context engine plugin slot owns ingest, assemble, compact, after-turn; `ownsCompaction`; failing plugin quarantined. | GA (plugin API experimental per the experimental-features page) | https://docs.openclaw.ai/concepts/context-engine, https://docs.openclaw.ai/concepts/experimental-features | unreviewed |
| goose | `compaction.md` prompt template overridable in `~/.config/goose/prompts/`. | GA | https://goose-docs.ai/docs/guides/context-engineering/prompt-templates.md | unreviewed |
| Hermes Agent | Compressor replaceable by a plugin (`context.engine`, one active). Detail page not opened. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins | unreviewed |
| rizzy-harness (ours) | None documented in the inventory. | none | not in the inventory | from own inventory |

#### Side questions, recaps and fresh-context hand-offs
What it is: ways to get an answer or a clean context without compacting the main thread.
Variants: side question kept out of history (`/btw`), display-only recap, ephemeral fork, hand-off brief into a new context.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/recap` (summarise without compacting), `tui.auto_recap` on by default while unfocused; `/side` ephemeral fork. | GA | https://learn.chatgpt.com/docs/developer-commands.md | reviewed (`/side`) |
| OpenCode | `/btw` side question not added to the conversation. | GA | https://opencode.ai/docs/cli/tui/ | unreviewed |
| Claude Code | `/btw` (no tools, replays the newest 20 side exchanges; `f` forks to a background subagent), `/recap`, `/clear`. | GA | https://code.claude.com/docs/en/interactive-mode.md | reviewed |
| Oh My Pi | `/btw`, `/tan` (tangential work in a background agent), `/handoff`. | GA | https://omp.sh/docs/slash | reviewed |
| Pi | Branch summarisation on `/tree` navigation (summary of the branch left attached to the branch entered). | GA | https://pi.dev/docs/latest/compaction | unreviewed |
| Antigravity | `/btw` side question kept out of main history. | GA | https://antigravity.google/docs/cli/commands/context | unreviewed |
| Warp | `/fork-and-compact`. | GA | https://docs.warp.dev/agents/capabilities/slash-commands/ | unreviewed |
| Cline | `/newtask` packaged hand-off. | GA | https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

### Area 7: memory and persistent instructions

#### Instruction files: which file names are read
What it is: plain-text project or user files loaded into the prompt every session.
Variants: `AGENTS.md` is read by nearly everyone; the split is on whether the harness also reads other vendors' names (OMP eight formats, goose via `CONTEXT_FILE_NAMES`, Zed nine names, Hermes five, Cline, Warp via `/init` linking) or a native name of its own (`CLAUDE.md`, `.cursor/rules/*.mdc`, `.clinerules`, `GEMINI.md`, `SOUL.md`). OpenCode V2 reads only `AGENTS.md`; Warp does not read `CLAUDE.md` directly.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `AGENTS.override.md` then `AGENTS.md` then `project_doc_fallback_filenames` at each level; global `$CODEX_HOME/AGENTS.override.md` else `AGENTS.md`; `/init` scaffolds one. | GA | https://learn.chatgpt.com/docs/agent-configuration/agents-md.md | unreviewed |
| OpenCode | `AGENTS.md` only in V2 (no `CLAUDE.md` fallback); an `instructions` array takes files, globs, `~/` paths, absolute paths and http(s) URLs (5 s fetch timeout). | GA | https://opencode.ai/docs/instructions/ | unreviewed (V1-to-V2 CLAUDE.md change confirmed in review) |
| Claude Code | `CLAUDE.md` / `.claude/CLAUDE.md` / `CLAUDE.local.md`; `AGENTS.md` read via a built-in plugin (v2.1.277+) only when no CLAUDE.md exists at that level (modes `claude-md-or-agents-md`, `claude-md-and-agents-md`, `claude-md`, `managed-only`); not read: `AGENTS.local.md`, `AGENTS.override.md`, `.agents/`. Whether `.claude/AGENTS.md` is read was not found in the docs (review). | GA | https://code.claude.com/docs/en/memory.md | reviewed; `.claude/AGENTS.md` claim unverified |
| Claude Desktop | Own survey documents no instruction-file mechanism for chat; Cowork projects hold instructions; the Code tab is the Claude Code binary. | GA | https://claude.com/docs/cowork/guide/projects.md | reviewed (projects) |
| DeepSeek harness | The repo `AGENTS.md` is for harness developers; the system-prompt page does not mention loading user-workspace instruction files. | unclear | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/system-prompt | unreviewed |
| Oh My Pi | Native `AGENTS.md`, `.omp/AGENTS.md`, `RULES.md` (always on), `APPEND_SYSTEM.md`, `SYSTEM.md` (replaces built-in body); compat providers for Claude `.claude/CLAUDE.md`, `.agent(s)/AGENTS.md`, Codex, Gemini, OpenCode, Copilot; Cursor and Cline rules treated as conditional rules. | GA | https://omp.sh/docs/context-files | corrected |
| Pi | `AGENTS.md`, `CLAUDE.md` and variants from the agent dir, working dir and parents; `AGENTS.override.md`; `SYSTEM.md` replaces and `APPEND_SYSTEM.md` appends the system prompt. | GA | https://pi.dev/docs/latest/configuration, https://pi.dev/docs/latest/cli | unreviewed |
| Cursor | Project Rules `.cursor/rules/*.mdc` (plain `.md` ignored), User Rules, Team Rules; also `AGENTS.md` (no frontmatter). Loads `.claude/` and `.codex/` agents, skills and hooks (cross-tool). | GA | https://cursor.com/docs/rules | reviewed |
| Antigravity | `AGENTS.md` or `GEMINI.md` in workspace folders and parents up to the repo root (not subfolders), `<dir>/.agents/AGENTS.md`, modular `.agents/rules/*.md`; global `~/.gemini/...`. | GA | https://antigravity.google/docs/rules, https://antigravity.google/docs/customization-discovery | unreviewed |
| OpenHands | `AGENTS.md` loads whole into the system prompt; `CLAUDE.md` and `GEMINI.md` recognised as model-specific repo context; legacy `.openhands/microagents/` and `.openhands/skills/`. | GA | https://docs.openhands.dev/overview/skills.md | unreviewed |
| OpenClaw | Workspace files `AGENTS.md`, `SOUL.md`, `IDENTITY.md`, `USER.md`, `BOOT.md`, `BOOTSTRAP.md` injected into the system prompt; sub-agents get only `AGENTS.md`. | GA | https://docs.openclaw.ai/concepts/agent-workspace, https://docs.openclaw.ai/concepts/system-prompt | unreviewed |
| goose | `AGENTS.md` and `.goosehints` by default; `CONTEXT_FILE_NAMES` changes the list (examples include `CLAUDE.md`, `.cursorrules`); needs the Developer extension. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md | unreviewed |
| Hermes Agent | One project file wins, first match of `.hermes.md`/`HERMES.md`, `AGENTS.override.md`, `AGENTS.md`, `CLAUDE.md`, `.cursorrules`; global identity in `SOUL.md`. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/context-files | unreviewed |
| Zed/ACP | Personal `~/.config/zed/AGENTS.md`; project file is the first match of `.rules`, `.cursorrules`, `.windsurfrules`, `.clinerules`, `.github/copilot-instructions.md`, `AGENT.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`. External agents read their own files. The page also says the Zed Agent loads `AGENTS.md`, `CLAUDE.md` and copilot-instructions, which is unclear against the nine-name list. | GA | https://zed.dev/docs/ai/instructions.md | corrected |
| Warp | Global Rules (Warp Drive) and Project Rules in `AGENTS.md` (legacy `WARP.md` wins if both exist); `CLAUDE.md` not read directly; `/init` can link it and `.cursorrules`, `GEMINI.md`, `.clinerules`, `.windsurfrules`, `.github/copilot-instructions.md`. | GA | https://docs.warp.dev/agents/capabilities/rules/ | unreviewed |
| Cline | `.clinerules/` or `.cline/rules/` (workspace), global dirs; also auto-detects `.cursorrules`, `.windsurfrules`, `AGENTS.md`, `~/.agents/AGENTS.md`; `/newrule`. | GA | https://docs.cline.bot/customization/cline-rules.md | unreviewed |
| OpenCodex | Nothing documented as user memory or instruction files; prompt layers (read/toggle/edit base prompts) exist in the management API, edits need a consent-bound dashboard session. | unverified (prompt layers not read) | https://opencodex.me/reference/management-api/ | unverified |
| rizzy-harness (ours) | The agent does not read `AGENTS.md`/`CLAUDE.md` or any repo instruction file. Per-project instructions exist only as text typed into the Projects UI (up to 4000 chars). | none | `crates/master/src/projects.rs:1-12` | from own inventory |

#### Instruction scopes, precedence, imports and conditional loading
What it is: how several instruction files combine, and when they load.
Variants: concatenation root-first, nearer wins by position (Codex, Claude Code, Antigravity "cumulative, no override order"); shadowing where one provider wins per depth (OMP); first match wins (Hermes, Zed); explicit precedence list (Cursor Team > Project > User; Warp subdirectory > root > Global; Cline workspace over global); lazy loading of subdirectory files on file access (OpenCode, Claude Code, goose, Hermes); glob-scoped rules (Cursor, Claude Code `.claude/rules`, Antigravity, Cline `paths:`); `@path` imports (Claude Code four hops, OMP five hops, goose, Antigravity).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Global, then every directory from project root to cwd, concatenated root-first so closer files come later; chain rebuilt each run; `project_root_markers` changes root detection. | GA | https://learn.chatgpt.com/docs/agent-configuration/agents-md.md, https://learn.chatgpt.com/docs/config-file/config-advanced.md | unreviewed |
| OpenCode | Global `~/.config/opencode/AGENTS.md`; upward discovery; nested `AGENTS.md` load when a file beneath is read (nearest first, not re-injected while in history, edits not detected mid-session); sources concatenated, conflicts not resolved; `OPENCODE_DISABLE_PROJECT_CONFIG=1` skips project discovery. | GA | https://opencode.ai/docs/instructions/ | unreviewed |
| Claude Code | Four scopes (managed, user, project, local) loaded broadest first and concatenated; delivered as a user message after the system prompt; subdirectory CLAUDE.md load on demand when a file there is read; `@path` imports up to four hops, out-of-cwd project imports need a one-time approval; `.claude/rules/**/*.md` with `paths:` globs load when matching files are touched; managed `claudeMd` and `claudeMdExcludes`; `--add-dir` instruction files load only with an env flag. Docs state CLAUDE.md is guidance, not enforcement. | GA | https://code.claude.com/docs/en/memory.md, https://code.claude.com/docs/en/large-codebases.md | reviewed |
| Oh My Pi | One provider wins per depth (shadowing), priority native > claude > agents = codex (tied) > gemini > opencode > github > agents-md; several compat sources are launch-directory-only or user-wide-only; `@path` imports (5 hops, cycle-safe). | GA | https://omp.sh/docs/context-files | corrected |
| Pi | A file applies to its own directory and below; `AGENTS.override.md` replaces `AGENTS.md`/`CLAUDE.md` only in the same directory; loaded without project trust (a security note); `-nc` disables. | GA | https://pi.dev/docs/latest/configuration, https://pi.dev/docs/latest/security | unreviewed |
| Cursor | Four rule modes (always, intelligent by description, glob, manual); precedence Team, then Project, then User; nested `AGENTS.md` scope to their directory, most specific wins. | GA | https://cursor.com/docs/rules | reviewed |
| Antigravity | Rules cumulative with no override order; `trigger` frontmatter (`always_on`, `model_decision`, `glob`, `manual`) and missing or invalid frontmatter silently discards the file; `@[label](path)` inlines; `rules.json` manifest; re-read before each turn; subfolder `AGENTS.md` not loaded. | GA | https://antigravity.google/docs/rules | unreviewed |
| OpenHands | `AGENTS.md` loaded whole as repo context; skills carry the conditional layer (area 13). | GA | https://docs.openhands.dev/overview/skills.md | unreviewed |
| goose | Nested files from the working directory up to the git root load at start; subdirectory files load when files there are touched, then stay; `@path` includes content; local beats global; precedence of `AGENTS.md` versus `.goosehints` not stated. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md | unreviewed |
| Hermes Agent | In git repos `AGENTS.md` files from git root to cwd load, deeper taking precedence; subdirectory files discovered lazily when tool calls touch paths (up to five parents); flagged project files are blocked, a flagged `SOUL.md` loads with a warning. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/context-files | unreviewed |
| Zed/ACP | Project wins over personal; first-match rule over the nine names; the rules library was replaced (on-demand Rules became Skills, always-on Rules became personal `AGENTS.md`). | GA | https://zed.dev/docs/ai/instructions.md | corrected |
| Warp | Root and current-directory files applied automatically, subdirectory files best-effort; precedence subdirectory, root, then Global Rules. | GA | https://docs.warp.dev/agents/capabilities/rules/ | unreviewed |
| Cline | Workspace wins over global on conflict; conditional rules via `paths:` globs load only when matching files are in context; invalid YAML fails open (rule active); `paths: []` disables. The config page does not define override order. | GA | https://docs.cline.bot/customization/cline-rules.md, https://docs.cline.bot/getting-started/config.md | unreviewed |
| OpenClaw | Owner-created standing intents (keyword-triggered, rule-based prefilter, cooldown 24h, max 3 fires, expiry 90 days) and "standing orders" written in `AGENTS.md`. | GA | https://docs.openclaw.ai/concepts/standing-intents, https://docs.openclaw.ai/automation/standing-orders | unreviewed |
| rizzy-harness (ours) | No file-based scopes; one project-level instructions field. | none | `crates/master/src/projects.rs:1-12` | from own inventory |

#### Instruction size limits and trust of loaded files
What it is: caps on how much instruction text is injected and whether a repo's own files are trusted.
Variants: byte caps combined (Codex 32 KiB default, but the pages disagree whether per file or combined), per-file plus aggregate (Antigravity, OpenClaw), launch-time skip and on-demand truncation (Claude Code), scaled-to-window with head/tail truncation (Hermes), recommended line counts only (Cursor under 500 lines, Claude Code under 200 lines).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `project_doc_max_bytes` (32 KiB default); the AGENTS.md page says combined size, the advanced config page says per file. Managed `additional_developer_instructions` up to 10,000 estimated tokens. | GA | https://learn.chatgpt.com/docs/agent-configuration/agents-md.md, https://learn.chatgpt.com/docs/config-file/config-advanced.md | unreviewed (contradiction recorded) |
| Claude Code | Launch-time files over 4 MiB skipped entirely; on-demand files truncated at 40,000 chars on 200K windows (150,000 or 200,000 on 1M windows), with a note to Read the rest (v2.1.309+); block-level HTML comments stripped; oversize warnings at startup and `/status`; recommended under 200 lines. Imports outside cwd need approval. | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| Antigravity | 24,000 bytes per rule file; 20,000-token aggregate for global/always-on rules; larger files reduced to pointers. | GA | https://antigravity.google/docs/rules | unreviewed |
| OpenClaw | Bootstrap files capped 20,000 chars each and 60,000 total; `USER.md` capped 4,000 chars. | GA | https://docs.openclaw.ai/concepts/system-prompt, https://docs.openclaw.ai/concepts/memory | unreviewed |
| goose | Per-turn persistent instructions (`GOOSE_MOIM_MESSAGE_TEXT` / `_FILE`) capped at 64 KB, re-read every turn; hints files have no stated cap. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-persistent-instructions.md | reviewed |
| Hermes Agent | Per-file cap scales with the model window, head 70% / tail 20% truncation; subdirectory discovery capped at 32,000 chars. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/context-files | unreviewed |
| Cursor | Docs recommend rules under 500 lines. | GA | https://cursor.com/docs/rules | reviewed |
| Zed/ACP | Skills catalog capped at 50KB; project skills load for trusted worktrees only. | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| Warp | No numeric limits documented. | nothing documented | https://docs.warp.dev/agents/capabilities/rules/ | unreviewed |
| Pi | Context files load regardless of project trust (documented as a security note). | GA | https://pi.dev/docs/latest/security | unreviewed |
| rizzy-harness (ours) | Project instructions at most 4000 chars. | works | `crates/master/src/projects.rs:1-12` | from own inventory |

#### Generating and auditing instruction files (`/init`, prompt audit)
What it is: commands that write or lint the instruction file.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/init` generates an `AGENTS.md` scaffold in the current directory. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Claude Code | `/init` (interactive multi-phase flow with `CLAUDE_CODE_NEW_INIT=1`); `/doctor prompt-audit` (v2.1.283+) audits CLAUDE.md, AGENTS.md, rules, skills, commands, subagents and output styles for stale or conflicting instructions; `/import` copies another agent's config once. | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| Warp | `/init` can link `CLAUDE.md` and other tools' files. | GA | https://docs.warp.dev/agents/capabilities/rules/ | unreviewed |
| Cline | `/newrule` creates a rule file. | GA | https://docs.cline.bot/customization/cline-rules.md | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Agent-written memory: storage format and what is injected
What it is: notes the agent (or a background job) writes for later sessions.
Variants: curated Markdown index injected, details read on demand (Claude Code `MEMORY.md` first 200 lines or 25 KB; OpenHands `MEMORY.md` about 6,000 chars; OpenClaw `MEMORY.md`); two tiny frozen files (Hermes 2,200 and 1,375 chars); account-side topic store (Claude chat); a hidden generated store (Codex); SQLite facts (ours); append-only journal (Cursor SDK). Mid-session writes are frozen until next session in Hermes and Claude Code (CLAUDE.md edits).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | When enabled, turns eligible idle past chats into local memory files under `$CODEX_HOME/memories_v2/` (or `memories/`); skips active or short sessions, redacts secrets, runs in the background, pauses when rate-limit remainder is low; extract and consolidation models configurable. Off by default (the 0.165.0 "memory v2 default" refers to the store version, not enablement). | GA, off by default | https://learn.chatgpt.com/docs/customization/memories.md, https://learn.chatgpt.com/docs/changelog | reviewed |
| Claude Code | Auto memory: typed notes (`user`, `feedback`, `project`, `reference`) at `~/.claude/projects/<project>/memory/`; `MEMORY.md` index (one line per memory) plus topic files read on demand; first 200 lines or 25KB of the index load each session; over-limit writes return an error telling it to rewrite the index; `modified` timestamp added; on by default locally, off by default in self-hosted environments; exempt from the transcript cleanup sweep; not loaded into subagents (except forks). | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| Claude Desktop | Chat memory: topic-based, edited per topic under Settings; replaced an earlier daily-summary design (Jul 10 2026); included in data exports. Cowork project memory is a separate local store. | GA | https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context, https://claude.com/docs/cowork/guide/projects.md | reviewed |
| DeepSeek harness | No built-in memory; a third-party MCP memory server can be connected (three default-off example configs: Memorix 1.3.0, the MCP reference memory server 2026.7.4, Engram 1.20.0). | preview (integration pattern) | https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory | unreviewed |
| Oh My Pi | Off by default. `local` backend distils past sessions into `memory_summary.md`, `MEMORY.md`, `learned.md` and `skills/` per project; `mnemopi` and `hindsight` backends. | GA (autolearn experimental) | https://omp.sh/docs/memory, https://omp.sh/docs/tools | corrected |
| Pi | Nothing documented in core; the `pi-chat` extension adds account-wide and channel-specific memory files. | unclear | https://github.com/earendil-works/pi-chat | unreviewed |
| Cursor | Editor "Memories" launched as a beta in Cursor 1.0 (changelog 2025-06-04); not in current docs; a forum reply (not labelled staff) says only the UI was removed. Automations keep notes across runs in `MEMORIES.md` (on by default, editable, disableable); the Agent SDK has a JSONL journal with no recall tool; each Grok Bot has its own memory. | unclear (editor); GA (automations, no label); unclear (SDK, Bots) | https://cursor.com/docs/cloud-agent/automations, https://cursor.com/docs/agent-sdk/reference/memory, https://forum.cursor.com/t/memory-tool-dissapeared/148405 | corrected (editor Memories); unreviewed (others) |
| Antigravity | No memory or knowledge feature documented; the `/learn` page says agents do not remember conversations and persistence is through rule and skill files. An earlier "Knowledge" feature cannot be ruled out from the sources. | not documented | https://antigravity.google/docs/commands/learn | reviewed |
| OpenHands | Opt-in (`AgentContext(load_memory=True)` or a Canvas toggle). `MEMORY.md` curated index is the only part injected (combined cap about 6,000 chars, oldest whole lines dropped first); `YYYY-MM-DD.md` daily logs read on demand. The agent writes it, typically at end of task. | GA | https://docs.openhands.dev/sdk/guides/persistent-memory.md | reviewed |
| OpenClaw | Markdown in the workspace: `MEMORY.md` (loaded at session start), `memory/YYYY-MM-DD.md` daily notes, `USER.md`, `DREAMS.md`; a silent pre-compaction turn saves notes (can be disabled). The project says memory guides behaviour but enforces no policy. | GA | https://docs.openclaw.ai/concepts/memory, https://docs.openclaw.ai/concepts/agent-workspace | reviewed (dreaming); unreviewed (rest) |
| goose | A built-in Memory extension "remembers your preferences over time"; no mechanism documented. | unclear | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | `MEMORY.md` (agent notes, 2,200 chars) and `USER.md` (user profile, 1,375 chars) in `~/.hermes/memories/`, per profile; loaded as a frozen snapshot at session start; `memory` tool actions add, patch, remove (no read or replace); over-limit writes error and force consolidation; entries scanned for injection and invisible Unicode; exact duplicates rejected. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory | reviewed |
| Zed/ACP | Nothing documented (no memory store; only instructions and skills). | unclear | https://zed.dev/llms.txt | unreviewed |
| Warp | Agent Memory shared across harnesses (Warp Agent, Claude Code, Codex): extracted after a conversation ends, retrieved in the background, kept in personal, agent and team stores. Research preview, per-team waitlist; on by default for new agents in the Oz web app; third-party harnesses only as cloud agents. | preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |
| Cline | Memory Bank is a convention: a `memory-bank/` directory of six Markdown files read at the start of each task, via custom instructions; not a built-in feature; the page looks legacy. | GA (documented convention) | https://docs.cline.bot/best-practices/memory-bank.md | unreviewed |
| OpenCode | No memory feature documented in the V2 pages opened. | nothing documented | https://opencode.ai/docs/llms.txt | unreviewed |
| OpenCodex | Nothing documented as a user memory feature. | nothing documented | https://opencodex.me/reference/management-api/ | unreviewed |
| rizzy-harness (ours) | Facts in `memory.db` (SQLite, FTS5), text up to 500 chars with hidden/bidi characters stripped; optional Qdrant vectors, optional mem0 server replacing the store, or off. | works | `crates/memory/src/lib.rs:1-12`, `:84-125`, `crates/memory/src/sqlite.rs:15-50` | from own inventory |

#### Who approves an agent memory write
What it is: whether the human or a policy gate sits between the agent proposing a memory and it entering the prompt.
Variants: pre-activation confirmation by the owner (ours; Antigravity `/learn` for rule and skill files); opt-in staging queue, default off (Hermes `memory.write_approval`, `skills.write_approval`); no approval, review after the fact via viewing and edit (Claude Code, Claude chat memory, OpenHands, Cursor automations, Codex); promotion gated by scoring plus a rewrite-loss guard (OpenClaw dreaming); consent enforced by an administrator (Warp: read-only or read-write attachments).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | No approval step documented; users control use and generation per chat with `/memories`; chats that used MCP or web search can be excluded (`memories.disable_on_external_context`). | GA | https://learn.chatgpt.com/docs/customization/memories.md | reviewed |
| Claude Code | No approval step documented; the agent writes with plain Write/Edit; `/memory` shows and edits. Behaviour under `blockReadsOutsideWorkingDirectories`: a repo-chosen memory directory is neither read nor written. | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| Claude Desktop | Stored automatically; users can pause, reset, edit and delete per topic; per-chat off switch only before the first message. | GA | https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context | reviewed |
| Antigravity | `/learn` classifies lessons as rule or skill, prefers editing existing files, drafts a `learning_proposal.md` artifact, and writes only after explicit approval; workspace or global scope chosen by the agent. | GA (App, CLI, IDE) | https://antigravity.google/docs/commands/learn | reviewed |
| OpenHands | No approval; the agent writes at the end of a task; content injected inside an untrusted-content wrapper. | GA | https://docs.openhands.dev/sdk/guides/persistent-memory.md | reviewed |
| OpenClaw | Dreaming promotes candidates to `MEMORY.md` only if they pass `minScore`, `minRecallCount`, `minUniqueQueries`; a model proposes add/merge/supersede; a rewrite dropping more than 25% of prior entries is rejected; prior `MEMORY.md` saved in SQLite. No human step. The project admits promoted memories have no time-based retention and deletion has documented gaps. | GA | https://docs.openclaw.ai/concepts/dreaming, https://docs.openclaw.ai/concepts/memory | reviewed (guard, schedule; thresholds not on page) |
| Hermes Agent | `memory.write_approval` and `skills.write_approval` (default false) stage writes for `/memory pending|approve|reject` and `/skills pending|diff|approve|reject`; `/journey` timeline of what was learned with list, edit, delete. Background self-review runs on its own cheaper model with an input-token cap. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/features/skills | reviewed |
| Warp | Memory stores attach read-only or read-write; extraction runs in the background after a conversation. | preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |
| Cursor | Automation memory viewable, editable and disableable; docs warn that untrusted input can poison it. | GA (no label) | https://cursor.com/docs/cloud-agent/automations | unreviewed |
| rizzy-harness (ours) | The model only proposes (`remember`); facts are inactive until the owner confirms them on the Memory page; users can add, edit and delete. Project facts: activation by project members is an open audit finding (S12) on `main`. `remember` is not registered on agentic nodes, so agentic sessions read but cannot propose memory. | works (S12 open) | `crates/memory/src/lib.rs:34-48`, `crates/master/src/http.rs:143-150`, `crates/master/src/lib.rs:1123-1133`, `crates/cli/src/main.rs:447-451` | from own inventory |

#### Memory scopes (user, workspace, project, repository, team, agent)
What it is: how far a memory reaches and who can see it.
Variants: per account with per-project partition and no cross-project (Claude chat); per git repository, machine-local, shared across worktrees (Claude Code auto memory); two tiers user plus workspace/project (OpenHands); global, per-project and per-project-tagged (OMP); personal, agent and team stores (Warp); per channel and workspace (Claude Tag); per profile (Hermes); per workspace directory (OpenClaw, Codex under `$CODEX_HOME`); per agent (Claude Code subagent memory `user|project|local`, Cursor Bots, Cursor automations).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | One store under `$CODEX_HOME`; per-chat use and generation toggles; scopes beyond that nothing documented. Works across CLI, Desktop and IDE hosts. | GA | https://learn.chatgpt.com/docs/customization/memories.md | reviewed |
| Claude Code | Auto memory keyed by git repo (`~/.claude/projects/<project>/memory/`), machine-local, not synced; relocatable with `autoMemoryDirectory`. Subagent `memory: user|project|local` gives `~/.claude/agent-memory/<name>/`, `.claude/agent-memory/<name>/` (shareable via version control, recommended) or `.claude/agent-memory-local/<name>/`; first 200 lines or 25KB load. Claude Tag keeps notes per channel; workspace notes are written only from public channels and read everywhere; anyone in the channel can correct them (public beta). | GA; public beta (Tag) | https://code.claude.com/docs/en/memory.md, https://code.claude.com/docs/en/sub-agents.md, https://claude.com/docs/claude-tag/concepts/how-it-works.md | reviewed (auto memory); unreviewed (subagent, Tag) |
| Claude Desktop | Global topics plus per-project memory; project chats see project memory plus non-project memory but not other projects'; memory shared with cloud Cowork only; Cowork project memory is a separate local store, not synced or shareable. | GA | https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context, https://claude.com/docs/cowork/guide/projects.md | reviewed |
| Oh My Pi | Scoping global, per-project, per-project-tagged; mnemopi per-project. | GA | https://omp.sh/docs/memory | corrected |
| OpenHands | `~/.openhands/memory/` (user) and `<workspace>/.openhands/memory/` (project). | GA | https://docs.openhands.dev/sdk/guides/persistent-memory.md | reviewed |
| OpenClaw | Per-agent workspace (default `~/.openclaw/workspace`); workspace can be a private git repo; Control UI can import Markdown memory from Codex, Claude Code and Hermes into `memory/imports/`. | GA | https://docs.openclaw.ai/concepts/agent-workspace, https://docs.openclaw.ai/concepts/memory | unreviewed |
| Hermes Agent | Per profile under `~/.hermes/memories/`; Honcho session strategy per-session, per-directory, per-repo or global. Profile distributions carry SOUL, config, skills, cron and MCP but never credentials or memory. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/features/honcho | reviewed |
| Warp | Personal, agent and team stores; each attachable read-only or read-write. | preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |
| Cursor | Per automation (`MEMORIES.md`); Agent SDK journal is one file shared across all sessions and users (docs say treat records as untrusted); each Grok Bot has its own memory. | GA (automations); unclear (SDK, Bots) | https://cursor.com/docs/cloud-agent/automations, https://cursor.com/docs/agent-sdk/reference/memory, https://cursor.com/docs/grok-bot | unreviewed |
| rizzy-harness (ours) | `user` (personal facts) and `project` (shared by its members); no per-repo or workspace-shared scope; a user's facts can be switched off per workspace. | works | `crates/memory/src/tool.rs:10-60`, `crates/master/src/lib.rs:1095-1137` | from own inventory |

#### Recall, memory back ends and embeddings
What it is: how stored memory is found again.
Variants: inject an index and let the model read files (Claude Code, OpenHands, Hermes frozen snapshot); search tools over hybrid vector and keyword stores (OpenClaw `memory_search`); pluggable providers (Hermes, OMP, DeepSeek harness via MCP, Cursor SDK `MemoryBackend`); remote embeddings only (ours).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Index loaded at start; topic files read on demand with normal file tools; no search service documented. | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| DeepSeek harness | Via a connected third-party MCP memory server. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory | unreviewed |
| Oh My Pi | `mnemopi`: local SQLite, prompt-driven remember/recall/forget, LLM mode smol/remote/none, optional vector/graph/temporal recall; `hindsight`: remote server (default http://localhost:8888) that retains text every 3 user turns (mnemopi every 4). Tool availability per backend: `recall`/`retain`/`reflect` only with hindsight or mnemopi, `memory_edit` only mnemopi, `learn` needs autolearn plus a backend. docs/memory says no named tools, docs/tools and README list five (internal inconsistency). | GA | https://omp.sh/docs/memory, https://omp.sh/docs/tools | corrected |
| OpenClaw | `memory_search` (hybrid vector plus keyword; SQLite default, PostgreSQL on some runtimes; Honcho and LanceDB options), `memory_get`; today's and yesterday's daily notes loaded on `/new`. | GA | https://docs.openclaw.ai/concepts/memory | unreviewed |
| Hermes Agent | One external provider can run alongside built-in memory (RetainDB, ByteRover bundled; Honcho, Hindsight, Supermemory, Mem0, OpenViking, Holographic via catalog). Honcho derives user insights through a 1 to 3 pass "dialectic" and injects a base context plus a dialectic supplement; tools `honcho_profile`, `honcho_search`, `honcho_context`, `honcho_reasoning`, `honcho_conclude`. Hosting details unverified. | GA (plugins) | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/features/honcho | reviewed (built-in); hosting unverified |
| Warp | Memories retrieved in the background; mechanism not described. | preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |
| Cursor | Agent SDK has a pluggable `MemoryBackend`; default journal has no recall tool, the model reads it with file tools when instructed. | unclear | https://cursor.com/docs/agent-sdk/reference/memory | unreviewed |
| rizzy-harness (ours) | Top 6 confirmed facts per message by keywords (FTS5) or by meaning (embeddings from any OpenAI-shaped `/embeddings` endpoint, default Ollama, falling back to keywords when down); injected into the per-turn context after history. Embeddings are remote only, no local model in process, by design. | works | `crates/master/src/lib.rs:1123-1133`, `crates/memory/src/embed.rs:1-70`, `docs/storage-backends.md` | from own inventory |

#### Background consolidation and self-review of memory
What it is: a non-interactive pass that extracts, merges or prunes memory outside the user turn.
Variants: idle-time extraction from past chats (Codex), batch distillation (OMP `local`), scheduled cron consolidation with gates (OpenClaw dreaming 03:00 default), post-turn self-review on a cheaper model (Hermes), after-conversation extraction (Warp).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Idle past chats become memory files; skips active or short sessions; pauses when rate-limit remainder is low; extract and consolidation models configurable. | GA, off by default | https://learn.chatgpt.com/docs/customization/memories.md | reviewed |
| Oh My Pi | `local` backend batch-distils past sessions using default and smol roles; autolearn captures explicit lessons into `learned.md`. | GA (autolearn experimental) | https://omp.sh/docs/memory | corrected |
| OpenClaw | Dreaming: default-on `memory-core` pipeline, phases light, REM, deep; only deep writes `MEMORY.md`; cron default 03:00 daily; untrusted content kept out of promotion. | GA | https://docs.openclaw.ai/concepts/dreaming | reviewed |
| Hermes Agent | After turns, a background review can write memory or update skills on its own configurable cheaper model; Curator for skills (below). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory | reviewed |
| Warp | Extraction after a conversation ends, retrieval in the background. | preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |
| Claude Desktop | Monthly recap in Settings > Reflect (topics, most active day, work observations), requires memory on. | beta | https://support.claude.com/en/articles/12138966-release-notes | reviewed |
| rizzy-harness (ours) | None in the inventory. | none | not in the inventory | from own inventory |

#### Agent-written skills as memory (procedural memory)
What it is: the agent turns lessons or procedures into reusable skill files instead of facts.
Variants: staged-and-approved proposal artifact (Antigravity `/learn`); direct agent writes with optional approval and a managed lifecycle (Hermes `skill_manage`, Curator); experimental managed skills (OMP); user-driven authoring only (Zed `/create-skill`, the agent cannot edit `SKILL.md` without explicit authorisation).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Hermes Agent | `skill_manage` creates, patches (preferred), rewrites and deletes the agent's own skills; advisory linter warns on incident-style logs, over 60 reference files or bodies over about 24k chars; writes can be staged for approval (default false). The Curator runs idle-triggered (default every 168 h after 2 h idle): deterministic phase marks unused skills stale at 14 days and archives at 30; opt-in LLM phase merges near-duplicates; only agent-created skills managed; no auto-delete, pin, lock, backups, an append-only hash ledger and `hermes curator rollback`. `/learn` makes a skill from a session. README claim "skills improve during use" is a vendor claim. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills | reviewed |
| Oh My Pi | `autolearn.enabled` (default false) captures lessons into `learned.md`; the `learn` tool can turn a lesson into a managed skill; `manage_skill` creates, improves, retires; managed skills in `~/.omp/agent/managed-skills/`. | experimental | https://omp.sh/docs/memory, https://omp.sh/docs/tools, https://omp.sh/docs/skills | corrected |
| Antigravity | `/learn` drafts a `learning_proposal.md` and writes rule or skill files only after explicit approval. | GA | https://antigravity.google/docs/commands/learn | reviewed |
| OpenClaw | A `skills` "workshop" tier of agent-learned skills appears in the load precedence; the mechanism is not documented on the page read. | unverified | https://docs.openclaw.ai/tools/skills | unverified |
| Zed/ACP | Skills authored via `/create-skill`, a skills manager or share links; the agent cannot edit `SKILL.md` without explicit authorisation. | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| rizzy-harness (ours) | No skills mechanism (docs-only in PLAN section 0). | none | `PLAN.md:83-90` | from own inventory |

#### Memory content safety (untrusted text, secrets, injection)
What it is: treating stored memory as potentially attacker-influenced input and keeping secrets out.
Variants: untrusted-content wrapper plus "do not store secrets" instruction (OpenHands); scan for injection and invisible Unicode (Hermes); redact secrets at generation (Codex); keep untrusted content out of promotion (OpenClaw); exclude sensitive topics unless opted in (Claude chat); strip hidden/bidi characters and exclude untrusted turns (ours); docs-only warnings (Cursor).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Generation redacts secrets; `disable_on_external_context` keeps chats that used MCP or web search out. | GA | https://learn.chatgpt.com/docs/customization/memories.md | reviewed |
| Claude Desktop | Sensitive topics excluded unless opted in; some data classes never saved. | GA | https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context | reviewed |
| OpenHands | Injected inside an untrusted-content wrapper; the agent is told not to store secrets and to treat contents as unverified because cloned repos can ship their own. | GA | https://docs.openhands.dev/sdk/guides/persistent-memory.md | reviewed |
| OpenClaw | Untrusted content is kept out of promotion; the project admits deletion has gaps. | GA | https://docs.openclaw.ai/concepts/dreaming | reviewed |
| Hermes Agent | Entries scanned for injection and invisible Unicode; flagged project context files blocked. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory | reviewed |
| Cursor | Docs warn untrusted input can poison automation memory; Agent SDK journal records to be treated as untrusted. | GA (no label) | https://cursor.com/docs/cloud-agent/automations, https://cursor.com/docs/agent-sdk/reference/memory | unreviewed |
| Claude Code | Memory under `blockReadsOutsideWorkingDirectories`: a repo-chosen memory dir is neither read nor written. `REVIEW.md` (review-only instructions) is read only from the base or default branch and honoured by branch-protection level. | GA | https://code.claude.com/docs/en/memory.md, https://code.claude.com/docs/en/code-review-customize.md | reviewed (memory); unreviewed (REVIEW.md) |
| rizzy-harness (ours) | Hidden/bidi characters stripped; no memory in incognito chats, for agents with `memory: false`, in untrusted (webhook) turns, or where a team member keeps memory out. | works | `crates/memory/src/lib.rs:1-12`, `crates/master/src/lib.rs:1101-1122` | from own inventory |

#### Memory and instruction opt-outs
What it is: switches that turn memory or instruction loading off, per user, chat, workspace or organisation.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Off by default; `/memories` per-chat controls. | GA | https://learn.chatgpt.com/docs/customization/memories.md | reviewed |
| OpenCode | `OPENCODE_DISABLE_PROJECT_CONFIG=1` skips project discovery (instruction files). | GA | https://opencode.ai/docs/instructions/ | unreviewed |
| Claude Code | `/memory` toggle (`autoMemoryEnabled`), `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`; cannot re-enable inside a background session or a Claude-spawned `claude`; `claudeMdExcludes`; AGENTS.md mode setting. | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| Claude Desktop | Pause, reset, per-topic delete; per-chat off before the first message; incognito chats excluded from history, memory and search; owner can turn memory off org-wide (which deletes all members' memory). | GA | https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context | reviewed |
| Oh My Pi | Memory off by default. | GA | https://omp.sh/docs/memory | corrected |
| Pi | `-nc/--no-context-files`. | GA | https://pi.dev/docs/latest/configuration | unreviewed |
| Cursor | Automation memory disableable. | GA (no label) | https://cursor.com/docs/cloud-agent/automations | unreviewed |
| OpenHands | Off by default (`load_memory=True` or Canvas toggle). | GA | https://docs.openhands.dev/sdk/guides/persistent-memory.md | reviewed |
| OpenClaw | The silent pre-compaction memory-save turn can be disabled; dreaming is default-on (how to disable not stated in the entry). | GA | https://docs.openclaw.ai/concepts/memory | unreviewed |
| Warp | Auto-memory on by default only for new agents created in the Oz web app; research preview enabled per team. | preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |
| rizzy-harness (ours) | Off via incognito chats, per-agent `memory: false`, per-workspace user-facts switch, or the store set to off. | works | `crates/master/src/lib.rs:1101-1122` | from own inventory |

#### Memory import, export and portability
What it is: moving memory or instruction setups between tools or accounts.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | External-agent memories can be imported; `/import` reads Claude Code and Cursor setups and chats (50-chat cap is CLI-only). | GA | https://learn.chatgpt.com/docs/customization/memories.md | reviewed |
| Claude Code | `/import` copies another agent's config once. | GA | https://code.claude.com/docs/en/memory.md | reviewed |
| Claude Desktop | Memory import from other assistants and export on web and desktop (all plans); the import/export article was not opened by author or reviewer. | GA | https://support.claude.com/en/articles/12123587-import-and-export-your-memory-from-claude | unverified |
| Oh My Pi | Imports Claude and Codex history; reads other harnesses' instruction, skills, MCP configs. | GA | https://omp.sh/docs/context-files | unreviewed |
| OpenClaw | Control UI imports Markdown memory from Codex, Claude Code and Hermes into `memory/imports/`. | GA | https://docs.openclaw.ai/concepts/memory | unreviewed |
| Hermes Agent | `agent-importers` migrates from OpenClaw, Claude Code, Codex CLI, Cursor, Grok Bot and OpenBot: instructions become memory entries, MCP servers and skills carried over, secrets excluded unless `--migrate-secrets`; per-item plan shown first; `--sync` re-imports. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/import-from-other-agents | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

#### Searching past sessions as memory
What it is: recall by searching stored conversation history instead of curated notes.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Hermes Agent | All CLI and messaging sessions in SQLite with FTS5; `session_search` returns stored excerpts with no LLM summarisation (README says LLM summarisation; the sessions and memory pages disagree, README likely stale); in-place compaction soft-archives pre-compaction turns so they stay searchable. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/sessions | reviewed |
| Claude Desktop | Chat search over earlier non-project chats and chats within one project (not across projects); shown as tool calls with links to the source chats; can be turned off; paid plans. | GA | https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context | unreviewed |
| rizzy-harness (ours) | Not found in sections 4 and 5 of the inventory. | not in the inventory | not in the inventory | from own inventory |

## 3. Availability matrix

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

## 4. Common across the ecosystem

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

## 5. Only one harness has it

| Feature | Harness | Why interesting or not |
|---|---|---|
| Snapcompact: older context archived as images by a vision model, no summarisation call | Oh My Pi | Avoids lossy summaries but needs a vision model and an image font; unproven benefit. |
| Configurable compaction method chain (remote, snapcompact, handoff, shake, soft) | Oh My Pi | Best fallback design in the field; `shake` needs no LLM. |
| Failure ladder 60/300/900 s with a deterministic fallback summary; "summary model smaller than main model silently drops middle turns" documented as a failure mode | Hermes Agent | A concrete failure list worth testing. |
| Summary audit (`safeguard`: identifiers, pending asks, degraded marker) | OpenClaw | Cheap quality control on summaries. |
| Compaction as a locked log sequence with crash-orphan detection | DeepSeek harness | Fits an append-only event log; relevant to restart mid-compaction. |
| Idle compaction timed to prompt-cache expiry | Claude Code | Only worthwhile if cache TTL economics apply (the exact idle time is unverified). |
| Pre-written background summary swapped in | OpenCode | Experimental; held summaries lost on restart. |
| Keyed session instructions that do not touch the cached prompt | OpenCode | Directly reusable idea for per-turn memory injection. |
| `context_edit` entries that rewrite future context without mutating history | Pi | Maps to a fold over the event log. |
| Dreaming with scoring gates and a 25% rewrite-loss guard | OpenClaw | Defensible consolidation design; also the clearest example of no human approval and no retention (project's own admission). |
| Curator for agent-written skills (stale at 14 d, archive at 30 d, lock, pin, hash ledger, rollback) | Hermes Agent | Treats self-written skills as managed assets. |
| Write-approval staging queues for memory and skills with `/journey` | Hermes Agent | Closest field example of a human gate on agent-written memory (off by default). |
| `/learn` writing a reviewable proposal artifact before any rule or skill file changes | Antigravity | Approval-gated, file-based, workspace or global scope. |
| Cross-harness shared memory (personal, agent, team stores with read-only/read-write attachment) | Warp | Preview; mechanism undocumented. |
| Frozen memory snapshot with hard character caps (2,200 and 1,375) and no auto-compaction | Hermes Agent | Forces curation and preserves the cache prefix. |
| Per-turn re-injected instructions, 64 KB cap (MOIM) | goose | Counters forgetting; extension link unverified. |
| Standing intents (keyword-triggered owner instructions, no model call) | OpenClaw | Interesting for routines; owner-only on authenticated channels. |
| Rules fail open on bad frontmatter (Cline) versus silently discarded (Antigravity) | Cline / Antigravity | Opposite failure modes of the same design; pick deliberately. |
| `REVIEW.md` read only from the base branch, honoured by branch-protection level | Claude Code | Defence against a PR editing its own review instructions. |
| Embeddings code index with ignore files from five tools | Warp | The only documented embeddings index; compute location unstated. |

## 6. Ours against the field

Ranked by how much each matters for a self-hosted master/node harness.

1. **No overflow handling or `/compact` (largest gap).** Our trigger is a char/4 estimate against a fixed 24000 and a context-length error simply errors the turn (see Automatic compaction; Staged and fallback). Fifteen of 16 harnesses key compaction to the model's actual window or a reserve, and several add reactive compact-and-retry (OpenCode, Pi, OpenClaw, OpenHands, DeepSeek harness). On nodes with different models this is a correctness bug, not a polish item. The cheapest field-proven fix is overflow-then-compact-once plus a reserve (Pi 16384, OpenCode 15%).
2. **We do not read repo instruction files.** 14 of 16 harnesses read `AGENTS.md` or equivalents (see Instruction files). For a node that "owns the code" this means the agent cannot see the repo's own rules; our per-project instruction text lives on the master, not with the code. Note the trust questions the field already hit: Pi loads context files regardless of project trust, Claude Code gates out-of-cwd imports behind an approval, OpenHands and Hermes treat repo-shipped memory as untrusted text, Hermes blocks flagged files. The node (not the master) should read these, consistent with "the node decides".
3. **Memory approval: we are ahead of most.** Owner confirmation before activation is rarer than it looks: only Antigravity (`/learn`) and Hermes (opt-in, default off) have a gate in the surveys; Claude Code, Claude chat, OpenHands, OpenClaw, Codex write without approval. Our open S12 (project members can activate project facts) is the one real hole against this lead (see Who approves). Also `remember` is not registered on agentic nodes, so the feature that matters most for a coding harness does not write there.
4. **Memory scopes: user and project only.** The field splits on scope in ways we lack: per-repository (Claude Code auto memory keyed by git repo, shared across worktrees; Hermes Honcho per-repo), workspace, team (Warp), agent (Claude subagent memory, Cursor Bots). We have no per-repo or workspace-shared scope (see Memory scopes).
5. **Observation masking, output filter and caching are strong.** Our masking (batched to protect the cache), shell filter with retrievable full output, and per-session random output ids match or beat the field (see Pruning; Large output; Prompt caching). Only OpenClaw, OMP, Hermes and goose document anything similar for pruning; Claude Code documents none. No other harness documents cache-keeping batch masking.
6. **No context-usage display against the model window.** We show a total-token tooltip only; nine harnesses show fullness or a per-category breakdown (see Context usage display). It also depends on knowing the real window, which ties to item 1.
7. **No staged compaction, idle or background compaction.** OMP's chain, Hermes' ladder and OpenClaw's summary audit show what failures look like. Lower priority than item 1, but the summary audit and "full history stays in the log" are natural fits for our event log (our `history_summarized` event already keeps the log).
8. **Repo map: present but off by default.** We are one of two with a code map or index (Warp has embeddings). Worth turning on only when its cost is measured; the field offers little evidence either way.
9. **No agent-written skills, no consolidation, no memory import/export, no session-history search.** All are niche in the field (see Sections 4 and 5). Skills as memory should wait; if added, Antigravity's proposal-artifact and Hermes' staging queue are the approval shapes.

### Evidence bearing on the four scopes and on human approval

The reader's question is how to design four scopes (workspace, project, user facts, per-repository) and whether agent-written memory needs a human approval step. What the surveys say:

- **Scope examples in the field.** Claude Code auto memory is per git repository, machine-local and shared across worktrees; Claude chat is global topics plus per-project with no cross-project reads; OpenHands has user and project tiers; Warp has personal, agent and team stores with read-only or read-write attachment; OMP has global, per-project, per-project-tagged; Claude Tag has channel and workspace notes (workspace notes written only from public channels, readable everywhere). Nobody documents all four of our scopes. A per-repository scope is only well defined where the harness runs next to the code (the node); a master-side store cannot key it by git repo without the node reporting an identity.
- **Isolation rules to copy.** Claude chat: project chats see project memory plus non-project memory, not other projects' (a one-way containment rule). Warp: attachment is per store and read-only or read-write. Hermes: profile distributions carry config and skills but never credentials or memory.
- **Approval: mixed evidence.** Gate: Antigravity `/learn` and ours write only after approval; Hermes offers a staging queue defaulting to off. No gate: Claude Code, OpenHands, Codex, Claude chat, OpenClaw (scored promotion only). Failure evidence from the field for no-gate designs: Cursor's docs warn that untrusted input can poison automation memory and that the SDK journal is shared across all sessions and users; OpenClaw admits promoted memories have no retention limit and deletion has gaps; OpenHands and Hermes treat injected memory as untrusted text and scan it. Nothing in the surveys measures how often users accept or reject a proposal, or whether an approval step is abandoned in practice; that is unverified. A design implication that follows from the sources: the shared scopes (project, workspace, team) carry the poisoning risk across users, so an approval step is better justified there than for a single user's private facts. This is an inference, not a finding from a survey.
- **Size discipline.** Hard caps force curation: Hermes (2,200 / 1,375 chars), OpenHands (about 6,000 chars, oldest lines dropped), Claude Code (200 lines or 25 KB, error on overflow forcing a rewrite), OpenClaw `USER.md` 4,000 chars. Ours: 500 chars per fact, top 6 recalled.
- **Injection placement.** Injected after history to keep the prefix stable (ours); frozen at session start with mid-session writes visible only next session (Hermes, Claude Code CLAUDE.md edits); wrapped as untrusted content (OpenHands).
- **Exclusions.** Incognito, per-chat off switch, untrusted turns and external-context chats (Codex) are the documented exclusion points; ours already has similar ones.

## 7. Corrections, contradictions and unverified claims

Corrections applied (from reviews):
- Claude Code idle compaction: the survey had three conflicting entries; review kept "200,000 tokens", "one-hour cache TTL" and `idleCompaction: false (v2.1.290+)`, and moved "about 54 minutes", "Terminal 2.1.299, Oct 14, 2026", "/config row 2.1.302", "about an hour idle", "machine awake" and the 100,000 floor to Unverified.
- Claude Code `promptCacheTtl`/`subagentPromptCacheTtl` version is v2.1.242 (docs page), not 2.1.243.
- Claude Code `.claude/AGENTS.md` being read was not found in memory.md (Unverified).
- Oh My Pi: idle compaction is off by default, not "optionally on"; 20k keep-recent is a summarising-compaction target; `agents` and `codex` instruction providers are tied; scope limits added; per-backend memory tool availability and mnemopi retain interval (every 4 turns) added.
- Zed/ACP instructions: first match wins, in the listed order of nine files; project `.rules` still supported.
- goose tool deferral: mechanism is on the config-files page (names up front, schemas via `enable_tools`), not undocumented.
- Cursor: Memories launched in 1.0 beta (changelog); the forum reply was not shown as staff and two other threads were not opened.
- OpenCodex: the 256 MiB memory accounting figure was removed (not found).
- Pi's "under 1,000 tokens" prompt claim is from a 2025 post, not current docs.

Contradictions between a vendor's own pages:
- Codex `project_doc_max_bytes`: combined (AGENTS.md page) vs per file (config-advanced page). Codex memories "off by default" vs 0.165.0 "memory v2 default for fresh profiles" (the latter is the store version).
- Hermes: sessions page says compression creates a child session, compression page says in-place is the default (`in_place: true`); README says FTS5 session search uses LLM summarisation, memory and sessions pages say it does not; prompt-assembly page puts the skills index in two tiers.
- Oh My Pi: docs/memory says no named memory tools, docs/tools and README list `retain`/`recall`/`reflect`/`memory_edit`/`learn`; README says a `skill://` scheme exists, skills doc documents none; whether TTSR injections survive compaction (README claim) is unverified.
- Antigravity: `/context` page does not mention `/compact` while the changelog does.
- Zed: the Instructions page lists nine files and also says the Zed Agent loads `AGENTS.md`, `CLAUDE.md` and copilot-instructions; unclear whether `.rules` and the legacy names are still loaded by the Zed Agent.
- OpenClaw: dreaming is default-on yet promoted memories have no time-based retention (project's own admission); its page lists `TOOLS.md` and `HEARTBEAT.md` that the file map does not.
- OpenHands: `max_size` vs `max_context_length` for the same value; the guide gives no product defaults while the architecture page gives 120/4.
- Cline: Auto Compact and Memory Bank pages appear legacy (not in the llms.txt index); Auto Compact page still mentions todo lists after Focus Chain was deprecated.

Not confirmed by anyone:
- Claude Code: which model summarises; the 100K-1M window range and `CLAUDE_CODE_MAX_CONTEXT_TOKENS` detail (page truncated); whether old tool results are cleared.
- Cursor: whether Memories still produce agent-visible memories; codebase indexing, embeddings and `.cursorindexingignore`.
- Antigravity: whether an earlier Knowledge feature existed; algorithm and thresholds of compaction; `/codesearch`.
- goose: the context window report and "context relevance operation" (v1.58.0 notes); the Memory extension mechanism; whether the "Top of Mind" extension is the MOIM persistent-instructions feature.
- Warp: where embeddings are computed; Agent Memory retrieval mechanism.
- Claude Desktop: memory import/export article (not opened); Hermes Honcho hosting details; OpenClaw `workshop` skills tier mechanism; OpenCodex prompt layers; DeepSeek harness compaction thresholds (generated config catalogue not opened).
- Pi: mechanism of idle cache warming; how `context_edit` is created.
- OpenClaw dreaming numeric thresholds are not on the page (review).

## 8. Questions for Round 2

Source-code-settled questions for the open-source harnesses.

1. **Codex, memories**: what exactly is in an extracted memory file, how it is injected into a turn, and where redaction runs (`memories_v2` store); does `disable_on_external_context` apply at generation or at use?
2. **Codex, AGENTS.md**: is `project_doc_max_bytes` per file or combined, and are empty or override files counted?
3. **Codex, compaction**: what `model_auto_compact_token_limit` scope `body_after_prefix` counts, and what failed-compaction recovery does.
4. **OpenCode, compaction**: how the buffer, `keep.tokens` and summary reuse of the prompt cache are built; how the pre-written background summary is invalidated and what is lost on restart; where keyed session instructions are placed.
5. **OpenCode, instructions**: how nested `AGENTS.md` are tracked as "already in history" and re-injected after compaction.
6. **Oh My Pi, compaction**: how `methodOrder` falls through on failure, how snapcompact renders and re-reads images, and what survives (including TTSR injections).
7. **Oh My Pi, memory**: how the `local` distillation chooses and writes `memory_summary.md`/`MEMORY.md`/`learned.md`, whether `learn` and `manage_skill` writes need approval, and which memory tools exist per backend.
8. **Pi, compaction and `context_edit`**: how cut points, split-turn summaries and `context_edit` are implemented and whether users can create the latter; how idle cache warming decides.
9. **OpenClaw, dreaming and compaction**: the real thresholds (`minScore`, `minRecallCount`, `minUniqueQueries`), how the 25% rewrite guard works, how `safeguard` audits summaries, and the retention and deletion gaps.
10. **OpenClaw, workshop skills**: what the agent-learned skills tier is and who writes it.
11. **Hermes, memory and curator**: how the frozen snapshot, injection scan and `write_approval` staging are enforced; the in-place compaction lineage (`in_place` default); how the Curator lock blocks agent writes including terminal writes.
12. **Hermes, compression**: whether a manual compress command exists; the real behaviour when the summary model's window is smaller than the main model's.
13. **goose, compaction and memory**: how `GOOSE_TOOL_CALL_CUTOFF` and background tool-output summarisation work; what the built-in Memory extension stores; whether "Top of Mind" equals MOIM; precedence of `AGENTS.md` versus `.goosehints`.
14. **OpenHands, condenser and memory**: how `Condensation` events are filtered into the view and how the 6,000-char index trimming and untrusted wrapper are built.
15. **DeepSeek harness, compaction and spill**: numeric thresholds, how the locked sequence recovers from a crash orphan, and the config catalogue defaults.
16. **Zed, instructions and compaction**: which of the nine instruction files the Zed Agent actually loads; how the threshold syntax (percent, tokens, negative) is parsed.
17. **Cline, compaction**: what `basic` versus `agentic` compaction do and whether the Auto Compact and Memory Bank pages describe the current harness.
18. **Cursor (closed, public surface only)**: not source-settleable; the SDK `MemoryBackend` interface and automation `MEMORIES.md` handling are the only open-ish parts.

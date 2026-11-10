# rizzy-harness: feature inventory (own harness, as it is today)

- Date: 2026-11-10
- Commit read: `a268048` (branch `main`, working tree clean)
- Method: code under `crates/` and `web/src` is ground truth. `README.md`, `PLAN.md`, `docs/*.md`, `AUDIT.md`, `FIX_PLAN.md` were read second and checked against code. Citations were opened as `path:line`. Nothing was run or compiled; statuses mean "code and (where noted) tests exist", not "observed working".
- Status legend: `works` = code and tests exist; `partial`; `stub` = UI/setting/field exists, nothing behind it; `docs-only` = described, not found in code; `none`.
- Scale: ~85k lines of Rust, 672 `#[test]`/`#[tokio::test]` attributes under `crates/` (counted, not run); AUDIT reports 776 web tests.
- Unmerged branches (`integration/gate3`, `feat/agent-roster`) are described ONLY in the "Unmerged branches" section near the end, never in the main inventory.

Big picture that the README only half says: the product is a ChatGPT-style personal assistant (chat on the master, with web search and memory) plus a separate agentic mode (shell and file tools on a worker node). The two runtimes have **different tool sets**: the chat runtime has only web tools and `remember`; the agentic runtime has only file/shell tools. See sections 2 and 15.

---

## 1. Agent loop and execution

| Item | What exists | Status | Cite |
|---|---|---|---|
| Single-agent loop | One native loop: build messages, stream a completion, run the tool calls, repeat until a message has no tool calls. | works | `crates/agent-native/src/lib.rs:542-877` |
| Step limit | Hard cap of 50 model calls per turn; hitting it ends the turn with an error "stopped after 50 steps". Not configurable. | works | `crates/agent-native/src/lib.rs:33`, `:650`, `:872-876` |
| Streaming | Provider text deltas are coalesced (75 ms or 2 KiB) into `assistant_delta` events. Tool output streams live too. Reasoning/thinking text is not surfaced (only `Chunk` content is read). | works | `crates/agent-native/src/lib.rs:29-31`, `crates/agent-native/src/llm.rs:233-254` |
| Cancellation | Per-turn `CancellationToken`; bash kills its process group (SIGTERM, 2 s, SIGKILL). The loop deliberately does not abandon a running tool future (a never-firing token is used), so a non-bash tool that ignores cancel can block the turn (AUDIT S10; fixed only on `integration/gate3`). | partial | `crates/node/src/session.rs:865-869`, `crates/agent-native/src/lib.rs:1145-1163`, `crates/tools/src/bash.rs:176-203` |
| Tool timeout | Default 600 s per tool call, `timeout_secs` argument for bash capped at 3600. | works | `crates/agent-native/src/lib.rs:68`, `crates/tools/src/bash.rs:103-121` |
| Parallel tool calls | Not parallel: `calls` run one after another in order. Only `delegate` calls in the same step run in parallel. | works (sequential) | `crates/agent-native/src/lib.rs:744-870` |
| Token/cost budget per turn | None for the main agent. Only sub-agent caps (count and tokens) exist. The agent field `budget: lean/standard` is parsed and serialised but nothing in the loop reads it. | stub (`budget`), none (turn budget) | `crates/core/src/agent.rs:34-40`, `:102`; grep finds no consumer |
| Mid-turn steering / queued messages | None: a prompt while a turn runs is rejected with "a turn is already running". | none | `crates/node/src/session.rs:297-299` |
| Retry/backoff on provider errors | None. A model-level failure before any output moves to the next fallback model; otherwise the turn errors. | none (fallback only) | `crates/agent-native/src/lib.rs:693-701`, `crates/agent-native/src/provider.rs:104-116` |
| Crash isolation | The backend runs in its own spawned task so a panic ends the turn instead of the node. A restart mid-turn closes the turn as `interrupted` and denies pending approvals. | works | `crates/node/src/session.rs:415-449`, `:154-191` |
| Per-session loop invariants | One turn at a time per session; one writer appends events with contiguous `seq`. | works | `crates/node/src/session.rs:36`, `:194-230` |
| Usage accounting | Input/output/cache read/cache write tokens summed per turn into `turn_ended.usage`; UI shows a tooltip total per chat. No dollar cost anywhere. | partial | `crates/agent-native/src/llm.rs:246-253`, `web/src/components/Conversation.tsx:293-297` |

## 2. Tools

Registered for agentic nodes: `builtin()` plus `graph_query` (`crates/cli/src/main.rs:447-451`, `crates/tools/src/lib.rs:385-396`). Registered for the master's chat runtime: web tools plus `remember` (`crates/cli/src/main.rs:255-259`, `crates/master/src/providers.rs:66-96`). Tools are not loaded on demand; every allowed tool's schema is sent each step (PLAN §0 "schemas loaded on demand" is docs-only).

| Tool | What it does | Risk | Runs where | Status | Cite |
|---|---|---|---|---|---|
| `bash` | `bash -c` in the session folder, own process group, allow-listed env, combined stdout/stderr, exit code; optional output filtering. No sandbox on `main`. | Mutating (asked in `ask`) | agentic node | works | `crates/tools/src/bash.rs:86-259` |
| `read` | Text file with line numbers, offset/limit; 400-line window when bounded reads are on. | ReadOnly | agentic node | works | `crates/tools/src/fs.rs:15-100` |
| `write` | Create/overwrite, makes parents, no-follow open, refuses anything under `.git`. | Mutating | agentic node | works | `crates/tools/src/fs.rs:103-148`, `crates/tools/src/lib.rs:149-181` |
| `edit` | Exact-string replace (must match once unless `replace_all`); card shows a diff. | Mutating | agentic node | works | `crates/tools/src/fs.rs:150-220`, `web/src/components/ToolCard.tsx:125-131` |
| `grep` | Regex search, `.gitignore`-aware, skips `.git` and the data dir; 200 results (60 bounded). | ReadOnly | agentic node | works | `crates/tools/src/search.rs:56-160` |
| `glob` | Glob search newest first, 500 results (150 bounded). | ReadOnly | agentic node | works | `crates/tools/src/search.rs:164-230` |
| `tool_output` | Re-read the full text of a filtered or capped earlier command output, by id, line range or `contains`. Only offered when output filtering is on. | ReadOnly | agentic node | works | `crates/tools/src/outputs.rs:150-190` |
| `outline` | tree-sitter definitions of a file with line numbers (Rust, Python, JS, TS, Go). Offered only with `outline_tool` and not in chats. | ReadOnly | agentic node | works | `crates/tools/src/outline.rs:10-60`, `crates/codemap/src/lib.rs:1-14` |
| `publish` | Stage a file or folder (folder becomes `.tar.gz`) for the master to pull into Artifacts; capped per turn. | Mutating | agentic node | works | `crates/tools/src/publish.rs:48-80`, `crates/node/src/artifacts.rs:1-12` |
| `graph_query` | Asks a third-party `graphify` knowledge graph a question. Needs `graphify = true` in `node.toml`, an installed binary and the `graphify` saving. | ReadOnly | agentic node | partial (optional, off by default) | `crates/tools/src/graphify.rs:160-215`, `crates/node/src/policy.rs:21-24` |
| `delegate` | Not a `Tool`; synthesised by the loop for agents with `delegates`. Runs a sub-agent in its own child session and returns its final answer. | n/a (approval is inside the child) | agentic node, same machine | works | `crates/agent-native/src/lib.rs:167-216`, `:233-317` |
| `web_search` | SearXNG or Brave search; results carry citation labels. | ReadOnly | master chat runtime only | works | `crates/web/src/lib.rs:436-540` |
| `web_fetch` | Fetch a public URL, readability to Markdown, provenance and SSRF guards (section 15). | ReadOnly | master chat runtime only | works | `crates/web/src/lib.rs:296-420` |
| `remember` | Proposes a fact (user or project scope); unusable until a human confirms it. | ReadOnly | master chat runtime only | works | `crates/memory/src/tool.rs:10-60` |

Notes and gaps:
- There are **no web tools and no memory tool in agentic sessions**: the agentic node's tool list never includes `web_search`/`web_fetch`/`remember` (`crates/cli/src/main.rs:447-451`). The built-in `scout` and `writer` agents still list web tools (`crates/agents/builtin/scout.md:4`, `writer.md:4`), which therefore do nothing there. (README's "Token savings" table says `web_readability` applies to "chat + agentic": only chat has `web_fetch`.)
- Chat sessions have no file or shell tools at all (PLAN T11).
- Missing tools versus mainstream agents: no `ask_user` tool (questions are `ui:choices`/`ui:form` blocks in message text, not a tool; see 10), no todo/plan tool, no notebook edit, no multi-edit/patch tool, no LSP/diagnostics tool, no browser/computer use, no image generation, no MCP-provided tools (section 9).
- Mutating status is per tool, not per argument: `ask` mode asks for every `bash` call, including `ls`.

## 3. Permissions, approvals and sandboxing

| Item | What exists | Status | Cite |
|---|---|---|---|
| Approval modes | `ask_all` (everything), `ask` (default; mutating tools), `auto` (nothing). A request can only tighten what the node allows (`min`). | works | `crates/core/src/policy.rs:5-31`, `crates/node/src/policy.rs:226-232` |
| Approval flow | Mutating call emits `approval_requested`; the turn blocks on a oneshot until the UI answers; cancel or restart denies. Every decision is an event. | works | `crates/node/src/session.rs:955-1012`, `:154-191` |
| "Allow for this session" | The UI button says "for this session", but the allowance lives in a local variable of one `run_turn`: it covers one turn, and keys on tool name only, so one click allows every later `bash` command of that turn (AUDIT L10). The label overstates it (fixed/relabelled only on `integration/gate3`). | partial (label mismatch) | `crates/agent-native/src/lib.rs:625`, `:1104-1122`, `web/src/components/ToolCard.tsx:234-236` |
| No per-command rules | No allow/deny patterns for bash commands, no path rules, no persistent "always allow". | none | n/a |
| Node policy (`node.toml`) | Authoritative; unknown keys rejected. Keys: `max_approval_mode`, `workspace_roots` (default: `$HOME`), `denied_tools`, `allow_outside_workspace`, `checkpoints_keep`, `graphify`, `[artifacts]`, `env_pass`, `[limits] tool_stream_bytes`. No HTTP or protocol endpoint writes it. | works | `crates/node/src/policy.rs:9-105`, `:112-126` |
| Denied tools | Listed tools are refused with a recorded refusal. | works | `crates/node/src/session.rs:957-975` |
| Workspace confinement of file tools | Paths walked component by component with lstat; dangling or escaping symlinks refused; `write`/`edit` do not follow a final link and refuse `.git/**`; data directory refused in every mode. | works | `crates/tools/src/lib.rs:111-181`, `crates/tools/src/fs.rs:222-262` |
| Session folder rules | cwd must be inside a `workspace_roots` entry and must not contain, equal or sit inside the node's data dir; rechecked before every prompt and restore. | works | `crates/node/src/policy.rs:155-215`, `crates/node/src/session.rs:255-260`, `:292` |
| Environment handling | Child processes start from an empty env plus `PATH HOME USER LOGNAME SHELL LANG LANGUAGE LC_* TERM COLORTERM TZ TMPDIR` and `env_pass` globs; provider key vars, `.env` names and `HARNESS_*` are always stripped. | works | `crates/tools/src/env.rs:1-135` |
| Secret redaction | Known secret values scrubbed from tool output, model text and event payloads. A courtesy only (split or encoded keys pass). | partial | `crates/agent-native/src/lib.rs:805`, `:849`, `crates/node/src/session.rs:194-215`, PLAN T3 |
| Shell sandbox | **None on `main`.** `bash` runs as the node's OS user; it can read the data directory. Compensating controls: admins-only embedded node by default, `denied_tools`. Landlock sandbox exists only on `integration/gate3`. | none (on main) | `README.md` "The shell is not confined yet", AUDIT S1, `crates/master/src/config.rs:149-165` |
| Who may run agentic sessions | Per-user `can_agentic` grant AND workspace role with `agentic` permission; embedded node restricted to admins by default (`[nodes] local_node_users`). Rechecked on every prompt/approval/restore. | works | `crates/master/src/workspaces.rs:18-75`, `crates/master/src/config.rs:149-165`, PLAN T11 |
| Auto-approval by judge model | Described in PLAN §0 "Agent backends". No code. | docs-only | `PLAN.md:143-152` |
| Rendering safety | No model HTML, no remote images, links show real host, `ui:` blocks validated in Rust. | works | `crates/core/src/ui/mod.rs:56-83`, `README.md` "Rich messages" |

## 4. Context management

| Item | What exists | Status | Cite |
|---|---|---|---|
| History summarisation | When estimated history exceeds `[chat] summarize_after_tokens` (default 24000; 0 = never), everything but the last 2 turns is summarised once (<=300 words) and stored as a `history_summarized` event, reused afterwards. Trigger is a char/4 estimate against a fixed number, NOT the model's real context window or measured usage. Skipped when the kept turns alone are large. | works | `crates/agent-native/src/lib.rs:76-98`, `:570-600`, `:1036-1078`, `crates/agent-native/src/provider.rs:135-155` |
| No overflow handling | If a provider returns a context-length error the turn simply errors; there is no reactive compaction or `/compact`. | none | `crates/agent-native/src/lib.rs:702-708` |
| Observation masking | Tool outputs older than the newest `keep..2*keep` (default keep 8, >=400 chars) are replaced by a one-line note, in batches to keep the prompt cache valid. | works | `crates/agent-native/src/mask.rs:15-60`, `crates/core/src/savings.rs:48-67` |
| Shell output filter | Strip ANSI, last state of `\r` progress lines, drop build/download noise, replace passing tests with a count, collapse repeats; full text stored (2 MB, per-session random ids) and retrievable with `tool_output`. | works | `crates/tools/src/filter.rs:1-40`, `crates/tools/src/outputs.rs:15-25`, `crates/tools/src/bash.rs:206-232` |
| Output caps | Model sees <=30,000 chars per tool call (head+tail); live stream per call <=1 MiB (configurable 4 KiB-1 GiB); web page <=20,000 chars; read window 400/2000 lines. | works | `crates/tools/src/lib.rs:37`, `crates/tools/src/stream.rs:16-20`, `crates/web/src/lib.rs:32-35` |
| Repo map | tree-sitter tags + PageRank ranked map of definitions (Rust, Python, JS, TS, Go), token budget 256-8192, cached 1 h per folder, injected into the system prompt. Off by default. | works (opt-in) | `crates/codemap/src/lib.rs:1-14`, `:176-179`, `crates/agent-native/src/lib.rs:884-909` |
| Prompt caching | Anthropic `cache_control` breakpoints on the system message and the newest message; per-turn context (memory, project brief) is appended after the history to keep the prefix byte-stable. OpenAI-style providers cache by themselves. On by default. | works | `crates/agent-native/src/llm.rs:102-142`, `crates/agent-native/src/lib.rs:600-608` |
| Answer style | `off/lite/full/ultra` terse-output prompt fragments (chats capped at lite). | works | `crates/agent-native/src/style.rs:1-40`, `crates/core/src/savings.rs:9-18` |
| Token meter | Total in/out/cache tokens per chat as a tooltip; no live context-fullness indicator against the model window (PLAN §0 promised a "token meter"). | partial | `web/src/components/Conversation.tsx:293-297` |
| Project/instruction files | The agent does NOT read `AGENTS.md`/`CLAUDE.md` or any repo instruction file. Per-project instructions exist only as text typed into the Projects UI. | none (see 9) | grep finds no reader; `crates/master/src/projects.rs:1-12` |

## 5. Memory

| Item | What exists | Status | Cite |
|---|---|---|---|
| Scopes | `user` (personal facts) and `project` (facts about a project, shared by its members). No per-repo or per-workspace-shared scope; a user's facts can be switched off per workspace. | works | `crates/memory/src/tool.rs:10-60`, `crates/master/src/lib.rs:1095-1137`, `README.md` "Projects" |
| Storage | Built-in `memory.db` (SQLite) with FTS5; optional Qdrant for vectors; optional mem0 server replacing the store; or off. Fact text <=500 chars, hidden/bidi characters stripped. | works | `crates/memory/src/lib.rs:1-12`, `:84-125`, `crates/memory/src/sqlite.rs:15-50`, `crates/memory/src/qdrant.rs`, `crates/memory/src/mem0.rs` |
| Recall | Top 6 confirmed facts per message, by keywords (FTS5) or "meaning" (embeddings via any OpenAI-shaped `/embeddings` endpoint, default Ollama; falls back to keywords when down). Injected into the per-turn context after history. | works | `crates/master/src/lib.rs:1123-1133`, `crates/memory/src/embed.rs:1-70`, `docs/storage-backends.md` |
| Who may write | The model only proposes (`remember`); facts are inactive until the owner confirms them on the Memory page. Users can add, edit, delete. Project facts: activation by project members is an open audit finding (S12) on `main`. | works (S12 open on main) | `crates/memory/src/lib.rs:34-48`, `crates/master/src/http.rs:143-150`, AUDIT S12 |
| Exclusions | No memory in incognito chats, for agents with `memory: false`, in untrusted (webhook) turns, or in team workspaces where the member keeps memory out. | works | `crates/master/src/lib.rs:1101-1122` |
| Memory in agentic sessions | Recall is injected (master side) but `remember` is not registered on agentic nodes, so agentic sessions can read but not propose memory. | partial | `crates/master/src/lib.rs:1123-1133`, `crates/cli/src/main.rs:447-451` |
| Embeddings | Remote only (URL); no local model in process, by design. Presets fast/balanced/best map to Ollama model names. | works | `crates/memory/src/embed.rs:12-40` |

## 6. Sessions

| Item | What exists | Status | Cite |
|---|---|---|---|
| Persistence / event log | Append-only SQLite log per session (WAL, fsync Full on chat db, Normal on mirror); the UI view is a fold over events. Streaming deltas are stored. | works | `crates/store/src/lib.rs:253-262`, `:459-466`, `crates/core/src/view.rs:113-147`, `crates/core/src/event.rs:59-80` |
| Node/master mirror | The owning node is the only writer; the master mirrors events, rejects events for sessions another node owns. | works | `crates/node/src/session.rs:194-230`, `docs/security-node-link.md` |
| Resume | A chat is continued by sending another prompt (history rebuilt from the log, interrupted tool calls closed synthetically). Link reconnect replays events by `Resume{after}`. There is no "resume last session" CLI flag because there is no CLI chat. | works | `crates/agent-native/src/lib.rs:333-392`, `crates/node/src/lib.rs:323-330`, `:590-624`, `crates/master/src/ws.rs:1-8` |
| Fork / branch a conversation | None. (Parallel answers are variants of one message, not forks; no "edit and re-run from here".) | none | grep for fork finds nothing |
| Parallel answers (variants) | Same prompt sent to up to 4 models, no tools, side by side; the user picks one and history follows the choice. Chat mode only, not agentic. | works | `crates/agent-native/src/lib.rs:930-1030`, `crates/node/src/session.rs:279-281`, `:533-563`, `web/src/components/Conversation.tsx:387-395` |
| Replies to forms/questions | `ui:choices`/`ui:form` answers are checked against the log and sent as the next message. | works | `crates/node/src/session.rs:345-379`, `crates/core/src/ui/answer.rs` |
| Checkpoints / undo / rewind | Each agentic turn snapshots the folder before and after into a harness-owned shadow git repo (`<data>/checkpoints/<hash>.git`), never touching the user's `.git`; undo one turn or rewind to a turn; latest 10 kept (`checkpoints_keep`). Works for non-git folders too. | works | `crates/node/src/session.rs:476-530`, `:567-651`, `crates/node/src/git.rs:41-60`, `:431`, `:659-800`, `web/src/components/Conversation.tsx:43-80` |
| Incognito | Chat mode only: excluded from search, similar-chat suggestions, memory, images; purged after `incognito_ttl_minutes` idle (default 1440). | works | `crates/node/src/session.rs:289-291`, `crates/node/src/lib.rs:572-589`, `crates/master/src/lib.rs:1319-1350`, `crates/store/src/lib.rs:735`, `:834-840` |
| Search | FTS5 over titles/messages, plus Media/Artifacts/Projects tabs, per workspace, own chats only; "similar earlier chats" under the home bar. | works | `crates/master/src/search.rs:1-6`, `:371`, `crates/master/src/http.rs:40`, `docs/sidebar-search.md` |
| Delete | Deleting a chat overwrites events (secure delete), clears media keys. | works | `crates/node/src/lib.rs:537-571`, `crates/store/src/lib.rs:485-490` |
| Crash recovery | Open turns closed as `interrupted` on load. | works | `crates/node/src/session.rs:154-191` |
| Event log growth | Every streaming delta is kept forever (AUDIT D5); no compaction of the log itself. | partial (known weakness) | AUDIT D5 |
| Export/import transcripts | No export or import feature found (only copy-message and "Copy CSV" for tables). | none | no route in `crates/master/src/http.rs:26-175` |

## 7. Agents and sub-agents

| Item | What exists | Status | Cite |
|---|---|---|---|
| Agent definitions | Markdown with YAML frontmatter: `name`, `description`, `tools: [..]|all`, `model`, `effort`, `budget`, `ui`, `delegates`, `max_subagents`, `max_subagent_tokens`, `memory`, `project_memory`, `available_as`. Unknown fields rejected. | works (`budget` stub) | `crates/agents/src/lib.rs:1-33`, `:74-103` |
| Built-ins on `main` | `general` (all tools), `coder`, `writer`, `scout` (read-only), `ultracode` (orchestrator; `delegates: coder, scout, writer`; 4 sub-agents / 200k tokens). | works | `crates/agents/src/lib.rs:50-56`, `crates/agents/builtin/*.md` |
| Custom agents | Files in `<data>/agents/<id>.md` (a built-in's id replaces it); admins can also create agents in the UI (stored in master DB as feature switches), edit summary/extra instructions of built-ins, duplicate. Per-agent model, fallbacks and avatar in settings. | works | `crates/master/src/agent_edits.rs:1-12`, `crates/master/src/http.rs:129-137`, `docs/agent-pages.md` |
| Delegation | `delegate(agent, task)` starts a child session (`parent_id`) on the same node, same approval mode, returns its final answer; depth is one (children have no delegates). | works | `crates/agent-native/src/lib.rs:233-317`, `crates/node/src/session.rs:654-763`, `crates/core/src/agent.rs:114-118` |
| Parallelism | Several `delegate` calls in one step run concurrently (`join_all`). | works | `crates/agent-native/src/lib.rs:796-801` |
| Caps | Per turn: sub-agent count (default 4) and combined tokens (default 200,000), shown to the model and refused with a cap message. Tokens are checked between steps, so a step can overshoot. | works | `crates/core/src/agent.rs:117-130`, `crates/agent-native/src/lib.rs:750-775` |
| Worktrees | A sub-agent that can write/edit/bash and sits in a git repo gets its own git worktree seeded with the working state; changes are applied back as a patch, conflicts reported (not applied). Refused if the repo's config defines filter commands. | works | `crates/node/src/session.rs:765-856`, `crates/node/src/git.rs:861-960`, PLAN T19 |
| Sub-agent UI limits | Sub-agent replies are capped to `display` level (no questions/forms). | works | `crates/core/src/ui/mod.rs:132-145` |
| Switching/mentions | `@mention` routes one turn; `set_agent` records `agent_changed`; each turn records an agent snapshot hash. | works | `crates/master/src/lib.rs:300-336`, `:1069-1077`, `crates/node/src/session.rs:263-270` |
| Agent-level tool narrowing | `tools:` list narrows what is offered; node policy still decides execution. | works | `crates/agent-native/src/lib.rs:176`, `:1089-1097` |
| Skills attached to agents | PLAN §0 says agents get "attached skills". No skills mechanism exists. | docs-only | `PLAN.md:83-90`, `grep -i skill` finds none |
| Autonomous background agents | Only routines (section 11); no long-running spawned background tasks inside a chat. | none | n/a |

## 8. Models and providers

| Item | What exists | Status | Cite |
|---|---|---|---|
| Provider kinds | Two wire formats via the `genai` crate: Anthropic Messages (API key only) and OpenAI-compatible chat completions (OpenAI, OpenRouter, DeepSeek, Ollama, vLLM...). No native Gemini/Bedrock/Vertex adapter. | works | `crates/agent-native/src/llm.rs:1-27`, `:47-100` |
| Multiple providers | Many configured at once; model ids `<provider>::<model>`; routed by prefix; hot-swappable without restart. Presets for popular platforms. | works | `crates/agent-native/src/llm.rs:326-380`, `crates/master/src/presets.rs`, `crates/master/src/http/providers.rs` |
| Model capabilities | Context, max output, vision, tools, effort read from `/models` (Ollama `/api/show`); daily refresh; vanished models marked `unavailable` and kept. | works | `crates/agent-native/src/models.rs:1-30`, `crates/master/src/model_cache.rs:1-60` |
| Selection | Order: bar pick, agent's model, user default, workspace default, server default; per-agent model and per-workspace overrides. | works | `crates/master/src/resolver.rs:1-10`, `crates/master/src/lib.rs:951-1022` |
| Fallback chains | Per-agent ordered fallbacks; tried when the model is unavailable or fails before any output with a model-level error (404/408/429/5xx/connect); the chat records `model_fallback`. Not applied mid-stream. | works | `crates/agent-native/src/lib.rs:503-534`, `:640-701` |
| Reasoning effort | `low/medium/high/max` per agent or per message, sent only to models flagged `effort`. Anthropic models are forced to `effort=false` (thinking + tool replay unsupported, PLAN R8). | works (Anthropic: none) | `crates/agent-native/src/lib.rs:621-624`, `crates/agent-native/src/models.rs:187-190`, `PLAN.md:513-517` |
| Vision | Up to 4 images (PNG/JPEG/WebP/GIF, 5 MB) per message for models with `vision`; browser downscales; stored per chat. | works | `crates/protocol/src/lib.rs:24-36`, `crates/agent-native/src/images.rs`, `crates/node/src/images.rs`, `README.md` "Media" |
| Voice input | Mic button records in the browser; master forwards audio to a configured OpenAI-style `/audio/transcriptions` endpoint (e.g. Whisper). 25 MB cap; not stored. | works | `crates/master/src/voice.rs:1-40`, `web/src/components/Composer.tsx:160-170`, `crates/master/src/http.rs:145-149` |
| Voice output (TTS), realtime voice | None. | none | n/a |
| Subscription-login providers | Deliberately none (API key only). | none | `crates/agent-native/src/llm.rs:24-26` |
| Built-in mock provider | Scripted offline provider for tests and demos (`run:`, `read:`, `delegate:`, `ui:` ...). Default when no provider is configured. | works | `crates/agent-native/src/mock.rs:1-30` |
| Provider-specific tricks | Prompt-cache markers, same-origin-only redirects for keys; no request retry, no streaming-reasoning surfacing, no provider-side tool "strict" mode. | partial | `crates/memory/src/embed.rs:37-60`, `crates/agent-native/src/llm.rs:221-258` |

## 9. Extensibility

| Item | Status | Detail / cite |
|---|---|---|
| MCP (client or server) | **none** | grep for "mcp" over `crates/` and `web/src` finds nothing. PLAN §0 explicitly cuts "Plugins"; the sidebar has a "Connections" placeholder labelled "no route, no code" (`web/src/components/Sidebar.tsx:129-143`). PLAN cites MCP only in a reference to another project's design (`PLAN.md:154-185`). |
| Plugins | **none** | Same placeholder. |
| Skills | **none** (docs-only in PLAN §0 agent fields) | `PLAN.md:83-90` |
| Hooks (pre/post tool, lifecycle) | **none** | The only "hooks" in the tree are git hooks being disabled (`crates/node/src/git.rs:59`) and webhooks (section 11). |
| Custom slash commands | **none** | The web composer has `@agent` mentions only (`web/src/lib/mentions.ts`); no `/command` parser. `/` opens search (README). |
| Agent files | works | `<data>/agents/<id>.md` (`crates/agents/src/lib.rs:1-33`). |
| Output-format extension | partial | Model can emit `ui:choices|form|progress|chart` blocks; fixed set, validated in Rust (`crates/core/src/ui/mod.rs`). Not user-extensible. |
| Tool extensibility | none | Tool set is compiled in (`crates/tools/src/lib.rs:385-396`); only external helper is optional `graphify` (`crates/tools/src/graphify.rs`). |
| Pluggable backends | partial | `AgentBackend` trait allows driving external harnesses (PLAN §0 "Agent backends"), but only `NativeBackend` exists (`crates/agent-native/src/lib.rs:537-541`). |
| Pluggable memory/search/embeddings | works | Memory store trait + mem0/Qdrant, SearXNG/Brave, any OpenAI-style embeddings/transcription URL. |
| Event sources for routines | partial | Designed as `impl EventSource` but v1 has exactly one: the webhook (`crates/master/src/routines/events.rs:1-6`). |

## 10. UI surfaces

| Surface | What exists | Status | Cite |
|---|---|---|---|
| Web UI | React 19 + Vite + TS, embedded in the binary. Home bar with modes Search/Ask/Agentic, agent and model pickers, effort, mic, attachments, incognito, compare-with (second model), node picker. | works | `web/src/lib/modes.ts:4-5`, `web/src/components/Composer.tsx` (see `:151-152`, `:160`, `:586`, `:681`) |
| Conversation view | Markdown + GFM tables (sortable, CSV copy), math, callouts, syntax highlight, citations `[S2]` with Sources list, tool cards with live output, edit diffs, approval buttons, per-turn "N files changed / Undo / Rewind", variant picker, usage tooltip. | works | `web/src/components/Conversation.tsx`, `web/src/components/ToolCard.tsx:125-131`, `:234-236`, `web/src/md/` |
| Rich `ui:` blocks | choices, form, progress, chart; answers are validated against the log; secret-asking fields refused. | works | `crates/core/src/ui/`, `web/src/ui/`, `crates/node/src/session.rs:345-379` |
| Diff viewer | Only the old/new strings of an `edit` call; no whole-turn diff or file-tree view of changes (the checkpoint stores the file list). | partial | `web/src/components/ToolCard.tsx:125-131`, `crates/core/src/event.rs` (`Checkpoint.files`) |
| File browser / editor / terminal pane | None. | none | no such component under `web/src/components/` |
| Plan mode / todo panel | None (the `ui:progress` block is a model-drawn progress card, not a plan with approval). | none | `crates/core/src/ui/progress.rs` |
| Sidebar areas | Agents, Routines, Media, Artifacts, Projects, Recent, Connections placeholder (stub), search panel (Ctrl K). Mobile drawer and icon rail. | works (Connections stub) | `web/src/components/Sidebar.tsx`, `web/src/components/SearchPanel.tsx`, `docs/sidebar-search.md` |
| Notifications | Browser Notifications (secure context only) when a run finishes. "Waiting for you" attention list for approvals. | works | `web/src/lib/notify.ts:1-30`, `web/src/components/Attention.tsx`, `crates/master/src/http.rs:158` |
| Themes/i18n | Light/dark/system, accents, localisation catalogs, home backgrounds. | works | `web/src/lib/appearance.ts`, `web/src/lib/i18n.ts`, `web/src/bg/` |
| CLI | `serve`, `token`, `claim-code`, `doctor`, `node join`, `node run`, `graphify install|status`. No chat, `exec`/headless run, `--resume`, or one-shot prompt command. | partial (admin CLI only) | `crates/cli/src/main.rs:31-118` |
| TUI | **none** | no terminal UI code or dependency. |
| IDE integration | **none** (no LSP, ACP, VS Code or JetBrains plugin; ACP is only mentioned as a deferred idea in `PLAN.md:143-146`) | docs-only mention |
| Mobile | Responsive web (drawer, touch handling) and a bar mode for touch; no native app, no PWA manifest or service worker found. | partial | `web/src/components/Sidebar.tsx:38`, `web/index.html` (no manifest link) |
| Bundle budget | First-load budget 110 KB gzip (hard), total is a soft warning. | works | `web/scripts/size.mjs:11` |

## 11. Remote and distributed execution

| Item | What exists | Status | Cite |
|---|---|---|---|
| Master/node split | Master serves UI/API and chat runtime; agentic loop and tools run on the node that owns the folder (embedded `local` node or remote). Nodes call the model provider **directly** with their own `config.toml` key, so each node holds provider keys. | works | `crates/cli/src/main.rs:213-300`, `:500-540`, `README.md` "Remote worker nodes" |
| Link | WebSocket over TLS 1.2/1.3 (rustls) with a pinned self-signed master cert fingerprint; Ed25519 node key signs a nonce bound to the cert fingerprint; 8 MiB frame, 10 s handshake steps, 32 concurrent pre-auth handshakes, ping 20 s / idle 60 s. | works | `crates/link/src/lib.rs:43-49`, `crates/master/src/nodes.rs:16-21`, `docs/security-node-link.md` |
| Pairing | Admin creates a single-use, 15-minute join token (stored hashed); `node join <wss://...> --fingerprint ...` (token via env); revoke drops the link. | works | `crates/master/src/nodes.rs:16`, `:38-80`, `crates/cli/src/main.rs:102-118`, `crates/master/src/http.rs:124-127` |
| Multiple nodes | Yes; node picker per chat, default node per project, per-node `node.toml`. | works | `crates/master/src/lib.rs:557-600`, `crates/master/src/projects.rs` |
| Link back-pressure | Master reads node frames into an **unbounded** channel on `main` (a registered node can make it buffer without limit; AUDIT L1). Fixed only on `integration/gate3`. | partial | `crates/link/src/server.rs:23-24` |
| Message protocol | `session.create/prompt/select_variant/set_agent/cancel/restore/delete`, `approval.respond`, `resume`, `artifact.fetch/release/stage`; versioned (`PROTOCOL_VERSION=1`). | works | `crates/protocol/src/lib.rs:74-260` |
| Headless/CI mode | No one-shot CLI. Closest: HTTP API with bearer token plus routines. | none (CLI), partial (API) | `crates/cli/src/main.rs:31-118` |
| HTTP API | REST + `/api/ws`; JSON; cookie sessions for the UI, `Authorization: Bearer <api-token>` (admin-equivalent) for scripts. No OpenAPI spec or SDK. | works | `crates/master/src/http.rs:26-175`, `crates/master/src/auth.rs:528-580` |
| Webhooks | `POST /api/hooks/<trigger id>` with a per-routine bearer secret (shown once, rotatable, stored as SHA-256), idempotency key, 64 KiB body, token-bucket throttles that unknown ids cannot exhaust. Triggers a routine, not an arbitrary session. | works | `crates/master/src/http/hooks.rs:1-60`, `crates/master/src/routines/buckets.rs:1-10` |
| Routines / scheduling | Cron (5 fields, time zones), interval, one-shot; <=8 schedules or exactly one webhook per routine; run-now; missed-run rule; per-user/server concurrency and daily caps; run history with tokens and status; run chats auto-pruned. | works | `crates/master/src/routines/schedule.rs:1-12`, `runner.rs:1-12`, `spec.rs:1-35`, `crates/master/src/http/routines.rs:35-45`, `README.md` "Routines" |
| Unattended agentic runs | Routines can run agentic but **only in `ask`/`ask_all`**: a command waits for a human click (cancelled after 24 h); `auto` is rejected. So scheduled agents can only do read-only work unattended. | works | `crates/master/src/routines/spec.rs:131-141`, `runner.rs:95` |
| Untrusted webhook data | Webhook-triggered chats get no memory, no `web_*`, no `remember`, a banner, and the flag sticks to the chat. | works | `crates/master/src/lib.rs:257-297`, `crates/master/src/routines/events.rs:1-20` |
| Binding | Default `127.0.0.1:7777`; non-loopback needs `--insecure-bind` or a WireGuard address; `--tls` self-signed UI cert; `--allowed-host` for proxies; node listener off unless `--node-listen`. | works | `crates/cli/src/main.rs:33-70`, `crates/cli/src/net.rs` |
| Docker | Master image only (nodes cannot be containerised by design); compose with host networking; minimal and full (Qdrant + Ollama) templates; `edge` image follows `main`. | works | `Dockerfile:1-25`, `compose.yaml`, `docker/` |

## 12. Git integration

The harness does not wrap git as a feature; it uses git as plumbing.

| Item | What exists | Status | Cite |
|---|---|---|---|
| Checkpoints | Shadow repo per folder (also for non-git folders), reading the user's objects via alternates; every git call has a cleared env, no system/global config, hooks/fsmonitor/pager/credential helper off, 120 s timeout. | works | `crates/node/src/git.rs:1-60`, `:431`, `:659` |
| Undo / rewind | Restore exact bytes for a turn or from that turn on; forced restore when files changed since. | works | `crates/node/src/git.rs:721-800`, `crates/node/src/session.rs:567-651` |
| Sub-agent worktrees | See section 7. | works | `crates/node/src/git.rs:861-960` |
| Diff view | File list per turn only (`Checkpoint.files`); no rendered diff. | partial | `crates/node/src/session.rs:509-530` |
| Commit, branch, PR creation | No built-in commit/branch/PR tools or UI. The agent can run `git`/`gh` through `bash` (subject to approval and env allow-list; `SSH_AUTH_SOCK` and tokens are not passed unless `env_pass` lists them). | none (via bash only) | `crates/tools/src/env.rs:20-30` |
| Git metadata on artifacts | Published artifact versions record the git commit/dirty state. | works | `crates/node/src/artifacts.rs` (`GitInfo`, imported at `:23`), `README.md` "Artifacts" |

## 13. Files, artifacts, media

| Item | What exists | Status | Cite |
|---|---|---|---|
| Document attachments | Text and PDF uploads (10 MB, 5 per message), text extracted on the master at upload (PDF via `pdf-extract` in-process on `main`; scanned PDFs refused) and stored in the message event. | works (PDF parsing in-process: AUDIT S7) | `crates/master/src/attachments.rs:1-50` |
| Image attachments | Raster images to vision models; deleting media that chats still carry warns. | works | `crates/master/src/media.rs:1-10`, `crates/master/src/http/media.rs:31-48` |
| Media library | Private per-account library of images/video/audio (uploads, chat images, published media); browser-made thumbnails; kinds by magic bytes; inline only for sniffed raster/audio/video; sandbox CSP on served files. | works | `crates/master/src/sniff.rs:1-10`, `crates/master/src/http/media.rs`, `docs/media.md` |
| Artifacts | Versioned published outputs (binaries, packages, archives, docs) with sha256, size, git commit, source chat; share with workspace; pin versions (max 10), last 5 kept; scriptable latest-download URL. Nothing is rendered or run (only image/audio/video preview). | works | `crates/master/src/artifacts.rs`, `crates/master/src/http/artifacts.rs:32-50`, `crates/master/src/artifact_pull.rs:1-8`, `README.md` "Artifacts" |
| Pull protocol | Master pulls staged files in 256 KiB windows, 4 in flight, verifies size and sha256; node resolves stage ids through its own index. | works | `crates/protocol/src/lib.rs:17-22`, `crates/master/src/artifact_pull.rs`, `crates/node/src/artifacts.rs:1-12` |
| Blob storage | Files on disk sealed with AES-256-GCM STREAM, per-blob keys kept in `master.db` (crypto-shredding on delete, **not** confidentiality at rest; AUDIT L8). Quotas: per file 200 MB, per account 4 GB, server 20 GB, free-disk floor. | works | `crates/master/src/blobs.rs:1-12`, `crates/master/src/storage.rs:1-12`, `README.md` "Storage limits" |
| Storage backends | Local disk only. S3/object storage: none. | none | `docs/storage-backends.md` |
| Import by URL | Deliberately none. | none | PLAN T17 |

## 14. Multi-user features

| Item | What exists | Status | Cite |
|---|---|---|---|
| Accounts | First admin created with a one-time claim code; admins create accounts (temporary password, must change); no self-signup; Argon2id; last admin protected. | works | `crates/master/src/auth.rs:182-196`, `:229-262`, `:405-440` |
| Sessions/auth | Server-side session table (cookie holds a random id, only SHA-256 stored, 30-day sliding expiry), `HttpOnly; SameSite=Strict`, Host allowlist, Origin check, constant-time API token. | works | `crates/master/src/auth.rs:50`, `:445-470`, `:506-580`, `crates/store/src/lib.rs:315-332` |
| Login throttle | 5 failures per username per 15 min; lookup throttle for member adds. | works (pre-auth DoS open on main, S4) | `crates/master/src/auth.rs:69-75`, `:348-390` |
| Workspaces and roles | Personal workspace per account plus team workspaces; fixed roles owner/admin/member/guest mapped to permissions `agentic`, `settings`, `members`, `routines`; workspace-tier defaults for agent and models. | works | `crates/master/src/workspaces.rs:18-110`, `crates/master/src/http/workspaces.rs` |
| Privacy | Chats are private to their owner in every workspace, admins included; only the API token sees all. | works | `crates/master/src/auth.rs:138-145`, PLAN T10 |
| Sharing | Artifacts can be shared with a workspace; projects are shared inside a workspace (members use their own chats). Chats and memories are not shareable. | partial | `README.md` "Artifacts", "Projects", PLAN §0 principles |
| Projects | Name/icon/accent, instructions (<=4000 chars), project memory, kanban board with templates, default node and folder. The board is user-edited via UI/API; the open cards are injected into every project chat, but the agent has no tool to move cards. | works | `crates/master/src/projects.rs:1-12`, `crates/store/src/projects.rs:74-180`, `crates/master/src/http/projects.rs:27-54` |
| Per-user prefs | Appearance, default agent/model, search engines, mode classifier labels. | works | `crates/master/src/http.rs:29-36`, `web/src/lib/classifier.ts` |
| SSO / OIDC / SCIM | None. | none | n/a |
| Per-user cost/usage limits | None (only routine run caps and storage quotas). | none | n/a |

## 15. Web access

| Item | What exists | Status | Cite |
|---|---|---|---|
| Search | `web_search` through a self-hosted SearXNG or Brave API; results cleaned, each page gets a session-stable label `[S#]`. | works | `crates/web/src/lib.rs:420-540`, `crates/web/src/sources.rs:43-80` |
| Fetch | `web_fetch`: http/https only, no URL credentials, 20 s timeout, 2 MiB body cap, 20,000 char text; readability to Markdown (default on). | works | `crates/web/src/lib.rs:32-36`, `:91-120`, `:206-290` |
| SSRF defence | Custom DNS resolver drops non-public addresses (also on every redirect; env proxies ignored); IP-literal and `localhost` URLs checked. `is_public` misses 6to4, Teredo, IPv4-compatible IPv6 on `main` (AUDIT L12). | works (gap L12) | `crates/web/src/lib.rs:39-90`, `:125-165` |
| Provenance | A URL is fetched only if it appears in text the user or a tool produced (`observed`), so an injected page cannot make the model build an exfiltration URL. On `main` the observed set includes any tool output, error text and sub-agent task text (AUDIT L11). | works (weak: L11) | `crates/web/src/lib.rs:170-205`, `crates/agent-native/src/lib.rs:394-410`, `:837` |
| Citations | Model citations resolve only to pages the tools returned; links are shown with their real host; unusual links ask before opening. Sub-agent answers cite in the parent's numbering. | works | `crates/core/src/refs.rs`, `crates/agent-native/src/lib.rs:310-314`, `web/src/md/Cite.tsx` |
| Where available | Chat runtime only; agentic sessions have no network tool except what `bash` can reach (no network sandbox on `main`). | partial | `crates/cli/src/main.rs:255-259`, `:447-451` |
| Browser automation / headless rendering | None. | none | n/a |
| Favicons/link previews | Deliberately none in replies (a `favicon` helper exists for search-engine UI). | n/a | `crates/web/src/favicon.rs`, README "Deliberately not loaded" |

## 16. Observability

| Item | What exists | Status | Cite |
|---|---|---|---|
| Logs | `tracing` with `HARNESS_LOG` filter; headers never logged; startup logs chosen provider/model and tool names. | works | `crates/cli/src/main.rs:679-684`, `:297` |
| Audit | SQLite `audit` table: node commands received, join/revoke, artifact refusals, routine fires, webhook rejects (no content). No UI to browse it. | partial | `crates/store/src/lib.rs:293-297`, `:1268-1275`, `crates/node/src/lib.rs:303-312`, `crates/master/src/routines/mod.rs:312` |
| Event log as trace | The full tool-call/approval/checkpoint/usage trail is in the session log, visible in the chat. | works | `crates/core/src/event.rs:59-80` |
| Usage/cost | Tokens per turn and per routine run; cache hit tokens recorded. No cost in currency, no per-user or per-model dashboard. | partial | `crates/agent-native/src/llm.rs:246-253`, `README.md` "Routines" |
| Debug aids | `rizzy-harness doctor` (bash/git present, config and `node.toml` parse, token file mode); mock provider with scripted commands; Playwright e2e scripts. | works | `crates/cli/src/main.rs:557-632`, `web/e2e/` |
| Metrics / tracing export | None (no Prometheus, no OpenTelemetry). Errors in 500 bodies leak internals (AUDIT L4) on `main`. | none | grep finds no exporter |
| Raw provider request log | None found. | none | n/a |

## 17. Configuration and deployment

| Item | What exists | Status | Cite |
|---|---|---|---|
| Data directory | `~/.local/share/rizzy-harness` or `--data-dir` / `HARNESS_DATA_DIR`: `master.db`, `chat.db`, `memory.db`, `blobs/`, `keys/`, `secrets.toml` (0600), `api-token`, `claim-code`, `agents/`, `checkpoints/`, `config.toml`, `node.toml`. | works | `crates/cli/src/main.rs:130-142`, `README.md` "Configuration" |
| `config.toml` | providers, models, `[agent_models]`, avatars, `[chat]`, `[web]`, `[memory]`, `[voice]`, `[savings]`, `[routines]`, `[media]`, `[artifacts]`, `[storage]`, `[workspaces]`, `[nodes]`; mostly editable from Settings. | works | `crates/master/src/config.rs:1-420`, `README.md:295-510` |
| Environment | `.env` file and `HARNESS_*` variables override `config.toml` (read-only in the UI); `.env` names are hidden from tools. | works | `crates/master/src/env.rs:1-20`, `docker/env.reference` |
| Secrets | Keys by env-var name or the 0600 `secrets.toml`; `Secret<T>` redacts `Debug`; stored keys only sent to the base URL they were saved for. Mode of `api-token` is not enforced on read on `main` (AUDIT L7). | works (L7 open) | `crates/master/src/secrets.rs:1-15`, `crates/core/src/secret.rs` |
| TLS | UI: `--tls` self-signed (trust once); node link: pinned self-signed. No ACME; proxies supported via `--allowed-host`. | works | `crates/cli/src/net.rs`, `crates/link/src/tls.rs` |
| Migrations | None before first tag; dev data dirs are wiped on schema change. | by design | `CLAUDE.md`, `README.md:7-13` |
| CI | One GitHub workflow (fmt, clippy `-D warnings`, tests, deny, web checks). `unsafe` forbidden workspace-wide; `cargo deny` licences. | works | `.github/workflows/ci.yml`, `Cargo.toml:14` |
| Footprint target | Idle < 50 MB RSS (stated; not verified here). Heavy things are services behind URLs. | unverified | `PLAN.md:20-31` |
| Platform | Unix only (nix, process groups); Linux amd64 images. | partial | `crates/tools/src/bash.rs:6-7`, README Docker notes |

## 18. Security posture

Highlights, strongest first:
- Node-side authority: `node.toml` is the only authority; protocol and HTTP cannot widen it; approval mode is `min(requested, node max)` (`crates/core/src/policy.rs:19-23`).
- Contained data dir: file tools, grep/glob and session folders refuse the data directory in every mode (`crates/tools/src/lib.rs:111-181`, `crates/node/src/policy.rs:155-215`).
- Git is never run with the user's repo config for checkpoints (shadow repo, cleared env) (`crates/node/src/git.rs:41-60`).
- Environment allow-list for every child process (`crates/tools/src/env.rs`).
- SSRF-safe, provenance-gated `web_fetch`; no model HTML or remote images; `ui:` blocks validated in Rust.
- Webhook-triggered runs are marked untrusted and lose memory and web tools.
- Strong test culture around hostile nodes (`crates/master/tests/hostile_node.rs`).

Known open risks on `main` (from AUDIT.md at `62c17fd`, status re-checked against current code where noted):
- S1 (High): the agent shell is unsandboxed and can read the data dir; mitigated by admin-only embedded node and the file-tool deny. Still open on `main` (README line "The shell is not confined yet").
- S2, S3 (tool_output id collisions, unbounded shell stream): fixed on `main` (per-session random ids `crates/tools/src/outputs.rs:15-30`; stream budget `crates/tools/src/stream.rs:16-20`).
- S13 (install-and-run PyPI package via HTTP, HTTP edit of `node.toml`): removed (`git log` commit `1fa8f27`; `crates/cli/src/main.rs:84-100`).
- S4 login resource exhaustion, S5 sockets outliving logout, S6 no HTTP timeouts, S7 in-process PDF parsing, S10 tools can wedge a node, S11 weak redaction, S12 project member can plant facts: still open on `main`; fixed on `integration/gate3`.
- L10 "allow for session" is per tool name (per turn), L11 provenance laundering, L12 IPv6 gaps: open on `main`.
- No schema migrations (D2); the whole product is pre-release and the README tells users to keep it on `127.0.0.1` for themselves only (`README.md:7-13`).
- Event log stores every delta forever (D5); one SQLite connection behind one mutex (D3).
- Residuals in PLAN T19: git-lfs filters still run in sub-agent worktrees.
- Doc inconsistency inside PLAN: T3 says git children inherit the node environment ("Open:"), T19 and `crates/node/src/git.rs:52` say the env is cleared; code agrees with T19.
- FIX_PLAN freezes features until Gate 3 (`CLAUDE.md`, `FIX_PLAN.md:8-12`).

---

## Unmerged branches (not in `main`; effects only)

Both branches branch from `a268048` (merge-base). `feat/agent-roster` contains all of `integration/gate3`. Read via `git log`, `git diff --stat`, `git show <branch>:<path>`; nothing checked out. Line numbers refer to the branch version of the file.

### `integration/gate3` (security fixes; 30+ commits, 105 files, +10,682/-823)

| Effect | What changes | Cite (branch) |
|---|---|---|
| Landlock shell sandbox | `bash` and `graphify` run through `rizzy-harness sandbox-exec`: read-write only the session folder and a per-session TMPDIR; read-only system dirs and `[sandbox] read`; devices; optional `network = "off"` (TCP connect/bind). `[sandbox] mode = required` (default) refuses agentic sessions where Landlock is not enforced (ABI >= 3). Pathname Unix sockets (docker.sock, ssh-agent) stay reachable below ABI 9; UDP/DNS not covered. Self-test at start, fail closed. | `crates/tools/src/sandbox.rs:48`, `:66`, `:79`; `crates/node/src/policy.rs:56`; `crates/cli/src/main.rs:487-553`; tests `crates/cli/tests/sandbox.rs` |
| Tools cannot wedge a node | File tools open regular files only (O_NONBLOCK, fstat), `read` streams with a 32 MiB cap, `grep` skips files > 8 MiB, tools observe cancel; after cancel a tool gets a grace period then is abandoned; node commands go to per-session lanes so Cancel/approvals are answered inline. | `crates/agent-native/src/lib.rs:41` (`TOOL_CANCEL_GRACE`); `crates/node/src/lib.rs:47-62` |
| Approval semantics | "Allow for this turn" is honoured only after a tool's first approval in the turn, and recorded node-side. | commit `0d5cd95` |
| Redaction | `StreamScrubber` holds back the tail so secrets split across chunks are caught; JSON scrubbed by field. | `crates/core/src/secret.rs:109` |
| Login and server limits | Username checked before hashing, failure slot reserved before Argon2, 4 hash permits with 2 s wait (429), 65,536-entry failure map, 16 KiB login/setup bodies; own HTTP serve loop with 10 s header and TLS handshake timeouts, 512 connections, 30 s graceful shutdown, route-keyed request timeouts (30 s / 120 s). | `crates/master/src/auth/limits.rs:21-29`; `crates/master/src/serve.rs:56-62`; `crates/master/src/limits.rs:25-34` |
| Sockets end with login | Logout, password change, disable or delete close that user's WebSockets (code 4401); 60 s recheck; 8 sockets per user; 64 KiB frames. | `crates/master/src/ws.rs:54` |
| Response hardening | `x-request-id` on every response and sanitised 500 bodies; `Cache-Control: no-store` on `/api`; HSTS with `--tls`; Origin must match with scheme; refuse group/other-readable secret files; provider tests go through a public-only resolver (`Outbound`). | `crates/master/src/response.rs:31`; `crates/master/src/outbound.rs:25` |
| Node-link back-pressure | Bounded channels both ways (8 inbound parsed messages per node, 64 outbound commands), concurrent read/write pumps, node-side 10 s connect/TLS/challenge timeouts. | `crates/link/src/lib.rs:57-71`, `crates/link/src/pump.rs` |
| PDF helper | PDFs parsed in a child process `rizzy-harness pdf-text`: Landlock with no file access and no TCP, 512 MiB address-space limit, 2 concurrent, 20 s, 400 KB output; without Landlock PDFs are refused; dictation limits per account (daily requests/MB, one at a time). | `crates/master/src/pdf.rs:31-44`; `crates/master/src/voice.rs:19-24` |
| Web-fetch provenance | Only the user's words plus successful `web_search`/`web_fetch`/`read` outputs count; sub-agents inherit the parent's set. IPv6 6to4/Teredo/v4-compatible refused. | `crates/web/src/lib.rs:39-72`; commits `c8e071a`, `5ef6230`, `09dcdc7` |
| Other | Project facts activated only by a manager (`members_activate_facts` per project); mem0 ids percent-encoded and cross-origin redirects refused. | commits `0f3d236`, `8056000` |

Net: closes S1, S4-S7, S10-S12, L1, L3-L12 in AUDIT; the project's own FIX_PLAN Gate 3 would be near. Residual per its PLAN text: pathname-socket reach below Landlock ABI 9, UDP/DNS, TOCTOU on intermediate directories.

### `feat/agent-roster` (adds 9 commits on top of gate3; docs say "owner's decisions")

| Effect | What changes | Cite (branch) |
|---|---|---|
| New built-in agents | The five built-ins are replaced by eight: Assistant (`general`, alias `ai`; web tools and `remember`, no files), Smol (no tools, no memory), Researcher (`research`; web), Software Developer (`coder`, `dev`; all file/shell tools plus web and `remember`; delegates to scout, researcher, reviewer), Senior Teamlead (orchestrator, 4 sub-agents / 200k tokens), Writer, Scout (read-only, sub-agent only), Reviewer (read-only). `ultracode` is removed with no alias (chats naming it get `AgentGone`). Table-driven test guards each agent's tools. | `crates/agents/builtin/*.md` (`general.md`, `smol.md`, `researcher.md`, `coder.md`, `teamlead.md`, `writer.md`, `scout.md`, `reviewer.md`); commit `9c32186` |
| Aliases and categories | Optional `aliases: [..]` and `category:` in frontmatter and custom agents; aliases and ids share one namespace and resolve everywhere an agent is named (mention, API, routine, `delegates`); UI groups by category (General, IT). | `crates/agents/src/lib.rs:9-10`, `:37`; commit `b834fbe` |
| Server-side `available_as` enforcement | Sub-agent-only agents are refused as a chat's agent by the server (`Resolver::top_level`), not just hidden in the picker; stale agent ids refuse the next prompt (`AgentGone` 409) instead of silently using another agent. | `crates/master/src/resolver.rs:214` (`top_level`); commits `89cea2f`, `c2a9dd9` |
| Per-mode default agent | Chat-mode chats default to `general`, Agentic chats to `coder` (`DEFAULT_AGENTIC_AGENT`); workspaces and user prefs gain `default_agentic_agent`; saving refuses a nonexistent, sub-agent-only or (for agentic) tool-less agent; `GET /api/agents` adds `defaults`. | `crates/agents/src/lib.rs:59`; `crates/master/src/resolver.rs:199` (`default_for`); commit `03db83e` |
| Memory sharing default | A member's facts are off in team workspaces until the member shares them. | commit `00bface` |
| Still true | Agentic nodes still have no `web_search`/`web_fetch`/`remember`; the roster's web tools only apply to chat-mode (branch docs: "chat runtime offers no such tools ... calls get 'unknown tool'"). `budget` still unused. | branch `docs/agent-pages.md:47-53`, `crates/cli/src/main.rs:288-302` |

---

## (a) Things the docs promise that the code does not have

1. **MCP / plugins / "Connections"**: the sidebar shows a "Connections" entry; its own source says "no route, no code" (`web/src/components/Sidebar.tsx:129`). PLAN cut Plugins (`PLAN.md:101-110`). Not a promise so much as a visible placeholder.
2. **Agent "attached skills"** (`PLAN.md:83-90`): no skills mechanism.
3. **Agent `budget: lean|standard`** (`crates/agents/src/lib.rs:15`, README agent file docs, `PLAN.md:83-90`): parsed, shown, never used by the loop (stub).
4. **"Tools with schemas loaded on demand", "token meter in the UI"** (PLAN §0 Conversations): all tool schemas are sent every step; only a tooltip total exists (`web/src/components/Conversation.tsx:293-297`).
5. **Judge-model auto-approval** (`PLAN.md:148-152`): designed, no code; not a promise in README.
6. **"Open chat" omnibox mode** (`PLAN.md:48-52`): modes are only Search/Ask/Agentic (`web/src/lib/modes.ts:4`).
7. **Agentic web access and memory**: README's savings table lists `web_readability` for "chat + agentic" and the built-in Scout/Writer list `web_search`/`web_fetch`, but agentic nodes register neither (`crates/cli/src/main.rs:447-451`).
8. **"Allow X for this session"** (UI label `web/src/components/ToolCard.tsx:234`): behaves per turn (`crates/agent-native/src/lib.rs:625`).
9. **T5 "whether remote-created sessions are allowed at all"** (`PLAN.md` T5): no such `node.toml` key (`crates/node/src/policy.rs:9-34`); AUDIT L15 also notes that "mutual challenge-response" is node-signs-only plus a pinned certificate.
10. **Dockerfile comment** says the master uploads its binary to nodes (`Dockerfile:3-4`); SSH provisioning was removed (`git log` `1fa8f27`).
11. **PLAN T3 "Open: git children still inherit the node's environment"**: contradicted by `crates/node/src/git.rs:52` and PLAN T19.
12. **README Quick start / README status line** ("Phases 1-8 done") describes a product wider than the freeze allows; PLAN phase 9 items "deliberately not" built are listed in `PLAN.md:950-1007` (not re-verified individually).
13. **`docs/storage-backends.md` future MariaDB/Redis port**: explicitly future, no code (accurate, listed for completeness).

## (b) Things the code has that a newcomer would not expect from the README

1. **Shadow-repo checkpoints with per-turn undo and rewind** that never touch the user's `.git` and never run the repo's own git config (`crates/node/src/git.rs`, `crates/node/src/session.rs:476-651`). Most harnesses use the user's repo or none.
2. **Defence-in-depth around prompt injection**: `web_fetch` provenance (URLs must come from user/tool text), SSRF-proof resolver, citation labels that only resolve to returned pages, untrusted-webhook turns that lose memory and web tools, hidden-character stripping from memory facts and form-field secret refusal (`crates/web/src/lib.rs:170-205`, `crates/master/src/lib.rs:257-297`, `crates/memory/src/lib.rs:103-122`, `crates/core/src/ui/secret.rs`).
3. **Token-savings suite, each independently switchable**: observation masking, output filter with retrievable full output, bounded reads, outline tool, tree-sitter + PageRank repo map, Anthropic cache breakpoints with a byte-stable prefix, terse answer styles (`crates/core/src/savings.rs`, `crates/agent-native/src/mask.rs`, `crates/tools/src/filter.rs`, `crates/codemap/src/lib.rs`).
4. **A node that is the authority**: `node.toml` clamps the master; hostile-node tests; per-node approval caps; admins-only embedded node by default (`crates/node/src/policy.rs`, `crates/master/tests/hostile_node.rs`).
5. **Rich, validated structured replies** (`ui:choices|form|progress|chart`) with answers verified against the log (`crates/core/src/ui/`, `crates/node/src/session.rs:345-379`).
6. **Parallel answers from up to 4 models with a pick**, and fallback chains with a visible note (`crates/agent-native/src/lib.rs:930-1030`, `:503-534`).
7. **Routines with webhooks and idempotency, schedules with time-zone rules**, run history, human-click approval parking (`crates/master/src/routines/`).
8. **Workspaces with roles, projects with kanban boards and project memory**, private-by-default chats (`crates/master/src/workspaces.rs`, `crates/store/src/projects.rs`).
9. **Encrypted blob store with crypto-shredding**, magic-byte sniffing, sandbox CSP for served files (`crates/master/src/blobs.rs`, `sniff.rs`).
10. **Voice dictation, vision, mock provider with a scripted command language** for no-key demos and tests (`crates/master/src/voice.rs`, `crates/agent-native/src/mock.rs`).

## (c) Obvious absences versus mainstream coding agents

| Feature | Truly absent? | Detail |
|---|---|---|
| MCP client | Truly absent | No code, no config key. Nearest: pluggable memory/search/embeddings URLs; a placeholder "Connections" page (`web/src/components/Sidebar.tsx:129`). |
| Hooks (PreToolUse/PostToolUse etc.) | Truly absent | Closest: `denied_tools` and approval modes in `node.toml`; routine webhooks are inbound triggers, not agent hooks. |
| Slash commands / custom commands | Truly absent | `@agent` mentions only; `/` focuses search. Closest user-defined reusable prompt: a saved routine, or a custom agent. |
| Skills | Truly absent | Closest: custom agents (markdown + frontmatter) and project instructions. |
| Plan mode | Truly absent | No read-only plan-then-approve phase. Closest: `ask_all` approval mode and the read-only Scout/Reviewer agents; `ui:progress` is a display card. |
| TUI | Truly absent | Web UI and admin CLI only. |
| IDE integration (LSP/ACP/extension) | Truly absent | ACP appears only as a deferred idea in PLAN. |
| Headless one-shot / CI mode | Truly absent as CLI | HTTP API with bearer token or an agentic routine with a webhook can approximate it, but unattended agentic runs cannot use `auto` (`crates/master/src/routines/spec.rs:137`). |
| Project instruction files (AGENTS.md / CLAUDE.md) | Truly absent | Closest: per-project "instructions" typed in the UI (`docs/projects.md`), and agent system prompts. |
| Reactive context compaction / `/compact` / rewind-to-message | Partly | Automatic summarisation at a fixed estimated size exists (`crates/agent-native/src/lib.rs:570-600`); no manual compact, no overflow recovery, no edit-and-resume from an earlier message. Turn-level file rewind exists. |
| Session fork / branch / export | Truly absent | Variants are same-message alternatives; no export. |
| Git commit/PR helpers, worktree-per-task for the user | Truly absent (as features) | Agent can use `git`/`gh` via bash; sub-agent worktrees are internal. |
| Browser/computer use, image generation | Truly absent | n/a |
| Todo/task tool for the model | Truly absent | The project board is user-edited; agents cannot write it. |
| Web access inside agentic sessions | Truly absent on `main` | Only chat has `web_search`/`web_fetch`; coding agents can reach the network only through `bash`. |
| Memory inside agentic sessions (write) | Truly absent | `remember` is chat-only; recall is injected into agentic turns. |
| Per-command permission rules / persistent "always allow" | Truly absent | Only per-tool, per-turn "allow". |
| Cost display / budgets | Truly absent | Tokens only; no currency, no per-turn or per-user budget. |
| OS sandbox for the shell | Absent on `main`, present on `integration/gate3` | See unmerged section. |
| Retry/backoff, multi-provider native (Gemini/Bedrock) | Truly absent | Two wire formats via `genai`; OpenAI-compatible endpoints cover many. |
| Parallel tool-call execution | Truly absent (sequential) | Only sub-agent delegation is parallel. |
| Tests/lint auto-run, auto-verify loops | Truly absent | Left to the model via `bash`. |

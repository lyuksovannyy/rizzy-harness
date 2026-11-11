# OpenCode: Round 2 source investigation, cluster `safety`

## 1. Repository

- `anomalyco/opencode`, branch `dev`, pinned SHA `05d347d75054bf27778750ad5f2cd30336e81d8f`, commit date 2026-11-10T20:28:23Z.
- Clone: `.claude/research-repos/opencode`. Read only; nothing built or run.
- **Which engine is current.** There is no `packages/opencode` (the V1 engine package) at this commit. The only engine is the V2 TypeScript/Effect code in `packages/core` (agent loop, tools, permissions, shell, config), served by `packages/server` and launched by `packages/cli`. V1 survives only as config and data migration code: `packages/core/src/v1/config/*` (translates V1 `permission`/`tools`/`agent` keys into V2 rules) and `packages/core/src/database/v1-migration.bun.ts`. Every finding below is about V2 unless it says V1.
- Read in full or in the relevant part: `core/src/permission.ts`, `permission/saved.ts`, `managed-policy.ts`, `config.ts` (entry loading), `config/plugin/{policy,agent,command,source,shell}.ts`, `config/normalize.ts` (permission migration), `plugin/{agent,plan,hooks,supervisor,internal}.ts`, `tool.ts`, `tool/runtime.ts`, `tool/mcp.ts`, `tool/AGENTS.md`, `tool/plugin/{shell,read,subagent,webfetch}.ts`, `file-access.ts`, `shell.ts`, `shell/{parse,select}.ts`, the head and command-finishing part of `shell/scan.ts` (2,565 lines; not read end to end), `codemode/{tool,fetch}.ts`, `packages/codemode/README.md` and `src/interpreter/limits.ts`, `session/runner/{llm,step,index}.ts`, `session/model-request.ts` (tool execution part), `session/context.ts` (select), `environment/*`, `workspace/driver.ts`, `session/environment.ts`, `database/database.ts`, `plugin/provider/opencode.ts` (Console config load), `shared-cache.ts`, `cli/src/server-process.ts`, `cli/src/services/service-config.ts`, `cli/src/run/noninteractive.ts`, `server/src/handlers/session.ts` (create/update), `sdk/src/internal/host.ts`, and the scanner tests `core/test/shell-parse*.test.ts`.

## 2. Architecture as it bears on this cluster

- **Languages and packages.** TypeScript on Bun, written in Effect. `packages/core` holds the engine; `packages/server` the HTTP API; `packages/cli` the binary, service management and headless `run`; `packages/codemode` a self-contained JavaScript-subset interpreter for the `execute` tool; `packages/plugin` the plugin API types; `packages/process-daemon` is a Rust PTY daemon for user terminals (`portable-pty` + `libghostty-vt`), not part of the agent tool path.
- **Process model.** One server process owns all sessions; the loop, every tool, and every plugin run inside it, as the host user. Child processes are spawned directly (`ChildProcess.make(shell, ["-c", command])`), detached into their own process group on POSIX. https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382 (spawn with `env: invocation.env`, `stdin: "ignore"`, `detached` on non-Windows, 3 s force kill).
- **Everything is a plugin.** Built-in agents, tools, the plan mode, the policy enforcer and the config readers are internal plugins activated in a fixed order: internal `pre`, then user/package plugins, then internal `post` (which ends with the policy plugin). https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/supervisor.ts#L94-L98 (activation order pre, packages, post). https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/internal.ts#L285-L309 (post list ending with `ConfigPolicyPlugin`; `guarded` set that removal skips).
- **Where decisions live.**
  - Loop: `SessionRunner.drain` in `session/runner/llm.ts`, one step in `session/runner/step.ts`.
  - Tool registry, visibility filter and dispatch: `core/src/tool.ts` (`snapshot` and `execute`).
  - Authorisation: inside each tool's `execute`, which calls `Permission.assert`; the registry itself authorises nothing. `core/src/permission.ts` evaluates rules.
  - Shell parsing for permission: `shell/parse.ts` over the hand-written scanner `shell/scan.ts`.
  - Admin policy: `config/plugin/policy.ts` (a `permission.evaluate` hook plus catalogue filters) over `managed-policy.ts`.
  - Compaction (other cluster): `session/compaction.ts`, called from `runStep`.

## 3. Findings

### Agent loop and tool dispatch

#### Drain loop: one drain is one turn, steps until the model stops asking for tools
- What the code does: `drain` promotes queued inbox input, then loops `advanceToStep` (handles queued compaction and move controls, promotes input or steering) and `runStep` until a step reports no continuation. `runStep` compacts if needed, builds the request, and appends a synthetic assistant message `MAX_STEPS_PROMPT` with `toolChoice: "none"` once `agent.steps` is reached; the tool definitions stay in the request to keep the provider prompt cache. `agent.steps` has no default, so a session is unbounded unless configured.
- Trace: drain entry and inbox check https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L61-L75 ; the outer step loop https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L214-L222 ; step limit and `toolChoice: "none"` https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L259-L281 ; output-limit streak stop after three `length` finishes https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L343-L373
- Edge cases: a tool call that still arrives after the limit is refused in the step with "Tools are disabled after the maximum agent steps" https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L116-L118
- Round 1: `corrected`. Round 1 said "on the final step tools are removed"; the code keeps the definitions, sets `toolChoice: "none"`, injects an assistant-role instruction, and refuses late calls.
- For rizzy-harness: copy the "keep tool definitions, forbid by tool_choice, refuse at dispatch" trick; it keeps the cache and is enforced, not only prompted.
- Confidence: traced end to end (checked in review).

#### Tool calls start while the model is still streaming, unbounded concurrency
- What the code does: each `tool-call` event from the provider stream is forked immediately into its own fiber (`Effect.forkScoped`), so several tools run in parallel and before the stream ends. There is no concurrency cap. A user stop first asks tools that support cooperative cancel (`context.cancel`), waits `COOPERATIVE_STOP`, then interrupts the rest.
- Trace: fork per call https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L133-L161 ; cooperative stop then interrupt https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L104-L115
- Edge cases: a plain user decline is thrown as a defect so no tool can catch it and turn "no" into model-visible output; `SessionModelRequest.executeTool` recovers it and the step ends as interrupted. A decline with feedback stays a typed failure and the model continues. https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/model-request.ts#L418-L436 ; https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L271-L284
- Round 1: `new (not documented)`.
- For rizzy-harness: the "decline is a defect, feedback is a failure" split is worth copying: a tool author cannot accidentally swallow a refusal.
- Confidence: traced end to end (checked in review).

#### Dispatch: visibility filter in the registry, authorisation in each tool
- What the code does: `Tool.snapshot(permissions)` hides a tool only when the last rule matching its action name has resource `*` and effect deny ("wholly disabled"). `execute` runs the `tool.execute.before` plugin hook (which may rewrite both the input and the tool name), resolves the tool, decodes input against its schema, and calls the tool's own `execute`. The registry never calls `Permission`; each built-in tool calls `permission.assert` itself. A plugin tool that does not call `assert` runs with no check other than the wholly-disabled filter.
- Trace: wholly-disabled filter https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L389-L392 ; snapshot and dispatch https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L305-L327 ; schema decode and foreign-error wrapping https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/runtime.ts#L30-L46 ; snapshot built from agent plus session rules https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L145-L158
- Edge cases: the in-repo design note says this outright ("Tool filtering is catalog visibility, not execution authorization") https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/AGENTS.md#L51-L55 . The same note says the registry "has no `Permission.Service` dependency", but `tool.ts` does take `Permission.Service` and hands it to Code Mode for `fetch`; small doc drift.
- Round 1: `new (not documented)`.
- For rizzy-harness: avoid. Per-tool opt-in authorisation means every new tool is a chance to forget the check. Put the check at the dispatch boundary and let tools only declare their resources.
- Confidence: traced end to end (checked in review).

### Approvals and permission rules

#### Rule evaluation: flat list, last match wins, configured deny first
- What the code does: `evaluate` flattens rulesets and takes `findLast` whose action and resource both match (`Wildcard`), defaulting to ask. `evaluateInput` (1) builds configured rules = agent rules then session rules; (2) if any resource evaluates to deny under configured rules alone, the answer is deny and nothing else runs; (3) otherwise appends the saved "always" rules and computes per resource, any ask means ask, else allow; (4) runs the `permission.evaluate` plugin hooks, which may rewrite the effect; (5) if the result is still ask and an `Unattended` ruleset is in scope (schedules), the request is allowed only if the unattended rules allow every resource, else denied.
- Trace: `evaluate` https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L94-L104 ; configured rules https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L169-L174 ; full evaluation order https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L184-L210 ; matcher (`*` becomes `.*` and crosses `/`; a pattern ending in ` *` also matches without the suffix; case-insensitive on Windows) https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/util/wildcard.ts#L3-L14
- Edge cases: an agent ID that resolves to nothing gets `[*:* deny]` https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L20 . A closed permission service denies everything.
- Round 1: `confirmed` (last match wins, any deny denies, any ask asks, no match asks, `git status *` matches bare `git status`).
- For rizzy-harness: the "configured deny is evaluated before saved approvals and hooks" ordering is the right invariant for node.toml.
- Confidence: traced end to end (checked in review).

#### The real default is allow everything, including shell and external directories
- What the code does: every agent starts from `[*:* allow, read *.env ask, read *.env.* ask, read *.env.example allow]`. Nothing asks by default except reading `.env` files. Shell, edit, webfetch, MCP tools and `external_directory` are all allowed.
- Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L39-L52
- Edge cases: the `.env` asks are bypassable through shell (`cat .env` is a shell resource, governed by `shell` rules, allowed).
- Round 1: `confirmed` for the base policy; the consequence (no shell approval at all by default) was not spelled out.
- For rizzy-harness: our default must not be this. A harness that runs shell on remote nodes should default to ask for shell and edits.
- Confidence: read the definition (checked in review).

#### Precedence between built-in agent rules, config files, managed settings and per-agent rules
- What the code does: config entries load in this order (later wins for settings and for permission rules, since rules are concatenated and last match wins): managed policies (policies only), Console/integration documents, well-known documents, global files, explicit files, direct project files, project `.opencode` directories, `OPENCODE_CONFIG_CONTENT`, then managed settings (system dir and macOS MDM, with policies stripped). The agent plugin then builds each agent's list as: built-in agent rules, then every document's top-level `permissions` in load order, then every document's `agents.<id>.permissions` in load order. V1 `tools`/`permission` keys become rules placed before native `permissions` of the same document.
- Trace: load order https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config.ts#L305-L377 ; agent assembly https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L64-L107 ; V1 rule placement https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/normalize.ts#L180-L183
- Consequences, read from the order above:
  1. A user's global `permissions: [{action:"edit", resource:"*", effect:"allow"}]` comes after the plan agent's built-in `edit * deny` and overrides it.
  2. Managed (system/MDM) top-level `permissions` beat user and project top-level `permissions`.
  3. A project file's `agents.build.permissions` is appended after all top-level rules, including the managed ones, so a repository file can lift a managed top-level deny for a named agent. Only `policies` (below) are immune to this.
  4. Session rules, set by any HTTP client, are appended after all of these (next finding).
- Edge cases: `~` and `$HOME` are expanded only for `read`, `edit` and `external_directory` resources, deliberately not for shell text https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L112-L118
- Round 1: `corrected`. Round 1 said "Agent rules append after global rules". Built-in agent rules come before global rules; configured per-agent rules come after all top-level rules of every layer, including managed.
- For rizzy-harness: this is the failure mode to avoid. An "authority" layer that is simply earlier or later in one flat list will be overtaken by some other list that is appended later. Keep the node's ceiling as a separate pass that is applied last, as OpenCode does for `policies`, never as rules in the same list.
- Confidence: traced end to end (consequence 3 is derived from the order, not from a test) (checked in review).

#### Session-level rules come over HTTP and are appended last
- What the code does: `POST session.create` and the session update route accept a `permissions` ruleset from the client and store it on the session; `configured()` appends it after the agent's rules, so it wins over every config layer. Sub-sessions created by the `subagent` tool do copy the parent's session rules at creation: `Session.create` sets `permissions: input.permissions ?? parent?.permissions`, and the subagent tool passes no `permissions` of its own.
- Trace: create https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/session.ts#L101-L120 ; update https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/session.ts#L254-L257 ; merge https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L169-L174 ; child creation passing no rules https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L184-L198 ; parent inheritance in `create` https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314
- Edge cases: any holder of the server password can widen a session. The inheritance is a one-time copy when the child is created: a client that narrows the parent afterwards does not narrow an already existing child (a follow-up call with the child's `sessionID` reuses it). The child's agent rules (for example `general`'s default allow-all) still sit before the inherited session rules.
- Round 1: `new (not documented)`.
- For rizzy-harness: this is exactly what our invariant "nothing received over the protocol widens node.toml" forbids. Session rules from the master may only narrow; OpenCode inherits them into children at creation, which is right, but as a copy it goes stale if the parent is narrowed later, so a node should hold one ceiling per session tree rather than copies.
- Confidence: traced end to end (checked in review) (corrected in review: the original said sub-sessions do not copy the parent's session rules; `session.ts` L311-L314 shows they do, as a creation-time copy)

#### Admin `policies`: highest-authority document wins, inside it the last statement wins; enforced as the last `permission.evaluate` hook
- What the code does: `ManagedPolicy.statements` reverses document order so that, under `findLast`, the earliest-loaded document (MDM preferences, then system dir, then Console, then well-known, global, project) is consulted first and decides if it has any matching statement. Inside one document statement order is kept, so a later `allow` lifts an earlier `deny`. `decision` defaults to allow. `tool.use` is enforced by the policy plugin's `permission.evaluate` hook: per resource it finds the deciding `tool.use` statement for `"<action>:<resource>"` and, if any is deny, sets the effect to deny with the message. The hook only ever sets deny, so a policy allow never grants anything. Other actions (`provider.use`, `mcp.use`, `skill.use`, `websearch.use`, `integration.use`) remove items from catalogues; `plugin.use` blocks plugin loading.
- Trace: statement ordering and match https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/managed-policy.ts#L9-L39 ; `tool.use` hook https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L98-L113 ; catalogue filters https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L56-L97 ; managed policies split off and loaded first https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config.ts#L309-L330 ; `plugin.use` gate https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/module.ts#L53
- Exact precedence for one shell call: (1) configured deny (agent + config + session) ends it; (2) saved "always" allows can turn ask into allow; (3) user-plugin `evaluate` hooks run in plugin order and may set any effect; (4) the policy hook runs last (it is in `post`, after package plugins) and can only force deny; (5) `Unattended` settles a remaining ask. So policy deny beats "always" and beats user plugins registered at activation. Hooks are a mutable event passed in registration order https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/hooks.ts#L88-L95
- Edge cases: the policy cannot lift a configured deny (step 1 never reaches the hook), and it is not an execution sandbox: a plugin registering a hook later than activation, or simply running code in-process, is not constrained (inferred). Repository config cannot remove the policy plugin (`guarded`).
- Round 1: `confirmed` with precision: "a policy allow can lift an earlier policy deny" is true only inside the same document; across documents the higher-authority document's statement decides. Also "only tightens" is exact for `tool.use`.
- For rizzy-harness: copy the shape (a separate ceiling, applied after everything, that can only deny) as the node-side "node.toml decides" pass. Do not copy "later statement in the same file wins": an administrator expects a deny to be final.
- Confidence: traced end to end (checked in review).

#### Console policies when the Console is unreachable: in memory only
- What the code does: the Console config (which carries organisation `policies`) is fetched through `SharedCache` with a one-minute freshness. A failed load keeps the last config for the same connection ("dropping it would lift organization policy"). `SharedCache` is an in-memory service; nothing is persisted.
- Trace: load and keep-last https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/provider/opencode.ts#L222-L243 and https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/provider/opencode.ts#L290-L301 ; cache contract https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shared-cache.ts#L21-L28
- Edge cases: a process that starts while the Console is unreachable has no Console policies until a fetch succeeds: fail open at startup, fail sticky while running (inferred from the cache being in memory; startup path not traced line by line).
- Round 1: `new (not documented)` (Round 1 dropped the persistence claim for lack of evidence; the code shows there is none).
- For rizzy-harness: a policy that arrives over the network must be cached on disk and enforced fail-closed on start, or it is advisory.
- Confidence: read the definition (checked in review).

#### Approval replies: once, always (project-scoped, permanent), reject (cascades)
- What the code does: `reject` fails the request and every other pending request of the same session; with a message the failures carry the feedback so the model continues. `always` writes the request's `save` patterns as allow rows for the current project in SQLite, then re-evaluates every pending request and auto-approves those now allowed. Saved rules are loaded for every agent in the project, with no expiry.
- Trace: reply handling https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L306-L366 ; saved rules become allow rules https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L159-L167 ; storage by project https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission/saved.ts#L59-L84
- Edge cases: the approval is bound to the request ID only. Any client with the server password can reply to any pending request.
- Round 1: `confirmed`.
- For rizzy-harness: bind an approval to the exact call (argv, cwd, node) and give "always" a scope the user sees.
- Confidence: traced end to end (checked in review).

#### "Always" widens far beyond what was shown
- What the code does: most tools save `["*"]`. A single "always" on reading one `.env` file saves `read:*`, which is appended after the base `.env` asks and so allows every read, including every `.env`, for every agent of the project, permanently. For shell, the saved pattern is the command prefix from an arity table plus ` *`; commands not in the table keep only their first word. So "always" on `sudo apt update` saves `sudo *`, on `env FOO=1 make` saves `env *`, on `bash -c '...'` saves `bash *`, on `python -c '...'` saves `python -c *` (python has arity 2), on `npx foo` saves `npx foo *`.
- Trace: read save `["*"]` https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-access.ts#L139-L155 ; arity table https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L21-L160 ; prefix choice (first word when unknown) https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L603-L609 ; save derivation https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L225-L245 ; test fixing the expected prefixes https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/test/shell-parse.test.ts#L8-L19
- Edge cases: wrappers (`sudo`, `env`, `xargs`, `timeout`, `nohup`, `bash -c`) are not unwrapped for matching or saving; only `builtin`/`command` are unwrapped, and only to find `cd`.
- Round 1: `corrected`. Round 1 said "always saves the tool's proposed patterns, e.g. shell prefixes"; it did not say that the patterns are `*` for most tools or first-word for unknown commands.
- For rizzy-harness: show the exact pattern that "always" will save, refuse to save first-word patterns for known wrappers and interpreters, and never let a path approval widen to `*`.
- Confidence: traced end to end (checked in review).

#### `permission.evaluate` plugin hooks: no failure channel, can loosen
- What the code does: permission hooks are typed with no failure channel ("Only tool execute.before may fail"), and the event is mutable: a plugin can change ask to allow, allow to deny, or ask to deny. It cannot see configured denies (they return before the hook).
- Trace: failure types https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/hooks.ts#L21-L30 ; hook call site https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L190-L201 ; event type https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/plugin/src/effect/permission.ts#L10-L19
- Edge cases: a hook that throws dies as a defect; the tool call fails rather than runs (inferred from the Effect typing, not traced through the Promise plugin adapter).
- Round 1: `confirmed` ("can change allow/ask but not configured deny").
- For rizzy-harness: if we add hooks to the approval path, a hook may only add denials on the node.
- Confidence: read the definition (checked in review).

#### Headless `run`: rejects asks, or `--auto` answers "once" to all
- What the code does: the non-interactive runner replies to every permission request of its session tree: `reject` with feedback by default, `once` with `--auto`. Configured denies and policy denies still hold.
- Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/run/noninteractive.ts#L97-L112 ; schedules use the `Unattended` rules instead https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L59-L65
- Round 1: `confirmed` (`--auto` flag exists; behaviour new).
- For rizzy-harness: for routines, the `Unattended` pattern (an explicit allow list that settles asks, deny otherwise) is better than a blanket auto-approve.
- Confidence: traced end to end (checked in review).

#### No model-based approver or classifier
- What the code does: nothing in the permission path calls a model. Searched `core/src` for classifier, approver, guardian, auto-approve; the only model "judge" is the `/goal` plugin (agents cluster), which does not authorise tools.
- Round 1: `not in code` (Round 1 did not claim one).
- Confidence: inferred from search.

### Shell scanner

#### Hand-written scanner per dialect; refusal on what it cannot model
- What the code does: `ShellParse.scan` picks the PowerShell scanner if the shell's basename is `pwsh`/`powershell`, else the POSIX-family scanner in `bash`, `zsh` or `posix` dialect (anything else, including `sh` and `dash`, is `posix`). The scanner is a custom character-level parser (no tree-sitter, no bash library) with a 64 KiB input cap and a substitution depth of 32. It returns either `scanned` (a list of simple commands, each with its source text as the permission resource) or `opaque` with one of nine reasons; opaque makes the tool fail with "Portable shell scanner cannot analyze command" before anything runs.
- Trace: dialect choice and refusal https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L162-L171 ; opaque reasons and result type https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/scan.ts#L3-L29 ; limits https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/scan.ts#L93-L94 ; top-level scan and the eval-sink check https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/scan.ts#L227-L239 ; shell arguments (`-c`, zsh `+o nomatch -c`, PowerShell `-NoProfile -NonInteractive -Command`) https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/select.ts#L165-L172
- Edge cases: Windows `cmd.exe` (last fallback when no PowerShell or Git Bash is found) is not a PowerShell name, so cmd syntax is scanned with POSIX rules https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/select.ts#L96-L109 . `fish` and `nu` are excluded from the tool's "compat" choice https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/select.ts#L13-L23
- Round 1: `confirmed` (four shells; unparseable input refused).
- For rizzy-harness: the refuse-on-opaque design is right. A maintained parser (tree-sitter-bash, as Codex uses) would be less code to trust than 2,500 lines of hand-written grammar.
- Confidence: traced end to end for the entry and refusal; the 2,565-line scanner body was read only in part (checked in review).

#### Chains are split; substitutions are scanned, not refused
- What the code does: `&&`, `||`, `|`, `;`, newlines and `if`/`for`/`case` bodies yield one resource per simple command. `$(...)` and backticks are scanned recursively and their inner commands are added as separate resources; the outer command keeps its literal text. `eval`, `set -k`, alias definitions, zsh options that enable evaluation, and `compgen -C/-F` mark the scan as dynamic and refuse it (eval only in combination with a bound expansion). Declarations (`export`, `declare`, `local`, `readonly`, `typeset`, `unset`) are dropped as standalone resources; their nested substitutions still count.
- Trace: command finishing, eval and the dynamic builtins https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/scan.ts#L376-L438 ; declaration skip https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L173-L175 ; tests: `echo $(curl evil | sed s/x/y/)` scans to commands, `export X` yields no resource https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/test/shell-parse.test.ts#L36-L93
- Edge cases: because declarations are not resources, `export PATH=/tmp/x:$PATH; git status` asks only about `git status`; the export changes which `git` runs (inferred from the declaration rule). If the scan yields zero commands, no shell permission check runs at all https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L147-L153
- Round 1: `corrected`. The docs (as surveyed) say "dynamically evaluated commands" error out; command substitutions are in fact analysed and allowed, only specific evaluation forms are refused.
- For rizzy-harness: treat environment assignments to `PATH`, `LD_PRELOAD`, `GIT_*` and similar as resources in their own right.
- Confidence: read the definition (plus tests) (checked in review).

#### What the scanner cannot see
- What the code does: a resource is the command's source text. Matching is a glob over that text. Consequences: (1) redirections stay inside the resource, so a rule `git status *` also allows `git status > ~/.bashrc`; redirect targets are not checked against `edit` or `external_directory`. (2) Interpreters and wrappers (`bash -c '...'`, `python -c`, `node -e`, `xargs`, `find -exec`, `sudo`) are one opaque resource; what they run is invisible. (3) Shell functions and aliases from rc files: the tool runs `-c` (non-login, non-interactive for bash) but zsh still reads `.zshenv` (inferred). (4) The commands of any script the agent wrote and then runs.
- Trace: tests showing redirects kept in the resource with a prefix save https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/test/shell-parse-compatibility.test.ts#L133-L156 ; matcher https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/util/wildcard.ts#L3-L14
- Round 1: `new (not documented)` beyond the docs' "best effort" warning.
- For rizzy-harness: a parser only ever narrows the question; it cannot make an allowlist safe. That is the argument for an OS sandbox under the rules, which OpenCode does not have.
- Confidence: traced end to end for (1) and (2); (3) inferred (checked in review).

#### `cd` targets: lexical, relative to the tool's working directory
- What the code does: `cd`, `pushd`, `popd`, `chdir`, `Set-Location`, `Push-Location` (also behind `builtin`/`command`) have their operands resolved: `~`, `~user` (via `/etc/passwd`), `~+`, PowerShell `$env:X`, `$HOME`, `$PWD`. Anything else containing `$` or a backtick is "unknown" and kept as a shell resource (so it is permission-checked as a command). Resolved directories go to `external_directory` checks. A fully resolved `cd` is not itself a shell resource. All targets are resolved against the tool's `workdir`, not against an earlier `cd` in the same chain.
- Trace: unwrap and resolution https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L219-L224 , https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L251-L267 , https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L542-L563 ; tool side https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L135-L153
- Edge cases: `cd a && cd ../../etc` resolves `../../etc` against the workdir, not against `a` (inferred from `FileAccess.resolvePath(target.absolute, directory)`). Since `external_directory` is allowed by default, this matters only for users who configure it.
- Round 1: `confirmed` ("best effort").
- Confidence: traced end to end (checked in review).

### Sandboxing

#### No OS sandbox in V2
- What the code does: nothing. Searched all packages for `sandbox-exec`, `bwrap`/bubblewrap, `landlock`, `seatbelt`, `seccomp`, `firejail`, `unshare`; the only hits are unrelated (a comment in MCP code, UI types). The `project.sandboxes` column is the V1 name for worktree lists. Commands run directly as the host user with the full environment.
- Trace: spawn https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382 ; path containment is lexical (`path.relative`), so a symlink inside the project pointing outside is "internal" https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/util/src/fs-util.ts#L269-L272 and https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-access.ts#L90-L101
- Round 1: `confirmed` (nothing documented; nothing in code).
- For rizzy-harness: this is the gap a node-side sandbox fills. Also resolve paths with `realpath` before containment checks.
- Confidence: inferred from search plus the spawn path (checked in review).

#### Code Mode (`execute`) is a custom interpreter, not an isolate
- What the code does: `@opencode/codemode` parses with `acorn` and runs a JavaScript subset on its own AST interpreter (default `legacy`) or an experimental bytecode VM (flag `codemode_vm`). The program can reach only what the host injects: `tools.*` (the Code Mode catalogue) and one extension global, `fetch`. No `process`, filesystem, imports or timers exist in the language. Limits: 16 MiB strings, 10 M array elements, 10,000 pending promises, depth 32 at the host boundary; the timeout is checked between interpreter steps.
- Trace: design statement https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/codemode/README.md#L7-L18 ; limits https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/codemode/src/interpreter/limits.ts#L1-L21 ; runtime built with tools and the fetch extension only https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L278-L290 ; engine flag https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L123
- Edge cases: the interpreter runs inside the server process, so an interpreter bug is a server bug. It is not a security boundary in the OS sense.
- Round 1: `confirmed` (no fs, imports, timers; only `fetch`); engine `new (not documented)`.
- For rizzy-harness: "only run the language you implemented" is a reasonable stance in-process, but 14k lines of interpreter is a large trusted surface. For us, model-written code belongs on the node under the same sandbox as shell.
- Confidence: read the definition.

#### What Code Mode can reach, and how nested calls are authorised
- What the code does: the catalogue holds every tool not registered with `codemode: false`. The core built-ins (`shell`, `read`, `edit`, `write`, `patch`, `glob`, `grep`, `webfetch`, `websearch`, `subagent`, `skill`, `question`) are all `codemode: false`, so Code Mode reaches MCP tools, plugin tools and the `opencode.*` namespace (sessions, worktrees, schedules, artifacts). Each nested call goes through the same `run` (before-hook, schema decode, the tool's own `assert`), with the parent call's context; MCP tools assert action `<server>_<tool>` on resource `*`. `fetch` asserts `webfetch` on the URL as written (fragment stripped), refuses `file:` and URLs with credentials, follows redirects manually with `assertNotDenied` on each hop (max 20), strips `Authorization`/`Cookie` cross-origin, 30 s timeout, any method and body.
- Trace: nested call path https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L126-L154 ; MCP assert https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/mcp.ts#L61-L69 ; fetch https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/fetch.ts#L17-L56 and https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/fetch.ts#L91-L108 ; shell is direct-only https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L220-L227
- Edge cases: there is no private-address or loopback guard on `fetch` or on the `webfetch` tool (only an http(s) scheme check) https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/webfetch.ts#L144-L158 , so with the default allow the model can POST to local services, including the OpenCode server itself (it still needs the password). The response body is read whole with no size cap https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/fetch.ts#L110-L114
- Round 1: `confirmed` that nested tools enforce their own permissions; new detail on what is in the catalogue.
- For rizzy-harness: copy "redirect hops re-checked against deny rules" and "credentials in URL refused". Add the loopback and RFC 1918 block they lack.
- Confidence: traced end to end (checked in review).

#### Denying `execute` does not remove the tools; it moves them to the direct list
- What the code does: when `execute` is wholly denied, the snapshot exposes all Code Mode tools as direct tools instead.
- Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L270
- Round 1: `refuted`. Round 1 (from docs) said "Denying `execute` for `*` removes Code Mode and tools reachable only through it". The code comment reads "Without Code Mode its tools would be unreachable, so expose them as direct tools instead."
- For rizzy-harness: an admin who wants MCP off must deny the MCP actions, not `execute`.
- Confidence: read the definition (checked in review).

#### Plan agent: read-only only for edit tools; shell, subagents and Code Mode stay open
- What the code does: the plan agent gets `question allow`, `edit * deny`, `edit ~/.opencode/plan/* allow` appended to the base allow-all. Nothing denies `shell`, `subagent` or `execute`. An `execute.after` hook rewrites a blocked edit's error into a friendlier message, and a context hook re-inserts the plan reminder ("do not modify any other files or ask a subagent to do so") if compaction drops it. The `explore` subagent, described as read-only, allows `shell *`.
- Trace: plan rules and hook https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L53 ; reminder text https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L14-L21 ; explore rules https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L74-L93 ; `general` denies only `question` and `subagent` https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L63-L72 ; subagent permission check uses the child's agent https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L140-L151
- Edge cases: so plan can (a) `sed -i` or `echo > file` through shell (redirects are not edit-checked), (b) launch `general`, whose rules allow edit, (c) be overridden by any global `edit allow` rule (precedence finding above). The restriction on (a) and (b) is the prompt only.
- Round 1: `corrected`. Round 1's "plan denies edits except under `~/.opencode/plan`" is true for the edit tools only; the Round 2 question "does plan deny shell" is answered: no. "explore is read-only" is `refuted` as an enforced property (prompt-only for shell).
- For rizzy-harness: a read-only mode must be enforced below the tools (read-only mount or sandbox profile), and child agents must inherit the parent's ceiling.
- Confidence: traced end to end (checked in review).

### Running commands somewhere else

#### Workspace drivers: a provider seam, no built-in backend
- What the code does: a Location can be bound to a `workspaceID`; then the `Environment` (files and process spawner) comes from a `WorkspaceDriver` (`create`, `connect`, `suspendForIdle`, `destroy`, idempotent per workspace ID) instead of the local driver. The default registry is empty; drivers are supplied only by SDK embedders (`OpenCode.create({ workspaceProviders })`). When a workspace is bound, shells start with no inherited host environment.
- Trace: environment selection https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/environment/environment.ts#L18-L35 ; driver contract and empty default https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L26-L62 and https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L81-L88 ; SDK injection https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/sdk/src/internal/host.ts#L43-L45 ; host env dropped for workspaces https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L331-L346
- Edge cases: permission evaluation still happens in the server process; the remote side only executes. No container, VM or SSH backend ships.
- Round 1: `new (not documented)` (Round 1: "no master/worker model documented").
- For rizzy-harness: this is the inverse of our design (decision on the master, execution remote). Our node keeps the decision. The `Driver` split (files + spawner) is a clean seam to look at for testability.
- Confidence: traced end to end (checked in review).

### Secrets

#### No environment scrubbing for agent shells
- What the code does: the shell's environment is the whole server `process.env` (or a per-session map a client set), plus `TERM`, `OPENCODE_TERMINAL=1`, `AGENT=1`, `OPENCODE=1`, `AI_AGENT`, `OPENCODE_SESSION_ID`. Provider API keys in the environment are inherited by every command the model runs.
- Trace: base environment https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L331-L348 ; added variables https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L253-L258 ; per-session map (in memory, set via the session API) https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/environment.ts#L17-L31
- Edge cases: plugins see and can edit the environment in `shell.create.before`, which runs before the permission check, so the checked command is the one that runs https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L347-L348
- Round 1: `new (not documented)`.
- For rizzy-harness: we already contain the shell environment; keep it an allow list.
- Confidence: traced end to end (checked in review).

#### Server password reaches agent shells outside stdio mode; the service password sits in a 0600 file the agent can read
- What the code does: `OPENCODE_PASSWORD`/`OPENCODE_SERVER_PASSWORD` are deleted from `process.env` only in `stdio` mode ("Keep the lease credential out of the environment inherited by tools"). In `serve`/default mode an environment password stays and is inherited by every shell. In service mode the password is generated once and stored in the CLI's service config file (mode 0600) in the user's config directory.
- Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L85-L97 ; service password persistence https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/service-config.ts#L175-L191
- Edge cases: with the default allow on shell and `external_directory`, a prompt-injected agent can read the password and call the local API: reply "always" to its own pending requests or set allow-all session rules (inferred; the API routes are the ones cited above).
- Round 1: `new (not documented)`.
- For rizzy-harness: the node's credential for the master must never be readable by the agent's process tree (separate user, or a file the sandbox hides), and approvals must not be answerable by a credential the agent can obtain.
- Confidence: traced end to end for the env handling; the self-approval path is inferred (checked in review).

#### Provider credentials: plaintext JSON in SQLite, file mode 0600
- What the code does: credentials (API keys, OAuth tokens) are rows in a `credential` table, `value` a JSON column, no encryption or OS keychain. The database file and its WAL/SHM sidecars are chmod 0600 on POSIX; Windows relies on profile ACLs.
- Trace: table https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/credential/sql.ts#L5-L14 ; file mode https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/database/database.ts#L85-L100
- Edge cases: 0600 protects from other users, not from the agent, which runs as the same user and may read `~/.local/share/opencode/opencode.db` by default.
- Round 1: `new (not documented)` (Round 1 only had the docs' warning about snapshots and heap dumps).
- For rizzy-harness: keys on the master, never on the node's disk; on the node, the sandbox must hide the harness's own data directory (we already moved the data dir).
- Confidence: read the definition (checked in review).

#### No output redaction, no egress proxy, no credential broker
- What the code does: searched `core/src` and `server/src` for scrub/redact: hits are in voice, integration, session transfer and the server auth layer, none in tool output. Shell output streams in full to a file under the output directory; the model gets a bounded tail. No egress proxy exists; `HTTP(S)_PROXY` is passed through like any variable.
- Trace: output capture to file https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L398-L413
- Round 1: `not in code`.
- Confidence: inferred from search plus the capture path.

### Hooks, trust prompts

#### Command templates run `` !`cmd` `` with no permission check, after argument substitution
- What the code does: when a custom command is invoked, `evaluateTemplate` first substitutes `$ARGUMENTS`/`$1..` (or appends the user's text), then runs every `` !`...` `` in the resulting text through the configured shell with `cwd` = project, inherited environment, concurrency 2, no permission call, no `shell.create.before` hook, no policy hook. Command files are auto-loaded from `command/` and `commands/` under every config directory, including the project's.
- Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L165-L227 ; discovery https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L122-L131
- Edge cases: because substitution happens before the shell scan, text typed as the command's arguments that contains `` !`...` `` is executed too (inferred directly from the order of lines 178-190). A repository that ships `.opencode/commands/review.md` runs its shell blocks whenever the user types `/review`, with no prompt.
- Round 1: `confirmed` (outside the permission flow) and extended with the argument-injection order.
- For rizzy-harness: never interpolate user or model text into anything that is then scanned for executable markup; if templates run commands, they go through the same approval path as tools.
- Confidence: traced end to end (checked in review).

#### Project plugins and config load with no trust prompt
- What the code does: every config directory, including the project's `.opencode`, is scanned for `plugin/` or `plugins/` entries (`.ts`/`.js` files or directories); each is loaded as an in-process server plugin. Project `opencode.json` may also list npm or Git plugin specs. The only gate is an administrator `plugin.use` policy. Searched config and plugin code for "trust": nothing.
- Trace: auto-discovery https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136 ; directory scan https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/source-directory.ts#L9-L33
- Edge cases: opening a hostile repository runs its code inside the server with every credential the server holds (inferred: activation of a discovered target was read in `supervisor.ts`, the module import itself was not traced).
- Round 1: `confirmed` ("no plugin sandbox"); the missing trust prompt is new.
- For rizzy-harness: project-supplied executable config must require an explicit, per-hash trust decision on the node.
- Confidence: read the definition (checked in review).

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Ordered rules, last match wins, no match asks | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L94-L104 |
| Several resources: any deny denies, else any ask asks | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L184-L189 |
| Base policy allow all, ask on `*.env` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L46-L51 |
| Agent rules append after global rules | corrected (built-in agent rules come before; per-agent config rules after all top-level rules, including managed) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L64-L107 |
| Plan denies edits except `~/.opencode/plan` | corrected (edit tools only; shell, subagent, execute allowed) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 |
| Explore is read-only | refuted as enforcement (`shell *` allowed) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93 |
| Custom subagent uses only its own permissions | corrected in review (child uses its agent's rules plus a copy of the parent session's session-level rules) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314 |
| Shell scanner for Bash, Zsh, sh, PowerShell; unparseable input does not run | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L162-L171 |
| Dynamically evaluated commands error | corrected (substitutions are scanned; only eval-like forms refused) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/scan.ts#L402-L438 |
| `git status *` matches bare `git status` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/util/wildcard.ts#L11 |
| `always` saves proposed patterns, never overrides deny; `reject` rejects all pending in session | confirmed (patterns are often `*`) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L317-L363 |
| `external_directory` allowed by default | confirmed (falls under base `*:* allow`) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-access.ts#L120-L137 |
| Policies only tighten; a policy allow can lift an earlier policy deny | confirmed, only within one document; across documents the higher authority decides | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/managed-policy.ts#L9-L39 |
| Denied `tool.use` overrides "Allow always" | confirmed (hook after saved rules) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L98-L113 |
| Console and policy plugins cannot be disabled | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/internal.ts#L307-L309 |
| Console policy changes apply about once a minute | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/provider/opencode.ts#L222-L224 |
| No OS sandbox | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382 |
| Code Mode: no filesystem, imports or timers; only `fetch` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L278-L290 |
| Denying `execute` removes Code Mode and tools reachable only through it | refuted (they become direct tools) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L270 |
| Nested tools in Code Mode enforce their own permissions | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L126-L145 |
| Command `` !`cmd` `` runs outside the permission flow | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L190-L215 |
| `permission.evaluate` can change allow/ask but not configured deny | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L184-L201 |
| Final step removes tools | corrected (`toolChoice: "none"`, definitions kept, late calls refused) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L259-L281 |
| Shell default timeout 2 minutes; background has none | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L229 |
| Plugins run unsandboxed | confirmed (in-process, auto-discovered from project dirs) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136 |

## 5. In the code but not in the documentation

- Session-level permission rules settable over HTTP and appended last (they beat managed `permissions`); copied to subagent child sessions at creation (corrected in review).
- A project file's `agents.<id>.permissions` beats managed top-level `permissions` (only `policies` are a true ceiling).
- "Always" on most tools saves `*`; unknown shell commands save their first word (`sudo *`, `env *`, `bash *`).
- Declarations (`export PATH=...`) are not permission resources; redirect targets are not checked at all.
- `Unattended` rule set for schedules: an ask becomes allow only if explicitly allowed, else deny.
- Code Mode is a custom acorn-based interpreter with an experimental bytecode VM behind `codemode_vm`, and its catalogue excludes the core built-ins.
- No SSRF/loopback guard on `fetch` or `webfetch`.
- Server password stays in the shell environment outside `stdio` mode; service password in a 0600 file readable by the agent.
- Credentials stored as plaintext JSON in SQLite.
- Workspace driver seam for remote execution (SDK only), with host environment dropped for workspaces.
- Command template arguments are substituted before `` !`...` `` blocks are executed.
- User declines travel as defects so tools cannot swallow them.

## 6. Could not find or could not settle

- The full scanner (`core/src/shell/scan.ts`, 2,565 lines): read the header, limits, top-level entry and the command-finishing logic; the heredoc, arithmetic, parameter-expansion and PowerShell sections were not read. Exactly which `$(...)` forms become `command-substitution` opaque (versus scanned) would need lines ~1340-1600 and the PowerShell part from line 1818.
- Whether a Promise-API plugin's thrown error in `permission.evaluate` fails closed: the Effect typing says it becomes a defect; the adapter in `core/src/plugin/promise.ts` was not read.
- The Console policy startup path (whether any persisted copy is read before the first successful fetch): `SharedCache` is in memory and no persistence was found, but `fetchConfig` and the connection bootstrap were not traced to the first call.
- Whether hooks registered after activation (from a forked fiber) run after the policy hook: registration appends to the list, so they would, but no plugin doing so was looked for.
- Server-side authorisation of `permission.reply` beyond the shared password (sessions cluster; `server/src/middleware/authorization.ts` not read).

## 7. Citation check

```
research/round2/opencode.safety.md: 133 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit
```

## 8. Review

Checked 26 of the 30 findings. Not checked: no model-based approver (search absence), `cd` targets, the Code Mode interpreter description and its limits, and no output redaction (search absence). One finding was partly wrong, the rest held up.

| Finding | Verdict | Reason |
|---|---|---|
| Drain loop, step limit | supported | `llm.ts` L259-L281 and `step.ts` L116-L118 match; `steps` has no default |
| Tool calls forked while streaming | supported | fork per call in `step.ts` |
| Dispatch: visibility filter, authorisation in each tool | supported | `whollyDisabled` and `snapshot`/`execute` in `tool.ts` |
| Rule evaluation: flat list, last match, configured deny first | supported | `permission.ts` L94-L104, L169-L210 |
| Default is allow everything | supported | `schema/src/agent.ts` L39-L52 |
| Precedence of agent, config, managed, per-agent rules | supported | `config.ts` L305-L377 puts managed settings last; `config/plugin/agent.ts` L64-L107 appends top-level rules, then per-agent rules; plan and agent plugins are in `pre` |
| Session rules over HTTP, appended last | partly | mechanism right; the claim that subagent children do not copy them is wrong (`session.ts` L311-L314 copies the parent's permissions); fixed |
| Admin `policies` ceiling | supported | `managed-policy.ts`, `policy.ts` L98-L113: hook only sets deny, runs last in `post` |
| Console policies kept in memory | supported | keep-last on failure at `opencode.ts` L290-L301; `shared-cache.ts` has no persistence; startup path still not traced |
| Approval replies | supported | `permission.ts` L306-L366 |
| "Always" widens | supported | read saves `["*"]`; `sudo`, `env`, `bash` are not in `ARITY`; `python` is 2. Small gap: `external_directory` saves the project-root glob, not `*` |
| `permission.evaluate` hooks | supported | `evaluateInput` calls the hook after configured-deny return |
| Headless `run` | supported | `noninteractive.ts` L97-L112; `Unattended` at `permission.ts` L59-L65 |
| Scanner entry, nine opaque reasons, 64 KiB, depth 32 | supported | `scan.ts` L3-L29, L93-L94; `parse.ts` L162-L171 |
| Chains, substitutions, declarations, zero-command case | supported | `parse.ts` L173-L175, compat tests, `shell.ts` tool L147-L153 |
| What the scanner cannot see | supported | redirect test at `shell-parse-compatibility.test.ts` L133-L156 |
| No OS sandbox | absence confirmed | searched `sandbox-exec`, `bwrap`, `bubblewrap`, `landlock`, `seatbelt`, `seccomp`, `firejail` across core, cli, server, plugin, sdk, codemode, util, process-daemon, gui-plugins: no hits |
| Denying `execute` exposes tools as direct | supported | `tool.ts` L265-L270 |
| Code Mode nested calls, `fetch`, no SSRF guard | supported | `fetch.ts` L17-L60, `webfetch.ts` L144-L158 (scheme check only); `codemode: false` on built-ins; MCP asserts `<server>_<tool>` on `*` |
| Plan agent denies only edit tools; `explore` allows shell | supported | `plan.ts` L33-L53, `plugin/agent.ts` L74-L93 |
| Workspace drivers | supported | host env dropped when a workspace is bound (`shell.ts` L331-L346) |
| No environment scrubbing | supported | `shell.ts` L331-L348, `tool/plugin/shell.ts` L253-L258 |
| Server password reaches shells outside stdio mode | supported | `server-process.ts` L85-L97, `service-config.ts` L175-L191 (0600) |
| Credentials plaintext in SQLite, 0600 | supported | `database.ts` L82-L100 |
| Command templates run `!` blocks unchecked, after argument substitution | supported | `command.ts` L165-L227 |
| Project plugins load with no trust prompt | supported | `source.ts` L125-L136 scan of every directory entry; module import not traced |

Overall judgement: the permission, precedence and policy findings, the default allow-all, the absence of a sandbox, the plan/explore finding and the environment findings can be trusted. The one real error was the subagent non-inheritance claim, also repeated in the Round 1 table and section 5 (fixed there too): children copy the parent's session rules at creation. The unchecked remainder is lower risk but the scanner body (2,565 lines) was only read in part by the author, so claims about specific shell forms should be treated as design-level, not exhaustive. The agents file already states the inheritance correctly.

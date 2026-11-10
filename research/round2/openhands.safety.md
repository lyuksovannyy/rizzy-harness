# OpenHands (Agent Canvas): source-code investigation, cluster safety

## 1. Repository

- `OpenHands/OpenHands`, pinned SHA `d7293dfd671e23dab182fe71c247ff05f48e780b`, commit date 2026-11-10T21:12:50+01:00 (clone at `.claude/research-repos/openhands`, 74 MB).
- What was read: the launcher (`bin/agent-canvas.mjs`, `scripts/dev-with-automation.mjs`, `scripts/dev-safe.mjs`, `scripts/bind-host.mjs`, `scripts/ingress.mjs`, `scripts/static-server.mjs`, `scripts/proxy-utils.mjs` headers only, `scripts/runtime-services-info.mjs`), the MARS (DigitalOcean) bridge (`scripts/mars-web-bridge.mjs`, `scripts/mars-secret-store.mjs`, `scripts/mars-api.mjs`), the all-in-one image (`docker/Dockerfile`, `docker/entrypoint.sh`), the Helm chart (`helm/agent-canvas`), and in the React client the start-conversation builder (`src/api/agent-server-adapter.ts`), the conversation, settings, secrets, hooks, bash and ACP services, the confirmation UI, client-tool handlers (child conversation, secret request), WebSocket auth, backend registry storage and the Canvas extension loader. `config/defaults.json`. `docs/architecture.md`, `README.md` and `docs/SELF_HOSTING.md` were read only to find my way.

### What is and is not in this repository (stated plainly)

This repository is **only the Agent Canvas client and its launchers**: a React/TypeScript browser UI, Node scripts that start and front the backend services, an Electron shell, a Dockerfile that layers the UI onto the upstream agent-server image, and a Helm chart. Its own architecture note says Canvas is "not responsible for executing agent actions directly" or "providing the sandbox or workspace isolation layer" ([docs/architecture.md L14-L19](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docs/architecture.md#L14-L19)).

**Not in this repository** (only referenced; no source here):

- The agent loop, tool dispatch, tool executors, `file_editor`, terminal, condenser, stuck detector.
- The security analyzers (`LLMSecurityAnalyzer`, `PatternSecurityAnalyzer`, `PolicyRailSecurityAnalyzer`) and the confirmation-policy enforcement.
- The Agent Server (FastAPI) including its auth middleware, `X-Session-API-Key` checks, `expose_secrets` modes, secret registry and output masking, `sanitized_env()`.
- The per-conversation Docker runtime (`DockerExecutionWorkspace`, the `docker run` invocation, hardening flags, the "outer" server proxy logic, idle stop), deferred init / warm pods, ACP agent hosting.
- The Automation Server, Cloud and Enterprise servers. There is no `enterprise/` directory in this repository.

The agent-server is pulled as a pinned PyPI package through `uvx` ([scripts/dev-safe.mjs L576-L596](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L576-L596): `openhands-agent-server==${DEFAULT_AGENT_SERVER_VERSION}` plus sdk/tools/workspace at the same version), version `1.61.0` per [config/defaults.json L3-L10](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/config/defaults.json#L3-L10); the Docker image is built `FROM ${AGENT_SERVER_IMAGE}` ([docker/Dockerfile L77-L78](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L77-L78)).

**Repositories a complete study would need** (not cloned, per the brief): `OpenHands/software-agent-sdk` (openhands-sdk, openhands-tools, openhands-workspace, openhands-agent-server: loop, analyzers, confirmation, Docker runtime, auth, secrets, ACP), `OpenHands/typescript-client` (the wire API Canvas calls), `OpenHands/automation` (Automation Server), `OpenHands/extensions` (skills/plugins registry), and whatever repository holds the Cloud/Enterprise app-server and Runtime API (not identified).

## 2. Architecture as it bears on this cluster

- **Process model (npm launcher, the default "Option 1")**: `agent-canvas` imports `scripts/dev-with-automation.mjs` and calls `main()` ([bin/agent-canvas.mjs L157-L172](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/bin/agent-canvas.mjs#L157-L172)). It starts, as host processes of the user: the agent-server (bound `127.0.0.1`), the automation backend (bound `127.0.0.1`), a static or Vite frontend, and a Node ingress that routes paths to them ([scripts/dev-with-automation.mjs L1194-L1210](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1194-L1210), [L1228-L1237](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1228-L1237), [L1485-L1516](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1485-L1516)). Every flag combination except `--frontend-only` launches both backends ([L454-L456](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L454-L456)).
- **Process model (Docker all-in-one)**: one container runs agent-server, automation (uvicorn on `0.0.0.0` inside the container), and `static-server.mjs` acting as frontend plus proxy ([docker/entrypoint.sh L605-L620](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L605-L620), [L653-L664](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L653-L664), [L767-L787](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L767-L787)), as user `openhands` ([docker/Dockerfile L188-L206](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206)).
- **Where decisions live**: the *browser client* composes the whole conversation spec, including the confirmation policy, the analyzer, the workspace kind, container limits, hook config, tool modules and secret references, in `buildStartConversationRequest` ([src/api/agent-server-adapter.ts L1206-L1245](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1206-L1245)), and posts it to the agent-server ([src/api/conversation-service/agent-server-conversation-service.api.ts L745-L753](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-service/agent-server-conversation-service.api.ts#L745-L753)). Enforcement of all of it is server side and not visible here.
- **One credential for everything**: a single session key authenticates the agent-server and the automation backend ([scripts/dev-with-automation.mjs L499-L513](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L499-L513); [docker/entrypoint.sh L343-L353](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L343-L353)).

## 3. Findings

### The agent loop and tool dispatch

The loop, tool validation and execution are not in this repository (searched `src/`, `scripts/`, `tools/` for `step(`, executors, `Action`/`Observation` handling: only client-side rendering and the legacy Canvas UI tool shim). What Canvas controls is listed below.

#### Canvas fixes the loop's limits per conversation: 500 iterations, stuck detection on
- What the code does: every start request carries `max_iterations` (the configured value, else a hard-coded 500) and `stuck_detection: true`; planner conversations do the same.
- Trace: default constant [src/api/agent-server-adapter.ts L176-L180](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L176-L180); set on the payload [L1232-L1236](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1232-L1236).
- Edge cases and failure handling: any non-number falls back to 500; there is no client-side wall-clock or token limit.
- Round 1: `new (not documented)`; Round 1 named the stuck detector but not who switches it on.
- For rizzy-harness: an iteration cap belongs in node policy, not in what the client sends.
- Confidence: `read the definition`. (checked in review)

#### Client tools are executed by the browser tab, acknowledged by the server first, and answered through a user message
- What the code does: Canvas registers JSON "client tools" (`canvas_ui_control`, `launch_child_conversation`, `request_secret`) on the conversation. The agent-server acknowledges the call immediately; the browser watching the WebSocket does the work, de-duplicates by tool-call id in `localStorage`, and posts the outcome back as a `role: "user"` message with a prefix.
- Trace: registration [src/api/agent-server-adapter.ts L1217-L1231](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1217-L1231); dispatch on WebSocket events [src/contexts/conversation-websocket-context.tsx L1021-L1040](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/contexts/conversation-websocket-context.tsx#L1021-L1040); replay ledger [src/utils/claim-client-tool-call.ts L10-L34](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/claim-client-tool-call.ts#L10-L34); result channel [src/services/client-tool-result.ts L16-L29](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/client-tool-result.ts#L16-L29).
- Edge cases and failure handling: if `localStorage` is full the ledger write is skipped and the call proceeds ("accepting replay risk", L29-L32). A tool call only runs while some browser tab has that conversation's socket open; a second tab with a separate storage could act again. The result arrives in the user channel, so the model cannot distinguish it from a user message except by prefix.
- Round 1: `new (not documented)`.
- For rizzy-harness: avoid. Tool results must be events written by the session's owner, never user-role text produced by a UI.
- Confidence: `traced end to end` (client side). (checked in review)

#### `launch_child_conversation` runs without a human gate and without a depth or count limit in Canvas
- What the code does: when the agent calls the tool, the browser validates the arguments (target, isolation, cross-target rules), then starts a child using the *current global settings and the parent's profile*, in the parent's working directory, as a worktree with fallback to the shared directory.
- Trace: handler [src/services/child-conversation-launch.ts L511-L551](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/child-conversation-launch.ts#L511-L551); argument checks [L120-L131](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/child-conversation-launch.ts#L120-L131); local launch and worktree fallback [L265-L317](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/child-conversation-launch.ts#L265-L317).
- Edge cases and failure handling: a failed worktree silently falls back to `shared` and tells the agent (L305-L317). A grep for `depth`, `nest`, `limit` in the launch code and tool schema found nothing; any cap would have to be server side. The child's confirmation policy comes from the global setting at launch, not from the parent's live policy (inferred from `launchConversation` reading `SettingsService.getSettings()`, [agent-server-conversation-service.api.ts L660-L668](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-service/agent-server-conversation-service.api.ts#L660-L668)).
- Round 1: `corrected`. Round 1 said "launch inputs validated"; true for argument shape only, with no approval, no depth limit and no policy inheritance in this code.
- For rizzy-harness: children must inherit the parent's approval mode on the node, with a depth cap enforced where the session lives.
- Confidence: `traced end to end` (client side). (checked in review)

### Approvals and permission rules

#### Canvas's default confirmation policy is `NeverConfirm`
- What the code does: the client maps two settings to a policy. `confirmation_mode !== true` gives `NeverConfirm`; with confirmation on, analyzer `"llm"` gives `ConfirmRisky{threshold: HIGH, confirm_unknown: true}`, any other analyzer gives `AlwaysConfirm`. The built-in default is `confirmation_mode: false`, `security_analyzer: "llm"`.
- Trace: defaults [src/services/settings.ts L11-L12](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/settings.ts#L11-L12) and [L52-L56](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/settings.ts#L52-L56); mapping [src/api/agent-server-adapter.ts L777-L789](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L789); server settings merged over defaults [src/api/settings-service/settings-service.api.ts L519-L558](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L519-L558); sent on every start [agent-server-adapter.ts L1232-L1233](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1232-L1233).
- Edge cases and failure handling: the start path does not read these built-in defaults. It uses `getSettingsForConversation`, which returns the server's stored `conversation_settings` as they are and throws if the fetch fails ([settings-service.api.ts L650-L686](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L650-L686)), so the policy is `NeverConfirm` unless that server-stored object has `confirmation_mode: true` (a missing field also gives `NeverConfirm`). The built-in `confirmation_mode: false` default only applies to the display path `getSettings`, which falls back to it when the fetch fails (L635-L639). What value a fresh agent-server stores for `confirmation_mode` is SDK code and not visible here. Because Canvas always sends a policy, the agent-server's own default for "no policy given" never applies to Canvas conversations.
- Round 1: `corrected`. The docs' "ConfirmRisky is the default" is the SDK's framing; through Canvas the effective default is no confirmation at all.
- For rizzy-harness: our default should stay "ask"; and the policy must never be a client-supplied field.
- Confidence: `traced end to end` (client side) (checked in review) (corrected in review: the fetch-failure fallback to defaults applies to the display path, not to starting a conversation; the effective default depends on the server's stored conversation settings, which this repository cannot show).

#### Only the LLM analyzer changes which actions are confirmed; pattern and policy-rail only label
- What the code does: the analyzer kind is sent separately (`LLMSecurityAnalyzer`, `PatternSecurityAnalyzer`, `PolicyRailSecurityAnalyzer`, or none). With confirmation on, only `"llm"` produces `ConfirmRisky`; choosing `pattern` or `policy_rail` yields `AlwaysConfirm`, so those analyzers' risk ratings do not decide anything, they only colour the prompt.
- Trace: [src/api/agent-server-adapter.ts L784-L802](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L784-L802); attached at [L1267-L1271](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1267-L1271). There is no `EnsembleSecurityAnalyzer` option in the client.
- Edge cases and failure handling: whether `LLMSecurityAnalyzer` makes a separate LLM call cannot be settled here (SDK code).
- Round 1: `corrected`. The analyzers exist as named; the Ensemble analyzer is not reachable from Canvas, and only one analyzer feeds the decision.
- For rizzy-harness: a deterministic rule layer that can deny is the piece OpenHands lacks; keep ours deciding, not labelling.
- Confidence: `read the definition`. (checked in review)

#### The local planner always runs `NeverConfirm`, made safe only by its tool list
- What the code does: "Create a Plan" sends a raw agent spec with three tools (`glob`, `grep`, a planning file editor bound to one `plan_path`) and `confirmation_policy: NeverConfirm`.
- Trace: [src/api/agent-server-adapter.ts L1352-L1363](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1352-L1363) and [L1400-L1402](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1400-L1402).
- Edge cases and failure handling: the restriction rests on the server honouring the tool list and the planning editor's path binding (SDK).
- Round 1: `new (not documented)`. Round 1 found no plan mode; there is one, implemented by tool removal.
- For rizzy-harness: plan mode by tool removal is a sound shape; keep the path binding on the node.
- Confidence: `read the definition`. (checked in review)

#### An approval answers "whatever is pending"; it is not bound to an action id
- What the code does: the confirm/reject buttons appear when the conversation is `WAITING_FOR_CONFIRMATION`; the request body is only `{accept}` (a `reason` field exists in the type but is never filled). The "awaiting action" shown is simply the latest agent event; its `security_risk` drives a HIGH-risk banner.
- Trace: [src/components/shared/buttons/conversation-confirmation-buttons.tsx L30-L62](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/components/shared/buttons/conversation-confirmation-buttons.tsx#L30-L62); risk display [L112-L134](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/components/shared/buttons/conversation-confirmation-buttons.tsx#L112-L134); request [src/hooks/mutation/use-respond-to-confirmation.ts L15-L31](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/hooks/mutation/use-respond-to-confirmation.ts#L15-L31); type [src/api/event-service/event-service.types.ts L1-L4](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/event-service/event-service.types.ts#L1-L4); endpoint `respond_to_confirmation` [src/api/event-service/event-service.api.ts L25-L28](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/event-service/event-service.api.ts#L25-L28).
- Edge cases and failure handling: duplicate clicks are suppressed per event id client side only. If several actions are pending (parallel tool calls), one accept covers whatever the server holds; a race between "event shown" and "event pending" is not checked by the client. Keyboard shortcuts Cmd/Ctrl+Enter accept (L83-L88).
- Round 1: `new (not documented)` (Round 1 could not find how confirm/reject is wired).
- For rizzy-harness: bind each approval to the exact action id and argument hash, and record it as an event.
- Confidence: `traced end to end` (client side). (checked in review)

#### The "confirmation on" lock icon reflects the global setting, not the conversation's policy
- What the code does: the chat shows a lock icon when the *global* `confirmation_mode` is true.
- Trace: [src/components/features/chat/confirmation-mode-enabled.tsx L7-L14](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/components/features/chat/confirmation-mode-enabled.tsx#L7-L14).
- Edge cases and failure handling: a planner conversation (always `NeverConfirm`) or a conversation started before the setting changed shows the wrong state (inferred from the two code paths).
- Round 1: `new (not documented)`.
- For rizzy-harness: show the policy the node is enforcing for that session, read from the session.
- Confidence: `inferred`.

#### There is no command parsing, rule grammar or hard-deny layer in Canvas
- What the code does: nothing in this repository parses shell commands or matches rules. The policy choices are the three kinds above. The closest thing is the workspace hook file, passed through untouched (next topic).
- Trace: searched `src/` and `scripts/` for `deny`, `allowlist`, `permission` and for `shell-quote` users. `shell-quote` is a dependency ([package.json L66](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/package.json#L66-L66)) but its only importer splits the user-typed ACP launch command into argv (quote direction only; the server spawns it without a shell) ([src/utils/acp-command.ts L1-L20](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/acp-command.ts#L1-L20)); no permission code uses it. The policy kinds: [agent-server-adapter.ts L777-L789](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L789).
- Edge cases and failure handling: n/a.
- Round 1: `confirmed` (the review's "two mechanisms and no hard-deny layer: confirm").
- For rizzy-harness: our deterministic rule layer is a real differentiator; keep it.
- Confidence: `read the definition`. (checked in review)

#### ACP-hosted agents: Canvas has no separate approval path
- What the code does: for ACP profiles Canvas sends the same start request with `client_tools: []`, tags the server name, and uses the same `WAITING_FOR_CONFIRMATION` UI. Separately, it detects provider logins by running each CLI's status command through the server-level bash endpoint (`claude auth status --json`, `codex login status`, file checks for Gemini and Pi).
- Trace: ACP gets no client tools [src/api/agent-server-adapter.ts L1217-L1231](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1217-L1231) and the `acpserver` tag [L1253-L1255](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1253-L1255); login probes [src/api/acp-service/acp-service.api.ts L75-L118](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/acp-service/acp-service.api.ts#L75-L118).
- Edge cases and failure handling: how ACP `session/request_permission` is mapped (auto-allow, or turned into `WAITING_FOR_CONFIRMATION`) is in the SDK's ACP agent, not here.
- Round 1: `not in code` (the question in section 14 cannot be answered from this repository).
- For rizzy-harness: still open; read `software-agent-sdk`'s ACP agent before designing `AgentBackend` approvals.
- Confidence: `read the definition`.

#### Server-level shell execution with the session key, outside any conversation or policy
- What the code does: the client uses `BashClient.executeCommand` against the agent-server directly, at server level for local backends, for login probes, git probes and automation output. Any holder of the session key can therefore run commands on the agent-server host without a conversation, a confirmation policy or an analyzer.
- Trace: [src/api/acp-service/acp-service.api.ts L110-L118](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/acp-service/acp-service.api.ts#L110-L118); [src/hooks/use-bash-command-runner.ts L12-L42](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/hooks/use-bash-command-runner.ts#L12-L42); server-level scope explained [src/api/bash-service/bash-service.api.ts L24-L44](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44).
- Edge cases and failure handling: combined with the next section (the key is handed to the agent's shell), confirmation mode is not a boundary against a prompt-injected agent.
- Round 1: `new (not documented)`. Round 1 had "anyone with the key can request execution", which is right, but the key reaching the agent is the part that matters.
- For rizzy-harness: no endpoint that runs commands outside a session; the master never runs tools, and nodes accept only session-scoped tool calls.
- Confidence: `traced end to end` (client side). (checked in review)

### Hooks and trust prompts

#### Repository `.openhands/hooks.json` is loaded and attached to every conversation, with no trust prompt, failing open
- What the code does: on each start Canvas asks the server to read `<project_dir>/.openhands/hooks.json` and puts the result into the start request as `hook_config` (unless conversation settings already carry one). On any error the conversation starts without hooks.
- Trace: [src/api/hooks-service.ts L12-L42](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L12-L42); fetched in parallel at start [src/api/agent-server-adapter.ts L1646-L1662](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1646-L1662); attached [L1281-L1285](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1281-L1285).
- Edge cases and failure handling: two consequences. (a) A cloned repository that ships `.openhands/hooks.json` gets its shell commands run as hooks on the next conversation, without a prompt (searched `src/` for `trust`: the only trust gate is the plugin-launch checkbox below). (b) If the call that loads `hooks.json` fails or exceeds its 5 s limit (`HOOKS_LOAD_TIMEOUT_MS`, hooks-service.ts L8), the client logs a `console.warn` and starts the conversation with no hooks, so a hook-based deny rule that cannot be loaded does not apply. The 5 s is the load call, not a per-hook execution timeout; how a hook that times out while running behaves is SDK code. (c) Hooks are not loaded at all when the conversation runs in its own container ([conversation-workspace.ts L90-L92](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-workspace.ts#L90-L92)). That the hook commands then run on the server host is the SDK's behaviour and is not shown here.
- Round 1: `confirmed` for "hooks fail open"; `new (not documented)` for the absence of a project trust prompt.
- For rizzy-harness: project-supplied hooks need an explicit, recorded trust decision per repository and content hash; a policy hook that cannot load must block, not vanish.
- Confidence: `traced end to end` (client side) (corrected in review: the 5 s limit is on loading the file, not on running a hook; the load failure is logged with `console.warn`, not fully silent; hooks are skipped for container workspaces).

#### Plugin launches carry a trust checkbox; plugins see the account's agent secrets
- What the code does: the plugin-launch modal disables "Start" until the user ticks "I trust this plugin from {sources} with the agent secrets defined in my account".
- Trace: [src/components/features/launch/plugin-launch-modal.tsx L398-L420](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/components/features/launch/plugin-launch-modal.tsx#L398-L420); text [src/i18n/translation.json L20954-L20955](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/i18n/translation.json#L20954-L20955).
- Edge cases and failure handling: the checkbox is a one-shot UI gate, not stored or bound to a plugin version.
- Round 1: `new (not documented)`.
- For rizzy-harness: copy the honesty of the wording (it says secrets are exposed); bind the decision to source and revision.
- Confidence: `read the definition`. (checked in review)

#### Canvas Apps run as same-origin modules with full access to the page
- What the code does: an extension's ESM bundle is fetched through the authenticated client, turned into a Blob URL and `import()`ed into the Canvas page; the only check is that it exports `activate`.
- Trace: [src/extensions/canvas-extension-module-loader.ts L25-L42](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/extensions/canvas-extension-module-loader.ts#L25-L42).
- Edge cases and failure handling: the page's `localStorage` holds every backend's API key in plain JSON ([src/api/backend-registry/storage.ts L95-L102](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/backend-registry/storage.ts#L95-L102)), so an App can read all of them.
- Round 1: `confirmed` ("no sandbox for app code").
- For rizzy-harness: never load third-party code into the dashboard origin; our "no model HTML" invariant extends to extensions.
- Confidence: `read the definition`. (checked in review)

### Sandboxing

#### The default stack has no sandbox: the agent-server runs as the user on the host
- What the code does: the npm launcher spawns the agent-server through `uvx` as a child process of the user, with the full launcher environment merged in, working directory `~/.openhands/agent-canvas/workspaces`.
- Trace: spawn with `env: { ...process.env, ...options.env }` [scripts/dev-with-automation.mjs L727-L736](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L727-L736); agent-server start [L1154-L1211](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1154-L1211).
- Edge cases and failure handling: the agent's shell runs as the same OS user that owns `~/.openhands/agent-canvas/api-key.txt` and `secret-key.txt` ([scripts/dev-safe.mjs L84-L106](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L84-L106)), so file mode 0600 does not keep them from the agent.
- Round 1: `confirmed` (host process, no isolation).
- For rizzy-harness: the node must keep its own credentials out of the sandbox user's reach, not merely chmod them.
- Confidence: `traced end to end`.

#### Per-conversation Docker mode: Canvas only forwards env knobs; the container invocation is upstream
- What the code does: `OH_CONVERSATION_RUNTIME`, `..._SELECTABLE`, `OH_CONVERSATION_IMAGE`, `..._IMAGE_HAS_BROWSER`, `..._CONTAINER_MEMORY`, `..._CONTAINER_CPUS`, `..._CONTAINER_PIDS_LIMIT`, `..._CONTAINER_STARTUP_TIMEOUT` are copied from the launcher environment into the agent-server environment when set. For a Docker conversation the client sends `workspace: {kind: "DockerExecutionWorkspace", working_dir: "/workspace"}` and `worktree: false`.
- Trace: pass-through list [scripts/dev-safe.mjs L1009-L1028](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1009-L1028); workspace kind [src/api/agent-server-adapter.ts L987-L996](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L987-L996); worktree off [L1240-L1243](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1240-L1243); runtime choice [src/api/conversation-service/agent-server-conversation-service.api.ts L681-L688](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-service/agent-server-conversation-service.api.ts#L681-L688).
- Edge cases and failure handling: the `docker run` flags (user, caps, `no-new-privileges`, port binding, mounts), the image pull policy, the outer server proxy and idle stop are not in this repository. Searched for `no-new-privileges`, `cap-drop`, `CapDrop`, `docker run` in code: none outside docs.
- Round 1: `not in code` (hardening flags and outer proxy cannot be verified here; Round 2 question 16 needs `software-agent-sdk`).
- For rizzy-harness: the shape (one container per session, outer server on the host relaying to an inner server) is worth comparing, but the invocation must be read in the SDK before copying.
- Confidence: `read the definition`. (checked in review)

#### Client-supplied container limits are forwarded without bounds
- What the code does: the Runtime settings page values `container_memory` (any non-empty string), `container_cpus` (any number) and `container_browser` become `container: {memory, cpus, browser}` on the start request.
- Trace: [src/api/agent-server-adapter.ts L804-L835](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L804-L835); merged into the payload [L1244](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1244-L1244).
- Edge cases and failure handling: no client cap, no validation beyond `typeof`; whether the server clamps to `OH_CONVERSATION_CONTAINER_*` is not visible here. PID limit is env-only (no client field).
- Round 1: `confirmed` as far as the client goes (uncapped client-supplied memory/CPU); the server half is not in this repository.
- For rizzy-harness: node.toml sets maxima; a request may only lower them.
- Confidence: `read the definition`. (checked in review)

#### Docker mode widens the bind to every interface by default
- What the code does: if `OH_CONVERSATION_RUNTIME=docker` or `..._SELECTABLE=1` and no `OH_BIND_HOST`, the launcher binds the ingress (and frontend) to `0.0.0.0`, so containers can reach the host through `host.docker.internal`. On a non-loopback bind the session key is no longer injected into HTML and a warning is logged.
- Trace: [scripts/dev-with-automation.mjs L528-L543](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543); container-side host alias [L1545-L1549](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1545-L1549); agent's automation URL becomes the ingress [L1524-L1543](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1524-L1543).
- Edge cases and failure handling: the "isolated" mode thus exposes the whole API surface (agent-server and automation through the ingress) on the LAN, guarded only by the session key, which the next section shows is in the agent's environment.
- Round 1: `confirmed` ("Docker runtime mode binds all interfaces").
- For rizzy-harness: a container reaching back to the host must use a dedicated, scoped endpoint (a Unix socket or a per-session token), never the operator's API.
- Confidence: `traced end to end`. (checked in review)

#### If `server_info` is unreachable, Canvas falls back to a host workspace payload
- What the code does: the runtime kind comes from a 3-second `server_info` probe; on failure it returns `null`, and the payload becomes `LocalWorkspace` with a host path.
- Trace: [src/api/agent-server-adapter.ts L236-L254](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L236-L254); [L987-L996](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L987-L996); comment "Only a *successful* probe ... may isolate" [src/api/conversation-workspace.ts L37-L53](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-workspace.ts#L37-L53).
- Edge cases and failure handling: whether a Docker-only server rejects or reinterprets a `LocalWorkspace` payload is server side (not settled).
- Round 1: `new (not documented)`.
- For rizzy-harness: the isolation choice belongs to the node's config, never to a client-side probe result.
- Confidence: `traced end to end` (client side). (checked in review)

#### Docker all-in-one: agent, state and keys share one container and one uid
- What the code does: the image runs everything as `openhands`; `/home/openhands/.openhands` (settings, encrypted secrets, conversations, automation DB, `api-key.txt`, `secret-key.txt`) and `/projects` are volumes owned by that user.
- Trace: [docker/Dockerfile L188-L206](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206); key files generated and persisted [docker/entrypoint.sh L301-L341](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L301-L341).
- Edge cases and failure handling: the encryption key (`secret-key.txt`) sits next to what it encrypts, readable by the agent's shell, so "encrypted at rest" protects only against someone who copies part of the volume.
- Round 1: `corrected`. "Docker whole-stack (container, mounted dirs only)" is the filesystem boundary toward the host; inside it the agent can read every credential the stack has.
- For rizzy-harness: split the agent user from the node user even inside one container.
- Confidence: `traced end to end`. (checked in review)

#### Helm: single pod, non-root uid, no container hardening by default, optional namespace admin
- What the code does: one replica StatefulSet; pod `runAsNonRoot`, uid/gid 10001; container `securityContext: {}` with `drop: [ALL]` and `allowPrivilegeEscalation: false` only as comments; service-account token automounted; RBAC off by default, but `rbac.namespaces` binds the built-in `admin` ClusterRole per namespace and `clusterAdmin` binds `cluster-admin`.
- Trace: [helm/agent-canvas/values.yaml L1-L10](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/helm/agent-canvas/values.yaml#L1-L10); [L41-L62](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/helm/agent-canvas/values.yaml#L41-L62); [L169-L192](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/helm/agent-canvas/values.yaml#L169-L192); [helm/agent-canvas/templates/rbac.yaml L13-L30](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/helm/agent-canvas/templates/rbac.yaml#L13-L30).
- Edge cases and failure handling: with RBAC on, the agent's shell holds a token that can create pods in the listed namespaces (an escape route out of its own pod).
- Round 1: `confirmed` (single shared pod).
- For rizzy-harness: if we ship a chart, hardening on by default and no automounted token for the agent.
- Confidence: `read the definition`. (checked in review)

### Running commands somewhere else

#### DigitalOcean "MARS" sessions: the server holds the cloud token and tunnels to a remote agent-server
- What the code does: the Node server (or Electron main process) creates sessions through DigitalOcean's API with a flat manifest (`agent: openhands`, `template: openhands`, `keep_warm: false`, optional size slug checked by regex), holds the bearer token in the OS keychain, an AES-256-GCM file keyed by HKDF of `OH_SECRET_KEY`, or memory, and reverse-proxies `/mars/sessions/<id>/…` to a local tunnel listener. `/mars/*` requires the session key (timing-safe compare) or an `HttpOnly; SameSite=Strict` cookie derived from it, refuses cross-origin requests, and on a loopback bind also refuses foreign `Host` headers (DNS rebinding).
- Trace: manifest [scripts/mars-api.mjs L53-L83](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-api.mjs#L53-L83); trust model [scripts/mars-web-bridge.mjs L30-L49](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L30-L49); mount rules [L182-L200](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L182-L200); origin and rebinding checks [L260-L284](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L260-L284); authorisation [L426-L439](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L426-L439), [L546-L566](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L546-L566); token stores [scripts/mars-secret-store.mjs L214-L227](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-secret-store.mjs#L214-L227), [L351-L387](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-secret-store.mjs#L351-L387).
- Edge cases and failure handling: with no key and a loopback bind the bridge mounts open to "local callers" (L194), which includes the agent's own shell in host mode; with no key on a non-loopback bind it refuses to mount. The size slug regex blocks YAML injection from the renderer (L60-L64). The launcher deliberately keeps one bridge on the ingress only, citing that an unkeyed bridge "would take destroySession and savePat from any local process, the agent's shell included" ([scripts/dev-with-automation.mjs L1460-L1483](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1460-L1483)).
- Round 1: `confirmed` (DigitalOcean backend, PAT auth) with mechanism added.
- For rizzy-harness: the bridge is a good pattern: cloud token stays in the server, the browser gets a same-origin cookie scoped to one path, plus a `Host` check for rebinding. Copy the rebinding check on our loopback master.
- Confidence: `traced end to end` (corrected in review: the bridge is an experimental opt-in, off unless `MARS_WEB` is exactly `1` ([mars-web-bridge.mjs L182](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L182-L185), [docker/entrypoint.sh L35-L37](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L35-L37)); the finding as first written omitted this, so the loopback-open and cloud-token risks apply only to stacks that turned it on).

#### Warm or dormant sandboxes and deferred init are not in this repository
- Searched for `DEFERRED_INIT`, `/api/init`, `X-Init-API-Key`, `warm`: the only hit is MARS's `keep_warm: false` ([scripts/mars-api.mjs L72-L74](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-api.mjs#L72-L74)). Round 1's deferred-init entry is `not in code` here.

### Secrets

#### The session key that authenticates the whole stack is deliberately put in the agent's environment
- What the code does: the launcher sets `OPENHANDS_AUTOMATION_API_KEY = sessionApiKey` in the agent-server's environment "to terminal commands spawned by the agent-server", and also seeds it into the agent-server's secret store under the same name after start. The Docker entrypoint exports the same variable. The system-prompt block tells the agent to send `X-Session-API-Key: $OPENHANDS_AUTOMATION_API_KEY`. The automation backend and the agent-server accept the same key.
- Trace: env [scripts/dev-with-automation.mjs L975-L985](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985), merged into agent-server env [L1173-L1186](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1173-L1186); seeding [L1632-L1651](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1632-L1651) called at [L1998-L2003](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1998-L2003); Docker [docker/entrypoint.sh L343-L353](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L343-L353); prompt text [scripts/runtime-services-info.mjs L128-L141](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/runtime-services-info.mjs#L128-L141).
- Edge cases and failure handling: every saved secret is attached to every conversation (next finding), so the seeded key also reaches per-conversation Docker containers. With it, a prompt-injected agent can call `respond_to_confirmation` on its own pending action, run server-level bash, read or change settings, and (inferred from the client's own comment) ask for `expose_secrets=plaintext` provider keys ([src/api/settings-service/settings-service.api.ts L576-L583](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L576-L583)). A code comment also notes the SDK's `sanitized_env()` strips `SESSION_API_KEY` from bash subprocesses, while the automation preset reads `OH_SESSION_API_KEYS_0` "already in env" ([scripts/dev-safe.mjs L1056-L1069](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1056-L1069)); which names `sanitized_env` actually strips is SDK code.
- Round 1: `new (not documented)`; it contradicts the docs' framing that confirmation mode protects the user, and Round 1's "secret values stay out of chat, agent events and tool arguments" does not cover this case.
- For rizzy-harness: the most important lesson here. An agent must never hold a credential that can approve its own actions or reach the control plane. Our invariant "the node decides" must include "the sandbox has no node or master credential".
- Confidence: `traced end to end` (launcher side; the server's acceptance of the key is stated by both launchers' comments). (checked in review)

#### Secrets travel as `LookupSecret` URLs that embed the session key in their headers
- What the code does: for each saved secret the start request carries `{kind: "LookupSecret", url: "/api/settings/secrets/<name>", headers: {"X-Session-API-Key": <key>}}`; the agent-server resolves them "from its own store at spawn time". The value never passes through the browser, but the master key is copied into every conversation's start payload.
- Trace: [src/api/agent-server-adapter.ts L1114-L1144](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1114-L1144); header builder [src/api/backend-registry/auth.ts L9-L18](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/backend-registry/auth.ts#L9-L18); all secrets fetched at start [agent-server-adapter.ts L1646-L1668](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1646-L1668) and attached [L1302-L1305](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1302-L1305).
- Edge cases and failure handling: whether the server persists the request (and so the key) in `base_state.json` is not settled here. Profile-level secret scoping (`secret_refs`) is applied server side, if at all.
- Round 1: `new (not documented)`.
- For rizzy-harness: reference secrets by name only; the node resolves them with its own authority, never with a credential carried in the request.
- Confidence: `read the definition`. (checked in review)

#### Provider keys round-trip through the browser encrypted, never in plaintext
- What the code does: the client reads settings in one of three modes: redacted (display), `encrypted` (cipher text, for starting conversations), `plaintext` ("DO NOT USE from frontend"). The start request reuses the encrypted `agent_settings`/`conversation_settings` with `secrets_encrypted: true`; the agent-server decrypts with `OH_SECRET_KEY`. If encrypted settings cannot be fetched the start fails rather than falling back to redacted values.
- Trace: modes [src/api/settings-service/settings-service.api.ts L576-L593](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L576-L593); encrypted path [L642-L686](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L642-L686); planner payload flag [src/api/agent-server-adapter.ts L1421-L1423](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1421-L1423).
- Edge cases and failure handling: the ciphertext is cached in an in-memory map per backend and connection revision ([L188-L207](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L188-L207)). The plaintext mode exists on the server for any key holder.
- Round 1: `confirmed` (`OH_SECRET_KEY` encrypts sensitive values) with mechanism added.
- For rizzy-harness: we avoid the need entirely, since keys live on the node; but "encrypted blob the client may only replay" is a reasonable pattern if the dashboard must hand settings to a node.
- Confidence: `traced end to end` (client side).

#### Session and encryption keys: generation, storage and pinning
- What the code does: keys are 32 random bytes hex-encoded; persisted at `~/.openhands/agent-canvas/api-key.txt` and `secret-key.txt` with mode 0600 (falling back to an in-memory key if the write fails); `LOCAL_BACKEND_API_KEY` / `OH_SECRET_KEY` override. `AUTOMATION_KV_SECRET` is pinned on first launch with a careful protocol (refuse symlinks and FIFOs, `O_NOFOLLOW|O_NONBLOCK`, fchmod 0600, create by `link()` so concurrent launchers converge, a lock directory for blank pins).
- Trace: [scripts/dev-safe.mjs L72-L74](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L72-L74); [L148-L180](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L148-L180); [L743-L761](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L743-L761); KV pin [scripts/dev-with-automation.mjs L1014-L1078](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1014-L1078).
- Edge cases and failure handling: the session-key file read does not check for symlinks or permissions (only the KV pin does); an unreadable KV pin stops the launch instead of re-keying (L1122-L1124). The KV secret's first value *is* the session key, so the two are equal on a fresh install.
- Round 1: `confirmed` (auto-generated, persisted).
- For rizzy-harness: the KV-pin routine is a good reference for our secret files (we already do O_NOFOLLOW-style checks in 3.6); note the derived-key reuse smell.
- Confidence: `read the definition`.

#### Session key delivery to the browser: injected into HTML on loopback, opt-in on Docker
- What the code does: the static server writes `window.__AGENT_CANVAS_SESSION_API_KEY__` and a `localStorage` copy into `index.html` (`Cache-Control: no-store`) when it has a key; `applySessionKeyPolicy` strips it on a non-loopback bind unless `--allow-lan-session-key`. The Docker entrypoint always binds `::`/`0.0.0.0` and so injects only with `AGENT_CANVAS_ALLOW_LAN_SESSION_KEY=true`. The key is passed in the child's environment, not argv.
- Trace: [scripts/static-server.mjs L389-L407](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L389-L407), [L486-L497](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L486-L497), env fallback [L198-L204](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L198-L204); policy [scripts/bind-host.mjs L50-L77](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/bind-host.mjs#L50-L77); Docker [docker/entrypoint.sh L748-L770](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L748-L770).
- Edge cases and failure handling: on loopback the page is unauthenticated, so any local process (the host-mode agent included) can `GET /` and read the key. `--public` requires `LOCAL_BACKEND_API_KEY` and never injects ([bin/agent-canvas.mjs L68-L84](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/bin/agent-canvas.mjs#L68-L84); [scripts/dev-with-automation.mjs L463-L471](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L463-L471)).
- Round 1: `corrected`. Round 1 said the browser embeds the key "only with explicit Docker opt-in"; in the default npm loopback mode it is embedded by default.
- For rizzy-harness: our loopback master uses a session cookie after login; do not serve credentials in unauthenticated HTML, even on loopback.
- Confidence: `traced end to end`. (checked in review)

#### The bundled VS Code editor's token equals the master key and shares the Canvas origin
- What the code does: the editor is served under `/vscode` on the Canvas origin; the entrypoint's comment states the agent-server derives the editor's query-string connection token from `session_api_keys[0]`, "the same secret that authenticates /api", so the route is excluded from the unauthenticated test origin and the ingress sends `Referrer-Policy: no-referrer` under that prefix. `defaults.json` notes the shared origin also shares Canvas `localStorage` (where backend keys live).
- Trace: [docker/entrypoint.sh L802-L815](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L802-L815); [scripts/ingress.mjs L196-L201](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L196-L201); [config/defaults.json L32-L33](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/config/defaults.json#L32-L33).
- Edge cases and failure handling: the token is in a navigable URL (history, logs); upstream issue `software-agent-sdk#4317` is cited as the fix owner.
- Round 1: `new (not documented)`.
- For rizzy-harness: matches our "no secret in a URL" invariant; any embedded tool UI needs its own origin and its own scoped credential.
- Confidence: `read the definition` (the derivation itself is upstream; stated in the comment).

#### WebSocket auth is a first message, not a header or URL parameter
- What the code does: after `onopen` the client sends `{"type":"auth","session_api_key": key}`.
- Trace: [src/hooks/use-websocket.ts L66-L78](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/hooks/use-websocket.ts#L66-L78); [src/utils/websocket-auth.ts L1-L18](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18).
- Edge cases and failure handling: what the server sends before the auth frame, and whether it closes unauthenticated sockets, is server side.
- Round 1: `new (not documented)` (Round 2 question on WebSocket upgrade auth: the client never sends the header on upgrade; the server check is unsettled).
- For rizzy-harness: first-frame auth keeps keys out of URLs; we use the session cookie on upgrade plus an Origin check, which is stronger against cross-site sockets.
- Confidence: `read the definition`. (checked in review)

#### Client can name Python modules for the server to import
- What the code does: `tool_module_qualnames` from conversation settings is forwarded on the start request (minus Canvas's own names); the launcher also sets `OH_EXTRA_PYTHON_PATH` to the repository's `tools/` and `--import-modules` at server start.
- Trace: [src/api/agent-server-adapter.ts L1287-L1296](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1287-L1296); [scripts/dev-safe.mjs L1071-L1073](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1071-L1073), [L594-L596](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L594-L596); the shim [tools/canvas_ui_tool.py L1-L19](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/tools/canvas_ui_tool.py#L1-L19).
- Edge cases and failure handling: what the server allows to be imported is upstream (inferred risk: a key holder can load any module on the server's path).
- Round 1: `new (not documented)`.
- For rizzy-harness: tool sets are node configuration; never accept code locations over the wire.
- Confidence: `inferred`.

### Server authentication

#### No user accounts in the open-source build: one shared key, no identity
- What the code does: the client's auth for a local backend is one header (`X-Session-API-Key`), or `Authorization: Bearer` / cookie for Cloud. The ingress itself performs no authentication; it routes by longest prefix and leaves auth to the upstream services. Backend keys (local and Cloud) are stored in `localStorage` as JSON.
- Trace: [src/api/backend-registry/auth.ts L9-L18](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/backend-registry/auth.ts#L9-L18); ingress request path [scripts/ingress.mjs L185-L226](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L185-L226); storage [src/api/backend-registry/storage.ts L95-L102](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/backend-registry/storage.ts#L95-L102); local conversations reuse the backend key as their `session_api_key` [src/api/agent-server-adapter.ts L501-L502](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L501-L502).
- Edge cases and failure handling: no rate limit, lockout or rotation flow in this code (rotation = delete the file, [scripts/dev-safe.mjs L76-L83](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L76-L83)). Every key holder is the owner of everything.
- Round 1: `confirmed` ("no built-in authentication", "unauthenticated, single-tenant").
- For rizzy-harness: our accounts, private chats and per-node pinning are the gap OpenHands' open tier leaves open.
- Confidence: `traced end to end` (client and ingress). (checked in review)

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| ConfirmRisky is the default confirmation policy | corrected: Canvas sends `NeverConfirm` by default | [agent-server-adapter.ts L777-L789](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L789), [settings.ts L52-L56](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/settings.ts#L52-L56) |
| Analyzers: LLM, pattern, policy-rail, ensemble | corrected: three selectable, no ensemble; only LLM affects which actions are confirmed | [agent-server-adapter.ts L791-L802](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L791-L802) |
| No hard-deny layer; two mechanisms | confirmed | [agent-server-adapter.ts L777-L802](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L802) |
| Hooks fail open | confirmed (hook load errors drop hooks) | [hooks-service.ts L30-L42](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L30-L42) |
| Per-conversation Docker hardening (caps, no-new-privileges, 127.0.0.1, limits) | not in code (upstream SDK) | [dev-safe.mjs L1009-L1028](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1009-L1028) |
| Client-supplied memory/CPU not capped | confirmed for the client | [agent-server-adapter.ts L804-L835](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L804-L835) |
| Docker runtime mode binds all interfaces | confirmed | [dev-with-automation.mjs L528-L536](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L536) |
| Key embedded in page only with explicit Docker opt-in | corrected: embedded by default on npm loopback | [bind-host.mjs L50-L65](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/bind-host.mjs#L50-L65), [entrypoint.sh L759-L765](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L759-L765) |
| `LOCAL_BACKEND_API_KEY` auto-generated and persisted; required with `--public` | confirmed | [dev-safe.mjs L756-L761](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L756-L761), [dev-with-automation.mjs L463-L471](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L463-L471) |
| No authentication in the open tier; one key gates the server | confirmed | [auth.ts L9-L18](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/backend-registry/auth.ts#L9-L18) |
| Secret values stay out of tool arguments and the agent's reach | corrected: the master key is placed in the agent's environment and secret store | [dev-with-automation.mjs L975-L985](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985) |
| `OH_SECRET_KEY` encrypts stored values | confirmed; key file sits beside the data | [entrypoint.sh L301-L320](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L301-L320) |
| Whole-Docker mode: agent sees mounted dirs only | corrected: inside the container it sees every stack credential | [Dockerfile L188-L206](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206) |
| Helm: one shared pod, unauthenticated single-tenant | confirmed | [values.yaml L1-L10](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/helm/agent-canvas/values.yaml#L1-L10) |
| Canvas Apps unsandboxed, same browser context | confirmed | [canvas-extension-module-loader.ts L30-L42](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/extensions/canvas-extension-module-loader.ts#L30-L42) |
| Child conversations: launch inputs validated | corrected: shape only; no approval, no depth cap | [child-conversation-launch.ts L511-L551](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/child-conversation-launch.ts#L511-L551) |
| No plan mode | refuted: planner conversation with three tools | [agent-server-adapter.ts L1352-L1363](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1352-L1363) |
| Deferred init / warm pods | not in code | [mars-api.mjs L72-L74](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-api.mjs#L72-L74) (only warm-related code) |
| LLM analyzer: inline annotation vs separate LLM call | not in code | n/a (SDK) |

## 5. In the code but not in the documentation

- The session key is deliberately exported to the agent's terminal and seeded as a secret (`OPENHANDS_AUTOMATION_API_KEY`), and the system prompt tells the agent to use it.
- Every saved secret is attached to every conversation as a `LookupSecret` whose headers carry the master key.
- Approval requests carry only `{accept}`; no action id, no reason.
- Server-level bash calls from the UI (login probes, git probes) that bypass conversations and policy.
- `.openhands/hooks.json` from the workspace is auto-attached without a trust prompt.
- The planner mode (three tools, `NeverConfirm`).
- If `server_info` fails, the start payload falls back to a host `LocalWorkspace`.
- The client can pass `tool_module_qualnames` for the server to import.
- MARS bridge: same-origin, path-scoped `HttpOnly` cookie derived from the key, `Host` check against DNS rebinding, token store keychain/AES-GCM file/memory.
- The careful KV-secret pinning protocol (symlink/FIFO refusal, link-based atomic create, lock directory).
- The VS Code connection token equals the master key and shares the Canvas origin.

## 6. Could not find or could not settle

- Agent loop, tool validation, analyzers (including whether `LLMSecurityAnalyzer` makes a separate call), confirmation enforcement, `sanitized_env()`, output masking: all in `OpenHands/software-agent-sdk`; would be settled by reading `openhands-sdk` (security, conversation) and `openhands-tools`.
- Per-conversation `docker run` flags, mounts, user, network, image pull policy, whether `container.memory/cpus` are clamped by `OH_CONVERSATION_CONTAINER_*`, the outer-server proxy and idle stop: `openhands-workspace` / `openhands-agent-server` in the SDK repository.
- Whether the agent-server checks the key on WebSocket upgrade or only on the first `auth` frame, and what `expose_secrets=plaintext` requires: agent-server auth middleware.
- Whether the start request (with `LookupSecret` headers) is persisted to disk: agent-server conversation persistence.
- ACP permission mapping (auto-approve or confirmation state): the SDK's ACP agent.
- How a Docker-only server treats a `LocalWorkspace` payload: agent-server start handler.
- Deferred init and warm pools: SDK agent-server (not in this repository).
- Enterprise code and licence: no enterprise directory here; repository not identified.

## 7. Citation check

`research/round2/openhands.safety.md: 151 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`


## 8. Review

Independent review against the clone at the pinned commit. Checked 25 of 32 findings, plus the structural "not in this repository" claim; the Round 1 table rows for the ones checked were followed too.

| Finding | Verdict | Reason |
|---|---|---|
| Structural: repository holds only client, launchers, Dockerfile, Helm chart | absence confirmed | Listed top level and `src/`, `scripts/`, `docker/`, `helm/`, `tools/`; the only Python is `tools/canvas_ui_tool.py` plus tests and mocks; no loop, analyser, `docker run` or `no-new-privileges` outside docs and tests |
| 500 iterations, stuck detection on | supported | adapter L176-L180, payload sets both |
| Client tools executed by the browser | supported | ledger, result channel and dispatch read as described |
| `launch_child_conversation` no gate, no depth or count limit | supported / absence confirmed | grepped `src`, `tools`, `docs`, `scripts` for depth, nest, child cap: nothing |
| Default confirmation policy `NeverConfirm` | partly | mapping and defaults right; the "settings fetch fails, defaults" edge case applies to the display path only; start path uses the server's stored settings and throws on failure (corrected) |
| Only LLM analyzer changes confirmation | supported | `getConversationConfirmationPolicy` and analyzer switch |
| Planner runs `NeverConfirm` with three tools | supported | adapter L1352-L1363, L1401 |
| Approval answers whatever is pending | supported | body is `{accept}` only; "awaiting action" is latest agent event |
| No command parsing or hard-deny layer | absence confirmed | `shell-quote` only in `acp-command.ts`, quote direction |
| Server-level bash outside a conversation | supported | `AcpService.getAuthStatus` calls `BashClient.executeCommand` at server level |
| Repo `hooks.json` attached without trust prompt, fails open | partly | attached and no prompt confirmed; the 5 s is the load call, not hook execution; failure is a `console.warn`; hooks skipped for container workspaces (corrected) |
| Plugin launch trust checkbox | supported | modal L396-L424, translation text |
| Canvas Apps same-origin import | supported | blob `import()`; extension host also exposes `agentServer.request` |
| Default stack runs as the user | supported | spawn with merged `process.env` |
| Docker mode env pass-through | supported | dev-safe L1009-L1028 |
| Container limits forwarded without bounds | supported | adapter L804-L835 |
| Docker mode binds `0.0.0.0` | supported | dev-with-automation L528-L543 |
| `server_info` failure gives `LocalWorkspace` | supported | 3 s probe returns null; adapter L987-L996 |
| Docker all-in-one one uid, keys beside data | supported | Dockerfile `USER openhands`; entrypoint key files |
| Helm single pod, empty securityContext, RBAC options | supported | values.yaml and rbac.yaml |
| MARS bridge | partly | mechanism confirmed; omitted that the bridge is an experimental opt-in (`MARS_WEB=1`, off by default) (corrected) |
| Session key in the agent's environment and system prompt | supported | env at L975-L985, seeded as secret, prompt text built in the adapter from `runtime_services` and sent as `system_message_suffix_append`; only present when the automation service is advertised |
| `LookupSecret` carries the session key header | supported | `buildCustomSecrets` uses `buildAuthHeaders(backend)` |
| Session key injected into HTML and `localStorage` | supported | static-server L388-L406; policy in bind-host.mjs |
| WebSocket first-frame auth | supported | only `after_seq` in the URL |
| No accounts, one shared key, ingress unauthenticated | supported | ingress has no auth code |
| Round 1 table: ConfirmRisky default `corrected`; plan mode `refuted`; child launch `corrected` | supported (with the NeverConfirm caveat above) | |

Not checked: lock-icon finding (the component does read only the global setting, but the stale-state claim was not traced), provider-key round trip, key-generation protocol detail, KV pin, VS Code token, `tool_module_qualnames`, ACP detection, deferred-init absence.

Trust in the unchecked remainder: reasonable. The pattern is that client-side mechanisms are described accurately, and the main error type is overreach on edge-case behaviour that depends on server code. Treat any "silently", "fails open" or "default" claim that leans on server behaviour as unconfirmed. Noticed in passing and not added as a finding: the planner conversation also receives all saved secrets (with the session key header) through the same `buildCustomSecrets` call.

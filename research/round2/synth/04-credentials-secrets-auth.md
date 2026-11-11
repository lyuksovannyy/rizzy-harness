# 04. Credentials, secrets and authentication of the harness's own servers

Round 2 synthesis, research date 2026-11-10. Source: the Round 2 per-repository files in
`research/round2/`, each pinned to one commit (`research/round2/00-repos.tsv`). Nothing here
is new research; every claim comes from an input file, with its permalink copied as given and
its trust label.

Trust labels used below:

- **checked in review**: a reviewer opened the cited code and agreed.
- **corrected in review**: a reviewer changed the finding; the corrected text is used.
- **unchecked**: the author read the code; no reviewer checked it.
- **inferred**: the author reasoned from code without tracing the behaviour.
- **absence confirmed**: a negative claim a reviewer searched for and confirmed.

Terms used throughout:

- **Child environment**: the environment variables a process the harness starts (shell
  command, hook, MCP server, plugin) receives.
- **Credential broker**: a component that holds a real secret and injects it into outbound
  traffic, so the process that makes the request never holds the secret.
- **Model-call proxy (gateway)**: a server that accepts model requests from clients and calls
  the provider with a key the clients never see.
- **Worker credential**: what a remote executor (node, worker, executor, device) presents to
  the control side.
- **Trust gate**: a check that decides whether project-supplied files (config, hooks, plugins,
  MCP servers, skills) are loaded at all.

## 1. Scope

The group: where provider keys live at rest and in memory and what a tool's shell can read of
them; the child environment, including MCP servers; output and log redaction; egress proxies
and credential brokers; model-call proxies and gateways (data path, buffering, memory bounds,
per-key scopes, fail-over); worker credentials (per session, per dispatch, lifetimes, hashing,
rotation, revocation); keys an agent is handed to its own control plane; authentication of
local servers, daemons and hubs (tokens, cookies, Origin checks, loopback assumptions, tokens
in URLs, secrets in browser storage); pairing codes and tokens derived from shared secrets;
project and extension trust gates.

Files read (findings in this group and every file's section 8):

| Project | Files and sections |
|---|---|
| Codex | `codex.safety.md` 3.4 (proxy parts), 3.5, 3.6, 3.7, 3.8; `codex.sessions.md` 3.3, 3.4, 3.7 (OAuth), 3.8 |
| OpenCode | `opencode.safety.md` Running commands elsewhere, Secrets, Hooks and trust; `opencode.sessions.md` Client and server split, Pairing, MCP client |
| OpenClaw | `openclaw.safety.md` Running commands elsewhere, Secrets; `openclaw.sessions.md` Client and server split, Remote workers, webhooks, MCP; `openclaw.agents.md` auth profiles |
| Hermes Agent | `hermes-agent.safety.md` Running commands elsewhere, Secrets, Prompt injection; `hermes-agent.sessions.md` Client and server split, relay, MCP client, Accounts and trust; `hermes-agent.agents.md` 3.10 credential pools, summariser redaction |
| Oh My Pi | `oh-my-pi.safety.md` Secrets, Hooks and trust; `oh-my-pi.sessions.md` export, RPC, broker, gateway, other servers, MCP OAuth and config, accounts, memory back ends |
| Pi | `pi.safety.md` Project trust gate, Sandboxing (env), pi-env, Secrets; `pi.sessions.md` RPC, project trust, web UI (`apps/p2`) |
| DeepSeek harness | `deepseek-harness.safety.md` 20 to 25; `deepseek-harness.sessions.md` 3.3, 3.6, 3.7 |
| goose | `goose.safety.md` shell environment, hooks and trust, extensions, Secrets; `goose.sessions.md` Client and server split, Roaming, MCP and extensions, Skills and plugins |
| Cline | `cline.safety.md` 3.2 (hub approvals), 3.4, 3.5, 3.8 to 3.11; `cline.agents-sessions.md` hub, SSH remote and core/host split, MCP and trust |
| OpenHands | `openhands.safety.md` scope note, Hooks and trust, Sandboxing (bind, Docker), Running elsewhere (MARS), Secrets, Server authentication; `openhands.agents-sessions.md` client/server split, remote workers, ACP |
| Zed | `zed.md` 3.3 (fetch and terminal egress), 3.4 (ACP spawn) |
| Warp | `warp.md` 2, 3.6, 3.7, 3.8 |
| OpenCodex | `opencodex.md` A (1 to 7), B (8 to 17), C and D (18 to 25, as they bear on keys and fail-over), F (29 to 37) |

What was not available:

- **OpenHands**: this repository is only the Agent Canvas client and its launchers. The agent
  loop, the Agent Server's auth middleware, `X-Session-API-Key` checks, `expose_secrets`,
  output masking and `sanitized_env()` live in `OpenHands/software-agent-sdk`, which was not
  cloned (`openhands.safety.md` section 1). Everything below about OpenHands is client side.
- **Warp**: the agent loop and model call are server-side; the self-hosted worker is the
  separate `warpdotdev/oz-agent-worker` repository, not cloned. Warp's account is the client.
- **Zed**: the files read cover sandboxing, egress and the ACP client. Zed's own provider-key
  storage and its collaboration server authentication were not in the Round 2 scope for Zed.
- Vendor relays and registries (Codex remote control and exec-server registry, Hermes relay
  connector, OpenCode's OpenTunnel relay, goose roaming relays) are hosted services; only the
  client side is in the code read.

## 2. Implementations by project

### Codex

**Keys at rest.** For MCP OAuth, tokens go to the OS keyring, else
`CODEX_HOME/.credentials.json`; mode `Auto` pins the store chosen at first load "so refreshes
cannot switch to a possibly stale credential source"; the `File` mode doc says the file is
readable by any process of the same user.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/oauth.rs#L1-L17 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/types.rs#L150-L166
(unchecked). The OAuth login callback is a `tiny_http` server on `127.0.0.1` with `state` and
a 300 s timeout:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/perform_oauth_login.rs#L379-L410
(unchecked). Where the provider key itself is stored was not part of the files read.

**Child environment.** `populate_env` starts from `inherit` (`all` by default; `core` keeps
PATH, SHELL, TMPDIR, HOME, LANG, LC_*, LOGNAME, USER). The `*KEY*`/`*SECRET*`/`*TOKEN*`
excludes apply only if `ignore_default_excludes` is false, and the default is true. Then
custom excludes, `set`, `include_only`, `CODEX_THREAD_ID`, and finally an always-strip list of
Codex launch secrets (exec-server Noise token, Node REPL token, Guardian decisions key,
workload-identity variables). `OPENAI_API_KEY` is not on that list and reaches commands by
default.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L14-L28 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L105-L175 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/shell_environment_policy.rs#L133-L136
(checked in review). Background Git operations for plugins remove `GIT_DIR`, `GIT_CONFIG*`,
`GIT_WORK_TREE` and others from the child environment so a hostile launch project cannot
redirect them; user-initiated operations keep them:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core-plugins/src/git_policy.rs#L7-L50
(unchecked). The scoped exec-server deliberately does not start telemetry exporters because
"configured exporters may contain account-wide credentials":
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L253-L256
(checked in review, as part of the executor finding).

**Redaction.** One four-regex best-effort function (OpenAI `sk-` keys, AWS access key ids,
bearer tokens, `key|token|secret|password = value`):
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/secrets/src/sanitizer.rs#L1-L22.
It is called from memory generation, a login error path and client-facing command
presentation (`item_builders.rs` L58, L156-L182). No call was found on the tool-output path to
the model or the rollout (corrected in review: the client-presentation call sites were added;
the absence on tool output was searched with ripgrep).

**Process hardening.** `pre_main_hardening` (non-dumpable, core size 0, strip `LD_*`/`DYLD_*`)
exists but is used only by `responses-api-proxy`, `voice-host` and the Linux proxy bridge, not
by the main binary:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/process-hardening/src/lib.rs#L7-L61
(absence confirmed in review).

**Credential broker in the network proxy.** Off by default; enabling it forces MITM on. Known
credentials in the child environment (`GH_TOKEN`/`GITHUB_TOKEN`, enterprise GitHub tokens bound
to `GH_HOST`, `OPENAI_API_KEY` bound to `OPENAI_BASE_URL`, configured providers) are replaced
with shaped dummy values, and the proxy writes the real value into the request header only for
the host the credential is bound to. A failure-mode setting decides whether unmasked
credentials left in the environment are warned about, removed, or reject the launch. Managed
proxy env and dummies are stripped before escalated (unproxied) launches.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/config.rs#L185-L228 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/credential_broker.rs#L43-L57 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/credential_broker.rs#L1440-L1484 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/credential_broker/providers/github.rs#L13-L50 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/proxy.rs#L700-L706.
Label: checked in review in part (the defaults and the forced MITM in `config.rs`; the
reviewer did not open `credential_broker.rs`).

How the proxy is the only way out, on Linux: with managed networking, bwrap unshares the
network namespace; the outer helper forks one host-side bridge per proxy endpoint (exact env
keys, loopback endpoints and allow-listed ports only); inside, a loopback listener is passed
over a Unix socketpair (SCM_RIGHTS) to the host bridge, which connects to the real proxy and
writes an attribution token first, at most 128 concurrent connections; seccomp allows only IP
sockets. A missing or unparseable proxy env in managed mode is a hard error.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/proxy_routing.rs#L59-L121 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/proxy_routing.rs#L131-L206 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/proxy_routing.rs#L359-L414
(unchecked). The proxy re-checks every resolved address at `connect` (DNS rebinding closed for
direct dials), but skips the check when an upstream proxy is configured:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/connect_policy.rs#L124-L153 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/connect_policy.rs#L199-L223
(checked in review).

**Worker credentials: the exec-server.** `codex exec-server --remote <registry>` registers with
a hosted registry and keeps an outbound WebSocket to a relay. Registry auth comes from a
ChatGPT token on stdin, an agent-identity token, AWS SigV4, a one-shot bearer on stdin, or
"rotating executor credentials" read from stdin; the process exits when the pipe closes.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/remote.rs#L72-L88
(corrected in review: the direct `--listen ws://IP:PORT` mode takes the same auth settings but
they are optional there). The channel is a Noise `hybridIK` handshake (X25519 + ML-KEM-768,
AES-GCM); the harness pins the executor's key from the registry, and the executor asks the
registry whether the harness key is authorised (10 s timeout, 32 pending validations) before
completing; 8 failed handshakes cool down for 10 s; frames are capped at 256 KiB before
parsing.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/noise_channel.rs#L1-L37 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/relay.rs#L640-L690
(unchecked). The policy direction is the inverse of ours: the exec-server applies the sandbox
the caller describes, and a request without `sandbox` runs unsandboxed:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L142-L172
(checked in review).

**Remote control.** The app-server enrols with a ChatGPT-hosted service, receives a
`remote_control_token` with `expires_at`, and keeps an outbound WebSocket; the relay URL must
be a `chatgpt.com`/`chatgpt-staging.com` host or localhost, which stops a poisoned config from
redirecting the token:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L21-L56 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L209-L226
(unchecked). Pairing-code expiry is server-side and not in the repository.

**Server authentication.** Transports: stdio (default), a `unix://` control socket, a
`ws://IP:PORT` listener, and remote control. WebSocket auth modes are `capability-token` (a
token file or its SHA-256, constant-time compare) and `signed-bearer-token` (HS256 JWT, secret
of at least 32 bytes, 30 s skew), checked once at the HTTP upgrade, before `initialize`. Any
request with an `Origin` header gets 403. Credentials are loaded once at startup. Expiry and
revocation are not enforced after the socket is established.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L130 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L236-L237
(checked in review). A non-loopback app-server listener without auth refuses to start:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L279-L284.
Corrected in review: this refusal covers the app-server only. The exec-server `--listen
ws://IP:PORT` listener never calls the predicate, its module comment says unauthenticated
listeners remain supported for compatibility, and it serves process and file RPCs. Loopback
`ws://` with no auth is accepted for both, so any local process can drive the agent, including
a sandboxed command whose network is fully enabled (inferred).

The Unix control socket has no token; it lives under a hashed path in a directory that must be
0700 and owned by the user, the socket is 0600, and a client accepts the advertised symlink
only if it points at the expected protected path, so an SSH-forwarded or attacker-bound socket
is not trusted:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L71-L145 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352
(unchecked). The remote TUI reads its bearer only from a named environment variable and refuses
it unless the URL is `wss://` or loopback `ws://`; Unix endpoints cannot carry a token:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L2649-L2676
(checked in review).

**Trust gates.** Project trust (`[projects.<path>] trust_level` in user config) disables the
project layer's config, hooks and exec policies when not trusted:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/loader/mod.rs#L1100-L1170
(checked in review). Repo skills under `.agents/skills` load with no trust check (inferred from
absence, checked in review). Hook trust is a hash of the normalised `{event, matcher, handler}`;
plugin hooks add a SHA-256 over the plugin directory, re-verified before each execution; trust
state is read only from the User and SessionFlags layers, so a repository cannot write its own
approval. The hash does not cover the bytes of a script a non-plugin hook points at, so
`./scripts/check.sh` can change under an approved hook. `--dangerously-bypass-hook-trust`
exists for automation.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L836-L882 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L915-L925 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/config_rules.rs#L10-L30 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/plugin_trust.rs#L27-L69
(checked in review). Legacy plugin approvals migrate by trust on first use, and remote plugins
listed for the signed-in account are trusted automatically:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/plugin_trust.rs#L97-L115 ;
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/effective_plugin_change.rs#L80-L102
(checked in review, part of the hook-trust finding).

### OpenCode

**Keys at rest.** Provider credentials (API keys, OAuth tokens) are rows in a SQLite
`credential` table, `value` a JSON column, no encryption, no keychain; the database and its
WAL/SHM are chmod 0600 on POSIX. The agent runs as the same user and can read the file.
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/credential/sql.ts#L5-L14 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/database/database.ts#L85-L100
(checked in review).

**Child environment.** Shells get the whole server `process.env` (or a per-session map a
client set) plus `TERM`, `OPENCODE_TERMINAL=1`, `AGENT=1` and session variables. Plugins can
edit the environment in `shell.create.before`, before the permission check.
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L331-L348 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L253-L258
(checked in review). When a workspace driver is bound, shells start with no host environment;
the default driver registry is empty:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L26-L62
(checked in review). MCP stdio servers are spawned through the location's `Environment`:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/mcp/stdio.ts#L10-L16
(checked in review; OAuth details not read).

**The control-plane secret in the agent's reach.** `OPENCODE_PASSWORD`/
`OPENCODE_SERVER_PASSWORD` are deleted from `process.env` only in `stdio` mode ("Keep the lease
credential out of the environment inherited by tools"). In `serve` mode an environment password
stays and every shell inherits it; in service mode the password is generated once and stored in
a 0600 service config file the agent can read. With default allow on shell, a prompt-injected
agent can read it and call the local API to answer its own pending approvals (inferred).
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L85-L97 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/service-config.ts#L175-L191
(checked in review; the self-approval path is inferred).

**Redaction, broker, proxy.** None found in tool output; `HTTP(S)_PROXY` is passed through like
any variable; shell output streams in full to a file:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L398-L413
(inferred from search; not checked in review).

**Server authentication.** The server will not start without a password; it binds
`127.0.0.1` by default. Credentials are Basic auth with the fixed user `opencode`, or a session
cookie, or `?auth_token=` (a base64 `user:password` in the URL). The raw password comparison is
`===`; only the session-token HMAC uses `timingSafeEqual`. Foreground `serve` prints the
password to stdout when no env password was given. There is one fixed identity: `authorized`
returns a boolean, handlers get no identity.
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/process.ts#L55-L58 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L13-L40 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L33-L37 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L46-L55 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L11-L14
(checked in review; absence of per-session scoping confirmed). Browsers are kept out of
cross-site calls by a `Sec-Fetch-Site` check plus a CORS allow-list that admits any
`http://localhost:*`, `http://127.0.0.1:*`, `tauri://localhost`, `oc://renderer` and
`https://*.opencode.ai`:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/cors.ts#L11-L20 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/request-site.ts#L15-L24.
The in-memory SDK router (`ServerFetch`) has no check at all; the embedder must front it:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/fetch.ts#L34-L36.

Service discovery: `service.json` (url, pid, version, password, instance id) under the state
dir, mode 0600, re-checked every 5 s; the server shuts down if the file no longer names it:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/service-registration.ts#L39-L74
(corrected in review on the default port only). `--standalone` passes a random password to a
private child in its environment, the child deletes it from its own environment, and exits on
stdin EOF:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L87-L90
(checked in review).

**Pairing.** `POST /api/pair` (authenticated) mints `randomBytes(16)` base64url into an
in-memory cache (capacity 1000, TTL 5 min). `GET /auth/connect/:code` is unauthenticated,
atomically removes the code, and issues a session token: browsers get a 302 with an
`httpOnly`, `sameSite=lax`, 30-day cookie with no `Secure` attribute; JSON clients get the
token to use as a Basic password.
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/pairing.ts#L7-L34 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/server.ts#L31-L55
(checked in review). The token is `"<expiry>.<HMAC>"` keyed by a key derived from the server
password. It is accepted anywhere the password is, has no identity, no list and no individual
revocation; "rotating the password revokes every session". A paired phone can mint further
pairing codes.
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L39-L55 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L63-L66
(checked in review). Remote access over SSH reads `service*.json` back over the channel to get
the loopback URL and password, filters SSH options to an allow-list, and scrubs the
registration block from bootstrap output before it can reach error text:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/command.ts#L236-L243
(checked in review). `--remote` uses a third-party tunnel with a secret 16-hex subdomain plus
the password; end-to-end encryption cannot be settled from this repository (unchecked).

**Trust gate.** None. Project `.opencode/plugin(s)/` are loaded as in-process server plugins;
the only gate is an administrator `plugin.use` policy; command templates run `` !`cmd` `` with
no permission check, after argument substitution.
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136 ;
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L165-L227
(checked in review).

### OpenClaw

**Keys at rest and auth-profile rotation.** Provider credentials are auth profiles. Within a
provider, profiles are ordered by explicit order, then type (OAuth, token, API key), then
expiry and least-recently-used; cooldown depends on the failure reason: `rate_limit` follows
only the provider's retry-after, other transient errors 30 s, 1 min, 5 min, `billing` 10 min
doubling to 24 h in a separate `disabledUntil` window, `auth_permanent` 10 min doubling to 1 h;
cooldown state is persisted in SQLite.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/order.ts#L190-L286 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L18-L46
(checked in review). Round 1 noted that plaintext in config or `.env` stays readable by the
agent; the code confirms that and adds the next point.

**Child environment.** Host exec (Gateway or node host) copies `process.env` and drops about
110 "blocked everywhere" keys (`NODE_OPTIONS`, `PYTHONPATH`, `BASH_ENV`, `GIT_*` executables,
the `LD_`, `DYLD_` and `BASH_FUNC_` prefixes), node-service identity variables, and the names
the Gateway reads its own credentials from via `${NAME}` references. Only the hooks token and
the database engine register those names, so `ANTHROPIC_API_KEY` in the Gateway's environment
is inherited by `exec`. Request overrides of `PATH` and other dangerous keys are rejected.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L277-L309 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-spawn.ts#L63-L70
(checked in review). Sandbox runs get none of the inherited env, only config-declared vars,
value-checked:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/sanitize-env-vars.ts#L113-L118.
SSH children get `sanitizeEnvVars`, which blocks `*_API_KEY`, `*_TOKEN`, `*_SECRET`,
`*_PASSWORD` and known provider names:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/sanitize-env-vars.ts#L15-L31 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/secret-env-name.ts#L1-L2.

**Secret egress proxy.** A granted exec gets `HTTPS_PROXY`/`HTTP_PROXY` =
`http://openclaw:<token>@127.0.0.1:<port>` plus a local CA bundle in `NODE_EXTRA_CA_CERTS`,
`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, `GIT_SSL_CAINFO`. Each secret is a
binding of name, sentinel and `allowedHosts`; the proxy rewrites sentinels in request bodies
across chunk boundaries and refuses unresolved sentinels or hosts not on the binding's list.
The per-process grant is revoked on cancel or exit. Durable exec, sandbox exec and PTY exec
refuse secret-egress grants. A process that ignores the proxy env bypasses it; no network
namespace was found on this path (inferred).
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/environment.ts#L3-L14 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/proxy-server.ts#L58-L67 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/registry.ts#L40-L47 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/stream-substitution.ts#L26-L31 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-runtime.ts#L593-L631
(checked in review).

**Model-call proxy for workers.** `worker.inference.start` carries `modelRef` plus context.
The Gateway refuses a model ref that differs from the approved one, any `provider/model@profile`
suffix (a worker cannot pick an auth profile), a provider change after alias resolution, a
session mismatch, or `runEpoch != ownerEpoch`, then streams with the key held in the Gateway
process. Pending and terminal inference turns are persisted for replay. A worker-local
inference mode keeps endpoint and credentials on the node; the Gateway still approves the model
ref. Minting a new credential cancels in-flight inference for the environment.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L122-L153 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L180-L194 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L34-L89 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L2642-L2660
(checked in review).

**Worker credentials.** Step by step:

1. Mint: 32 random bytes, registered for log redaction, stored only as
   `sha256Base64Url("openclaw-worker-credential-v1\0" + credential)`, or for a session turn
   `sha256(domain-v1-turn \0 serialized turn claim \0 credential)`, binding the secret to the
   exact durable claim. TTL 10 minutes. One row per environment (primary key `environment_id`),
   so minting replaces the old one.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60 ;
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126 ;
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L2598-L2608
   (checked in review).
2. Renew preconditions: owner epoch unchanged, environment `ready`/`idle`/`attached`, no
   destroy requested, a bootstrap receipt exists; a credential cannot be marked delivered after
   it expired.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store.kernel.ts#L382-L421.
3. Admission: the worker's first frame is a worker `connect` with an admission block. The
   Gateway hashes the presented credential, compares in constant time with the stored hash,
   then requires: environment ready and not destroyed; bundle hash equal in credential,
   bootstrap receipt and expected build; same version; same session; same owner epoch in
   admission, credential and environment; RPC set version 1; identical feature lists.
   Admission is serialised per client IP, only failures are charged to the rate limiter, and
   every public rejection is the opaque `invalid-handshake`.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L79-L186 ;
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L218-L262
   (checked in review; corrected in review: the Gateway does open outbound SSH to the worker box
   for bootstrap, tunnel control and workspace transfer, with `-F none`,
   `StrictHostKeyChecking=yes`, `IdentityAgent=none`, no forwarding:
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291).
4. Per frame: `validateWorkerConnection` re-checks the stored hash (constant time), expiry,
   environment state and `ownerEpoch` before every request, so a rotated credential or a newer
   owner fences a stale socket on its next frame; the expiry check is waived for an exact
   durable turn. The method list is closed; any other method closes with 1008.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L217 ;
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L691-L716 ;
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L90-L149
   (corrected in review: the durable-turn expiry exception).
5. Revocation: the owner epoch is `max(all epochs) + 1`, never reused; transitions out of
   `ready`/`idle`/`attached` delete the credential; a timer at expiry re-runs validation;
   `credential-replaced` and `owner-epoch-mismatch` are terminal on the worker.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-transitions.ts#L303-L350 ;
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-contract.ts#L22-L26
   (corrected in review: the epoch bump only applies from ready, idle or attached, and a mint
   does not bump it).

The worker endpoint schema forbids credentials in the URL (no userinfo, no query) and allows a
TLS pin only with `wss`:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-endpoint.ts#L45-L86
(checked in review).

**A secret that does reach the worker.** The launch descriptor carries a GitHub `token` (up to
2,048 characters), refreshed by heartbeat; the worker writes it into a per-turn `gh` profile
directory, resets credential helpers and registers it for redaction. The agent's shell can read
it. The owner-only check on the profile was seen on Windows only.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/github-binding.runtime.ts#L159-L198
(checked in review).

**Server authentication (Gateway).** The socket sends a `connect.challenge` nonce first.
Device proof is Ed25519 over device id, client id and mode, role, scopes, `signedAt`, the
presented token and the nonce, with 2 minutes of skew; signing the token binds the bearer to
the key.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/connect-auth.ts#L369-L401 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L205-L245
(checked in review). Roles are `operator` or `node`; an unclassified method requires admin:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/method-scopes.ts#L269-L317
(unchecked). Auth defaults to a 24-byte token generated at startup; mode `none` exists, and a
local backend client may skip device pairing under it:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/auth-resolve.ts#L55-L84 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L165-L187
(checked in review). Device tokens (32 random bytes, one per role) are stored in plaintext in
`tokens_json` and compared with `safeEqualSecret`; bootstrap tokens are plaintext with a
10-minute TTL; only browser pairing requests store `approved_token_sha256`:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/pairing-token.ts#L4-L18 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-token-policy.ts#L54-L108
(checked in review).

Browser storage: the shared Gateway token is in `sessionStorage` scoped to the origin, a
`#token=` fragment is accepted and `?token=` warns, but the browser's Ed25519 device seed and
the issued device tokens live in `localStorage`; a non-extractable WebCrypto key is not used.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/app/settings.ts#L287-L333 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/app/startup-settings.ts#L198-L221 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L239-L251
(checked in review).

Node pairing by SSH verification (on by default): a fresh, scopeless `role: node` request from
a private or CGNAT address is auto-approved if the Gateway can SSH in with `BatchMode=yes` and
`StrictHostKeyChecking=yes` and `sh -lc 'openclaw node identity --json'` prints a matching
device id and key. Anyone who can make that host print the JSON line (a shell profile, since
`sh -l`) satisfies it (inferred).
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L61-L86 ;
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.runtime.ts#L20-L45
(checked in review). Webhooks reject `?token=` with 400, accept only a header token compared
with `safeEqualSecret`, rate-limit failures per client with loopback not exempt:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L220-L256
(unchecked). MCP OAuth may run as the operator or as the requester:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/mcp-transport.ts#L82-L200
(unchecked).

**Trust gate.** Plugin install trust and workspace-origin plugins were not read in the files of
this round (`openclaw.safety.md` "Hooks and trust prompts").

### Hermes Agent

**Keys at rest.** Credential pools per provider (API keys or OAuth tokens) are persisted under
`credential_pool` in `auth.json`, merged with concurrent writers; reference-only rows are
secret-free on disk. Strategies `fill_first` (default), `round_robin`, `random`, `least_used`;
401 benches for 5 minutes, 429 and other failures for one hour, a sole credential for 60 s
unless billing; per-model 429 benches only the (credential, model) pair.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/credential_pool.py#L118-L138 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/credential_pool.py#L2255-L2314 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/auth.py#L1081-L1106
(unchecked). Profiles separate homes and secrets; when one process serves several profiles,
`get_secret` raises, rather than falling back to `os.environ`, if no scope is bound:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/secret_scope.py#L1-L10
(unchecked).

**Child environment: two policies.** Terminal children (local) inherit the host environment
minus Hermes-managed names: a static list (provider keys and base URLs, messaging secrets,
`GH_TOKEN`, Modal, Daytona and Vercel tokens, dashboard secrets), names derived from the
provider registry, adapter secrets, and dynamic `AUXILIARY_*_API_KEY` and `GATEWAY_RELAY_*`.
The AWS chain is deliberately inherited and `CLAUDE_CODE_OAUTH_TOKEN` is deliberately not
stripped. `execute_code` children keep only safe prefixes (`PATH`, `HOME`, `LANG`, `LC_`,
`XDG_`, ...) and drop any name containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `CREDENTIAL`,
`PASSWD`, `AUTH`, `DSN`, `WEBHOOK`, `CREDS`, `BEARER`, `APIKEY` or `COOKIE`.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/local_env_policy.py#L12-L52 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/local.py#L275-L306 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_env.py#L57-L92
(unchecked). Skill- or config-declared passthrough names cannot register a Hermes-managed
credential (the fix for GHSA-rhgp-j443-p4rf), re-checked at use time and failing closed if the
blocklist import fails:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/env_passthrough.py#L46-L79.
`docker_forward_env` is an operator opt-in that bypasses the blocklist:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/remote_common.py#L23-L62.
Docker passes values through the docker client's own environment with name-only `-e KEY`, so
they stay out of `/proc/*/cmdline`:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L539-L542
(checked in review). SSH passes values in the ssh client's environment with `SendEnv`, but
syncs credential files to the remote `~/.hermes` before each command (checked in review).

MCP stdio servers start from `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR` and `XDG_*`
plus the server's own `env:`; HTTP bodies are capped at 10 MiB; auth is stripped on
cross-origin redirects; error text passes a credential redactor:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_common.py#L85-L88
(checked in review).

**Redaction.** Terminal output is truncated, stripped of ANSI, then `redact_terminal_output`:
vendor-prefixed keys, auth headers, JWTs and private keys always; the `KEY=value` pass only for
env-dump commands (`env`, `printenv`, `set`, `export`, `declare`). File reads replace prefixed
keys with a non-reusable sentinel so the agent cannot write a masked key back into config. On
unless `HERMES_REDACT_SECRETS` or `security.redact_secrets` is false; `force=True` call sites
ignore the switch; logs are redacted. Redaction runs after an output-transform plugin hook, so
the hook sees raw output.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/terminal_tool_result.py#L227-L243 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/redact.py#L1491-L1530 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_logging.py#L175-L176
(unchecked). The compaction summariser prompt forbids credentials and its inputs and output pass
a redactor:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L3890-L3935
(unchecked).

**Credential broker: iron-proxy.** `hermes egress setup` mints one token per provider,
`<prefix>-<32 hex>`, into `mappings.json` (0600); `proxy.yaml` gets one `secrets` rule per
mapping: on the listed hosts, replace the token in the listed headers (`Authorization`,
`x-api-key`, `api-key`, `x-goog-api-key`) and in query strings, with `require: true` so a
request without the token is rejected there; an allowlist transform admits only inference
hosts. iron-proxy reads the real keys from its own environment, built from an allowlist plus
the mapped names (optionally refetched from Bitwarden with no cache, failing closed). It binds
the `docker0` IPv4 (validated private, non-loopback) or `127.0.0.1`, never `0.0.0.0`;
management on loopback with a bearer; upstream SSRF denies cover loopback, link-local and
metadata, RFC 1918, ULA, IPv4-mapped IPv6, CGNAT, 198.18/15. The CA key is a 10-year RSA-4096 in
`~/.hermes/proxy/ca.key` (0600). A binary on PATH wins over the pinned, checksum-verified one.
SigV4 and service-account credentials are listed as uncovered.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L47-L81 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L433-L478 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L808-L830 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L400-L430 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L162-L177
(checked in review).

What it covers: the Docker backend only. With `proxy.enabled`, Docker gets the CA mounted
read-only, `HTTPS_PROXY`/`HTTP_PROXY` to `host.docker.internal`, CA variables,
`NODE_OPTIONS=--use-openssl-ca`, and the tokens under the real provider names. With
`enforce_on_docker` (default true) a half-configured or stopped proxy refuses to start the
sandbox, and `docker_forward_env`, `docker_env`, `docker_extra_args` that would override those
names (including `-e`, `--env-file`, `--network`) are rejected. What bypasses it: Hermes' own
model calls on the host use the real keys directly; the container stays on the default bridge,
so a process that ignores `HTTPS_PROXY` reaches the internet directly (holding only useless
tokens); Modal, Daytona and SSH are not wired despite the module docstring.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L786-L812 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker_egress.py#L29-L76 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker_egress.py#L180-L211
(checked in review).

**Server authentication.** The local control socket (`$HERMES_HOME/gateway.sock`) reads the
peer uid; only a same-user peer gets a ticket. `TicketStore.mint` stores SHA-256 of a
`token_urlsafe(32)` ticket, 30 s expiry, a purpose (`interactive`, `native-http`,
`worker-adoption`, ...), a capability set, a subject and a scope, at most 1,024 outstanding;
`redeem` deletes on first use and checks profile and purpose. The WebSocket carries the ticket
in a subprotocol (`hermes-gateway-ticket.<ticket>`), not the URL.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L381-L409 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L25-L75 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L156-L173
(checked in review). A non-loopback dashboard requires an auth provider (RFC 1918 counts as
public), and `--insecure` is ignored; the source names a June 2026 incident; the Host header is
checked against the bound interface or declared public hosts; cookies are `HttpOnly` with
`__Host-`/`__Secure-` prefixes; all principals act as one owner.
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L111-L119 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662
(checked in review). The OpenAI-compatible API server binds `127.0.0.1:8642`, refuses to start
without a strong `API_SERVER_KEY`, compares with `hmac.compare_digest`, returns 429 above 10
concurrent runs; one shared key, no scopes:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L1428-L1435 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L3907-L3931
(checked in review). The relay upgrade bearer is
`base64url(gateway_id:exp:HMAC-SHA256(gateway_id:exp, secret))` with a 300 s TTL; the
connector keeps a rotating list of secrets; close code 4401 after a good handshake is terminal
revocation:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/auth.py#L36-L40
(unchecked; gateway side only).

**Trust gate.** A regex scanner at plugin install (`dangerous` blocks, `--force` does not
override; `caution` needs confirmation); plugins run in-process by default, `host` isolation
moves third-party plugin code into a separate process:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/plugin_guard.py#L1-L9 ;
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/plugin_isolation.py#L1-L18
(unchecked). Project context files with a threat-pattern hit are replaced by a `[BLOCKED: ...]`
marker:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/prompt_builder.py#L99-L111
(checked in review). The SSH sync-back can write new remote files into the host skills tree
(inferred; mechanism checked in review).

### Oh My Pi

**Keys at rest.** Credentials live in `agent.db` (SQLite), directory 0700, file chmod 0600 best
effort, no encryption; no read or bash refusal protects `agent.db`, the broker token file or
the placeholder key (absence by search):
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/sqlite-credential-store.ts#L588-L605
(checked in review).

**Child environment.** The spawn env is the process env, filtered only for names containing `=`
or NUL, macOS malloc variables, values that came from the project's `.env` files, and git
repo-location variables (`GIT_DIR` etc.), then `SHELL`, `GIT_EDITOR=true`, `CI=true` and a
non-interactive overlay. Nothing removes `*_API_KEY` or `*_TOKEN`. MCP stdio servers get
`{...Bun.env, ...config.env}`.
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/procmgr.ts#L43-L55 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/env.ts#L54-L69 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/non-interactive-env.ts#L6-L57
(checked in review).

**Redaction (obfuscation).** With `secrets.enabled` (default false), a `SecretObfuscator` is
built from env names matching `KEY|SECRET|TOKEN|PASSWORD|PASS|AUTH|CREDENTIAL|PRIVATE|OAUTH`
(values of 8+ characters), passwords in connection URLs, `secrets.yml` (global and project) and
credential regexes. Provider-bound messages carry HMAC-keyed placeholders (key in a 0600 file),
and `transformToolCallArguments` turns placeholders back into real values before execution, so
a placeholder written into a `curl` argument exfiltrates the real secret (inferred). A project
`secrets.yml` can override global entries.
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/secrets/settings.ts#L13-L25 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/secrets/index.ts#L182-L223 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L4814-L4824
(checked in review). `/share` redacts with the obfuscator before sealing with AES-256-GCM, key in
the URL fragment:
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/share.ts#L260-L282
(unchecked). Memory back ends call `redactMemorySecrets` before writing (call sites by grep;
not re-read in review).

**Auth broker (a credential distributor).** `omp auth-broker serve` binds `127.0.0.1:8765`
unless `--bind` (accepted with no warning; loopback is a default, not a rule), one random
32-byte bearer (0600 file), plain HTTP. Every route except `/v1/healthz` needs the bearer,
checked with `Set.has` (not constant time). A snapshot carries every credential: API keys
verbatim and OAuth access tokens in clear with refresh tokens replaced by `__remote__`. The same
bearer can upsert credentials. The broker refreshes OAuth tokens expiring within 5 minutes,
every 60 s. An empty token set disables auth in `startAuthBroker`. Unknown paths are logged as
`<unrouted>`, so a secret in a URL is not logged.
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/types.ts#L177-L184 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L109-L116 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/pool.ts#L715-L735 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L1054-L1078
(checked in review). Clients cache the snapshot on disk with AES-GCM keyed by SHA-256 of the
bearer, AAD = magic, version and broker URL; anyone with the token can decrypt:
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/snapshot-cache.ts#L279-L301
(checked in review).

**Auth gateway (a model-call proxy).** `omp auth-gateway` on `127.0.0.1:4000` accepts OpenAI
chat, Anthropic messages and OpenAI Responses requests (plus images, speech, transcription) and
calls providers with broker credentials resolved per request; callers never hold a key. Inbound
auth is one gateway bearer, or scoped "clients" configured only by the SHA-256 digest of their
token, each limited to a set of `provider/id` models and named OAuth accounts (no fallback to a
sibling account); gateway-wide routes are refused to scoped clients; a bearer outside the
`Authorization` header is rejected; a client-table reload leaves admitted requests on their old
scope. No token expiry was found. Responses carry `Access-Control-Allow-Origin: *`, `OPTIONS` is
unauthenticated, and `--no-auth` empties the token set, so any web page can call the loopback
gateway and read the response (inferred from the headers).
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/server.ts#L1022-L1068 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/dispatch.ts#L128-L141 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/http.ts#L306-L323 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L60 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/types.ts#L166-L175
(checked in review; routing after the auth check not traced).

**Other servers.** RPC mode is NDJSON over stdio, no authentication; the trust boundary is the
parent process. The browser relay binds `127.0.0.1`, rejects any WebSocket `Origin` on the CDP
side, requires a `chrome-extension://` origin on the extension side, and compares an optional
`?token=` with `!==`, skipped if no token is configured:
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160
(checked in review). No user accounts exist (absence checked in review). MCP OAuth is PKCE
`S256` with a loopback callback and dynamic registration; credentials keyed
`mcp_oauth:profile:<profile>:<serverUrl>`:
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/oauth-flow.ts#L20-L35
(unchecked).

**Trust gate: hard-coded open.** `isProjectTrusted()` is kept for compatibility with Pi and
returns `true` at both construction sites; `.omp/extensions`, `.omp/config.yml` and other
project inputs load unconditionally. TypeScript hooks in the project's `.claude/hooks/pre|post/`
and custom tools in `.claude/tools/` are imported as in-process modules at session start.
Project MCP servers from `.claude/.mcp.json`, root `mcp.json`/`.mcp.json`, `.omp/mcp.json`,
`.cursor/`, `.vscode/`, `.gemini/` start at session open (`mcp.enableProjectConfig` defaults
true).
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L1814-L1814 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L9771-L9771 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 ;
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/settings.ts#L9-L12
(checked in review; corrected in review on which provider reads which MCP file).

### Pi

**Keys at rest.** `<agent-dir>/auth.json`, directory 0700 and file 0600 on creation only, not
encrypted, no keychain, locked with `proper-lockfile`; MCP OAuth tokens in `mcp-auth.json`;
`--api-key <key>` on the command line is visible in the process list; the `read` tool can open
`auth.json`.
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/auth-storage.ts#L68-L79 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/runtime-credentials.ts#L5-L24 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/cli/args.ts#L119-L120
(checked in review). A key or header value starting with `!` runs as a shell command (cached for
the process lifetime, 10 s timeout); `$VAR` is interpolated; project-level settings are inside
the trust gate, global ones are not:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resolve-config-value.ts#L185-L212
(checked in review).

**Child environment.** `getShellEnv()` is `{...process.env}` with the bin dir on PATH, plus
`PI_SESSION_ID`, `PI_PROVIDER`, `PI_MODEL` and similar; an extension `spawnHook` is the only seam
to scrub. MCP stdio servers inherit `process.env` unless `inheritEnv === false`, which the
coding agent never sets.
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/utils/shell.ts#L171-L183 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/bash.ts#L214-L240 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/mcp/src/transports/stdio.ts#L98-L103
(checked in review). The Gondolin micro-VM example passes the host env into the VM:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/examples/extensions/gondolin/index.ts#L325-L358
(checked in review).

**Redaction, broker.** None in the core; redaction exists only in `bug-report.ts`:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/bug-report.ts#L19-L61
(corrected in review: `apps/p2` has a log redactor for its server log buffer, not tool output).
Docker Sandboxes' placeholder credential at a proxy is documented only, no code:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/docs/containerization.md#L74-L86
(checked in review).

**Remote tools (pi-env).** The client spawns `ssh ... <daemon> serve --token <hex>`; the token is
a sync marker the daemon never checks; the security boundary is the SSH login. SSH flags include
`BatchMode=yes`, `ClearAllForwardings=yes`, `ForwardAgent=no`, `SendEnv=-*`,
`RemoteCommand=none`, `StrictHostKeyChecking=yes`, a private `UserKnownHostsFile` with explicit
`scanHostKey` and refusal of a changed key. The exec environment is built from an explicit
`shellEnv` option, never the client's `process.env`, so provider keys are not sent unless the
host puts them there.
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/connection.ts#L344-L345 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L59-L123 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L55-L58
(checked in review). pi-env is used only by the `apps/p2` prototype (corrected in review).

**Server authentication.** RPC mode is JSONL over stdio with no authentication (by
construction). The `apps/p2` prototype web UI binds `127.0.0.1`, judges every request by Host
(loopback or `P2_ALLOWED_HOSTS`), refuses proxy-marker or forwarded headers on loopback,
refuses cross-site Fetch Metadata except top-level navigations, rejects cross-origin requests;
sign-in is a one-use, ten-minute token printed in the server's terminal, exchanged for an owner
session cookie.
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/src/hosts/principals.ts#L17-L38 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/src/hosts/server.ts#L755-L766
(checked in review).

**Trust gate.** `resolveProjectTrusted`: CLI override, then user and CLI extensions through the
`project_trust` event, then the saved decision for the folder or nearest saved parent, then
`defaultProjectTrust` (default `ask`; headless `ask` answers false). `defaultProjectTrust` is
read from global settings only, so a repository cannot relax its own prompt.
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/settings-manager.ts#L1020-L1023
(checked in review). Gated: `.pi/{settings.json,mcp.json,extensions,skills,prompts,themes,SYSTEM.md,APPEND_SYSTEM.md}`
in the cwd and `.agents/skills` in cwd or ancestors. Not gated: `AGENTS.md`, `CLAUDE.md`, the
`sessionDir` lookup, tool calls, and a `.pi` in an ancestor directory only:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/trust-manager.ts#L30-L39 ;
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L679-L685
(checked in review). The store is `~/.pi/agent/trust.json`, a per-directory boolean with no
content binding; trusting a parent trusts everything below it; a `git pull` that adds
`.pi/extensions/` loads with no new prompt (inferred):
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/trust-manager.ts#L217-L254
(checked in review). Startup is two-pass: user and CLI extensions run before the trust
decision, project code only after; a failure during resolution leaves the project untrusted:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L531-L556
(unchecked).

### DeepSeek harness

**Keys at rest.** `$DSH_HOME/.credentials.yaml`, written atomically under a cross-process lock,
0600 in a 0700 directory; on POSIX a file readable by group or others makes the provider refuse
to start ("run chmod 600"); parse errors are described without quoting source lines. Resolution
order: that file, the process environment, the cwd `.env`, then `$DSH_HOME/.env`, so a checked-out
project can supply a provider key (a deliberate trust of the launch project per the comment).
The HMAC secret for browser cookies is a record in the same store.
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/credentials/credentials-local/src/index.ts#L1-L30 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/credentials/credentials-local/src/index.ts#L112-L146 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/credentials/credentials-local/src/index.ts#L677-L712
(checked in review).

**Child environment.** Every child starts from `scrubbedParentEnv()`: the parent environment
minus names matching `/KEY|PASSWORD|SECRET|TOKEN/i` and minus `DSH_*`, plus proxy variables for
child Node processes. MCP stdio uses the same scrub. Names such as `DATABASE_URL`,
`GITHUB_PAT` or a proxy URL with a password survive. The scrubbed keys sit in
`.credentials.yaml`, which every confined command can read because no profile restricts reads
(inferred from the profiles).
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subprocess/subprocess/src/index.ts#L40-L75 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/transport.ts#L12-L22
(checked in review).

**Redaction.** No output redaction, no broker. MCP config strings may use `${credential:...}`
templates; every resolved value is added to a redaction set for diagnostics and tool-error
text:
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/credentials.ts#L9-L47
(checked in review).

**Server authentication.** Each process mints a 32-byte launch token in memory and prints it as
`?token=` on the start URL; the URL goes to stdout and to the OS browser opener as an argument,
so it lands in process arguments and scrollback. `GET /?token=` with one matching token
(constant time) mints a cookie `v1.<base64url JSON {authority, issuedAt, expiresAt}>.<HMAC-SHA256>`,
named `dsh-auth-<sha256(authority)>`, `HttpOnly; SameSite=Strict; Path=/`, `Secure` only when the
listener itself serves TLS, 30 days by default, and redirects 303. The HMAC secret persists, so
cookies survive restarts; no logout or revocation exists except deleting the secret; anyone who
can read `.credentials.yaml`, including a confined `bash`, can forge a cookie.
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L61-L180 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L182-L199 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/src/index.ts#L146-L150
(checked in review). The fence, in order, on every `/api` request and upgrade: Host must be
loopback, the bind address or a `--trusted-host`; `Sec-Fetch-Site: cross-site` refused; an
`Origin`, when present, must equal the Host authority (`null` refused); then a valid cookie (403
for fence failure, 401 for auth). The fence "never establishes identity". Wildcard binds are
refused at config load ("binding every interface would expose remote code execution to the
network"); default `127.0.0.1:3080`.
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/api-request-trust.ts#L113-L176 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/rpc-host.ts#L122-L167 ;
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/host/webserver/src/index.ts#L357-L366
(checked in review). Behind a TLS-terminating proxy the cookie lacks `Secure`. Every principal is
`owner`; the owner cookie can add an MCP stdio server (an arbitrary command), so the cookie is a
code-execution credential (unchecked).

**Trust gate.** `plugin_manager` asks for an escalation per call; `approvedBuilds` (permission to
run install scripts) is a model-supplied argument that becomes a persistent permission;
installed plugins run in the host process:
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/boot/plugin-manager/src/tools.ts#L79-L111
(checked in review).

### goose

**Keys at rest.** `get_secret` returns the upper-cased environment variable if set, else
storage. Keyring storage keeps all secrets as one JSON blob under service `goose`, user
`secrets`. On a keyring availability error goose sets `GOOSE_DISABLE_KEYRING=1` in its own
environment and silently falls back to plaintext `~/.config/goose/secrets.yaml` (0600 temp file
and rename). Secrets in memory are plain `String`s; the flag it sets is inherited by every
child.
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L942-L956 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L999-L1020 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1262-L1281 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/private_file.rs#L240-L257
(checked in review).

**Child environment.** The Developer shell inherits goose's full environment; the only change is
`AGENT_SESSION_ID`:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L616-L622
(checked in review). Stdio extensions get the inherited environment plus their `envs`, with
`PATH` replaced and `envs` filtered through a 31-key disallow list (`PATH`, `LD_PRELOAD`,
`DYLD_*`, `NODE_OPTIONS`, `PYTHONPATH`, ...); the container path puts values on the `docker`
command line, visible in the process list.
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension.rs#L91-L122 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/stdio.rs#L46-L52
(checked in review). Git runs with `-c safe.bareRepository=explicit -c core.fsmonitor=false`:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/subprocess.rs#L34-L45
(checked in review). No redaction of secrets in tool output was found (author's search).

**Server authentication.** `goose serve` reads the secret only from `GOOSE_SERVER__SECRET_KEY`;
without it, it refuses to start unless `--dangerously-unauthenticated`. Every `/acp` request
carries the secret in `X-Secret-Key` or the `token` query parameter, compared with
`subtle::ConstantTimeEq`; no accounts, no per-client credentials, no expiry, no rotation. The
desktop authenticates its WebSocket with `?token=`. The MCP-app proxy compares its `secret`
with `!=` (those routes also require a loopback peer). Plain HTTP on `0.0.0.0` is accepted.
`/health` and `/status` are unauthenticated.
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L9-L36 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/mcp_app_proxy.rs#L225-L232 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1906-L1913
(checked in review). WebSocket upgrades with an `Origin` are checked against an allowlist (by
default loopback origins plus `null` and `file://`; wildcards rejected); requests without an
`Origin` pass:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/mod.rs#L27-L99
(unchecked). The desktop passes a fresh 32-byte secret to its loopback child in the environment
(not argv), then builds `wss://127.0.0.1:<port>/acp?token=<secret>` for the renderer:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/gooseServe.ts#L357-L372
(unchecked). There is no per-method authorisation: any secret holder can add a stdio extension,
call `tools/call`, upsert config (checked in review).

TLS: a self-signed certificate (SANs `127.0.0.1`, `localhost`) cached under `<config>/tls/`;
`GOOSED_CERT_FINGERPRINT` printed to stdout; the server checks nothing about clients. The
desktop keeps per-hostname trust records; no record fails closed; a blank fingerprint means TOFU
(first certificate wins); trust is by hostname, not port.
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/tls.rs#L17-L30 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L305-L379
(checked in review).

**Pairing (roaming, experimental, feature-gated).** A persisted ed25519 key (0600) is the iroh
endpoint id; a "connection card" carries no secret and never expires; its fingerprint is the
first 16 bytes (128 bits) of SHA-256 of the public key in eight 4-hex groups. Access exists only
if the host's trust book lists the peer's key. The trust book is re-read on every connection
(read failure refuses), revoked keys win over allowed, writes are atomic under `flock`, and a
watcher polls mtime every 2 s and closes live connections of removed keys; `accept` re-checks
after registering to close the time-of-check gap.
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/card.rs#L56-L75 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/trust.rs#L19-L125 ;
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/node.rs#L324-L355
(identity unchecked; revocation checked in review). A peer gets the full ACP surface and can list
and load the host's own sessions (checked in review).

**Trust gate.** Project plugins under `.agents/plugins/` are recorded in `config.yaml` as
`enabled: true` on first sight and their `hooks.json` runs, with no trust prompt:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152
(checked in review). The extension allowlist is a Desktop-only `startsWith` match on deeplink
installs; the CLI, `goose serve`, config edits and `manage_extensions` are not covered:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204
(corrected in review). An OSV `MAL-*` check for `npx`/`uvx` packages fails open:
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L397-L478
(checked in review).

### Cline

**Keys at rest.** SDK provider settings (`providers.json`, API keys and OAuth tokens) are
written atomically 0600 in a 0700 directory and re-chmodded at startup; VS Code secrets are in
`secrets.json` 0600; no keychain; any command the model runs can `cat` the file; MCP OAuth tokens
and the PKCE verifier sit in `cline_mcp_settings.json` with no explicit file mode.
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/storage/provider-settings-manager.ts#L186-L214 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/shared/storage/storage-context.ts#L140-L143 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/config-loader.ts#L302-L320
(checked in review).

**Child environment.** The shell tool spawns with `{...process.env, ...config.env}`; hook runner,
plugin worker and plugin subprocess ("sandbox" mode, crash isolation only) also copy
`process.env`; stdio MCP servers inherit the whole environment plus the entry's `env`. The only
scrub is the hub daemon deleting its own launcher variables.
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L733-L742 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/client.ts#L457-L463 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/runtime/hub-daemon-env.ts#L121-L127
(shell and MCP checked in review; the scrub unchecked). The standalone core captures its host
bridge token from `CLINE_CORE_CONNECTION_TOKEN` into memory and deletes it from `process.env`
before it can launch any child (then logs its entire environment, after the scrub):
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/hosts/external/host-bridge-auth.ts#L4-L47
(unchecked).

**Redaction.** None on tool output or model-bound text; a telemetry sanitiser (bearer tokens,
`/home/<user>`); the marketplace redacts installer output lines matching key, token, secret,
password or authorization:
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/services/telemetry.ts#L426-L437 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/marketplace.ts#L296-L310
(telemetry checked in review; marketplace unchecked).

**Key handed to a remote host.** SSH remote runs local `ssh` with `BatchMode=yes`,
`StrictHostKeyChecking=yes`, `ConnectTimeout=30`, uploads the CLI binary (hash-named, `umask
077`, 0700 directory), runs `hub ensure --host 127.0.0.1` remotely, takes the hub's auth token
from that command's stdout, and tunnels with `ssh -N -L`. For each session start the desktop
attaches the selected provider's API key or access token (refresh token removed) to the session
config, which travels through the tunnel. Where the key is dropped from hub memory after the
session was not found.
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L704-L732 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L1018-L1045 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/examples/desktop-app/sidecar/chat-session.ts#L921-L980
(checked in review; the credential lifetime on the remote is not settled).

**Server authentication (hub).** The daemon binds `127.0.0.1`, mints `randomBytes(32).hex` per
start, writes it to a discovery file (0600), and checks it in constant time from
`Sec-WebSocket-Protocol: cline-hub-auth.<token>` or a Bearer header. It **also** authorises an
upgrade when the bind host is local and the `Origin` header's hostname is `localhost`,
`127.0.0.1` or `::1`. `Origin` is client-supplied: any local process can set it, and any page on
a local port sends it; the caller does not need the 0600 file. Origin-only connections are
refused `providers.*` and `mcp.*` and pinned to the hub's workspace, but `session.create`,
run, send-input and approval commands do not check the flag, and `session.create` takes `cwd`
and `toolPolicies` (including global `autoApprove`) from the payload.
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L153-L166 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L874-L884 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/session-handlers.ts#L497-L560
(checked in review; corrected in review: the Origin path does not need the token file; the
end-to-end exploit was not run). Approvals: `approval.respond` resolves by `approvalId` with no
check that the responder belongs to the session; `session.messages` returns any session's
transcript to any connection:
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/approval-handlers.ts#L123-L161 ;
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/session-handlers.ts#L1055-L1077
(checked in review). The dashboard (`apps/cline-hub`) refuses a non-loopback host without
`ROOM_SECRET`, checks Host and Origin against an allowlist, and compares `?roomSecret=` with
`===`; the local default has no secret:
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cline-hub/src/server/browser-auth.ts#L109-L122
(checked in review). The standalone core serves gRPC on `127.0.0.1:26040` with insecure
credentials and no authentication of its own (unchecked).

**Trust gate.** Workspace `.cline/plugins`, `.cline/hooks`, skills and agents load with no trust
prompt; the vendor-neutral "Agent Plugins" lane scans only the home directory so "opening a
repository cannot activate repository-controlled MCP servers":
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/storage/paths.ts#L495-L509
(inferred from search; checked in review). The enterprise MCP allow-list compares the server's
local name, which the user chooses, and applies only when non-empty:
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/services/mcp/McpHub.ts#L396-L439
(corrected in review). Remote config fails open everywhere; `yoloModeAllowed` is read by no code
(checked in review).

### OpenHands

Only the Agent Canvas client and launchers are in this repository; the Agent Server's own
authentication, secret masking and `sanitized_env()` are not.

**Keys at rest.** The launcher generates 32-byte keys, persisted at
`~/.openhands/agent-canvas/api-key.txt` and `secret-key.txt` (0600; in-memory fallback if the
write fails). In the Docker all-in-one image, settings, encrypted secrets, the automation DB and
both key files are on volumes owned by the same user the agent runs as, so the encryption key
sits next to what it encrypts.
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L148-L180 ;
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L301-L341
(key generation unchecked; all-in-one checked in review). Provider keys reach the browser only as
ciphertext the client replays; a `plaintext` mode exists on the server for any key holder:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/settings-service/settings-service.api.ts#L576-L593
(unchecked). Every backend's API key is stored in `localStorage` as plain JSON:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/backend-registry/storage.ts#L95-L102
(checked in review).

**The control-plane key handed to the agent.** The launcher sets `OPENHANDS_AUTOMATION_API_KEY =
sessionApiKey` in the agent-server's environment "to terminal commands spawned by the
agent-server", seeds it into the secret store, and the system prompt tells the agent to send
`X-Session-API-Key: $OPENHANDS_AUTOMATION_API_KEY`. The agent-server and automation backend accept
the same key. With it, a prompt-injected agent can answer its own pending confirmation, run
server-level bash, change settings, and (inferred) request plaintext provider keys.
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985 ;
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/runtime-services-info.mjs#L128-L141 ;
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44
(checked in review). Secrets are attached to every start request as `LookupSecret` URLs that embed
the session key in their headers:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1114-L1144
(checked in review).

**Server authentication.** One shared key (`X-Session-API-Key`), no identity; the ingress does no
authentication:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L185-L226
(checked in review). On loopback the static server injects the key into `index.html`
(`window.__AGENT_CANVAS_SESSION_API_KEY__`) and `localStorage`; the page is unauthenticated, so
any local process can `GET /` and read the key; on a non-loopback bind it is stripped unless
`--allow-lan-session-key`:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L389-L407 ;
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/bind-host.mjs#L50-L77
(checked in review). Docker mode binds `0.0.0.0` by default:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543
(checked in review). WebSocket auth is a first message `{"type":"auth","session_api_key":...}`,
never the URL; whether the server closes unauthenticated sockets is server code:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18
(checked in review). The bundled VS Code editor's query-string token is derived from the same
master key and shares the Canvas origin (and its `localStorage`); the ingress sends
`Referrer-Policy: no-referrer` for that prefix:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L196-L201
(unchecked). Canvas Apps are `import()`ed same-origin and can read every backend key (checked in
review).

**Cloud token behind a bridge (MARS, opt-in with `MARS_WEB=1`).** The Node side holds the
DigitalOcean token in the OS keychain, an AES-256-GCM file keyed by HKDF of `OH_SECRET_KEY`, or
memory, and reverse-proxies `/mars/sessions/<id>/...`; `/mars/*` needs the session key
(timing-safe) or an `HttpOnly; SameSite=Strict` cookie derived from it, refuses cross-origin
requests and, on loopback, foreign `Host` headers. With no key on loopback it mounts open to local
callers, which includes the agent's own shell in host mode.
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L260-L284 ;
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-secret-store.mjs#L351-L387
(corrected in review: opt-in only).

**Trust gate.** Repository `.openhands/hooks.json` is loaded and attached to every conversation
with no prompt; a load failure or 5 s load timeout starts the conversation without hooks; plugin
launches have a one-shot "I trust this plugin ... with the agent secrets" checkbox not bound to a
version:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L12-L42 ;
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/components/features/launch/plugin-launch-modal.tsx#L398-L420
(corrected in review; plugin checkbox checked in review).

### Zed

Only egress and ACP spawn bear on this group in the files read; Zed's provider-key storage and
collaboration-server authentication were not studied.

**Egress proxy (no credential substitution).** For restricted network the sandbox crate starts
an in-process HTTP(S) proxy on a Unix socket, reserves a loopback port, sets `HTTPS_PROXY`,
`HTTP_PROXY`, `ALL_PROXY` (and lower case) and blanks `NO_PROXY`; it matches hostnames against
exact or `*.` patterns, denies IP literals, resolves the name itself and connects to the vetted
addresses, caps 256 concurrent connections. Tools that ignore proxy variables fail because the
namespace or Seatbelt rule blocks their sockets.
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L1179-L1198 ;
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/http_proxy/src/proxy.rs#L404-L412
(unchecked). The sandbox does not scrub the environment; the rest of the project environment is
inherited. `fetch` runs in the editor process with a forbidden-IP gate that is "a gate, not a full
pin":
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/fetch_tool.rs#L308-L318
(checked in review).

**ACP agents.** Spawned through the system shell with `envs(environment)` added to the inherited
environment, so the agent child sees whatever the editor has:
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/transport.rs#L31-L110
(unchecked). Registry archives have an optional `sha256`; npx agents take the registry version as
a ceiling, not a pin:
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/project/src/agent_server_store.rs#L1131-L1340
(unchecked).

### Warp

The model call runs on Warp's server; this repository is the client.

**Keys at rest and in transit.** BYO provider keys live in the platform secure storage under
`AiApiKeys`; each request copies them into `Request.settings.api_keys`, so every model call goes
through Warp's server with the user's key. Custom endpoints must be HTTPS and not localhost or
private IPs. Team policy can disable member BYO keys.
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/ai/src/api_keys.rs#L30-L34 ;
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L59-L63 ;
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/ai/src/api_keys.rs#L371-L410
(unchecked). Managed secrets are sealed client-side with a Tink HPKE public key, with associated
data `{actor_uid, secret_name, secret_type}`, fetched by a run with an OIDC task identity;
workload tokens last 3 hours:
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/managed_secrets/src/envelope.rs#L33-L80 ;
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_server_client/src/base_client.rs#L47-L52
(unchecked).

**Child environment.** MCP stdio servers are spawned with only the configured env added (no
`env_clear`); OAuth credentials are in secure storage; the MCP log header warns that logs can
contain secrets:
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/mcp/src/runtime.rs#L235-L256
(checked in review).

**Redaction.** A client regex pass over outbound inputs (query, context, attachments, command
output, file contents, search results, diffs, review comments), off by default
(`safe_mode_enabled` false; the set is empty until safe mode or enterprise redaction fills it).
Relay inputs from other agents and codebase indexing are not redacted.
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/terminal/safe_mode_settings.rs#L74-L82 ;
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/redaction.rs#L90-L94 ;
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/secret_redaction/src/lib.rs#L296-L360
(checked in review).

**Trust gate.** Project-scoped MCP servers never auto-spawn and need an explicit opt-in; edits to
MCP config files need approval even in auto mode:
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/mcp/file_based_manager.rs#L396-L416 ;
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L1243-L1257
(unchecked). Worker authentication is in the uncloned `oz-agent-worker`.

### OpenCodex

OpenCodex is a model-call gateway first; it has no agent loop of its own except three bounded
in-request loops and the Remote Workspace hub.

**Keys at rest and in memory.** `apiKey` is a literal (plaintext in `config.json`), a `$VAR`
reference, or `keychain:<provider>[/<pool id>]`. Moving to the keychain is an API action that
probes the keychain (write, read back, delete), writes and verifies each secret, and only then
rewrites `config.json`; failure deletes what it wrote. Reads fail closed: "no plaintext fallback".
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/providers/api-key-resolve.ts#L106-L125 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/providers/key-store.ts#L75-L121
(checked in review). OAuth tokens (`auth.json`) and the Codex account pool are plaintext JSON
written with `wx` 0600 temp file, `fchmod`, rename, in a 0700 directory; refresh is serialised by
per-account locks and an intent file:
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/config/atomic-write.ts#L238-L262
(checked in review). In memory, `routedProviderConfig` copies the provider row with the resolved
key for that request; keychain values are cached in a module-level `Map` for the process
lifetime; caller credentials are forwarded only to the canonical Codex destination through a
header allow-list:
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/providers/routed-config.ts#L103-L113 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/codex/forward-transport-headers.ts#L12-L19
(unchecked).

**Log redaction by value.** Before an upstream diagnostic is kept, the request log checks it
against a set of every configured credential (provider keys and pool keys with references
resolved, credential-named fields, configured header values, OAuth tokens), rebuilt when the
content digest of `config.json` or `auth.json` changes; if any store cannot be read the set is
marked incomplete and the check fails closed (retried after 30 s).
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/configured-credentials.ts#L1-L30
(unchecked).

**The model-call proxy data path.** Step by step:

1. Three inbound wires (`/v1/responses`, `/v1/messages`, `/v1/chat/completions`). Each checks
   draining, resolves the caller's credential, checks `Origin` (cross-origin 403 even with a valid
   key), then `runAdmittedHttpTurn`. Responses and Chat refuse `x-api-key`; Messages accepts it.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L652-L691
   (checked in review).
2. Body admission: default cap 256 MiB decompressed (opt-in up to 512 MiB); only bodies above
   256 MiB take a reservation from a global FIFO byte budget (default 512 MiB, max 2 GiB); the
   queue holds at most 32 waiters for 120 s, then 503 `server_busy` with `Retry-After: 1`; the
   lease shrinks to the measured size and is held until the response finishes. A turn gate allows
   256 concurrent turns. The comment says the body is materialised several times, so "these
   values do not cap RSS".
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/inbound-body-budget.ts#L63-L69 ;
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/inbound-body-admission.ts#L143-L193 ;
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admitted-http-turn.ts#L26-L30
   (checked in review).
3. Translation to one internal Responses request, charged to a per-request `TranslatorBudget`
   (3x body bytes during the copy); over budget gives 413 `translation_buffer_limit`:
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/claude-messages.ts#L1240-L1262
   (unchecked).
4. Routing, then scope: a key's `allowedProviders`, `allowedModels` and
   `allowMeteredComboTargets` are checked against the resolved provider and model, after alias,
   combo pick, fallback and compaction rewrites; the environment token and loopback admission
   carry no scope.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L8-L29
   (checked in review).
5. Send budget: transient 5xx retry capped at 3 attempts per logical request including recovery
   legs; a combo child inherits the parent's spend; every send is also charged to a workflow-root
   counter keyed by `x-codex-parent-thread-id` so fan-out cannot multiply sends.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/responses/request-send-budget.ts#L36-L48
   (unchecked).
6. Streaming back: `bridgeToResponsesSSE` is pull-driven with a default queue of one; a slow
   client stops the upstream read; disconnect cancels the upstream fetch. Limits: 2 MiB per
   tool-call argument, 32 MiB per turn, 32 MiB per SSE event, 4 MiB per SSE frame. Other paths do
   buffer (combo preflight up to 4 MiB, the web-search loop, the Cursor adapter up to 32 MiB).
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/bridge/sse.ts#L1670-L1686 ;
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/lib/translator-budget.ts#L3-L6
   (corrected in review: the "buffer of one" holds for the generic SSE bridge only).
7. Stall watchdog and heartbeat share a 2 s timer; 300 s without upstream activity ends the turn
   with `upstream_stall_timeout` on public upstreams, disabled by default for loopback, private,
   `.local` and `.lan` upstreams; connect deadline 200 s on public upstreams; the heartbeat is a
   typed `response.heartbeat` event:
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/stall-timeout.ts#L29-L36
   (unchecked).
8. Fail-over: a combo child's stream is read into a replay buffer until output commits, a
   terminal event arrives, or 4 MiB / 4,096 chunks; any unknown event, unparseable payload or tool
   call commits the target; only a failed terminal before commit advances the combo. Hard stops,
   target-local hops and cooldown scope are separate decisions; cooldowns are persisted with
   absolute expiries (60 s default, 10 min for spent windows and provider-scoped failures, 5 s for
   transient rate limits; `Retry-After` capped at a day).
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/responses/combo-stream-preflight.ts#L35-L70 ;
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/failover.ts#L1177-L1230 ;
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/cooldown-store.ts#L1-L25
   (preflight unchecked; failover unchecked; cooldowns checked in review). Metered targets need
   consent per key (`allowMeteredComboTargets`), never for the env token:
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/responses/core-combo.ts#L663-L678
   (unchecked).

**Keys kept off the worker (Remote Link).** The Rust `ocx-child` relay on the Child machine strips
caller credential headers (`authorization`, `x-api-key`, `x-goog-api-key`,
`x-opencodex-api-key`, `chatgpt-account-id`, `cookie`, ...) and writes the link token into the
request head (one allocation, wiped on drop). Before each upstream connection it sends a
challenge with a caller HMAC proof keyed by the SHA-256 fingerprint of the link token, and
requires a 204 whose listener proof matches, so a local impostor on the tunnel port is refused.
The Home holds provider keys; the Child holds only a link key. Limits: 256 MiB body, 64 KiB
headers, 32 workers, 256 control connections, 300 s timeout.
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/http_wire.rs#L3-L16 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/relay.rs#L225-L246 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/relay.rs#L321-L343 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/link/relay-auth.ts#L52-L67
(checked in review; Home listener not traced end to end).

**Server authentication.** `isApiAuthRequired` is "hostname is not loopback"; on loopback no
token is read at all. A non-loopback bind without a token fails at startup. Keys are compared
with `timingSafeEqual` against `config.apiKeys[].key`, stored as literal text; a `pendingRotation`
stays valid 10 minutes; no hashing at rest, no expiry.
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L337-L339 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L455-L482
(checked in review). A separate unauthenticated loopback listener serves only inference routes,
with a startup warning that "any local process can use it ... spends account quota and paid
provider credentials":
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802
(unchecked). The management credential (`OPENCODEX_ADMIN_AUTH_TOKEN` or a generated file) must
not equal any data-plane secret or startup fails; GUI sessions carry a 32-byte CSRF token, are
bound to server and browser origins, and last 5 minutes on loopback, 12 hours remote; if the
token file cannot be hardened the management API answers 503 rather than use it:
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/management-auth.ts#L222-L252 ;
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/gui-session.ts#L135-L156
(unchecked).

**Pairing and worker credentials (Remote Workspace, experimental).**

1. The dashboard asks the hub for a grant: 12 characters from a 32-letter alphabet (about 60
   bits), 10 minutes, at most 16 outstanding; the hub stores only SHA-256 of the normalised code.
2. The executor reads the code from stdin and posts name, platform, Ed25519 public key, requested
   capabilities and root ids (no paths). The hub refuses requests with an `Origin` header,
   rate-limits by kernel-observed peer (10 failures in 10 minutes, then 429, 1,024 sources),
   compares with `timingSafeEqual`, **deletes the grant before validating the metadata**, and
   returns `ocxrw_<43 base64url>` plus its Ed25519 public key. It stores `SHA-256(token)`.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425 ;
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L484-L513
   (checked in review). The hub's own Ed25519 private key is in `remote-workspace-hub.json` in the
   clear (0600).
3. The executor dials `wss://hub/remote-workspace/agent` with the bearer; the hub refuses
   `Origin`, finds the device by `timingSafeEqual` over stored hashes, closes any previous
   connection ("latest connection wins"), and refuses a presence that claims a capability beyond
   the stored ones. The stored capabilities are what the executor declared at pairing, not an
   operator decision. The bearer has no expiry and is not bound to a connection. `revokeDevice`
   removes durable state first, then closes.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L427-L463
   (corrected in review).
4. Per session, a signed ephemeral ECDH (P-256, HKDF-SHA256, AES-256-GCM, strict counter) against
   the pinned hub key; the hub terminates the cipher, so it protects against a TLS-terminating
   proxy, not against the hub (unchecked).

**Trust gate.** Not applicable in the same sense; for the Remote Workspace agent the hub disables
the user's own MCP servers and hooks from the effective Codex config and sends every turn with
`approvalPolicy: "never"` against an auto-approved remote tool bridge:
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-codex-runtime.ts#L392-L414
(checked in review).

## 3. Comparison

### 3.1 Keys, environment, redaction

| Project | Keys at rest | Keys reachable from the shell | Child environment (shell / MCP) | Redaction |
|---|---|---|---|---|
| Codex | MCP OAuth in keyring, else a user-readable file | Yes: `OPENAI_API_KEY` inherited by default; with the broker, a dummy | Inherit all; `*KEY*` excludes off by default; fixed always-strip list for launch secrets; Git vars scrubbed for plugin Git | Four regexes, not on tool output |
| OpenCode | SQLite JSON, 0600 | Yes: env, and the DB file; server password in env outside stdio mode | Whole `process.env`; workspace drivers start empty (none ship) | None |
| OpenClaw | Auth profiles in SQLite; config/`.env` readable | Host exec: yes for provider keys in env; sandbox: no; GitHub token on workers: yes | Inherit minus ~110 injection keys and registered credential names; sandbox: declared only; SSH: `*_API_KEY` etc. blocked | Worker credentials and GitHub tokens registered for log redaction |
| Hermes Agent | `auth.json` pools; per-profile homes | Terminal: unlisted secrets yes; managed keys no; Docker with iron-proxy: tokens only | Terminal blocklist; `execute_code` and MCP allow-list | On by default; pattern plus env-dump pass; sentinel on file reads |
| Oh My Pi | `agent.db` SQLite, 0600, unencrypted | Yes: env and `agent.db` | Whole env minus project `.env` values and Git location vars | Opt-in obfuscation to the provider, reversed before tools run |
| Pi | `auth.json` 0600 on creation | Yes: env and `auth.json`; `--api-key` in argv | Whole env; MCP inherits | None in core |
| DeepSeek harness | `.credentials.yaml` 0600, refuses loose mode; cwd `.env` accepted | Env scrubbed by name; store file readable by confined bash | Name-pattern scrub (`KEY|PASSWORD|SECRET|TOKEN`, `DSH_*`), MCP same | Resolved MCP credential values only |
| goose | Env first, keyring blob, silent plaintext fallback | Yes: env-supplied keys | Whole env; extension `envs` filtered by 31-key disallow list | None found |
| Cline | `providers.json`/`secrets.json` 0600 | Yes: env and file | Whole env for shell, hooks, plugins, MCP | Telemetry and marketplace output only |
| OpenHands (client) | Key files 0600; Docker: key beside ciphertext | The master session key is put in the agent's env on purpose | Server side, not in repo | Server side, not in repo |
| Zed | Not studied | Not studied | Inherited (terminal and ACP agents) | Not studied |
| Warp | OS keychain, then sent to Warp per request | Not shown (client) | MCP inherits whole env | Client regex, off by default |
| OpenCodex | Plaintext `config.json` unless keychain; OAuth plaintext 0600 | Gateway: n/a (no agent shell); Remote Link child: link key only | Remote Workspace `exec`: bwrap `--clearenv` plus `HOME`, `PATH`, `LANG`, `LC_ALL` | Logs refuse any configured credential value; fail closed |

### 3.2 Brokers, proxies, workers, servers, trust

| Project | Broker or proxy, and coverage | Worker credentials | Server authentication model | Trust gate |
|---|---|---|---|---|
| Codex | Network-proxy credential broker (off by default, MITM), dummy env values, host-bound; netns bridge makes it the only path on Linux | Exec-server: registry auth via stdin, Noise IK with pinned key; caller's sandbox applies | WS: capability token or HS256 JWT at upgrade, `Origin` 403; loopback unauthenticated; non-loopback refused for app-server only; Unix socket by file mode | Project trust for config, hooks, exec policy; hook hash in user layer; skills ungated |
| OpenCode | None | None (SSH tunnel to a per-machine server) | One password (Basic, cookie or `?auth_token=`), `===` compare; pairing token HMAC of the password | None |
| OpenClaw | Secret egress proxy with sentinels and allowed hosts (cooperative); inference brokered by model ref | 32 bytes, domain-separated hash bound to the turn claim, 10 min TTL, per-frame revalidation, owner-epoch fencing | Challenge nonce, Ed25519 device proof over token and scopes; startup token; device tokens plaintext | Not read this round |
| Hermes Agent | iron-proxy token swap with `require: true`; Docker only; own model calls bypass | Relay HMAC token, 300 s | Same-user control socket, single-use 30 s tickets in a subprotocol; non-loopback dashboard needs auth; API server one key | Regex scanner at install; context files blocked on hits |
| Oh My Pi | Auth broker hands out keys; auth gateway proxies with digest-stored scoped clients (no expiry); wildcard CORS | None | Static bearers, `Set.has`; `--no-auth` | Hard-coded open |
| Pi | None (documented third-party only) | pi-env: none beyond SSH | stdio only; `apps/p2` one-use token, Host and Fetch-Metadata fence | Per-directory boolean, no content binding, context files ungated |
| DeepSeek harness | None | None | Launch token in URL, then 30-day HMAC cookie; Host, `Sec-Fetch-Site`, Origin fence; wildcard bind refused | `approvedBuilds` set by the model |
| goose | None | Roaming: key in trust book, re-read per connection, revocation closes live connections | Shared secret, header or `?token=`, constant time; `--dangerously-unauthenticated` | Project plugins auto-enabled |
| Cline | None | Provider key sent per session to a remote hub | Per-start token **or** loopback `Origin`; approvals answerable by anyone | None (home-only lane excepted) |
| OpenHands (client) | MARS bridge keeps a cloud token server-side | n/a in repo | One shared key, injected into HTML on loopback, kept in `localStorage`; first-frame WS auth | Hooks ungated; plugin checkbox |
| Zed | Egress allow-list proxy, no credential substitution | n/a | Not studied | ACP registry checksum optional |
| Warp | Model calls through Warp's server with the user's key | In uncloned repo | Server side | Project MCP never auto-starts |
| OpenCodex | Full model-call gateway with admission, budgets, scopes; Remote Link relay strips caller credentials | Pair code 60 bits, hash stored, deleted before validation; bearer hashed, no expiry | None on loopback; mandatory key off loopback; admin token distinct from data keys | n/a |

## 4. Patterns and trade-offs

**4.1 Where the key lives relative to the agent.** Four positions exist.

- *Key in the agent's process and environment* (OpenCode, Oh My Pi, Pi, goose, Cline, Codex by
  default, OpenClaw host exec). Cheapest; every shell command can read it. Every one of these
  also keeps the key in a 0600 file the same user's agent can open (OpenCode, Oh My Pi, Pi,
  Cline, DeepSeek), so scrubbing the environment alone does not hide it (DeepSeek finding 21).
- *Key in the harness process, environment scrubbed* (Hermes terminal and `execute_code`,
  DeepSeek, OpenClaw sandbox and SSH). Blocklists miss names they do not know (Hermes: "any user
  secret not on the list ... reaches local terminal children"; DeepSeek: `DATABASE_URL`,
  `GITHUB_PAT`). Allow-lists (Hermes `execute_code` and MCP, OpenClaw sandbox, OpenCodex bwrap
  `--clearenv`) do not have that failure.
- *Key held by a broker on the same machine, dummy or token in the child* (Codex credential
  broker, OpenClaw secret egress proxy, Hermes iron-proxy). Costs a MITM CA inside the sandbox
  (Codex, Hermes, OpenClaw all install one). Coverage is the weak point: only Codex pairs it with
  a network namespace that makes the proxy the only path; OpenClaw's and Hermes' are cooperative
  (a process that ignores `HTTPS_PROXY` goes direct), Hermes covers Docker only and its own model
  calls bypass it, and SigV4 or service-account credentials cannot be substituted (Hermes lists
  them as uncovered).
- *Key on the control side, worker gets none* (OpenClaw inference by model reference, Oh My Pi
  auth gateway, OpenCodex Home/Child relay, Warp server, OpenHands MARS bridge). The only
  position where a compromised worker cannot spend a key except through the control side's
  rules. Costs a data path on the control side that must be bounded (OpenCodex shows how much
  code that is), and a scope check that must run after routing (OpenCodex 14) and refuse
  worker-chosen profiles (OpenClaw refuses `@profile`).

A fifth, distinct case: *key sent per session to a worker that keeps it in memory* (Cline SSH
remote; OpenClaw GitHub token). Cline drops the refresh token but its lifetime on the remote was
not found; OpenClaw writes the GitHub token to a per-turn profile directory, readable by the agent.

**4.2 Control-plane credentials inside the agent's reach.** OpenHands puts the master session key
in the agent's environment on purpose and tells the model to use it; OpenCode leaves the server
password in the environment outside stdio mode; DeepSeek keeps the cookie-signing secret in a file
confined bash can read; Oh My Pi's broker token file is unprotected. In every case the credential
can answer the agent's own approvals or reach settings. The counter-examples are OpenCode's stdio
mode and Cline's core, which capture the spawn secret into memory and delete it from the
environment before any child starts, and Codex's always-strip list.

**4.3 Redaction.** Approaches: none (OpenCode, Pi, goose, Cline on model-bound text); regex
patterns (Codex, Warp, Hermes, Cline marketplace); by known value (OpenCodex logs, DeepSeek MCP
credential set, OpenClaw registering minted credentials and GitHub tokens); reversible placeholders
(Oh My Pi). Regex misses unprefixed values; value-based needs the full set and should fail closed
when it cannot enumerate it (OpenCodex). Reversible placeholders hide secrets from the provider
but deobfuscate into tool arguments, so they are not a leak control. Ordering matters: Hermes
redacts after an output plugin hook, which sees raw output.

**4.4 Worker credentials.** Range from none (pi-env, trusts SSH), to a shared static bearer
(Oh My Pi broker), to a hashed device bearer with no expiry and latest-connection-wins fencing
(OpenCodex), to OpenClaw's per-dispatch, domain-separated, claim-bound hash with a 10-minute TTL,
per-frame revalidation and a global never-reused epoch. Codex checks bearer credentials once at
upgrade and says so; OpenClaw re-checks on every frame. OpenClaw stores device tokens in plaintext
even though it hashes worker credentials.

**4.5 Local server authentication.** Approaches, weakest first:

- No auth on loopback (Codex app-server and exec-server defaults, OpenCodex data plane, Oh My Pi
  `--no-auth`, goose `--dangerously-unauthenticated`, OpenHands MARS without a key, Pi and Oh My
  Pi RPC on stdio by construction). Any local process, including the agent's own shell with network
  on, can drive it.
- `Origin` as authentication (Cline hub). `Origin` is a client-supplied header.
- One shared secret with no identity (OpenCode, goose, Hermes API server, OpenHands, OpenCodex data
  keys), sometimes in a URL (`?auth_token=`, `?token=`, `?roomSecret=`, `#token=`, VS Code token).
- Launch token then a signed cookie (DeepSeek, Pi `apps/p2`); revocation only by rotating the
  signing secret.
- Short single-use tickets bound to purpose and capability over a same-user socket (Hermes).
- Device keys signing a challenge plus the token and scopes (OpenClaw).

Refusing a wider bind without auth is common (Codex app-server, Hermes dashboard with no override
flag, OpenCodex, DeepSeek refusing wildcard binds, Cline dashboard); the Codex exec-server and
goose show the gap when one listener of several is left out.

**4.6 Pairing codes.** OpenCode: 128-bit code, 5 minutes, atomic consume, but the resulting token
is derived from the server password and cannot be revoked alone. OpenCodex: 60-bit code, 10
minutes, hash stored, rate-limited by source, `Origin` refused, grant deleted before metadata
validation, device bearer hashed. OpenClaw: setup tokens plaintext with 10-minute TTL, plus
SSH-verified auto-approval that rests on network position and a shell profile (inferred). goose:
no code, a 128-bit fingerprint for out-of-band comparison and a trust book.

**4.7 Trust gates.** Hard-coded open (Oh My Pi), absent (OpenCode, goose, Cline, OpenHands
hooks), path boolean without content binding (Pi), per-layer with hashes the repository cannot
write (Codex, but script bytes and skills uncovered), "project servers never auto-start" (Warp).
None binds trust to the content of everything that will execute.

## 5. Documentation versus code

| Project | What the documentation said (Round 1) | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Non-loopback WebSocket listeners allow unauthenticated connections during rollout | App-server refuses to start; flag removed. Exec-server `--listen` still allows it | refuted (app-server), corrected in review for exec-server | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 |
| Codex | `ignore_default_excludes = true` | Confirmed; `OPENAI_API_KEY` reaches commands | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/shell_environment_policy.rs#L133-L136 |
| Codex | Placeholder credential substitution (Codex Cloud only) | A local credential broker exists, off by default | new | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/config.rs#L185-L228 |
| Codex | DNS check is "best effort" | Dialed address re-checked; gap only with an upstream proxy | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/connect_policy.rs#L199-L223 |
| Codex | A changed hook needs review again | True for declarations and plugin bundles, not for scripts a repo hook points at | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L836-L882 |
| OpenCode | Password protected by default | One shared secret, no accounts, also accepted as `?auth_token=` | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L13-L40 |
| OpenCode | Pairing: single-use, 5 minutes, 30-day cookie | Confirmed; token derived from the password, no individual revocation | confirmed, with new detail | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L39-L55 |
| OpenCode | `--remote` is end-to-end encrypted | Encryption lives in an external package | not settled | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/remote-tunnel.ts#L7-L47 |
| OpenClaw | GitHub publication via a Gateway broker (no GitHub credential on the worker) | A turn-scoped GitHub token reaches the worker and its shell | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 |
| OpenClaw | Workers are outbound-only | Agent link is outbound; the Gateway SSHes in for bootstrap and sync | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| OpenClaw | Control UI token kept in session storage per tab | True for the shared token; device key and device tokens in `localStorage` | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L239-L251 |
| OpenClaw | Per-dispatch credential stored as a hash; inference proxied | Confirmed; hash also binds the turn claim | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60 |
| OpenClaw | Egress proxy with sentinels | Confirmed; cooperative, not enforced by a namespace (inferred) | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/stream-substitution.ts#L26-L31 |
| OpenClaw | Billing failures disable a credential 10 minutes | 10 minutes doubling to 24 hours | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L18-L46 |
| Hermes Agent | iron-proxy is for remote and Docker sandboxes (docstring: Docker/Modal/SSH) | Wired into Docker only; own model calls bypass it | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L786-L812 |
| Hermes Agent | iron-proxy binary is pinned | A binary on PATH wins over the pinned one | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L162-L177 |
| Hermes Agent | Non-loopback dashboard fails closed without auth; single-use 30 s tickets | Confirmed; `--insecure` is ignored | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662 |
| Hermes Agent | Stdio MCP servers get a safe env baseline | Confirmed | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 |
| Oh My Pi | Auth broker is loopback-only | Loopback default; `--bind 0.0.0.0` accepted without warning | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/types.ts#L177-L184 |
| Oh My Pi | Gateway has its own inbound bearer | Also scoped clients by digest, model and account allow-lists | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L60 |
| Oh My Pi | Hooks live in `.omp/hooks/` or the user dir | Also imported from the project `.claude/hooks/` | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 |
| Oh My Pi | No trust prompt documented | `isProjectTrusted()` returns `true` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937 |
| Pi | Trust order: CLI, extensions, saved, default `ask`; context files load regardless | Confirmed; no content binding; cwd-only `.pi` check | confirmed, new detail | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97 |
| Pi | pi-env: credentials stay local | True because only the tools move and the client env is not forwarded | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L55-L58 |
| DeepSeek harness | Launch token, signed cookie, Host/Origin fence | Confirmed; 30-day persistent secret, no revocation; wildcard bind refused | confirmed, new detail | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L182-L199 |
| DeepSeek harness | Credential-style names scrubbed from children | Confirmed; defeated by unrestricted reads of the store | confirmed, with gap | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subprocess/subprocess/src/index.ts#L40-L75 |
| goose | Secret in `X-Secret-Key` | Also the `token` query parameter | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L9-L36 |
| goose | Fingerprint printed, otherwise trust on first use | TOFU only in the desktop client; server checks nothing | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L305-L379 |
| goose | Extension allowlist command must match exactly | Prefix match, Desktop deeplinks only | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204 |
| goose | Keyring preferred, plaintext fallback, env precedence | Confirmed; one blob; silent fallback sets a process-wide flag | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1262-L1281 |
| goose | `GOOSE_TERMINAL=1`, `AGENT=goose` set for commands | Only for recipe retry checks | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/retry.rs#L51-L64 |
| Cline | No hub authentication documented | Per-start token plus a loopback-Origin shortcut | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | Provider key sent over the tunnel per session, not written to host settings | Sent in `session.create`; on-disk persistence not traced | confirmed in part | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/examples/desktop-app/sidecar/chat-session.ts#L921-L980 |
| Cline | Keys plaintext under `~/.cline/data/settings/`; encryption not documented | Not encrypted | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/storage/provider-settings-manager.ts#L186-L214 |
| Cline | `CLINE_PLUGIN_MODE=sandbox` restricts plugins | Crash isolation only | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/worker-sandbox.ts#L267-L276 |
| Cline | Remote config `yoloModeAllowed` | Read by no code | confirmed (unenforced) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/remote-config/schema.ts#L199-L201 |
| OpenHands | Browser embeds the key only with explicit Docker opt-in | Embedded by default on loopback | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L389-L407 |
| OpenHands | Docker whole-stack isolates to mounted dirs | Inside it, agent and keys share one uid | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206 |
| OpenHands | Whether `X-Session-API-Key` is checked on WebSocket upgrades | The client sends it as a first frame instead | corrected (server side not in repo) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18 |
| Warp | Redaction is off by default, regex-based | Confirmed; coverage list new | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/terminal/safe_mode_settings.rs#L74-L82 |
| Warp | Do local model calls go through Warp? | Yes, always, with BYO keys | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L59-L63 |
| OpenCodex | Keys are env references or pasted; OAuth in separate stores, not `config.json` | Pasted keys land in `config.json` as plaintext unless keychain | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/providers/api-key-resolve.ts#L26-L29 |
| OpenCodex | One-time pairing code, hash of bearer, 429 after ten failures | Confirmed; lifetime 10 minutes | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425 |
| OpenCodex | Read-only default; write needs a command-sandbox probe | The probe gates `exec`; writes are offered without it | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | 4 MiB WebSocket relay frame cap | 4 MiB is the SSE frame cap; live-sideband WS frame cap is 50 MiB | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/sse-frame-buffer.ts#L4-L4 |
| OpenCodex | Cooldown memory-only | Combo cooldowns are persisted | refuted | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/cooldown-store.ts#L1-L25 |

Our own design note `docs/nodes-credentials-environments.md` §7 cited documentation-level
patterns. The code changes two of them: "Key sent per session, never written" (Cline) is only
confirmed in part, and no lifetime on the remote was found; "Inference proxied by the control
side" is confirmed for OpenClaw, Oh My Pi's gateway and OpenCodex, and OpenClaw also keeps a
worker-local mode.

## 6. For rizzy-harness

Our position, for reference: on `main` the node starts children from an empty environment plus an
allow-list and `env_pass` globs, always stripping provider key vars, `.env` names and `HARNESS_*`
(`crates/tools/src/env.rs`); known secret values are scrubbed from tool output, model text and
events (a courtesy, `crates/node/src/session.rs:194-215`); the master has accounts, cookie
sessions stored as SHA-256 with `HttpOnly; SameSite=Strict`, a Host allow-list and Origin check,
and an admin-equivalent API bearer; the node dials out over TLS to a pinned certificate and signs
a nonce; each node holds its own provider key in `config.toml` (`research/round1/own-rizzy-harness.md`).
Pull request 4 adds the Landlock shell sandbox, the empty allow-listed child environment and
redaction across chunk boundaries. The design note moves keys to sealed per-workspace storage on
the master, hands them to workspace nodes per turn (`session.grant`), reserves `ModelAccess::Proxy`
for per-user nodes, and replaces join tokens with a pair code.

Ranked by importance.

**1. The agent must not be able to reach any control-plane credential. Test it.** The worst
failures in this group are not provider keys; they are the harness's own credentials in the
agent's reach: OpenHands puts the master session key in the agent's environment and prompt;
OpenCode leaves the server password in the environment outside stdio mode; DeepSeek's
cookie-signing secret is readable by confined bash, so the agent can forge a login. On a node
that means three things, each with an abuse test written first:

- The node's link private key (`node.key` or `$CREDENTIALS_DIRECTORY/node.key`), its data dir and
  `node.db` are unreadable under the Landlock profile from pull request 4. Test: a confined `cat`
  of each path fails. DeepSeek is the counter-example (finding 21).
- Grant keys live only in the node process's memory and never in a child's environment, argv or
  a file. Test: start a turn with a grant, run `env` and `cat /proc/self/environ` and
  `cat /proc/$PPID/environ` in the shell, assert the key bytes are absent.
- The node process is non-dumpable, so a same-uid child cannot read its memory through
  `/proc/<pid>/mem` or ptrace. Codex has `pre_main_hardening` but does not apply it to its main
  binary (absence confirmed in review):
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/process-hardening/src/lib.rs#L7-L61.
  We forbid `unsafe`, so use a safe `prctl(PR_SET_DUMPABLE, 0)` wrapper (for example `rustix`,
  which needs the dependency check in CLAUDE.md). Test: the shell's `cat /proc/$PPID/mem` or a
  `gdb -p` attach fails. Note this is not enough without the sandbox; whether Landlock alone stops
  ptrace was not in the code read (section 7).

On the master side, the same rule applies to the local node and the master's chat runtime, which
run in the master process (design note §4.3): the master's `api-token`, `secrets.toml`,
`keys/secrets.key` and `master.db` must be unreadable from the embedded node's shell. That embedded
node is the case OpenHands' host mode shows going wrong.

**2. Shape the per-turn grant like OpenClaw's worker credential, not like a long-lived key
drop.** Read `src/gateway/worker-environments/credential.ts`, `admission.ts`,
`inference-model.ts` and `store-transitions.ts` in OpenClaw. Concretely:

- *Lifetime.* The design note's `expires_ms` default of 6 hours renewed while a turn runs is
  longer than it needs to be. OpenClaw's credential lives 10 minutes and is renewed while the
  owner is current, with a timer at expiry that re-runs validation; Hermes' tickets live 30 s and
  are single-use. A grant TTL does not stop the node from copying a provider key (the design note
  says so, and only rotation fixes that), but it bounds what an attacker who later steals the
  node's link key receives, and what a crashed turn leaves in memory. Recommendation: a short TTL
  (minutes), renewed by the master on the link while the turn is live, dropped on `grant.drop`,
  on turn end, on link loss after the running turn ends (A3), and when a newer grant for the same
  session is sent. Minting a new grant should cancel any in-flight model call that used the old
  one, as OpenClaw's mint does
  (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126).
- *Scoping.* Bind the grant to `(node_id, session_id, turn or epoch)`. The node must refuse to use a
  grant for any other session, and the master must refuse to send one to a node whose
  `workspace_id` differs from the session's effective workspace. Scope it to the providers the
  resolver computed, and check that list against the *resolved* models after alias expansion, not
  the names the agent spec carries (OpenCodex finding 14:
  https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L8-L29).
  Never let the node name a key or profile (OpenClaw refuses `@profile`).
- *What to store.* On the node: nothing. On the master: `node_key_grants` with name and times, as
  designed, plus the grant id and its hash only if the grant itself becomes a bearer (it will for
  the proxy, item 3). Sealed `workspace_secrets` should use AES-GCM associated data that binds
  `workspace_id` and `name`, so a row copied from one workspace or name to another fails to open.
  Warp binds `{actor_uid, secret_name, secret_type}` into HPKE sealing
  (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/managed_secrets/src/envelope.rs#L33-L80),
  and Oh My Pi binds the broker URL into its cache's AAD. The design note does not say what the AAD
  is; it should.
- *Register every granted value with the node's `SecretSet` at receipt* (the design note does this),
  the way OpenClaw registers each minted credential and GitHub token for redaction.
- *Revalidate per frame.* Codex checks bearer credentials once at upgrade and documents that expiry
  is not enforced after
  (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330);
  OpenClaw re-checks hash, expiry, state and epoch before every worker request. Our link should
  refuse a `session.prompt` whose `grant_id` is expired or revoked on the node, and the master
  should drop the link of a node removed mid-turn (the design note's `grant.drop` then link drop
  does this; test that a prompt sent after removal is refused).
- *Forge tokens* will be readable by the agent if they are in git's environment inside the
  sandbox (OpenClaw corrected finding). Hand them only to the git child, scoped to `[forge] hosts`,
  short-lived, and expect the agent can read them when it can run git.

**3. The proxy path (`ModelAccess::Proxy`) must be bounded before it is built.** OpenCodex is the
reference; read `src/server/inbound-body-admission.ts`, `src/lib/translator-budget.ts`,
`src/bridge/sse.ts`, `src/server/responses/request-send-budget.ts`,
`src/server/responses/combo-stream-preflight.ts` and `src/combos/failover.ts`. Oh My Pi's
`auth-gateway/clients.ts` is the reference for scoped tokens. Our process must stay under 50 MB
idle, and OpenCodex says its byte budgets "do not cap RSS" because bodies are materialised several
times. So:

- Stream the request body through without materialising it more than once; cap it well below
  OpenCodex's 256 MiB (model requests with images are the only large ones); admit against a shared
  byte budget with a bounded wait queue and shed with 503 plus `Retry-After`.
- Make the response a pull stream with a buffer of one event and cancel the upstream on client
  drop, as `bridgeToResponsesSSE` does. Cap per-event and per-turn retained bytes.
- Put the retry budget on the logical request and on the turn (OpenCodex charges a workflow root so
  fan-out cannot multiply sends). Pull request 7's provider retry with backoff must count against
  this budget when the call goes through the proxy.
- Fail over only before output, with a byte-bounded replay buffer that commits on any event it does
  not recognise; separate "may try another target", "cool this target" and "cooldown scope", and
  persist cooldowns as absolute times.
- Check scope after routing; refuse metered or paid targets unless the workspace flagged them
  (OpenCodex finding 25).
- The proxy token is a bearer. Store only its hash with a domain tag (OpenClaw), bind it to one
  session and one node, give it the grant's TTL, and test that it does not work for another session
  (design note phase 8 already lists that test). Oh My Pi's gateway tokens have no expiry; do not
  copy that.
- The relay side on a per-user node should strip any credential header the agent's request carries
  and add only its own, as `ocx-child` does
  (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/relay.rs#L225-L246).
  For our link the node already authenticates to a pinned master, so the mutual HMAC challenge is
  not needed.

**4. Refuse these authentication shortcuts outright.** Each has a project in this group that ships
it, and each would break an invariant or our multi-user model:

- **`Origin` as authentication** (Cline hub). Origin is a CSRF check on top of a credential, never
  instead of one. Our master already requires the cookie; keep any future local socket the same.
- **Unauthenticated loopback** (Codex app-server and exec-server, OpenCodex data plane and
  companion listener, Oh My Pi `--no-auth`, goose `--dangerously-unauthenticated`, OpenHands MARS
  without a key). The agent's shell is a local process; a sandboxed command with network on reaches
  loopback (Codex finding, inferred). No route on the master or node may be open on loopback,
  including a future proxy or health route that does more than return `ok`.
- **Any credential in a URL**: `?auth_token=` (OpenCode), `?token=` (goose desktop, DeepSeek launch
  URL, Oh My Pi relay), `?roomSecret=` (Cline dashboard), the VS Code editor token (OpenHands),
  `#token=` (OpenClaw). Our invariant already forbids it; the claim code and the pair code must
  travel in a POST body, never as a link. OpenClaw's endpoint schema (no userinfo, no query) is a
  test to copy for `node join` URLs.
- **Credentials in served HTML or browser storage** (OpenHands injects its key into `index.html`
  and `localStorage`; OpenClaw keeps a device seed in `localStorage`). Our UI must hold nothing but
  the `HttpOnly` session cookie; if we ever add browser device keys, use a non-extractable WebCrypto
  key.
- **Non-constant-time comparison of any secret** (OpenCode `===`, Cline dashboard `===`, Oh My Pi
  `Set.has` and `!==`, goose MCP-app proxy `!=`). Grep our code for `==` on secret types; compare
  hashes in constant time as the design note's pair-code step 4 says.
- **Tokens derived from one shared secret with no per-token revocation** (OpenCode pairing tokens
  are an HMAC of the password; DeepSeek cookies are HMACs of one persistent secret). Keep
  server-side session rows, as we do.
- **One shared admin-equivalent bearer** (Hermes API server key, goose secret, OpenHands session
  key, OpenCodex env token, which carries no scope). Our `api-token` is admin-equivalent and "only
  the API token sees all" chats (`own-rizzy-harness.md`), which is the same weakness and sits
  against "chats are private to their owner, for admins too". This is outside the design note;
  it should get its own finding: per-user API tokens, stored as hashes, scoped, revocable.
- **Bearers stored in plaintext** (OpenClaw device tokens, OpenCodex data keys in `config.json`).
  Store hashes of anything we issue; OpenClaw does it for worker credentials and not for device
  tokens, which shows how easy it is to be inconsistent.
- **Approvals answerable by any connection** (Cline `approval.respond`). Bind an approval to the
  session, the owner and the tool-call identity on the master; the node already decides policy.
- **A silent fallback to a weaker store** (goose keyring to plaintext; OpenHands in-memory key).
  OpenCodex shows the alternative: fail closed with a clear error ("no plaintext fallback"; 503 if a
  token file cannot be hardened). The design note's "systemd credentials, else 0600 file" must be an
  explicit, logged choice at join, not a fallback on error.
- **Trust on first use with a blank fingerprint** (goose desktop) and SSH-verified auto-approval by
  network position (OpenClaw). Our pairing binds to a pinned certificate and a typed code; keep it.
- **Secrets on the command line** (Pi `--api-key`, goose's container path `-e KEY=VALUE`). Codex
  reads its remote token only from a named environment variable and only over `wss` or loopback;
  Hermes passes Docker values by name only.

On pairing specifically, OpenCodex's rules to copy into design note §4.4: store only the code's
hash; delete the pending request before validating anything else (one shot even on rejected
metadata); refuse requests carrying an `Origin` header; rate-limit by the kernel-observed peer
address; give every failure one generic message. Our design inverts the direction (the node shows
the code, the admin types it), so an attacker who reaches the node port can open pending requests
and print codes on its own terminal. The defence is that the admin compares the key prefix. The
design note shows 16 hex characters (64 bits); goose uses a 128-bit fingerprint in eight groups for
out-of-band comparison
(https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/card.rs#L56-L75).
Show 128 bits, grouped, on both sides.

**5. Check our environment allow-list and redaction against these lists.** Tests to add in
`crates/tools/src/env.rs` and the redaction code (pull request 4):

- *Always strip, even if `env_pass` names it.* Codex keeps a non-inheritable list that `set` cannot
  re-add (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L14-L28);
  Hermes had a published advisory (GHSA-rhgp-j443-p4rf) because a passthrough name could register a
  managed credential, and now re-checks at use time and fails closed
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/env_passthrough.py#L46-L79).
  Test: `env_pass = ["*"]` and `env_pass = ["*_KEY", "HARNESS_*"]` still pass no provider key, no
  `HARNESS_*`, no link credential.
- *Secret-name substrings for any future blocklist layer:* Hermes `execute_code` drops names
  containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `CREDENTIAL`, `PASSWD`, `AUTH`, `DSN`,
  `WEBHOOK`, `CREDS`, `BEARER`, `APIKEY`, `COOKIE`
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_env.py#L57-L92).
  DeepSeek's shorter pattern lets `DATABASE_URL`, `GITHUB_PAT` and proxy URLs with passwords
  through; use those three as test cases for `env_pass` warnings.
- *Injection variables:* OpenClaw's ~110 blocked-everywhere keys and prefixes (`NODE_OPTIONS`,
  `PYTHONPATH`, `BASH_ENV`, `GIT_*` executables, `LD_`, `DYLD_`, `BASH_FUNC_`)
  (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65)
  and goose's 31-key disallow list
  (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension.rs#L91-L122).
  Test that `env_pass` cannot pass any of them and that `PATH` cannot be overridden from a request.
- *Git location variables:* Codex and Oh My Pi strip `GIT_DIR`, `GIT_CONFIG*`, `GIT_WORK_TREE`;
  goose adds `-c safe.bareRepository=explicit -c core.fsmonitor=false`. Our checkpoint git already
  clears the environment; the forge git path (design note §4.7) must too.
- *Project `.env`:* Oh My Pi strips values that came from the project's `.env`; DeepSeek accepts a
  provider key from the cwd `.env`. Test: a repository `.env` neither enters a child environment
  nor supplies a key.
- *Our own spawn secrets:* capture them into memory and delete them from the process environment
  before any child is spawned (Cline core; OpenCode stdio mode). Test: `HARNESS_*` and the node key
  path variable are absent from `/proc/<child>/environ`.
- *MCP and future plugin children:* start them from the same allow-list (Hermes:
  `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR, XDG_*` plus declared env), not inherited
  (Cline, Pi, Warp, Zed and Oh My Pi inherit everything).
- *Redaction, by value first.* OpenCodex's log guard checks text against every value it holds,
  rebuilds the set on a content digest, and fails closed when it cannot enumerate it
  (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/configured-credentials.ts#L1-L30).
  Our `SecretSet` should be the same: grant keys, forge tokens and the link key added at receipt,
  and the scrubber should refuse to emit (not emit unredacted) when the set is known to be
  incomplete.
- *Redaction patterns as a second layer:* Hermes always masks vendor-prefixed keys, auth headers,
  JWTs and private keys and runs a `KEY=value` pass on env-dump commands; for file reads it writes a
  non-reusable sentinel so the agent cannot copy a masked value back into config
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/redact.py#L1491-L1530).
  Warp's default pattern list (Google, OpenAI, Anthropic, `sk-`, AWS, Slack, GitHub, Stripe, JWT and
  others) is a test corpus
  (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/secret_redaction/src/lib.rs#L296-L360).
- *Ordering and coverage:* redact before any hook or plugin sees output (Hermes redacts after an
  output hook); redact before compaction summaries are stored (Hermes redacts summariser inputs and
  output; pull request 7's compaction should be tested with a key in a tool result); and never use
  reversible placeholders that turn back into real values in tool arguments (Oh My Pi).

**6. Trust gates, for what nodes load from a repository.** The node will eventually load project
files that execute or steer (skills, MCP servers, agent definitions, setup scripts). Copy Codex's
rule that trust state lives only in a layer the repository cannot write
(`codex-rs/hooks/src/config_rules.rs`), Pi's rule that the default cannot be relaxed from the
project (`settings-manager.ts` L1020-L1023), Pi's two-pass startup (decide with trusted code only),
and Warp's "project MCP servers never auto-start". Add what none of them has: bind the decision to a
hash of every file that will execute, including scripts a hook points at (Codex's gap), and store it
in `node.toml`'s directory, not on the master. Oh My Pi's `isProjectTrusted() => true` is the
anti-pattern; OpenCode and goose show what a cloned repository can do without a gate.

**7. Smaller items.**

- Refuse to start on a loose file mode for every secret file (DeepSeek's `.credentials.yaml`
  check; our AUDIT L7 notes the `api-token` mode is not enforced on read on `main`).
- Describe parse errors of secret files without quoting lines (DeepSeek).
- Keep idempotency and webhook tokens in headers only; OpenClaw rejects `?token=` with 400 and does
  not exempt loopback from failure throttling. Ours already uses a bearer header and stores hashes.
- Credential pools (Hermes, OpenClaw auth profiles, OpenCodex) belong on the master per workspace
  once keys move there; key the backoff on the failure reason, separate "disabled until a human
  acts" (billing, permanent auth) from "cooling", and never record pool state in the event log.

## 7. Not settled

- **OpenHands Agent Server**: whether it closes a WebSocket that never sends the auth frame, which
  names `sanitized_env()` strips, whether `base_state.json` persists the session key carried in
  `LookupSecret` headers, and server-side output masking. Needs `OpenHands/software-agent-sdk`.
- **Warp**: server-side redaction, the self-hosted worker's authentication (`oz-agent-worker`).
- **Codex**: `credential_broker.rs` was not opened by the reviewer (defaults only); the netns proxy
  bridge internals are unchecked; whether Codex guards an unauthenticated loopback listener from a
  sandboxed command with network on was not found; remote-control pairing-code expiry is
  server-side.
- **OpenClaw**: the secret egress proxy's bypass by a process that ignores proxy variables is
  inferred (no namespace found on that path); plugin install trust and the browser sandbox CDP token
  were not read in this round; the POSIX permission check on the GitHub profile directory was not
  seen.
- **Hermes Agent**: the terminal blocklist, the output redaction finding and credential pools are
  unchecked; the relay connector's buffer and use of the delivery key are not in the repository.
- **Oh My Pi**: no gateway token expiry was found, and routing after the gateway's auth check was not
  traced; MCP OAuth and export redaction are unchecked; the CORS attack on `--no-auth` is inferred.
- **OpenCode**: `--remote` encryption lives in `@opentunnel/*`; MCP OAuth was not read; the
  self-approval path with the leaked password is inferred.
- **Cline**: the provider key's lifetime in the remote hub's memory and whether it reaches disk;
  the Origin-shortcut exploit was not run against a hub.
- **OpenCodex**: the Home side of the Remote Link relay was not traced end to end; the value-based
  log guard (finding 11), admin-token rules (16), the unauthenticated loopback listener (15), the
  stream preflight (24), failover ordering (22), the send budget (7) and metered consent (25) are
  unchecked. Recommendations in section 6 item 3 rest on several of these; re-read
  `combo-stream-preflight.ts`, `request-send-budget.ts` and `configured-credentials.ts` before
  copying numbers.
- **Zed**: whether `download_server_binary` refuses a `None` checksum; Zed's provider-key storage
  and collaboration auth were not studied.
- **goose**: the WebSocket Origin policy and the desktop's token-in-URL flow are unchecked.
- **For our own design**: whether Landlock (pull request 4) blocks `ptrace` and `/proc/<pid>/mem`
  of the node process from a confined child, and whether `systemd-creds` works for user services,
  were not in any code read; both need a test on our side before item 1 is claimed done.
- Repositories not cloned that would settle open points: `OpenHands/software-agent-sdk`,
  `warpdotdev/oz-agent-worker`, the `@opentunnel` packages, and the vendor relays and registries
  named above.

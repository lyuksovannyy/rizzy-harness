# Synthesis 07: models, providers and inference configuration (Area 14) and observability (Area 19)

Research date 2026-11-10. Synthesis only: every fact below comes from the survey and review files in `research/round1/`. "Nothing documented" means the survey found no page, not that the feature is absent.

## 1. Scope

Areas covered: **Area 14** (models, providers and inference configuration) and **Area 19** (observability: usage and cost, logs, tracing, debugging). A few Area 14 facts live in other areas' entries (retry and watchdogs, voice, goals, hosted pricing) and were pulled in by keyword search; they are cited to their own entries.

Files and ranges read:

| File | Ranges read |
|---|---|
| `own-rizzy-harness.md` | sections 8 and 16, plus the loop and multi-user tables (budget, retry, usage, per-user limits), lines 1-404 searched |
| `codex.md` | 54-90 (goals, retry), 688-730 (Area 14), 882-910 (Area 19), 986-1196 (sections 3, 6, 7, 9) |
| `opencodex.md` | 101-195 (Area 14, Area 19), 195-328 (sections 3, 6, 7, 9), Part B |
| `opencode.md` | 100-106, 340-452 (Area 14, Area 19, hosted offerings), 458-610 filtered |
| `claude-code.md` | 150-260 (retry, watchdogs, budget caps), 2981-3274 (Area 14), 3324-3328 (voice), 4150-4152 (gateway spend limits), 4216-4456 (Area 19), 5055-5066 (legal), 5202-5400 filtered (sections 3, 6, 7); never read whole |
| `claude-code.review-1/2/3.md` | review-2 whole; review-1 and review-3 searched for model, cost, telemetry items |
| `claude-desktop.md` | 282-300, 407-420, 469-607 filtered |
| `deepseek-harness.md` | 363-400, 451-480, 533-677 filtered |
| `antigravity.md`, `cline.md`, `cursor.md`, `goose.md`, `hermes-agent.md`, `oh-my-pi.md`, `openclaw.md`, `openhands.md`, `pi.md`, `warp.md`, `zed-acp.md` | Area 14 and Area 19 sections in full, voice/vision/retry/subscription entries by keyword search, sections 3, 6, 7, 9 filtered by keyword |

Could not read / limits:
- `claude-code.md` was read only in the ranges above. Many Claude Code Area 14 entries (providers, gateway, Bedrock, Vertex) were **not** checked by review-2 (it confirmed 47 of about 190 entries); they carry `unreviewed`.
- URL shorthands expanded as the surveys define them: Hermes Agent paths are relative to `https://hermes-agent.nousresearch.com/docs`; OpenHands `D:` is `https://docs.openhands.dev`; Oh My Pi `D` is `https://omp.sh/docs` and `R` is `https://github.com/can1357/oh-my-pi`; Warp pages were read "via" index files (`agents.txt`, `support.txt`, `enterprise.txt`), and I give the page URL.
- Short name **OpenCodex** means the gateway `lidge-jun/opencodex` throughout; where I mean the unrelated `RyensX/OpenCodex` browser middleware I say "OpenCodex (RyensX)".
- Check labels: `reviewed` = entry marked verified or confirmed in review; `corrected` = corrected in review and the corrected text used; `unreviewed` = not marked; `unverified` = survey or review could not confirm. Where a row mixes a reviewed fact with unreviewed ones I say so in the row.
- The reader's harness ("ours") is described only from `own-rizzy-harness.md`.

---

## 2. Features

### Area 14: models, providers and inference configuration

#### Provider support and bring-your-own-key
What it is: which model providers a harness can talk to with a key the user supplies, and how that key is stored and resolved.
Variants: breadth ranges from a single vendor's models (Claude Code, Codex) to 40-60+ providers (OpenCodex, Oh My Pi, goose). Credential handling differs: Pi, Oh My Pi and Codex document a lookup order; DeepSeek harness makes keys write-only in the UI; Warp and Cursor restrict BYOK to specific plans or run modes; OpenClaw and Antigravity are unclear or contradictory about BYOK. Provider counts on home pages are repeatedly stale (Pi, goose) or unverified (OpenCodex, Oh My Pi).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Built-in providers `openai`, `ollama`, `lmstudio`, `amazon-bedrock`, `gcp` (Vertex); the provider must speak the Responses API. API-key use is billed at API rates; ChatGPT sign-in uses plan limits. | GA | https://learn.chatgpt.com/docs/models.md, https://learn.chatgpt.com/docs/config-file/config-advanced.md, https://learn.chatgpt.com/docs/auth.md | corrected |
| OpenCodex | Gateway with 40+ built-in providers (README/home claim; full matrix not opened) plus any OpenAI-compatible endpoint. Credentials: OAuth with auto-refresh (xAI, Anthropic, Kimi), forwarded `codex login`, pasted keys, `${ENV_VAR}` resolved per request; OAuth tokens kept in separate stores, not `config.json`. | GA | https://github.com/lidge-jun/opencodex, https://opencodex.me/guides/model-routing/, https://opencodex.me/reference/configuration/ (Secrets), https://opencodex.me/reference/architecture/ | reviewed (provider count) / unreviewed (auth row) |
| OpenCode | Catalogue from models.dev plus native packages (OpenAI, Anthropic, Google, Vertex, Azure, Bedrock, OpenRouter, xAI), npm or `file://` provider packages, `/connect` flow. | GA | https://opencode.ai/docs/providers/, https://opencode.ai/docs/models/ | unreviewed |
| Claude Code | Anthropic models only. Anthropic API key or Console, `apiKeyHelper` command (cached 5 min), or cloud providers (see cloud-platform feature). A gateway may sit in front, but Anthropic does not support routing non-Claude models through any gateway. | GA | https://code.claude.com/docs/en/llm-gateway-connect.md, https://code.claude.com/docs/en/gateways.md, https://code.claude.com/docs/en/llm-gateway.md | unreviewed |
| Claude Desktop | "3P" mode routes all inference through Google Cloud Agent Platform, Amazon Bedrock, Microsoft Foundry, an LLM gateway or the Anthropic API directly; config via MDM, bootstrap endpoint or admin console. | GA (inferred; page has no label) | https://claude.com/docs/third-party/claude-desktop/overview.md | corrected |
| DeepSeek harness | Settings -> Models takes a DeepSeek key; other shipped providers (anthropic, openai, moonshotai, zai) via a catalogue. Keys are write-only in the UI, stored in `$DSH_HOME/.credentials.yaml`, referenced by env-var name, resolved per request so rotation needs no restart. Encryption at rest: nothing documented. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/credentials | corrected (credentials) / unreviewed (catalogue) |
| Oh My Pi | README claims 60+ providers (docs list about 69 IDs, give no total): frontier APIs, coding-plan logins, cloud, gateways, local. Credential source order: `--api-key`, `models.yml` key, stored OAuth, `/login` key, env var. Subscription and API products are separate provider IDs. | GA | https://omp.sh/docs/providers, https://github.com/can1357/oh-my-pi | reviewed |
| Pi | `/login` for subscription or API key, env vars for CI; more than thirty documented providers (homepage says "15+"). Lookup order: `--api-key`, `auth.json`, `models.json` apiKey, env/ambient cloud credentials. | GA | https://pi.dev/docs/latest/providers, https://pi.dev/docs/latest/models | unreviewed (the "15+" homepage text is confirmed in review) |
| Cursor | Curated model list (Cursor-labelled Composer and Grok models plus Claude, GPT, Gemini and others). BYOK not available on Plus; on Teams and Enterprise the Cursor Token Rate applies to BYOK usage. | GA | https://cursor.com/docs/models-and-pricing | reviewed (BYOK) |
| Antigravity | Gemini, Claude and GPT-OSS models gated by plan. Plans page says BYOK or custom endpoints for extra rate limits are not supported, yet CLI settings expose `modelProvider` (with `GEMINI_API_KEY`) and the SDK takes Gemini keys and Vertex. | unclear (CLI), GA (SDK) | https://antigravity.google/docs/plans, https://antigravity.google/docs/cli/reference, https://antigravity.google/docs/settings | unreviewed |
| OpenHands | Named LLM profiles (provider, model, base URL, key or a Provider Connection) per backend; calls go through LiteLLM. Backend, not browser, must reach the base URL. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/llm-profiles.md, https://docs.openhands.dev/llms.txt | unreviewed |
| OpenClaw | Model refs `provider/model` in JSON5; Anthropic, OpenAI, Google, Bedrock, Mistral, xAI, DeepSeek, Groq, Moonshot, Qwen, Z.AI, OpenRouter, LiteLLM, gateways, Copilot, local. Per-person credentials tied to a Gateway profile in multi-user mode. Providers page does not list auth methods. | GA | https://docs.openclaw.ai/providers, https://docs.openclaw.ai/concepts/features, https://docs.openclaw.ai/concepts/multi-user | unreviewed |
| goose | Direct APIs, cloud platforms, gateways, subscription providers, local engines, CLI and ACP providers. Auth: API key, cloud credentials, browser PKCE, device flow, refreshable commands. README says 15+, docs list well over 40. | GA | https://goose-docs.ai/docs/getting-started/providers.md | unreviewed |
| Hermes Agent | Nous Portal, subscription OAuth, routers, direct providers, Bedrock, Vertex, Azure Foundry, any OpenAI-compatible server. Keys in `~/.hermes/.env`, OAuth in `auth.json`, `key_cmd` helpers; needs at least 64K context for agent work. | GA | https://hermes-agent.nousresearch.com/docs/integrations/providers, https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| Zed/ACP | Zed-hosted models, API keys (Anthropic, OpenAI, Google, Mistral, DeepSeek, xAI, OpenCode), gateways, subscriptions, local. Keys in the system keychain. For external ACP agents, the agent owns auth, model, tools and billing. | GA | https://zed.dev/docs/ai/llm-providers.md, https://zed.dev/docs/ai/use-api-access.md, https://zed.dev/docs/ai/agent-settings.md | unreviewed |
| Warp | BYOK for Anthropic, OpenAI and Google keys, stored on device, local runs only; cloud runs do not support personal keys; Auto models always use Warp inference; Warp-credit fallback off by default; orgs of more than 10 need Business or Enterprise. | GA | https://docs.warp.dev/agents/inference/bring-your-own-api-key/, https://docs.warp.dev/agents/cli/models-and-usage/ | unreviewed |
| Cline | Cline provider (300+ models in Desktop), ClinePass, Anthropic, OpenAI, Google, Bedrock, Vertex, Azure, DeepSeek, Qwen, OpenRouter, OpenAI-compatible, "30+ other", local, LiteLLM (enterprise). CLI `-P`, `-m`, `-k`. List taken from nav titles. | GA | https://docs.cline.bot/sdk/model-providers.md, https://docs.cline.bot/cli/cli-reference.md, https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| rizzy-harness (ours) | Two wire formats through the `genai` crate: Anthropic Messages (API key only) and OpenAI-compatible chat completions (OpenAI, OpenRouter, DeepSeek, Ollama, vLLM). Many providers at once, ids `<provider>::<model>`, routed by prefix, hot-swappable; presets; keys by env-var name or 0600 `secrets.toml`, stored keys only sent to the base URL they were saved for. No native Gemini, Bedrock or Vertex adapter. | works | `crates/agent-native/src/llm.rs:1-27`, `:47-100`, `:326-380`, `crates/master/src/presets.rs`, `crates/master/src/secrets.rs:1-15` | from own inventory |

Nothing documented as a distinct BYOK path: Claude Desktop beyond 3P, OpenCodex (RyensX) (fronts Codex).

#### Cloud-platform providers and enterprise bring-your-own-cloud
What it is: using models served from the customer's own AWS, Google Cloud or Azure account instead of the vendor's API.
Variants: Claude Code and Claude Desktop treat these as first-class provider modes with startup checks and region/prefix logic; Warp routes its own agent through the org's cloud via federation; most others list Bedrock/Vertex/Azure as one provider among many.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `CLAUDE_CODE_USE_BEDROCK`, `_VERTEX`, `_FOUNDRY`, `_ANTHROPIC_AWS`, `_MANTLE`; setup wizards (`/setup-bedrock`, `/setup-vertex`); AWS and ADC credential chains with refresh commands; startup model-access checks and remembered refusals on Bedrock and Agent Platform; Foundry has no startup check. Aliases resolve to different model versions per provider; docs say to pin `ANTHROPIC_DEFAULT_*_MODEL` before rollout. Telemetry and error reporting are off by default on these providers. `allowedProviders` can restrict them (see governance). | GA | https://code.claude.com/docs/en/amazon-bedrock.md, https://code.claude.com/docs/en/google-vertex-ai.md, https://code.claude.com/docs/en/microsoft-foundry.md, https://code.claude.com/docs/en/claude-platform-on-aws.md, https://code.claude.com/docs/en/model-config.md | unreviewed |
| Claude Desktop | 3P deployment routes Chat, Cowork and Code through Agent Platform, Bedrock, Foundry or a gateway; computer use, Claude in Chrome (mostly), cloud sessions and Remote Control unavailable on 3P. | GA (inferred) | https://claude.com/docs/third-party/claude-desktop/overview.md, https://claude.com/docs/third-party/claude-desktop/feature-matrix.md | corrected |
| Codex | Built-in `amazon-bedrock` and `gcp` (Vertex) providers; enterprise gateway, Bedrock and service-account pages were only skimmed. | GA | https://learn.chatgpt.com/docs/models.md, https://learn.chatgpt.com/docs/amazon-bedrock.md | unreviewed |
| Warp | Enterprise BYOLLM through the org's cloud: Bedrock (interactive via each member's AWS CLI session; cloud agents assume an IAM role by OIDC federation) and Gemini Enterprise / Vertex (Workload Identity Federation; interactive only). | GA (Enterprise) | https://docs.warp.dev/enterprise/enterprise-features/bring-your-own-llm/ | unreviewed |
| Cline | Bedrock with API key, IAM or CLI profile/SSO; Vertex; Azure. | GA | https://docs.cline.bot/sdk/model-providers.md | unreviewed |
| goose | Bedrock, SageMaker TGI, Azure AI Foundry and Azure OpenAI, Vertex AI, Snowflake, Tanzu. | GA | https://goose-docs.ai/docs/getting-started/providers.md | unreviewed |
| Hermes Agent | Bedrock (AWS chain), Vertex AI (ADC), Azure Foundry. | GA | https://hermes-agent.nousresearch.com/docs/integrations/providers | unreviewed |
| Oh My Pi | `models.yml` api families include `bedrock-converse-stream`, `google-vertex`, `azure`. | GA | https://omp.sh/docs/custom-models | unreviewed |
| Pi | Azure OpenAI, Amazon Bedrock, Cloudflare AI Gateway and Workers AI, Google Vertex AI. | GA | https://pi.dev/docs/latest/providers | unreviewed |
| OpenCode | Native Vertex, Azure and Bedrock packages. | GA | https://opencode.ai/docs/providers/ | unreviewed |
| Antigravity | CLI `gcp` settings; SDK supports Vertex. | unclear (CLI), GA (SDK) | https://antigravity.google/docs/settings, https://antigravity.google/docs/sdk/local-models | unreviewed |
| Cursor | Bedrock configuration exists in the CLI (`/bedrock`, feature-flagged). | GA (feature-flagged) | https://cursor.com/docs/models-and-pricing | reviewed |
| DeepSeek harness | Provider-managed credential files (AWS profiles, Google ADC) are not detected. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers | unreviewed |
| rizzy-harness (ours) | None: no native Bedrock or Vertex adapter; an OpenAI-compatible proxy in front is the only route. | none | `crates/agent-native/src/llm.rs:1-27` | from own inventory |

Nothing documented: OpenCodex (cloud-platform providers not enumerated in what was read), OpenClaw beyond Bedrock in its provider list, Zed beyond Bedrock as a gateway.

#### Custom endpoints, wire formats and gateways as a provider
What it is: pointing the harness at any HTTP endpoint that speaks a known wire protocol (Chat Completions, Responses, Messages), and how it copes with quirks.
Variants: the protocol the endpoint must speak differs sharply. Codex accepts only the Responses API. Claude Code accepts only Anthropic-format (and Bedrock/Vertex-format) endpoints and publishes a candid compatibility guide for gateways. DeepSeek harness, Oh My Pi and Pi expose compat-flag matrices. Warp requires the endpoint to be publicly reachable.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `[model_providers.<id>]` with `base_url`, `env_key`, headers, command-backed bearer token (refresh interval configurable; 300 s is only the doc example), native gateway OAuth, capability flags. Responses API only; `wire_api = "chat"` unsupported. Project config cannot set providers or base URLs. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md, https://learn.chatgpt.com/docs/models.md | corrected |
| Claude Code | Set `ANTHROPIC_BASE_URL` plus one credential. Supported client formats: Messages, Bedrock InvokeModel, Agent Platform rawPredict. Gateway must forward `anthropic-beta` verbatim, stream event by event, keep SSE pings. The client auto-retries with a rejected beta value or field removed (thinking type, cache_control, effort, context_management, tool search) and remembers the omission for stated durations. Optional gateway model discovery via `GET /v1/models`. Optional hint headers (`x-claude-code-request-class` and others). | GA | https://code.claude.com/docs/en/llm-gateway-connect.md, https://code.claude.com/docs/en/llm-gateway-protocol.md | unreviewed |
| OpenCode | npm or `file://` provider packages; `compatibility` block for API quirks; WebSocket transport per provider; variants carry settings, headers, body. | GA | https://opencode.ai/docs/models/, https://opencode.ai/docs/config/ | unreviewed |
| OpenCodex | Gateway that serves OpenAI Responses, Chat Completions and Anthropic Messages shapes so many clients can use it; client configs for 28 clients exported or managed (the survey's "about 25" was low; review). | GA | https://opencodex.me/reference/adapters/, https://opencodex.me/reference/client-support/ | corrected |
| DeepSeek harness | Custom model API over OpenAI Chat Completions, OpenAI Responses or Anthropic Messages with a base URL and a manual model list or `GET /models` discovery; compat switches `supportsDeveloperRole`, `maxTokensField`, `thinkingFormat: deepseek`. Advanced settings in `cordis.patch.yml`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers | unreviewed |
| Oh My Pi | `models.yml` providers with baseUrl, api family (openai-completions/responses, codex-responses, azure, anthropic-messages, bedrock-converse-stream, google variants), apiKey (env, `!cmd`, literal), per-model reasoning/cost/context/tokenizer, discovery, large compat-flag matrix. | GA | https://omp.sh/docs/custom-models | unreviewed |
| Pi | `models.json` providers with `baseUrl`, `api` type, `apiKey` and headers (literal, env interpolation, or `!command` run per request), `modelOverrides`. | GA | https://pi.dev/docs/latest/models | unreviewed |
| goose | Custom providers by JSON in OpenAI, Anthropic or Ollama formats with headers, model lists, per-model prices. | GA | https://goose-docs.ai/docs/getting-started/providers.md | unreviewed |
| Warp | Any OpenAI-compatible Chat Completions endpoint (OpenRouter, LiteLLM, z.ai) for local agents; must be publicly reachable (localhost and private addresses rejected); requests relayed through Warp; no cloud runs; Auto models ignore it. | GA | https://docs.warp.dev/agents/inference/custom-inference-endpoint/ | reviewed |
| OpenClaw | Custom OpenAI- or Anthropic-compatible endpoints. | GA | https://docs.openclaw.ai/providers | unreviewed |
| Zed/ACP | Anthropic- and OpenAI-compatible endpoints; an RFD for "configurable LLM providers (custom endpoint)" in ACP (title only). | GA (Zed); RFD (ACP) | https://zed.dev/docs/ai/llm-providers.md, https://agentclientprotocol.com/updates.md | unreviewed |
| Antigravity | SDK works with any OpenAI-compatible server; CLI `customModelsConfig` exists but its format is not documented. | unclear | https://antigravity.google/docs/sdk/local-models, https://antigravity.google/docs/cli/reference | unreviewed |
| rizzy-harness (ours) | OpenAI-compatible chat completions only (no Responses API in the inventory); same-origin-only redirects so keys do not follow redirects; presets for popular platforms; providers editable from the UI. | works | `crates/agent-native/src/llm.rs:221-258`, `crates/master/src/http/providers.rs` | from own inventory |

#### Local models
What it is: running or connecting to a model on the user's own hardware.
Variants: most harnesses simply point at Ollama, LM Studio, vLLM or llama.cpp through the OpenAI-compatible path. Pi ships a built-in llama.cpp router that loads GGUF files on demand; goose has an in-process llama.cpp inference crate and a `local-models` command; Cline Desktop can install and start Ollama and LM Studio. Warp is the counter-case: its custom endpoint must be publicly reachable.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `--oss` with `--local-provider` or `oss_provider` (ollama or lmstudio); `codex exec` errors if none is set. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md (OSS mode) | unreviewed |
| OpenCode | Local auto-discovery for Ollama, LM Studio and vLLM. | GA | https://opencode.ai/docs/providers/ | unreviewed |
| Cline | Desktop Settings, Provider, Local: Ollama or LM Studio; can install and start them, recommend models for hardware, download, show a memory indicator. v4.1.25: local requests no longer fail after 5 minutes of silence. | GA (Desktop) | https://docs.cline.bot/usage/cline-desktop.md, https://github.com/cline/cline/releases | unreviewed |
| goose | Ollama, LM Studio, Docker Model Runner, Lemonade, Ramalama, Atomic Chat; `goose local-models` searches, downloads, lists and deletes; in-process llama.cpp (GGUF) and optional MLX in the `goose-local-inference` crate. How the desktop Local provider relates is not stated. | unclear | https://goose-docs.ai/docs/guides/goose-cli-commands.md, https://goose-docs.ai/docs/gdk/concepts/providers.md | unreviewed |
| Hermes Agent | Any OpenAI-compatible local server (Ollama, vLLM, SGLang, llama.cpp, LM Studio, LiteLLM); needs at least 64K context for agent work. | GA | https://hermes-agent.nousresearch.com/docs/integrations/providers | unreviewed |
| OpenClaw | Local model pages for Ollama, LM Studio, vLLM, SGLang, llama.cpp; experimental `localModelLean` override. | GA / experimental | https://docs.openclaw.ai/concepts/experimental-features | unreviewed |
| Oh My Pi | Local engines auto-detected; discovery modes for ollama, llama.cpp, lm-studio. | GA | https://omp.sh/docs/providers, https://omp.sh/docs/custom-models | reviewed (providers) |
| Pi | Built-in llama.cpp router (`/llama`) loads GGUF files on demand; Ollama, LM Studio, vLLM, SGLang via `models.json`. | GA | https://pi.dev/docs/latest/models | unreviewed |
| Zed/ACP | llama.cpp, Ollama, LM Studio, OpenAI-compatible. | GA | https://zed.dev/docs/ai/use-a-local-model.md | unreviewed |
| OpenCodex | Ollama (local and cloud) listed among providers. | GA | https://github.com/lidge-jun/opencodex | reviewed |
| Antigravity | SDK supports local LiteRT (Gemma 4) and any OpenAI-compatible server (Ollama, LM Studio, vLLM). | GA (SDK) | https://antigravity.google/docs/sdk/local-models | unreviewed |
| OpenHands | A "local LLMs" provider page exists. | GA | https://docs.openhands.dev/llms.txt | unreviewed |
| Warp | Custom endpoint must be publicly reachable; localhost and private addresses are rejected, which rules out local servers. | GA | https://docs.warp.dev/agents/inference/custom-inference-endpoint/ | reviewed |
| rizzy-harness (ours) | Ollama (capabilities read from `/api/show`) and vLLM through the OpenAI-compatible path. | works | `crates/agent-native/src/models.rs:1-30` | from own inventory |

Nothing documented: Claude Code, Claude Desktop (Anthropic models only), Cursor, DeepSeek harness (beyond the custom model API).

#### Using another harness or a subscription as a provider
What it is: three different things share this name: (a) signing in with a consumer subscription (ChatGPT, Claude, Copilot, SuperGrok) so the harness bills against plan limits; (b) wrapping another harness or CLI as the model backend; (c) exposing your own subscription to other tools as a local proxy.
Variants and legal limits: **Anthropic** states third-party developers, including Agent SDK products, must use Console API keys or cloud providers and may not offer claude.ai login or route requests through Free/Pro/Max/Ultra credentials. **Antigravity**'s FAQ calls third-party tools accessing consumer plans a ToS violation. **OpenCodex** docs warn pooling gives no protection from provider enforcement and that Anthropic may suspend accounts routed through third-party proxies. **OpenCode** and **Oh My Pi** do not assess provider terms. goose and Zed implement (b) through ACP adapters; Hermes implements (c).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code (legal) | Commercial vs Consumer terms by plan. Embedding Claude Code in a product needs Commercial Terms, an unmodified binary and each end user authenticating with their own credential. OAuth is only for subscribers using Claude Code and native Anthropic apps; third parties may not offer claude.ai login, route through Free/Pro/Max/Ultra credentials, or store claude.ai session tokens. Anthropic may enforce without notice. | GA (legal terms) | https://code.claude.com/docs/en/legal-and-compliance.md | unreviewed |
| Codex | Sign in with a ChatGPT plan (Free through Enterprise) or an API key; usage shared with ChatGPT plan limits and credits. OpenCodex can forward a `codex login`. | GA | https://learn.chatgpt.com/docs/pricing.md, https://learn.chatgpt.com/docs/auth.md | unreviewed |
| OpenCode | Subscription OAuth for ChatGPT Plus/Pro and GitHub Copilot (device OAuth). A "Claude Agent" provider uses a local Claude Code install (`claude auth login`). Vendor terms "not assessed". | GA | https://opencode.ai/docs/providers/, https://opencode.ai/changelog | unreviewed |
| OpenCodex | ChatGPT account pool (quota-aware, thread affinity), Claude account pool (labelled experimental), Anthropic and xAI OAuth. Client modes for Claude Desktop "first-party" interception may breach Anthropic's terms per its own docs. | GA; Claude pool and first-party mode experimental | https://opencodex.me/guides/web-dashboard/, https://opencodex.me/guides/claude-code/, https://opencodex.me/reference/client-support/ | corrected (pool) |
| goose | ACP providers `amp-acp`, `claude-acp`, `codex-acp`, `pi-acp` use an existing subscription through the vendor's ACP adapter (replacing deprecated CLI providers); goose modes map onto the agent's modes; `session resume`/`fork` unsupported. Also ChatGPT Codex, Copilot and Z.AI Coding Plan providers. Deprecated CLI providers: `claude-code`, `codex`, `gemini-cli`. | GA; CLI providers deprecated | https://goose-docs.ai/docs/guides/acp-providers.md, https://goose-docs.ai/docs/guides/cli-providers.md, https://goose-docs.ai/docs/getting-started/providers.md | reviewed |
| Hermes Agent | Subscription OAuth logins (OpenAI Codex, Anthropic via Claude Max, xAI, Qwen, MiniMax); can adopt Codex CLI and Claude Code OAuth logins (`auth.adopt_external_logins` default true); per the page Claude Pro cannot use the OAuth path and Max consumes extra-usage credits only. A local subscription proxy (127.0.0.1:8645) lets other OpenAI-compatible apps use a Nous Portal or xAI subscription; raw inference only, no auth of its own. | GA | https://hermes-agent.nousresearch.com/docs/integrations/providers, https://hermes-agent.nousresearch.com/docs/user-guide/features/subscription-proxy | unreviewed |
| Oh My Pi | Coding-plan or subscription logins (Copilot, Cursor, GitLab Duo, Devin, Kimi, MiniMax, Z.AI); docs do not discuss provider terms of service. | GA | https://omp.sh/docs/providers | reviewed |
| Pi | `/login` for a subscription; the pi-ai README says Codex and GitHub Copilot require OAuth. | GA | https://pi.dev/docs/latest/providers | unreviewed |
| OpenClaw | Subscription OAuth (OpenAI Codex); an `openclaw` runtime plus a bundled `codex` plugin that runs OpenAI models through the Codex app-server (Codex owns loop and compaction); opt-in `copilot` and `muse` plugins; Claude CLI backend. | GA | https://docs.openclaw.ai/providers, https://docs.openclaw.ai/concepts/features | unreviewed |
| Warp | ChatGPT-plan sign-in (counts against ChatGPT limits; a saved OpenAI key takes precedence) and X Premium / SuperGrok OAuth (tokens on device; Warp cannot enforce ZDR for those requests). `--harness oz\|claude\|codex` runs another harness in cloud runs with auth through secrets. | GA | https://docs.warp.dev/agents/inference/chatgpt-subscription/, https://docs.warp.dev/agents/inference/grok-subscription/, https://docs.warp.dev/agents/cli/oz-cli/ | unreviewed |
| Zed/ACP | Provider sign-in with ChatGPT and Copilot Chat; external agents (Claude Agent, Codex, Gemini and others) run as ACP child processes that own auth, model and billing; Zed charges nothing for them. ACP `terminal` auth method lets the client run the agent's own login flow without credentials crossing the protocol. | GA | https://zed.dev/docs/ai/use-an-existing-subscription.md, https://zed.dev/docs/ai/external-agents.md, https://agentclientprotocol.com/protocol/v1/authentication.md | unreviewed |
| Antigravity | FAQ says third-party tools accessing consumer plans violate the terms. | GA (policy) | https://antigravity.google/docs/faq | unreviewed |
| Cline | OpenAI (Codex/subscription) provider. | GA | https://docs.cline.bot/sdk/model-providers.md | unreviewed |
| OpenHands | ACP agents as a "backend agent kind" per profile with credential flow by secret name; SDK "subscriptions" guide listed (not opened). | GA | https://docs.openhands.dev/llms.txt | unverified (subscriptions page not opened) |
| rizzy-harness (ours) | Deliberately none: API key only. An `AgentBackend` trait allows external harnesses but only `NativeBackend` exists. | none | `crates/agent-native/src/llm.rs:24-26`, `crates/agent-native/src/lib.rs:537-541` | from own inventory |

#### Model selection per session, per agent and per task
What it is: choosing which model answers, at which scope (whole install, workspace, session, agent, sub-agent, single turn), and what happens on resume.
Variants: Claude Code has the most explicit precedence ladder; Oh My Pi uses named roles; Cline splits Plan and Act models; goose and Hermes override per sub-agent; Zed lets each feature (commit message, summaries, compaction) pick its own model. Ours has a five-step order and is alone in offering parallel answers from several models.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Priority: `/model` in session, `--model`, `ANTHROPIC_MODEL`, `model` setting, `ANTHROPIC_DEFAULT_MODEL`. Picker saves to user settings, `s` for session only; resumed sessions keep the transcript model unless retired; sub-agent model order documented; `opusplan` alias uses Opus in plan mode and Sonnet to execute; `PreModelSwitch` hooks can block a switch. Mistyped model values are not rejected up front. | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/cli-reference.md | reviewed (precedence, sub-agent order) / unreviewed (opusplan) |
| Claude Desktop | Model picker per session; effort and fast mode for Code sessions; Remote Control can change model and effort from another device. | GA | https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/remote-control | reviewed |
| OpenCode | `provider/model#variant` selectors; precedence provider, model, variant; `agents.title.model` replaces `small_model`. | GA | https://opencode.ai/docs/models/, https://opencode.ai/docs/config/ | unreviewed |
| Codex | Effort can change mid-conversation (default since 0.165.0); `--profile` overlay files; per-turn `--cyber-access-program`. Per-agent model override: nothing documented in the entries read (sub-agents cannot override `model_provider`). | GA | https://learn.chatgpt.com/docs/agent-configuration/subagents.md, https://learn.chatgpt.com/docs/config-file/config-advanced.md | reviewed (sub-agent limits) |
| DeepSeek harness | Picking a model sets the default for new sessions; sub-agent model default comes from the catalogue. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers | reviewed (sub-agent default) |
| Oh My Pi | Ten roles (default, smol, slow, vision, plan, designer, commit, tiny, task, advisor), each a selector with ordered alternatives, `@role` aliases and thinking suffix; Alt+M hub, Ctrl+P cycles `cycleOrder`; launch overrides `--smol/--slow/--plan`; per-agent overrides; storage global or project. | GA | https://omp.sh/docs/roles, https://omp.sh/docs/agents-and-roles | reviewed |
| Pi | `/model` (searches only authenticated providers), Ctrl+P cycles, `/scoped-models`, `--models`, `enabledModels`; `--model` accepts fuzzy `provider/id:thinking`; model and thinking changes recorded as session entries and restored on resume. | GA | https://pi.dev/docs/latest/models, https://pi.dev/docs/latest/cli | unreviewed |
| Cursor | Model picker; Auto modes; sub-agent `model`; on legacy request-based plans without Max Mode, sub-agents use Composer regardless of `model`. | GA | https://cursor.com/docs/models-and-pricing | reviewed (sub-agent note) |
| Antigravity | Switching mid-run applies from the next turn; sub-agent frontmatter `model: inherit\|flash\|pro`. | GA | https://antigravity.google/docs/models, https://antigravity.google/docs/subagents | unreviewed |
| OpenHands | `/model` switches mid-conversation preserving history; named profiles per backend; routing by prefix (`openai/`, `litellm_proxy/`, `openhands/`). | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/llm-profiles.md | unreviewed |
| OpenClaw | `agents.defaults.model.primary`; separate sub-agent model `agents.defaults.subagents.model`; named gateway roles can carry a model policy; a user-chosen model is strict (no masking fallback). | GA | https://docs.openclaw.ai/providers, https://docs.openclaw.ai/concepts/model-failover | unreviewed |
| goose | `/model` in CLI (the in-session page says the CLI cannot change model via a setting; docs disagree); per-sub-agent provider and model; recipe `settings.goose_provider/goose_model/temperature/max_turns`. | GA (sub-agent); rest unclear | https://goose-docs.ai/docs/guides/multi-model/index.md, https://goose-docs.ai/docs/guides/goose-cli-commands.md | unreviewed |
| Hermes Agent | Delegated children accept per-child model/provider overrides. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation | unreviewed |
| Cline | Separate Plan and Act models; CLI samples route phases to different models with separate `--config` dirs and `--thinking`. | GA / sample | https://docs.cline.bot/core-workflows/plan-and-act.md, https://docs.cline.bot/cli/samples/model-orchestration.md | unreviewed |
| Warp | Profiles carry a base model (also used for planning unless a separate planning model is set) and permissions; profiles sync to each host. | GA | https://docs.warp.dev/agents/inference/model-choice/ | unreviewed |
| Zed/ACP | Favourites; per-feature models (`inline_assistant_model`, `commit_message_model`, `thread_summary_model`, `compaction_model`, `subagent_model`). In ACP, session config options carry categories `mode`, `model`, `model_config`, `thought_level`, set with `session/set_config_option`. | GA | https://zed.dev/docs/ai/agent-settings.md, https://agentclientprotocol.com/protocol/v1/session-config-options.md | reviewed (compaction/sub-agent model) / unreviewed (config options) |
| rizzy-harness (ours) | Order: bar pick, agent's model, user default, workspace default, server default; per-agent model and per-workspace overrides; parallel answers from up to 4 models with a pick. | works | `crates/master/src/resolver.rs:1-10`, `crates/master/src/lib.rs:951-1022`, `crates/agent-native/src/lib.rs:930-1030` | from own inventory |

#### Reasoning effort and thinking controls
What it is: a user-facing control for how much a model reasons before answering.
Variants: level names differ (Claude Code `low..max` plus a separate `ultracode`; Codex `low..ultra`; Pi and Oh My Pi `off..max`; Antigravity `low/medium/high` with a contradictory `max` mention). Claude Code and Pi save effort per model; Claude Code states the same level name is calibrated differently per model. DeepSeek harness shows the menu only for models that declare levels. Ours forwards effort only to models flagged `effort` and disables it for Anthropic models.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `low/medium/high/xhigh/max` by model; resolution: env, in-session pick, `--effort`, settings (`modelSettings` per model), model default (medium on current models); `max` session-only; an unsupported level falls back to the highest below it; `ultrathink` raises one turn; thinking cannot be turned off on current models; `maxEffortLevel` caps it for orgs. | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/settings-reference.md | reviewed (defaults) / unreviewed (levels table) |
| Claude Desktop | Effort for Code sessions. | GA | https://code.claude.com/docs/en/desktop | reviewed |
| Codex | Effort `low` through `ultra` depending on the model; mid-conversation change (0.165.0); verbosity setting. | GA | https://learn.chatgpt.com/docs/agent-configuration/speed.md, https://learn.chatgpt.com/docs/changelog | unreviewed |
| OpenCodex | Reasoning-effort mapping across providers; `jev` strategy can choose effort. | GA | https://opencodex.me/reference/configuration/, https://opencodex.me/guides/combos/ | reviewed |
| OpenCodex (RyensX) | A plugin adds an Auto model option choosing model and reasoning effort per task, with custom tiers. | GA | https://github.com/RyensX/OpenCodex/releases | reviewed |
| DeepSeek harness | Effort menu appears only where a model declares `reasoningEfforts`; `thinkingFormat: deepseek` compat. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers | unreviewed |
| Oh My Pi | `off/minimal/low/medium/high/xhigh/max/auto` per model; Shift+Tab cycles; default `high` (stated on docs/settings). Roles can carry a thinking suffix. | GA | https://omp.sh/docs/roles, https://omp.sh/docs/settings | reviewed |
| Pi | `off` through `max` limited to what the model supports, default `medium`; per-model overrides; `samplingParamsByThinkingLevel` for OpenAI-compatible APIs. | GA | https://pi.dev/docs/latest/models, https://pi.dev/docs/latest/settings | unreviewed |
| Antigravity | Low/Medium/High per turn (`--effort`, `/effort`); the CLI reference says `low` through `max` (contradiction). | GA | https://antigravity.google/docs/models, https://antigravity.google/docs/cli/reference | unreviewed |
| Cline | CLI `--thinking none..xhigh`. | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| Warp | Effort-level suffixes for most model families. | GA | https://docs.warp.dev/agents/inference/model-choice/ | unreviewed |
| Zed/ACP | ACP session config category `thought_level`; Zed `agent.model_parameters` temperature. | GA | https://agentclientprotocol.com/protocol/v1/session-config-options.md, https://zed.dev/docs/ai/agent-settings.md | unreviewed |
| rizzy-harness (ours) | `low/medium/high/max` per agent or per message, sent only to models flagged `effort`; Anthropic models forced to `effort=false`; reasoning text is not surfaced in the stream. | works (Anthropic: none) | `crates/agent-native/src/lib.rs:621-624`, `crates/agent-native/src/models.rs:187-190`, `PLAN.md:513-517` | from own inventory |

Nothing documented: OpenCode (variants carry settings but no effort control is described), goose, OpenHands, OpenClaw, Hermes Agent, Cursor.

#### Speed tiers (fast mode)
What it is: a paid, faster serving configuration of the same model.
Variants: Claude Code and Codex both charge more (Codex counts 2.5x against included limits); both document separate rate limits or a fallback to standard speed.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/fast` or `fastMode`, up to 2.5x faster at higher per-token cost on a few models; separate cache (enabling mid-conversation re-reads context uncached); on limit falls back to standard speed with a grey icon; not on Bedrock/Vertex/Foundry; Opus 4.8 fast mode deprecated 2026-10-30, removal 2026-11-30. | GA (some models research preview) | https://code.claude.com/docs/en/fast-mode.md | reviewed |
| Codex | `/fast` or `service_tier = "fast"` costs 2.5x included limits (2x on credits); "Ultrafast" for Astra and Sol on eligible plans. The speed page still cites GPT-5.6, which looks stale. | GA | https://learn.chatgpt.com/docs/agent-configuration/speed.md | unreviewed |
| OpenCodex | `--fast` selectors request the provider's priority tier. | GA | https://opencodex.me/guides/model-routing/ | reviewed |
| Claude Desktop | Fast mode for Code sessions (Opus 4.8 fast mode deprecated, removal 2026-11-30). | GA | https://code.claude.com/docs/en/desktop | reviewed |
| Antigravity | Some Flash models show a "Fast" label. | GA | https://antigravity.google/docs/models | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | n/a | from own inventory |

#### Model fallback chains
What it is: automatically retrying a request on a different model when the chosen one fails or is unavailable.
Variants (this is where the harnesses differ most):
- **Trigger.** Claude Code: overload, unavailability and other non-retryable server errors, never auth, billing, rate-limit, request-size or transport errors. OpenCodex: 401/403/404/408/429/5xx hop errors; invalid requests and policy refusals stop; context overflow moves on without cooling. Hermes: 429/5xx after retries, but 401/403/404 immediately. Ours: 404/408/429/5xx/connect before any output.
- **Scope.** Claude Code: current turn only, up to three after dedupe. Hermes: once per turn. OpenClaw: next turns prefer a working fallback for up to 30 minutes after a rate-limit fallback. Oh My Pi: `fallbackRevertPolicy` cooldown-expiry or never.
- **Mid-stream.** OpenCodex and ours fail over only before any output. Claude Code's separate content-based fallback keeps the session on the fallback model.
- **What it will not fall back to.** Claude Code drops entries outside `availableModels` and will not fall back to a smaller-context model during compaction. OpenClaw never masks a user-chosen model. OpenCodex needs consent for metered targets and keeps tool calls on the target that issued them.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `--fallback-model a,b` or `fallbackModel` list; switch lasts for the current turn; also applies to sub-agents; first switch notice is the only visible sign (not in `/status`). Separate automatic content-based fallback: flagged cyber or biology requests re-run on a category-specific model, and the session stays there; `switchModelsOnFlag: false` prompts instead. | GA | https://code.claude.com/docs/en/model-config.md | reviewed |
| OpenCodex | `combo/<id>` virtual models over ordered targets; strategies failover (default), weighted round-robin with `stickyLimit`, random, least-used, reset-window, `jev`; hop errors cool the target; cooldown from `Retry-After` then `cooldownMs`; last-resort targets; failover only before output. `jev` fails open to the first eligible target. | GA | https://opencodex.me/guides/combos/ | reviewed |
| Hermes Agent | Turn-scoped `fallback_providers` chain; reset-aware cooldown; separate fallback chains for auxiliary tasks; switching invalidates the prompt cache. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers, https://hermes-agent.nousresearch.com/docs/integrations/providers | unreviewed |
| OpenClaw | Rotate auth profiles within a provider first, then `agents.defaults.model.fallbacks`; billing failures disable a credential 10 minutes; a user-chosen model is strict. | GA | https://docs.openclaw.ai/concepts/model-failover | unreviewed |
| Oh My Pi | `retry.fallbackChains` keyed by role, exact `provider/model`, `provider/*` or prefix; `fallbackRevertPolicy`; `retry.usageAwareFallback` (default false) moves between accounts and models using coding-plan quota reports, with confirm/auto/fail-closed policy. | GA | https://omp.sh/docs/roles | reviewed |
| Warp | Fallback routing switches to a comparable model during outages. | GA | https://docs.warp.dev/agents/inference/model-choice/ | unreviewed |
| Claude Desktop | Research uses Opus 5.5 with an Opus 4.8 fallback. | GA | https://support.claude.com/en/articles/11088861-use-research-on-claude | reviewed |
| OpenHands | SDK guides on model routing and fallback are listed in the docs index; the LLM routing/fallback page was not opened. | GA (index) | https://docs.openhands.dev/llms.txt | unverified |
| goose | No lead/worker or fallback documented; docs recommend routing through Tetrate or OpenRouter for failover. | unclear | https://goose-docs.ai/docs/guides/handling-llm-rate-limits-with-goose.md | unreviewed |
| rizzy-harness (ours) | Per-agent ordered fallbacks; tried when the model is unavailable or fails before any output with 404/408/429/5xx/connect; chat records `model_fallback`; not applied mid-stream. | works | `crates/agent-native/src/lib.rs:503-534`, `:640-701` | from own inventory |

Nothing documented: Codex, OpenCode, Cursor, Cline, Pi, DeepSeek harness, Antigravity, Zed.

#### Provider retry, backoff and timeouts
What it is: re-sending the same request to the same provider after a transient error, and how long to wait or give up.
Variants: Claude Code documents the fullest stack (10 retries with exponential backoff, an unattended mode that waits out 429/529 for hours, four streaming watchdogs). Pi's defaults are small and exact (3 retries, 2000 ms base, 60000 ms max; provider-level retries 0). Codex and OpenClaw key off provider headers. Most others document nothing.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Transient failures (5xx, overloaded, pre-output timeouts, dropped connections, temporary 429, stalled streams, retryable context-overflow with smaller `max_tokens`) retried up to 10 times with exponential backoff (`CLAUDE_CODE_MAX_RETRIES`, cap 15). `CLAUDE_CODE_RETRY_WATCHDOG=1` retries 429/529 indefinitely for unattended runs (about three hours for other retries). 529 backoff base/max 500 ms / 32000 ms. Four streaming idle watchdogs abort quiet streams. Gateway `retry-after` above 60 s stops retries; a gateway spend-limit 429 is not retried. OTel `api_error` fires once after retries are exhausted. | GA | https://code.claude.com/docs/en/errors.md, https://code.claude.com/docs/en/llm-gateway-protocol.md, https://code.claude.com/docs/en/monitoring-usage.md | unreviewed |
| Codex | Retries honour the server's `Retry-After`; a WebSocket transport falls back to HTTP; retrying stops when the provider reports the model was not found. | GA | https://learn.chatgpt.com/docs/changelog (0.161.0, 0.162.0, 0.165.0) | unreviewed |
| Pi | Agent-level retry on by default: 3 retries, 2000 ms base delay, 60000 ms max; provider-level retries default 0; HTTP idle timeout 300000 ms. | GA | https://pi.dev/docs/latest/settings (Retries and network) | reviewed |
| OpenCodex | Cooldown from `Retry-After` then `cooldownMs`; request log shows classified upstream failure reasons and retry counts. | GA | https://opencodex.me/guides/combos/, https://opencodex.me/guides/web-dashboard/ | reviewed / corrected |
| OpenClaw | Rate-limit cooldown follows provider headers; other errors back off 30 s, 1 min, 5 min cap; per-attempt timeout enforced by the run. | GA | https://docs.openclaw.ai/concepts/model-failover | unreviewed |
| Hermes Agent | Fallback fires on 429/5xx "after retries"; the retry policy itself is not described in the entry. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers | unreviewed |
| goose | Bedrock and Databricks have retry env vars; no generic provider retry documented (recipe `retry` is task-level, not provider-level). | unclear | https://goose-docs.ai/docs/guides/handling-llm-rate-limits-with-goose.md, https://goose-docs.ai/docs/guides/environment-variables.md | unreviewed |
| Oh My Pi | A `retry.*` settings namespace exists (hosts the fallback chains); backoff values not in the entries read. | GA | https://omp.sh/docs/roles | reviewed |
| rizzy-harness (ours) | None. A model-level failure before any output moves to the next fallback model; otherwise the turn errors. Tool timeout 600 s default; step cap 50 model calls per turn. | none (fallback only) | `crates/agent-native/src/lib.rs:693-701`, `crates/agent-native/src/provider.rs:104-116`, `crates/agent-native/src/lib.rs:68`, `:33` | from own inventory |

Nothing documented: OpenCode, Cursor, Cline (only a local-request 5-minute fix), DeepSeek harness, Antigravity, Warp, Zed, OpenHands.

#### Routing across providers and credential pools
What it is: spreading requests over several providers, accounts or keys by rules, quota or cost.
Variants: a gateway-style router (OpenCodex combos, Claude Code's apps gateway upstream list) versus in-client pools (Hermes, OpenClaw, Oh My Pi) versus classifier routers that pick a model per task (Cursor Router, Warp custom routers). Claude Code's gateway documents "no circuit breaker". Anthropic and OpenCodex both warn that cross-provider failover or pooling changes the governing agreement or risks account action.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Fixed first-match routing order (account selector, combo id/alias, explicit `provider/model`, bare OpenAI-family ids, provider `defaultModel`, built-in prefixes, provider `models[]`, `defaultProvider`). ChatGPT account pool: quota-aware (5h, weekly, 30d windows), per-account selection order, lowest-usage choice for new sessions, thread affinity (default on, keeps caches warm), cooldown after 429, reauth flag after 401/403, manual pin. Pooling gives no protection from provider enforcement. | GA | https://opencodex.me/guides/model-routing/, https://opencodex.me/guides/web-dashboard/, https://opencodex.me/getting-started/how-it-works/ | reviewed (routing) / corrected (pool) |
| Claude Code | Apps gateway (in the `claude` binary): ordered `upstreams` (anthropic, bedrock, mantle, anthropicAws, vertex, foundry) with failover on 5xx, 429, 401, 403, 404, 501 and timeout; other 4xx returned; no circuit breaker; model-ID translation per upstream; per-user identity headers. Docs warn failover across providers changes the governing agreement and geography. | GA | https://code.claude.com/docs/en/claude-apps-gateway-config.md, https://code.claude.com/docs/en/claude-apps-gateway.md | unreviewed |
| Hermes Agent | Credential pools rotate keys for one provider before cross-provider fallback; `provider_routing` for OpenRouter. Pool details page not opened. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers | unreviewed |
| OpenClaw | Auth-profile order: per-agent override, `auth.order`, configured, stored; OAuth before keys; least-recently-used first; profile pinned per session for cache warmth. | GA | https://docs.openclaw.ai/concepts/model-failover | unreviewed |
| Oh My Pi | README claims round-robin credentials with session affinity and per-credential backoff; docs/providers does not describe round-robin; `omp dry-balance` simulates OAuth account selection; usage-aware fallback. | GA (README claim) | https://omp.sh/docs/providers, https://omp.sh/docs/roles | reviewed |
| Cursor | Auto has Cost, Balance and Intelligence modes; Balance and Intelligence use a classifier router (Cursor Router), Teams and Enterprise only, off by default on Enterprise. | GA (no label) | https://cursor.com/docs/cursor-router, https://cursor.com/docs/models-and-pricing | unreviewed |
| Warp | Auto models (Responsive, Cost-efficient, Genius, Open-weights); custom model routers as YAML in `~/.warp/custom_model_routers/` choosing a model by complexity (easy, medium, hard) or ordered natural-language rules; targets must be Warp-supported models. | GA | https://docs.warp.dev/agents/inference/model-choice/, https://docs.warp.dev/agents/inference/custom-routers/ | unreviewed |
| OpenHands | Routing by model-string prefix; Enterprise runs a managed LiteLLM gateway. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/llm-profiles.md, https://docs.openhands.dev/enterprise/architecture.md | unreviewed |
| OpenCode | Copilot "Auto" routing (v2.2.7); web-search provider `"random"` with 429 cooldown failover. | GA | https://opencode.ai/changelog, https://opencode.ai/docs/websearch/ | unreviewed |
| rizzy-harness (ours) | Routing by provider prefix only; selection order and fallbacks cover the rest. No pools, rotation or quota-aware choice in the inventory. | partial | `crates/agent-native/src/llm.rs:326-380` | from own inventory |

Nothing documented: Codex, Antigravity, DeepSeek harness, Pi, Zed, goose (docs point to external routers).

#### Model roles for auxiliary work
What it is: using a different, usually cheaper or stronger, model for side tasks (titles, compaction, summaries, classification, approval review, advice).
Variants: Oh My Pi and Hermes make this a first-class, per-task configuration; Claude Code and Zed expose a handful of named slots; OpenCodex uses sidecar models to give weaker models vision and search.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | Roles `smol`, `slow`, `vision`, `plan`, `designer`, `commit`, `tiny`, `task`, `advisor`; `models.yml` has `compactionModel` and `contextPromotionTarget`; the advisor role will not inherit default. | GA | https://omp.sh/docs/roles, https://omp.sh/docs/custom-models | reviewed |
| Hermes Agent | Compression, vision, summaries, goal judge, curator, background review and smart approval can each be routed to a different provider and model, defaulting to the main model. | GA | https://hermes-agent.nousresearch.com/docs/integrations/providers, https://hermes-agent.nousresearch.com/docs/user-guide/features/goals | unreviewed |
| Claude Code | Auto-mode classifier defaults to Sonnet 5 (`CLAUDE_CODE_AUTO_MODE_MODEL` overrides); background tasks such as titles use a small/fast model (Haiku-class; on Bedrock and Agent Platform the default Sonnet); `ANTHROPIC_DEFAULT_HAIKU_MODEL` pins it. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/amazon-bedrock.md | unreviewed |
| Claude Code (advisor) | Server-side advisor tool: a stronger model consulted mid-task with the full conversation; must rank at or above the main model; Anthropic API only; billed at the advisor's rates. | experimental | https://code.claude.com/docs/en/advisor.md | reviewed |
| Zed/ACP | `inline_assistant_model`, `commit_message_model`, `thread_summary_model`, `compaction_model`, `subagent_model`. | GA | https://zed.dev/docs/ai/agent-settings.md | reviewed (compaction, sub-agent) |
| OpenCodex | Vision sidecar (a model describes each image for text-only models; three at a time, 8 per turn default) and a web-search sidecar that gives any model a search tool. | GA | https://opencodex.me/guides/sidecars/ | reviewed |
| OpenClaw | Advisor model reviews work and may send one correction; "decision assistance" needs a Decision model; both off by default. | experimental | https://docs.openclaw.ai/concepts/experimental-features | unreviewed |
| Codex | Auto-review reviewer uses extra model calls; memory extract and consolidation models configurable. | GA | https://learn.chatgpt.com/docs/sandboxing/auto-review.md | reviewed |
| OpenCode | `agents.title.model` replaces `small_model`. | GA | https://opencode.ai/docs/config/ | unreviewed |
| Pi | Non-chat model types (classifiers, image generators) hidden from `/model`, reachable through `codemode` or the model registry. | GA | https://pi.dev/docs/latest/models, https://pi.dev/docs/latest/codemode | unreviewed |
| Warp | Separate planning model per profile. | GA | https://docs.warp.dev/agents/inference/model-choice/ | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | n/a | from own inventory |

#### Vision and other modalities
What it is: image input, image generation, and handling for models that lack vision.
Variants: ours and Pi document concrete limits (ours 4 images, 5 MB; Pi auto-resize 2000x2000 px, 4.5 MiB). OpenCodex describes images for text-only models with a sidecar. Several harnesses generate images.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Vision sidecar for models marked text-only; default backend Anthropic if an OAuth account is usable, else OpenAI; `data:` descriptions cached. Image and video bridge guides exist but were not opened. | GA; bridges unverified | https://opencodex.me/guides/sidecars/, https://opencodex.me/guides/image-bridge/ | reviewed (sidecar) / unverified (bridges) |
| Oh My Pi | `generate_image` (Gemini, GPT, Grok), `inspect_image` via the vision role; both generation and TTS off by default. | GA | https://omp.sh/docs/tools | unreviewed |
| Pi | Image inputs auto-resized (defaults 2000x2000 px, 4.5 MiB encoded, JPEG quality 80); image generators as hidden model types. | GA | https://pi.dev/docs/latest/models, https://pi.dev/docs/latest/settings | unreviewed |
| OpenCode | `experimental_image_generate` creates or edits images when a provider has an image model; runs in the background (enabled by default when supported, v2.2.2). | experimental | https://opencode.ai/docs/tools/, https://opencode.ai/changelog | unreviewed |
| Cursor | Read-file tool reads images for vision models; image-generation tool saves to `assets/`. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| Codex | GPT-6.1 Terra described as fast with vision and computer use. | GA | https://learn.chatgpt.com/docs/models.md | corrected |
| Zed/ACP | Images for vision models via mentions; ACP prompt capabilities cover image and audio. | GA | https://zed.dev/docs/ai/agent-panel, https://agentclientprotocol.com/protocol/v1/overview.md | unreviewed |
| Hermes Agent | Clipboard image paste; aux vision model; Nous Tool Gateway offers image generation. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/overview, https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-gateway | unreviewed |
| DeepSeek harness | Vision via an Image input type on a custom model. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers | unreviewed |
| rizzy-harness (ours) | Up to 4 images (PNG/JPEG/WebP/GIF, 5 MB) per message for models with `vision`; browser downscales; stored per chat. No image generation in the inventory. | works | `crates/protocol/src/lib.rs:24-36`, `crates/agent-native/src/images.rs`, `crates/node/src/images.rs` | from own inventory |

Nothing documented: Claude Code, Antigravity, goose, OpenClaw (beyond nodes with camera), Warp beyond image paste in the CLI.

#### Voice (dictation, text-to-speech, realtime)
What it is: speech in or out of the agent. Three distinct mechanisms: dictation into the prompt, speech output, and realtime voice conversation.
Variants: Claude Code's dictation streams audio to Anthropic and requires a claude.ai login (not usable with API keys or cloud providers). Ours sends audio to a configured OpenAI-style transcription URL. OpenClaw and Hermes are the broadest (TTS providers, wake words, realtime). Voice is marked experimental or rolling out in Codex, Antigravity and DeepSeek harness.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/voice [hold\|tap\|off]` dictation; audio streams to Anthropic servers; needs claude.ai sign-in (not API key, Bedrock, Vertex, Foundry) and a local microphone; 20 languages; recording stops after 15 s silence or 2 min; transcription does not consume tokens. | GA | https://code.claude.com/docs/en/voice-dictation.md | unreviewed |
| Claude Desktop | Dictation on macOS and Windows, not Linux. | GA | https://code.claude.com/docs/en/desktop-linux | reviewed (Linux limit) |
| Codex | `/voice` in the TUI; Codex Micro hardware key with voice; voice pages not read; OpenCodex proxies `/v1/live` and `/v1/realtime/calls`. | GA (voice experimental) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/features/codex-micro.md, https://opencodex.me/reference/architecture/ | unreviewed |
| Antigravity | App Gemini Live voice mode, live dictation Ctrl+M; CLI `/voice live`, `/voice dictate`, `agy mic-serve` over SSH; dictation unavailable for business and enterprise accounts. | GA (rolling out v2.29.0 to 2.30.0) | https://antigravity.google/docs/features, https://antigravity.google/docs/cli/using | unreviewed |
| Cursor | Voice Chat (realtime, 2026-11-06; dictation was earlier "Voice Mode"); Siri voice on iOS. | GA (no label) | https://cursor.com/changelog, https://cursor.com/docs/cloud-agent/mobile | unreviewed |
| goose | Desktop dictation with a local provider or ElevenLabs, Groq, OpenAI; v1.59.0 mentions live voice. | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md, https://github.com/aaif-goose/goose/releases | unreviewed |
| Hermes Agent | Voice mode in CLI, Telegram, Discord voice channels; wake word "Hey Hermes"; TTS with ten provider options; voice-memo transcription. Dedicated voice pages not opened. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/overview, https://hermes-agent.nousresearch.com/docs/user-guide/messaging | unreviewed |
| OpenClaw | Talk mode loop (platform speech recognition, TTS via ElevenLabs, local MLX or system); realtime variants (OpenAI WebRTC, Google, Gateway-relay); 8 concurrent GPT-Live sessions, 30-min TTL; Gateway-owned wake words. | GA | https://docs.openclaw.ai/nodes/talk, https://docs.openclaw.ai/nodes/voicewake | unreviewed |
| Oh My Pi | `tts` (xAI voice, local or provider), `/live` realtime voice, `omp say`. | GA (TTS off by default) | https://omp.sh/docs/tools, https://omp.sh/docs/slash | unreviewed |
| OpenHands | Voice input: browser speech recognition or an OpenAI-compatible transcription endpoint. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| DeepSeek harness | Experimental speech-to-text providers (including SenseVoice). | experimental | https://deepseek.com/harness | unreviewed |
| Cline | Desktop voice input via a provider such as ElevenLabs. | GA | https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| Warp | Voice input in the app and CLI (Ctrl+S, `/voice`); not in the official Linux build. | GA | https://docs.warp.dev/agents/cli/models-and-usage/ | unreviewed |
| OpenCode | A Hex voice-dictation product exists (not surveyed); the HTTP API has a `voice` group. | unclear | https://opencode.ai/product, https://opencode.ai/docs/api/ | unverified |
| rizzy-harness (ours) | Browser mic recording; master forwards audio to a configured OpenAI-style `/audio/transcriptions` endpoint; 25 MB cap; not stored. No TTS or realtime voice. | works (input only) | `crates/master/src/voice.rs:1-40`, `web/src/components/Composer.tsx:160-170` | from own inventory |

Nothing documented: Pi, Zed.

#### Hosted model offerings (commercial services)
What it is: model access sold by the harness vendor itself. **Everything in this section is a commercial service, not a harness feature.** Prices are as stated on vendor pages on 2026-11-10 and change often.
Variants: pay-as-you-go credits (OpenCode Console), flat subscriptions with rolling usage windows (Go, ClinePass, Cursor, Warp), pass-through at list price plus a margin (Zed), and platform bundles (Nous Portal).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode (Console, commercial) | Pay-as-you-go catalogue (formerly "Zen") billed per request against credits with a 4.6% + $0.31 purchase fee; Go subscription ($10/month, Go Plus $40, Go Teams $20 per member) with per-model dollar allowances in 5-hour, weekly and monthly windows; time-limited free models (some train on data; hide with `OPENCODE_DISABLE_FREE_MODELS=1`); other harnesses can use the models through `/inference` base URLs. "100% free" on the product page is a vendor claim. | GA (commercial) | https://opencode.ai/docs/console/, https://opencode.ai/docs/console/models/, https://opencode.ai/docs/console/go/, https://opencode.ai/docs/console/harnesses/ | reviewed (budgets, Go prices) |
| Cline (ClinePass, commercial) | $9.99/month for open-weight coding models, "2-5x" usage of API rates (vendor claim); limits over 5-hour, weekly and monthly windows with exact limits unpublished; free models rotate. | GA | https://docs.cline.bot/getting-started/clinepass.md, https://docs.cline.bot/cline-overview.md | unreviewed |
| Hermes Agent (Nous, commercial) | Nous Portal subscription; paid Nous Tool Gateway routes web search, image generation, TTS, cloud browser and connectors through Nous infrastructure with one bill; tools stop working if the subscription lapses; privacy of gateway traffic not covered. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-gateway | unreviewed |
| Zed (commercial) | Zed-hosted models billed through Zed: Pro includes a monthly $5 token credit, usage at provider list price plus 10%, default $10 extra-spend limit; Anthropic "Covered Models" retained at least 30 days for safety review. Hosted-model list not opened. | GA | https://zed.dev/docs/account/plans-and-pricing.md, https://zed.dev/docs/ai/privacy-and-security.md | unreviewed |
| Cursor (commercial) | Cursor-labelled models (Composer 2.5, Grok) draw on a weekly pool; Premium $30 adds Cloud Agents and third-party models; Max Mode (legacy plans only) billed at API rate plus 20%. | GA | https://cursor.com/docs/models-and-pricing | reviewed (plans) |
| Warp (commercial) | Auto models always use Warp inference; plans Free, Build, Max, Business, Enterprise; platform usage $3 per agent hour on dollar-billed tiers; hosted compute metered per minute. | GA | https://docs.warp.dev/support-and-community/plans-and-billing/ | unreviewed |
| Antigravity (commercial) | Models gated by Free, Plus, Pro, Ultra, Enterprise; weekly and five-hour limits per model family; AI Credits overage setting; no prices stated. | GA | https://antigravity.google/docs/plans, https://antigravity.google/docs/models | unreviewed |
| Codex (commercial) | Included in ChatGPT plans, sharing usage and credits with ChatGPT Work; API-key use billed at API rates. | GA | https://learn.chatgpt.com/docs/pricing.md | unreviewed |
| Claude Code / Claude Desktop (commercial) | Teams/Enterprise seats with 5-hour and weekly allowances, Console pay-as-you-go, usage credits; Fable models may bill to usage credits with a consent prompt (unattended contexts wait `dialogExpiry`, `-p` bills without asking). | GA | https://code.claude.com/docs/en/costs.md, https://code.claude.com/docs/en/model-config.md | unreviewed |
| OpenHands (commercial) | OpenHands Cloud and Enterprise: managed LiteLLM gateway with budgets; open-source tier lacks auth, RBAC, budgets. Pricing not listed on docs pages. | GA | https://docs.openhands.dev/enterprise/enterprise-vs-oss.md | reviewed |
| rizzy-harness (ours) | None. | none | n/a | from own inventory |

#### Model catalogue, capability discovery and pinning
What it is: how the harness learns what each model can do (context, vision, tools, effort, price) and how users freeze or relabel models.
Variants: ours reads `/models` and Ollama `/api/show`; Claude Code infers capability by model-ID pattern unless declared; OpenCodex rewrites another client's catalogue file; Oh My Pi and DeepSeek harness discover from the provider.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| rizzy-harness (ours) | Context, max output, vision, tools, effort read from `/models` (Ollama `/api/show`); daily refresh; vanished models marked `unavailable` and kept. | works | `crates/agent-native/src/models.rs:1-30`, `crates/master/src/model_cache.rs:1-60` | from own inventory |
| Claude Code | `modelOverrides` maps Anthropic IDs to provider strings; `modelPicker` custom lineup (user/managed settings only, not project); `ANTHROPIC_DEFAULT_*_SUPPORTED_CAPABILITIES` declares capability; optional gateway discovery (`GET /v1/models`, 3 s timeout, redirects treated as failure so credentials cannot leak, cached on disk); capability otherwise by model-ID pattern. | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/llm-gateway-protocol.md | unreviewed |
| Codex | `model_catalog_json` custom catalogue (profile can override); `model/list` and `modelProvider/capabilities/read` over the API. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md, https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCodex | `disabledModels`/`selectedModels` filtering, new-model policy (default off, marked NEW), per-provider context caps; rewrites Codex's `model_catalog_json` to steer the client without patching it. | GA | https://opencodex.me/reference/configuration/, https://opencodex.me/guides/model-routing/ | reviewed |
| OpenCode | Catalogue from models.dev. | GA | https://opencode.ai/docs/providers/ | unreviewed |
| DeepSeek harness | Manual model list or `GET /models`; `reasoningEfforts` declared per model. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers | unreviewed |
| Oh My Pi | Per-model reasoning, cost, context, tokenizer; discovery for ollama, llama.cpp, lm-studio, openai-models-list, litellm, proxy. | GA | https://omp.sh/docs/custom-models | unreviewed |
| Pi | `modelOverrides`; `--list-models`. | GA | https://pi.dev/docs/latest/models | unreviewed |

#### Model and provider governance (allowlists, locks)
What it is: admin controls over which models, providers or effort levels users may use.
Variants: Claude Code has the most detailed lock set (managed-only, prefix vs exact, denylist); OpenCodex scopes models per client key; Cursor and Warp gate models by plan or admin switch.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `availableModels` (prefix match by default), `availableModelsMatch: "exact"`, `deniedModels` (both managed-only), `enforceAvailableModels`, `maxEffortLevel`; `allowedProviders` (managed) refuses sessions on unlisted providers at startup, login and next API contact, and endpoint values must be pinned in managed env; server and machine lists intersect. | GA | https://code.claude.com/docs/en/settings-reference.md, https://code.claude.com/docs/en/managed-settings.md | reviewed (model locks) / unreviewed (providers) |
| OpenCodex | Per-client `allowedModels` on the remote hub (403 when out of scope); usage scoped by `apiKeyId`. | GA | https://opencodex.me/guides/remote-hub/ | corrected |
| OpenClaw | Named gateway roles under `gateway.roles.definitions` can set a model policy and allowed agents; scopes are a guardrail inside one trust domain, not tenant isolation. | GA | https://docs.openclaw.ai/gateway/operator-scopes | reviewed |
| Cursor | Some models lack ZDR and are off by default for Enterprise; Fable models need admin approval for Enterprise/Privacy Mode customers; Router modes off by default on Enterprise. | GA | https://cursor.com/docs/enterprise/privacy-and-data-governance, https://cursor.com/docs/cursor-router | reviewed |
| Antigravity | Third-party Claude requires admin opt-in on Enterprise. | GA | https://antigravity.google/docs/enterprise | unreviewed |
| OpenCode (Console) | Policies pushed to every connected client. | GA (commercial) | https://opencode.ai/docs/policies/ | reviewed |
| rizzy-harness (ours) | Per-workspace default overrides; no allowlist, denylist or effort cap in the inventory. | partial | `crates/master/src/lib.rs:951-1022` | from own inventory |

#### Prompt-cache controls and cache-aware behaviour
What it is: settings and design choices that keep provider prompt caches warm (a cost lever).
Variants: Claude Code exposes toggles, TTL and miss attribution; OpenCode, OpenClaw and OpenCodex pin sessions to keep caches warm; Hermes notes that switching provider invalidates the cache.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `DISABLE_PROMPT_CACHING` globally or per tier; `ENABLE_PROMPT_CACHING_1H` on cloud providers; `/usage` shows hit ratio, misses and likely cause (main conversation only); idle compaction timed before cache expiry. | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/prompt-caching.md | reviewed (`/usage` line) / unreviewed (toggles) |
| OpenCode | `settings.cache.ttl` per provider/model; cache warming, keyed session instructions, summary requests that reuse the main prompt cache. | GA | https://opencode.ai/docs/models/, https://opencode.ai/docs/config/ | unreviewed |
| OpenClaw | Auth profile pinned per session for cache warmth. | GA | https://docs.openclaw.ai/concepts/model-failover | unreviewed |
| OpenCodex | Thread affinity (default on) keeps a conversation on one account. | GA | https://opencodex.me/guides/web-dashboard/ | corrected |
| Hermes Agent | Switching provider on fallback invalidates the prompt cache (noted). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers | unreviewed |
| rizzy-harness (ours) | Prompt-cache markers; cache hit tokens recorded; observation masking in batches to keep the cache valid. | works | `crates/memory/src/embed.rs:37-60`, `crates/agent-native/src/mask.rs:15-60` | from own inventory |

### Area 19: observability (usage and cost, logs, tracing, debugging)

#### Token and cost display
What it is: showing the user how many tokens and how much money a turn, session or run used.
Variants: most cost figures are local estimates from a price table, not billing (Claude Code, OpenCodex, Pi's "billed vs estimated"); OpenClaw instead shows provider-reported quota windows and spend and hides per-token dollars for subscription sessions. Ours stops at tokens.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/usage` (aliases `/cost`, `/stats`): session cost computed locally from token counts at list price (or at `modelPricing` rates), API and wall duration, lines changed, per-model input/output/cache tokens; resets on `/clear`; a spinner shows an approximate live output-token count; SDK results carry `total_cost_usd` and per-model `modelUsage` with a `costBasis` field. Docs say explicitly not to bill end users from these fields. | GA | https://code.claude.com/docs/en/costs.md, https://code.claude.com/docs/en/agent-sdk/cost-tracking.md | reviewed (`/usage`) / unreviewed (SDK) |
| Codex | `/status` shows model, policies, writable roots and token use; `/usage` shows account usage; the app-server exposes rate limits and token usage. No cost in currency described. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | `opencode stats --days --models --cost --project --json`; API `usage/limits` for Go allowance and Console budgets. | GA | https://opencode.ai/docs/cli/commands/, https://opencode.ai/docs/troubleshooting/ | unreviewed |
| OpenCodex | Usage & Logs page: per-app token share, estimated cost (API list-price equivalent, not billing), cache hit/write rates, tokens per second, coverage warnings. | GA | https://opencodex.me/guides/web-dashboard/, https://opencodex.me/reference/architecture/ | corrected |
| Cline | TUI status area shows model, context usage and cost; SDK run results carry tokens and `totalCost`; the model catalogue carries input/output/cache pricing. | GA | https://docs.cline.bot/usage/tui.md, https://docs.cline.bot/sdk/model-providers.md | unreviewed |
| Pi | Footer shows context usage and cost; `/session` shows token usage and cost; session files include `usage` entries; pi-ai reports billed vs estimated cost. | GA | https://pi.dev/docs/latest/usage, https://pi.dev/docs/latest/sessions, https://github.com/earendil-works/pi/blob/main/packages/ai/README.md | unreviewed |
| Oh My Pi | Status line shows cost; Agent Hub shows per-worker cost; `/usage` shows provider limits and resets. | GA | https://omp.sh/docs/slash, https://omp.sh/docs/cli | unreviewed |
| Warp | Per-conversation usage popover (changelog 2026.09.23), `/usage` and `/cost` in the CLI, statusline model and usage; cost attached to the run record. | GA | https://docs.warp.dev/agents/cli/models-and-usage/, https://docs.warp.dev/platform/managing-cloud-agents/ | unreviewed |
| Hermes Agent | Status bar shows cost and context; `/usage`, `/insights`. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/cli | unreviewed |
| OpenClaw | Provider-reported quota windows and spend, not estimated billing; `/usage off\|tokens\|full\|cost`, `openclaw status --usage`, Control UI Usage page; subscription sessions hide per-token dollars; Anthropic and OpenAI admin keys add organisation cost history. | GA | https://docs.openclaw.ai/concepts/usage-tracking | unreviewed |
| OpenHands | Per-conversation budget meters and token usage in Canvas (1.38.0); automation run cost in USD. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/managing-automations.md, https://docs.openhands.dev/sdk/guides/observability.md | unreviewed |
| goose | Token usage indicators (desktop dot, CLI colours) and `/status` (session id, model, provider, mode, tokens); per-model prices can be set on custom providers; no cost dashboard documented. | unclear | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md, https://goose-docs.ai/docs/getting-started/providers.md | unreviewed |
| Antigravity | Token usage in headless JSON and SDK `usage_metadata`; quotas in `/usage`; no prices stated. | GA | https://antigravity.google/docs/cli/headless, https://antigravity.google/docs/cli/reference | unreviewed |
| Zed/ACP | Token usage shown in the panel near the profile selector; cost dashboard at dashboard.zed.dev. ACP lists "end-turn token usage" as an RFD and already has usage updates in v1 (titles only). | GA | https://zed.dev/docs/ai/agent-panel, https://zed.dev/docs/account/plans-and-pricing.md | unreviewed |
| Cursor | Usage per run and in total via the API. Details beyond index titles not opened. | GA (no label) | https://cursor.com/docs/cloud-agent/api/endpoints | unreviewed |
| Claude Desktop | Settings > Usage card appears in 3P builds; Cowork OTel events carry cost, tokens and cache. | GA | https://claude.com/docs/third-party/claude-desktop/overview.md, https://claude.com/docs/cowork/monitoring.md | reviewed (OTel) / unreviewed (card) |
| rizzy-harness (ours) | Input, output, cache read and cache write tokens summed per turn into `turn_ended.usage`; UI tooltip total per chat; routine runs record tokens. No currency cost anywhere. | partial | `crates/agent-native/src/llm.rs:246-253`, `web/src/components/Conversation.tsx:293-297` | from own inventory |

Nothing documented or unverified: DeepSeek harness (the survey lists "token cost display and usage UI" as unknown; a `token-meter` subsystem page exists but was not read).

#### Budgets and limits
What it is: caps that stop or warn on spend, tokens or time, set by the user or an admin, as distinct from plan quotas set by a vendor.
Variants: three enforcement points exist. Per request, before it starts (OpenCode Console returns HTTP 429; Claude Code's gateway returns 429 `billing_error` with `x-should-retry: false`). After each model response (Claude Code `maxBudgetUsd` can overshoot by one response plus in-flight sub-agent spend). Inside a goal loop (Codex goal token budget, Hermes 20 continuation turns). Plan quotas (Antigravity, ClinePass, Cursor pools) are vendor limits, listed for contrast.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code (SDK/CLI) | `maxBudgetUsd` / `--max-budget-usd` and `maxTurns`, unlimited by default; budget includes sub-agent spend; at the cap spawning a sub-agent fails with `Budget limit reached` and background sub-agents are stopped; checked after each response so it can overshoot; docs recommend a budget as the production default. A data-residency 1.1x multiplier and `modelPricing` apply to the figure. | GA | https://code.claude.com/docs/en/agent-sdk/agent-loop.md, https://code.claude.com/docs/en/costs.md | reviewed (sub-agent caps) / unreviewed |
| Claude Code (apps gateway) | `admin:` block serves `/v1/organizations/spend_limits`; caps `{scope, amount, period}` with scope user, rbac_group or organization, USD cents, periods daily/weekly/monthly (UTC reset); resolution: per-user override, most restrictive group cap, org default; `pooled_budget` shared monthly group pool; over-cap returns 429 `billing_error`, `x-should-retry: false`, `retry-after`; `count_tokens` never blocked; spend-limit bar for gateway users. | GA | https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md, https://code.claude.com/docs/en/claude-apps-gateway-config.md | unreviewed |
| Codex | `/goal` with token budget: app-server `thread/goal/set` carries `tokenBudget`, `tokensUsed`, `timeUsedSeconds`; `goals.max_goal_token_budget` caps it; persisted thread state with automatic continuation. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/long-running-work.md | reviewed |
| OpenCode (Console, commercial) | Per-member, tagged and workspace budgets in USD resetting monthly 00:00 UTC, enforced as HTTP 429 at request start; Go allowance windows (5-hour 20%, weekly 50%, monthly). | GA (commercial) | https://opencode.ai/docs/console/budgets/, https://opencode.ai/docs/console/go/ | reviewed |
| OpenHands (Enterprise, commercial) | Managed LiteLLM gateway: monthly org budget, default user budget, per-user overrides, reset on the 1st or 15th, alerts at 80/90/100% for the org budget only. Open-source tier has no budgets. | GA (Enterprise) | https://docs.openhands.dev/openhands/usage/cloud/organizations/budgets.md, https://docs.openhands.dev/enterprise/enterprise-vs-oss.md | reviewed (OSS gap) |
| Zed (commercial) | Org spend limit on Business; Pro default $10 extra-spend limit. | GA | https://zed.dev/docs/account/plans-and-pricing.md | unreviewed |
| Hermes Agent | Iteration budget (default 500, `agent.max_turns`), sub-agents get their own; goal loop default 20 continuation turns then auto-pause. The distinctive-features section also mentions "spend limits" in the approval layer, but no entry describes them. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop, https://hermes-agent.nousresearch.com/docs/user-guide/features/goals | reviewed (iteration budget) / unverified (spend limits) |
| OpenCodex | Quota chip bar (amber at 70%, red at 90%); `reset-window` strategy picks the target whose quota resets soonest; metered-target consent for combos. No user budgets. | GA | https://opencodex.me/guides/web-dashboard/, https://opencodex.me/guides/combos/ | reviewed |
| Oh My Pi | `retry.usageAwareFallback` keeps a reserve percentage of plan quota with confirm/auto/fail-closed policy. | GA | https://omp.sh/docs/roles | reviewed |
| Cursor | Plan pools (weekly pool for Cursor models); Enterprise pooled usage and Team Pools. | GA | https://cursor.com/docs/models-and-pricing | reviewed |
| Antigravity | Weekly and five-hour limits per model family; AI Credits overage setting ("Never"/"Always"). | GA | https://antigravity.google/docs/plans, https://antigravity.google/docs/models | unreviewed |
| Warp | Plan usage per seat (monthly, no rollover); purchased usage rolls over 12 months; Warp-credit fallback for BYOK off by default. | GA | https://docs.warp.dev/support-and-community/plans-and-billing/ | unreviewed |
| Cline | ClinePass limits over 5-hour, weekly and monthly windows; exact limits unpublished. | GA | https://docs.cline.bot/getting-started/clinepass.md | unreviewed |
| rizzy-harness (ours) | None for the main agent: only sub-agent caps (count and tokens), a hard 50-model-call step cap per turn, routine run caps and storage quotas. The agent field `budget: lean/standard` is parsed but nothing reads it. No per-user cost or usage limits. | none (stub) | `crates/core/src/agent.rs:34-40`, `:102`, `crates/agent-native/src/lib.rs:33`, `:650` | from own inventory |

Nothing documented: Pi, goose, DeepSeek harness (only `maxGoalRounds` on goals), OpenClaw, Claude Desktop.

#### Usage dashboards and quota views
What it is: an aggregated view across sessions, users or accounts, in the product or via API.
Variants: Claude Code and Warp offer org analytics APIs; OpenCodex and Hermes ship a local dashboard; Oh My Pi's `omp stats` dashboard has no docs page (404), so its contents are unverified.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/usage` plan bars with attribution by skill, sub-agent, plugin and MCP server; Teams/Enterprise dashboard (adoption, accept rate, PR attribution, CSV); Console dashboard; Enterprise Analytics and Claude Code Analytics APIs; Code Review analytics with estimated cost (list price, can differ from invoice). Contribution metrics are public beta. | GA (contribution metrics beta) | https://code.claude.com/docs/en/analytics.md, https://code.claude.com/docs/en/costs.md, https://code.claude.com/docs/en/code-review-costs.md | reviewed (`/usage`) / unreviewed |
| OpenCodex | Eight-page local dashboard including Usage & Logs, usage scoped by `apiKeyId`; public opt-out usage dashboard at opencodex.me fed by telemetry (release notes). | GA | https://opencodex.me/guides/web-dashboard/, https://opencodex.me/guides/remote-hub/ | corrected |
| OpenCode | `opencode stats`; Console usage breakdown with CSV export. | GA | https://opencode.ai/docs/cli/commands/, https://opencode.ai/docs/console/ | unreviewed |
| Oh My Pi | `/stats` and `omp stats` launch a local usage dashboard (`omp-stats` per README); `omp bench` measures latency and throughput. No docs page; contents unverified. | GA (README) | https://omp.sh/docs/slash, https://omp.sh/docs/cli | unverified |
| Hermes Agent | Dashboard Analytics page; `/insights`; curator reports. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/web-dashboard | unreviewed |
| Warp | Enterprise Analytics API (summary, users, events; 365-day events); Oz Runs page; Factories dashboard. | GA (Enterprise API) | https://docs.warp.dev/enterprise/enterprise-features/analytics-api/, https://docs.warp.dev/platform/managing-cloud-agents/ | unreviewed |
| Cursor | Analytics API (agent edits, MCP/skills/plan adoption, Bugbot). | GA | https://cursor.com/docs/cloud-agent/api/endpoints | unreviewed |
| Zed | Billing dashboard at dashboard.zed.dev. | GA | https://zed.dev/docs/account/plans-and-pricing.md | unreviewed |
| OpenHands | Enterprise Conversation Metrics cookbook and analytics page. | GA (Enterprise) | https://docs.openhands.dev/enterprise/architecture.md | unreviewed |
| OpenClaw | Control UI Usage page. | GA | https://docs.openclaw.ai/concepts/usage-tracking | unreviewed |
| Codex | `/usage` and app-server rate limits; no dashboard described. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Antigravity | `/usage` quota view per model family. | GA | https://antigravity.google/docs/models | unreviewed |
| rizzy-harness (ours) | No per-user or per-model dashboard. | none | grep finds none (own inventory) | from own inventory |

#### OpenTelemetry and other export
What it is: sending traces, metrics or events to an external collector or analytics service.
Variants: **content defaults** differ: Codex redacts prompts by default and caps tool results at 2,048 bytes; goose adds message content only when `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` is set; DeepSeek harness exports everything unless a listener redacts; Claude Desktop captures metadata only by default. **Who controls the destination**: Codex and Claude Code both stop repository config from setting it; Claude Code managed settings strip conflicting developer endpoints. **Signals**: Claude Code and Codex cover metrics, logs and traces; OpenHands, goose and OpenClaw lead with traces.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Off by default; OTLP HTTP or gRPC for logs, traces and metrics with TLS client certificates; events cover API requests, SSE and WebSocket events, prompts, tool decisions and results; prompts redacted by default; `log_agent_responses` opt-in; project config cannot set `otel`. Built-in Statsig metrics follow `analytics.enabled`. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed |
| Claude Code | Opt-in (`CLAUDE_CODE_ENABLE_TELEMETRY=1`); metrics (cost, tokens, sessions, tool decisions), events (including `api_request`, `api_error`, `api_retries_exhausted`, `tool_decision`, `safety_event`, opt-in request/response bodies) and traces (beta: `interaction` > `llm_request` / `tool`, W3C `traceparent` injected into subprocesses and sent on model requests). Repo settings cannot enable telemetry or choose a destination; managed settings lock the OTLP destination and remove conflicting developer variables. Claude apps gateway can relay OTLP (no buffering or retry). Self-hosted runner exposes Prometheus `/metrics` (beta). Does not set `gen_ai.usage.*`. | GA (traces beta; runner metrics beta) | https://code.claude.com/docs/en/monitoring-usage.md, https://code.claude.com/docs/en/claude-apps-gateway-config.md, https://code.claude.com/docs/en/self-hosted-environments-reference.md | unreviewed |
| Claude Desktop | Team/Enterprise admins set OTLP endpoint, protocol and headers in Organization settings; events `user_prompt`, `assistant_response`, `tool_result`, `api_request` (cost, tokens, cache), `api_error`, `tool_decision` correlated by `prompt.id`; content capture per device via `otlpContentCapture` (metadata only by default); does not cover Code-tab sessions. Traces on 3P are beta. | GA | https://claude.com/docs/cowork/monitoring.md | reviewed |
| OpenHands | Env-only OTEL tracing: `conversation` > `conversation.run` > `agent.step` > `llm.completion` / `tool.execute`; Laminar default instrumentation; any OTLP backend. | GA | https://docs.openhands.dev/sdk/guides/observability.md | unreviewed |
| goose | `OTEL_EXPORTER_OTLP_ENDPOINT` enables export for traces, metrics, logs; message content off by default; W3C trace context from ACP prompts reaches MCP tool calls (v1.55.0). Langfuse via `LANGFUSE_*` keys (what is sent is not described). | unclear (no label) | https://goose-docs.ai/docs/guides/environment-variables.md, https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| Oh My Pi | OTLP export (http/protobuf only) when an endpoint env is set; optional message-content capture warned to expose prompts. | GA | https://omp.sh/docs/env | unreviewed |
| OpenClaw | `diagnostics-otel` plugin: metrics, traces, logs over OTLP/HTTP protobuf; W3C `traceparent` on provider calls; `diagnostics-prometheus` plugin behind Gateway auth. Default content-capture privacy not read. | GA | https://docs.openclaw.ai/gateway/opentelemetry | unreviewed |
| DeepSeek harness | OTel backend mirrors every canonical session event through a redaction waterfall; the seam ships no redaction rules; CLI README says upload is feedback-gated (`FEEDBACK_ONLY` default, `DSH_TELEMETRY_MODE=DISABLED`, `FULL` rejected); best-effort delivery. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-telemetry, https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md | reviewed |
| Cline | Enterprise OTel export of metrics and logs; Prompt Storage syncs each task's `api_conversation_history.json` (contains code) to the customer's S3 or R2 bucket with a retry queue (`maxRetries` 5, `maxQueueSize` 1000, 7 days); no built-in erasure. | GA | https://docs.cline.bot/enterprise-solutions/monitoring/prompt-storage.md, https://docs.cline.bot/enterprise-solutions/monitoring/telemetry.md | unreviewed |
| Cursor | OpenTelemetry export for enterprise and an OTel guide for the Agent SDK; details beyond index titles not opened. | GA (no label) | https://cursor.com/llms.txt | unreviewed |
| Pi | `pi-telemetry`: vendor-neutral spans/metrics contract with no-op default, in-memory and SQLite adapters, conformance suite, trace renderer; no exporter included. | unclear | https://github.com/earendil-works/pi/blob/main/packages/telemetry/README.md | unreviewed |
| Warp | Self-hosted workers can export OpenTelemetry metrics. | GA | https://docs.warp.dev/factories/self-hosting/ | reviewed |
| OpenCode | The V1 `openTelemetry` config key is ignored in V2. | GA | https://opencode.ai/docs/migrate-v1/ | reviewed |
| rizzy-harness (ours) | None: no Prometheus and no OpenTelemetry. | none | grep finds no exporter (own inventory) | from own inventory |

Nothing documented: Antigravity, Zed/ACP, Hermes Agent (pages read), OpenCodex (usage logs as JSONL only).

#### Logs, transcripts and raw request logging
What it is: durable records a developer can read after the fact: application logs, per-session transcripts, and raw provider requests.
Variants: only goose documents raw LLM request logs by default (last ten, with a sensitivity warning inferred from the page); Claude Code has opt-in request/response body events; Pi gives extensions before/after provider hooks; Oh My Pi has a `before_provider_request` hook; ours keeps the event log as the trace and has no raw provider log.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| goose | CLI, server (`goosed`, includes JSON-RPC traffic) and desktop logs; LLM request logs (raw request/response, last 10 as `llm_request.0..9.jsonl`); date-based logs deleted after two weeks; local only. | GA | https://goose-docs.ai/docs/guides/logs.md | unreviewed |
| Claude Code | `--debug` writes `~/.claude/debug/<session-id>.txt` (or `--debug-file`); hook execution lines per command hook; JSONL session transcripts; daemon log; opt-in OTel `api_request_body` / `api_response_body` events (content truncated at 60 KB by default). | GA | https://code.claude.com/docs/en/hooks.md, https://code.claude.com/docs/en/agent-view.md, https://code.claude.com/docs/en/monitoring-usage.md | unreviewed |
| Codex | `log_dir` enables a plaintext `codex-tui.log`; `/feedback` uploads logs; conversation events kept in `rollout-*.jsonl` with a SQLite index. | GA | https://learn.chatgpt.com/docs/agent-configuration/agents-md.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCode | Log at `~/.local/share/opencode/log/opencode.log` with `run` and `role` fields; durable per-session event log with sequence numbers and tail-follow. | GA | https://opencode.ai/docs/troubleshooting/, https://opencode.ai/docs/api/ | unreviewed |
| OpenCodex | Request log ring with filters by provider, model, status, time, speed bucket and conversation ID; classified upstream failure reasons; JSONL usage logs. | GA | https://opencodex.me/guides/web-dashboard/, https://opencodex.me/reference/architecture/ | corrected |
| Pi | `/debug` writes `pi-debug.log` (may contain sensitive data); extensions get `before_provider_request` and `after_provider_response` hooks; session files are versioned JSONL with usage entries. | GA | https://pi.dev/docs/latest/usage, https://pi.dev/docs/latest/extensions | unreviewed |
| Oh My Pi | Logs under `~/.omp/logs/`, `PI_TIMING`, `--log-level debug`; `before_provider_request` hook; auto-QA reports (`xd://report_issue`, consent on first report). | GA | https://omp.sh/docs/env, https://omp.sh/docs/tools | unreviewed |
| Cline | `~/.cline/data/logs/cline.log` and `hub-daemon.log`, `CLINE_LOG_PATH`, `/reportbug` collects config, errors and system info. | GA | https://docs.cline.bot/cli/cli-reference.md, https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| Antigravity | `cli.log`, `language_server.log`, `--log-file`, sub-agent JSONL transcripts, hook stdin includes `transcriptPath`; enterprise request/response logging to the customer's Cloud project. | GA | https://antigravity.google/docs/enterprise, https://antigravity.google/docs/cli/reference, https://antigravity.google/docs/subagents | unreviewed |
| Zed/ACP | `dev: open acp logs` shows ACP messages between Zed and an external agent; a thread can be opened as Markdown for bug reports. | GA | https://zed.dev/docs/ai/external-agents.md (Debugging), https://zed.dev/docs/ai/agent-panel | unreviewed |
| Warp | Per-session network log of the client's requests and responses, opened from the Command Palette; crash-report traffic not captured. | GA | https://docs.warp.dev/support-and-community/privacy-and-security/network-log/ | unreviewed |
| Hermes Agent | `agent.log`, A2A audit log, `tool_progress: log` audit routing, gateway health export. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/cli, https://hermes-agent.nousresearch.com/docs/user-guide/messaging/a2a | unreviewed |
| OpenClaw | `openclaw logs`, `/trace`, `/verbose` (can expose internals), diagnostics export, audit history. | GA | https://docs.openclaw.ai/gateway/opentelemetry | unreviewed |
| Cursor | CLI `/logs` and request-ID copy; Cursor Cloud MCP lets an agent inspect runs, transcripts and setup logs. | GA | https://cursor.com/docs/cli/reference/slash-commands, https://cursor.com/docs/cloud-agent/capabilities | unreviewed |
| DeepSeek harness | Logs under `$DSH_HOME`; a developer trace view with timelines and tool-call inputs/outputs is described on the landing page only (vendor claim, no technical page). | preview | https://deepseek.com/harness | unreviewed |
| Claude Desktop | Enterprise Compliance API can retrieve Cowork session transcripts. | GA | https://claude.com/docs/cowork/monitoring.md | unreviewed |
| rizzy-harness (ours) | `tracing` with `HARNESS_LOG` filter (headers never logged); SQLite audit table (no UI); the full tool-call/approval/checkpoint/usage trail in the session event log, visible in the chat. No raw provider request log. | partial | `crates/cli/src/main.rs:679-684`, `crates/store/src/lib.rs:293-297`, `crates/core/src/event.rs:59-80` | from own inventory |

#### Debugging and diagnostics tools
What it is: commands that check installation, configuration and connectivity, or profile a running process.
Variants: Claude Code also has a safe mode that disables customisations to bisect problems and a `claude plugin eval` harness; OpenClaw's `triage` produces a sanitised diagnosis that can be handed to a coding agent; Warp's network log is a client-side request inspector.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `claude doctor` (read-only), `/doctor` (proposes fixes), `/debug`, `/heapdump` (snapshot contains conversation and credentials), `/status` (setting sources, provider, base URL, credential source), `/context`, `--safe-mode`, an error-reference page quoting literal messages; warning past 2.5 GB heap. | GA | https://code.claude.com/docs/en/cli-reference.md, https://code.claude.com/docs/en/debug-your-config.md, https://code.claude.com/docs/en/errors.md | unreviewed |
| Codex | `codex doctor [--network-only]`, `/debug-config` (config layers), `/warnings`. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| OpenCode | `opencode debug heap-dump`, SIGPROF/SIGUSR1 profiles, `debug paths\|config\|agents`. | GA | https://opencode.ai/docs/troubleshooting/, https://opencode.ai/docs/cli/commands/ | unreviewed |
| OpenCodex | `ocx health`, `ready`, `status`, `doctor`; `/healthz` and `/readyz`; provider test (connection, completion with a 32-token cap, test-all with confirmation); memory accounting endpoint. | GA | https://opencodex.me/getting-started/for-agents/, https://opencodex.me/guides/web-dashboard/ | corrected |
| Oh My Pi | `omp bench` (latency and throughput), `omp dry-balance`, `--log-level debug`. | GA | https://omp.sh/docs/cli | unreviewed |
| Cline | `cline doctor` (`fix` kills all Cline processes; `log`), `/reportbug`. | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| Hermes Agent | `hermes doctor` and per-component doctor commands. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/cli | unreviewed |
| OpenClaw | `openclaw doctor` (`--fix`, lint, post-upgrade, SQLite maintenance), `openclaw triage` (read-only sanitised diagnosis). | GA | https://docs.openclaw.ai/start/getting-started | unreviewed |
| goose | `goose info -v`, `goose session diagnostics`, `--debug`. | GA | https://goose-docs.ai/docs/guides/goose-cli-commands.md | unreviewed |
| Pi | `/debug`; provider payload hooks (example `provider-payload.ts`); `packages/evals` behaviour evals. | GA | https://pi.dev/docs/latest/usage, https://github.com/earendil-works/pi/blob/main/packages/evals/README.md | unreviewed |
| DeepSeek harness | `dsh rescue`; settings revision checks; trace view (vendor claim). | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md | unreviewed |
| Antigravity | `/usage`, `/credits`, `/context`, local log files; no doctor command documented. | GA | https://antigravity.google/docs/cli/reference | unreviewed |
| Zed/ACP | ACP log viewer; thread export. | GA | https://zed.dev/docs/ai/external-agents.md | unreviewed |
| rizzy-harness (ours) | `rizzy-harness doctor` (bash/git present, config and `node.toml` parse, token file mode); scripted mock provider for tests and demos; Playwright e2e scripts. | works | `crates/cli/src/main.rs:557-632`, `crates/agent-native/src/mock.rs:1-30`, `web/e2e/` | from own inventory |

#### Vendor telemetry and data-upload defaults
What it is: what the harness itself sends to its vendor by default, and how to turn it off. Distinct from the user-configured export above. Included because a self-hosted harness's stance here is a visible product choice.
Variants: default-on with opt-out (OpenCodex, Cline, DeepSeek session-log upload); gated by explicit feedback (DeepSeek OTel `FEEDBACK_ONLY`); off on third-party providers (Claude Code); no telemetry statement (Hermes).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Default-on anonymous daily summary after a consent popup (daily-changing install hash, version, platform, provider kind, model family, counts; no prompts, outputs, keys, paths); opt out with `ocx telemetry disable`, `OCX_TELEMETRY=0` or `DO_NOT_TRACK=1`; arrived in v2.100.0 (2026-11-10). The default-on claim comes from the release feed, which review did not reopen. | GA | https://github.com/lidge-jun/opencodex, https://opencodex.me/guides/web-dashboard/ | unreviewed |
| DeepSeek harness | For official-API use a session-log contributor uploads session-log events with requests by default (up to 8 MiB per request); opt out in Settings -> General. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md, https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk | reviewed |
| Cline | Telemetry on by default with opt-outs; device id; prompts excluded except to their own provider; admin can force it off in VS Code and JetBrains only. | GA | https://docs.cline.bot/enterprise-solutions/monitoring/telemetry.md | reviewed |
| Claude Code | Telemetry and error reporting off by default on Bedrock, Vertex and Foundry; error reports only for Pro/Max/Ultra on direct API; `DISABLE_TELEMETRY`; some features need flag fetches and are off without them. | GA | https://code.claude.com/docs/en/data-usage.md (Telemetry services; Default behaviors by API provider), https://code.claude.com/docs/en/llm-gateway-rollout.md | reviewed |
| Claude Desktop | 3P: crash reports and analytics scrubbed and disableable; only `downloads.claude.ai` required egress when other Anthropic services are disabled. | GA | https://code.claude.com/docs/en/desktop (Enterprise configuration), https://claude.com/docs/third-party/claude-desktop/overview.md | unreviewed |
| Codex | Built-in Statsig metrics follow `analytics.enabled`; `/feedback` uploads logs, `feedback.enabled` switches it off; explicit OTLP exporters keep working. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| Pi | Install telemetry and analytics defaults documented on the settings page; what is sent is not described. | GA | https://pi.dev/docs/latest/settings | reviewed (defaults exist) / unverified (content) |
| Antigravity | Interactions collected with an opt-out (not stored for business accounts); default on/off state not stated. | GA | https://antigravity.google/docs/settings, https://antigravity.google/docs/faq | unreviewed |
| Hermes Agent | Docs state API calls go only to the configured provider and data stays in `~/.hermes/`; managed Tool Gateway and Portal logins send data to Nous. | GA | https://hermes-agent.nousresearch.com/docs/guides/secure-hermes-on-a-work-machine, https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| Zed/ACP | No retention of prompts or code by default; ratings send the full thread to Zed; Anthropic Covered Models retained 30 days, also with an own key; `disable_ai: true` master switch. | GA | https://zed.dev/docs/ai/privacy-and-security.md, https://zed.dev/docs/ai/ai-improvement.md | unreviewed |
| OpenHands | `AGENT_CANVAS_DISABLE_TELEMETRY` exists; what is collected is not documented. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/docker.md | unverified |
| OpenCode | Not covered by the pages opened; product page says data is stored only with free models (vendor claim). | unclear | https://opencode.ai/product | unverified |
| rizzy-harness (ours) | Not in the inventory. | none | n/a | from own inventory |

---

## 3. Availability matrix

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

## 4. Common across the ecosystem

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

## 5. Only one harness has it

| Capability | Harness | Why it is interesting or not |
|---|---|---|
| Parallel answers from up to 4 models with a pick | rizzy-harness (ours) | No equivalent found in the surveyed entries for this area; cheap model comparison, but multiplies cost with no budget to bound it. |
| Content-based safety fallback (flagged cyber or biology requests re-run on another model, session stays there) | Claude Code | Vendor-specific safety routing; not portable, but shows fallback can be triggered by content, not only by errors. |
| `modelPricing` multiplier and per-model overrides for chargeback, applied to `/usage`, `--max-budget-usd` and OTel cost | Claude Code | Directly useful for a multi-user master that bills teams; managed-only, so users cannot edit their own rates. |
| Gateway-side spend caps by user, group and org, with pooled budgets, enforced on every request | Claude Code (apps gateway) | The closest design to a master enforcing per-user budgets; it is a gateway service, not a client setting. |
| Managed OTLP destination lock (developer endpoints stripped when admin sets one; a user endpoint once redirected away from the collector, closed in v2.1.217) | Claude Code | A security detail: telemetry destination is policy, not preference. |
| Cache-miss attribution in `/usage` and idle compaction timed to the cache TTL | Claude Code | Cost optimisation users notice; ties observability to prompt-cache behaviour. |
| `claude plugin details` projected token cost of a plugin | Claude Code | Cost shown before install; out of our scope until we have plugins. |
| Six routing strategies, tool-call stickiness within a turn, quota-window-aware selection | OpenCodex | The most developed provider-failover design in the set; fits a master's provider layer. |
| Estimated cost labelled as list-price equivalent with coverage warnings | OpenCodex | Honest-estimate UX bar. |
| Ten model roles with `@role` aliases and per-role fallback chains | Oh My Pi | Cleaner than per-feature settings; one resolver for all auxiliary work. |
| `omp bench` and `omp dry-balance` (simulate account selection) | Oh My Pi | Testing the routing logic without sending traffic. |
| Per-task auxiliary models, each with its own fallback chain | Hermes Agent | Shows that auxiliary calls need their own failure handling. |
| Local subscription proxy exposing a user's own subscription to other apps (no auth of its own) | Hermes Agent | Convenient and risky; legal status not discussed in the entry. |
| Custom model routers by complexity or natural-language rules in YAML | Warp | User-authored routing; targets restricted to Warp-supported models. |
| Raw LLM request logs, last ten kept | goose | Useful for debugging; the page's own implication is that they hold raw prompts. |
| Redaction waterfall on a full-ledger OTel mirror, with the seam shipping no rules | DeepSeek harness | Shows the danger of an export with no default redaction. |
| Provider-reported quota and spend rather than estimates; subscription sessions hide dollars | OpenClaw | Avoids showing a wrong number; needs provider cooperation. |
| Goal token and time budget as persisted thread state | Codex | Budget is part of the log, not a runtime flag. |
| Config options (`mode`, `model`, `model_config`, `thought_level`) as protocol-level session state | ACP | Model and effort selection that a web UI can fold from events. |

---

## 6. Ours against the field

Ranked by how much it matters for a self-hosted master/node harness that runs unattended work and shares a master across users.

1. **No provider retry or backoff (largest functional gap).** A single 429 or 5xx either triggers a fallback model or ends the turn (`crates/agent-native/src/provider.rs:104-116`). The others document: Claude Code retries transient failures up to 10 times with exponential backoff, has an unattended mode that waits out 429/529 for hours, aborts quiet streams with four watchdogs, treats gateway `retry-after` above 60 s as "stop", and never retries a spend-limit 429. Pi's defaults are 3 retries, 2000 ms base, 60000 ms max, with provider-level retries 0 so layers do not multiply. Codex honours `Retry-After` and stops when the model is not found. OpenClaw backs off 30 s, 1 min, 5 min and disables a billing-failed credential for 10 minutes. OpenCodex cools a target from `Retry-After` then a configured delay. Hermes fires fallback only after retries (429/5xx) but immediately for 401/403/404. A node running a long routine overnight is exactly the unattended case Claude Code's watchdog exists for. See the retry feature entry.
2. **No cost tracking.** We record four token counts per turn and nothing in currency. The field computes cost locally from a price table and labels it an estimate (Claude Code `/usage`, `costBasis` in SDK results; OpenCodex "list-price equivalent, not billing" with coverage warnings; Pi billed vs estimated; Oh My Pi and goose carry per-model prices in config). OpenClaw shows the opposite choice: provider-reported spend, no dollars for subscription sessions. We already read per-model data from `/models`; a price field and a per-turn cost event would be the minimum. Claude Code's `modelPricing` multiplier (discount or markup, managed-only) is the design to study for team chargeback. See token and cost display.
3. **No budgets, and a dead `budget` field.** The agent frontmatter `budget: lean/standard` is parsed and never read, which is worse than absent because it promises something. Meanwhile the per-user limit is none and the main agent has only a 50-call step cap. The field's enforcement points are instructive: OpenCode Console enforces at request start (429); Claude Code's SDK cap checks after each response and can overshoot; Claude Code's gateway caps per user, group and org with daily/weekly/monthly windows and a pooled option, returning a non-retryable 429. For a shared master, per-user budgets belong in the master and should be checked before a turn starts. See budgets and limits.
4. **No export (OpenTelemetry or Prometheus).** Our event log already contains the facts (tool calls, approvals, usage). Field practice to copy: opt-in, off by default; project or repo config must not be able to set the destination (Codex, Claude Code); redact prompts by default (Codex) or make content capture a separate switch (goose); tag every datapoint with a prompt id so logs, metrics and traces join. DeepSeek harness is the warning: a full mirror with no default redaction rules exports message text and tool results. Our invariant "no secret in an event, a URL or a log line" makes the redaction default non-negotiable. See OpenTelemetry and other export.
5. **No raw provider request log.** Only goose documents one by default and flags that it holds raw prompts; Claude Code makes bodies opt-in; Pi and Oh My Pi expose hooks instead. Debugging provider quirks (the whole reason Claude Code's gateway guide and Oh My Pi's compat-flag matrix exist) is hard without it. If added, it must follow the secrets invariant and be off by default.
6. **Provider breadth and reasoning.** Two wire formats cover a lot through OpenAI-compatible endpoints, but there is no native Gemini, Bedrock or Vertex adapter and no Responses API (Codex accepts only Responses, so we cannot be a drop-in for a Codex-style provider). Anthropic models are forced to `effort=false` and reasoning text is not surfaced, so Claude users get no thinking control while Claude Code, Oh My Pi and Pi expose it. See provider support, cloud-platform, effort.
7. **No credential pools, rotation or per-person credentials.** OpenClaw ties credentials to a gateway profile per person; Hermes and OpenClaw rotate keys inside a provider before falling back across providers; OpenCodex pools accounts with thread affinity. For a self-hosted master with several users this matters more than for a single-user CLI, but pooling subscription accounts carries the legal risk Anthropic and OpenCodex both state. Key pooling for API keys does not. See routing and credential pools.
8. **No model governance.** Claude Code has `availableModels`, `deniedModels`, `maxEffortLevel` and `allowedProviders`; OpenCodex scopes `allowedModels` per client key. We have per-workspace default overrides only. An admin on a shared master will ask for this before asking for dashboards.
9. **Model roles for auxiliary work.** Hermes and Oh My Pi route compaction, vision, titles and approval review to chosen models; our history summarisation exists but the inventory names no separate model for it. Low cost to add given per-agent models exist.
10. **No fast/speed tier.** Not in the inventory; only relevant for providers that sell one.

Where we are level or ahead:
- **Fallback with a visible record.** Per-agent ordered fallbacks, a `model_fallback` event in the chat, and no switching mid-stream match OpenCodex's "only before output" rule. Claude Code's own docs admit its fallback is not visible in `/status` (a discoverability gap); ours is in the log.
- **Capability discovery.** Reading context, vision, tools and effort from `/models` with a daily refresh and keeping vanished models as `unavailable` is on par with Claude Code's gateway discovery and ahead of harnesses that rely on model-ID patterns.
- **Selection order and parallel answers.** A five-step selection order and up to 4 parallel answers with a pick; no equivalent found.
- **Key safety.** Same-origin-only redirects for stored keys echoes Claude Code's treatment of a discovery redirect as failure so credentials cannot leak.
- **No subscription logins.** Our deliberate stance avoids the legal limits Anthropic and Antigravity state, though it costs convenience that OpenCode, Hermes, goose and Pi offer.
- **Event log as trace.** The session log already carries what other harnesses add through OTel traces.
- **Vision with enforced limits** and **voice input** (OpenAI-style transcription, not stored) are comparable to the field; we lack TTS and realtime voice.

---

## 7. Corrections, contradictions and unverified claims

Changed by review (the corrected text is used above):
- **Codex.** The four model descriptions are confirmed but the page names no default model; the claim that GPT-6.1 Sol became the bundled default in 0.161.0 was not confirmed. "Responses API only, `wire_api = "chat"` unsupported" is on `models.md`, not the config page. The 300 s figure is an example `refresh_interval_ms`; 0 means refresh only after an auth retry. The project-config blocklist also includes `apps_mcp_product_sku`. The Speed page still cites GPT-5.6 beside GPT-6 (stale).
- **OpenCodex.** The account-pool selection wording ("round-robin or fill-first") was not on the dashboard page; the page describes a selection order and lowest-usage choice for new sessions. The "256 MiB memory budget" was removed (not found). Client support lists 28 clients, not about 25. Pairing code lifetime is five minutes. "Remember device" is a 30-day record expiry, off by default. The default-on telemetry claim (v2.100.0) comes from an Atom release feed the reviewer did not reopen.
- **Claude Desktop.** The 3P overview carries no GA label ("GA" is an inference); the list of unavailable features (computer use, cloud sessions, Remote Control) comes from other pages, not the overview.
- **DeepSeek harness.** The credential file location and "keys are write-only" are on the providers guide, not the credentials subsystem page, which gives no file location, permissions or encryption. The two telemetry sources differ in tone (subsystem page: no end-user opt-out described; CLI reference: `FEEDBACK_ONLY` default).
- **Cursor.** The Legacy Privacy Mode statement is on the cloud security page; the privacy page confirms Anthropic harm-review retention for Fable models.
- **Claude Code (review-2 and review-3).** Model flags, default model and effort, fallback chains, content-based fallback, advisor, fast mode, model allowlists and `/usage` cost semantics were confirmed. Routine naming and several non-model statuses were corrected (outside this group). Review-1 notes that no entry says which model summarises during compaction, though fallback chains apply to compaction.
- **Oh My Pi.** README lists nine roles; docs list ten (adds `designer`). README claims round-robin credentials; docs/providers does not describe it. "Default thinking level `high`" is stated on docs/settings, not docs/roles.
- **Pi.** Agent retry defaults confirmed (3, 2000 ms, 60000 ms). "15+ providers" on the homepage confirmed, versus more than thirty on the docs page.
- **goose.** ACP providers and the deprecated CLI providers confirmed; the troubleshooting binary is `claude-agent-acp`, not `claude-acp`.

Contradictions between a vendor's own pages:
- **Claude Code.** `fast-mode.md` still says fast mode is unavailable on Google Cloud, a day after 2.1.322 added it on Vertex. Default-model advice (Sonnet for most work) contradicts every plan defaulting to Opus 5.5; `costs.md` describes thinking budgets that `model-config.md` says current models ignore. A group spend cap is "not a shared pool" on one page while the config reference documents `pooled_budget`. Auto mode is "available on every provider" on one page and model-gated on another. Enterprise cost averages ($13 per developer-day and similar) are vendor claims without method.
- **Antigravity.** Plans page says no BYOK or custom endpoints, yet CLI settings list `modelProvider`, `customModelsConfig` and `GEMINI_API_KEY`, and the SDK supports Gemini keys, Vertex and local servers. Effort: `max` in the CLI reference versus low/medium/high elsewhere.
- **goose.** In-session page says the CLI cannot change model via a setting; the CLI reference lists `/model`. README says 15+ providers; docs list well over 40. The multi-model page promises dynamic switching strategies but documents only `/model` and per-sub-agent models.
- **Hermes Agent.** Sub-agent iteration cap is 50 on one page and `delegation.max_iterations` 250 on another.
- **Zed.** Remote development page is contradictory on where model calls originate.
- **Pi and goose** home pages understate provider counts.

Unverified or only vendor claims:
- **Oh My Pi** `omp stats` dashboard contents (no docs page; `/docs/stats` returns 404). Provider terms of service for subscription logins (docs silent).
- **OpenCode** telemetry behaviour of the local app; product page "100% free" and "data stored only for free models" are vendor claims.
- **OpenHands** LLM routing, fallback and subscriptions pages (index only); telemetry contents.
- **Hermes Agent** "spend limits that YOLO cannot skip" appears in a distinctive-features bullet but no entry describes it; credential-pool page not opened.
- **DeepSeek harness** token cost display and usage UI; trace view is a landing-page claim.
- **Cline** ClinePass "2-5x" is a vendor claim; provider list taken from nav titles; OpenTelemetry events and env pages not opened.
- **Cursor** Analytics and OTel details beyond index titles; BYOK page is brief.
- **Warp** BYOK, pricing and analytics API entries were not checked in review; model-choice pages at one path returned 404 (real path `/agents/inference/model-choice/`). Whether local-run model calls always go via Warp's servers is not spelled out.
- **Zed** hosted-model list and per-token prices page not opened; several ACP RFDs (usage, compaction, custom LLM endpoint) are title-only.
- **Claude Code** most provider and gateway entries (Bedrock, Vertex, Foundry, gateway upstreams, spend limits, OTel) were outside review-2's 47 checked entries; they are unreviewed, not wrong. Changelog-derived version and date claims were only partly reachable by reviewers.
- **OpenCodex** image and video bridge guides and the full provider matrix were not opened.

---

## 8. Questions for Round 2 (source code only)

Open-source harnesses only; Claude Code, Cursor, Antigravity and Claude Desktop are closed.

- **Codex, retry and overload.** How are retries counted and delayed (the entry only says `Retry-After` is honoured), what exactly makes it stop on "model not found", and does the WebSocket-to-HTTP fallback preserve partial output?
- **Codex, goals.** How is `tokenBudget` enforced: checked before each continuation or per model call, and what happens on overshoot?
- **Codex, OTel.** Where is the 2,048-byte tool-result cap applied and is prompt redaction a pattern match or a drop?
- **OpenCodex, combos.** How are hop errors classified, how is cooldown stored (memory only, as thread affinity is), and how does "metered-target consent" gate a hop? How does the estimated-cost table get its prices and how are coverage warnings computed?
- **OpenCodex, account pool.** What does "lowest-usage" compare across 5h/weekly/30d windows when windows disagree?
- **OpenCode, cost and stats.** How `opencode stats --cost` computes cost (price source, cache tokens) and how Go allowance windows are enforced in the client versus the Console.
- **OpenCode, retries.** Nothing documented; settle by reading the provider layer. Also what cache warming does and when `settings.cache.ttl` applies.
- **DeepSeek harness, telemetry.** What the redaction waterfall does by default when no rules are registered, and what the `token-meter` subsystem exposes for cost.
- **DeepSeek harness, compat flags.** How `supportsDeveloperRole`, `maxTokensField` and `thinkingFormat` change request bodies.
- **Oh My Pi, fallback and pools.** How `usageAwareFallback` reads coding-plan quota and applies the reserve; whether round-robin credential selection with session affinity exists as the README claims (`omp dry-balance`); the backoff constants in the `retry.*` namespace; where `omp stats` stores data.
- **Oh My Pi, roles.** How roles auto-resolve from authenticated models when `modelRoles` is empty, and why `advisor` will not inherit default.
- **Pi, retry.** How agent-level retry (3, 2000, 60000) and provider-level retry (0) interact on a streamed response; how billed vs estimated cost is determined; what `pi-telemetry` spans contain and who calls them.
- **OpenHands, LiteLLM layer.** What retry and fallback LiteLLM is configured with; how per-conversation budget meters and Enterprise budgets are enforced; which spans carry token counts.
- **OpenClaw, failover.** The exact profile-rotation and cooldown algorithm (30 s, 1 min, 5 min cap; 10 minutes for billing failures) and how "prefer the working fallback for 30 minutes" is stored; what `/usage cost` reads from providers.
- **goose, rate limits and logs.** Whether any generic retry exists beyond Bedrock and Databricks env vars; how `llm_request.N.jsonl` rotation works and whether it redacts headers; what the Langfuse integration sends.
- **Hermes Agent, fallback and pools.** The retry policy before fallback fires, the credential-pool rotation rules, how auxiliary fallback chains differ from the main chain, and what "spend limits" in the approval layer are.
- **Cline, cost.** How `totalCost` is computed (catalogue prices, cache pricing) and what the OpenTelemetry events contain.
- **Warp client (open), routing.** Where custom model routers are evaluated (client or Warp's control plane) and how fallback routing between comparable models is chosen.
- **Zed and ACP, config options and usage.** How `session/set_config_option` categories (`model`, `thought_level`) are rendered, and what the v1 usage update carries versus the end-turn usage RFD.

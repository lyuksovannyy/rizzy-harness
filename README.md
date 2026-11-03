# harness

A self-hosted coding-agent harness: one Rust binary that serves a web dashboard
for coding agents, with the agent loop running on the machine that owns the
code. See [`PLAN.md`](PLAN.md) for architecture, event schema, threat model and
phase status.

**Status:** Phases 1–8 done and most of Phase 9 (checkpoints with undo, sub-agent
worktrees, parallel answers, voice input). See PLAN.md for what is left out
and why.

## Quick start

```bash
cd web && npm ci && npm run build && cd ..
```

```bash
cargo build --release
```

```bash
./target/release/harness serve
```

Open http://127.0.0.1:7777. On first run it asks for a one-time claim code
(so nobody else can take over a fresh server), creates your **admin**
account, and walks you through connecting an AI provider, your name and time
zone, and a search engine. Admins add other people under **Users**; new accounts get a temporary
password and must change it at first login. Everyone sees only their own
chats. Agentic sessions run shell commands on a worker node, so an admin has
to grant them per user. Print the claim code with:

```bash
./target/release/harness claim-code
```

Scripts can call the API with `Authorization: Bearer <token>`, using the token
printed by `harness token`.

Without configuration the built-in **mock** provider is used, so you can try
the UI with no API key. Try `run: ls -la`, `read: README.md` or `slow: 50`.

## Configuration

Everything lives in the data directory (`~/.local/share/harness`, or
`--data-dir` / `HARNESS_DATA_DIR`).

`harness.toml`: provider. API keys never go in this file: reference an
environment variable (`api_key_env`) or a key stored in the 0600
`secrets.toml` (`api_key_secret`; onboarding and Settings → AI provider write
it for you).

```toml
[provider]
kind = "openai"                      # any OpenAI-compatible endpoint
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_env = "DEEPSEEK_API_KEY"
```

```toml
[provider]
kind = "anthropic"                   # Anthropic API key only (no subscription login)
model = "<model id>"
api_key_secret = "anthropic"         # [keys] anthropic = "..." in secrets.toml
```

`effort = "low" | "medium" | "high" | "max"` is only sent to models declared
with `[[models]] id = "..."` and `effort = true`. Do not enable it for
Anthropic models yet: extended thinking combined with tool calls is not
supported until risk R8 in `PLAN.md` is fixed.

Local Ollama: `base_url = "http://localhost:11434/v1"`, no key.

Chats and web tools (restart after changing):

```toml
[chat]
incognito_ttl_minutes = 1440         # incognito chats deleted after this idle time
summarize_after_tokens = 24000       # 0 = never summarise older turns

[web]
search = "searxng"                   # none | searxng | brave
searxng_url = "http://localhost:8888" # JSON format must be enabled in SearXNG
# brave_key_secret = "brave"         # for search = "brave" ([keys] brave = "..." in secrets.toml)
fetch = true                         # web_fetch: public addresses only, URLs from you or a search

[memory]
backend = "builtin"                  # builtin | mem0 | off (restart to switch)
embeddings_url = "http://localhost:11434/v1"  # optional: recall by meaning (settable in the UI)
preset = "balanced"                  # fast | balanced | best (Ollama model names)
# mem0_url = "http://localhost:8888" # for backend = "mem0" (run with MEM0_TELEMETRY=false)
```

Memories are proposed by the assistant during chats and used only after you
confirm them under Settings → Memory.

Dictation (the mic button) needs a transcription service, for example a local
Whisper server with an OpenAI-compatible API:

```toml
[voice]
url = "http://localhost:8000/v1"     # serves /audio/transcriptions
model = "Systran/faster-whisper-small"
# key_secret = "voice"               # if the service needs a key
```

In a git repository every agentic turn is checkpointed (under
`refs/harness/`, without touching your index or branches) and its file
changes can be undone from the chat. Folders without git get a private
checkpoint repository in the data directory (nothing is written into the
folder). Only the latest 10 checkpoints per folder are kept (`checkpoints_keep`
in `node.toml`). "Rewind" undoes a turn and everything after it.

`node.toml`: the node's own policy. It is authoritative: the master can only
ask for stricter settings.

```toml
max_approval_mode = "ask"            # ask_all | ask | auto
workspace_roots = ["/home/me/code"]  # session cwd must be inside one of these
denied_tools = []                    # e.g. ["bash"]
allow_outside_workspace = false
```

`harness doctor` checks the setup.

## Using it from other devices (WireGuard)

Bind the UI to this machine's WireGuard address; it is then reachable only by
your WireGuard peers. Add `--tls` so the browser treats the page as secure
(dictation and notifications need that), and trust `keys/ui-cert.pem` in the
data directory once on each device:

```bash
./target/release/harness serve --bind 10.8.0.1:7777 --tls
```

## Remote worker nodes

Agentic sessions can run on other machines. Start the master with a node port
(TLS with a pinned self-signed certificate; only joined nodes get in):

```bash
./target/release/harness serve --node-listen 0.0.0.0:7443
```

The easy way: Settings → Nodes → **Set up a server over SSH**. Check the
server's host key, log in once with a password or key (harness installs its
own key and does not keep yours), and pick:

- **Automatic** for a fresh VPS, logging in as **root**: harness creates its
  own user `harness-admin`, sets up WireGuard, an nftables firewall, key-only
  SSH, the container engine and the node (which runs as the unprivileged
  user `harness`), then removes its own sudo. Risky steps revert themselves
  after 2 minutes unless harness can still log in.
- **Manual** for a server with other things on it, logging in as a **normal
  user**: a checklist with the commands to run. With root, harness asks
  whether it may create its own user (without sudo) instead.

Container engine: rootless **Podman** (default) or **Docker** (the `harness`
user joins the `docker` group, which is effectively root). Fully supported:
Debian 13 and Ubuntu 26.04 LTS; other Debian-based systems work but are not
tested. Test it on a throwaway VPS first.

By hand: in Settings → Nodes choose **Create join command** and run the two printed
commands on the other machine (same binary). That machine needs its own
`harness.toml` (provider + key) and `node.toml` (its workspace roots and
limits: the node, not the master, decides what may run there). Revoke a node in
the same place.

The **Ultracode** agent splits big jobs into parallel sub-tasks for Coder,
Scout and Writer (at most 4 per turn and 200k tokens by default; set
`delegates`, `max_subagents` and `max_subagent_tokens` in an agent file).

## Development

```bash
cargo test --workspace
```

```bash
cd web && npm test && npm run typecheck
```

UI dev server with hot reload (proxies `/api` to a running `harness serve`):

```bash
./target/debug/harness serve --allowed-host localhost:5173
```

```bash
cd web && npm run dev
```

Web bundle budget (also in CI):

```bash
cd web && npm run build && npm run size
```

End-to-end UI flows against the real binary (needs Playwright + Chromium):

```bash
node web/e2e/phase3.cjs target/debug/harness
```

```bash
node web/e2e/phase4.cjs target/debug/harness
```

```bash
node web/e2e/phase5.cjs target/debug/harness
```

```bash
node web/e2e/phase6.cjs target/debug/harness
```

```bash
node web/e2e/phase7.cjs target/debug/harness
```

```bash
node web/e2e/phase8.cjs target/debug/harness
```

```bash
node web/e2e/phase9.cjs target/debug/harness
```

```bash
node web/e2e/attention.cjs target/debug/harness
```

```bash
cargo build -p harness-provision --features test-util --example fake_sshd
```

```bash
node web/e2e/setup.cjs target/debug/harness target/debug/examples/fake_sshd
```

The Phase 1 acceptance test (`crates/cli/tests/acceptance_restart.rs`)
SIGKILLs the real binary mid-turn, restarts it, resumes a WebSocket client from
its cursor, and asserts the client and server logs are identical.

## Licence

MIT. See [`LICENSE`](LICENSE).

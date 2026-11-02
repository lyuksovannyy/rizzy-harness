# harness

A self-hosted coding-agent harness: one Rust binary that serves a web dashboard
for coding agents, with the agent loop running on the machine that owns the
code. See [`PLAN.md`](PLAN.md) for architecture, event schema, threat model and
phase status.

**Status:** Phase 1 (single machine, master with embedded node). Remote nodes
are Phase 2.

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
(so nobody else can take over a fresh server) and then creates your account.
Print the code with:

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

`harness.toml`: provider. API keys are referenced by environment-variable
name and never written to disk:

```toml
[provider]
kind = "openai"                      # any OpenAI-compatible endpoint
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_env = "DEEPSEEK_API_KEY"
```

Local Ollama: `base_url = "http://localhost:11434/v1"`, no `api_key_env`.

`node.toml`: the node's own policy. It is authoritative: the master can only
ask for stricter settings.

```toml
max_approval_mode = "ask"            # ask_all | ask | auto
workspace_roots = ["/home/me/code"]  # session cwd must be inside one of these
denied_tools = []                    # e.g. ["bash"]
allow_outside_workspace = false
```

`harness doctor` checks the setup.

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

The Phase 1 acceptance test (`crates/cli/tests/acceptance_restart.rs`)
SIGKILLs the real binary mid-turn, restarts it, resumes a WebSocket client from
its cursor, and asserts the client and server logs are identical.

## Licence

MIT. See [`LICENSE`](LICENSE).

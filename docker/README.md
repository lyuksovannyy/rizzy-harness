# Self-hosting templates

Two Docker Compose templates for the central server (web UI, chats, node
management). Agentic nodes do not run in containers by design: they need
the real host. Set them up from Settings → Server → Nodes.

| Template | Services | Use it for |
|---|---|---|
| [`minimal/`](minimal/) | harness | one server, everything in the data directory (SQLite, logins included) |
| [`full/`](full/) | harness, Qdrant, Ollama | memory recall by meaning: vectors in Qdrant, embeddings from a local Ollama |

Neither template starts a service the server does not use. Everything but
memory vectors is stored in SQLite in the data directory; a shared database
is a later plan (see [docs/storage-backends.md](../docs/storage-backends.md)).

## Start one

```bash
cd docker/minimal
```

```bash
cp .env.example .env
```

Edit `.env` (the `full` template needs a `QDRANT_API_KEY`; `openssl rand -hex 24`
makes one), then:

```bash
mkdir -p data && sudo chown 10001:10001 data
```

```bash
docker compose up -d
```

```bash
docker compose exec harness rizzy-harness claim-code
```

Open http://127.0.0.1:7777 and claim the server with that code.

## `.env`

[`env.reference`](env.reference) lists every setting. Precedence:
**environment > `.env` > `config.toml` > defaults**.

- Settings marked `[config]` there also exist in `config.toml`. Set in the
  environment, they win at runtime and show read-only in the UI. They are
  written into `config.toml` only where it has no value yet, so an existing
  `config.toml` is never changed.
- `HARNESS_PROVIDER_*` adds a first AI provider to `config.toml` while it
  has none, so onboarding starts with it filled in. The key stays in the
  environment: the provider refers to `HARNESS_PROVIDER_API_KEY` by name.
- No `HARNESS_*` variable and nothing a `.env` sets reaches an agent's shell.

## One harness process

Run one server process per data directory. Logins are rows in its SQLite
database, and every other table lives there too, so a second process behind
the same address is not a supported setup.

## Remote nodes from a container

The templates publish only the UI, on loopback. To accept remote nodes, add
`HARNESS_NODE_LISTEN=0.0.0.0:7443` to `.env` and publish the port to the
address the nodes reach (a WireGuard address, never the internet):

```yaml
    ports:
      - "127.0.0.1:7777:7777"
      - "10.77.0.1:7443:7443"
```

The repository's top-level `compose.yaml` (host networking) is still the
simplest way when the server's WireGuard tunnel runs on the same host.

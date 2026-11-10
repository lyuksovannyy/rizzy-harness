# Nodes, credentials and environments: design note

Status: **proposal, nothing built.** For after Gate 3 of `FIX_PLAN.md` (feature freeze until
then). Depends on pull request 4 (`integration/gate3`: the Landlock sandbox, the bounded node
link, the PDF helper) being merged. The decisions were made by the owner on 2026-11-10, and
the owner answered this note's open questions the same day (2026-11-10). Costs and conflicts
with the code are stated, not argued away. One question is still open (§9).

**Summary.** Today a node is global, holds its own provider key in its own `config.toml`, and
runs every session in a folder on the host. This note makes a node belong to one workspace,
added by an owner or admin who types the node's pair code. It moves model credentials to the
workspace (bring your own key, nothing server-wide for chat models) and gives every workspace
node just the keys a turn needs, in memory only. It adds **environments**: a per-workspace
volume on one node in which sessions run inside rootless Podman containers, each in its own git
worktree. Deleting an environment never migrates anything. It classifies every session, gives
people time to save, and archives the sessions. To continue elsewhere you **fork**. Forge
accounts come through per-user **Connections**. **Model aliases** let agent definitions and
helper slots name a group of models instead of a provider-specific id.

Terms used below:

- **Workspace node**: a node registered to exactly one workspace. Adding it is what makes
  it trusted: there is no separate trust mark.
- **Pair code**: a short, single-use code the joining node shows and a workspace owner or
  admin types into the master's UI to add that node.
- **Grant**: what the master sends a node so one turn of one session can call its models.
- **Environment**: a named volume (checkout plus caches) on one node, belonging to one
  workspace. The container that uses it is disposable.
- **Forge**: the git host a repository lives on (GitHub, GitLab, Gitea...).
- **Connection**: a forge account attached in the harness, either by a user (theirs, saved per
  user) or by a workspace admin (the workspace's).

## 1. The problem and the evidence

| # | Problem | Evidence |
|---|---|---|
| P1 | Nodes are global. Any server admin's join token registers a node every agentic user can pick; nothing ties a node to a workspace. | `nodes` has no owner or workspace (`crates/store/src/lib.rs:280-286`); `join_tokens` only `created_by` (`:288-292`); join token is admin-only (`crates/master/src/http.rs:1790-1808`); `/api/nodes` filters only the local node (`http.rs:425-434`). |
| P2 | Every remote node holds a long-lived provider key on disk. | `node_run` loads its own `config.toml` and starts a `ProviderRuntime` (`crates/cli/src/main.rs:509-512`); the README says to put "provider and key" next to `node.toml` (`README.md:694-698`, `:719`). Research: "a prompt-injected `bash` on `main` can read the node's `config.toml`" (`research/harnesses-round-1.md` §7, "The central tension"). |
| P3 | Model chains name models the remote node may not have. | The master resolves through `resolver_for` and sends `model` (`crates/master/src/lib.rs:969-991`); the node resolves it against one router built from its own file (`crates/cli/src/main.rs:451-466`). |
| P4 | Providers are server-wide. Team workspaces only overlay chains; Personal workspaces have no settings. | `providers.rs:1-3` ("the whole server uses"); `Config.providers` (`crates/master/src/config.rs:16`); `overlay` (`crates/master/src/resolver.rs:27-54`); Personal gets the server's (`resolver.rs:95-112`). |
| P5 | Two sessions in one folder undo each other's changes. | `checkpoints_keep` is "per working folder" (`crates/node/src/policy.rs:20-21`); shadow repo keyed by work-tree path (`crates/node/src/git.rs:542-545`); `prune` keeps the latest N groups of the whole repository (`git.rs:821-863`). |
| P6 | No isolation beyond Landlock: the shell runs on the host as the node user. | `Confinement` is `Unconfined`, `Confined` or `Refused` (`git show integration/gate3:crates/tools/src/sandbox.rs`, lines 281-293); residuals: pathname Unix sockets before ABI 9, UDP, DNS (lines 17-26). |
| P7 | No forge credentials exist; "Connections" is a placeholder. | `web/src/components/Sidebar.tsx:129-130`. |
| P8 | No archived or read-only session state, no fork. | `sessions` columns (`crates/store/src/lib.rs:224-244`); read-only is only computed from grants and roles (`crates/master/src/workspaces.rs:429`). |

## 2. The decisions, with their costs

| Decision | Cost |
|---|---|
| **Podman** for session containers and self-hosting; Docker only for self-hosting. | Rootless Podman needs setuid `newuidmap`, `/etc/subuid` and a user runtime directory; the README's unit (`NoNewPrivileges=yes`, `nologin` user, `README.md:706`, `:742-749`) must change. Docker hosts get no container sessions. |
| **Chat-model keys per workspace (BYOK)**, no global store, nothing copied. Onboarding writes its first provider into the first admin's Personal workspace. **R1 (reading, to confirm):** the `HARNESS_PROVIDER_*` settings seed that Personal workspace. | `providers`, `default_model`, `[agent_models]` (`config.rs:16-24`) and `HARNESS_DEFAULT_MODEL` (`crates/master/src/env.rs:35`) go; Personal workspaces gain settings; notices and the model cache go per workspace. `.env` seeds apply at config load (`env.rs:1-15`), before any account exists, so R1 needs a seed deferred to the claim (§4.4). |
| **Embeddings and voice are harness (server-wide) settings** with their own endpoints. They never reuse a workspace provider. Onboarding offers Ollama (the default) or a custom API endpoint for embeddings, and no "reuse the provider you just added". | Voice already has its own URL and key (`config.rs:236-243`). Embeddings lose `source = "provider"` and `provider_id` (`config.rs:285-292`, `:304`) and the onboarding path that reuses the provider. |
| **A1 (to confirm):** title generation, summarisation and the vision bridge use the workspace's models. | Summarisation already uses the turn's model on the node (`crates/agent-native/src/lib.rs:1058`); I found no title generator or vision bridge on `main` (`crates/master/src/lib.rs:229-235` refuses instead): slots to add. |
| **Every workspace node is trusted and may receive grants.** Adding it (an owner or admin types its pair code) is the act of trust. A grant carries only the keys the turn needs, held in memory only, kept out of the command log and redacted. A restarted node needs the master before it can run a turn. `node.toml` may refuse grants or use local keys. **No proxy now**, but the hand-over is a grant that a proxy can fill later. | A node's operator can read the keys its turns use; a stolen node key yields API keys; removal cannot un-send keys (the UI lists which to rotate). |
| **No long-lived secret in a shell command**; the node generates its own key pair; the private key is kept under systemd encrypted credentials, else in a 0600 file. **A2 (to confirm):** "PAM" read as systemd encrypted credentials. | `systemd-creds encrypt` needs root, so the systemd path is a second, root step (§4.4). |
| **A3 (to confirm):** an offline node finishes its running turn; a new turn waits. | Grants must outlive the longest turn (§4.3). |
| **Nodes belong to a workspace** and carry their own permissions (host shell, containers). | Reverses "per-workspace node access ... dropped ... could be bypassed through Personal" (`PLAN.md:1765-1768`). The bypass is closed by a workspace check at create and before every prompt (§4.1). |
| **Private per-user node** (own confirm code, model access by proxy) is experimental and comes last; restricting by source IP was dropped. **Landlock stays**; containers are a further `Confinement` kind. | Two execution paths to test and document. |
| **Environments**: the volume is kept; the node builds the invocation; the image comes from an allow-list, plus a setup script; one worktree per session; files are shared, chats stay private. | File tools run in the node process and the shell runs in the container, so paths and file ownership must agree (§4.6). Shared files change T10 (§5). |
| **Deletion**: no migration. Sessions are classified (a)/(b)/(c); other users are named, with their counts; autosave is optional, the deleter's choice. Anything unsafe forces a schedule (7 days by default, at least 1). Sessions are then archived; to continue, fork. **A4 (to confirm):** nothing unsafe may be deleted immediately. **A5 (confirmed):** a repository with no forge remote is class `c`. | Needs Connections (P7), a git path with network access (forbidden today, §4.7), archive and fork (P8). The names and counts in the dialog and autosaving colleagues' files need T10 changed (§5). My dissent is recorded in §9. |
| **Forge accounts through Connections, per user**, plus one workspace account added by admins. The workspace account makes the co-author line link properly. | OAuth needs a registered app per forge and a callback a loopback master often cannot receive. Users have no e-mail today (`users`, `crates/store/src/lib.rs:303-312`), which the plain fallback needs (§4.7). |
| **Model aliases** per workspace: a value with `::` is a concrete model, anything else an alias, never `@`. Named by capability or family; checked by capability. | "Alias" already means an agent handle on `feat/agent-roster` (`crates/agents/src/lib.rs:9` there), so always say "model alias". |

## 3. What does not change

The node dials out over TLS to a pinned certificate and signs a nonce bound to it
(`crates/link/src/identity.rs:60-64`); a node joins only once per key, in one transaction
(`crates/store/src/lib.rs:867-927`); the event log, replication and hardened git (T19) stay.

## 4. Design

### 4.1 Data model (master.db; changed in place, no migrations before the first tag)

```sql
-- nodes: gains its workspace and who added it. No trust column: being here is trust.
ALTER nodes ADD workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id);
ALTER nodes ADD added_by TEXT NOT NULL;
DROP TABLE join_tokens;                      -- replaced by pair codes (memory only, §4.4)

CREATE TABLE workspace_providers (           -- non-secret provider config, per workspace
  workspace_id TEXT REFERENCES workspaces ON DELETE CASCADE,
  provider_id TEXT, config TEXT NOT NULL,    -- JSON ProviderConfig without the key
  PRIMARY KEY (workspace_id, provider_id));
CREATE TABLE workspace_secrets (             -- provider keys and the workspace forge token
  workspace_id TEXT REFERENCES workspaces ON DELETE CASCADE,
  name TEXT, sealed BLOB NOT NULL, updated_ms INTEGER NOT NULL,
  PRIMARY KEY (workspace_id, name));
CREATE TABLE model_aliases (
  workspace_id TEXT REFERENCES workspaces ON DELETE CASCADE,
  name TEXT, models TEXT NOT NULL,           -- JSON ordered list of "<provider>::<raw>"
  PRIMARY KEY (workspace_id, name));
CREATE TABLE node_key_grants (               -- which keys a node has ever received
  node_id TEXT, workspace_id TEXT, secret_name TEXT,
  first_ms INTEGER, last_ms INTEGER, PRIMARY KEY (node_id, secret_name));
CREATE TABLE user_connections (              -- a user's own forge accounts
  connection_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users ON DELETE CASCADE,
  host TEXT NOT NULL, account_id TEXT NOT NULL, login TEXT NOT NULL,
  display_name TEXT, commit_email TEXT,      -- the e-mail the forge links to the account
  sealed_token BLOB NOT NULL, created_ms INTEGER, UNIQUE (user_id, host, account_id));
CREATE TABLE workspace_connection_choice (   -- which of my accounts I use in a workspace
  workspace_id TEXT REFERENCES workspaces ON DELETE CASCADE,
  user_id TEXT REFERENCES users ON DELETE CASCADE, host TEXT,
  connection_id TEXT NOT NULL REFERENCES user_connections ON DELETE CASCADE,
  PRIMARY KEY (workspace_id, user_id, host));
CREATE TABLE workspace_forge_accounts (      -- added by owner/admin; token in workspace_secrets
  workspace_id TEXT REFERENCES workspaces ON DELETE CASCADE, host TEXT,
  account_id TEXT NOT NULL, login TEXT NOT NULL, display_name TEXT, commit_email TEXT,
  PRIMARY KEY (workspace_id, host));
CREATE TABLE environments (
  env_id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces,
  node_id TEXT NOT NULL, name TEXT NOT NULL, image TEXT NOT NULL,   -- allow-list name
  repo_url TEXT, setup TEXT NOT NULL DEFAULT '', created_by TEXT, created_ms INTEGER,
  state TEXT NOT NULL);                      -- active | scheduled | deleting | gone
CREATE TABLE session_environments (          -- master-side, like session_workspaces
  session_id TEXT PRIMARY KEY, env_id TEXT NOT NULL REFERENCES environments, branch TEXT);
CREATE TABLE env_deletions (
  env_id TEXT PRIMARY KEY REFERENCES environments, requested_by TEXT NOT NULL,
  requested_ms INTEGER, deadline_ms INTEGER NOT NULL, autosave INTEGER NOT NULL,
  status TEXT NOT NULL, last_error TEXT);    -- scheduled | waiting_node | done | cancelled
CREATE TABLE archived_sessions (
  session_id TEXT PRIMARY KEY, archived_ms INTEGER NOT NULL, reason TEXT NOT NULL);
CREATE TABLE session_forks (
  session_id TEXT PRIMARY KEY, parent_id TEXT NOT NULL, through_seq INTEGER NOT NULL);
```

In `config.toml` (harness settings), `[memory]` keeps `embeddings_url` and
`embeddings_model` and gains `embeddings_key_secret`. `source` is `ollama` (the default) or
`custom`, and `provider_id` goes. `[voice]` is unchanged.

Notes:

- **Keys leave `secrets.toml`.** Today keys are `provider-<id>` in one 0600 file
  (`crates/master/src/providers.rs:34-36`, `crates/master/src/secrets.rs:48-52`, `:91`).
  Workspace keys and every Connection token are sealed with AES-256-GCM (`aws-lc-rs`, already a
  dependency, `PLAN.md:468-472`) under a key-encryption key in `<data>/keys/secrets.key`
  (0600). A copied `master.db` alone then leaks no key, and deleting a workspace or a user
  removes theirs by cascade. `secrets.toml` keeps the harness keys (embeddings, voice, web
  search).
- **Archived is master-side state, not an event.** The node that owns the log may be gone, and
  only it may append. An archived session refuses prompt, approval, restore and set-agent with
  409, whatever the node says.
- **Workspace check.** Create and every prompt require `nodes.workspace_id` to equal the chat's
  effective workspace (`EFFECTIVE_WS`, `crates/store/src/lib.rs:361-368`). A member who leaves
  falls back to Personal, and their chats on the team's nodes become blocked, the way
  `agentic_blocked` works (`workspaces.rs:429`).
- **Who may do what.** Add or remove nodes, create and delete environments, and manage
  providers, model aliases and the workspace forge account all need `settings` (owner and
  admin, `workspaces.rs:74-82`). Using a node or environment needs `agentic` plus the account
  grant (T11). Users manage their own Connections.
- **Workspace deletion** is refused (409) while environments exist. It revokes the
  workspace's nodes.

### 4.2 Model aliases and slots

- A value containing `::` is a concrete model; anything else is a model alias
  (`[a-z][a-z0-9-]{0,31}`, never `@`). Agent frontmatter `model:`, `AgentChain` entries
  (`config.rs:212-216`) and slots accept either.
- An alias expands to its models in order. A chain is the expanded main entry, then the
  expanded fallbacks, with duplicates removed and the result capped at `MAX_FALLBACKS`
  (`config.rs:173`). The first available model that passes the slot's capability check runs,
  and the `model_fallback` event shows which.
- Built-ins `small`, `vision`, `long-context` exist in every workspace and stay empty until a
  provider exists. The provider page proposes members from the reported capabilities
  (`crates/master/src/providers.rs:129-154`).
- Slots in `WorkspaceSettings` (`workspaces.rs:124-131`): `title`, `summary`,
  `vision_bridge`. Saving a `vision_bridge` alias without a vision model is refused. `summary`
  skips, at use time, any model whose context window is smaller than the turn's main model
  (`research/harnesses-round-1.md` §8, "Model slots and memory scopes"). Slot output is
  untrusted model text.

### 4.3 Granting model access (protocol)

New `MasterToNode` messages (beside `crates/protocol/src/lib.rs:123-258`). A grant is sent on
the same ordered link just before the `session.prompt` that uses it.

```rust
"session.grant" { req_id, session_id, grant_id, expires_ms,
                  routes: Vec<{ provider_id, access: ModelAccess }> }
enum ModelAccess { Direct { kind, base_url, key: Secret<String> },
                   Proxy  { url, token: Secret<String> } }   // reserved, not built
"session.prompt" { ..., grant_id: Option<String> }          // additive field
"grant.drop" { grant_id }                                   // turn ended, node removed
```

- **Carries:** only the providers the turn can reach: the agent's chain, each delegate's
  chain, `compare` models, and the `summary` and `vision_bridge` slots. The master's
  `Resolver` works these out from the agent spec it already builds.
- **Never carries:** another workspace's keys, a forge token (sent separately, §4.7), or
  anything that persists. A grant never goes into an event (it is not part of `AgentSpec`).
  The node's audit row stays kind plus session id (`crates/node/src/lib.rs:303-311`).
- **Master side:** sent to any node of the session's workspace that is connected, not revoked,
  and announced `credentials:master` in `node.hello` (`protocol/src/lib.rs:74-83`). Each send
  updates `node_key_grants`.
- **Node side:** `[credentials] model_keys` decides (§4.5). With `"master"` (the default),
  keys go into an in-memory map and into the `SecretSet`. They are dropped on `grant.drop`, at
  `expires_ms` (set to cover the longest turn, 6 h by default, renewed while a turn runs), and
  on link loss once the running turn ends (A3). With `"local"` or `"none"` the node does not
  announce the capability, and it refuses a grant with an ack error. The backend takes a
  router per turn instead of one provider for the whole node (`crates/cli/src/main.rs:451-466`).
  This is the main code change on the node.
- **Removing a node:** a `grant.drop` for each live grant, then the existing link drop
  (`nodes.rs:57-79`). The removal dialog then lists every key in `node_key_grants` and says:
  "rotate these keys at the provider; this node may still hold them".
- **The embedded `local` node and the master's chat runtime** run in the master process. They
  get the same per-turn router in-process, with nothing on the wire (working assumption, §9).

### 4.4 Adding a node with a pair code

**The node shows the code; an owner or admin types it into the master's UI.**

1. An owner or admin opens the workspace's Nodes page and chooses "Add node". The UI shows a
   command that carries no secret: `rizzy-harness node join wss://host:port --fingerprint …`.
   It also shows an input for the pair code, and keeps the dialog open.
2. On the node, `node join` generates its key pair as today (`identity.rs:14-38`). It connects
   over TLS to the pinned fingerprint and sends `Hello::Pair { public_key, hostname,
   signature }`, signing the master's nonce as it does now. The master keeps the request in
   memory and answers with a pair code: 8 characters from a 32-letter alphabet (40 bits),
   valid 10 minutes. The node prints the code and the first 16 hex characters of its key.
3. The admin types the code into the dialog. The master matches it to the pending request and
   inserts the node with the dialog's workspace and `added_by`. The UI shows the key prefix for
   comparison. The master then sends `Accepted { node_id }` on the still-open connection, and
   the code is gone.
4. Properties: single use; 10 minutes; bound to the workspace whose authenticated admin
   types it; no secret in any shell command; the key pair is the node's own. Limits: 5 wrong codes per dialog close it, at most 32 pending pair requests in all,
   at most 4 per source address. Codes are compared as hashes in constant time.
5. Key at rest. With `--systemd-creds` the node writes the PKCS#8 bytes only to stdout,
   piped into `systemd-creds encrypt --name=node.key - /etc/credstore.encrypted/rizzy-harness-node.key`
   run as root, and the unit gets `LoadCredentialEncrypted=node.key`. `node run` then reads
   `$CREDENTIALS_DIRECTORY/node.key`. Without systemd it is today's 0600 file
   (`crates/link/src/lib.rs:58-75`).

Code impact: pairing holds a connection for minutes, but today's handshake has a 10-second
timeout and one shared 32-slot semaphore (`crates/master/src/nodes.rs:17-21`), so pairing gets
its own. `join_tokens`, `create_join_token` and `POST /api/admin/nodes/join` go. Separately (R1),
`HARNESS_PROVIDER_*` is applied to the first admin's Personal workspace when the claim creates it.

### 4.5 `node.toml` additions

```toml
[execution]
host_shell = true            # sessions in a host folder, under [sandbox] (Landlock)
containers = false           # sessions in environments

[credentials]
model_keys = "master"        # "master": accept grants (memory only)
                             # "local": use this node's own keys file, refuse grants
                             # "none": refuse grants; no model calls here
local_keys_file = ""         # 0600, operator-managed; only with model_keys = "local"

[containers]
runtime = "podman"           # the only value; rootless is required, not a setting
environments_root = "/home/rizzy-harness/environments"   # outside the data dir
max_environments = 8
network = "on"               # "on" (pasta, no published ports) | "off" (--network=none)
allow_host_network = false   # only true makes network = "host" accepted
memory = "4g"
cpus = 2
pids = 1024
setup_timeout_s = 900
keep_deleted_days = 3        # deleted environments go to a node-local trash first
[containers.images]          # allow-list: name -> pinned reference
rust = "docker.io/library/rust@sha256:…"

[forge]
push = false                 # let the node clone with and push using forge tokens
hosts = ["github.com"]       # the only hosts a forge token may be used against
```

`model_keys` is the node deciding, not a second approval. Being added to a workspace is the
trust; this setting is what the node will accept. `"local"` is the operator's choice to keep a
key on that machine, against the default of only the link key on disk. As T5 says, nothing
received over the protocol or HTTP widens these settings. These are refused with an ack error,
each with a test (§8): an image not on the list, a network mode the node does not allow, a
grant to a node that is not `"master"`. `environments_root` must pass `check_protected`
(`crates/node/src/policy.rs:189-216`).

### 4.6 The container the node builds

One container per session, on the environment's volume: processes, `/tmp` and signals stay
apart, and the files are shared by decision. Nothing from the master, the project or the
repository enters the arguments except allow-list values:

```
podman run --detach --name rh-<env>-<session> --label rizzy.env=<env>
  --userns=keep-id --user <node uid>:<gid>     # same uid inside and out
  --cap-drop=all --security-opt=no-new-privileges
  --read-only --tmpfs /tmp:size=1g,mode=1777 --tmpfs /run
  --network=pasta | none                       # host only if allow_host_network
  --memory=<m> --memory-swap=<m> --cpus=<n> --pids-limit=<p>
  --mount type=bind,src=<root>/<env>,dst=<root>/<env>,relabel=private
  --workdir <root>/<env>/worktrees/<session>
  --env HOME=<root>/<env>/home --env <allow-listed names only>
  --init --pull=never --log-driver=none
  <image@sha256:…> sleep infinity
```

Why: `keep-id` so the node's file tools (in the node process) and checkpoint git can use what
the container wrote; no privilege or capabilities (the floor in
`research/round1/synth/02-permissions-sandbox-security.md` F15); `--read-only` so setup cannot
persist outside the volume; no published ports; `--pull=never` (pulled by digest at creation);
`relabel=private` for SELinux; the same absolute mount path because worktrees store absolute
paths.

Each `bash` call runs as `podman exec --workdir … <ctr> bash -c <cmd>`, with the allow-listed
environment (T3). Killing the `podman exec` client may not kill the process inside, so the
command runs under a wrapper that prints its process group id first. On cancel or timeout the
node runs `podman exec <ctr> kill -TERM -<pgid>`, then `-KILL`, then stops the container (T9).

**Image and setup.** The creator picks an image from the allow-list the node announced. The
setup script is stored on the master, shown to the creator, and run once with the
environment's network setting and a timeout. It is never read from the repository, and
compose files are never read.

**Worktrees and checkpoints.** `<root>/<env>/` holds `repo.git` (bare clone),
`worktrees/<session>` (branch `rizzy/<session-short>`, not detached like sub-agents,
`git.rs:866-876`), `cache/` and `home/`. The shadow repository is keyed by work-tree path
(`git.rs:542-545`), so checkpoints, pruning and undo are per session; the no-checkpoint rule
for `.git/harness-worktrees` (`git.rs:536-540`) keeps covering sub-agents only, which isolate
inside the same container (`crates/node/src/session.rs:769-824`).

### 4.7 Forge operations, Connections and commit identity

**Connections (per user).** A user attaches their own forge accounts under Connections. The
flow is OAuth device authorisation where the forge offers it, because a self-hosted master on
loopback often cannot receive a callback; otherwise a personal access token. Accounts are
saved per user (`user_connections`). When a user connects a forge in a workspace and already
has an account for that host, the prompt asks: "Use @login" or "Add another account". The
answer goes into `workspace_connection_choice`. Owners and admins also add the workspace's
own forge account (`workspace_forge_accounts`).

**Git with network.** The hardened git refuses every transport (`protocol.allow=never`,
`credential.helper=`; `crates/node/src/git.rs:59-70`). Clone, classification fetch and push
use a separate path: `protocol.https.allow=always` on top of the same overrides, the URL given
explicitly (never the repository's `remote.*`), and the token as an `http.<url>.extraHeader`
through `GIT_CONFIG_COUNT`/`KEY`/`VALUE`, never in `argv` (Landlock leaves `/proc` readable,
`sandbox.rs` `SYSTEM_READ`, lines 44-50 on gate3). It arrives in a `forge.grant` (one host, one
operation), only with `[forge] push = true` and the host in `[forge] hosts`.

**Commit identity for autosave.** These rules are my reading of the owner's words: the
workspace account pushes and commits, and the session owner's account gives the linked
co-author.

| Part | Rule |
|---|---|
| Push credential | The workspace forge account's token. No workspace account for that host: no autosave (§9). |
| Author and committer | The workspace forge account: its display name and the commit e-mail the forge links to it. |
| `Co-authored-by` trailer | The session owner, from the account chosen in `workspace_connection_choice` for that host: `Co-authored-by: <display name> <commit e-mail>`. The forge links a co-author by that e-mail, so this is what makes the line link to the account. |
| Fallback trailer | No connected account: `Co-authored-by: <harness display name> <e-mail>`. A co-author is shown, but it is not linked. The harness stores no e-mail today, so the fallback needs one (§9). |
| Message | `rizzy-harness autosave before environment deletion`, then the trailer. No session title. |

### 4.8 Deleting an environment

New messages: `env.create`, `env.delete`, `env.classify` → `env.classification`, `env.save` →
`env.saved`. `node.hello` lists the node's environments so the master can reconcile.
`env.classification` carries, per session: id, class, reason codes (`dirty`, `untracked`,
`unpushed`, `stash`, `submodule`, `detached`, `in_progress_op`, `forge_unreachable`,
`no_remote`) and counts. It never carries file names or content.

**Classification (node, read-only git):**

| Finding in the session's worktree | Class |
|---|---|
| Not a git repository, or a repository with no forge remote the environment may push to (nothing can be saved automatically; A5) | c |
| Clean status (untracked included, ignored excluded), no stash, no merge/rebase in progress, submodules clean, and HEAD reachable from a ref on the forge checked by `ls-remote` now | a |
| Anything else, or the forge cannot be reached | b |

A stash belongs to the repository, not to one worktree, so any stash in `repo.git` makes
every session in the environment class `b`. Ignored files are never work and are never
pushed; their owner sees how many there are.

**The flow:**

1. A user with `settings` opens "Delete environment", and the master sends `env.classify`. If
   the node is offline, only scheduling is offered, and classification runs at the deadline.
2. The dialog lists the requester's own sessions by title with their class. For everyone
   else it shows counts per class and, per user, name, handle and number of unsaved sessions
   ("John Doe @john has 2 unsaved sessions"). The autosave choice is offered when the node has
   `[forge] push` and the workspace has a forge account for the host.
3. If every session is class `a`, the requester may delete now or schedule. Any `b` or `c`
   means schedule only: 7 days by default, at least 1.
4. **Grace period.** New sessions are refused (409, naming the deadline). Existing sessions
   keep working so people can commit and push. Every affected user gets an in-app notice
   (`crates/master/src/notices.rs:1-8` gains a kind) and a WebSocket event; there is no e-mail.
   The requester or any other owner or admin can cancel. Every step writes an audit row.
5. **Deadline** (master clock; the scheduler also runs at start): classify again. If autosave
   was chosen, send `env.save` for every `b` session, then `env.delete`, then archive the
   sessions. A failed push keeps that session `b` and delays the deletion once, by a day, with
   a notice to the owner. After that the deletion goes ahead.
6. **Node offline at the deadline:** sessions are archived at once, the deletion is
   `waiting_node`, and the autosave (if chosen) and `env.delete` run at the next hello.
   **Node dead:** an owner can "forget" the environment, and its files are lost with the
   machine.
7. On the node, `env.delete` stops the containers and moves the folder to the trash for
   `keep_deleted_days`, then removes it.

**Autosave (`env.save`).** Commit the tracked changes and the untracked files that are not
ignored. Skip secret-like names (the artifact list, T18), files over 10 MB and anything past
100 MB in total. Push to a new branch `rizzy/autosave/<env-short>/<session-short>`, never with
force, never onto an existing branch. Commit with the identity in §4.7. Report skipped files to
their owner only.

**Removing a node** that hosts environments first schedules deletion of each of them. The node
is revoked when the last one is gone or forgotten.

### 4.9 Fork

"Continue elsewhere" is for the session's owner only. The master creates a new session and
sends the old history in `session.seed` batches: user and assistant text, each tool call cut
to one line, at most 2 MiB per batch. The new node writes them as `history_imported` events in
its own log, and `session_forks` links the two sessions. Files come back from the forge (the
autosave branch, if there is one); a fork copies no files.

## 5. Invariants

| Invariant (`CLAUDE.md`) | Effect | Mechanism or change |
|---|---|---|
| The node decides | Kept | `[credentials] model_keys`, `[execution]`, `[containers]` and `[forge]` are local to the node, and the master cannot widen them. Who may add a node is a master-side rule (owner or admin plus a pair code), and it only narrows. |
| The master never runs tools | Unaffected | Git, forge pushes and containers run on the node. The future proxy would relay model calls, not tools. |
| One writer per session | Kept | Archive and fork links are master-side tables. A fork is a new session, and its own node writes the imported history. |
| Chats are private to their owner | **Changed, needs the owner's explicit yes when built** | Members using a shared environment see its files. The deletion dialog names other users and their unsaved counts. Autosave pushes other users' files. `PLAN.md` T10 (line 369) must say all three. |
| No secret in an event, URL or log line | Kept | Grants stay outside `AgentSpec` and events. Audit rows hold kind plus id. Forge tokens exist only in git's environment. No shell command carries a secret, the pair code included. |
| Nothing the model writes is trusted | Kept | Setup scripts come from the master. Compose files and the repository's remote config are never read. Slot output is untrusted. |
| `unsafe` forbidden | Unaffected | Podman and `systemd-creds` are child processes. |
| Loopback by default | Kept | Containers publish no ports. `network = "host"` needs `allow_host_network`. |
| Harness stays light | Kept | Containers and `conmon` are separate processes. Measure node RSS with 8 containers. |
| First-load bundle budget | Kept | The new pages and dialogs are lazy chunks. Run `pnpm size` in every phase. |

`PLAN.md` lines that change: 341-346 (pairing), 363 (T3), 365 (T5), 367 (T7: a node key
obtains grants; adding needs a pair code), 369 (T10), 370 (T11), 394-396 (secrets file),
427-432 (model registry), 499-502 (R2), 1765-1768 (per-workspace node access), and §3's
message list (319-331).

## 6. How this breaks

- **A hostile repository or prompt-injected agent edits the environment's configuration**
  (`.git/config`, `compose.yaml`, `.devcontainer`). None of these is read. It can still poison
  the shared `cache/` for other sessions (§9).
- **A node's operator reads the keys.** Root on any workspace node can read the keys its
  turns use from memory. Adding a node is the admin vouching for that machine, and the Add-node
  dialog says so. An operator who does not want this sets `model_keys = "none"` or `"local"`.
- **Pair-code abuse.** Anyone who reaches the node port can open pair requests. The caps in
  §4.4 limit this, and an admin types only the code their own node shows; the key prefix is
  shown on both sides. Guessing a pending code (40 bits, 5 tries per dialog) is not a practical
  attack.
- **A stolen node key.** The thief can connect as the node and receive grants for turns that
  users start on it. A second connection replaces the first (`nodes.rs:208-212`), so the two
  copies kick each other off. The master revokes a node that flaps more than 3 times in 10
  minutes and tells the admins, who must pair it again. With a TPM, systemd credentials make
  the key useless on another machine.
- **A revoked node still holds keys.** Only rotation fixes this; the removal dialog lists the
  keys from `node_key_grants`.
- **An admin deletes an environment to destroy colleagues' work.** Anything unsafe forces at
  least a day of grace. Everyone affected is notified, another owner can cancel, the node keeps
  a trash, and there are audit rows. Left over: a sole owner can still destroy the work after a
  day.
- **Autosave pushes secrets or huge files.** Ignored files are never pushed; secret-like names
  and the size caps are skipped. An untracked `config.local.json` holding a token is still
  pushed, to a branch that every reader of the repository can see. This happens for other
  users' sessions too, by the deleter's choice. A failed push, or a forge that cannot be
  reached: §4.8 step 5.
- **The dialog as disclosure.** Names and counts tell an admin who works in the environment.
  Decided, and it needs the T10 change.
- **Wrong commit identity.** A user may connect an account they do not own. Device flow
  proves the login. A personal access token proves the account but not the e-mail it claims,
  so the commit e-mail is read from the forge's API, never typed in.
- **Two sessions in one environment.** Undo and processes are kept apart. `cache/` and `home/`
  are shared. Pushes are serialised per environment.
- **Full disk.** A plain bind mount has no quota. Below a free-space floor the node refuses
  new sessions and environments (as with artifacts, T18), and it reserves space for
  classification and autosave.
- **Podman missing, rootless not available, or the harness running in a container.** The node
  announces `containers` only after a self-test passes (run, exec, kill, `keep-id` ownership).
  When the harness itself runs in a container (`HARNESS_CONTAINER`,
  `crates/cli/src/net.rs:51-53`, `Dockerfile:33`), container execution is refused.
- **Image supply chain.** Only digests from `node.toml` are used, and a tag is refused at load.
  The UI shows each image's age.
- **Clocks.** Only the master's clock sets deadlines. The only thing the node deletes on its
  own is its trash.
- **Wrong classification.** Unpushed commits on a branch that exists on the forge are caught,
  because the check is reachability, not whether the branch exists. A stash, a dirty submodule
  or a detached HEAD gives `b`. Files outside the worktree are lost, and the dialog says so.
  Any later change on the forge is caught by classifying again at the deadline.
- **A cancel that does not reach the process inside the container** would break T9. The
  wrapper in §4.6 handles it, with a test.

## 7. What the others do

These are documentation-level findings from `research/`, to be checked by a source-code
round.

| Pattern | Who (source) | Copy or avoid |
|---|---|---|
| Key sent per session, never written | Cline SSH remote (`research/harnesses-round-1.md` §7 "Keeping credentials away from the execution side", pattern 4) | Copy, and add per-turn scope and a rotation list. |
| Inference proxied by the control side | OpenClaw cloud workers, Oh My Pi `auth-gateway`, OpenCodex hub (same section, pattern 3) | Last phase. Our master already sees every event (§7 "The central tension"). |
| Pairing codes; per-dispatch credentials; fencing | OpenCodex one-time pairing code with 429 after failures, OpenClaw device pairing and owner-epoch fencing, Claude Code work orders (`research/round1/synth/09-remote-distributed-architecture.md` §2.0 and §2.4) | Copy the pairing limits and fencing. |
| Container floor | OpenHands drops every capability but lets the client override its limits; Hermes skips approvals and checkpoints in containers (`research/round1/synth/02-permissions-sandbox-security.md` F15) | Copy the floor; avoid both mistakes. |
| Environment = image plus setup | Warp (fresh container, idempotent setup), Claude Code (setup snapshot kept about 7 days) (`09-…` §2.5) | Copy the idempotent-setup rule. |
| Moving work between machines | Codex Handoff, Warp handoff; the Claude Code runner "loses unpushed work" (`09-…` §2.0) | Classify, save and fork instead. |
| One "small model" key for several slots | Claude Code (`harnesses-round-1.md` §8 "What we should not copy") | Avoid: an alias is a per-slot default. |

## 8. Build order

Each phase is useful alone; tests are written first and fail before it. Phase 3 shrinks (no
trust mark); old phase 6 splits into Connections (6) and deletion (7); the experimental phase
moves from 7 to 8. Nothing else moves.

| Phase | What | Tests (abuse cases marked *) | Size |
|---|---|---|---|
| 1 | Workspace nodes with pair codes: `workspace_id` and `added_by`, `Hello::Pair`, its own pairing cap and timeout, `join_tokens` removed, workspace check, a Nodes page per workspace. | A code typed in workspace A registers the node in A. *A reused or expired code is refused. *A 6th wrong code closes the dialog. *A 33rd pending request is dropped. *A Personal chat that names a team node gets 403 at create and at prompt. *A member who leaves is blocked. *A non-member gets 404. | M |
| 2 | BYOK: `workspace_providers`, sealed `workspace_secrets`, Personal settings, server model settings removed, onboarding writing to Personal, the R1 seed, embeddings on their own endpoint (Ollama or custom), model aliases and slots, a per-turn router in the master. | *Workspace B cannot use A's provider id. *No key appears in plain text in `config.toml` or `master.db`. Onboarding offers no "reuse provider" for embeddings. A `vision_bridge` alias without a vision model is refused. The summary slot skips a model with a smaller window. | L |
| 3 | Grants: `[credentials] model_keys`, `session.grant`/`grant.drop`, a per-turn router on the node, `node_key_grants`, the systemd credentials option. | *`model_keys = "none"` or `"local"` refuses a grant. *The key bytes are absent from `node.db`, its WAL and the audit table. *A turn with agent X and delegate Y receives exactly those providers. *After a restart, a turn waits for its grant. Removing a node lists the keys it received. | M |
| 4 | Environments: `[containers]`, the image allow-list, a container per session, a worktree per session, checkpoints per worktree, the self-test. | *`podman inspect` shows no privilege, no capabilities, only the volume mounted, `keep-id`. *An image off the list is refused. *A repository `compose.yaml` changes nothing. Restore in session 1 leaves session 2's files alone. *Cancel kills `sleep` inside the container within 2 s. *Low disk refuses a new session. | L |
| 5 | Archived state and fork. | *An archived chat refuses with 409 even while its node is online. *A fork by a non-owner gets 404. A fork's first events are `history_imported`. | M |
| 6 | Connections: user accounts, the workspace account, the "use existing or add" prompt, the forge git path, `forge.grant`. | *Another user cannot see or use my connection. *A token never appears in `argv` (read `/proc/<pid>/cmdline` during a clone). *A host not in `[forge] hosts` is refused. The commit e-mail comes from the forge API. | L |
| 7 | Classification, scheduled deletion, autosave, node removal. | The classification table, case by case. *Immediate deletion is refused when anything is unsafe; the minimum is 1 day. *Autosave never pushes an ignored or secret-named file, never forces, never writes onto an existing branch. Author, committer and trailer match §4.7, and so does the fallback. Node offline at the deadline: archived now, deleted at the next hello. | L |
| 8 | Experimental: `ModelAccess::Proxy` (measure load on a small home server first), then the private per-user node with its own confirm code. | *A proxy token does not work for another session. *A confirm code is single-use and binds one user. | L |

Phases 3 and 4 can run in parallel after 2, and 6 after 1.

## 9. Open questions and recorded dissent

**Blocking (the one question left).** Where does the built-in in-process `local` node fit
once nodes belong to a workspace? It is server-wide and admin-only (`PLAN.md` T11). **Working
assumption, not yet confirmed by the owner:** it stays a server-admin node, usable in any
workspace the admin is a member of, with grants made in-process.

**To confirm, not blocking:**

- A1 (slots use the workspace's models), A2 (systemd credentials, 0600 fallback), A3 (an
  offline node finishes its turn), A4 (no immediate deletion while anything is unsafe).
- R1: the `HARNESS_PROVIDER_*` settings seed the first admin's Personal workspace. This is
  my reading of the owner's "yes".
- Commit identity (§4.7) is my reading: the workspace account pushes and commits, and the
  owner's connected account is the linked co-author. Still unclear:
  - Should the session owner be the git *author*, with the workspace account only
    committing?
  - With no workspace account for the host, is there no autosave, or does the owner's own
    connection push?
  - What does a user's own connection do beyond identity: clone private repositories, push
    from their sessions?
  - Which e-mail does the plain fallback use, given that accounts have none today?
- Which forges in the first build (GitHub only, or GitLab and Gitea too), and is device flow
  enough, or does a forge need a callback URL?
- Unanswered from the first round: per-user caches against shared `cache/` (cache
  poisoning between members); environments only for container sessions; workspace deletion
  refused while environments exist; always saying "model alias", never just "alias".

**Recorded dissent (decided; not blockers).** The owner kept these as written:

- Autosave of other users' sessions is the deleter's choice. I would have saved only the
  requester's sessions and those whose owner opted in.
- The dialog names users with their counts. I would have shown totals only.
- New workspaces start with no provider. I would have kept the `demo` provider in every
  workspace.

The first two need the T10 change in §5, and that needs the owner's explicit yes when it is
built.

**Not verified:**

- where chat titles come from today;
- whether `podman exec` passes signals to the process inside (§4.6 assumes not);
- whether `systemd-creds` works for user services on the distributions in the README;
- which forges offer device flow.

# Nodes, credentials and environments: design note

Status: **proposal, nothing built.** For after Gate 3 of `FIX_PLAN.md` (feature freeze until
then). Depends on pull request 4 (`integration/gate3`: the Landlock sandbox, the bounded node
link, the PDF helper) being merged. The decisions were made by the owner on 2026-11-10 and
2026-11-11; the answers to this note's first open questions are in. What is marked
**recommendation** in §4.10 and §9 awaits the owner's yes. Costs and conflicts with the code
are stated, not argued away.

Code citations marked `gate3:` are read on branch `integration/gate3` (`git show
integration/gate3:<path>`); unmarked ones are as the first version of this note cited them, on
`main`.

**What changed on 2026-11-11** (the owner: any node may be attached to several workspaces,
built around that, and `local` is an ordinary node under the rule; ADOPTION_PLAN C.12):

- A node is **attached** to one or more workspaces. The first version's workspace column on `nodes` is gone; a
  `workspace_nodes` table holds one row per attachment (§4.1).
- Each attachment is its own pair code, and **the node keeps its own list of the workspaces it
  serves** in a file only its CLI widens. The node refuses a session for any workspace not on
  it, so the master cannot attach a node alone (§4.4, §4.10.1).
- Every session names its workspace on the wire; node and master both check it (§4.10.1).
- Each attached workspace gets its own **data root** on the node; the shell sandbox, the file
  tools and container mounts never reach another workspace's root (§4.10.3).
- **Recommendation:** a node attached to more than one workspace refuses host sessions
  unless `node.toml` allows them; containers are the separated path (§4.10.3).
- Grants are keyed by `(workspace, provider)`; `node_key_grants` gains `workspace_id` in its
  key (§4.1, §4.3).
- A per-workspace **share** of the node's command slots and running containers (§4.10.4).
- **Detach** (one workspace leaves a node) is new; removing a node is now detaching it from
  every workspace (§4.10.5, §4.8).
- `local` is attached to the first admin's Personal workspace; other attachments only from
  the server's own CLI, and host sessions on it stay server-admin-only (§4.10.6).
- A draft threat-model row and the tests the build is committed to (§5, §8).

**Summary.** Today a node is global, holds its own provider key in its own `config.toml`, and
runs every session in a folder on the host. This note makes a node serve the workspaces it is
attached to, each attachment made by a pair code the node shows and that workspace's owner or
admin types, and recorded on the node itself. It moves model credentials to the workspace
(bring your own key, nothing server-wide for chat models) and gives a node just the keys a
turn needs, in memory only, usable only by sessions of the workspace that owns them. It adds
**environments**: a per-workspace volume on one node in which sessions run inside rootless
Podman containers, each in its own git worktree. Workspaces that share a node are separated by
a data root per workspace, a sandbox profile that never grants another root, and a share of
the node's capacity; on the host, under one Unix user, that separation has named holes, so a
shared node runs host sessions only when its owner says so. Deleting an environment never
migrates anything. It classifies every session, gives people time to save, and archives the
sessions. To continue elsewhere you **fork**. Forge accounts come through per-user
**Connections**. **Model aliases** let agent definitions and helper slots name a group of
models instead of a provider-specific id.

Terms used below:

- **Workspace node**: a node attached to one or more workspaces. Being attached to a
  workspace is what makes it trusted there: there is no separate trust mark.
- **Attachment**: one (workspace, node) pair, made by one pair code, recorded on the master
  (`workspace_nodes`) and on the node (its attached list). A session may run only where both
  records agree.
- **Attached list**: the node's own file of the workspaces it serves (§4.10.1).
- **Data root**: the folder on the node that belongs to one attached workspace: its
  environments, caches and checkpoints (§4.10.3).
- **Shared node**: a node with more than one attachment.
- **Host session**: a session whose shell runs on the node's machine under Landlock, not in a
  container.
- **Pair code**: a short, single-use code the node shows and a workspace owner or admin types
  into the master's UI to attach that node to that workspace.
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
| P6 | No isolation beyond Landlock: the shell runs on the host as the node user. | `Confinement` is `Unconfined`, `Confined` or `Refused` (`gate3:crates/tools/src/sandbox.rs:279-292`); residuals: pathname Unix sockets before ABI 9, UDP, DNS (`:17-26`). |
| P7 | No forge credentials exist; "Connections" is a placeholder. | `web/src/components/Sidebar.tsx:129-130`. |
| P8 | No archived or read-only session state, no fork. | `sessions` columns (`crates/store/src/lib.rs:224-244`); read-only is only computed from grants and roles (`crates/master/src/workspaces.rs:429`). |
| P9 | Nothing on a node knows which workspace a session belongs to, so two workspaces on one node are not separated at all. | `session.create` carries no workspace (`gate3:crates/protocol/src/lib.rs:131-150`); the cwd comes from the client (`gate3:crates/master/src/http.rs:1228`) and is accepted anywhere under any `workspace_roots` entry, `$HOME` by default (`gate3:crates/node/src/policy.rs:251-276`, `:166`); caches in `[sandbox] write` are read-write to every session (`gate3:crates/tools/src/sandbox.rs:215-217`); full findings in §4.10.2. |

## 2. The decisions, with their costs

| Decision | Cost |
|---|---|
| **Podman** for session containers and self-hosting; Docker only for self-hosting. | Rootless Podman needs setuid `newuidmap`, `/etc/subuid` and a user runtime directory; the README's unit (`NoNewPrivileges=yes`, `nologin` user, `README.md:706`, `:742-749`) must change. Docker hosts get no container sessions. |
| **Chat-model keys per workspace (BYOK)**, no global store, nothing copied. Onboarding writes its first provider into the first admin's Personal workspace. **R1 (reading, to confirm):** the `HARNESS_PROVIDER_*` settings seed that Personal workspace. | `providers`, `default_model`, `[agent_models]` (`config.rs:16-24`) and `HARNESS_DEFAULT_MODEL` (`crates/master/src/env.rs:35`) go; Personal workspaces gain settings; notices and the model cache go per workspace. `.env` seeds apply at config load (`env.rs:1-15`), before any account exists, so R1 needs a seed deferred to the claim (§4.4). |
| **Embeddings and voice are harness (server-wide) settings** with their own endpoints. They never reuse a workspace provider. Onboarding offers Ollama (the default) or a custom API endpoint for embeddings, and no "reuse the provider you just added". | Voice already has its own URL and key (`config.rs:236-243`). Embeddings lose `source = "provider"` and `provider_id` (`config.rs:285-292`, `:304`) and the onboarding path that reuses the provider. |
| **A1 (to confirm):** title generation, summarisation and the vision bridge use the workspace's models. | Summarisation already uses the turn's model on the node (`crates/agent-native/src/lib.rs:1058`); I found no title generator or vision bridge on `main` (`crates/master/src/lib.rs:229-235` refuses instead): slots to add. |
| **Every workspace node is trusted by each workspace it is attached to and may receive that workspace's grants.** Attaching it (an owner or admin types its pair code) is the act of trust. A grant carries only the keys the turn needs, held in memory only, kept out of the command log and redacted, usable only by a session of the workspace that owns the keys. A restarted node needs the master before it can run a turn. `node.toml` may refuse grants or use local keys. **No proxy now**, but the hand-over is a grant that a proxy can fill later. | A node's operator can read the keys its turns use, for every workspace it serves; a stolen node key yields the API keys of every attached workspace; detaching cannot un-send keys (the UI lists which to rotate). |
| **No long-lived secret in a shell command**; the node generates its own key pair; the private key is kept under systemd encrypted credentials, else in a 0600 file. **A2 (to confirm):** "PAM" read as systemd encrypted credentials. | `systemd-creds encrypt` needs root, so the systemd path is a second, root step (§4.4). |
| **A3 (to confirm):** an offline node finishes its running turn; a new turn waits. | Grants must outlive the longest turn (§4.3). |
| **Nodes are attached to workspaces, one or several** (owner, 2026-11-11), and carry their own permissions (host shell, containers). One key, one link, one link generation per node, whatever the number of attachments. | Reverses "per-workspace node access ... dropped ... could be bypassed through Personal" (`PLAN.md:1765-1768`); the bypass is closed by a workspace check at create and before every prompt, on the master and on the node (§4.10.1). Two workspaces now share a machine, a Unix user, a node process and a link: separation has to be built and tested (§4.10.3), and on the host it has holes the sandbox cannot close. One node per workspace would have had that separation for free. |
| **Private per-user node** (own confirm code, model access by proxy) is experimental and comes last; restricting by source IP was dropped. **Landlock stays**; containers are a further `Confinement` kind. | Two execution paths to test and document. |
| **Environments**: the volume is kept; the node builds the invocation; the image comes from an allow-list, plus a setup script; one worktree per session; files are shared within the workspace, chats stay private. | File tools run in the node process and the shell runs in the container, so paths and file ownership must agree (§4.6). Shared files change T10 (§5). |
| **Deletion**: no migration. Sessions are classified (a)/(b)/(c); other users are named, with their counts; autosave is optional, the deleter's choice. Anything unsafe forces a schedule (7 days by default, at least 1). Sessions are then archived; to continue, fork. **A4 (to confirm):** nothing unsafe may be deleted immediately. **A5 (confirmed):** a repository with no forge remote is class `c`. | Needs Connections (P7), a git path with network access (forbidden today, §4.7), archive and fork (P8). The names and counts in the dialog and autosaving colleagues' files need T10 changed (§5). My dissent is recorded in §9. |
| **Forge accounts through Connections, per user**, plus one workspace account added by admins. The workspace account makes the co-author line link properly. | OAuth needs a registered app per forge and a callback a loopback master often cannot receive. Users have no e-mail today (`users`, `crates/store/src/lib.rs:303-312`), which the plain fallback needs (§4.7). |
| **Model aliases** per workspace: a value with `::` is a concrete model, anything else an alias, never `@`. Named by capability or family; checked by capability. | "Alias" already means an agent handle on `feat/agent-roster` (`crates/agents/src/lib.rs:9` there), so always say "model alias". |

## 3. What does not change

The node dials out over TLS to a pinned certificate and signs a nonce bound to it
(`crates/link/src/identity.rs:60-64`); a node joins only once per key, in one transaction
(`crates/store/src/lib.rs:867-927`); the event log, replication and hardened git (T19) stay.
A node has one key and one link however many workspaces it serves.

## 4. Design

### 4.1 Data model (master.db; changed in place, no migrations before the first tag)

```sql
-- nodes: no workspace and no trust column. An attachment is the trust.
ALTER nodes ADD link_gen INTEGER NOT NULL DEFAULT 0;        -- C.1, bumped on accept
DROP TABLE join_tokens;                      -- replaced by pair codes (memory only, §4.4)

CREATE TABLE workspace_nodes (               -- one row per attachment
  workspace_id TEXT NOT NULL REFERENCES workspaces ON DELETE CASCADE,
  node_id      TEXT NOT NULL REFERENCES nodes ON DELETE CASCADE,
  added_by     TEXT NOT NULL,                -- user id; text, the user may be deleted later
  added_ms     INTEGER NOT NULL,
  label        TEXT,                         -- this workspace's own name for the node
  state        TEXT NOT NULL,                -- pending_node | active | detaching
  PRIMARY KEY (workspace_id, node_id));
CREATE INDEX workspace_nodes_by_node ON workspace_nodes(node_id);

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
  first_ms INTEGER, last_ms INTEGER,
  PRIMARY KEY (node_id, workspace_id, secret_name));   -- two workspaces may both name "provider-openai"
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
  env_id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, node_id TEXT NOT NULL,
  name TEXT NOT NULL, image TEXT NOT NULL,   -- allow-list name
  repo_url TEXT, setup TEXT NOT NULL DEFAULT '', created_by TEXT, created_ms INTEGER,
  state TEXT NOT NULL,                       -- active | scheduled | deleting | gone
  FOREIGN KEY (workspace_id, node_id) REFERENCES workspace_nodes ON DELETE RESTRICT);
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

What cascades and what does not:

| Event | `workspace_nodes` | Sessions of that pair | Environments |
|---|---|---|---|
| Detach (§4.10.5) | Row goes to `detaching`, deleted when the pair's environments are gone | Archived, epochs bumped (C.1), grants dropped | §4.8 flow scoped to the pair; `RESTRICT` keeps the row until it ends |
| Node revoked | Every row of the node deleted in the revoke transaction (the `nodes` row stays with `revoked_ms`, so `CASCADE` does not fire) | Same, for every workspace | Same, for every workspace; "forget" if the node is dead |
| Workspace deleted | Refused (409) while any of its environments exist; otherwise every attachment is detached first, then the workspace row goes and `CASCADE` removes what is left | Already archived by the detaches | None exist |

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
- **Workspace check (master).** Create and every prompt, approval, restore and set-agent
  require an `active` row in `workspace_nodes` for the chat's effective workspace
  (`EFFECTIVE_WS`, `gate3:crates/store/src/lib.rs:381-387`) and the session's node, and that
  this workspace equals the one the session was created in. A member who leaves falls back to
  Personal; their chats on the team's nodes then fail the check and become blocked, the way
  `agentic_blocked` works (`gate3:crates/master/src/workspaces.rs:429`). The node checks again
  (§4.10.1).
- **Who may do what.** Attach and detach nodes, create and delete environments, and manage
  providers, model aliases and the workspace forge account all need `settings` in that
  workspace (owner and admin, `workspaces.rs:74-82`). Using a node or environment needs
  `agentic` plus the account grant (T11). Revoking a node outright is a server admin's act
  (it ends every workspace's use of it). Users manage their own Connections.
- **What a workspace's admins see of a node:** its label, hostname, key prefix, connection
  state, capabilities, this workspace's share and this workspace's sessions and environments.
  Never the other attachments, their count, their sessions, environments, grants or load. A
  server admin sees every node with its attachment count, not the other workspaces' contents.

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
"session.grant" { req_id, session_id, workspace_id, grant_id, expires_ms,
                  routes: Vec<{ provider_id, access: ModelAccess }> }
enum ModelAccess { Direct { kind, base_url, key: Secret<String> },
                   Proxy  { url, token: Secret<String> } }   // reserved, not built
"session.prompt" { ..., workspace_id, grant_id: Option<String> }   // additive fields
"grant.drop" { grant_id }                                   // turn ended, node detached
```

- **Carries:** only the providers the turn can reach: the agent's chain, each delegate's
  chain, `compare` models, and the `summary` and `vision_bridge` slots. The master's
  `Resolver` works these out from the agent spec it already builds, for the session's
  effective workspace only.
- **Never carries:** another workspace's keys, a forge token (sent separately, §4.7), or
  anything that persists. A grant never goes into an event (it is not part of `AgentSpec`).
  The node's audit row stays kind plus session id (`crates/node/src/lib.rs:303-311`).
- **Master side:** sent only to the session's node, only while that node has an `active`
  attachment to the session's workspace, is connected, is not revoked, and announced
  `credentials:master` in `node.hello` (`protocol/src/lib.rs:74-83`). Each send updates
  `node_key_grants` under `(node, workspace, secret)`.
- **Node side, keyed by workspace.** `[credentials] model_keys` decides (§4.5). With
  `"master"` (the default) the node accepts a grant only if its `workspace_id` equals the
  workspace the node recorded for `session_id` at create, and that workspace is on its
  attached list. Keys go into an in-memory map keyed by `(workspace_id, provider_id)` and
  bound to the grant's session (C.3 adds epoch and turn), and into the `SecretSet`. At
  `session.prompt` the turn's router is built only from grants of that session; a grant of
  workspace A can never be looked up for a session of B, because the lookup key includes the
  session's own recorded workspace, not the one the message names. Keys are dropped on
  `grant.drop`, at `expires_ms` (C.3 and O10 set the lifetime), on detach of that workspace,
  and on link loss once the running turn ends (A3). With `"local"` or `"none"` the node does
  not announce the capability, and it refuses a grant with an ack error. The backend takes a
  router per turn instead of one provider for the whole node
  (`crates/cli/src/main.rs:451-466`). This is the main code change on the node.
- **Never in a child's environment.** Grants live in the node process only. The child
  environment is built by `env_clear()` plus an allow-list (`gate3:crates/tools/src/env.rs`,
  PLAN.md T3), and a grant is never an environment variable of the node itself, so no
  allow-list entry can pass it. The `SecretSet` is node-wide: B's outputs are also scrubbed of
  A's keys, which costs nothing and tells B nothing (the scrubber's marker names no key).
- **Removing a node or detaching a workspace:** a `grant.drop` for each live grant in scope,
  then the existing link drop for a revoke (`gate3:crates/master/src/nodes.rs:57`). The
  dialog then lists every key in `node_key_grants` for that workspace (for a revoke, each
  workspace's admins see their own) and says: "rotate these keys at the provider; this node
  may still hold them".
- **The embedded `local` node** runs in the master process; it gets the same per-turn router
  in-process, with nothing on the wire, and the same workspace keying (§4.10.6). The master's
  chat runtime is not a workspace node: it is unchanged by this note except that it uses the
  chat's workspace's providers.

### 4.4 Attaching a node with a pair code

**The node's owner starts it on the node; a workspace owner or admin types the code the node
shows; the node's owner confirms; only then does the node write the attachment.** Each
attachment is a separate run of this flow. The first one also registers the node's key.

1. An owner or admin opens the workspace's Nodes page and chooses "Attach node". The UI shows
   a command that carries no secret: `rizzy-harness node pair wss://host:port --fingerprint …`.
   It also shows an input for the pair code, and keeps the dialog open.
2. On the node, `node pair` loads the node's key pair, or generates it on the first pairing
   (`identity.rs:14-38`). It connects over TLS to the pinned fingerprint and sends
   `Hello::Pair { public_key, hostname, signature }`, signing the master's nonce with the
   domain tag it uses now (`gate3:crates/link/src/wire.rs:24-43` holds today's `Join` and
   `Auth`). A pairing connection is never a link: it does not replace the running `node run`
   link (today a second connection does, `gate3:crates/master/src/nodes.rs:209`) and does not
   bump `link_gen`. The master keeps the request in memory and answers with a pair code: 8
   characters from a 32-letter alphabet (40 bits), valid 10 minutes. The node prints the code
   and the first 16 hex characters of its key.
3. The admin types the code into the dialog. The master burns the code first (C.5), then
   matches it to the pending request. It inserts the node row if the key is new, and a
   `workspace_nodes` row for the dialog's workspace with `added_by` and state `pending_node`.
   The UI shows the key prefix for comparison. The master sends `Paired { node_id,
   workspace_id, workspace_name }` on the still-open connection.
4. The node's CLI shows `attach this node to workspace "<name>" (<id>) on <master
   fingerprint>? [y/N]` and the node's current attached list. On yes it adds the entry to its
   attached list (§4.10.1) and sends `PairConfirmed`; the master sets the row `active`. On no,
   a closed connection or 10 minutes without an answer, the master deletes the row. The node
   checks `node.toml [attach]` (§4.5) before asking; a refusal there ends the flow with the
   reason printed on the node and one opaque failure in the dialog.
5. Properties: single use; 10 minutes; bound to the workspace whose authenticated admin
   types it; needs the node owner's yes on the node; no secret in any shell command; the key
   pair is the node's own. Limits: 5 wrong codes per dialog close it, at most 32 pending pair
   requests in all, at most 4 per source address. Codes are compared as hashes in constant
   time.
6. Key at rest. With `--systemd-creds` the node writes the PKCS#8 bytes only to stdout,
   piped into `systemd-creds encrypt --name=node.key - /etc/credstore.encrypted/rizzy-harness-node.key`
   run as root, and the unit gets `LoadCredentialEncrypted=node.key`. `node run` then reads
   `$CREDENTIALS_DIRECTORY/node.key`. Without systemd it is today's 0600 file
   (`crates/link/src/lib.rs:58-75`).

**What a replayed, guessed or misdirected code gets.**

| Attempt | Result |
|---|---|
| A code typed a second time | Already burned: the one opaque failure ("code not valid"), counted against the dialog. |
| A guessed code (40 bits, 5 tries per dialog, 10 minutes) | Not a practical attack. If it did hit, it would bind someone else's pending node to the guesser's workspace on the master only: the node's owner sees the wrong workspace name at step 4 and says no. |
| A compromised master that binds the pairing to another workspace | Same: the node's owner sees the name. A compromised master can lie about the name; it already holds every key and chat (T6), so this adds nothing. |
| A master that adds a `workspace_nodes` row with no pairing | The node refuses every session for that workspace: it is not on the node's list (§4.10.1). |

**What a second workspace's admin learns from pairing: nothing about the first.** The pairing
answer and the Nodes page carry only what any fresh node shows: hostname, key prefix,
capabilities. The master does not say that the key is already known, and the node id is not
shown in the UI (each workspace names the node with its own `label`). The node's CLI shows the
full list to the node's owner only, on the node. Residuals: the hostname is the same in every
workspace; the share (§4.10.4) is shown as an absolute number, from which the count of
workspaces can sometimes be guessed; a member of both workspaces can match the two entries by
key prefix. A user's agent on a shared host session can see other workspaces' processes in
`/proc` (§4.10.2): that is not pairing, and it is why host sessions are off on a shared node.

Code impact: pairing holds a connection for minutes, but today's handshake has a 10-second
timeout and one shared 32-slot semaphore (`gate3:crates/master/src/nodes.rs:17-21`), so
pairing gets its own. `join_tokens`, `create_join_token` and `POST /api/admin/nodes/join` go.
`node join` becomes `node pair`, and `node detach` and `node workspaces` are new (§4.10.5).
Separately (R1), `HARNESS_PROVIDER_*` is applied to the first admin's Personal workspace when
the claim creates it.

### 4.5 `node.toml` additions

```toml
[attach]
max_workspaces = 4           # pairing beyond this is refused on the node
allow = []                   # workspace ids; non-empty means only these may be attached
workspace_data = "/home/rizzy-harness/workspaces"   # data roots live here, one per workspace

[execution]
host_shell = true            # sessions in a host folder, under [sandbox] (Landlock)
host_shell_shared = false    # host sessions while attached to more than one workspace (§4.10.3)
containers = false           # sessions in environments

[limits]                     # beside today's tool_stream_bytes, max_steps, bash_timeout_max_s
max_commands = 4             # tool child processes running at once, node-wide (§4.10.4)
share = "borrow"             # "borrow" | "strict"

[credentials]
model_keys = "master"        # "master": accept grants (memory only)
                             # "local": use this node's own keys file, refuse grants
                             # "none": refuse grants; no model calls here
local_keys_file = ""         # 0600, operator-managed; only with model_keys = "local"

[containers]
runtime = "podman"           # the only value; rootless is required, not a setting
max_environments = 8         # node-wide; shared between workspaces by the strict rule
max_running = 4              # running session containers, node-wide; strict share
network = "on"               # "on" (pasta, no published ports) | "off" (--network=none)
allow_host_network = false   # only true makes network = "host" accepted
memory = "4g"
cpus = 2
pids = 1024
setup_timeout_s = 900
keep_deleted_days = 3        # deleted environments and detached data roots go to a trash first
[containers.images]          # allow-list: name -> pinned reference
rust = "docker.io/library/rust@sha256:…"

[forge]
push = false                 # let the node clone with and push using forge tokens
hosts = ["github.com"]       # the only hosts a forge token may be used against
```

`model_keys` is the node deciding, not a second approval. Being attached to a workspace is the
trust; this setting is what the node will accept. `"local"` is the operator's choice to keep a
key on that machine, against the default of only the link key on disk; on a shared node every
attached workspace's turns then use (and are billed to) that one key, and `node pair` says so.
As T5 says, nothing received over the protocol or HTTP widens these settings. These are
refused with an ack error, each with a test (§8): an image not on the list, a network mode the
node does not allow, a grant to a node that is not `"master"`, a session for a workspace not
on the attached list, a host session on a shared node without `host_shell_shared`.

What `node.toml` can and cannot be overridden by: the attached list can only hold workspaces
`[attach]` allows. If `node.toml` is edited to allow fewer than are attached, the node refuses
to start and names the attachments to detach, rather than choosing which to drop. Nothing the
master sends, and nothing in the attached list, raises a value here; an attachment entry may
only narrow (its own host roots, §4.10.3). `workspace_data` must pass `check_protected`
(`gate3:crates/node/src/policy.rs:303-334`) and must not lie under a folder the sandbox grants
every command (`SYSTEM_READ`, `gate3:crates/tools/src/sandbox.rs:48-50`, or `[sandbox]
read`/`write`). `environments_root` of the first version is replaced by
`<workspace_data>/<workspace>/environments` (§4.10.3).

### 4.6 The container the node builds

One container per session, on the environment's volume: processes, `/tmp` and signals stay
apart, and the files are shared by decision within the workspace. Nothing from the master,
the project or the repository enters the arguments except allow-list values:

```
podman run --detach --name rh-<env>-<session> --label rizzy.env=<env> --label rizzy.ws=<ws>
  --userns=keep-id --user <node uid>:<gid>     # same uid inside and out
  --cap-drop=all --security-opt=no-new-privileges
  --read-only --tmpfs /tmp:size=1g,mode=1777 --tmpfs /run
  --network=pasta | none                       # host only if allow_host_network
  --memory=<m> --memory-swap=<m> --cpus=<n> --pids-limit=<p>
  --mount type=bind,src=<wsroot>/environments/<env>,dst=<wsroot>/environments/<env>,relabel=private
  --workdir <wsroot>/environments/<env>/worktrees/<session>
  --env HOME=<wsroot>/environments/<env>/home --env <allow-listed names only>
  --init --pull=never --log-driver=none
  <image@sha256:…> sleep infinity
```

`<wsroot>` is `<workspace_data>/<workspace>` (§4.10.3). The only mount is one environment of
the session's own workspace; the node checks, before `podman run`, that the mount source is
inside the session's workspace root and inside no other.

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
compose files are never read. Image layers are shared between workspaces on the node; they
are read-only and pinned by digest, so sharing them leaks only which images exist.

**Worktrees and checkpoints.** `<wsroot>/environments/<env>/` holds `repo.git` (bare clone),
`worktrees/<session>` (branch `rizzy/<session-short>`, not detached like sub-agents,
`git.rs:866-876`), `cache/` and `home/`. The shadow repository is keyed by work-tree path
(`gate3:crates/node/src/git.rs:568-572`) under a per-workspace checkpoints folder
(§4.10.3), so checkpoints, pruning and undo are per session; the no-checkpoint rule for
`.git/harness-worktrees` (`git.rs:536-540`) keeps covering sub-agents only, which isolate
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
`gate3:crates/tools/src/sandbox.rs:44-50`). It arrives in a `forge.grant` (one host, one
operation, one session, so one workspace), only with `[forge] push = true` and the host in
`[forge] hosts`.

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
`env.saved`. Each names its workspace, and the node refuses one whose environment is not in
that workspace's data root. `node.hello` lists the node's environments by workspace so the
master can reconcile. `env.classification` carries, per session: id, class, reason codes
(`dirty`, `untracked`, `unpushed`, `stash`, `submodule`, `detached`, `in_progress_op`,
`forge_unreachable`, `no_remote`) and counts. It never carries file names or content.

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

**Detaching a workspace from a node** that hosts environments of that workspace first
schedules deletion of each of them (§4.10.5); **revoking a node** does it for every workspace.
The attachment is removed when its last environment is gone or forgotten.

### 4.9 Fork

"Continue elsewhere" is for the session's owner only. The master creates a new session and
sends the old history in `session.seed` batches: user and assistant text, each tool call cut
to one line, at most 2 MiB per batch. The new node writes them as `history_imported` events in
its own log, and `session_forks` links the two sessions. Files come back from the forge (the
autosave branch, if there is one); a fork copies no files. The new session's node must be
attached to the session's workspace; a fork never changes a session's workspace.

### 4.10 A node in several workspaces

#### 4.10.1 The attached list and the checks

**The node decides which workspaces it serves.** `<data>/attachments.toml`, 0600, in the node's
data directory, which no session may include and no tool may touch
(`gate3:crates/node/src/policy.rs:288-334`, PLAN.md T10):

```toml
[[workspace]]
id = "ws-…"                  # not a secret
name = "Team X"              # as the master named it at pairing; display only
paired_ms = 1762819200000
master = "sha256:…"          # the master certificate it was paired with
host_roots = ["/srv/code/team-x"]   # optional; may only narrow (§4.10.3)
```

| Rule | Why |
|---|---|
| Only `node pair` adds an entry, after the node owner's yes (§4.4). | Widening is the node owner's act on the node, never the master's. |
| `node detach <id>` removes one, offline if need be (§4.10.5). The running node may also remove an entry when the master reports a detach (a narrowing). **Recommendation** (Q3). | A detach from the master's side must take effect on the node without waiting for its owner. Removing is narrowing, so it is allowed; adding never is. |
| Writes go through a lock file and an atomic rename; `node run` re-reads the file when its modification time changes, at each check. | `node pair` runs while `node run` runs; no restart is needed to attach. |
| A missing or unparseable file means "no workspace": every session is refused, with the reason. | Fail closed. |
| The node announces its list in `node.hello` (`workspaces: [id…]`). | The master reconciles; it never adds an attachment from it. |

**Where the workspace id lives in the protocol.** `session.create` gains a required
`workspace_id` (beside `gate3:crates/protocol/src/lib.rs:131-150`); `session.prompt`,
`session.grant`, `env.*` and the approval, restore and set-agent commands carry it too. It is
not a secret: it is an opaque id that names a workspace, like `session_id`. The node stores it
in its own session row in `node.db`, not in an event, because T10 keeps the workspace mapping
master-side and the node must not assert it (T7).

**The checks.**

| Where | When | Check |
|---|---|---|
| Master | create, prompt, approval, restore, set-agent | An `active` `workspace_nodes` row for (effective workspace, node), and the effective workspace equals the session's (§4.1). |
| Node | create | `workspace_id` is on the attached list; the cwd or environment is inside that workspace's root (§4.10.3); host sessions only as §4.10.3 allows. Stores the id. |
| Node | every later command for the session | The command's `workspace_id` equals the stored one, and the stored one is still on the list. A mismatch is refused and logged: it means a master bug or a lying master. |
| Master | `node.hello` | Attachments active on the master but missing from the node's list are shown as "the node no longer serves this workspace" and treated as a node-side detach (§4.10.5). Entries on the node's list with no master row are ignored, and the node is told so (`attachment.unknown`) so `node workspaces` can show it. |

**One of each per node.** One key, one link, one link generation (C.1). All attached
workspaces' sessions travel on that link, each command naming its workspace. A reconnect
resumes every session the node holds, whatever its workspace; the master resumes only those
whose attachment is active and fences the rest (C.1's epoch).

#### 4.10.2 What one session can reach of another workspace's today

Established by reading `integration/gate3`, for two workspaces' sessions on one node as the
code stands (one Unix user, `[sandbox] mode = "required"` enforced):

| Path | What A reaches of B today | Evidence |
|---|---|---|
| Choosing the folder | **Anything.** A session may be created in any folder under any `workspace_roots` entry, `$HOME` by default; the cwd is whatever the client sent. A can simply open B's project folder. | `gate3:crates/node/src/policy.rs:14-15`, `:166`, `:251-276`; `gate3:crates/master/src/http.rs:1228` |
| Shell (Landlock) | Not the whole filesystem. Read-write: the session's cwd, its own temp folder `<data>/sandbox-tmp/<session>` (0700), `[sandbox] write` and five device files. Read-only: `/usr /bin /sbin /lib* /etc /opt /proc` and `[sandbox] read`. So B's folder is unreachable unless it lies inside A's cwd, under one of those system folders, or under `[sandbox] read`/`write`. `/tmp` is not writable. | `gate3:crates/tools/src/sandbox.rs:11-15`, `:44-61`, `:203-226`, `:337-397`; `gate3:crates/cli/tests/sandbox.rs:167-188` |
| Shared caches | **Read-write for every session of every workspace**: whatever `[sandbox] write` lists (`~/.cache`, `~/.npm`) is one folder for all. A poisons B's build cache. | `gate3:crates/tools/src/sandbox.rs:215-217`; `gate3:crates/node/src/policy.rs:65-66` |
| `/proc` | Readable. Other domains' `environ`, `fd`, `mem`, `cwd` are denied (ptrace-class); tested only against the harness process itself. `cmdline`, `status` and `stat` of B's processes are not ptrace-gated, so A sees B's command lines. Not tested. | `gate3:crates/tools/src/sandbox.rs:44-47`; `gate3:crates/cli/tests/sandbox.rs:231-253` |
| Signals | Denied outside the domain only with Landlock ABI 6 (best-effort); on an older kernel A can kill B's processes (same uid). Tested against the harness only. | `gate3:crates/tools/src/sandbox.rs:22-23`, `:614-619`; `gate3:crates/cli/tests/sandbox.rs:402` |
| Unix sockets | Before ABI 9, A can connect to any pathname socket, including B's and the session bus, which starts programs outside the sandbox: a full escape to the node user. | `gate3:crates/tools/src/sandbox.rs:23-26`; gate3 `PLAN.md` T10, T11 (lines 463-464); `gate3:crates/cli/tests/sandbox.rs:371` |
| Loopback network | With `network = "on"` (the default) A connects to whatever B's sessions listen on at `127.0.0.1`. With `"off"`, TCP is denied, UDP is not. Not tested between sessions. | `gate3:crates/tools/src/sandbox.rs:76-85` |
| `read`/`grep`/`glob`/`write`/`edit` | Run in the node process, not under Landlock. Confined to the session's cwd by a path walk; with `allow_outside_workspace = true` (node-wide) they reach every path the node user can, except the data directory, so all of B. | `gate3:crates/tools/src/lib.rs:204-252`; `gate3:crates/node/src/policy.rs:17-19` |
| Checkpoints | Shadow repositories `<data>/checkpoints/<hash of folder>.git`, node-wide and keyed by folder: sessions of A and B in one folder share one, and A's restore rewrites B's files (P5). Not reachable from the shell. | `gate3:crates/node/src/git.rs:568-572`; `gate3:crates/cli/src/main.rs:525` |
| Git worktrees | A sub-agent worktree's shell gets read-write on the repository's `.git`. Cross-workspace only when the folder is shared. | `gate3:crates/tools/src/sandbox.rs:365-372` |
| Other node state | `tool_output` is in memory per session with random ids; graphify output is keyed by folder hash (node-wide); the artifact stage is one node-wide folder inside the data directory, and a fetch looks a stage up by session and stage id together. None is reachable from the shell. | `gate3:crates/tools/src/outputs.rs:1-5`; `gate3:crates/tools/src/graphify.rs:48-53`; `gate3:crates/node/src/artifacts.rs:52`, `:790-814` |
| Containers | Not built. Planned: one bind mount of one environment, own PID, mount and network namespaces, same uid as the node (`keep-id`). Whether rootless `pasta` lets a container reach the host's loopback is not established (test D14, C.6). | §4.6 |
| Capacity | No node-wide cap on running turns or commands: one lane per session, any number of lanes. The link's queues are shared (64 in, 256 out on the node). | `gate3:crates/node/src/lib.rs:40-49`, `:282-330` |

**What that means.** The largest hole is not the sandbox: it is that nothing ties a folder to a
workspace. The sandbox itself is an allow-list and already keeps a session out of folders it
was not given; what it shares between workspaces is the caches, `/proc` metadata, the loopback
network, and, before ABI 9 (Linux 7.1, per `research/round2/synth/05-links-workers-protocols.md`
§6.5), an escape to the node user through the session bus.

#### 4.10.3 Separation: decided

**A data root per workspace.**

```
<workspace_data>/<workspace_id>/          0700, created at pairing, removed at detach (to trash)
  environments/<env>/                     volumes (§4.6)
  cache/                                  this workspace's caches (replaces [sandbox] write)
  home/                                   HOME for host sessions on a shared node
<data>/checkpoints/<workspace_id>/<key>.git   shadows, per workspace, inside the data dir
<data>/sandbox-tmp/<session>              unchanged (per session already)
```

Host session folders: an attachment's `host_roots` (set by `node pair --root`, each inside a
`workspace_roots` entry of `node.toml`). The node refuses, at pairing and at start, two
attachments whose roots overlap (one equal to or inside another), a root under a folder the
sandbox grants every command, and a root that contains the data directory or
`workspace_data`. A host session's cwd must be inside its own workspace's `host_roots`. Host
roots are the operator's folders and are never deleted by the harness.

**The sandbox profile.** Landlock only adds rights, and a right on a folder covers everything
below it; there is no deny rule. So "A cannot reach B's root" is built, not configured: the
profile for a session of A grants exactly A's cwd (inside A's root), A's session temp folder,
A's `cache/` (and `home/` on a shared node), the device files, `SYSTEM_READ` and `[sandbox]
read`, and before applying it the node checks that no granted path equals, contains or is
inside the data directory or **any other attached workspace's root**. This is today's
`exposes` check (`gate3:crates/tools/src/sandbox.rs:132-140`) with the protected set
extended by the other roots, computed per session. On a shared node `[sandbox] write` is
refused at start (it would be one folder for all workspaces); its entries become per-workspace
`cache/` with `XDG_CACHE_HOME` pointing there. `[sandbox] read` stays shared: it holds the node
owner's toolchains, read-only, and must not overlap any root.

**File tools.** `read`, `grep`, `glob`, `write`, `edit` run in the node process and are not
under Landlock in any mode, the container mode included. Their separation is the path walk in
`ToolCtx::resolve` (`gate3:crates/tools/src/lib.rs:217-252`), extended so that every other
workspace's root is refused as the data directory is (`refuse_protected`). On a shared node
`allow_outside_workspace = true` is refused at start: it would let A's tools read all of B.

**Host sessions on a shared node: refused by default. Recommendation (Q1).** A node with more
than one attachment refuses host sessions unless `[execution] host_shell_shared = true`, and
even then only with `[sandbox] mode = "required"` enforced. With the key set, the node prints
at start, and the Attach dialog shows, what stays open between workspaces under one Unix user:

| Residual for host sessions under one Unix user | Closed by |
|---|---|
| Before Landlock ABI 9, the session bus or any pathname socket starts a program outside the sandbox: A becomes the node user and reads B, every grant in memory included. | ABI 9 (Linux 7.1); or a container. |
| A sees B's command lines and process list in `/proc`. | A container (own PID namespace); `hidepid` on `/proc` is the host's choice. |
| A connects to anything B listens on at `127.0.0.1`; with network off, UDP still goes. | A container with its own network namespace (if D14 shows `pasta` keeps loopback apart); seccomp (O4) for UDP. |
| Before ABI 6, A can signal B's processes. | ABI 6 (Linux 6.12); or a container. |
| A fills the disk, the CPU or the memory B needs. | The share (§4.10.4) for slots; nothing for disk on a plain folder. |
| A bug in the file tools' path walk is a read of B. | Nothing on the host or in a container: file tools run in the node process. Tests in §8. |

Containers are the separated path: B's volume is not mounted, B's processes are in another
PID namespace, and the network is its own. They still share the uid with the node (an escape
from the container is the node user) and the file tools' path walk.

The price of the other answer (host sessions allowed by default on a shared node): every
shared node would carry the residuals above without its owner choosing them, and on kernels
before 7.1 (most kernels in use today; not established per distribution), one prompt-injected agent in any
workspace could read every other workspace's files and keys on that node. The price of this
answer: until environments exist (N.4, months after N.1), a shared node runs host sessions
only for owners who set the key; the common home case (my Personal workspace plus one team on
my own machine) needs that one line. The separation a different Unix user per workspace would
give is not planned: it needs root and a privileged helper on the node.

#### 4.10.4 A fair share of the node

Shared by every workspace on the node: command slots (`[limits] max_commands`, new), running
containers (`[containers] max_running`, new), environments (`max_environments`), the link, the
disk. Turns are not capped: a turn mostly waits for a model.

| Resource | Rule | Bound for B while A is at its share |
|---|---|---|
| Command slots (a `bash` call or other child process), `share = "borrow"` (default, **recommendation**, Q4) | Each attached workspace is guaranteed `max(1, max_commands / n)` slots, n the number of attachments. A free slot may be borrowed by anyone. When a slot frees, a workspace below its guarantee gets it before any workspace at or above it. No command is preempted. | B's command starts when the first running command on the node ends: at most the shortest running command, capped by `bash_timeout_max_s`. |
| Command slots, `share = "strict"` | Each workspace has `max(1, max_commands / n)` slots and never more. | B never waits for A. A alone uses only its share of the machine. |
| Running containers | Strict: `max(1, max_running / n)` each. A container lives for its session and cannot be handed back. | B never waits for A. |
| Environments | Strict: `max(1, max_environments / n)` each. | B never waits for A. |
| The link | Shared, first come. One call streams at most `tool_stream_bytes`; B.7 decides whether control frames need their own lane. | Not bounded per workspace; measured in B.10. |
| Disk | A free-space floor for the whole node (§6). No per-workspace quota on a plain folder. | None: A can fill it, and B's new sessions are refused. Residual. |

`max_workspaces` must not exceed `max_commands` (refused at load), so every workspace has at
least one slot. The caps live in `node.toml` only; the master is told the workspace's own share
in `node.hello` and shows it. What a user sees when their workspace is at its share: the
command's card shows "waiting for a free slot on this node (this workspace may run 2 at a
time)" as a line of the tool's live output, which needs no new event type; the same text
appears when a container cannot start. It never names another workspace or how many there are.

#### 4.10.5 Detach and revoke

**Detach one workspace W from node N.** Allowed to W's owners and admins from the master, and to
N's owner from N's CLI. The CLI path must always work, offline included: it is the machine
owner's right to stop serving a workspace.

| Step | From the master (W's admin) | From the node (`node detach W`) |
|---|---|---|
| 1 | Environments of W on N: the §4.8 flow, scoped to them; scheduled when anything is unsafe. The attachment goes to `detaching`; new sessions of W on N are refused (409). | The CLI runs the classification locally (read-only git) and prints it; the owner confirms. No autosave (no forge grant offline). |
| 2 | At the deadline or at once if all safe: `env.delete` for each, then `workspace.detach { workspace_id }` to N. | The entry leaves the attached list at once; the running node kills W's running commands and refuses W's commands. |
| 3 | N removes W from its list, kills W's commands, drops W's grants, moves W's data root to the trash. | W's data root goes to the trash. At the next hello the master sees W missing and does the master side: archive, bump epochs, drop the row. |
| 4 | The master archives W's sessions on N, bumps their `owner_epoch` (C.1), drops the row. | |

In both, the other workspaces' sessions on N keep running: the link stays, `link_gen` is not
bumped, and nothing outside W's sessions, grants and data root is touched. A late append from
one of W's sessions is refused by the epoch.

**Revoke N.** A server admin, or N's owner by `node leave`. Every attachment is detached as
above, the key is marked revoked, `link_gen` bumps and the link drops. The master revokes N by
itself when it flaps more than 3 times in 10 minutes (§6), which stops every workspace's use of
it at once. **Recommendation (Q5):** the last detach also revokes the node, so no pinned key
outlives every attachment; `local` is the exception (§4.10.6).

**What survives on N's disk.** W's data root (environments, caches, `home/`) in
`<data>/trash/` for `keep_deleted_days` (3 by default), then removed. W's sessions in `node.db`
are deleted with it; the master's mirror keeps their history, archived and read-only. W's
checkpoints go with the data root. Host roots, the operator's own folders, are never deleted.
After a revoke the node's key file stays until its owner deletes the data directory.

#### 4.10.6 The `local` node

`local` is an ordinary node under these rules, with one fact that no rule removes: it runs
inside the master process, as the master's OS user, beside `master.db`, the key-encryption key
and, once BYOK exists, every workspace's provider keys in memory
(`gate3:crates/cli/src/main.rs:267-284`, `:472-532`). That is why it is admin-only today
(gate3 `PLAN.md` T11, line 464; `[nodes] local_node_users`, `gate3:crates/master/src/config.rs:149`;
`gate3:crates/master/src/workspaces.rs:402-421`).

- **At first start** it is attached to the first admin's Personal workspace, when the claim
  creates it (`ensure_personal_workspaces`, `gate3:crates/cli/src/main.rs:263`).
- **Its attached list** is `attachments.toml` in the master's data directory. The master
  process could write it, since master and node are one process: "the node decides" holds for
  `local` only as far as nothing over HTTP or the protocol writes it. **Recommendation (Q2):**
  further attachments of `local` are made only by `rizzy-harness node attach-local <workspace>`
  run on the server's machine as the server's user, never from the web UI. The UI shows that
  command. This keeps the invariant literal, at the price of a shell on the server for that one
  act.
- **Host sessions on `local`** stay what T11 says: server admins only (`local_node_users =
  "admins"`), whatever workspace the chat is in. In a team workspace, non-admin members may use
  `local` for **container sessions only**, once N.4 exists.

What attaching `local` to a team workspace exposes: every agentic member's shell would sit, on
the host, one Landlock rule away from every workspace's keys, every chat and the key that
unseals the rest, with the session bus escape open before ABI 9 and the file tools running in
the master's own address space. In a container, the shell is in another mount and PID
namespace without the data directory; the file tools remain in the master process, guarded by
the path walk. Options and their cost:

| Option | Cost |
|---|---|
| **Allowed for containers only; host sessions server-admin-only (recommended)** | Until N.4, `local` in a team workspace is usable only by server admins. |
| Allowed with a warning, host sessions included | Every agentic member of that team is a server admin in effect: T11 is abandoned. **Needs the owner's explicit yes and a `PLAN.md` change.** |
| Refused outright for any workspace but server admins' Personal ones | Simplest; a small install with one machine cannot let a team use it at all, even in containers. |

## 5. Invariants

| Invariant (`CLAUDE.md`) | Effect | Mechanism or change |
|---|---|---|
| The node decides | Kept, and widened in scope | `[credentials] model_keys`, `[execution]`, `[attach]`, `[limits]`, `[containers]` and `[forge]` are local to the node, and the master cannot widen them. Which workspaces a node serves is the node's attached list, widened only by its CLI with its owner's yes (§4.10.1). For `local` this holds only if attachments are made from the server's CLI (Q2). Who may attach is also a master-side rule (owner or admin plus a pair code), and it only narrows. |
| The master never runs tools | Unaffected | Git, forge pushes and containers run on the node. The future proxy would relay model calls, not tools. |
| One writer per session | Kept | Archive and fork links are master-side tables. A fork is a new session, and its own node writes the imported history. Detach fences by epoch. |
| Chats are private to their owner | **Changed, needs the owner's explicit yes when built** | Members using a shared environment see its files. The deletion dialog names other users and their unsaved counts. Autosave pushes other users' files. `PLAN.md` T10 (line 369) must say all three. A shared node adds nothing here: separation between workspaces is about files and keys, not chats, which stay on the master. |
| No secret in an event, URL or log line | Kept | Grants stay outside `AgentSpec` and events. Audit rows hold kind plus id. Forge tokens exist only in git's environment. No shell command carries a secret, the pair code included. The workspace id on the wire is not a secret. |
| Nothing the model writes is trusted | Kept | Setup scripts come from the master. Compose files and the repository's remote config are never read. Slot output is untrusted. |
| `unsafe` forbidden | Unaffected | Podman and `systemd-creds` are child processes. |
| Loopback by default | Kept | Containers publish no ports. `network = "host"` needs `allow_host_network`. On a shared node, loopback is reachable between host sessions (§4.10.3). |
| Harness stays light | Kept | Containers and `conmon` are separate processes. The share scheduler is a counter per workspace. Measure node RSS with 8 containers. |
| First-load bundle budget | Kept | The new pages and dialogs are lazy chunks. Run `pnpm size` in every phase. |

`PLAN.md` lines that change: 341-346 (pairing), 363 (T3), 365 (T5: the attached list), 367
(T7: a node key obtains grants of every attached workspace; attaching needs a pair code and the
node's yes), 369 (T10), 370 (T11: `local` attached per §4.10.6), 373 (T9: the share), 394-396
(secrets file), 427-432 (model registry), 499-502 (R2), 1765-1768 (per-workspace node access),
and §3's message list (319-331). On `integration/gate3` the rows sit at lines 455-484.

**Draft row for `PLAN.md`** (to add when N.1 is built; number assigned then):

| # | Threat | Mitigation |
|---|---|---|
| T-shared | A node shared between workspaces: a session, agent or member of workspace A reaches workspace B's files, keys, processes or capacity on the same machine | A node serves only the workspaces on its own attached list (`<data>/attachments.toml`, widened only by `node pair` with the node owner's yes), and checks every command's workspace against it and against the session's recorded workspace; the master also requires an active `workspace_nodes` row. Each workspace has its own data root; a session's Landlock profile and container mounts are checked to include no other workspace's root, and the file tools refuse other roots as they refuse the data directory. On a shared node `[sandbox] write` and `allow_outside_workspace` are refused, and host sessions are refused unless `host_shell_shared = true`. Grants are keyed by workspace and bound to the session. Command slots, containers and environments are shared per workspace (`[limits] share`). Detaching one workspace archives and fences only its sessions. **Residual:** host sessions under one Unix user (when allowed) see each other's `/proc` metadata and loopback services, can signal each other before ABI 6, and escape to the node user through the session bus before ABI 9; file tools are a userspace path check in every mode; disk is not shared fairly; a stolen node key or a root operator gets every attached workspace's grants; a second workspace may guess the count of workspaces from its share. |

Existing rows that change: T5 (adds the attached list), T7 (a stolen key reaches every attached
workspace), T9 (the share), T10 (workspace roots on a node; T10's privacy changes above are
separate), T11 (`local`).

## 6. How this breaks

- **A hostile repository or prompt-injected agent edits the environment's configuration**
  (`.git/config`, `compose.yaml`, `.devcontainer`). None of these is read. It can still poison
  the shared `cache/` for other sessions of the same workspace (§9); another workspace's cache
  is out of reach (§4.10.3).
- **A node's operator reads the keys.** Root on any workspace node can read the keys its
  turns use from memory, for every workspace it serves. Attaching a node is the admin vouching
  for that machine, and the Attach dialog says so. An operator who does not want this sets
  `model_keys = "none"` or `"local"`.
- **Pair-code abuse.** Anyone who reaches the node port can open pair requests. The caps in
  §4.4 limit this, an admin types only the code their own node shows, the key prefix is shown
  on both sides, and the node's owner confirms the workspace name. Guessing a pending code
  (40 bits, 5 tries per dialog) is not a practical attack.
- **A master that attaches a node by itself.** It can write `workspace_nodes`; the node
  refuses every session for a workspace not on its own list (§4.10.1). For `local`, see Q2.
- **A stolen node key.** The thief can connect as the node and receive grants for turns that
  users of every attached workspace start on it. With C.1 each connection bumps `link_gen`, so
  the two copies kick each other off and the loser's queued frames are dropped. The master
  revokes a node that flaps more than 3 times in 10 minutes and tells every attached
  workspace's admins, who must pair it again. With a TPM, systemd credentials make the key
  useless on another machine.
- **A revoked or detached node still holds keys.** Only rotation fixes this; the dialog lists
  the keys from `node_key_grants`, per workspace.
- **One workspace on a shared node reaches another.** §4.10.2 and §4.10.3: the folder rule,
  the profile check and the tool path check close the files; the residual table is printed when
  host sessions are allowed; containers are the separated path.
- **One workspace starves another.** The share in §4.10.4 bounds command and container waits;
  disk and link bandwidth are not shared fairly.
- **An admin deletes an environment to destroy colleagues' work.** Anything unsafe forces at
  least a day of grace. Everyone affected is notified, another owner can cancel, the node keeps
  a trash, and there are audit rows. Left over: a sole owner can still destroy the work after a
  day. A node owner's offline `node detach` destroys it after `keep_deleted_days`; that is the
  machine owner's right, and the CLI shows the classification first.
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
  are shared within the workspace. Pushes are serialised per environment.
- **Full disk.** A plain bind mount has no quota. Below a free-space floor the node refuses
  new sessions and environments (as with artifacts, T18), and it reserves space for
  classification and autosave. On a shared node one workspace can drive another below the
  floor.
- **Podman missing, rootless not available, or the harness running in a container.** The node
  announces `containers` only after a self-test passes (run, exec, kill, `keep-id` ownership).
  When the harness itself runs in a container (`HARNESS_CONTAINER`,
  `crates/cli/src/net.rs:51-53`, `Dockerfile:33`), container execution is refused.
- **Image supply chain.** Only digests from `node.toml` are used, and a tag is refused at load.
  The UI shows each image's age.
- **Clocks.** Only the master's clock sets deadlines. The only thing the node deletes on its
  own is its trash.
- **A restart mid-detach.** The master's row stays `detaching` and the step runs again at
  start; the node's list is rewritten by atomic rename, so it holds the workspace or not, never
  half.
- **Wrong classification.** Unpushed commits on a branch that exists on the forge are caught,
  because the check is reachability, not whether the branch exists. A stash, a dirty submodule
  or a detached HEAD gives `b`. Files outside the worktree are lost, and the dialog says so.
  Any later change on the forge is caught by classifying again at the deadline.
- **A cancel that does not reach the process inside the container** would break T9. The
  wrapper in §4.6 handles it, with a test.

## 7. What the others do

These are documentation-level findings from `research/`, to be checked by a source-code
round, except where a permalink is given.

| Pattern | Who (source) | Copy or avoid |
|---|---|---|
| Key sent per session, never written | Cline SSH remote (`research/harnesses-round-1.md` §7 "Keeping credentials away from the execution side", pattern 4) | Copy, and add per-turn scope and a rotation list. |
| Inference proxied by the control side | OpenClaw cloud workers, Oh My Pi `auth-gateway`, OpenCodex hub (same section, pattern 3) | Last phase. Our master already sees every event (§7 "The central tension"). |
| Pairing codes; per-dispatch credentials; fencing | OpenCodex one-time pairing code with 429 after failures, OpenClaw device pairing and owner-epoch fencing, Claude Code work orders (`research/round1/synth/09-remote-distributed-architecture.md` §2.0 and §2.4; `research/round2/synth/05-links-workers-protocols.md` §6.1, §6.6) | Copy the pairing limits, burning the code first, and fencing. |
| The executor declares, the hub narrows | OpenCodex: the executor posts its capabilities and root ids and labels (no paths) at pairing, and the hub refuses a later presence claiming more (`05` §6.3; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L427-L463) | Copy: the node's attached list and its host roots are the node's declaration. |
| One worker, one tenant | OpenClaw binds a provisioned box to one environment inside the pairing transaction (`05` §2 "OpenClaw", "Cloud workers" item 1; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/node-enrollment.ts#L85-L106) | The owner chose otherwise; this is the separation we give up. |
| A worker serving several tenants | Not found: `05` §6.1 to §6.6 describe no surveyed project that binds one worker to more than one tenant. | Nothing to copy; our design here is our own and is tested as such. |
| Container floor | OpenHands drops every capability but lets the client override its limits; Hermes skips approvals and checkpoints in containers (`research/round1/synth/02-permissions-sandbox-security.md` F15) | Copy the floor; avoid both mistakes. |
| Environment = image plus setup | Warp (fresh container, idempotent setup), Claude Code (setup snapshot kept about 7 days) (`09-…` §2.5) | Copy the idempotent-setup rule. |
| Moving work between machines | Codex Handoff, Warp handoff; the Claude Code runner "loses unpushed work" (`09-…` §2.0) | Classify, save and fork instead. |
| One "small model" key for several slots | Claude Code (`harnesses-round-1.md` §8 "What we should not copy") | Avoid: an alias is a per-slot default. |

## 8. Build order

Each phase is useful alone; tests are written first and fail before it. Phase 3 shrinks (no
trust mark); old phase 6 splits into Connections (6) and deletion (7); the experimental phase
moves from 7 to 8. On 2026-11-11 phases 1, 3, 4 and 7 grew for several workspaces; the order
is unchanged.

| Phase | What | Tests (abuse cases marked *) | Size |
|---|---|---|---|
| 1 | Workspace nodes with pair codes: `workspace_nodes`, `link_gen`, `Hello::Pair` with the node's confirmation, its own pairing cap and timeout, `join_tokens` removed; the attached list, `node pair`/`detach`/`workspaces`/`leave`; `workspace_id` on every session command, checked on master and node; per-workspace data roots and host roots, the sandbox and file-tool checks against other roots, `[sandbox] write` and `allow_outside_workspace` refused on a shared node, host sessions refused there by default; the command share; detach for host sessions; `local` attached to the first admin's Personal workspace; a Nodes page per workspace. | A code typed in workspace A attaches the node to A. *A reused or expired code is refused. *A 6th wrong code closes the dialog. *A 33rd pending request is dropped. *A Personal chat that names a team node gets 403 at create and at prompt. *A member who leaves is blocked. *A non-member gets 404. Plus the C.12 tests below for N.1. | L (was M) |
| 2 | BYOK: `workspace_providers`, sealed `workspace_secrets`, Personal settings, server model settings removed, onboarding writing to Personal, the R1 seed, embeddings on their own endpoint (Ollama or custom), model aliases and slots, a per-turn router in the master. | *Workspace B cannot use A's provider id. *No key appears in plain text in `config.toml` or `master.db`. Onboarding offers no "reuse provider" for embeddings. A `vision_bridge` alias without a vision model is refused. The summary slot skips a model with a smaller window. | L |
| 3 | Grants: `[credentials] model_keys`, `session.grant`/`grant.drop` keyed by workspace, a per-turn router on the node, `node_key_grants` per workspace, the systemd credentials option. | *`model_keys = "none"` or `"local"` refuses a grant. *The key bytes are absent from `node.db`, its WAL and the audit table. *A turn with agent X and delegate Y receives exactly those providers. *After a restart, a turn waits for its grant. Detaching or removing lists the keys it received. Plus the C.12 tests for N.3. | M (+half a day) |
| 4 | Environments: `[containers]`, the image allow-list, a container per session in its workspace's root, a worktree per session, checkpoints per worktree, the self-test, the container and environment shares. | *`podman inspect` shows no privilege, no capabilities, only the volume mounted, `keep-id`. *An image off the list is refused. *A repository `compose.yaml` changes nothing. Restore in session 1 leaves session 2's files alone. *Cancel kills `sleep` inside the container within 2 s. *Low disk refuses a new session. Plus the C.12 tests for N.4. | L (+1 day) |
| 5 | Archived state and fork. | *An archived chat refuses with 409 even while its node is online. *A fork by a non-owner gets 404. A fork's first events are `history_imported`. *A fork onto a node not attached to the session's workspace is refused. | M |
| 6 | Connections: user accounts, the workspace account, the "use existing or add" prompt, the forge git path, `forge.grant`. | *Another user cannot see or use my connection. *A token never appears in `argv` (read `/proc/<pid>/cmdline` during a clone). *A host not in `[forge] hosts` is refused. The commit e-mail comes from the forge API. | L |
| 7 | Classification, scheduled deletion, autosave; detach with environments, from the master and from the node's CLI; node revocation. | The classification table, case by case. *Immediate deletion is refused when anything is unsafe; the minimum is 1 day. *Autosave never pushes an ignored or secret-named file, never forces, never writes onto an existing branch. Author, committer and trailer match §4.7, and so does the fallback. Node offline at the deadline: archived now, deleted at the next hello. Plus the C.12 tests for N.7. | L (+1 to 2 days) |
| 8 | Experimental: `ModelAccess::Proxy` (measure load on a small home server first), then the private per-user node with its own confirm code. | *A proxy token does not work for another session. *A confirm code is single-use and binds one user. | L |

Phases 3 and 4 can run in parallel after 2, and 6 after 1.

**The C.12 tests the build is committed to** (abuse cases; each written first, failing on the
old code):

| Task | Test |
|---|---|
| N.1 | *A `session.create` for a workspace not on the node's attached list is refused by the node even when the master has an active `workspace_nodes` row and sends it. |
| N.1 | *A command naming workspace B for a session the node recorded as A is refused and logged. |
| N.1 | *A host session of A cannot read or write a file in B's host root: `cat`, `ls`, `touch`, `ln` and `mv` from `bash`, and `read`, `grep`, `glob`, `write`, `edit` from the tools, all refused. |
| N.1 | *A session of A created with a cwd inside B's root, or inside an overlapping root, is refused at create. |
| N.1 | *On a shared node, `[sandbox] write` and `allow_outside_workspace = true` stop the node at start; a host session without `host_shell_shared` is refused with the key's name. |
| N.1 | *A second workspace's admin pairing a node already attached to A sees the same fields as for a fresh node: no other workspace id, name, count, session or environment in the pairing answer or the Nodes page. |
| N.1 | *Detaching A (from the master, and from `node detach` with the master unreachable) leaves B's running `sleep` alive and B's next prompt answered; A's later appends are refused. |
| N.1 | *With `max_commands = 2`, `share = "borrow"`, A running `sleep 1` and `sleep 30`: B's command starts within 1.5 s, before any queued command of A; with `share = "strict"`, B's command starts at once while A has five queued. |
| N.1 | One link drop and reconnect resumes sessions of A and of B, each to its head, with no frame applied twice. |
| N.3 | *A grant whose `workspace_id` is A is refused for a session of B on the same node, and a correct grant for A is not used by B's next turn. |
| N.3 | *No grant value appears in `/proc/<pid>/environ` of any child of either workspace (test D10, grant case). |
| N.4 | *A container session of A has no mount of B's root (`podman inspect`), and `ls` of B's volume path inside it fails. |
| N.4 | *An `env.create` naming workspace B with A's session, or a mount source outside the session's workspace root, is refused by the node. |
| N.4 | *With `max_running = 2` and two workspaces, A cannot start a second container while B's first starts at once. |
| N.7 | *Detaching A with an unsafe environment schedules it; B's environment on the same node is not classified, scheduled or touched. |
| N.7 | *After a detach, A's data root is in the trash and gone after `keep_deleted_days`; B's root is untouched. |

**What it costs**, against the first version: phase 1 grows from M to L (about three days: the
attached list and CLI, workspace on the wire, roots and profile checks, the command share, host
detach); phase 3 by half a day (keying and the check); phase 4 by about a day (roots, mount
check, container and environment shares); phase 7 by one to two days (detach scoped to a pair,
the offline CLI detach with local classification). About six to seven days in all, most of it
in phase 1, which is on the critical path.

If time is short, cut in this order: the `borrow` share (ship `strict` only: about a day less,
and a lone workspace uses half the machine); `node attach-local` (keep `local` on the first
admin's Personal workspace only); per-workspace labels; the CLI's local classification before an
offline detach (print a warning instead). Do not cut the attached list, the workspace check on
the node, the root checks or the default refusal of host sessions: they are the separation.

## 9. Questions for the owner, and what remains

**Blocking the build** (each with my recommendation; the build of N.1 starts from these):

| # | Decision | Recommended | If you choose the other |
|---|---|---|---|
| Q1 | Host sessions on a node attached to more than one workspace | **Refused unless `[execution] host_shell_shared = true`**, and then only with the sandbox enforced; containers are the separated path (§4.10.3). | Allowed by default: every shared node carries the residuals of §4.10.3 unasked, and before Linux 7.1 one injected agent in any workspace can read every other workspace's files and keys on that node. |
| Q2 | How `local` is attached beyond the first admin's Personal workspace, and what for | **Only by `rizzy-harness node attach-local` on the server's machine; non-admins get container sessions only; host sessions on `local` stay server-admin-only** (§4.10.6). | From the web UI: the master widens its own node over HTTP, against "the node decides" (**needs your explicit yes**). Host sessions for team members: T11 is abandoned and every agentic member of that team is admin-equivalent (**needs your explicit yes and a `PLAN.md` change**). |
| Q3 | May the running node remove an entry from its attached list when the master reports a detach? | **Yes: removing narrows; only `node pair` adds.** | Only the CLI writes the list: a master-side detach takes effect on the node only through the master's own refusal until the node's owner runs `node detach`, and `node workspaces` shows a stale entry. |
| Q4 | The command share's default | **`borrow` with priority for a workspace below its guarantee**; `strict` as a setting (§4.10.4). | `strict` by default: simpler and B never waits for A, but a node with two attachments gives a lone user half its slots. |
| Q5 | The last detach of a node | **Also revokes it** (except `local`), so no pinned key outlives every attachment. | The key stays pinned with no workspace: re-attaching needs no new key, and a stolen copy of it can still connect and wait. |

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
  poisoning between members of one workspace); environments only for container sessions;
  workspace deletion refused while environments exist; always saying "model alias", never just
  "alias".

**Recorded dissent (decided; not blockers).** The owner kept these as written:

- Autosave of other users' sessions is the deleter's choice. I would have saved only the
  requester's sessions and those whose owner opted in.
- The dialog names users with their counts. I would have shown totals only.
- New workspaces start with no provider. I would have kept the `demo` provider in every
  workspace.
- A node may serve several workspaces (O3). I recommended one workspace per node, which
  separates workspaces for free; this note builds the owner's choice and prices it (§4.10.3).

The first two need the T10 change in §5, and that needs the owner's explicit yes when it is
built.

**Not verified:**

- where chat titles come from today;
- whether `podman exec` passes signals to the process inside (§4.6 assumes not);
- whether `systemd-creds` works for user services on the distributions in the README;
- which forges offer device flow;
- that Landlock denies `environ`, `fd` and `mem` of a sibling session's processes (tested only
  against the harness itself, `gate3:crates/cli/tests/sandbox.rs:231-253`), and that
  `cmdline` of them is readable (kernel behaviour, not tested here);
- whether rootless `pasta` keeps a container off the host's loopback (test D14);
- which kernels the distributions in the README ship, so how many nodes sit below Landlock
  ABI 6 and ABI 9 today.

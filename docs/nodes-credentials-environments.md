# Nodes, credentials and environments: design note

Status: **signed off by the owner on 2026-11-12; nothing built, nothing blocks the build.**
For after Gate 3 of `FIX_PLAN.md` (feature freeze until then). The decisions were made by the
owner on 2026-11-10, 2026-11-11 and 2026-11-12; every recommendation of the 2026-11-11 draft
was accepted (§9). Costs and conflicts with the code are stated, not argued away.

Citations: every claim about current code added on 2026-11-11 was read on `main` at merge
`976670e` (which includes pull request 4). Citations in §1 rows P1 to P8 and in the passages
kept from the first version were made against an older `main` and were not all re-checked;
line numbers there may have moved.

**What changed on 2026-11-11.** First answer (ADOPTION_PLAN O3, C.12): any node may be
attached to several workspaces, and the design is built around that.

- A node is **attached** to one or more workspaces; a `workspace_nodes` table holds one row
  per attachment. The first version's workspace column on `nodes` is gone (§4.1).
- Each attachment is its own pair code, and **the node keeps its own list of the workspaces it
  serves**; only its own pairing flow, with its owner's yes, adds to it. The node refuses a
  session for any workspace not on it (§4.4, §4.10.1).
- Every session names its workspace on the wire; node and master both check it (§4.10.1).
- Grants are keyed by `(workspace, provider)` (§4.3); a per-workspace share of the node
  (§4.10.4); detach and revoke (§4.10.5); a draft threat-model row and committed tests (§5,
  §8).

Second answer (the owner, later the same day), finished here:

- **Two trees on the node.** Session files live in `<base>/<workspace-id>/<session-id>/files/`;
  node internals live in `<data>/ws/<workspace-id>/`. The node derives a session's folder
  from the two ids; **the master no longer sends a folder**, and `workspace_roots` goes
  (§4.11).
- **A session of a project with a repository works in a git worktree** of one bare repository
  per project in the workspace's directory (§4.11.2).
- **A folder that already exists on the machine** is used only through a link the node's
  owner makes on the node's command line (§4.11.3).
- **No conversation at rest on the node.** The node still writes every event first, durably,
  into a per-workspace **spool**, and deletes it once the master has acknowledged it; the
  master's log becomes the durable record, and the master sends a session's history to the
  node when it is opened (§4.12). Memory only, the owner's first inclination, was priced as the
  alternative; the owner chose the spool on 2026-11-12.
- **`local` is shareable like any other node**, attached to further workspaces by the usual
  pair flow; my earlier restriction is withdrawn (§4.10.6). This changes T11, with the owner's
  explicit yes of 2026-11-11.
- Host sessions on a node with several workspaces: re-decided with the new layout (§4.10.3).

**What changed on 2026-11-12.** The owner signed off every recommendation (§9) and decided that
**agentic work happens only inside a git repository**: every agentic session's `files/` is a
worktree of a repository, a session without a project gets its own repository, a new project
starts with one the node initialises, and a linked folder must already be a repository
(§4.11.5).

**Summary.** Today a node is global, holds its own provider key in its own `config.toml`,
runs every session in a folder on the host that the client names, and keeps every
conversation in its own database for good. This note makes a node serve the workspaces it is
attached to, each attachment made by a pair code the node shows, that workspace's owner or
admin types, and the node's owner confirms. Every session gets its own folder under its
workspace's directory, derived on the node, so the path decides the workspace, and always
inside a git repository the node created or the node's owner linked. The node keeps
no conversation at rest: it spools each event until the master has stored it. Model
credentials move to the workspace (bring your own key) and reach a node per turn, in memory
only, usable only by that workspace's sessions. **Environments** add a per-workspace volume
in which sessions run inside rootless Podman containers. Workspaces on one machine share a
Unix user; the sandbox gives them separate files, and what it cannot separate is named and,
below a kernel that closes the known escape, keeps host sessions off a shared node unless its
owner allows them. Deleting an environment never migrates anything; it classifies, schedules
and archives, and you **fork** to continue. Forge accounts come through per-user
**Connections**. **Model aliases** let agent definitions and slots name a group of models.

Terms used below:

- **Workspace node**: a node attached to one or more workspaces. Being attached to a
  workspace is what makes it trusted there: there is no separate trust mark.
- **Attachment**: one (workspace, node) pair, made by one pair code, recorded on the master
  (`workspace_nodes`) and on the node (its attached list). A session may run only where both
  records agree.
- **Attached list**: the node's own file of the workspaces it serves, with their links
  (`<data>/attachments.toml`, §4.10.1).
- **Shared node**: a node with more than one attachment. `local` always counts as shared
  (§4.10.6).
- **Base directory**: the one folder on the node under which every workspace's session folders
  live (`[node] workspaces_dir`, §4.11).
- **Session folder**: `<base>/<workspace-id>/<session-id>/`; its `files/` is the session's
  working folder.
- **Linked folder**: a git repository that already exists on the machine, made usable by one
  workspace by the node's owner (`node link`, §4.11.3).
- **Scratch session**: an agentic session with no project; it gets a repository of its own
  (§4.11.5).
- **Spool**: the node's durable store of events the master has not yet acknowledged (§4.12).
- **Host session**: a session whose shell runs on the node's machine, not in a container: each
  command in bubblewrap with Landlock inside, or under Landlock alone where bubblewrap cannot
  run (decision O12 of 2026-11-12, from B.11, `docs/container-experiment.md`; built as T.8a to
  T.8c).
- **Pair code**: a short, single-use code the node shows and a workspace owner or admin types
  into the master's UI to attach that node to that workspace.
- **Grant**: what the master sends a node so one turn of one session can call its models.
- **Environment**: a named volume (repository plus caches) on one node, belonging to one
  workspace. The container that uses it is disposable.
- **Forge**: the git host a repository lives on (GitHub, GitLab, Gitea...).
- **Connection**: a forge account attached in the harness, either by a user (theirs, saved per
  user) or by a workspace admin (the workspace's).

## 1. The problem and the evidence

| # | Problem | Evidence |
|---|---|---|
| P1 | Nodes are global. Any server admin's join token registers a node every agentic user can pick; nothing ties a node to a workspace. | `nodes` has no owner or workspace (`crates/store/src/lib.rs:293-299`); join tokens (`:301`); `/api/nodes` filters only the local node (`crates/master/src/http.rs:438-450`). |
| P2 | Every remote node holds a long-lived provider key on disk. | `node_run` loads its own `config.toml` and starts a `ProviderRuntime` (`crates/cli/src/main.rs`, `node_run`); the README says to put "provider and key" next to `node.toml`. Research: "a prompt-injected `bash` on `main` can read the node's `config.toml`" (`research/harnesses-round-1.md` §7, "The central tension"). |
| P3 | Model chains name models the remote node may not have. | The master resolves through `resolver_for` and sends `model`; the node resolves it against one router built from its own file (first version's citations). |
| P4 | Providers are server-wide. Team workspaces only overlay chains; Personal workspaces have no settings. | `providers.rs:1-3` ("the whole server uses"); `Config.providers` (`crates/master/src/config.rs:16`); `overlay` (`crates/master/src/resolver.rs`). |
| P5 | Two sessions in one folder undo each other's changes. | `checkpoints_keep` is "per working folder" (`crates/node/src/policy.rs:20-21`); shadow repository keyed by folder path (`crates/node/src/git.rs:568-572`). |
| P6 | No isolation beyond Landlock: the shell runs on the host as the node user. | `Confinement` is `Unconfined`, `Confined` or `Refused` (`crates/tools/src/sandbox.rs:279-292`); residuals: pathname Unix sockets before ABI 9, UDP, DNS (`:17-26`). |
| P7 | No forge credentials exist; "Connections" is a placeholder. | `web/src/components/Sidebar.tsx` (first version's citation). |
| P8 | No archived or read-only session state, no fork. | `sessions` columns (`crates/store/src/lib.rs:234-256`). |
| P9 | Nothing on a node ties a session to a workspace, so two workspaces on one node are not separated at all. | `session.create` carries no workspace and a client-chosen `cwd` (`crates/protocol/src/lib.rs:131-150`; `crates/master/src/http.rs:1240`), accepted anywhere under any `workspace_roots` entry, `$HOME` by default (`crates/node/src/policy.rs:251-276`, `:166`); full findings in §4.10.2. |
| P10 | Every conversation stays on every node's disk for good, in a file its owner can read. | The node's store is the authoritative log, `synchronous=FULL` (`PLAN.md` §1, §2 "Durability"; `crates/cli/src/main.rs:524`); the master's mirror is `Normal` because "it can always re-fetch from the node" (`crates/store/src/lib.rs:49-54`; `crates/cli/src/main.rs:260`). |

## 2. The decisions, with their costs

| Decision | Cost |
|---|---|
| **Podman** for session containers and self-hosting; Docker only for self-hosting. | Rootless Podman needs `newuidmap`/`newgidmap` with privileges (setuid, or file capabilities as on Arch: `cap_setuid=ep`), `/etc/subuid` and a user runtime directory. `no_new_privs` disables those privileges, so a node started by the README's systemd unit (`NoNewPrivileges=yes`) cannot create its first container: measured, `newuidmap` fails with "Could not set caps" (B.11, `docs/container-experiment.md` §7). The unit must drop that line or the node must run as a user service; a `nologin` user without linger has no runtime directory (not reproduced in B.11). Rootless Podman does not run nested in the published image, so a node there has no environments. Docker hosts get no container sessions. |
| **Chat-model keys per workspace (BYOK)**, no global store, nothing copied. Onboarding writes its first provider into the first admin's Personal workspace. **R1 (reading, to confirm):** the `HARNESS_PROVIDER_*` settings seed that Personal workspace. | `providers`, `default_model`, `[agent_models]` and `HARNESS_DEFAULT_MODEL` go; Personal workspaces gain settings; notices and the model cache go per workspace. `.env` seeds apply at config load, before any account exists, so R1 needs a seed deferred to the claim (§4.4). |
| **Embeddings and voice are harness (server-wide) settings** with their own endpoints. They never reuse a workspace provider. Onboarding offers Ollama (the default) or a custom API endpoint for embeddings, and no "reuse the provider you just added". | Voice already has its own URL and key. Embeddings lose `source = "provider"` and `provider_id` and the onboarding path that reuses the provider. |
| **A1 (to confirm):** title generation, summarisation and the vision bridge use the workspace's models. | Summarisation already uses the turn's model on the node; I found no title generator or vision bridge (first version): slots to add. |
| **Every workspace node is trusted by each workspace it is attached to and may receive that workspace's grants.** Attaching it is the act of trust. A grant carries only the keys the turn needs, in memory only, kept out of the log and redacted, usable only by a session of that workspace. `node.toml` may refuse grants or use local keys. **No proxy now**. | A node's operator can read the keys its turns use, for every workspace it serves; a stolen node key yields the API keys of every attached workspace; detaching cannot un-send keys (the UI lists which to rotate). |
| **No long-lived secret in a shell command**; the node generates its own key pair; the private key is kept under systemd encrypted credentials, else in a 0600 file. **A2 (to confirm).** | `systemd-creds encrypt` needs root, so the systemd path is a second, root step (§4.4). |
| **A3 (to confirm):** an offline node finishes its running turn; a new turn waits. | Grants must outlive the longest turn (§4.3); the spool must hold that turn's events (§4.12). |
| **Nodes are attached to workspaces, one or several** (owner, 2026-11-11). One key, one link, one link generation per node. | Reverses "per-workspace node access ... dropped" (`PLAN.md` §5). Two workspaces share a machine, a Unix user, a node process and a link: separation is built and tested (§4.10.3), and on the host it has holes the sandbox cannot close. One node per workspace would have had it for free. |
| **The path decides the workspace** (owner, 2026-11-11): `<base>/<workspace-id>/<session-id>/files/`, derived on the node; the master sends no folder (§4.11). | Today's "open my checkout in `~/code/foo`" becomes a one-time `node link` on the node (§4.11.3); projects stop carrying a folder path (`crates/master/src/projects.rs:131`). |
| **No conversation at rest on the node** (owner, 2026-11-11): a spool of unacknowledged events; the master's log is the durable record (§4.12). | The master must store with `synchronous=FULL` and acknowledge each event; it sends histories to nodes; a master that loses its database loses every conversation, so the master must be backed up. `PLAN.md` §1 to §3 change (§4.12.5). |
| **`local` is an ordinary, shareable node** (owner, 2026-11-11). | A sandbox escape on `local` reaches the master process, every workspace's keys and the database. T11 changes (§4.10.6). |
| **Private per-user node** (own confirm code, model access by proxy) is experimental and comes last. **Landlock stays**; containers are a further `Confinement` kind. Since O12 (2026-11-12, B.11) a host session's commands also run in bubblewrap, with Landlock inside where the kernel has it. | Two execution paths to test and document. |
| **Environments**: the volume is kept; the node builds the invocation; the image comes from an allow-list, plus a setup script; one worktree per session; files are shared within the workspace, chats stay private. | File tools run in the node process and the shell runs in the container, so paths and file ownership must agree (§4.6). Shared files change T10 (§5). |
| **Deletion**: no migration. Sessions are classified (a)/(b)/(c); other users are named, with their counts; autosave is optional, the deleter's choice. Anything unsafe forces a schedule (7 days by default, at least 1). Sessions are then archived; to continue, fork. **A4 (to confirm):** nothing unsafe may be deleted immediately. **A5 (confirmed):** a repository with no forge remote is class `c`. | Needs Connections (P7), a git path with network access (§4.7), archive and fork (P8). The names and counts in the dialog and autosaving colleagues' files need T10 changed (§5). |
| **Forge accounts through Connections, per user**, plus one workspace account added by admins. | OAuth needs a registered app per forge and a callback a loopback master often cannot receive. Users have no e-mail today, which the plain fallback needs (§4.7). |
| **Model aliases** per workspace: a value with `::` is a concrete model, anything else an alias, never `@`. | "Alias" already means an agent handle (`crates/agents`), so always say "model alias". |

## 3. What does not change

The node dials out over TLS to a pinned certificate and signs a nonce bound to it
(`crates/link/src/identity.rs:60-64`); a node joins only once per key, in one transaction
(`crates/store/src/lib.rs:945`). **The node is still the single writer** of its sessions: it
assigns `seq` and appends each event durably before the effect it describes
(`crates/node/src/session.rs:223-263`). Replication by `seq` with the divergence rule stays
(`crates/store/src/lib.rs:552-740`), and so does hardened git (T19). A node has one key and
one link however many workspaces it serves. What moves is where the long-term copy of the
log lives (§4.12).

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

CREATE TABLE node_links (                    -- linked folders the node reported (§4.11.3)
  node_id TEXT NOT NULL, workspace_id TEXT NOT NULL,
  name TEXT NOT NULL,                        -- the link's name; the master never sees the path
  reported_ms INTEGER NOT NULL,
  PRIMARY KEY (node_id, workspace_id, name),
  FOREIGN KEY (workspace_id, node_id) REFERENCES workspace_nodes ON DELETE CASCADE);
-- A cache of what the node said in its last hello, for the UI. Never an authority.

-- sessions (existing table): `cwd` stops being an input. New columns:
--   files_kind TEXT NOT NULL      -- scratch | project | link | environment (never a plain folder)
--   link_name  TEXT               -- with files_kind = link
--   (project and environment are already linked through session_projects / session_environments)
-- projects (existing): `cwd` goes; gains `repo_url TEXT` (a forge URL, or NULL for an
--   empty repository created on the node) and `default_branch TEXT`.

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
  project_id TEXT,                           -- uses that project's repository (§4.11.2)
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

`master.db` is opened with `Durability::Full` instead of `Normal`
(`crates/cli/src/main.rs:260`): it is now the only durable copy of a conversation (§4.12).
Nothing new is stored for hydration: the master sends the events it already mirrors.

What cascades and what does not:

| Event | `workspace_nodes` | Sessions of that pair | Environments and session folders |
|---|---|---|---|
| Detach (§4.10.5) | Row goes to `detaching`, deleted when the pair's environments and folders are gone | Archived, epochs bumped (C.1), grants dropped | §4.8 flow scoped to the pair; `RESTRICT` keeps the row until it ends |
| Node revoked | Every row of the node deleted in the revoke transaction (the `nodes` row stays with `revoked_ms`, so `CASCADE` does not fire) | Same, for every workspace | Same, for every workspace; "forget" if the node is dead |
| Workspace deleted | Refused (409) while any of its environments exist; otherwise every attachment is detached first, then the row goes and `CASCADE` removes what is left | Already archived by the detaches | None exist |

In `config.toml` (harness settings), `[memory]` keeps `embeddings_url` and
`embeddings_model` and gains `embeddings_key_secret`. `source` is `ollama` (the default) or
`custom`, and `provider_id` goes. `[voice]` is unchanged.

Notes:

- **Keys leave `secrets.toml`.** Workspace keys and every Connection token are sealed with
  AES-256-GCM (`aws-lc-rs`, already a dependency) under a key-encryption key in
  `<data>/keys/secrets.key` (0600). A copied `master.db` alone then leaks no key, and deleting
  a workspace or a user removes theirs by cascade. `secrets.toml` keeps the harness keys
  (embeddings, voice, web search).
- **Archived is master-side state, not an event.** The node that owns the session may be
  gone, and only it may append. An archived session refuses prompt, approval, restore and
  set-agent with 409, whatever the node says.
- **Workspace check (master).** Create and every prompt, approval, restore and set-agent
  require an `active` row in `workspace_nodes` for the chat's effective workspace
  (`EFFECTIVE_WS`, `crates/store/src/lib.rs:384-390`) and the session's node, and that this
  workspace equals the one the session was created in. A member who leaves falls back to
  Personal; their chats on the team's nodes then fail the check and become blocked, the way
  `agentic_blocked` works (`crates/master/src/workspaces.rs:473`). The node checks again
  (§4.10.1).
- **Who may do what.** Attach and detach nodes, create and delete environments, and manage
  providers, model aliases and the workspace forge account all need `settings` in that
  workspace (owner and admin). Using a node or environment needs `agentic` plus the account
  grant (T11). Revoking a node outright is a server admin's act. Users manage their own
  Connections.
- **What a workspace's admins see of a node:** its label, hostname, key prefix, connection
  state, capabilities, this workspace's share, its link names, and this workspace's sessions
  and environments. Never the other attachments, their count, their sessions, environments,
  links, grants or load. A server admin sees every node with its attachment count.

### 4.2 Model aliases and slots

- A value containing `::` is a concrete model; anything else is a model alias
  (`[a-z][a-z0-9-]{0,31}`, never `@`). Agent frontmatter `model:`, `AgentChain` entries and
  slots accept either.
- An alias expands to its models in order. A chain is the expanded main entry, then the
  expanded fallbacks, with duplicates removed and the result capped at `MAX_FALLBACKS`. The
  first available model that passes the slot's capability check runs, and the
  `model_fallback` event shows which.
- Built-ins `small`, `vision`, `long-context` exist in every workspace and stay empty until a
  provider exists. The provider page proposes members from the reported capabilities.
- Slots in `WorkspaceSettings`: `title`, `summary`, `vision_bridge`. Saving a `vision_bridge`
  alias without a vision model is refused. `summary` skips, at use time, any model whose
  context window is smaller than the turn's main model (`research/harnesses-round-1.md` §8,
  "Model slots and memory scopes"). Slot output is untrusted model text.

### 4.3 Granting model access (protocol)

New `MasterToNode` messages (beside `crates/protocol/src/lib.rs:128`). A grant is sent on the
same ordered link just before the `session.prompt` that uses it.

```rust
"session.grant" { req_id, session_id, workspace_id, grant_id, expires_ms,
                  routes: Vec<{ provider_id, access: ModelAccess }> }
enum ModelAccess { Direct { kind, base_url, key: Secret<String> },
                   Proxy  { url, token: Secret<String> } }   // reserved, not built
"session.prompt" { ..., workspace_id, grant_id: Option<String> }   // additive fields
"grant.drop" { grant_id }                                   // turn ended, node detached
```

- **Carries:** only the providers the turn can reach: the agent's chain, each delegate's
  chain, `compare` models, and the `summary` and `vision_bridge` slots, for the session's
  effective workspace only.
- **Never carries:** another workspace's keys, a forge token (sent separately, §4.7), or
  anything that persists. A grant never goes into an event (it is not part of `AgentSpec`),
  so never into the spool.
- **Master side:** sent only to the session's node, only while that node has an `active`
  attachment to the session's workspace, is connected, is not revoked, and announced
  `credentials:master` in `node.hello`. Each send updates `node_key_grants` under
  `(node, workspace, secret)`.
- **Node side, keyed by workspace.** `[credentials] model_keys` decides (§4.5). With
  `"master"` (the default) the node accepts a grant only if its `workspace_id` equals the
  workspace the session's folder belongs to (§4.11) and that workspace is on its attached
  list. Keys go into an in-memory map keyed by `(workspace_id, provider_id)` and bound to the
  grant's session (C.3 adds epoch and turn), and into the `SecretSet`. At `session.prompt` the
  turn's router is built only from grants of that session; the lookup key uses the session's
  own workspace, not the one the message names. Keys are dropped on `grant.drop`, at
  `expires_ms` (C.3, O10), on detach of that workspace, and on link loss once the running
  turn ends (A3). With `"local"` or `"none"` the node does not announce the capability and
  refuses a grant with an ack error. The backend takes a router per turn instead of one
  provider for the whole node.
- **Never in a child's environment.** Grants live in the node process only. The child
  environment is `env_clear()` plus an allow-list (`crates/tools/src/env.rs:19`, PLAN.md T3),
  and a grant is never an environment variable of the node itself.
- **Removing a node or detaching a workspace:** a `grant.drop` for each live grant in scope,
  then the existing link drop for a revoke (`crates/master/src/nodes.rs:57`). The dialog then
  lists every key in `node_key_grants` for that workspace and says: "rotate these keys at the
  provider; this node may still hold them".
- **The embedded `local` node** runs in the master process; it gets the same per-turn router
  in-process, with nothing on the wire, and the same workspace keying.

### 4.4 Attaching a node with a pair code

**The node's owner starts it on the node; a workspace owner or admin types the code the node
shows; the node's owner confirms; only then does the node write the attachment.** Each
attachment is a separate run of this flow. The first one also registers the node's key. For
`local`, see §4.10.6.

1. An owner or admin opens the workspace's Nodes page and chooses "Attach node". The UI shows
   a command that carries no secret: `rizzy-harness node pair wss://host:port --fingerprint …`.
   It also shows an input for the pair code, and keeps the dialog open.
2. On the node, `node pair` loads the node's key pair, or generates it on the first pairing.
   It connects over TLS to the pinned fingerprint and sends `Hello::Pair { public_key,
   hostname, signature }`, signing the master's nonce (today's `Join` and `Auth` are in
   `crates/link/src/wire.rs:24-43`). A pairing connection is never a link: it does not
   replace the running link (today a second connection does,
   `crates/master/src/nodes.rs:209`) and does not bump `link_gen`. The master keeps the
   request in memory and answers with a pair code: 8 characters from a 32-letter alphabet (40
   bits), valid 10 minutes. The node prints the code and the first 16 hex characters of its
   key.
3. The admin types the code into the dialog. The master burns the code first (C.5), then
   matches it to the pending request. It inserts the node row if the key is new, and a
   `workspace_nodes` row with `added_by` and state `pending_node`. The UI shows the key prefix
   for comparison. The master sends `Paired { node_id, workspace_id, workspace_name }` on the
   still-open connection.
4. The node's CLI shows `attach this node to workspace "<name>" (<id>) on <master
   fingerprint>? [y/N]` and the node's current attached list. On yes it adds the entry to its
   attached list, creates `<base>/<workspace-id>/` and `<data>/ws/<workspace-id>/` (§4.11),
   and sends `PairConfirmed`; the master sets the row `active`. On no, a closed connection or
   10 minutes without an answer, the master deletes the row. The node checks `node.toml
   [attach]` (§4.5) before asking.
5. Properties: single use; 10 minutes; bound to the workspace whose authenticated admin
   types it; needs the node owner's yes; no secret in any shell command; the key pair is the
   node's own. Limits: 5 wrong codes per dialog close it, at most 32 pending pair requests in
   all, at most 4 per source address. Codes are compared as hashes in constant time.
6. Key at rest. With `--systemd-creds` the node writes the PKCS#8 bytes only to stdout,
   piped into `systemd-creds encrypt --name=node.key - /etc/credstore.encrypted/rizzy-harness-node.key`
   run as root, and the unit gets `LoadCredentialEncrypted=node.key`. Without systemd it is
   today's 0600 file (`crates/link/src/lib.rs`).

**What a replayed, guessed or misdirected code gets.**

| Attempt | Result |
|---|---|
| A code typed a second time | Already burned: the one opaque failure ("code not valid"), counted against the dialog. |
| A guessed code (40 bits, 5 tries per dialog, 10 minutes) | Not a practical attack. If it hit, it would bind someone else's pending node to the guesser's workspace on the master only: the node's owner sees the wrong workspace name at step 4 and says no. |
| A compromised master that binds the pairing to another workspace | The node's owner sees the name. A compromised master can lie about the name; it already holds every key and chat (T6). |
| A master that adds a `workspace_nodes` row with no pairing | The node refuses every session for that workspace: it is not on the node's list (§4.10.1). |

**What a second workspace's admin learns from pairing: nothing about the first.** The pairing
answer and the Nodes page carry only what any fresh node shows: hostname, key prefix,
capabilities. The master does not say that the key is already known, and the node id is not
shown in the UI. Residuals: the hostname is the same in every workspace; the share
(§4.10.4) is an absolute number from which the count of workspaces can sometimes be guessed; a
member of both workspaces can match the two entries by key prefix.

Code impact: pairing holds a connection for minutes, but today's handshake has a 10-second
timeout and one shared 32-slot semaphore (`crates/master/src/nodes.rs:17-21`), so pairing
gets its own. Join tokens, `create_join_token` and its endpoint go. `node join` becomes `node
pair`; `node detach`, `node workspaces`, `node link`, `node unlink` and `node leave` are new.
Separately (R1), `HARNESS_PROVIDER_*` is applied to the first admin's Personal workspace when
the claim creates it.

### 4.5 `node.toml` additions

```toml
[node]
workspaces_dir = "~/Workspaces"   # the base directory (§4.11); the default; `$HOME` itself is refused
# workspace_roots goes: no session folder is chosen by path any more (§4.11)

[attach]
max_workspaces = 4           # pairing beyond this is refused on the node
allow = []                   # workspace ids; non-empty means only these may be attached

[execution]
host_shell = true            # sessions in a host folder, under [sandbox] (bubblewrap with Landlock inside; O12)
host_shell_shared = "abi9"   # host sessions on a shared node: "never" | "abi9" | "always" (§4.10.3)
containers = false           # sessions in environments

[limits]                     # beside today's tool_stream_bytes, max_steps, bash_timeout_max_s
max_commands = 4             # tool child processes running at once, node-wide (§4.10.4)
share = "borrow"             # "borrow" | "strict"
spool_mb = 64                # unacknowledged events per workspace; beyond, appends fail (§4.12)
history_mb = 64              # histories held in memory, all sessions together (§4.12)

[credentials]
model_keys = "master"        # "master": accept grants (memory only)
                             # "local": use this node's own keys file, refuse grants
                             # "none": refuse grants; no model calls here
local_keys_file = ""         # 0600, operator-managed; only with model_keys = "local"

[containers]
runtime = "podman"           # the only value; rootless is required, not a setting
max_environments = 8         # node-wide; shared between workspaces by the strict rule
max_running = 4              # running session containers, node-wide; strict share
network = "on"               # "on" (pasta with the host mapping off, no published ports) | "off" (--network=none); §4.6
allow_host_network = false   # only true makes network = "host" accepted
memory = "4g"
cpus = 2
pids = 1024
setup_timeout_s = 900
keep_deleted_days = 3        # deleted environments, session folders and detached workspaces go to a trash first
[containers.images]          # allow-list: name -> pinned reference
rust = "docker.io/library/rust@sha256:…"

[forge]
push = false                 # let the node clone with and push using forge tokens
hosts = ["github.com"]       # the only hosts a forge token may be used against
```

`model_keys` is the node deciding, not a second approval. `"local"` on a shared node makes
every attached workspace's turns use, and be billed to, that one key, and `node pair` says so.
As T5 says, nothing received over the protocol or HTTP widens these settings. These are
refused with an ack error, each with a test (§8): an image not on the list, a network mode the
node does not allow, a grant to a node that is not `"master"`, a session for a workspace not on
the attached list, a host session on a shared node the rule does not allow.

What `node.toml` can and cannot be overridden by: the attached list can only hold workspaces
`[attach]` allows. If `node.toml` is edited to allow fewer than are attached, the node refuses
to start and names the attachments to detach. Nothing the master sends, and nothing in the
attached list, raises a value here. `workspaces_dir` must pass `check_protected`
(`crates/node/src/policy.rs:303-334`), must not lie under a folder the sandbox grants every
command (`SYSTEM_READ`, `crates/tools/src/sandbox.rs:48-50`, or `[sandbox] read`/`write`),
and must neither contain nor be inside the data directory. On a shared node `[sandbox] write`
and `allow_outside_workspace = true` stop the node at start (§4.10.3).

### 4.6 The container the node builds

One container per session, on its environment's volume and its own session folder: processes,
`/tmp` and signals stay apart. Nothing from the master, the project or the repository enters
the arguments except allow-list values:

```
podman run --detach --name rh-<env>-<session> --label rizzy.env=<env> --label rizzy.ws=<ws>
  --userns=keep-id --user <node uid>:<gid>     # same uid inside and out
  --cap-drop=all --security-opt=no-new-privileges
  --read-only --tmpfs /tmp:size=1g,mode=1777 --tmpfs /run
  --network=pasta:--map-guest-addr,none | none # host only if allow_host_network
  --memory=<m> --memory-swap=<m> --cpus=<n> --pids-limit=<p>
  --mount type=bind,src=<base>/<ws>/<session>,dst=<base>/<ws>/<session>,relabel=private
  --mount type=bind,src=<repo>,dst=<repo>,relabel=private      # the environment's repository
  --mount type=bind,src=<base>/<ws>/environments/<env>,dst=<same>,relabel=private  # cache/, home/
  --workdir <base>/<ws>/<session>/files
  --env HOME=<base>/<ws>/environments/<env>/home --env <allow-listed names only>
  --init --pull=never --log-driver=none
  <image@sha256:…> sleep infinity
```

`<repo>` is the project's repository when the environment belongs to a project
(`<base>/<ws>/projects/<project-id>.git`, §4.11.2), else
`<base>/<ws>/environments/<env>/repo.git`. A container sees its own session folder, not its
siblings' (an improvement on the first version, which mounted every session's worktree). Before
`podman run` the node checks every mount source: real path inside `<base>/<ws>` of the
session's own workspace, no symlink in the path, and inside no other workspace's directory.
**That check is not enough on its own** (B.11, `docs/container-experiment.md` §4): Podman
resolves a mount source that is a symlink at `podman run` time and mounts its target, so a
folder swapped for a link between the check and the run is followed (the review's S-2 window).
Podman cannot be handed the node's checked descriptor instead (`src=/proc/<node>/fd/<n>` is
refused: "statfs … permission denied"). So after the container starts, and before its first
command, the node runs `stat -c %d:%i <dst>` inside it for every mount and compares the result
with the device and inode of the pin it checked (measured: the same numbers inside and on the
host); on any mismatch it removes the container and refuses the session.

**Network.** `network = "on"` is `--network=pasta:--map-guest-addr,none`, not plain `pasta`
(B.11 §5). Rootless pasta keeps a container off the host's 127.0.0.1, but by default it maps the
host at `169.254.1.2`, which the container's `/etc/hosts` names `host.containers.internal`, and
every host service bound to `0.0.0.0` or to the LAN address answered there, the connection
arriving from the host's own LAN address. With `--map-guest-addr,none` that address timed out
and the internet stayed reachable. IPv6 was not tested. Each container with the network on has
its own `pasta` process: 26.4 MB RSS (25.7 MB PSS) idle, against 2.7 MB PSS for the rest of an
idle session container (conmon, init, `sleep`).

Why: `keep-id` so the node's file tools and checkpoint git can use what the container wrote; no
privilege or capabilities (the floor in `research/round1/synth/02-permissions-sandbox-security.md`
F15); `--read-only` so setup cannot persist outside the volume; no published ports;
`--pull=never` (pulled by digest at creation); `relabel=private` for SELinux; the same absolute
mount paths because worktrees store absolute paths.

Each `bash` call runs as `podman exec --workdir … <ctr> bash -c <cmd>`, with the allow-listed
environment (T3). **Stopping a command (corrected by B.11, `docs/container-experiment.md` §2,
§3).** A SIGTERM or SIGINT to the `podman exec` client is forwarded inside; a SIGKILL reaches
nothing, and a node `kill -9` leaves the container and every process in it running. The command
inside is already its own process group and session leader, so the wrapper prints its pid
(`$$`) first, and that is the group to signal. `podman exec <ctr> kill -TERM -<pid>`, then
`-KILL`, ends that group (106 ms for the extra exec), **but not a child that called `setsid`**:
it survived both. Only stopping the container ended it, through the PID namespace's init
(`--init`). So the node does, on cancel, on timeout and at the end of every turn:

1. `kill -TERM -<pid>` inside, the grace `bash.rs` uses today, then `kill -KILL -<pid>`;
2. one `podman exec <ctr> ps -eo pid,ppid,pgid` (about 70 ms): a process whose parent is pid 1
   and that is neither the `sleep infinity` holder nor a command the node tracks (T.8d's
   background commands) was left behind;
3. if anything was left, `podman stop -t 2` and a fresh `podman run` before the next command
   (measured: up to 2 s for the stop, 72 ms for the run), with the event saying why.

Recommended: stop only when something is left, not always. Always stopping is simpler and
certain, but every cancel would then cost a container restart, lose `/tmp` and kill the
session's tracked background commands, which are legitimate; the check costs one exec and
misses only a detached process whose parent is still a tracked command, which step 2 finds
when that command ends. Files live in the mounts and survive a stop either way. A restarted
node finds leftover containers by their labels (`podman ps -a --filter label=rizzy.env=…`) and
removes them (74 ms each) before it serves the workspace. Not yet tested: running each command
inside the container under `bwrap --unshare-pid`, which would end a command's tree with the
command as on the host; B.11 found bwrap needs the container's `/proc` unmasked
(`--security-opt unmask=ALL`) to mount its own, so N.4 should measure it before choosing.

**Image and setup.** The creator picks an image from the allow-list the node announced. The
setup script is stored on the master, shown to the creator, and run once with the
environment's network setting and a timeout. It is never read from the repository, and
compose files are never read. Image layers are shared between workspaces on the node; they
are read-only and pinned by digest, so sharing them leaks only which images exist.

**Checkpoints** are per session folder, in `<data>/ws/<ws>/checkpoints/` (§4.11). C.13
redesigns what they hold.

### 4.7 Forge operations, Connections and commit identity

**Connections (per user).** A user attaches their own forge accounts under Connections. The
flow is OAuth device authorisation where the forge offers it, because a self-hosted master on
loopback often cannot receive a callback; otherwise a personal access token. Accounts are
saved per user (`user_connections`). When a user connects a forge in a workspace and already
has an account for that host, the prompt asks: "Use @login" or "Add another account". The
answer goes into `workspace_connection_choice`. Owners and admins also add the workspace's
own forge account (`workspace_forge_accounts`).

**Git with network.** The hardened git refuses every transport (`protocol.allow=never`,
`credential.helper=`; `crates/node/src/git.rs`). Clone, classification fetch and push use a
separate path: `protocol.https.allow=always` on top of the same overrides, the URL given
explicitly (never the repository's `remote.*`), and the token as an `http.<url>.extraHeader`
through `GIT_CONFIG_COUNT`/`KEY`/`VALUE`, never in `argv` (Landlock leaves `/proc` readable,
`crates/tools/src/sandbox.rs:44-50`). It arrives in a `forge.grant` (one host, one operation,
one session, so one workspace), only with `[forge] push = true` and the host in `[forge]
hosts`. The first clone of a project's repository uses the same path (§4.11.2).

**Commit identity for autosave.** These rules are my reading of the owner's words: the
workspace account pushes and commits, and the session owner's account gives the linked
co-author.

| Part | Rule |
|---|---|
| Push credential | The workspace forge account's token. No workspace account for that host: no autosave (§9). |
| Author and committer | The workspace forge account: its display name and the commit e-mail the forge links to it. |
| `Co-authored-by` trailer | The session owner, from the account chosen in `workspace_connection_choice` for that host: `Co-authored-by: <display name> <commit e-mail>`. |
| Fallback trailer | No connected account: `Co-authored-by: <harness display name> <e-mail>`. Shown, not linked. The harness stores no e-mail today, so the fallback needs one (§9). |
| Message | `rizzy-harness autosave before environment deletion`, then the trailer. No session title. |

### 4.8 Deleting an environment

New messages: `env.create`, `env.delete`, `env.classify` → `env.classification`, `env.save` →
`env.saved`. Each names its workspace, and the node refuses one whose environment is not in
that workspace's directory. `node.hello` lists the node's environments by workspace so the
master can reconcile. `env.classification` carries, per session: id, class, reason codes
(`dirty`, `untracked`, `unpushed`, `stash`, `submodule`, `detached`, `in_progress_op`,
`forge_unreachable`, `no_remote`) and counts. It never carries file names or content. The same
classification serves deleting a session folder and detaching a workspace (§4.10.5, §4.11).

**Classification (node, read-only git):**

| Finding in the session's `files/` | Class |
|---|---|
| A repository with no forge remote the session may push to: a scratch session, a project created empty, a linked repository without a remote (nothing can be saved automatically; A5). Since 2026-11-12 there is no "not a git repository" case (§4.11.5). | c |
| Clean status (untracked included, ignored excluded), no stash, no merge/rebase in progress, submodules clean, and HEAD reachable from a ref on the forge checked by `ls-remote` now | a |
| Anything else, or the forge cannot be reached | b |

A stash belongs to the repository, not to one worktree, so any stash in the shared repository
makes every session using it class `b`. Ignored files are never work and are never pushed;
their owner sees how many there are.

**The flow:**

1. A user with `settings` opens "Delete environment", and the master sends `env.classify`. If
   the node is offline, only scheduling is offered, and classification runs at the deadline.
2. The dialog lists the requester's own sessions by title with their class. For everyone
   else it shows counts per class and, per user, name, handle and number of unsaved sessions.
   The autosave choice is offered when the node has `[forge] push` and the workspace has a
   forge account for the host.
3. If every session is class `a`, the requester may delete now or schedule. Any `b` or `c`
   means schedule only: 7 days by default, at least 1.
4. **Grace period.** New sessions are refused (409, naming the deadline). Existing sessions
   keep working so people can commit and push. Every affected user gets an in-app notice and a
   WebSocket event. The requester or any other owner or admin can cancel. Every step writes an
   audit row.
5. **Deadline** (master clock): classify again. If autosave was chosen, send `env.save` for
   every `b` session, then `env.delete`, then archive the sessions. A failed push keeps that
   session `b` and delays the deletion once, by a day, with a notice to the owner.
6. **Node offline at the deadline:** sessions are archived at once, the deletion is
   `waiting_node`, and the autosave (if chosen) and `env.delete` run at the next hello.
   **Node dead:** an owner can "forget" the environment.
7. On the node, `env.delete` stops the containers and moves the folders to the trash for
   `keep_deleted_days`, then removes them, with the checks of §4.11.4.

**Autosave (`env.save`).** Commit the tracked changes and the untracked files that are not
ignored. Skip secret-like names (the artifact list, T18), files over 10 MB and anything past
100 MB in total. Push to a new branch `rizzy/autosave/<env-short>/<session-short>`, never with
force, never onto an existing branch. Commit with the identity in §4.7. Report skipped files to
their owner only.

**Detaching a workspace** that has environments or session folders on a node first runs this
flow for them (§4.10.5); **revoking a node** does it for every workspace.

### 4.9 Fork

"Continue elsewhere" is for the session's owner only. The master creates a new session and
sends the old history in `session.seed` batches: user and assistant text, each tool call cut
to one line, at most 2 MiB per batch. The new node writes them as `history_imported` events,
and `session_forks` links the two sessions. Files come back from the forge (the autosave
branch, if there is one); a fork copies no files. The new session's node must be attached to
the session's workspace; a fork never changes a session's workspace.

### 4.10 A node in several workspaces

#### 4.10.1 The attached list and the checks

**The node decides which workspaces it serves.** `<data>/attachments.toml`, 0600, at the top
of the node's data directory, which no session may include and no tool may touch
(`crates/node/src/policy.rs:288-334`, PLAN.md T10):

```toml
[[workspace]]
id = "0193f4c2…"             # harness_core::new_id(): UUID v7, 32 lowercase hex; not a secret
name = "Team X"              # as the master named it at pairing; display only
paired_ms = 1762819200000
master = "sha256:…"          # the master certificate it was paired with

[[workspace.link]]           # §4.11.3; written only by `node link`
name = "foo"
path = "/home/me/code/foo"
```

| Rule | Why |
|---|---|
| Only `node pair` adds a workspace and only `node link` adds a link, each after the node owner's yes on the node. | Widening is the node owner's act, never the master's. |
| `node detach <id>` and `node unlink` remove, offline if need be (§4.10.5). The running node also removes a workspace when the master reports a detach. | A detach from the master's side must take effect on the node without its owner; removing only narrows. |
| Writes go through a lock file and an atomic rename; `node run` re-reads the file when its modification time changes, at each check. | `node pair` runs while `node run` runs; no restart is needed. |
| A missing or unparseable file means "no workspace": every session is refused, with the reason. | Fail closed. |
| The node announces its workspace ids and, per workspace, its link names (never paths) in `node.hello`. | The master reconciles and shows link names; it never adds an attachment or a link from it. |

**Where the workspace id lives in the protocol.** `session.create` loses `cwd` and gains a
required `workspace_id` and `files: Scratch | Project { project_id, repo_url?, branch } | Link {
name } | Environment { env_id }` (beside `crates/protocol/src/lib.rs:131-150`);
`session.prompt`, `session.grant`, `env.*` and the approval, restore and set-agent commands
carry `workspace_id` too. It is not a secret. The node keeps it implicitly: the session's
folder is `<base>/<workspace-id>/<session-id>/`, so the path holds it, and a restart finds it
there. It is not written into an event: T10 keeps the workspace mapping master-side.

**The checks.**

| Where | When | Check |
|---|---|---|
| Master | create, prompt, approval, restore, set-agent | An `active` `workspace_nodes` row for (effective workspace, node), and the effective workspace equals the session's (§4.1). |
| Node | create | `workspace_id` is on the attached list; the folder is derived (§4.11), never taken from the message; a link name must be a link of that workspace; host sessions only as §4.10.3 allows. |
| Node | every later command | The command's `workspace_id` equals the one in the session's folder path, and it is still on the list. A mismatch is refused and logged. |
| Master | `node.hello` | Attachments active on the master but missing from the node's list are treated as a node-side detach (§4.10.5). Entries on the node's list with no master row are ignored, and the node is told (`attachment.unknown`). |

**One of each per node.** One key, one link, one link generation (C.1). All attached
workspaces' sessions travel on that link, each command naming its workspace. A reconnect
resumes every session the node holds, whatever its workspace; the master resumes only those
whose attachment is active and fences the rest (C.1's epoch).

#### 4.10.2 What one session can reach of another workspace's today

Established by reading `main`, for two workspaces' sessions on one node as the code stands (one
Unix user, `[sandbox] mode = "required"` enforced):

| Path | What A reaches of B today | Evidence |
|---|---|---|
| Choosing the folder | **Anything.** A session may be created in any folder under any `workspace_roots` entry, `$HOME` by default; the cwd is whatever the client sent. | `crates/node/src/policy.rs:14-15`, `:166`, `:251-276`; `crates/master/src/http.rs:1240` |
| Shell (Landlock) | Not the whole filesystem. Read-write: the session's cwd, its temp folder `<data>/sandbox-tmp/<session>` (0700), `[sandbox] write`, five device files. Read-only: `/usr /bin /sbin /lib* /etc /opt /proc` and `[sandbox] read`. `/tmp` is not writable. | `crates/tools/src/sandbox.rs:11-15`, `:44-61`, `:203-226`, `:337-397`; `crates/cli/tests/sandbox.rs:167-188` |
| Shared caches | Read-write for every session of every workspace: `[sandbox] write` is one folder for all. | `crates/tools/src/sandbox.rs:215-217`; `crates/node/src/policy.rs:65-66` |
| `/proc` | Readable. Other domains' `environ`, `fd`, `mem`, `cwd` are denied (ptrace-class), tested only against the harness process. `cmdline`, `status`, `stat` of B's processes are not ptrace-gated: A sees B's command lines. Not tested. | `crates/tools/src/sandbox.rs:44-47`; `crates/cli/tests/sandbox.rs:231-253` |
| Signals | Denied outside the domain only with Landlock ABI 6 (best-effort). Tested against the harness only. | `crates/tools/src/sandbox.rs:22-23`, `:614-619`; `crates/cli/tests/sandbox.rs:402` |
| Unix sockets | Before ABI 9, any pathname socket, the session bus included, which starts programs outside the sandbox: a full escape to the node user. | `crates/tools/src/sandbox.rs:23-26`; `PLAN.md` T10, T11 (lines 484-485); `crates/cli/tests/sandbox.rs:371` |
| Loopback network | With `network = "on"` (default) A connects to whatever B listens on at `127.0.0.1`. With `"off"`, TCP is denied, UDP is not (since T.8b, only where the shell is not contained). | `crates/tools/src/sandbox.rs:76-88` (`Network`) |
| File tools | Run in the node process, not under Landlock; confined to the session's cwd by a path walk; with `allow_outside_workspace = true` they reach every path the node user can, except the data directory. | `crates/tools/src/lib.rs:204-252`; `crates/node/src/policy.rs:17-19` |
| Checkpoints | `<data>/checkpoints/<hash of folder>.git`, keyed by folder: sessions in one folder share one, and a restore by one rewrites the other's files. Not reachable from the shell. | `crates/node/src/git.rs:568-572`; `crates/cli/src/main.rs:530` |
| Conversations | Every session of every workspace is in `<data>/node.db` for good, readable by the node's owner. Not reachable from the shell (data directory). | `crates/cli/src/main.rs:524`; `PLAN.md` §1 |
| Other node state | `tool_output` in memory per session; graphify output keyed by folder hash; the artifact stage is one folder in the data directory, a fetch looks a stage up by session and stage id. | `crates/tools/src/outputs.rs:1-5`; `crates/tools/src/graphify.rs:48-53`; `crates/node/src/artifacts.rs:52`, `:790-814` |
| Capacity | No node-wide cap on running turns or commands: one lane per session, any number of lanes. The link's queues are shared (64 in, 256 out on the node). | `crates/node/src/lib.rs:40-49`, `:282-330` |

#### 4.10.3 Separation: decided

**Files are separated by construction.** Every session folder is under its own workspace's
directory and is derived on the node (§4.11). The Landlock profile of a host session grants
read-write to exactly its own `files/` and its own temp folder (plus its workspace's `cache/`
and the git directory of its project repository, §4.11.2), read-only to `SYSTEM_READ` and
`[sandbox] read`; other sessions' and other workspaces' folders are not granted, and Landlock is
an allow-list. Before applying the profile the node checks, as today's `exposes` does
(`crates/tools/src/sandbox.rs:132-140`), that no granted path equals, contains or is inside the
data directory or another workspace's directory. The file tools' path walk
(`crates/tools/src/lib.rs:217-252`) is the same boundary: the session's `files/` only. On a
shared node `[sandbox] write` is refused (its entries become `<base>/<ws>/cache/` with
`XDG_CACHE_HOME` pointing there) and `allow_outside_workspace = true` is refused.

**What stays shared under one Unix user**, whatever the layout:

| Residual for host sessions | Closed by |
|---|---|
| Before Landlock ABI 9 (Linux 7.1, `research/round2/synth/05-links-workers-protocols.md` §6.5), the session bus or any pathname socket starts a program outside the sandbox: A becomes the node user and reads B's files and every grant in the node's memory. | ABI 9; or a container; for pathname sockets also T.8c's mount view, in which a socket outside the grants does not exist (abstract sockets belong to the network namespace: ABI 6, or T.8b with the network off). |
| Before ABI 6, A can signal B's processes. | ABI 6, included in ABI 9; or T.8a's PID namespace, in which B's processes have no pid A can name. |
| A sees B's command lines in `/proc`. | T.8a: bubblewrap's PID namespace with a fresh `/proc` (B.11 §4: a bwrap command saw 5 processes, a `sandbox-exec` command 422 with every command line); a container; `hidepid` on `/proc`, the host's choice. |
| A connects to anything B listens on at `127.0.0.1`; with network off, UDP still goes. | With the network off: T.8b's network namespace closes TCP, UDP and DNS (B.11 §5). With the network on: nothing for host sessions, which share the host's network. A container: its own network namespace; test D14 is answered (§4.6, "Network"). seccomp (O4) only on the Landlock-only path. |
| A fills the disk, CPU or memory B needs. | The share (§4.10.4) for slots; nothing for disk. |
| A bug in the file tools' path walk is a read of B. | Nothing: file tools run in the node process in every mode. Tests in §8. |

**Host sessions on a shared node: allowed from Landlock ABI 9, refused below it by default.
Decided (owner, 2026-11-12).** `[execution] host_shell_shared = "abi9"` (the default) allows host
sessions on a shared node only when the sandbox self-test reports ABI 9 or newer enforced;
below that they are refused with the reason; `"always"` allows them below ABI 9 (an explicit,
logged choice); `"never"` refuses them. My first draft refused them always by default, because
files were not separated; with the layout they are, and the decisive remaining hole is the
escape, which ABI 9 closes. What is left at ABI 9 (command lines in `/proc`, loopback services,
UDP, capacity) is a leak between people each workspace trusts with a shell, not a way into the
other workspace's files or keys.

Price of the decision: few kernels have ABI 9 today (not established per distribution), so in
practice a shared node refuses host sessions until its kernel is new, unless its owner sets
`"always"`; and at ABI 9 a prompt-injected agent in A can still reach B's unauthenticated
services on loopback. Price of "allowed" by default: below ABI 9 one injected agent in any
workspace reads every other workspace's files and keys on that node. Price of "refused" by
default: the common home case (my Personal workspace and one team on my own machine) needs a
setting or Podman even on a new kernel.

Containers are the separated path for all of it: own mount, PID and network namespaces. They
still share the uid with the node (an escape from the container is the node user) and the file
tools' path walk. A different Unix user per workspace, the real separation for host sessions,
is not planned: it needs root and a privileged helper. B.11 confirmed the namespaces for files,
`/proc` and network (`docs/container-experiment.md` §4, §5; with pasta's host mapping off).

**Open question for the owner (B.11, 2026-11-12; not decided, the rule above stands): can
`host_shell_shared` relax once host sessions run in bubblewrap?** The rule refuses host sessions
on a shared node below ABI 9 because of the pathname-socket escape. What the namespaces change,
step by step:

| Residual | After T.8a (PID namespace) | After T.8b (network namespace, network off only; built, `crates/cli/tests/network_off.rs`) | After T.8c (mount view from the grants) |
|---|---|---|---|
| Pathname socket escape (the session bus) | unchanged | unchanged | **closed**: a socket that is not granted is not in the view |
| Abstract Unix socket escape | unchanged | **closed with the network off**, on every Landlock ABI (abstract sockets are per network namespace: the host's answer `ECONNREFUSED`, `network_off_hides_the_hosts_abstract_sockets`) | unchanged with the network on (ABI 6 closes it) |
| Signals to B | **closed** | | |
| B's command lines | **closed** | | |
| Loopback services, UDP | unchanged | **closed with the network off** (TCP, UDP and DNS to the host's loopback and LAN address: `network_off_udp_never_reaches_the_host`, `network_off_tcp_fails_and_only_loopback_is_there`, `network_off_dns_lookups_fail`); open on the Landlock-only fallback | unchanged with the network on |
| Capacity, the file tools' path walk | unchanged | unchanged | unchanged |

Recommendation: keep `"abi9"` until T.8c is built and its tests pass. Then add a value (for
example `"namespaces"`, the new default) that allows host sessions on a shared node when the
node runs them in bubblewrap with the mount view and either Landlock ABI 6 or the network off,
because that closes both known escapes without ABI 9. What stays is the same as at ABI 9: B's
loopback services with the network on, capacity and the path walk, a leak between people who
each trust the other with a shell, not a way into B's files or keys. Not to be relaxed before
T.8c: T.8a and T.8b alone leave the pathname socket open, which is the full escape.

#### 4.10.4 A fair share of the node

Shared by every workspace on the node: command slots (`[limits] max_commands`, new), running
containers (`[containers] max_running`, new), environments, the spool's disk, the link, the
disk. Turns are not capped: a turn mostly waits for a model.

| Resource | Rule | Bound for B while A is at its share |
|---|---|---|
| Command slots, `share = "borrow"` (default, decided 2026-11-12) | Each attached workspace is guaranteed `max(1, max_commands / n)` slots. A free slot may be borrowed by anyone. When a slot frees, a workspace below its guarantee gets it first. No command is preempted. | B's command starts when the first running command on the node ends: at most the shortest running command, capped by `bash_timeout_max_s`. |
| Command slots, `share = "strict"` | Each workspace has `max(1, max_commands / n)` slots and never more. | B never waits for A; A alone uses only its share. |
| Running containers | Strict: `max(1, max_running / n)` each. | B never waits for A. |
| Environments | Strict: `max(1, max_environments / n)` each. | B never waits for A. |
| Spool | `spool_mb` per workspace (§4.12). | A's full spool ends A's turns, not B's. |
| The link | Shared, first come; one call streams at most `tool_stream_bytes`; B.7 decides whether control frames need a lane. | Not bounded per workspace; measured in B.10. |
| Disk | A free-space floor for the whole node (§6). No per-workspace quota. | None. Residual. |

`max_workspaces` must not exceed `max_commands` (refused at load). Separately, a turn has a
soft limit of 100 changed files (a warning, never an error), designed in `docs/undo-and-stash.md`
(on its own branch); it is not part of the share. The caps live in `node.toml`
only; the master is told the workspace's own share in `node.hello`. A user whose workspace is at
its share sees, as a line of the tool's live output, "waiting for a free slot on this node (this
workspace may run 2 at a time)"; it never names another workspace or how many there are.

#### 4.10.5 Detach and revoke

**Detach one workspace W from node N.** Allowed to W's owners and admins from the master, and to
N's owner from N's CLI, which must always work, offline included.

| Step | From the master (W's admin) | From the node (`node detach W`) |
|---|---|---|
| 1 | Environments and session folders of W on N: the §4.8 flow; scheduled when anything is unsafe. The attachment goes to `detaching`; new sessions of W on N are refused (409). | The CLI runs the classification locally (read-only git) and prints it; the owner confirms. No autosave (no forge grant offline). |
| 2 | At the deadline, or at once if all safe: `env.delete` for each, then `workspace.detach { workspace_id }` to N. | The entry leaves the attached list at once; the running node kills W's commands and refuses W's commands. |
| 3 | N stops W's containers and commands, drops W's grants and W's histories in memory, flushes W's spool (§4.12; anything not acknowledged is sent first, or kept in the trash if the link is down), removes W from its list, then removes both trees (§4.11.4). | Same, minus the flush when offline: the unacknowledged spool goes to the trash with the folders. |
| 4 | The master archives W's sessions on N, bumps their `owner_epoch` (C.1), drops the row. | At the next hello the master sees W missing and does step 4. |

The other workspaces' sessions on N keep running: the link stays, `link_gen` is not bumped, and
nothing outside W's sessions, grants, spool and two directories is touched. A late append from
one of W's sessions is refused by the epoch. Linked folders of W are never deleted: they are the
owner's; the link entries go.

**Revoke N.** A server admin, or N's owner by `node leave`. Every attachment is detached as
above, the key is marked revoked, `link_gen` bumps and the link drops. The master revokes N by
itself when it flaps more than 3 times in 10 minutes (§6). **Decided (2026-11-12):** the last
detach also revokes the node, so no pinned key outlives every attachment; `local` is the
exception.

**What survives on N's disk.** W's two trees in the trash for `keep_deleted_days` (3 by
default), then removed. After a revoke the node's key file stays until its owner deletes the
data directory. Linked folders stay.

#### 4.10.6 The `local` node

`local` is an ordinary node: attached to the first admin's Personal workspace at first start
(when the claim creates it, `ensure_personal_workspaces`, `crates/cli/src/main.rs:263`), and to
further workspaces by the pair flow (owner, 2026-11-11; my earlier restriction is withdrawn).

**Pairing a node with no terminal of its own.** The server's admins are `local`'s owner. A
server admin starts the pairing either on the server-admin page ("This server's node", "Attach
to a workspace"), which shows the pair code, or by `rizzy-harness node pair --local` on the
server's machine. The workspace's owner or admin types the code as for any node. The node
owner's yes (step 4 of §4.4) is then a second, separate act by a server admin on that page or in
that terminal, after it shows the workspace's name. So "the master cannot attach a node by
itself" holds for `local` in this sense: no workspace admin alone, and no code path without a
server admin's yes, attaches it. It does not hold in the stronger sense: master and node are one
process, and the yes arrives over HTTP. **That reading of "the node decides" for `local` is the
owner's explicit decision of 2026-11-11** and must be written into `PLAN.md` T5 and T11 when
built.

**The same rule as any shared node, with `local` always counted as shared.** The master's own
data (every workspace's keys in memory, `master.db`, the key-encryption key) sits beside
`local`'s sessions as if it were one more tenant, so §4.10.3 applies to `local` from its first
attachment: host sessions under `host_shell_shared` (ABI 9 by default), the session's own
folder only, `[sandbox] write` and `allow_outside_workspace` refused.

**The price, not softened.** A sandbox escape on any other node reaches that node: its
sessions, its spool, the grants in its memory. On `local` it reaches the master process:
**every workspace's provider keys, the database with every account and every conversation, and
the key that unseals the rest** (`PLAN.md` T11, line 485; `crates/cli/src/main.rs:267-284`,
`:477`). The file tools on `local` run in the master's own address space, guarded by the path
walk. ABI 9 closes the known escape (the session bus); a kernel bug or a bug in the path walk
does not need it.

**What changes in T11.** "The embedded node is for server admins (`[nodes] local_node_users =
"admins"`)" and the 403 for other accounts (`crates/master/src/workspaces.rs:445-470`) go; any
agentic member of a workspace `local` is attached to may use it, under the shared-node rule.
`local_node_users` goes from `config.toml` (`crates/master/src/config.rs:149`). This needed the
owner's explicit yes, given on 2026-11-11.

**Containers on `local`** change the picture for the shell: it runs in another mount, PID and
network namespace, without the data directory mounted, so the session bus escape and `/proc`
reading are gone. They do not change it for the file tools (in the master process) or for a
container escape (the master's uid). **Decided (2026-11-12):** the Attach dialog for `local` says
this, and offers container sessions first where the node has them.

### 4.11 The two trees on the node

```
<base>/                                   [node] workspaces_dir; created 0700 with a marker file
  .rizzy-harness-base                     marks the folder as a base directory (§4.11.4)
  <workspace-id>/                         0700; created at pairing
    <session-id>/                         one per session, sub-agent sessions included
      files/                              the session's working folder (its cwd), always a git worktree
      repo.git                            a scratch session's own bare repository (§4.11.5)
    projects/<project-id>.git             bare repository, one per project (§4.11.2)
    environments/<env>/                   cache/, home/, and repo.git when not a project's
    cache/                                host sessions' caches (replaces [sandbox] write)
    .trash/                               removed sessions, kept keep_deleted_days
<data>/                                   the node's data directory, never visible to a session
  keys/node.key   attachments.toml   node.toml
  ws/<workspace-id>/                      0700
    spool.db                              unacknowledged events (§4.12)
    checkpoints/<session-id>.git          checkpoint store, per session folder
    tmp/<session-id>/                     the session's TMPDIR (today <data>/sandbox-tmp/<session>)
    artifacts-stage/                      staged files (today <data>/artifacts-stage)
```

Ids are `harness_core::new_id()` (UUID v7, 32 lowercase hex, `crates/core/src/lib.rs:43-46`),
which workspace ids already are (`crates/store/src/lib.rs:1780`). **Session ids are not**:
`SessionId` is a `Uuid` whose `Display` is the hyphenated form (`crates/core/src/event.rs:13-31`),
and today's temp folder name comes from it (`crates/agent-native/src/lib.rs:1841`). Directory
names use the simple 32-hex form of both, and the node refuses any directory name that is not
exactly `[0-9a-f]{32}` where an id is expected.

#### 4.11.1 The path decides the workspace

The master no longer sends a folder: `session.create` names the workspace and what the files are
(§4.10.1), and the node derives `<base>/<workspace-id>/<session-id>/files/`. A `cwd` field from
the master does not exist in the protocol type; a forged one is an unknown field, ignored, and
has no effect (test in §8). **`workspace_roots` goes** (`crates/node/src/policy.rs:14-15`): the
base directory and the linked folders (§4.11.3) replace it, so a session's folder can no
longer be any folder under `$HOME`.

The profile and the tool boundary are those of §4.10.3. A sub-agent session gets its own
`<base>/<ws>/<child-id>/files/`: a worktree of the parent's project repository on its own
branch when it can write, as A.4 requires on `main` (`crates/node/src/session.rs:1075-1078`);
read-only children are granted the parent's `files/` read-only. That replaces today's
sub-agent folders under `<data>/sessions/<child-id>/files/` (R.1: node-owned, a git
directory of their own, never `worktree add` in the user's repository; PLAN.md T32), which
already follow the per-session shape; whatever replaces them keeps R.1's rule that the node's
git never runs with a git directory a session can write. Since every agentic `files/` is in a repository (§4.11.5), A.4's refusal of a
writer child in a folder that is not a repository (`crates/node/src/session.rs:1212-1230`)
can no longer be reached; it stays as a check.

Checkpoints become per session folder (`<data>/ws/<ws>/checkpoints/<session-id>.git`), which
ends P5 for derived folders: no two sessions share a folder any more, except through a linked
folder (§4.11.3). C.13 decides what a checkpoint holds.

#### 4.11.2 Projects

A session of a project with a repository gets `files/` as a **git worktree** of
`<base>/<ws>/projects/<project-id>.git`, a **bare** repository (it has no working tree of its
own; every session is a worktree), on its own branch `rizzy/<session-short>`. A session with no
project gets a repository of its own (§4.11.5); no agentic session gets a plain folder.

| Question | Decision |
|---|---|
| How the repository first arrives | Cloned from the forge through the creating user's Connection (the git path of §4.7, a `forge.grant` for one clone) when the project has a `repo_url`; otherwise auto-initialised by the node (§4.11.5). Before Connections exist (N.6), only auto-initialised repositories and linked ones. |
| Later sessions | `git fetch` with a fresh grant, then `git worktree add -b rizzy/<short> files/ <base ref>`, all through hardened git (T19). |
| What the shell must be granted | Read-write on the bare repository too: a commit writes objects and refs there. **So committed work is shared between all sessions of a project on that node**: a session can read a sibling's commits and branches through git, though not its uncommitted `files/`. This is the same as environments today and is accepted inside a workspace. A planted hook or config in the bare repository is C.8's residual; our git ignores both. |
| Deleting a session | Classify its `files/` (§4.8 table). Class `a`: `git worktree remove`, delete the branch if it is reachable from the forge, remove the session folder. Class `b` or `c`: the owner confirms; the worktree is removed, the branch is renamed `rizzy/trash/<session-short>` and kept with the folder in `.trash/` for `keep_deleted_days`. |
| Project deleted | Every session of it is deleted as above; then the bare repository goes to the trash. |

#### 4.11.3 Linked folders

Today's main single-user flow ("open my checkout in `~/code/foo`") is tied to no workspace by
its path. It stays possible only as an explicit link **made by the node's owner** on the node:

```
rizzy-harness node link <workspace> <path> [--name foo]
rizzy-harness node unlink <workspace> <name>
```

| Rule | Why |
|---|---|
| Recorded in `attachments.toml` under its workspace; never created, changed or widened by anything the master sends. The master knows only the name (§4.10.1). | The node decides; paths stay on the node, as OpenCodex keeps root ids and labels, not paths, on its hub (§7). |
| The path must exist, be canonical (no symlink in it), pass `check_protected`, and not equal, contain or lie inside the base directory, the data directory or another link. | One folder belongs to at most one workspace; detach of another workspace can never reach it. |
| The path must be the top of a git repository with at least one commit. Otherwise `node link` refuses and prints how to make it one (`git init` and a first commit, by the owner). With the explicit flag `--init`, `node link` does it itself: `git init -b main` where there is no repository, and one empty commit where there is no HEAD, through hardened git; it never stages or commits the owner's files and writes no `.gitignore`. | Agentic work is only inside a repository (§4.11.5). Initialising writes a `.git` into someone's folder, so it needs the owner's explicit act, on the node. |
| A session on a link uses the folder itself as its cwd; its session folder holds no `files/`. The sandbox treats the linked folder as that workspace's: other workspaces' profiles and tools refuse it. | The link is the workspace's folder. |
| Several sessions on one link share the folder, as today: A.15's restore refusal stays, and checkpoints are keyed per link. | That is P5, accepted for links. |
| Allowed on a shared node under the same rule as host sessions (§4.10.3), since a link is a host folder. A container session never mounts a link. | No separate rule needed. |
| Detach and revoke remove the link entries, never the folder. | It is the owner's. |

For the `local` node the server admin runs `rizzy-harness node link` on the server's machine.
The UI shows the command where it used to offer a folder picker. Cost: one command per folder,
once; the web UI can no longer open an arbitrary folder, which is the point.

#### 4.11.4 Removing a folder

Before the node removes `<base>/<ws>`, `<base>/<ws>/<session>` or `<data>/ws/<ws>`, in this order:

1. The id is exactly `[0-9a-f]{32}` and is the workspace (or session) being removed.
2. The base directory's canonical path equals the configured one, it is not `/` or `$HOME`
   itself, and it holds `.rizzy-harness-base`.
3. The target is opened with `O_NOFOLLOW | O_DIRECTORY` relative to the base's handle; it must
   be a directory, not a symlink, on the same filesystem.
4. Its containers are stopped and no command of it runs.
5. It is renamed into the same base's `.trash/<id>-<ms>` (atomic, same filesystem).
6. After `keep_deleted_days`, the trash entry is removed without following links (std's
   `remove_dir_all` on Linux does not follow symlinks inside the tree).

A linked folder is never a target. A failure at any step stops the removal and is reported.

#### 4.11.5 Agentic work only inside a git repository

Decided by the owner on 2026-11-12: an agentic session's `files/` is always inside a git
repository; there are no plain-folder agentic sessions; a new project starts with a repository
the node initialises. Chat sessions have no `files/` and are untouched.

| Session | Its `files/` |
|---|---|
| In a project | A worktree of `projects/<project-id>.git` (§4.11.2). |
| Without a project (a scratch session) | **A repository of its own**: the node initialises a bare `<base>/<ws>/<session-id>/repo.git` and adds `files/` as its worktree on branch `main`, the same shape as a project. |
| On a linked folder | The linked repository itself (§4.11.3). |
| A sub-agent that can write | A worktree of its parent's repository (§4.11.1). |
| In an environment | A worktree of the environment's or project's repository (§4.6). |

**Why a scratch repository and not "make a project first".** The quick "just try something"
session must stay one click; a project is a heavier object (board, instructions, members'
view). A scratch repository costs the node one `git init` and one empty commit, and gives the
session everything a project session has: worktrees for writer sub-agents, checkpoints that
borrow objects, classification. Promoting a scratch session to a project later is moving one
bare repository into `projects/`; that UI is not designed here.

**Auto-initialisation**, what the node does, always in a folder it owns under the base
directory and never in anything of the user's:

1. `git init --bare -b main <repo>` through the hardened git (`hardened`,
   `crates/node/src/git.rs:109-128`: no system or global config, hooks off, no transports).
   `main` is fixed; it is not a setting.
2. One empty commit (the empty tree, message `rizzy-harness: start`), so there is always a
   HEAD: a worktree needs a commit (`worktree_add`, `crates/node/src/git.rs:1828`), and a
   repository without one broke snapshots before A.15.
3. Identity: by §4.7's rule, the workspace forge account when there is one, otherwise the
   hardened git's fixed `harness <harness@localhost>` (`crates/node/src/git.rs:125-128`). No
   co-author trailer on this commit: it holds nothing of anyone's.
4. **No starter `.gitignore`.** The node cannot know the language, and a template is a guess
   that ends up committed. The harness's own exclusions stay in the checkpoint store, not in
   the repository.
5. `git worktree add -b <branch> files/ <that commit>`.

Nothing the master sends names a path: it can ask for a scratch session or a project, and the
node initialises only `<base>/<ws>/<session-id>/repo.git` or `<base>/<ws>/projects/<id>.git`,
after checking the ids and the base as §4.11.4 does.

**What this removes or simplifies.**

| Where | Today | Afterwards |
|---|---|---|
| Checkpoints | Two cases: a repository (the shadow borrows the user's objects) and a plain folder (the shadow stores everything, `crates/node/src/git.rs:1-12`, `:611-617`). Experiment B.3: a plain folder of 250,000 files cannot finish its first snapshot within the 60 s timeout, and the shadow is 478 MiB at 100,000 files (`docs/checkpoint-cost.md`, "What the numbers mean", "Disk"). | Only the repository case remains for agentic sessions; the plain-folder branch can go with C.13. Untracked files in a repository are still stored in full, so B.3's plain numbers still describe a repository full of untracked files. |
| Writer sub-agents | Refused in a folder that is not a repository (A.4, `crates/node/src/session.rs:1212-1230`). | Always satisfied; the check stays as a guard. |
| Deletion classes (§4.8) | Class `c` included "not a git repository". | Class `c` means only "a repository with no forge remote it may push to": it still cannot be saved to a forge. |
| `node link` | Any folder. | A repository with a commit, or `--init` by the owner (§4.11.3). |

Plain-folder agentic sessions that exist before this is built are not carried over: before the
first tagged release dev data directories are wiped (`CLAUDE.md`).

### 4.12 No conversation at rest on the node

#### 4.12.1 How the log works on `main`

- A session's `append` assigns `seq = head + 1`, scrubs secrets, writes to `node.db` with
  `synchronous=FULL`, then publishes on the link; the event stays in the store for good
  (`crates/node/src/session.rs:223-263`; `crates/store/src/lib.rs:49-54`).
- `emit` returns the append's error, and the first failure ends the turn (T28;
  `crates/node/src/session.rs:265-286`).
- The master applies events by `seq`, drops duplicates, marks a differing repeat diverged
  (`crates/store/src/lib.rs:552-740`), and on a gap or hello sends `resume { session_id,
  last_seq }`; the node replays from its store (`crates/master/src/lib.rs:902-918`;
  `crates/node/src/lib.rs:808-850`).
- **There is no acknowledgement of events.** The master never tells the node what it stored;
  `ack` exists only for commands (`crates/protocol/src/lib.rs:103`).
- The node rereads a session's whole log from its store when it loads the session, at the start
  of every prompt, at restore and at variant selection (`crates/node/src/session.rs:141`,
  `:190`, `:422`, `:492`, `:737`, `:787`).
- The mirror is `synchronous=NORMAL` because it "can always re-fetch from the node"
  (`crates/cli/src/main.rs:260`; `PLAN.md` §2 "Durability").

#### 4.12.2 The design: a spool, not a store

| Part | Decision |
|---|---|
| Writer | Unchanged: the session actor assigns `seq` and appends before any effect. |
| Where the append goes | `<data>/ws/<ws>/spool.db`: the same store code, `synchronous=FULL`, `secure_delete` on (already set for every store, `crates/store/src/lib.rs:525`). The append is fsynced before it is published, as today. |
| Acknowledgement | New `session.stored { session_id, through_seq }`, master to node, sent only after the mirror transaction that holds that `seq` has committed with `synchronous=FULL`. Batched: at most every 200 ms or 64 events per session. |
| Deletion | On `session.stored` the node deletes that session's spool rows up to `through_seq`. When a workspace's spool is empty, it runs `wal_checkpoint(TRUNCATE)` (`crates/store/src/lib.rs:1194`), so no deleted event lingers in the WAL. |
| At rest | Only events the master has not stored, normally none. |
| History | The node holds the folded history of active sessions in memory. On open or resume the master sends it: new `session.history { session_id, head_seq, events, last }`, in batches of at most 2 MiB (as snapshots today), master to node. |
| Memory bound | `[limits] history_mb` (64 MiB by default) for all histories together; a session with no running turn and no command for 10 minutes is dropped first; a session whose history alone exceeds the bound is refused with the reason. With no active session the node holds none, so idle RSS stays under the 50 MB invariant. |
| Reload cost | One `session.history` transfer: 500 events or 2 MiB per batch over the link, then a fold. A long session with stored deltas can be tens of MiB (deltas are stored events today, AUDIT D5); fix plan 4.8 decides whether hydration may skip them (not established that the fold is the same without them: `crates/core/src/view.rs:216`, `:282` fold deltas). |
| Spool limit | `[limits] spool_mb` per workspace (64 MiB). At the limit the append fails, and the failed-append rule ends the turn (T28): no effect without its event. |

#### 4.12.3 Spool against memory only

The owner leaned to "only in memory". What breaks with it, exactly:

| Case | Spool (decided) | Memory only |
|---|---|---|
| A tool's result when the link drops | Durable in the spool, sent at reconnect. | Lost if the node restarts before reconnecting: the effect happened, its record did not. |
| "The log is written first" | Kept: the spool append is the append. | Either every effect waits for the master's `session.stored` (a round trip before every tool start and approval, and the turn stops the moment the link drops), or "written" means "in this process's memory", which a crash erases. |
| Node restart mid-turn | The spool holds the turn's tail; the master sends the rest; the node closes the open turn (§4.12.4). | The tail since the last acknowledgement is gone; the master's copy ends mid-turn with no record of which tool ran. |
| Disk | Unacknowledged events only, normally nothing; `secure_delete` and WAL truncation. | Nothing. |
| Cost to build | A table, an ack message, deletion, the limit. | About the same code, and every effect path must wait for the master. |

**Decided (2026-11-12): the spool.** It keeps "the log is written first" true without a round trip per
effect, and what it leaves at rest is seconds of events while the link is up.

#### 4.12.4 What changes, case by case

| Case | Afterwards |
|---|---|
| Node restart mid-turn | At start the node has only its spools. Each session with spooled events is announced in `node.hello` with its spool head. The master sends `session.history` up to its own head; the node checks the spool tail is contiguous (`seq` = master head + 1 ...), sends it, then closes the open turn as `recover_interrupted` does today (`crates/node/src/session.rs:170`). Until the master answers, those sessions wait. |
| Link drop mid-turn | The turn goes on; events accumulate in the spool up to `spool_mb`; at the limit the turn stops (T28). At reconnect the spool is sent and acknowledged. A3's grant lifetime still bounds the turn. |
| Failed append ends the turn | Unchanged: the spool append is the append. |
| Divergence (two writers under one key) | Both processes take their head from the master; both append `head + 1` with different content; the master's rule catches it, as today. A node restored from an old disk has an old spool whose `seq` the master already holds with other content: diverged, refused. |
| Rewind and restore | Need the checkpoint events: from the history in memory, or from the master by hydration. Without a link and without the history in memory, refused with the reason. |
| Sub-agent sessions | Sessions like any other: own spool rows, acknowledged, histories in memory while the parent's turn runs. |
| Incognito chats | Spooled like any other, acknowledged, deleted; the master purges after its TTL. Today they sit in `node.db` until then. |
| Deleting a session | The master deletes its mirror; the node drops the history and the spool rows and removes the folder (§4.11.4). |
| A node offline from the master | Cannot open an old session (it has no history). It cannot receive a prompt either, so this costs only offline inspection. |
| A master that loses its database | Loses every conversation; before, nodes held copies. It already holds every account and key: **back up the master**; the README says so. |
| A master restored from an older backup | Its head is below events the node already deleted: those events are gone, and the node continues at the master's head + 1. The UI shows the gap as the end of the restored history. |
| The master's write load | Every event is fsynced on the master, deltas included. B.10 measures the cost; batching the acknowledgement keeps the commits grouped. |

#### 4.12.5 What still lives on the node, and what changes in `PLAN.md`

The claim is **no conversation at rest**, not nothing at rest. Still on the node: the session's
files, the project repositories, the checkpoint store (it holds file contents), session temp
folders, staged artifacts, and for a moment the spool. A shared node's owner can no longer read
other workspaces' chats from disk; they still control the process and its memory, which is the
trust every workspace extends to its nodes, as decided.

`PLAN.md` sentences that change when this is built (the owner proposed it, so the explicit yes
exists; `CLAUDE.md`'s "one writer per session" wording stays true, since the writer does not
move):

| `PLAN.md` | Today | Becomes |
|---|---|---|
| §1 diagram | "node store (node.db): authoritative event log" | "node spool: events until the master has stored them" |
| §1, first bullet | "It mirrors the node's log and forwards commands." | "It holds the durable log, sends histories to nodes, and forwards commands." |
| §1 "Data dir" | "`master.db`, `node.db`…" and "The embedded node keeps its **own** `node.db`…" | `node.db` replaced by per-workspace spools; the embedded node spools like any other. |
| §2 "Durability" | "The mirror uses `synchronous=NORMAL` ... it can always re-fetch from the node." | "The master's log uses `synchronous=FULL`; the node deletes an event once the master has stored it." |
| §3 message lists | no event acknowledgement, no history | add `session.stored` and `session.history`; `session.create` loses `cwd`, gains `workspace_id` and `files`. |
| §3 "Flow control" | "the next hello and `resume` replay from the node's log" | "… from the node's spool". |
| T7 residual | "its own log keeps the session" | "its spool keeps the unacknowledged events". |
| T28 | "The node's store cannot append" | "The node's spool cannot append". |

## 5. Invariants

| Invariant (`CLAUDE.md`) | Effect | Mechanism or change |
|---|---|---|
| The node decides | Kept, widened in scope; read differently for `local` | `node.toml` and the attached list decide; the list is widened only by `node pair` and `node link` with the node owner's yes. Folders are derived on the node. For `local` the yes is a server admin's separate act over HTTP or the server's CLI (owner's decision, 2026-11-11, §4.10.6). |
| The master never runs tools | Unaffected | Git, forge pushes and containers run on the node. Hydration sends events, not work. |
| One writer per session | Kept | The node still assigns `seq` and appends first; only the durable copy moves to the master (§4.12). Detach fences by epoch. |
| Chats are private to their owner | **Changed, needs the owner's explicit yes when built** | Members using a shared environment or project repository see its files and commits; the deletion dialog names other users; autosave pushes other users' files. T10 must say all three. A shared node adds nothing: chats live on the master and, now, not on nodes. |
| No secret in an event, URL or log line | Kept | Grants stay outside events, so outside the spool. Forge tokens exist only in git's environment. Link paths stay on the node. |
| Nothing the model writes is trusted | Kept | Setup scripts come from the master. Compose files and the repository's remote config are never read. |
| `unsafe` forbidden | Unaffected | Podman and `systemd-creds` are child processes. |
| Loopback by default | Kept | Containers publish no ports. Loopback is reachable between host sessions (§4.10.3). |
| Harness stays light | Kept | Histories in memory are bounded and dropped when idle (§4.12.2). Measure node RSS with 8 containers and with `history_mb` full. |
| First-load bundle budget | Kept | The new pages and dialogs are lazy chunks. |

**Draft row for `PLAN.md`** (to add when N.1 is built; number assigned then):

| # | Threat | Mitigation |
|---|---|---|
| T-shared | A node shared between workspaces: a session, agent or member of workspace A reaches workspace B's files, keys, conversations, processes or capacity on the same machine | A node serves only the workspaces on its own attached list, widened only by `node pair` with its owner's yes, and checks every command's workspace against it and against the workspace in the session's folder path; the master also requires an active `workspace_nodes` row. Session folders are derived on the node as `<base>/<workspace>/<session>/files/`; the master sends no folder; a linked folder is made only by `node link` on the node and belongs to one workspace. A session's Landlock profile grants only its own folder, temp folder, workspace cache and project repository, checked against the data directory and every other workspace's directory; file tools use the same boundary; container mounts are checked the same way. On a shared node (`local` always) `[sandbox] write` and `allow_outside_workspace` are refused, and host sessions need Landlock ABI 9 unless `host_shell_shared = "always"`. Grants are keyed by workspace and bound to the session. No conversation is kept on the node beyond the events the master has not stored. Command slots, containers, environments and spools are shared per workspace. Detaching one workspace fences only its sessions and removes only its two directories, after checking ids, the base marker and that no link is followed. **Residual:** host sessions under one Unix user see each other's `/proc` command lines and loopback services (and, when allowed below ABI 9, can escape to the node user); file tools are a userspace path check in every mode; committed work in a project repository is shared by its sessions; disk is not shared fairly; root or a stolen node key gets every attached workspace's grants; on `local` an escape reaches the master process, every key and every conversation. |

Existing rows that change: T5 (the attached list, links, derived folders), T7 (a stolen key
reaches every attached workspace; spool wording), T9 (the share), T10 (workspace directories;
its privacy changes above are separate), T11 (`local` shareable, `local_node_users` gone),
T28 (spool wording).

## 6. How this breaks

- **A hostile repository or prompt-injected agent edits the environment's configuration**
  (`.git/config`, `compose.yaml`, `.devcontainer`). None of these is read. It can poison the
  shared `cache/` or a project's bare repository for other sessions of the same workspace;
  another workspace's are out of reach.
- **A node's operator reads the keys or the files.** Root on any workspace node can read the
  keys its turns use from memory, the session folders and checkpoints, for every workspace it
  serves; since §4.12, not the conversations from disk. Attaching a node is the admin vouching
  for that machine, and the Attach dialog says so.
- **Pair-code abuse.** Anyone who reaches the node port can open pair requests. The caps in
  §4.4 limit this, the key prefix is shown on both sides, and the node's owner confirms the
  workspace name.
- **A master that attaches a node or links a folder by itself.** It can write
  `workspace_nodes`; the node refuses every session for a workspace not on its list, and no
  message creates a link. For `local`, see §4.10.6.
- **A master that sends a folder.** There is no field for it; the node derives the folder.
- **A stolen node key.** The thief can connect as the node and receive grants and histories for
  every attached workspace's sessions it is sent. With C.1 each connection bumps `link_gen`,
  so the copies kick each other off. The master revokes a node that flaps more than 3 times in
  10 minutes. Note: with §4.12 the thief also receives histories on resume, which before it
  could only read from the node's own disk.
- **A revoked or detached node still holds keys.** Only rotation fixes this.
- **One workspace on a shared node reaches another.** §4.10.2, §4.10.3: derived folders, the
  profile check and the tool path check close the files; the residuals are named.
- **One workspace starves another.** §4.10.4 bounds command and container waits; disk and link
  bandwidth are not shared fairly.
- **Detach or delete removes the wrong folder.** §4.11.4: id format, base marker, no symlink
  followed, rename into the same base's trash first; linked folders are never targets.
- **The link drops for long.** The spool grows to `spool_mb`, then the turn stops (§4.12.4).
- **The master's disk fills or its store fails.** It stops acknowledging; nodes' spools grow and
  their turns stop at the limit. No event is lost.
- **The master loses its database.** Every conversation is lost; back up the master.
- **An admin deletes an environment to destroy colleagues' work.** Anything unsafe forces at
  least a day of grace; everyone affected is notified; another owner can cancel; the node keeps
  a trash; there are audit rows. A node owner's offline `node detach` destroys it after
  `keep_deleted_days`, the machine owner's right; the CLI shows the classification first.
- **Autosave pushes secrets or huge files.** Ignored files are never pushed; secret-like names
  and the size caps are skipped. An untracked `config.local.json` holding a token is still
  pushed, to a branch every reader of the repository can see.
- **The dialog as disclosure.** Names and counts tell an admin who works in the environment.
  Decided, and it needs the T10 change.
- **Wrong commit identity.** The commit e-mail is read from the forge's API, never typed in.
- **Full disk.** Below a free-space floor the node refuses new sessions and environments, and
  reserves space for classification, autosave and the spool.
- **Podman missing, rootless not available, or the harness running in a container.** The node
  announces `containers` only after a self-test passes. When the harness itself runs in a
  container (`HARNESS_CONTAINER`, `crates/cli/src/net.rs`), container execution is refused:
  B.11 found rootless Podman does not run nested in a default rootless container at all (no
  subordinate ids inside; with `ignore_chown_errors`, a read-only `/proc/sys`), so the published
  image has no environments (`docs/container-experiment.md` §7). Under `NoNewPrivileges=yes`
  the self-test fails at the first user namespace (`newuidmap`: "Could not set caps").
- **Image supply chain.** Only digests from `node.toml` are used; a tag is refused at load.
- **Clocks.** Only the master's clock sets deadlines. The node deletes on its own only its
  trash and acknowledged spool rows.
- **A restart mid-detach.** The master's row stays `detaching` and the step runs again; the
  node's list is rewritten by atomic rename; a folder is either in place or in the trash.
- **Wrong classification.** Reachability, not branch existence; a stash, a dirty submodule or a
  detached HEAD gives `b`; classify again at the deadline.
- **A cancel that does not reach the process inside the container** would break T9. The
  wrapper's group kill in §4.6 does not reach a `setsid` child (B.11); the check for anything
  left and the container stop after it do, with a test that uses a `setsid` child.

## 7. What the others do

Documentation-level findings from `research/`, to be checked by a source-code round, except
where a permalink is given.

| Pattern | Who (source) | Copy or avoid |
|---|---|---|
| Key sent per session, never written | Cline SSH remote (`research/harnesses-round-1.md` §7, pattern 4) | Copy, with per-turn scope and a rotation list. |
| Inference proxied by the control side | OpenClaw cloud workers, Oh My Pi `auth-gateway`, OpenCodex hub (same section, pattern 3) | Last phase. |
| Pairing codes; per-dispatch credentials; fencing | OpenCodex, OpenClaw, Claude Code (`research/round1/synth/09-remote-distributed-architecture.md` §2.0, §2.4; `research/round2/synth/05-links-workers-protocols.md` §6.1, §6.6) | Copy the pairing limits, burning the code first, and fencing. |
| The executor declares, the hub narrows | OpenCodex: the executor posts capabilities and root ids and labels (no paths) at pairing; the hub refuses a later presence claiming more (`05` §6.3; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L427-L463) | Copy: the attached list and link names are the node's declaration; paths stay on the node. |
| Acknowledge only after durable admission | OpenClaw updates its ack cursor only after its check; Hermes acks after durable admission (`05` §6.4) | Copy for `session.stored`: only after the mirror commit with `FULL`. |
| One worker, one tenant | OpenClaw binds a provisioned box to one environment in the pairing transaction (`05` §2 "OpenClaw", "Cloud workers" item 1; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/node-enrollment.ts#L85-L106) | The owner chose otherwise; this is the separation we give up. |
| A worker serving several tenants | Not found: `05` §6.1 to §6.6 describe no project that binds one worker to more than one tenant. | Our own design, tested as such. |
| Container floor | OpenHands, Hermes (`research/round1/synth/02-permissions-sandbox-security.md` F15) | Copy the floor; avoid their overrides. |
| Environment = image plus setup | Warp, Claude Code (`09-…` §2.5) | Copy the idempotent-setup rule. |
| Moving work between machines | Codex, Warp; Claude Code "loses unpushed work" (`09-…` §2.0) | Classify, save and fork instead. |

## 8. Build order

Each phase is useful alone; tests are written first and fail before it. The order of the first
version is kept; phase 1 grew twice on 2026-11-11, and phase 1b is new.

| Phase | What | Tests (abuse cases marked *) | Size |
|---|---|---|---|
| 1 | Workspace nodes with pair codes: `workspace_nodes`, `link_gen`, `Hello::Pair` with the node's confirmation, the attached list, `node pair`/`detach`/`workspaces`/`leave`; `workspace_id` on every session command; **the two trees**, derived session folders, `workspace_roots` removed, `node link`/`unlink` (repositories only, `--init`), scratch repositories and auto-initialisation (§4.11.5), sub-agent worktrees as session folders, per-session checkpoints and temp folders under `<data>/ws/`; the sandbox and file-tool boundaries; the shared-node rules; the command share; detach and the removal checks; `local` pairing from the server-admin page and `node pair --local`, `local_node_users` removed; a Nodes page per workspace. | The first version's phase 1 tests (codes reused, expired, a 6th wrong code, a 33rd request, 403 for a Personal chat naming a team node, a member who leaves, a non-member) plus the C.12 tests below. | L+ (about 9 to 10 days; was M) |
| 1b | **The spool**: per-workspace `spool.db`, `session.stored`, deletion and WAL truncation, `session.history`, histories in memory with `history_mb`, restart recovery from spool plus master, `master.db` at `FULL`, B.10's measurement of the master's fsyncs. | The spool tests below. | L (4 to 5 days) |
| 2 | BYOK, aliases, slots (unchanged). | *Workspace B cannot use A's provider id. *No key appears in plain text in `config.toml` or `master.db`. | L |
| 3 | Grants keyed by workspace. | *`model_keys = "none"` or `"local"` refuses a grant. *Key bytes absent from the spool, its WAL and the audit table. *A turn with agent X and delegate Y receives exactly those providers. *After a restart, a turn waits for its grant. | M (+half a day) |
| 4 | Environments in the new layout, mount checks, container and environment shares. | *`podman inspect` shows no privilege, no capabilities, only the session folder, the repository and the environment mounted, `keep-id`. *An image off the list is refused. *A repository `compose.yaml` changes nothing. *Cancel kills `sleep` inside within 2 s, and a `setsid` child too (it survives the group kill, B.11; the container stop ends it). *A mount swapped for a link before `podman run` is caught by the check after start and the container removed. *`host.containers.internal` reaches no host service with `network = "on"`. *Low disk refuses a new session. | L (+1 day) |
| 5 | Archived state and fork. | *An archived chat refuses with 409 even while its node is online. *A fork by a non-owner gets 404. *A fork onto a node not attached to the session's workspace is refused. | M |
| 6 | Connections, the forge git path, `forge.grant`, **the first clone of a project's repository**. | *Another user cannot use my connection. *A token never appears in `argv`. *A host not in `[forge] hosts` is refused. | L (+1 day) |
| 7 | Classification, scheduled deletion, autosave; detach with environments and session folders, from the master and from the node's CLI; session deletion with its worktree and branch; node revocation. | The classification table, case by case; autosave rules; offline deadline. | L (+2 days) |
| 8 | Experimental: proxy, private per-user node. | *A proxy token does not work for another session. | L |

Phases 3 and 4 can run in parallel after 2, and 6 after 1. Phase 1b may run beside 2.

**The tests the build is committed to** (abuse cases; each written first, failing on the old
code):

| Task | Test |
|---|---|
| N.1 | *A `session.create` for a workspace not on the node's attached list is refused by the node even when the master has an active `workspace_nodes` row. |
| N.1 | *A `session.create` carrying a `cwd` (or any path) is created in `<base>/<ws>/<session>/files/` and nowhere else; the forged path is not touched. |
| N.1 | *A command naming workspace B for a session whose folder is in A is refused and logged. |
| N.1 | *A host session cannot read or write a sibling session's `files/` in the same workspace, nor any folder of another workspace: `cat`, `ls`, `touch`, `ln`, `mv` from `bash`, and `read`, `grep`, `glob`, `write`, `edit` from the tools, all refused. |
| N.1 | *No message from the master creates, changes or widens a linked folder; a link whose path contains, or is inside, the base, the data directory or another link is refused by `node link`. |
| N.1 | *On a shared node, `[sandbox] write` and `allow_outside_workspace = true` stop the node at start; below ABI 9 with `host_shell_shared = "abi9"`, a host session is refused naming the key; `local` applies the rule with one attachment. |
| N.1 | *A second workspace's admin pairing a node already attached to A sees nothing of A in the pairing answer or the Nodes page. |
| N.1 | *Detaching A (from the master, and from `node detach` with the master unreachable) leaves B's running `sleep` alive and B's next prompt answered; A's later appends are refused; `<base>/<A>` and `<data>/ws/<A>` end in the trash and then gone; a file outside both trees, a linked folder, and a symlink planted in `<base>/<A>` pointing outside are all untouched. |
| N.1 | *With `max_commands = 2`, `share = "borrow"`, A running `sleep 1` and `sleep 30`: B's command starts within 1.5 s; with `"strict"`, at once. |
| N.1 | One link drop and reconnect resumes sessions of A and of B, each to its head, with no frame applied twice. |
| N.1 | *Creating an agentic session of every kind (scratch, project, link, writer sub-agent) never yields a `files/` that is not inside a git repository. |
| N.1 | A new project's repository and a scratch session's repository each have a HEAD commit on `main`, by the identity of §4.7, with no file in it. |
| N.1 | *`node link` refuses a folder that is not a repository, or has no commit, and writes nothing into it; with `--init` it creates `.git` and one empty commit and stages none of the folder's files. |
| N.1 | *No message from the master makes the node initialise a repository anywhere but `<base>/<ws>/<session-id>/repo.git` or `<base>/<ws>/projects/<id>.git`; a forged path, or an id not in the 32-hex form, is refused. |
| N.1b | *After the master has sent `session.stored` for a session, a canary string from its messages is found nowhere under the data directory (spool, its WAL, any file), by a byte search. |
| N.1b | *A link dropped while a tool runs: the tool's result is in the master's log after reconnect, exactly once. |
| N.1b | *A node killed mid-turn and restarted recovers the turn's tail from its spool and the rest from the master, and closes the open turn once. |
| N.1b | *A spool at `spool_mb` ends the turn with no effect after the failed append; B's turns on the node continue. |
| N.1b | *`session.stored` is sent only after the mirror commit: with a mirror whose commit fails, the node deletes nothing. |
| N.3 | *A grant whose `workspace_id` is A is refused for a session of B on the same node, and a correct grant for A is not used by B's next turn. |
| N.3 | *No grant value appears in `/proc/<pid>/environ` of any child (test D10, grant case). |
| N.4 | *A container session of A has no mount of a sibling session or of any folder of B (`podman inspect`), and `ls` of those paths inside it fails. |
| N.4 | *An `env.create` naming workspace B with A's session, or a mount source outside the session's workspace directory, is refused by the node. |
| N.4 | *With `max_running = 2` and two workspaces, A cannot start a second container while B's first starts at once. |
| N.7 | *Detaching A with an unsafe session folder or environment schedules it; B's are not classified or touched. |
| N.7 | *Deleting a session of class `b` keeps its branch as `rizzy/trash/…` and its folder in the trash for `keep_deleted_days`. |

**What it costs**, against the first version: phase 1 from M to about 9 to 10 days (attachments
about 3, the two trees and derived folders 2, links 1, sub-agent and checkpoint moves 1 to 2,
`local` pairing half a day, repository-only sessions and auto-initialisation about 1, most of it
tests); phase 1b new, 4 to 5 days; phase 3 half a day; phase 4 a day;
phase 6 a day (first clone); phase 7 two days. About 18 to 20 days in all over the first
version, most of it on the critical path before N.2.

If time is short, cut in this order: the `borrow` share (ship `strict`); `local` pairing from
the web page (keep `node pair --local`); project repositories cloned from the forge (empty
repositories and links only until N.6); per-workspace labels; local classification before an
offline detach. Do not cut the attached list, derived folders, the workspace check on the node,
the removal checks, the shared-node rule or the spool's acknowledgement-after-commit: they are
the separation and the durability.

## 9. Decided, and what remains

The owner accepted every recommendation of the 2026-11-11 draft. Nothing blocks the build.

| # | Question | Decision | Date |
|---|---|---|---|
| Q1 | Host sessions on a node with several workspaces (and on `local` always) | Allowed from Landlock ABI 9, refused below it by default (`host_shell_shared = "abi9"`); `"always"` and `"never"` as explicit settings (§4.10.3). | 2026-11-12 |
| Q2 | Spool or memory only | The spool: events are durable until the master has stored them, then deleted (§4.12). | 2026-11-12 |
| Q3 | The default base directory | `~/Workspaces`, with the base marker; `$HOME` itself is refused as a base (§4.11). | 2026-11-12 |
| Q4 | The command share's default | `borrow`, with priority for a workspace below its guarantee; `strict` as a setting (§4.10.4). | 2026-11-12 |
| Q5 | The last detach of a node | Also revokes it, except `local` (§4.10.5). | 2026-11-12 |
| Q6 | Agentic work outside a git repository | Not allowed: every agentic `files/` is in a repository; scratch sessions and new projects get one the node initialises; `node link` needs a repository or `--init` (§4.11.5). | 2026-11-12 |

**To confirm, not blocking:** A1, A2, A3, A4; R1; commit identity (§4.7) and its four open
points (author or co-author; autosave without a workspace account; what a user's own connection
may do; the fallback e-mail); which forges first, and whether device flow is enough;
unanswered from the first round: per-user caches against a shared `cache/`, environments only
for container sessions, workspace deletion refused while environments exist, always "model
alias".

**Recorded dissent (decided; not blockers):**

- Autosave of other users' sessions is the deleter's choice. I would have saved only the
  requester's and opted-in sessions.
- The dialog names users with their counts. I would have shown totals only.
- New workspaces start with no provider. I would have kept `demo` in every workspace.
- A node may serve several workspaces (O3). I recommended one workspace per node.
- `local` is shareable (2026-11-11). I recommended keeping host sessions on it for server
  admins: on `local`, a sandbox escape is the whole server.

**Answered by B.11** (2026-11-12, `docs/container-experiment.md`):

- whether `podman exec` passes signals to the process inside: SIGTERM and SIGINT yes, SIGKILL
  no (§3 there; §4.6 here);
- whether rootless `pasta` keeps a container off the host's loopback (test D14): off
  127.0.0.1 yes, off the host no, unless `--network=pasta:--map-guest-addr,none` (§5 there;
  §4.6 here). IPv6 not tested;
- that the `cmdline` of other processes is readable under Landlock: yes, every host process's
  under `sandbox-exec` (§4 there), and the `environ` of a process outside the domain was
  denied; tested against the test driver, not a sibling session.

**Open, from B.11:** whether `host_shell_shared` may relax once host sessions run in
bubblewrap with a mount view (§4.10.3, the question and the recommendation).

**Not verified:**

- whether `systemd-creds` works for user services on the distributions in the README;
- which forges offer device flow;
- that Landlock denies `fd` and `mem` of a sibling session's processes (tested only against
  the harness, `crates/cli/tests/sandbox.rs:231-253`);
- whether a `nologin` node user without linger can run rootless Podman at all (B.11 could not
  reproduce the missing runtime directory: it needs root);
- which kernels the README's distributions ship, so how many nodes reach Landlock ABI 9;
- whether the session fold gives the same view without stored deltas (decides hydration size);
- what the master's per-event fsync costs on a small home server (B.10).

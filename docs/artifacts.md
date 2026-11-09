# Artifacts: design note

Status: **Phase B built** (publishing, "Save as artifact", the node stage, transfer, ingest,
retention, the API, search, the lineage's Media item). The contract is the sidebar-areas design
(v2), §7, §9, §10 and §12 (T18); this note says what the code does and why. The blob store that
holds the files is described in [media.md](media.md).

## What an artifact is

- An **artifact** is a named lineage of files an agent built for the user ("myapp"), keyed by
  owner, workspace, project and name. A **version** is one immutable file: a single file, or a
  folder the node packed into `.tar.gz`.
- **Stored files, never rendered or run.** Artifacts are downloads. Only the kinds a browser
  may show (raster images, audio, video; SVG and HEIC only inside `<img>`) get a preview, under
  the same T17 rules as Media; HTML, XML, PDF and everything else are attachments. This is not
  the "rendered artifacts" PLAN.md rules out (second origin): nothing here is rendered.
- Each version records its source chat and tool call, the node, its size and a sha256 the master
  verified, the kind and format the master sniffed, and metadata the node reported (toolchain,
  package, git commit, target, the first 200 entries of a pack). The metadata is display only.
- "Latest" is the newest ready version.

## How a file gets in

1. **The `publish` tool** (agentic chats only, offered with an agent's edit or command tools,
   also in Coder's and Writer's lists). It is a mutating tool: in `ask` mode the user approves
   each call, node.toml `denied_tools` can block it, and a turn may publish at most 10 files
   (counted on the node). Chat-runtime sessions never get it.
2. **"Save as artifact"** on a file a tool card shows, or the chat header's field:
   `POST /api/sessions/{id}/artifacts {path, name?}`. The user clicked, so there is no approval;
   the node checks the path and acks at once (its refusal is the 409's text), then copies in the
   background and records `artifact_staged` or `artifact_stage_failed` in the chat.

Nothing is detected or uploaded automatically.

## On the node (`crates/node/src/artifacts.rs`)

- **Confinement.** The path resolves against the chat's folder and must stay inside it; where
  node.toml allows work outside it (`allow_outside_workspace`), inside one of the canonical
  `workspace_roots` (a floor the file tools lack). The node's data dir is always refused: for the
  embedded node that is the master's own data dir (master.db, `blobs/`, keys, secrets).
- **Secret-like names** (`.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`,
  `.netrc`, `.npmrc`, `.pypirc`, `credentials*`, anything under `.ssh`, `.gnupg`, `.aws`) are
  refused to the agent and always skipped inside folder packs; the user may save such a file
  explicitly. node.toml `deny_globs` adds names that are refused on both paths.
- **Opening.** `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, then `fstat` must say regular file (a FIFO
  never blocks, a device is refused), then the descriptor's real path (`/proc/self/fd` on Linux)
  must still be inside the root: a folder swapped for a symlink after the check is caught here.
  Bytes are read from the descriptor only.
- **A staged copy.** The file is copied (hashed in the same pass, stopped past `max_mb`) to
  `<data>/artifacts-stage/<stage_id>/data` (0600) with a `meta.json` record; a folder is packed
  with tar over gzip: regular files and folders only, mode kept, uid/gid 0, at most 10,000
  entries, 255-byte paths and `max_mb` uncompressed; symlinks, special files, secret-like names,
  `.git`, the data dir and denied names are skipped and counted. What the master pulls cannot
  change under it (TOCTOU).
- **The stage index.** The node keeps `(session, stage_id) → stage` in memory, rebuilt at start
  from the records (stages of sessions it does not host are deleted). A stage id from the master
  is looked up, never joined into a path; `artifact.release` removes only an indexed folder. A
  compromised master can name only stages that exist. Stages expire after `stage_ttl_hours`, and
  the oldest go beyond `stage_max_mb`.
- **Never blocking.** `artifact.fetch` is taken off the command loop before it, answered from a
  task of its own (at most 8 at once; more get `busy`), and never acked: the chunk is the answer.
  `artifact.stage` acks after the path check and copies in a task. A 200 MB copy does not delay a
  prompt.
- **The capability.** The node announces `artifacts` in its hello only when it stages files
  (`NodeConfig.artifacts` set and node.toml `[artifacts] enabled`).

node.toml:

```toml
[artifacts]
enabled = true          # the node's own off switch
max_mb = 200            # one file or folder pack (uncompressed)
stage_max_mb = 2048     # all staged files; the oldest go first
stage_ttl_hours = 24
deny_globs = []         # more names never published
```

## On the master

### Ingest (`crates/master/src/artifacts.rs`)

When the mirror applies an `artifact_staged` event (duplicates never reach it), the master
validates it, fail-closed. A failed check records the version as **failed** with a plain reason,
so nothing is silently lost, except where noted:

- the event is older than the stage TTL (a mirror rebuilt from replay): ignored, no row;
- more than 30 staged events from the node this hour: ignored, no row, audit `artifact.throttled`;
- **owner, workspace and project come from the publishing session's own mirror row**, never from
  its parent and never from the node (T7, T10). A sub-agent's row inherits owner and workspace
  only from a parent on the same node, so a node cannot plant files in another user's artifacts
  through a sub-session; a row without owner or workspace is refused with **no row** (audit
  `artifact.refused`). API-token chats therefore cannot publish;
- a well-formed stage id and sha256; an agentic, non-incognito chat; the owner may still do
  agentic work there (account grant and `agentic` permission); the node announces `artifacts`
  ("the worker node is too old to publish; update it");
- room: Artifacts on, the file limit, the account and server quotas, the free-disk floor, all
  before any byte moves ("Your storage is full (4.0 of 4 GB)…");
- caps: 8 pending versions per node; 200 lineages per owner and workspace and 1,000 per workspace,
  counting only lineages that have a ready version;
- `UNIQUE(node_id, stage_id)` makes ingest idempotent.

### Transfer (`crates/master/src/artifact_pull.rs`)

- **Pull only**, from its own pool (2 per node, 4 on the server, queued), separate from the
  upload slots. The master sends `artifact.*` only to nodes whose hello announced the capability.
- 256 KiB chunks, 4 in flight. A chunk is accepted only as the answer to its own request
  (matched on node and request id) with exactly the bytes asked for and the right `eof`;
  anything else fails the version. Chunks are handed over with `try_send` and never awaited on
  the node link. `busy` is asked again after 500 ms. 60 s idle per request, 1 h in all.
- The file is hashed and sealed into the blob store's temp writer in offset order; at the end
  size and sha256 must match the node's declaration. Then one transaction: the blob (quota
  checked again, dedupe per owner), the version ready, retention, the name index, and for image,
  video and audio the lineage's Media item. Then the stage is released.
- On failure the temp file goes, the version fails, the stage is released. A node that goes away
  leaves its versions pending; they restart at its next hello (a restarted master too), or fail
  "too old" if it comes back without the capability. A revoked node's pending versions fail.

### Retention

After each ready version: the newest `keep_versions` (5) ready unpinned versions are kept, plus
pinned ones (at most `max_pinned`, 10); failed versions: the newest 3, none older than a week. The
name index (`artifact_fts.filenames`) is rewritten to the remaining versions in the same
transaction, then unreferenced blobs go (a blob a Media item still shows stays). Hourly
(`storage::hourly`): pending versions older than the stage TTL fail "the node never sent the
file", failed versions beyond retention go, and lineages with no ready and no pending version are
deleted. Until then a lineage whose versions all failed is not listed, so its failure still shows
on the chat's card.

### One Media item per lineage

When an image, video or audio version becomes ready, its lineage's Media item (origin `agent`)
points at it, in the same transaction: added if missing (also after the owner deleted it), else
moved to the new blob. An agent iterating on a chart therefore leaves one item, not twenty, and
old blobs follow artifact retention. Deleting the Media item never touches the artifact; deleting
the artifact keeps the item and the blob it shows.

## Who sees what (T10)

- An artifact is private to its owner. "Share with workspace" makes it visible to every member
  of the workspace it was published in, guests included (view and download).
- Its *effective* workspace is that workspace while the owner is still a member, else the
  owner's Personal workspace (`ARTIFACT_WS`); a shared artifact that fell back is private again.
- Anything invisible is 404, workspace admins and server admins included; server admins see
  storage numbers only. The API token sees everything.
- Only the owner renames, shares, pins or deletes; a member with the workspace's `settings`
  permission may delete someone else's shared artifact. Only the blob's owner sets its thumbnail
  and preview hints, so a viewer can never plant one.

## API

| Method and path | Who | Answer |
|---|---|---|
| GET `/api/artifacts?workspace=&project=&kind=builds\|archives\|documents\|media\|other&mine=1&q=&limit=&cursor=` | member | `{artifacts, next_cursor, usage:{used, quota}}` |
| GET `/api/artifacts/{id}` | visible | the artifact with `versions` |
| PATCH `/api/artifacts/{id}` `{name?, visibility?}` | owner | the artifact; 400 sharing in Personal; 409 name clash; 403 not the owner |
| DELETE `/api/artifacts/{id}` | owner; `settings` for a shared one | `{ok}` |
| GET `/api/artifacts/{id}/latest/download` | visible | 302 to the newest ready version (works with the API token) |
| GET / PATCH `{pinned?, no_thumb?}` / DELETE `/api/artifacts/{id}/versions/{vid}` | visible / owner / owner | the version; 409 past 10 pins |
| GET `/api/artifacts/{id}/versions/{vid}/download` | visible | the file, as an attachment, `X-Content-SHA256` |
| GET `/api/artifacts/{id}/versions/{vid}/preview` | visible | only for inline/img kinds, else 415 |
| GET / PUT `/api/artifacts/{id}/versions/{vid}/thumb` | visible / blob owner | like Media |
| GET `/api/sessions/{id}/artifacts` | the chat's owner | `[{call_id, stage_id, version_id, artifact_id, status, error, progress, by_user, path}]` |
| POST `/api/sessions/{id}/artifacts` `{path, name?}` | the chat's owner, agentic | 202 `{requested:true}`; 409 node offline, too old, or the node's refusal |

`latest` in a list is the newest ready version, or the newest of any status while none is ready
(a first upload still pending). A version's `width` and `height` are the image's header
dimensions, read at commit without decoding (Phase C), and `null` when unknown or for anything
but an image; a page can apply the 40 MP guard to them with Media off. `progress` is in memory,
for pending versions only. Changes send
the content-free `artifacts_changed` to the owner's sockets, or to every socket for a shared
artifact.

## Configuration

```toml
[artifacts]
enabled = true
keep_versions = 5
max_pinned = 10
```

Sizes and quotas are `[storage]`'s (media.md): one file at most `max_file_mb`, an account's Media
and Artifacts together at most `account_quota_mb`.

## Protocol and events (additive, capability-gated)

- `node.hello.capabilities` += `"artifacts"`.
- `artifact.fetch {req_id, session_id, stage_id, offset, len ≤ 262144}` (not acked),
  `artifact.chunk {req_id, stage_id, offset, data (base64), eof, error?}`,
  `artifact.release {req_id, session_id, stage_id}` and
  `artifact.stage {req_id, session_id, path, name?}` (both acked).
- Event `artifact_staged {stage_id, call_id?, by, name, filename, packed, size, sha256,
  kind_hint?, source_path, notes?, meta}` (meta ≤ 16 KiB): the reducers put it on its `publish`
  card, or make a standalone item for the user's. Event `artifact_stage_failed {path, name?,
  error}`: a standalone item.

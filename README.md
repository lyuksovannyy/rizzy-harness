# harness

[![ci](https://github.com/lyuksovannyy/rizzy-harness/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/lyuksovannyy/rizzy-harness/actions/workflows/ci.yml)
[![image](https://img.shields.io/badge/ghcr.io-rizzy--harness-blue?logo=docker)](https://github.com/lyuksovannyy/rizzy-harness/pkgs/container/rizzy-harness)
[![release](https://img.shields.io/github/v/release/lyuksovannyy/rizzy-harness?include_prereleases&sort=semver)](https://github.com/lyuksovannyy/rizzy-harness/releases)

> **Pre-release.** There is no tagged release and no upgrade path yet: the
> data directory's format still changes without migrations, so an update can
> require wiping it. Do not keep anything in it you cannot lose. A security
> review ([`AUDIT.md`](AUDIT.md)) is being worked through in
> [`FIX_PLAN.md`](FIX_PLAN.md); until its Phase 2 is done, run it on
> `127.0.0.1` for yourself only.

The image is the **central server** (web UI, chats); worker nodes run on real
hosts. `ghcr.io/lyuksovannyy/rizzy-harness:edge` follows `main` after CI
passes; `latest` and `X.Y.Z` will exist from the first release. Setup:
[Central server in Docker](#central-server-in-docker).

A self-hosted coding-agent harness: one Rust binary that serves a web dashboard
for coding agents, with the agent loop running on the machine that owns the
code. See [`PLAN.md`](PLAN.md) for architecture, event schema, threat model and
phase status.

**Status:** Phases 1–8 done and most of Phase 9 (checkpoints with undo, sub-agent
worktrees, parallel answers, voice input), [rich messages](#rich-messages), and
the sidebar areas: [Routines](#routines), [Media](#media),
[Artifacts](#artifacts) and the [search panel](#the-sidebar-search-and-the-account-menu).
See PLAN.md for what is left out and why.

## Quick start

The web UI builds with pnpm 12.17.0, the version `web/package.json` pins
(`npm install --global pnpm@12.17.0` if you do not have it).

```bash
cd web && pnpm install --frozen-lockfile && pnpm build && cd ..
```

```bash
cargo build --release
```

```bash
./target/release/rizzy-harness serve
```

Open http://127.0.0.1:7777. On first run it asks for a one-time claim code
(so nobody else can take over a fresh server), creates your **admin**
account, and walks you through connecting an AI provider, your name and time
zone, and a search engine. Admins add other people under Settings → Server → **Users**; new accounts get a temporary
password and must change it at first login. Everyone sees only their own
chats. Agentic sessions run shell commands on a worker node, so an admin has
to grant them per user. Print the claim code with:

```bash
./target/release/rizzy-harness claim-code
```

Scripts can call the API with `Authorization: Bearer <token>`, using the token
printed by `rizzy-harness token`.

Without configuration the built-in **mock** provider is used, so you can try
the UI with no API key. Try `run: ls -la`, `read: README.md` or `slow: 50`.

Settings → Account → Preferences → **Appearance** picks the theme (System follows the
operating system; Light and Dark stay put) and the accent colour. A pick shows
at once and is kept with Save; leaving without saving goes back. The browser
remembers the saved choice, so a reload paints it straight away, before the
preferences arrive from the server.

The home screen shows a faint background for the mode in the bar: Search, a
magnifier that drops in from above; Ask, a wall of commonly asked questions
that is laid out anew for every new chat; Agentic, a field of 0s and 1s with a
soft light that follows the mouse (not on touch screens). The wall's lists
ship with the app (`web/public/wall`). As your history grows, topics guessed
from your own chat titles (software, robotics, careers and so on) show up more
often; the guess runs in your browser only, is never stored or sent, and your
own words never appear on the wall. **Home background** turns the motion off
(**Still**) or the backgrounds off altogether (**Off**); the system's
reduce-motion setting also keeps them still. **Also in chats** adds a quieter,
still version beside chats when the window is wide enough.

## Workspaces

Every account has a **Personal** workspace. The account menu (your name at the
bottom of the sidebar) → **Workspaces** picks the workspace you work in: new
chats go there, and the chat list, search, Routines, Media, Artifacts and the
default agent and model follow it. Create more workspaces from the same menu
(**+ New workspace…**; a server setting decides whether everyone may, or only
server admins) and add other accounts by username.

A chat stays private to the person who started it, in every workspace and for
admins too: a workspace groups chats and decides what applies to them, not who
can read them. Removing someone from a workspace, or deleting a workspace,
puts their chats back in their Personal workspace; nothing is deleted.

Roles in a workspace, each a fixed set of permissions:

| Role | Can |
|---|---|
| Owner | everything an admin can, plus delete the workspace and make or remove owners |
| Admin | change the workspace's settings, add and remove members, run agentic sessions |
| Member | run agentic sessions |
| Guest | chat only |

Nobody can give someone a role above their own or change their own role. While
a workspace has other members, its last owner (whose account is enabled) can
neither leave nor lose the role; if every owner's account is disabled or
deleted, a server admin makes another member owner (Settings → Server →
Workspaces). The last member leaving a workspace deletes it.

Settings has four sections:

- **Account**: your profile, appearance, search engines, default agent and
  model, your memories, your password.
- **Workspace** (everyone in it): its name, your role, who is in it, the
  defaults it gives, and whether your memories are used in its chats.
- **Workspace admin** (admins and owners; not for Personal): its name, default
  agent, the model each agent uses there (General's is the workspace's default
  model), members and roles, deleting it.
- **Server** (server admins): providers, memory recall, token savings, nodes,
  accounts, and every workspace (owners, members, making someone owner,
  deleting). Agents have their own page in the sidebar (below).

A new chat's model: what you pick in the bar, else the agent's own model,
else your default from Preferences, else the workspace's default, else the
server's. Agentic sessions need both the server admin's agentic grant on your
account and a role with agentic permission in the workspace; losing either
stops your running agentic work there and leaves its approvals deny-only.

## The sidebar, search and the account menu

Top to bottom: **New chat**, **Agents**, then **Routines**, **Media** and
**Artifacts** (each shown once you can use it in this workspace; Routines
carries a dot while results wait unseen), **Connections** (a placeholder for
later), then the **Projects** and **Recent** sections and your account.
Collapsed, the sidebar is a rail of icons; on a phone it is a drawer.

**Search** (the magnifier, **Ctrl K** / **⌘ K**, or **/** outside a text
field) opens the search panel over the page. Its tabs are All, Chats, Media,
Artifacts and Projects (only those you have here), and it opens on the tab you
used last. A row shows the title, the first sentence of the latest message (or
the description) and, when the words matched elsewhere, a snippet; ↑/↓ pick
a row, Enter opens it, Esc closes the panel. Search covers only the workspace you are in, and
only your own chats, for admins too. Routine run chats are found by a search
but are not listed while the box is empty.

The **account menu** has your Profile, Settings, Log out and the
**Workspaces** submenu (Personal first, then your team workspaces with your
role, **+ New workspace…** and Workspace settings). It is a keyboard menu:
Enter or ↓ opens it, → opens Workspaces, Esc goes back. Design notes:
[docs/sidebar-search.md](docs/sidebar-search.md).

## Routines

A routine starts a chat for you on a schedule or when a webhook is called:
**Routines** in the sidebar → **New routine**, then the agent, the prompt and
when it runs. Schedules are cron (5 fields), every N minutes, or once, in your
time zone; a routine has either up to 8 schedules or exactly one webhook.
**Run now** works any time. Each run is a new chat titled `<routine> · <date
time>`; it shows in Recent while it runs, waits or has an unseen result, and the
result shows on Home with a notification (each routine says whether to notify
always, on failures or never). The Runs tab lists every run with its trigger,
status, duration, tokens and the first sentence of its answer.

- **Prompts** take variables (`{{now.date}}`, `{{routine.name}}`,
  `{{last_run.status}}`, and `{{event.body.<path>}}` / `{{event.text}}` for a
  webhook); the editor's preview renders the prompt and the next runs on the
  server.
- **Agentic routines** run in `ask` or `ask_all` only: a command, an edit or a
  publish waits for your click in "Waiting for you" and is cancelled after
  24 hours. Read-only work finishes on its own.
- **Webhooks**: `POST /api/hooks/<trigger id>` with
  `Authorization: Bearer <secret>` and a JSON or plain-text body of at most
  64 KiB (`Idempotency-Key` is honoured for 24 hours). The secret is shown once
  when you save (Rotate makes a new one). The server binds 127.0.0.1 by
  default, so a webhook reaches it only from this machine, over
  [WireGuard](#using-it-from-other-devices-wireguard), or through a TLS proxy
  whose Host is allowed (`--allowed-host`).
- **What a webhook sends is untrusted.** Its data only fills the prompt; that
  chat, and every later turn in it, gets no memory (no recall, no
  `remember`), no `web_fetch` or `web_search`, and a banner saying so.
- Guests cannot own routines. A routine runs as its owner: losing the
  workspace permission or the agentic grant pauses it at the next fire.

The newest 30 seen run chats per routine are kept (older ones are deleted
hourly, unless you continued them); runs keep their status and usage. Limits,
in `config.toml` (the defaults):

```toml
[routines]
enabled = true                       # false: nothing fires, nothing new can be saved
webhooks = true                      # false: hooks answer 409
min_interval_minutes = 5             # shortest gap between two runs of one schedule
max_per_user = 25
max_per_workspace = 100
max_triggers = 8                     # schedules per routine
max_concurrent_runs = 4              # server-wide
max_concurrent_per_user = 2
max_runs_per_user_per_day = 200
hook_rate_per_minute = 10            # per webhook, burst 5
keep_runs = 200                      # per routine, and none older than 90 days
keep_run_chats = 30
approval_timeout_hours = 24
```

Details, the API and the trusted-routine seam: [docs/routines.md](docs/routines.md).

## Media

**Media** is your private library of images, video and audio: files you
upload there, images you attach to chat messages, and the media files agents
publish (one item per artifact, showing its newest version). Nobody else sees
it, admins included; the workspace only filters what you see. The grid has
thumbnails (made by your browser), filters by kind and a name search; a
preview plays audio and video and offers Download and **Use in chat**.

Attach images (PNG, JPEG, WebP or GIF; up to 4 per message) by picking,
pasting or dropping them in the composer of a chat whose model has
`vision = true`. Your browser scales each down (which also strips EXIF and GPS
data) before it is uploaded to Media; temporary chats take no images.
Deleting an item that chats still carry tells you so ("Still in N chats"),
and deleting a chat can also delete the files it brought into Media. Copies
already sent to a model provider cannot be recalled. Media is on by default:

```toml
[media]
enabled = true                       # false: no Media page, no images in chats
```

Files are recognised by their bytes, never their name; only raster images,
audio and video are shown in the page (SVG and HEIC only as images), anything
else downloads. Design notes and the API: [docs/media.md](docs/media.md).

## Artifacts

**Artifacts** holds the files agents build for you: binaries, packages,
archives, documents and media, each name with its versions. In an agentic
chat an agent calls `publish` (Coder and Writer have it, and any custom agent
that may edit files or run commands); in `ask` mode you approve each publish. You can also
use **Save as artifact** on a file a tool card shows, or the field in the chat
header. The node copies the file (a folder is packed as `.tar.gz`), the
server pulls it and checks its size and sha256, and the chat card turns into
Download / Open in Artifacts.

- An artifact is private to you until you **Share with workspace** (team
  workspaces only); members then see and download it. Rename, delete, pin a
  version (up to 10) or delete one from its row or its page.
- Each version shows its size, sha256 (copy it and compare with
  `sha256sum`), git commit, node, the chat it came from and, for a pack,
  its files. The last 5 versions are kept, plus pinned ones.
- Scripts can fetch the newest version with the API token:
  `curl -LOJ -H "Authorization: Bearer $(rizzy-harness token)" http://127.0.0.1:7777/api/artifacts/<id>/latest/download`.
- Artifacts are stored files: nothing is ever rendered or run; only images,
  audio and video get a preview.

```toml
[artifacts]                          # config.toml
enabled = true
keep_versions = 5                    # ready, unpinned versions kept per artifact
max_pinned = 10
```

Each node decides for itself in its `node.toml` (the defaults):

```toml
[artifacts]
enabled = true                       # false: no publish tool, no "Save as artifact" there
max_mb = 200                         # one file or folder (uncompressed)
stage_max_mb = 2048                  # all files waiting to upload
stage_ttl_hours = 24                 # a file the server never fetched is deleted after this
deny_globs = []                      # names never published, e.g. ["*.sqlite", "dist/private/**"]
```

A node only publishes from the chat's folder (or its `workspace_roots` when
`allow_outside_workspace` is on), never from its data directory, never
through a symlink, and refuses secret-like names (`.env*`, `*.pem`, `*.key`,
`id_rsa*`, `.ssh/`, and so on) to agents. Design notes:
[docs/artifacts.md](docs/artifacts.md).

### Storage limits

Media and Artifacts share these limits, also under Settings → Server →
**Storage**, which shows each account's usage (numbers only). A file over a
limit is refused; nothing stored is ever removed to make room.

```toml
[storage]
max_file_mb = 200                    # one upload or published file
account_quota_mb = 4096              # Media + Artifacts per account; 0 = no limit
server_quota_mb = 20480              # every account's files together
min_free_mb = 1024                   # free space kept on the data disk
```

## Configuration

Everything lives in the data directory (`~/.local/share/rizzy-harness`, or
`--data-dir` / `HARNESS_DATA_DIR`). Back it up as a whole: the files under
`blobs/` (Media and Artifacts) are encrypted with per-file keys kept only in
`master.db`, so neither is any use without the other. That is for deleting
(crypto-shredding: a deleted file's key is gone from the live database), not
for keeping the directory secret: whoever can read `master.db` can read every
file, so use full-disk encryption for that. Old copies of `master.db` pages can
survive on SSDs and copy-on-write filesystems ([docs/media.md](docs/media.md)).

**Secret files must be private.** `serve` (and `node run`) refuse to start
when a file holding a secret is readable or writable by group or others,
naming it and the fix (`chmod 600 <file>`): `secrets.toml`, `api-token`,
`claim-code`, the private keys in `keys/` (`*-key.der`, `node.key`) and the
`.env` that `serve` reads. A symlink is judged by its target. The server
creates all of these 0600 itself; this catches a copied or restored file. In
Docker, the data directory's files keep their host mode through the bind
mount: fix the mode on the host. (Unix only; on other systems nothing is
checked.)

`config.toml`: model providers. API keys never go in this file: reference an
environment variable (`api_key_env`) or a key stored in the 0600
`secrets.toml` (`api_key_secret`; onboarding and Settings → Server → AI providers write
it for you, as `provider-<id>`). Several providers can be connected at once;
model ids are `<provider id>::<model id>` (`openrouter::openai/gpt-4o`), and
each request goes to the provider its prefix names.

```toml
default_model = "openrouter::openai/gpt-4o"   # unset: agent general's, else the first model

[agent_models.coder]                 # per-agent models (Settings); a chat can still pick another
model = "deepseek::deepseek-chat"
fallbacks = ["openrouter::openai/gpt-4o"]   # tried in order (see below)

[agent_avatars.coder]                # how the agent looks; unset: derived from its id
shape = "hexagon"                    # circle | triangle | square | hexagon | diamond | drop
eyes = "wide"                        # round | long | short | wide | sleepy
accent = "green"                     # blue | violet | green | orange | pink | gray

[[providers]]
id = "openrouter"                    # the prefix: a-z, 0-9, _ and -
name = "OpenRouter"
preset = "openrouter"                # fixed presets keep their own base_url
kind = "openai"                      # openai (any OpenAI-compatible endpoint) | anthropic
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
models = [
  { id = "openai/gpt-4o", context = 128000, max_output = 16384, vision = true, tools = true },
]

[[providers]]
id = "deepseek"
name = "DeepSeek"
preset = "deepseek"
kind = "openai"
base_url = "https://api.deepseek.com/v1"
api_key_secret = "provider-deepseek" # [keys] provider-deepseek = "..." in secrets.toml
models = [
  { id = "deepseek-chat" },
  { id = "deepseek-old", unavailable = true, missing_since = 1767225600000 },  # set by the server
  { id = "my-finetune", custom = true, context = 32000, tools = true },        # added by hand
]
```

The model parameters (`name`, `context`, `max_output`, `effort`, `vision`,
`tools`) are what the provider's `/models` reports (Ollama: `/api/show`) when
the provider is tested in the UI; a missing one counts as unsupported. A stored
key is only ever sent to the base URL it was saved for: change the URL and the
key must be entered again. A provider may list no models (one used only for
memory embeddings); while no provider has a chat model the built-in demo stays
as the default, and onboarding still asks for a chat provider.

The server keeps each provider's last listing (in the store) and fetches it
again once a day, or when an admin refreshes it. A saved model the latest
listing lacks gets `unavailable = true` and `missing_since` (ms): it is kept,
and cleared when it is listed again, until an admin removes it in Settings.
`custom = true` marks a model added by hand: its parameters are the admin's
and listings never mark it. Testing a saved provider in the UI refreshes its
listing too, but only with its saved key and preset: a test with a key typed
into the form, not yet saved, changes neither the listing nor the marks.

An agent has one model; one model may serve several agents. When a turn's
model is unavailable, or its request fails before any output with a model
problem (not found, rate limit, server error, no connection), the agent's
fallbacks are tried in order, skipping unavailable ones, and the chat notes
which model answered. An agent without an `[agent_models]` entry runs on
the default (General's main model) with General's fallbacks; a sub-agent
without a model of its own runs on its parent's model with the parent's
fallbacks. A reasoning effort goes only to the models of the chain that take
one. Unavailable models in use show as notices on the home screen:
admins see all of them, other users those behind an agent's main model, the
global default or their own default.

`effort = "low" | "medium" | "high" | "max"` on a provider is its default
reasoning effort, only sent to its models with `effort = true`. Anthropic
models are always listed with `effort = false`: extended thinking combined
with tool calls is not supported until risk R8 in `PLAN.md` is fixed, so do
not set it by hand either.

Local Ollama: preset `ollama`, `base_url = "http://localhost:11434/v1"`, no key.

**Local and LAN model servers need `HARNESS_PLAIN_HTTP_HOSTS`.** The
provider and model tests in Settings and the model listing are requests the
server makes to an admin-typed URL, so they reach public addresses only,
checked on the address actually connected to. A model server on this machine,
your LAN or a container network is allowed by host name or address, comma
separated, then restart: `HARNESS_PLAIN_HTTP_HOSTS=localhost` for Ollama on
this machine, `HARNESS_PLAIN_HTTP_HOSTS=ollama,192.168.1.20` for others. The
same list allows plain `http://` with a key. A test of an unlisted private host
fails with the exact value to add. (Chats themselves use the saved provider's
URL as before.)

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
recall = "meaning"                   # keywords | meaning (all of this is settable in the UI)
source = "ollama"                    # provider | ollama: where embeddings come from
# provider_id = "mistral"            # for source = "provider": uses that provider's URL and key
embeddings_url = "http://localhost:11434/v1"  # for ollama
preset = "balanced"                  # fast | balanced | best (Ollama model names)
# embeddings_model = "nomic-embed-text"  # overrides the preset; required for provider
# mem0_url = "http://localhost:8888" # for backend = "mem0" (run with MEM0_TELEMETRY=false)
```

Memories are proposed by the assistant during chats and used only after you
confirm them under Settings → Account → Memory.

Who may create workspaces (also a switch under Settings → Server → Workspaces):

```toml
[workspaces]
members_can_create = true            # false: only server admins create workspaces
```

Who may run agentic sessions on this server's own (embedded) node:

```toml
[nodes]
local_node_users = "admins"          # admins (default) | granted
```

The embedded node runs as the server's own OS user, next to its data
directory. Its shell is sandboxed (see "The shell sandbox"), but on kernels
before Linux 7.1 it can still reach Unix sockets such as the session bus,
which can start programs outside the sandbox. So by default only
server admins (and the API token) may use it; other accounts with the
agentic grant run their agentic sessions on a remote node (`node join`), and
get "use a remote node, or ask the server admin" on the built-in one, also
for chats they started there earlier. `"granted"` lets every account with
the grant use it: choose it only if you trust each of them like an admin.

Dictation (the mic button) needs a transcription service, for example a local
Whisper server with an OpenAI-compatible API:

```toml
[voice]
url = "http://localhost:8000/v1"     # serves /audio/transcriptions
model = "Systran/faster-whisper-small"
# key_secret = "voice"               # if the service needs a key
# daily_requests = 200               # dictations per account per UTC day (0 = no limit)
# daily_mb = 100                     # MiB of audio per account per UTC day (0 = no limit)
```

Each account dictates one recording at a time (a second one at once gets
"one dictation at a time", 429) and within the daily limits above (then
429 until midnight UTC). The counts are kept in memory: a server restart
starts the day afresh.

Files attached to a message (`+` in the composer) are read on the server:
text, code, Markdown, CSV, JSON, and PDFs with a text layer. A file is a PDF
only when its first bytes are `%PDF-`, whatever its name (`notes.pdf` with
plain text in it is text). PDFs are never parsed inside the server: it
starts its own binary as `rizzy-harness pdf-text`, which locks itself out of
every file and the network (Landlock), limits its memory, and is killed after
20 seconds; two run at once and a third waits up to 10 seconds ("the server
is busy reading other PDFs"). This needs Linux 6.7 or newer with Landlock
enabled on the server; elsewhere PDFs are refused with "this server cannot
read PDFs safely", and text files still work. Each account can hold 20
uploaded files not yet sent (8 MiB of text); unsent files are deleted after
the incognito TTL (`[chat] incognito_ttl_minutes`, a day by default).

Every agentic turn is checkpointed and its file changes can be undone
from the chat. Checkpoints live in a private repository in the node's data
directory (`checkpoints/<hash>.git`), for git repositories too: nothing is
written into your folder or your `.git`, and your repository's own git
config, hooks and filters are never run for a checkpoint or an undo (in a
repository the private copy borrows your committed objects instead of
storing them again, and honours `.gitignore`, `.git/info/exclude` and your
global git ignore file). The
first checkpoint of a large repository reads every file once. Undo writes
back the exact bytes. Sub-agents that can write get a temporary git
worktree; it is refused, and the sub-agent shares the folder, if the
repository's own config defines filter commands (git-lfs's standard ones
are allowed). Only the latest 10 checkpoints per folder are kept (`checkpoints_keep`
in `node.toml`). "Rewind" undoes a turn and everything after it.

`node.toml`: the node's own policy. It is authoritative: the master can only
ask for stricter settings.

**Choose a project folder.** A session's folder may not be the node's data
directory, be inside it, or contain it: with the default data directory
`~/.local/share/rizzy-harness`, your home folder and `~/.local` are refused
(use `~/projects/x` or similar). The file tools (`read`, `write`, `edit`,
`grep`, `glob`, `outline`) never reach into the data directory, even with
`allow_outside_workspace = true` or through a symlink. The shell runs in a
sandbox that cannot open it either (see "The shell sandbox" below); its
output is still scrubbed of the API token and known keys.

```toml
max_approval_mode = "ask"            # ask_all | ask | auto
workspace_roots = ["/home/me/code"]  # session cwd must be inside one of these
denied_tools = []                    # e.g. ["bash"]
allow_outside_workspace = false
graphify = false                     # let agents query a graphify knowledge graph (see below)
env_pass = []                        # extra variables the agent's shell gets (see below)

[limits]
tool_stream_bytes = 1048576          # live output one tool call may stream (4096 .. 1 GiB)
max_steps = 50                       # model calls one turn may make (1 .. 1000)
bash_timeout_max_s = 3600            # longest a bash call may run, whatever it asks for (1 .. 3600)

[sandbox]
mode = "required"                    # required | best-effort | off
network = "on"                       # on | off (off: no TCP connect or bind)
read = []                            # more read-only folders, e.g. "~/.cargo"
write = []                           # more read-write folders, e.g. "~/.cache"
```

**The shell sandbox.** `bash` (and graphify) run through `rizzy-harness
sandbox-exec`, which locks itself into a Linux Landlock sandbox and then
starts the command. A command may:

- read and change the session folder, and its own temp folder (`TMPDIR`, a
  fresh folder per chat, deleted with the chat);
- read and run `/usr /bin /sbin /lib* /etc /opt` and read `/proc`;
- use `/dev/null /dev/zero /dev/full /dev/random /dev/urandom`;
- read the folders in `[sandbox] read`, read and change those in
  `[sandbox] write`.

Nothing else can be opened: not the rest of your home folder (`~/.ssh`,
`~/.aws`, browser profiles), not the data directory, not `/tmp`, not other
users' folders, not `/dev/tty`. A sub-agent's git worktree also gets the
repository's `.git`, so `git status` and `git commit` work there. Programs
cannot gain privileges (`no_new_privs`): `sudo` and other setuid programs
fail inside the sandbox. From Linux 6.12 the shell also cannot signal
processes outside it, and from Linux 7.1 it cannot connect to Unix sockets
outside its folders (the session bus, the Docker socket, an ssh-agent); on
older kernels it **can**, see "Limits" below.

It needs Linux 6.2 or newer with Landlock enabled (`landlock` appears in
`/sys/kernel/security/lsm`; `network = "off"` needs 6.7). `rizzy-harness
doctor` shows whether it works and which Landlock version the kernel has.
With `mode = "required"` (the default) a node where it does not work refuses
agentic sessions with a message naming this setting; `node run` refuses to
start, `serve` starts for chats only. `"best-effort"` uses whatever the
kernel enforces and logs a warning; `"off"` runs the shell with every right
of the node's OS user and logs that on every start.

What breaks by default, and the fix: toolchains and caches under your home
folder. A command that fails with `Permission denied` on a path under `~`
needs that folder listed. The usual entries:

```toml
[sandbox]
read  = ["~/.rustup", "~/.cargo/bin", "~/.nvm", "~/.local/bin", "~/.pyenv",
         "~/.sdkman", "~/go/bin", "~/.gitconfig"]
write = ["~/.cargo/registry", "~/.cargo/git", "~/.npm", "~/.cache",
         "~/go/pkg", "~/.local/share/uv", "~/.gradle", "~/.m2"]
```

(`~/` is the node user's home; give `~/.cargo/registry` rather than all of
`~/.cargo`, which may hold `credentials.toml`.) Also missing by default:
`/tmp` (use `$TMPDIR`), `/dev/shm` (some Python and browser tests),
`/sys`, `/var`, `/run`, `/nix/store` on NixOS. A shell sandbox entry may not
contain, equal or be inside the data directory: the node refuses to start
with such an entry, because Landlock cannot carve a folder out of a grant.
For the same reason a data directory under `/usr`, `/etc` or `/opt` is
refused. Graphify additionally needs its install (found automatically:
its venv and the Python it was made from).

Limits: Landlock does not cover UDP, so `network = "off"` still allows DNS
and any UDP traffic; it does not hide that a file exists (`stat` and
`test -e` still work on paths outside, only opening is denied); `/etc` is
readable; the session folder's own `.git` is writable, so a command can
change that repository's hooks and config (the file tools refuse to).
Before Linux 7.1 a command can connect to Unix sockets such as
`/run/user/<uid>/bus` or `/var/run/docker.sock`, and through them start
programs outside the sandbox (`systemd-run --user`, `docker run -v
/:/host`): on such kernels run the node as its own OS user, without
membership in the `docker` group and without a login session.

**The agent's environment.** The shell and every other program a tool
starts get an empty environment plus `PATH HOME USER LOGNAME SHELL LANG
LANGUAGE LC_* TERM COLORTERM TZ TMPDIR`, and the names `env_pass` matches
(exact names or globs: `*`, `?`, `[...]`). Nothing else the server or node
was started with (cloud credentials, tokens, an SSH agent) reaches the
agent. Provider key variables, names loaded from `.env` and every
`HARNESS_*` setting are never passed, even when `env_pass` matches them. A
tool that worked before and now cannot find its home needs an entry here.
The usual toolchain variables:

```toml
env_pass = ["CARGO_HOME", "RUSTUP_HOME", "GOPATH", "NVM_DIR", "PYENV_ROOT",
            "VIRTUAL_ENV", "JAVA_HOME", "SSH_AUTH_SOCK",
            "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"]
```

Passing `SSH_AUTH_SOCK` lets the agent use every key in your SSH agent. An
invalid entry stops the node at start; `rizzy-harness doctor` prints the
effective list.

**Streamed output.** What a command prints is shown live and stored, up to
`[limits] tool_stream_bytes` per call (default 1 MiB). Past it the stream
ends with `[output truncated: further output is not shown or stored]`; the
command keeps running until it exits, times out or is cancelled, and the
rest of its output is read and dropped. The result the model gets has its
own, smaller cap and says when the live stream was cut.

**Step and time limits.** A turn ends after `[limits] max_steps` model calls
(default 50, 1 to 1000; a sub-agent's turn has the same cap) with the message
"stopped after N steps (`max_steps` in node.toml)". `bash` runs at most
`[limits] bash_timeout_max_s` seconds (default 3600, 1 to 3600): a call that
asks for more is cut at that value, and the tool description the model sees
states it. Neither an agent definition, a prompt nor the master can raise
either; a value outside its range stops the node at start.

**File limits.** The file tools only open regular files: a FIFO, a device
(`/dev/zero`), a socket or a directory is refused at once, also through a
symlink. `read` streams a file and stops after 32 MiB, saying where (use
bash `tail` or `sed -n` for later parts); `grep` skips files over 8 MiB and
says how many it skipped; `edit` and `outline` refuse files over 8 MiB.
Cancelling a turn stops any tool: one that does not return by itself
within 2 seconds is abandoned. A slow undo or delete in one chat never
delays Stop or an approval answer, in that chat or any other.

### Token savings

Settings → Server → Token savings switches each method on or off; it is
stored as `[savings]` in `config.toml` and applies from the next message.

| Method | Default | Applies to | What it does |
|---|---|---|---|
| `prompt_cache` | on | chat + agentic | Marks the stable start of each request (instructions, tools, history) cacheable; Anthropic bills re-reads at 10% or less. Per-turn context (memory recall) is sent after the history so it does not invalidate the cache. |
| `mask_old_outputs` | on, keep 8 | agentic | Tool outputs older than the newest 8–15 become a one-line note; hidden in batches so the cache stays valid. |
| `filter_shell_output` | on | agentic | Colours, progress bars, build noise and passing-test lines removed, repeats folded; the full output stays on the node (`tool_output`). |
| `bounded_reads` | on | agentic | 400-line read windows, 60 grep matches grouped by file. |
| `outline_tool` | on | agentic | `outline`: a file's definitions with line numbers (Rust, Python, JS, TS, Go). |
| `repo_map` | off | agentic | A ranked map of the repository's definitions (tree-sitter + PageRank) in the instructions. |
| `web_readability` | on | chat + agentic | `web_fetch` returns the main article as Markdown. |
| `chat_style` / `agentic_style` | lite / lite | | Answer style: `off`, `lite`, `full`, `ultra`. Chats are capped at `lite`. |
| `graphify` | off | agentic | `graph_query` tool, only on nodes that allow and installed it. |

```toml
[savings]
agentic_style = "full"
repo_map = true
repo_map_tokens = 1024
```

**graphify (optional).** A third-party Python program (Graphify-Labs/graphify,
Apache-2.0) that builds a knowledge graph of the repository. Each node,
the built-in one too, decides for itself; no web page or API call installs it
or changes `node.toml`. On the node:

```bash
rizzy-harness graphify install
```

then add `graphify = true` to that node's `node.toml` and restart it (the file
is read at start only). Settings → Server → Token savings shows whether it is
installed and allowed on the built-in node. The graph is stored in the node's
data folder, never in your repository, and graphify gets no API keys.

`rizzy-harness doctor` checks the setup.

## Projects

A project groups chats around one piece of work inside a workspace:
**Projects** in the sidebar → **+**, then pick a
name, an icon, an accent and a board template. In a team workspace owners and
admins manage projects; every member sees them and works in them with their
own chats. Start chats from the project's page, or add a chat from its header's
project picker. Every message in a project's chats gets, after the history:
the project's **instructions** (Settings tab, up to 4,000 characters), the
project **memory** recalled for that message, and the open cards of its
**board**. The model proposes project facts with `remember` (scope
`project`); a proposal shows only to the owner of the chat it came from until
they keep it on the project's Memory tab. Keeping your personal memory out of
a team workspace leaves its projects' memory in. Agentic chats started in a project
take its default node and folder. Temporary chats cannot join a project.
Design and open questions: [docs/projects.md](docs/projects.md).

## Rich messages

Replies render as Markdown: headings, lists, GFM tables (click a header to
sort, **Copy CSV**), task lists, footnotes, callouts (`> [!NOTE]`, `[!TIP]`,
`[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`), code with its language, **Copy**
and syntax highlighting once the reply is finished, and math written as
`\( … \)` or `$$ … $$` (a single `$` stays a dollar sign). **Copy** on a
reply copies its Markdown.

When a search or a fetched page backs a reply, its claims carry citations
such as `[S2]`: a pill naming the site, the title and address on hover, and
a **Sources** list under the reply. A citation can only point to a page the
web tools actually returned.

Agents can also put these blocks in a reply (a fence with `ui:<type>` and a
JSON body, which the server checks before anything is shown):

| Block | Shows |
|---|---|
| `ui:choices` | A question with options to tap (one or several, optionally "Other…"). Ends the reply. |
| `ui:form` | A short form: text, number, choice, toggle and date fields. Ends the reply. |
| `ui:progress` | A progress card that the agent updates as a job moves on; earlier states fold into one line each. |
| `ui:chart` | A bar or line chart, with **Show data** and **Copy CSV**. |

An answer to a question or form is sent as your next message. The agent gets
only the ids and values you chose (`Choice answer · colour: blue`), plus
anything you typed. Typing a normal message instead leaves the question
unanswered.
Never type passwords or keys into a chat: fields and questions that ask for
them are refused, and keys belong in Settings.

Each agent decides which blocks it may use, with `ui:` in its file
(`<data>/agents/<id>.md`; a file with a built-in's id, such as `coder.md`,
replaces that agent, so start from its copy in `crates/agents/builtin/`):

```markdown
---
name: Reviewer
description: Reads code and reports problems.
tools: [read, grep, glob]
ui: display        # rich (default): every block; display: progress and charts; off: none
---
You review code and report problems. You do not edit files.
```

With `off`, `ui:` fences stay ordinary code. Sub-tasks an agent delegates are
limited to `display`.

**Deliberately not loaded:** images in replies are never fetched (you see a
chip naming the image and its site, which works like a link), and there are
no link previews, favicons or unfurls, because each of them would tell a
site the model chose that you are reading the reply. Links always show their
real site, and an unusual one (with a query, or a long or odd address) shows
the full address and asks before it opens.

With the built-in demo provider, try `rich: demo`, `ui: choices`, `ui: form`,
`ui: chart`, `progress: 3` (Agentic mode) and `cite: rust` (needs a search
engine).

## Using it from other devices (WireGuard)

Bind the UI to this machine's WireGuard address; it is then reachable only by
your WireGuard peers. Add `--tls` so the browser treats the page as secure
(dictation and notifications need that), and trust `keys/ui-cert.pem` in the
data directory once on each device:

```bash
./target/release/rizzy-harness serve --bind 10.8.0.1:7777 --tls
```

With `--tls` every response for a host name (not `localhost`, not an IP
address) carries `Strict-Transport-Security: max-age=15552000` (180 days,
this host only): the browser then refuses plain `http://` for that name. If
you turn `--tls` off again, clear the name in the browser's HSTS settings or
wait out the 180 days.

The browser's `Origin` must match the server's scheme: a page served over
`http://` cannot drive a `--tls` server, and the reverse. Behind a reverse
proxy that terminates TLS for a plain-HTTP server, allow the proxy's origin
with its scheme: `--allowed-host https://chat.example.com` (a bare
`host:port` entry means the server's own scheme). `X-Forwarded-Proto` is not
read.

## Central server in Docker

Ready-made templates live in [`docker/`](docker/): **minimal** (the harness
alone) and **full** (with Qdrant and Ollama for memory recall by meaning),
each configured through a `.env` file
([`docker/env.reference`](docker/env.reference)). Settings in `.env` win over
`config.toml` and show read-only in the UI, without overwriting what
`config.toml` already holds. The rest of this section is the single-container
setup with host networking.

The master (web UI, chats, node management) can run in a container. **Nodes
cannot, by design**: a node needs the real host (WireGuard, the firewall, the
user's files and tools), so the image runs no agentic node; agentic sessions go
to remote nodes you set up by hand ([Set up a node by hand](#set-up-a-node-by-hand)).

```bash
mkdir -p data && sudo chown 10001:10001 data
```

```bash
HARNESS_TAG=edge docker compose up -d
```

(`HARNESS_TAG` defaults to `latest`, the newest release; `edge` is `main`. To
build this checkout instead, uncomment `build: .` in `compose.yaml`.)

```bash
docker compose exec harness rizzy-harness claim-code
```

Then open `http://127.0.0.1:7777` (from elsewhere: `ssh -L 7777:127.0.0.1:7777`
or the WireGuard address). Notes:

- `compose.yaml` uses **host networking** so the master can reach nodes over
  the host's WireGuard interface and bind loopback / the tunnel address exactly
  as on bare metal. With host networking `0.0.0.0` is refused.
- Without host networking (UI and chats only), publish on loopback:
  `-p 127.0.0.1:7777:7777`. Inside its own network the container may bind
  `0.0.0.0` (`HARNESS_CONTAINER=1`); **never publish on `0.0.0.0`**: Docker's
  published ports bypass ufw/iptables rules on the host. A different host
  port needs `HARNESS_ALLOWED_HOSTS=127.0.0.1:8080` (the Host header check).
- For remote nodes add `--node-listen <host VPN address>:7443` to the command.
- The image's binary is built on Debian 13: copied to a node, it runs on Debian 13
  and Ubuntu 26.04, not on older glibc.
- Building yourself and hitting Docker Hub rate limits:
  `docker build --build-arg REGISTRY=mirror.gcr.io/library .`
- Published images are linux/amd64 only.

## Remote worker nodes

Agentic sessions can run on other machines. Start the master with a node port
(TLS with a pinned self-signed certificate; only joined nodes get in):

```bash
./target/release/rizzy-harness serve --node-listen 0.0.0.0:7443
```

In Settings → Server → Nodes choose **Create join command** and run the two
printed commands on the other machine (same binary). That machine needs its own
`config.toml` (provider + key) and `node.toml` (its workspace roots and
limits: the node, not the master, decides what may run there). Revoke a node in
the same place.

### Set up a node by hand

On the node machine, as root (Debian/Ubuntu; adjust for others). The node runs
as its own unprivileged user and only works in that user's folder:

```bash
useradd --create-home --shell /usr/sbin/nologin rizzy-harness
install -d -o rizzy-harness -g rizzy-harness -m 0700 /var/lib/rizzy-harness /home/rizzy-harness/work
install -m 0755 rizzy-harness /usr/local/bin/rizzy-harness    # the same binary as the master
```

`/var/lib/rizzy-harness/node.toml` (owned by `rizzy-harness`):

```toml
# This node decides what the master may do here.
max_approval_mode = "ask"
workspace_roots = ["/home/rizzy-harness/work"]
```

Put the node's `config.toml` (provider and key) next to it, mode 0600. Then
open a shell as that user and paste the first command from **Create join
command** (the join token stays in the shell's environment, out of any
process's argument list):

```bash
sudo -u rizzy-harness bash
export HARNESS_DATA_DIR=/var/lib/rizzy-harness
# paste: HARNESS_JOIN_TOKEN=… rizzy-harness node join wss://<master>:7443 --fingerprint …
exit
```

Instead of the second command (`node run` in a terminal), run the node as a
service, `/etc/systemd/system/rizzy-harness-node.service`:

```ini
[Unit]
Description=rizzy-harness worker node
After=network-online.target
Wants=network-online.target

[Service]
User=rizzy-harness
ExecStart=/usr/local/bin/rizzy-harness --data-dir /var/lib/rizzy-harness node run
Restart=always
RestartSec=5
# The binary stops cleanly on Ctrl-C (SIGINT) only, not on SIGTERM.
KillSignal=SIGINT
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/rizzy-harness /home/rizzy-harness

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload && systemctl enable --now rizzy-harness-node
```

**Hardening the host is your job**: the firewall, SSH (keys only, no root
login), updates, and how the node reaches the master (a VPN such as WireGuard
rather than the open internet). rizzy-harness does not touch them.

The **Ultracode** agent splits big jobs into parallel sub-tasks for Coder,
Scout and Writer (at most 4 per turn and 200k tokens by default; set
`delegates`, `max_subagents` and `max_subagent_tokens` in an agent file).

**Agents** in the sidebar lists every agent. Server admins can make agents there
(name, handle, summary, instructions and feature switches: memory, project
memory, web search, files, commands, sub-agents, and whether it is offered as an agent,
a sub-agent or both), change them, delete them, and change any agent's look.
Built-in agents and agent files keep their name, handle, instructions and
features: only their summary and **additional instructions** (added after
their own) change there, and **Duplicate** makes an editable copy. **Write
from instructions** asks the default model for a summary. An agent file can
set `memory: false` (no recall, no `remember`), `project_memory: false` (no
project facts in project chats) and `available_as: [agent]` or
`[subagent]` (default both). Deleting an agent leaves its chats to the
default agent. Design notes: [docs/agent-pages.md](docs/agent-pages.md).

## Development

```bash
cargo test --workspace
```

```bash
cd web && pnpm test && pnpm typecheck
```

UI dev server with hot reload (proxies `/api` to a running `rizzy-harness serve`):

```bash
./target/debug/rizzy-harness serve --allowed-host localhost:5173
```

```bash
cd web && pnpm dev
```

Web bundle budget (also in CI):

```bash
cd web && pnpm build && pnpm size
```

End-to-end UI flows against the real binary (needs Playwright + Chromium):

```bash
node web/e2e/phase3.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/phase4.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/phase5.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/phase6.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/phase7.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/phase8.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/phase9.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/attention.cjs target/debug/rizzy-harness
```

Routines, Media and Artifacts (media.cjs needs ffmpeg to make its sample files):

```bash
node web/e2e/routines.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/media.cjs target/debug/rizzy-harness
```

```bash
node web/e2e/artifacts.cjs target/debug/rizzy-harness
```

Rich messages (Markdown, citations, `ui:` blocks; sections in `web/e2e/rich/`):

```bash
node web/e2e/rich.cjs target/debug/rizzy-harness
```

Token savings page (the graphify card is read-only; nothing installs it):

```bash
node web/e2e/savings.cjs target/debug/rizzy-harness
```

Token savings against a real model (key from the environment, never printed):

```bash
BENCH_BASE_URL=https://api.example/v1 BENCH_KEY=... BENCH_MODEL=some-model cargo run -p harness-agent-native --example savings_bench -- 3
```

The container (a running image published on `127.0.0.1:7781`, with a
config.toml without providers, i.e. the built-in demo one, in its data volume):

```bash
node web/e2e/container.cjs http://127.0.0.1:7781/ "$(docker exec <container> rizzy-harness claim-code)"
```

The Phase 1 acceptance test (`crates/cli/tests/acceptance_restart.rs`)
SIGKILLs the real binary mid-turn, restarts it, resumes a WebSocket client from
its cursor, and asserts the client and server logs are identical.

## Licence

MIT. See [`LICENSE`](LICENSE).

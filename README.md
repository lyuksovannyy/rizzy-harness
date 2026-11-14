# harness

[![ci](https://github.com/lyuksovannyy/rizzy-harness/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/lyuksovannyy/rizzy-harness/actions/workflows/ci.yml)
[![image](https://img.shields.io/badge/ghcr.io-rizzy--harness-blue?logo=docker)](https://github.com/lyuksovannyy/rizzy-harness/pkgs/container/rizzy-harness)
[![release](https://img.shields.io/github/v/release/lyuksovannyy/rizzy-harness?include_prereleases&sort=semver)](https://github.com/lyuksovannyy/rizzy-harness/releases)

> **Pre-release.** There is no tagged release and no upgrade path yet: until
> the first tag the data directory's format still changes in place, so an
> update can require wiping it. Each database records its schema version, and
> a build refuses to start on one it would misread, naming the file and
> leaving it unchanged. A database a newer build wrote (a downgrade, a rolled
> back update) means: run that build or a newer one; the data is intact. A
> database whose tables are not those of its version, including one from
> before schema versions whose tables are not exactly version 1's (a data
> directory from an earlier pre-release commit), means, until the first tag:
> remove the data directory (or move it aside) and start again. Do not keep
> anything in it you cannot lose. A security
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

It listens on `127.0.0.1:7777` and `[::1]:7777`, the two addresses a browser
tries for `http://localhost:7777`, and stops with an error if another program
already holds either: that program would otherwise answer for an address this
server calls its own, and could be handed your login. (On a machine without
IPv6 it listens on `127.0.0.1` only and accepts no `[::1]` address.) While
the server is stopped it holds nothing: a command of a second node on the
same machine keeps running then and can take the port, an open tab keeps
calling that address, and the next start is refused until the command ends.
If an agent's node shares the machine with the server, serve with `--tls`
(the other program has no certificate the browser trusts) or set
`[sandbox] network = "off"` on that node.

Open http://127.0.0.1:7777. On first run it asks for a one-time claim code
(so nobody else can take over a fresh server), creates your **admin**
account, and walks you through connecting an AI provider, your name and time
zone, and a search engine. Admins add other people under Settings → Server → **Users**; the server makes each new
account a temporary password (and a new one on **Reset password**), shows it to the admin
once with a copy button, and the user must change it at first login. Nobody chooses a
temporary password, and it is never shown again; a reset also signs the user out
everywhere. Scripts do the same with `POST /api/users` (`{"username": …}`) and
`POST /api/users/{id}/password`; each answer carries the password in its `password` field. Everyone sees only their own
chats.

Scripts use the API token (`rizzy-harness token`) as `Authorization: Bearer …`, as before. A
browser login's cookie alone no longer authenticates anything. Browsers send a cookie to every
port of its host, so a server the agent starts on your machine (a dev server on
`127.0.0.1:8000`, say) would receive it when you open that page. Each login therefore also has a
login key. It comes in the body of the login answer (`login_key`) and must go with the cookie
as the `x-harness-login` header, or as a WebSocket's first message
(`{"type":"login","key":…}`). The web UI keeps the key in this origin's local storage, so a new
tab stays signed in; signing out forgets it. A file the browser fetches by itself (a picture, a
video, a download) gets a short-lived link from the UI for that one address. Copied out of the
browser, such a link does nothing without the login. Agentic sessions run shell commands on a worker node, so an admin has
to grant them per user. Print the claim code with:

```bash
./target/release/rizzy-harness claim-code
```

Scripts can call the API with `Authorization: Bearer <token>`, using the token
printed by `rizzy-harness token`. Treat this token as an admin password: it acts
as an administrator and can read and drive every user's chats, which is an
accepted gap until per-user tokens exist (`PLAN.md`, T29). Do not give it to
automation that only needs one user's access, and do not copy it if no script needs it.

Without configuration the built-in **mock** provider is used, so you can try
the UI with no API key. Try `run: ls -la`, `read: README.md` or `slow: 50`.

Settings → Account → Preferences → **Appearance** picks the theme (System follows the
operating system; Light and Dark stay put) and the accent colour, one of eight:
Blue (the default), Purple, Pink, Red, Orange, Yellow, Green and Graphite
(Graphite leaves the interface grey apart from status colours). A pick shows
at once and is kept with Save; leaving without saving goes back. The browser
remembers the saved choice, so a reload paints it straight away, before the
preferences arrive from the server. The earlier names Violet and Gray are
gone: a saved preference with one of them shows as Blue until you pick again,
and an agent look in `config.toml` that says `violet` or `gray` must be
changed to `purple` or `graphite`. The interface follows the Rizzy Glass
design system ([docs/rizzy-glass.md](docs/rizzy-glass.md)); with the
system's "reduce transparency" setting its glass surfaces turn solid.

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

- **Account**: your profile, appearance, search engines, default agents (one
  for chats, one for Agentic chats) and model, your memories, your password.
- **Workspace** (everyone in it): its name, your role, who is in it, the
  defaults it gives, and whether your facts (your memories) are shared with
  its chats: always in Personal; in a team workspace off until you turn it
  on, for the workspace's creator too.
- **Workspace admin** (admins and owners; not for Personal): its name, default
  agents (chats and Agentic), the model each agent uses there (the Assistant's, `general`, is the
  workspace's default model), members and roles, deleting it.
- **Server** (server admins): providers, memory recall, token savings, nodes,
  accounts, and every workspace (owners, members, making someone owner,
  deleting). Agents have their own page in the sidebar (below).

A new chat's model: what you pick in the bar, else the agent's own model,
else your default from Preferences, else the workspace's default, else the
server's. Which agent chats you may start is your **level** (see "Execution
levels" below): the lower of your account's level, set by a server admin, and
your level in the workspace, set by its admins (in Personal, your account's
level). Lowering either stops your running chats that are above the new level
and leaves their approvals deny-only; your other chats go on.

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

What you type and have not sent stays in this browser as a draft of that
chat, for your account only. Logging out (or a login that ended) clears every
draft in the browser, with your recent folders and test events; deleting a
chat deletes its draft, here or on another device; incognito chats keep no
draft.

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
  when you save (Rotate makes a new one). The server binds loopback
  (`127.0.0.1` and `[::1]`) by default, so a webhook reaches it only from this machine, over
  [WireGuard](#using-it-from-other-devices-wireguard), or through a TLS proxy
  whose Host is allowed (`--allowed-host`).
- **What a webhook sends is untrusted.** Its data only fills the prompt; that
  chat, and every later turn in it, gets no memory (no recall, no
  `remember`), no `web_fetch` or `web_search`, and a banner saying so.
- Guests cannot own routines. A routine runs as its owner, at their level on
  the day it fires (its chat is always `sandbox`): losing the workspace
  permission, or a level below `sandbox`, blocks it at the next fire.

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
chat an agent calls `publish` (Software Developer, Senior Teamlead and Writer
have it, and any custom agent
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

**One process per data directory.** `serve`, `node run` and `node join` hold
a lock on `<data>/lock` for as long as they run. A second one started on the
same directory (typing `serve` twice, a `node run` pointed at the server's
directory) ends at once with one line naming the directory and the running
holder's pid, and changes nothing there; before, it emptied the running
node's session temp folders, removed its sub-agents' folders and closed its
running turns as interrupted. The kernel releases the lock however the holder
ends (`kill -9`, a crash), so nothing stale is left to clean up; the file
itself stays and only names the last holder. `doctor`, `token`, `claim-code`
and `graphify` take no lock and work beside a running server; `doctor` says
whether the lock is held and by which pid. A server and a node on the same
machine need a data directory each (`--data-dir`): both keep `node.db`,
`checkpoints/` and `sandbox-tmp/` in theirs. On a file system without file
locks (some NFS or FUSE mounts) the start goes on with a warning and nothing
stops a second process: start only one there.

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
accent = "green"                     # blue | purple | pink | red | orange | yellow | green | graphite

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

An agent has one model; one model may serve several agents. A model call
that fails before any output for a passing reason (rate limit 429, server
error 500/502/503/504, Anthropic's overloaded 529, no connection, a stream
that breaks before its first word) is retried up to 3 times, waiting 2, 4,
then 8 seconds (with jitter), or what the provider's `Retry-After` asks; a
cancel ends the wait at once. A `Retry-After` over 60 s (a quota window, say)
is not retried: the agent's next fallback model answers at once, or without
one the turn ends with a message naming the wait. A refused key, a missing
permission, an exhausted quota or a bad request is not retried, and nothing
is retried once part of the answer is on screen. The chat shows the turn as
still running while it waits. When a turn's
model is unavailable, or its request still fails before any output with a model
problem (not found, rate limit, server error, no connection), the agent's
fallbacks are tried in order, skipping unavailable ones, and the chat notes
which model answered. An agent without an `[agent_models]` entry runs on
the default (the Assistant's main model) with the Assistant's fallbacks; a sub-agent
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
# summarize_after_tokens = 24000     # unset: the model's context window less a reserve
                                     # (24000 when the window is unknown); 0 = never

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

Older turns are summarised when the history nears the model's context window
(the window from the provider's model listing or your `context` on a model,
less a fifth of it, at least 16k, for the reply and tool results), measured
with the token count the provider reported for the previous turn. Set
`summarize_after_tokens` to use a fixed number instead. If a provider still
refuses a request as too long, the turn summarises older turns, hides all but
the newest large tool output, and tries once more; a second refusal ends the
turn with a message saying so. A summary is only made by a model whose window
holds what it reads: a long history is summarised in pieces, and a single turn
larger than the window is left unsummarised rather than skipped. A summary
that is not at least a tenth smaller than the history it replaces is thrown
away (nothing is logged, the history stays as it was). After two failed or
useless summaries in one turn, the turn does not try again; if the context
then does not fit, the error says summarising was tried and did not help.

In agentic chats the summary is written for coding work, under six plain-text
headings: **Goal** (what you asked for, in your words where possible),
**Decisions** (and why), **Files** (each path changed or important, with what
changed), **Commands and results** (builds and tests run, what passed or
failed), **Open** (what is not done, errors not yet fixed) and **Next step**,
up to about 1,000 words. The agent's task list is given back on its own, so the
summary refers to it rather than copying it. A later summary updates these
sections rather than nesting the earlier one, so the shape holds however often
a long chat is summarised. Chat-profile chats keep a shorter general summary
(at most 300 words). The two newest turns are kept as they are. The latest
summary is shown, collapsed, where it was made in the chat.

In agentic chats the agent keeps its own task list with the `todo` tool
(Software Developer, Senior Teamlead, Writer, custom agents that edit files
or run commands, and agents with `tools: all`; add `todo` to a file agent's
`tools` to give it one). Each call replaces the whole list: at most 50 items
of one line (200 characters), each `pending`, `in_progress` or `done`, at
most one in progress. The list is stored in the chat's log, so it survives
summarising, a node restart and a reload; it is pinned above the composer,
collapsed when every item is done or no turn is running, and after older turns
are summarised the agent is given the list again with the summary. It writes
only the chat's own log, so no approval mode asks about it, `ask_all`
included (a node can still refuse it with `denied_tools = ["todo"]`). A
sub-agent keeps its own list in its own session. A new Agentic chat starts
with the Agentic default agent (your `default_agentic_agent`, else the
workspace's, else the Software Developer), not the chat default.

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
which can start programs outside the sandbox (contained in bubblewrap, only
those inside the folders it is granted, and abstract ones below Linux 6.12
with the network on). So by default only
server admins (and the API token) may use it; other accounts run their
agentic sessions on a remote node (`node join`), and get "use a remote node,
or ask the server admin" on the built-in one, also for chats they started
there earlier. `"granted"` lets every account use it within its level:
choose it only if you trust each of them like an admin.

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

The model is sent your message as you wrote it and, after it, each file as a
quoted document under a line that names it. Nothing in a file, its name
included, is ever taken as your words: the model is told it is the file's
content, not your request (though a model may still be swayed by what it
reads), and a link that is only inside a file is not fetched on the file's
say-so. The same holds for an image's name, which older messages show in
place of the image.
To have such a link read, paste it into your message.

Every agentic turn is checkpointed and its file changes can be undone
from the chat. Checkpoints live in a private repository in the node's data
directory (`checkpoints/<hash>.git`), for git repositories too: nothing is
written into your folder, no objects, refs, index or config are ever written
into your `.git` (git may update the timestamps of existing object files
there when a checkpoint reads content your repository already has), and
your repository's own git
config, hooks and filters are never run for a checkpoint or an undo (in a
repository the private copy borrows your committed objects instead of
storing them again, and honours `.gitignore`, `.git/info/exclude` and your
global git ignore file). The
first checkpoint of a large repository reads every file once. Undo writes
back the exact bytes. Sub-agents that can change files (with `write`, `edit` or `bash`) get
a folder of their own, a copy of yours that the node keeps in its data
directory (`sessions/<id>/files`), so your folder must be a git
repository (inside the node's `workspace_roots`, with a plain `.git`
directory) for them: elsewhere such a sub-agent is not started and the
agent that delegated is told why. The copy holds the folder as it is,
your uncommitted work included, and a git repository of its own whose
history is yours (`git log` works; `git status` shows your uncommitted
work as changes against `HEAD`), as bytes: your repository's filters are
not applied (an LFS repository's files arrive as the real files, line
endings are not converted). No other chat can reach it, and it is
deleted when the sub-agent ends. When the sub-agent ends, the files it changed are written into
the folder; if you or another sub-agent changed one of them meanwhile,
nothing is written and the agent is told. None of this runs your
repository's config, hooks, filters or attributes. Sub-agents that only
read work in the folder itself. Only the latest 10 checkpoints per folder are kept (`checkpoints_keep`
in `node.toml`). "Rewind" undoes a turn and everything after it. Before an
undo or rewind changes a file it saves the folder as it is, your own edits
since the turn included, as a checkpoint of its own, so "Undo anyway" loses
nothing: restoring that checkpoint (its id is in the chat's log, phase
`before_restore`; `POST /api/sessions/<id>/turns/<that id>/restore`) undoes
the undo. The chat has no button for it yet. At the end of each turn the
old versions of the files it changed are copied into the private
repository, and at the start of each turn so are your own changes since
the previous one, so you can commit, rewrite history and `git gc`
freely: the undo and rewind of kept turns still work. Not covered: old
versions that were already gone when they would have been copied (the
agent itself rewrote and pruned history during the turn, in which case
the chat says that turn's undo is incomplete, naming the files; or you
pruned your own between-turns change before the next turn started); such
an undo is refused with that reason and your folder is left exactly as it
was. The private repository's cleanup runs now and then after a turn;
while it runs, the next turn's checkpoint waits for it. If writing fails partway (a full disk, a read-only
folder), the error names the checkpoint that holds your folder as it was;
restoring it puts the folder back. If an undo would replace a folder by a
file and that folder holds files no checkpoint has (build output your
`.gitignore` skips, say), it is refused and names the folder; "Undo
anyway" deletes them and says which. If a checkpoint cannot be taken, the
chat says that turn cannot be undone.
A git repository inside your
folder (a sub-folder git treats as its own repository: a clone, a
submodule, a worktree, or one made with `git init`) is not covered by undo
and is never touched by it: checkpoints leave it out, and an undo neither
restores, deletes nor recreates anything in it. Use git in that repository
to undo changes there. The chat says so after an undo when such a
repository appeared or went away during the turn; each checkpoint in the
chat's log lists the repositories it left out (`excluded`), which the chat
does not show yet. Files a checkpoint already holds stay covered even if a
`.git` appears in their folder later: an undo puts them back and leaves the
`.git` as it is. An undo is refused while another chat is working in the
same folder (or in a folder that contains it, or one inside it), and no
chat can start working in that folder while an undo runs there; wait a
moment, or cancel the other chat, and try again.

**Approving a call.** In `ask` mode a command, a write, an edit or a publish
waits on a card that shows the whole call: every argument, the full path (and
whether it is absolute or outside the chat's folder), long text with its size,
beginning and end and a "show all" for the middle, runs of blanks as counted
markers, and hidden or look-alike characters as codes. "Allow once" runs this
call; "Allow this exact call for this turn" also lets the identical call run
again without asking until the turn ends (any other call still asks); "Deny"
refuses it. If you allow while part of the input is collapsed, the card says how
many characters you have not expanded.

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
`allow_outside_workspace = true` lets `write` and `edit` change any file
of the node's user outside the data directory, `~/.bashrc` and
`~/.gitconfig` included, which is the same as running a command outside
the sandbox at your next shell: leave it off unless the chats on that
node are ones you trust like your own shell.

A chat's folder is remembered as the directory it was when the chat was
made (its real path and its identity on disk). If that folder is later
replaced, by a link to somewhere else or by another directory of the same
name (deleted and cloned again, say), the chat refuses to work there
("this session's folder was replaced"): start a new chat in the folder.
Every turn also checks the folder against `workspace_roots` again, so
narrowing node.toml applies to existing chats.

```toml
max_approval_mode = "ask"            # ask_all | ask | auto
workspace_roots = ["/home/me/code"]  # session cwd must be inside one of these
denied_tools = []                    # e.g. ["bash"]; "delegate" turns sub-agents off
allow_outside_workspace = false
graphify = false                     # let agents query a graphify knowledge graph (see below)
env_pass = []                        # extra variables the agent's shell gets (see below)

[limits]
tool_stream_bytes = 1048576          # live output one tool call may stream (4096 .. 1 GiB)
max_steps = 50                       # model calls one turn may make (1 .. 1000)
bash_timeout_max_s = 3600            # longest a bash call may run, whatever it asks for (1 .. 3600)
max_subagents = 4                    # sub-agents one turn may start; the master may ask for fewer (0 .. 32)
max_delegation_depth = 1             # 1: the turn's agent may delegate, its sub-agents not; 0: off (0 .. 4)
background_per_session = 8           # background commands one chat may run at once; 0: off (0 .. 32)
background_per_node = 64             # background commands this node runs at once, all chats (0 .. 256)
background_output_kb = 256           # output kept in memory per background command (16 .. 4096)
callback_turns = 3                   # turns the agent may run in a row without you when commands end; 0: off (0 .. 20)
callback_max_steps = 20              # model calls one such turn may make, never above max_steps (1 .. 1000)

[sandbox]
mode = "required"                    # required | best-effort | off
network = "on"                       # on | off (off: no network for the shell, see "Network off")
read = []                            # more read-only folders, e.g. "~/.cargo"
write = []                           # more read-write folders, e.g. "~/.cache"
# bwrap = "/usr/bin/bwrap"           # which bubblewrap; default: the first root-owned one on PATH

[execution]
sandbox = "auto"                     # auto | strict | off: offer the sandbox mode (bubblewrap), see "Execution modes"
host = false                         # offer the host shell: commands as this OS user, no sandbox. Read "Execution modes" first
```

**Execution modes.** Each agentic chat runs its commands in one mode,
chosen when the chat is created and never changed (the chat's header
names it):

- **`sandbox`**: each shell command in bubblewrap, as "The shell sandbox"
  below describes (its own processes, its own file view, the guard,
  Landlock inside; background commands). Not every program a sandbox chat
  starts is contained: `graph_query` starts graphify, and the node runs
  its own git for checkpoints, outside bubblewrap under Landlock alone.
  Offered where bubblewrap works at the node's start (`[execution] sandbox
  = "auto"`, the default, with its gaps named; `"strict"` only where none
  is left; `"off"` never). The start probes up to three times, a second
  apart, and logs each failure; after that the start is the only moment
  that counts: restart the node once bubblewrap works.
- **`host`**: each command as the node's own OS user, **without
  bubblewrap**. Offered only with `[execution] host = true`. What still
  applies is the Landlock part of `[sandbox] mode` (which files a command
  can open; TCP with `network = "off"`), and nothing else: what a command
  starts can keep running after the call, the chat and the node; `/proc`
  shows every process; with the network off UDP and DNS still go out; no
  background commands; and below Linux 7.1 (Landlock ABI 9) a command can
  ask any service of this user over a socket (the session bus:
  `systemd-run --user`) to run anything with every right of the user, its
  keys and other projects included. Treat it as a shell on this machine
  that a web page the agent reads can steer: the approval card is then the
  only check, so a `host` chat always asks (it never runs in approval mode
  `auto`, whatever is asked or `max_approval_mode` allows), and its card
  says the command runs in the host shell. Run such a node as a dedicated
  OS user that holds nothing else.

A new chat is in the sandbox mode unless `host` is asked for by name: a
node that offers only `host` refuses a chat that does not name it. Who may
ask for which mode is their execution level (below); choosing `host` shows
that warning first. Every request for `host`, granted or refused by any
check, is written to the audit log (who, the workspace, the node, the
outcome; never chat content). A routine's chat always asks for `sandbox`. A connected node must have announced the mode it is sent: one
from before modes gets no agent chat, and the refusal says what to do on
the node; a node that is away is "not connected", as before. The chat's header names its mode and
the gaps its node announced at connect (network off without a namespace,
or not applied at all; the socket gap below Linux 7.1; no `/proc` of its
own); which modes a node offers is shown only to accounts that may run
agent chats.
A node that offers neither mode runs no agent chats and says so at start,
in `doctor` and when a chat is created ("this node offers no way to run
commands: install bubblewrap, or set `[execution] host = true`"); `node
run` refuses to start. **A data directory made by an earlier build does
not open with this one**: the schema changed in place, with no migration
before the first release (first the session table's mode, then the levels
below); `serve` refuses it at start as a schema mismatch. Remove it, or
start with a new one. **This
changed:** a node where bubblewrap did not
work used to run the shell with Landlock alone and only warn; it now
offers no `sandbox`, and without `host = true` nothing. A chat whose mode
the node no longer offers after a restart (bubblewrap gone, `host` turned
off) runs nothing and says why; it is never moved to another mode. Files
are shared between modes: what a `sandbox` chat writes in a folder (a
script, a `Makefile`, `.git/hooks`), a `host` chat in that folder may run
with every right of the node's user. See
[docs/execution-modes.md](docs/execution-modes.md).

**Execution levels.** Who may run which mode is one ordered level, `none` <
`container` < `sandbox` < `host`; each includes the ones before it.

- **An account's level** (its ceiling) is set by server admins in Settings →
  Users, never their own. Every new account starts at `none`, also one a
  server admin creates (the form does not ask: set it in the list). A server
  admin's is always `host` while they are one; what is set for them (the
  list shows it for server admins too) applies once they are not, and making
  a server admin a user says which level they will then have. A disabled
  account's is `none`.
- **A member's level in a team workspace** is set by its admins in Workspace
  admin → Members. Every new member starts at `none`. Nobody changes their
  own, nobody grants above their own level there or changes a member whose
  level is above it, and only an owner grants `host`. Admins of the workspace
  see every member's level, as set and as it counts; anyone else sees only
  their own. A role says what someone may administer, the level what they
  may run: changing one never changes the other. An owner's level there is always `host`, so their account's level
  decides. Making someone an owner therefore gives them `host` there,
  whatever the granting owner's own level (decided: a role carries its
  level). A Personal workspace has no level of its own: your account's
  level applies.
- **Your level** in a workspace is the lower of the two, and a chat's mode
  must be at most it: at create and on every later action that makes it
  work (a message, allowing an approval, restore, changing its agent,
  picking a parallel answer, saving a file as an artifact, a callback, a
  routine's run). The new-chat choice offers only modes within it. Reading a
  chat stays allowed whatever the level.
- `container` exists as a level, but no node offers containers yet, so an
  account or member at `container` runs no agent chats, and the composer
  says so.
- Granting `host`, as an account's level or a member's, shows the same
  warning as choosing it for a chat, before anything is saved; cancelling
  saves nothing.
- Lowering a level stops the chats above the new level at once (their
  running turn and background commands), and only those, also a turn whose
  message was on its way when the level changed.
- Removing a member, or deleting the workspace, stops every running chat
  they had in that workspace, whatever its mode. Those chats return to their
  owners' Personal workspaces, where each account's level applies; leaving a
  team therefore also unblocks a chat its level blocked (nodes are not yet
  tied to workspaces, below).
- Every change is one line in the audit log (who, whose, which workspace or
  account, from, to); none carries chat content. Two admins changing the
  same level at once, or an admin whose own role or level changed while
  they decided: refused and asked to reload.
- The API token, the machine owner's key, is `host` everywhere; on a
  member's chat it gets the member's answer.

**What a level in a team does not do yet:** a node is not tied to a
workspace (that comes with the node's list of workspaces, N.1), so a member
whose account's level is `sandbox` can run a `sandbox` chat on a team's node
from their Personal workspace even at `none` in that team. Today the
account's level is the boundary between people; a workspace's levels are
that team's own rule within it. The levels protect against members, not
against a stolen master, which can ask a node for anything up to the node's
own `[execution]` ceiling.

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
users' folders, not `/dev/tty`. A sub-agent works in its own folder in
the node's data directory (with its own git repository there, so `git
status`, `add`, `commit`, `diff` and `log` work), and may only read your
repository's object store for its history: nothing of your `.git` is
writable to it, and no other chat can open its folder. Programs
cannot gain privileges (`no_new_privs`): `sudo` and other setuid programs
fail inside the sandbox. From Linux 6.12 the shell also cannot signal
processes outside it, and from Linux 7.1 it cannot connect to Unix sockets
outside its folders (the session bus, the Docker socket, an ssh-agent); on
older kernels it **can** where the shell is not contained (below), see
"Limits" below.

**What a contained command sees.** Where the shell is contained in
bubblewrap (below), its file system is built from exactly that list and
nothing else: the system folders above (read-only; `/bin`, `/lib` and the
like are the same links as on your machine), the `[sandbox] read` and
`write` entries, its session folder and temp folder, a `/dev` of its own
(null, zero, full, random, urandom, a terminal device and `/dev/shm` of its
own; none of your machine's devices), `/proc` and an empty read-only
`/tmp`. The harness binary is not in it: the wrapper is started from the
node's own running image through a descriptor. Everything else
**does not exist** for it: `ls ~/.ssh` says "No such file or directory",
not "Permission denied"; another chat's folder, the data directory and the
target of a link that points out of the session folder are not there
either, and neither is any Unix socket outside those folders (the session
bus, the Docker socket). The folders above the session folder exist only
as the way down to it. This holds without Landlock too (an old kernel, or
`mode = "best-effort"` where Landlock is missing); Landlock stays inside as
a second layer with the same rules. A folder is mounted only if it is
still the one the node checked: one replaced by a link or another folder
since refuses the command. With bubblewrap 0.10 or newer the node's guard
opens each folder itself and bubblewrap mounts that very folder; with an
older one (Ubuntu 24.04 has 0.9) it mounts by path and the wrapper checks
the mounted folder before the command runs. `rizzy-harness doctor` lists
what is mounted on its "file view" line, and says which of the two ways.
In a `host` chat the shell is not contained: the command sees your whole
file system and Landlock alone decides what it can open, as the list above
says.

The node's own git, which takes the checkpoints and fills a sub-agent's
folder, runs in the same sandbox: it may read the chat's folder (and your
repository's object store), write only its own checkpoint store, and
nothing else. With `mode = "off"` it is not confined either; `doctor`
says which ("checkpoint git").

It needs Linux 6.2 or newer with Landlock enabled (`landlock` appears in
`/sys/kernel/security/lsm`; `network = "off"` needs 6.7). `rizzy-harness
doctor` shows whether it works and which Landlock version the kernel has.
With `mode = "required"` (the default) a node where it does not work refuses
agentic sessions with a message naming this setting; `node run` refuses to
start, `serve` starts for chats only. `"best-effort"` uses whatever the
kernel enforces and logs a warning; `"off"` runs the shell with every right
of the node's OS user and logs that on every start.

**Nothing a command starts outlives it, on Linux 7.1 or newer.** Each
`bash` call runs inside
[bubblewrap](https://github.com/containers/bubblewrap) (`bwrap`) in a
process namespace of its own, with the Landlock sandbox inside it
unchanged. When the command ends, is cancelled, times out, or the node
process dies (even by `kill -9`), every process it started ends too,
including ones that detached themselves (`setsid`, `nohup … &`, a daemon's
double fork). It also builds the command's file view ("What a contained
command sees" above). Each command has a small guard
process of its own between the node and bubblewrap that lives exactly as
long as the command: it ends everything and confirms it before a cancel
returns, and it ends the command when the node dies, even in the first
millisecond after a start.

**The condition.** A service of the node's user that a command can reach
over a Unix socket can start a process **outside** the containment, which
then outlives the command and runs without the sandbox: one call to the
session bus (`/run/user/<uid>/bus`, what `systemd-run --user` sends) is
enough, as is a container runtime's or a `tmux` server's socket. A
contained command cannot reach a socket outside its folders on any kernel:
it is not in its file view. On a kernel older than Linux 7.1 (Landlock ABI
below 9: Ubuntu 24.04, Debian 13 and most kernels in use today) it can
still reach a socket **inside** a folder it is granted (its session
folder, a `[sandbox] read` or `write` entry), and below Linux 6.12 (ABI 6)
with the network on, a service on an abstract socket (an X11 display,
some D-Bus and IDE sockets: not files, so no file view hides them;
`network = "off"` does). `doctor` then marks the line `warn` and names
what is left, and `[execution] sandbox = "strict"` does not offer the
sandbox mode there. Running the
node as its own OS user without a login session (no session bus, no
`docker` group, as "Limits" below advises) removes the usual services. On
every kernel, a `[sandbox] write` entry that holds another program's
socket (`/tmp`, where tmux and X11 keep theirs; `/run/user/<uid>`) is the
same way out: a read-write grant includes connecting to sockets in it. `doctor` warns for
`/tmp` and `/run/user/<uid>`; it does not look for sockets elsewhere. What
a command notices:

- `/proc` lists only its own processes; `ps` shows a handful, `$$` is 2.
  The node's and the rest of the machine's processes are not visible.
  Process 1 is bubblewrap's, and its command line (`/proc/1/cmdline`)
  lists the command's own grants and file view: its folder, its temp
  folder, the `[sandbox] read`/`write` entries, the system folders.
- A program that daemonizes to keep running after the call (a dev server
  started with `setsid`, a `--daemon` flag) stops when the call ends. Run
  it in the foreground within one call, with a timeout.
- Files owned by other users (`root` among them) show as owned by
  `nobody` in `ls -l`, and `id` lists the group `nobody`: the command runs
  in a user namespace. Your own files show as yours.
- The command has no controlling terminal (a new session).
- A command killed by a signal reports exit code 128 + the signal (137
  for SIGKILL) instead of "signal".
- On cancel or timeout the command still gets SIGTERM first and 2 s to
  exit, then everything left is killed, and the call returns only once
  all of it is gone. A process the kernel cannot end yet (stuck in a read
  from a stalled network or FUSE file system, which ignores even SIGKILL
  until the read returns) is not reported as cancelled: the result says
  it is still alive 2 s after the kill, and its guard keeps killing it
  until it ends. When the node process itself exits, restarts or
  re-executes itself, every running command is killed at once, without
  that grace: stop a node with its normal shutdown so running turns end
  first. If a command's guard is itself killed or stopped by a signal in
  its first millisecond (before bubblewrap protects itself: `kill`, or a
  service manager signalling every process of the node at once), the call
  that sees it ends what was left before it returns and says so; if the
  node dies in that same moment, the node ends it at its next start and
  says so in the log ("ended a command a previous run of this node left
  running"). One the kernel cannot end yet (stuck in I/O) is logged
  "STILL ALIVE": it has been killed and ends when the kernel releases it.
  A second server or node started on the same data directory is refused
  before it touches anything (see "One process per data directory").
- `/dev` is its own and `/tmp` is empty and read-only; use `TMPDIR` (the
  chat's temp folder, which compilers, `mktemp` and most tools already
  use). Your home folder is there only as far as a `[sandbox] read` or
  `write` entry or the session folder lies inside it, so a program that
  looks for `~/.gitconfig` or `~/.config` finds nothing (`git` then works
  with its defaults; give it `-c user.name=…` or a `read` entry for the
  file). The host name, `TMPDIR` and the environment are as without it.

**Network off.** `network = "off"` is about the node's shell (and
graphify and the node's own git); the agent's web tools (`web_search`,
`web_fetch`) are not affected: they have their own settings, `[web]` under
"Chats and web tools" above. With `network = "off"` a contained command
also runs in a network namespace of its own: it has only its own loopback
(`lo`), so no TCP, UDP or DNS from it reaches this machine's other
addresses, its loopback services or anywhere else, and it cannot reach
this machine's abstract Unix sockets (an X11 display, some D-Bus and IDE
sockets), which belong to the network namespace. Landlock still refuses
every TCP connect and bind inside it, its own loopback included, as
before: a test server on `127.0.0.1` cannot be started with the network
off; UDP to itself on `127.0.0.1` works. DNS lookups over the network fail
(`/etc/hosts` still answers). Not tested: a resolver that answers over a
named Unix socket (systemd-resolved's, nscd's) is outside the namespace,
which the file view leaves out (`/run` is not in it), so a contained
command cannot use one, on any kernel.

The node checks at start that a command really gets a namespace of its
own (its `/proc/self/ns/net` is not the node's). Where the probe with
`--unshare-net` fails and the one without it passes, the node tries once
more; if it fails again, `[execution] sandbox = "strict"` does not offer the
sandbox mode, and `"auto"` still contains every command (its own processes, the guard, Landlock inside)
without the namespace: TCP is stopped by Landlock, but UDP and DNS go out,
and abstract sockets are closed only from Linux 6.12 (Landlock ABI 6).
`doctor` and the start-up log mark the line `warn` ("network off WITH A
GAP") and say what failed: bubblewrap itself (its message), or the
probe's own check (the command's namespace was the node's or could not be
read). An `"auto"` node in that gap probes the namespace again beside a
shell command, at most once a minute and at most 2 s each (the command
that noticed does not wait for it); from the first probe that passes,
commands started from then on run in a namespace of their own, and the
node logs one line saying so. It only ever steps up: a command already
running stays as it started, and a node that has the namespace never goes
back (if bubblewrap fails later, the command fails). A node where
bubblewrap failed every probe at start does not offer the sandbox mode
and does not probe again: restart it once bubblewrap works. A chat in the
sandbox mode on a node in that gap says so in its header ("UDP and DNS
still go out"), as does every `host` chat with the network off. **Without
the containment** (a `host` chat) and for graphify and the node's own
git, `network = "off"` stops TCP only: **UDP and DNS still go out**, and
`doctor` and the log say so on the containment line.

It needs the `bubblewrap` package and unprivileged user namespaces. The
node takes the first `bwrap` on its `PATH` that its own user cannot replace
(the file and every folder above it owned by root and not writable by
group or others; a sticky folder such as `/nix/store` is fine), resolved
once at start and named in the log and in `doctor`; one an agent could
have planted (a project's `bin` ahead of `/usr/bin`) is skipped and named.
To use another, give its absolute path: `[sandbox] bwrap =
"/opt/bubblewrap/bin/bwrap"`, taken as given: it is your own choice, and a
wrapper there that gives the new network namespace a way out (user-mode
networking, say) defeats `network = "off"`, because the probe checks that
the namespace is new, not that it is empty. **Ubuntu 24.04 and later** deny those to
programs without an AppArmor profile: allow them with `sudo sysctl -w
kernel.apparmor_restrict_unprivileged_userns=0` (and a file in
`/etc/sysctl.d/` to keep it), or give `bwrap` an AppArmor profile that
permits `userns`. `rizzy-harness doctor` shows the result next to the
Landlock line ("shell containment"), and which execution modes the node
offers ("execution modes"). `[execution] sandbox` decides when the
sandbox mode is offered:

- `"auto"` (default): where the probe passes at start, with any gap
  above named in the log and in `doctor`, and in the chat's header as its
  node announced it at connect (the network, socket and `/proc` gaps). Where bubblewrap
  does not work, the sandbox mode is not offered (no command runs with
  Landlock alone in it): the node offers `host` if `[execution] host =
  true`, else nothing.
- `"strict"`: only where the guarantee above holds in full. It is
  therefore not offered where bubblewrap does not run, on kernels below
  Linux 7.1 (the condition above), in the form without a `/proc` of its
  own, with the network off and no namespace, and where a `[sandbox] write`
  entry holds `/tmp` or `/run/user/<uid>` (sockets elsewhere in a write
  entry are not looked for) or a folder on the node's `PATH`. On such a
  kernel set `"auto"`: the shell is contained, with that gap named.
- `"off"`: never (bubblewrap is not probed).

**Keep the harness binary out of every session's reach.** Once started,
the node runs its binary only through a descriptor of its own running
image, never through the binary's path: a contained command's guard and
wrapper, the wrapper of its own helpers (its checkpoint git, graphify) and
of a `host` chat's shell. Replacing, deleting or upgrading the file
at that path changes nothing for them, wherever the binary and the data
directory are and whatever its mode lets the node do (execute permission is
enough). **After a rebuild or an upgrade, a running node keeps running the
build it started with, silently, until you restart it**; nothing in the
log or in `doctor` says that the binary on disk is newer. If the node could
not get that descriptor, it says so on the `shell containment` line of its
log and of `doctor` ("the node holds no descriptor of its own binary"), it
then starts them by the path, and `[execution] sandbox = "strict"` does not
offer the sandbox mode.
If a workspace root or a `[sandbox] write` entry holds the binary (a node
run from its own build tree, with a session on that tree), a command that
keeps a name of the file the node runs (`ln`, before a rebuild) can change
its mode (`chmod 000`), after which no contained command and no checkpoint
of any session starts, the agent is told that the node's own binary could
not be executed, and the node cannot be restarted until you restore the
mode. `doctor` and the log warn ("a session can replace the harness
binary"), and `[execution] sandbox = "strict"` does not offer the sandbox
mode. Install the
binary in a folder outside every workspace root and write entry (for
example `/usr/local/bin`; `~/.cargo/bin` is inside a workspace root of
`$HOME`).

`mode = "required"` promises the Landlock file sandbox only, in both
modes; it does not require bubblewrap (whether the sandbox mode is offered
is `[execution] sandbox`), and where Landlock does not work it refuses
every agent chat, `host` too.
Where bubblewrap runs but cannot mount a fresh `/proc` (inside some
containers), it is used without one: every process still ends with the
command, but `/proc` then lists the outer processes, under numbers that
are not the command's own (`/proc/$$` is another process); the log and
`doctor` say "contained without a /proc of its own"; it is still the
sandbox mode. With `mode = "off"` there is no sandbox and no containment:
the sandbox mode is not offered (`[execution] sandbox = "strict"` is
refused at start), and a `host` chat's shell runs with every right of this
OS user. Nothing in a request, a chat or an agent definition can turn the
containment off: a chat's mode is chosen when it is created, from what
the node offers. Only the agent's shell is contained; graphify
and the node's own git and PDF reader keep the Landlock sandbox alone, in
a sandbox chat too.

What breaks by default, and the fix: toolchains and caches under your home
folder. A command that fails with `Permission denied` (or, contained, `No
such file or directory`) on a path under `~` needs that folder listed. Do
not list a folder on the node's own `PATH` under `write` (`~/.cargo` when
`~/.cargo/bin` is on it): a command could leave a program there that the
node or a later command runs by name; `doctor` warns about it. List
`~/.cargo/bin` under `read` and the caches under `write` instead. The usual entries:

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
The same holds for an entry that contains or equals a `workspace_roots`
folder (every chat's commands would reach every other chat's folder), one
inside another `write` entry (remove it: the outer entry already grants
it), and one inside a workspace root whose path goes through a symbolic
link below the root (list the real path instead). The node remembers
where each entry really leads when it starts, and grants it to a command
only while it still leads there: one replaced by a link (or below a
folder replaced by one) is left out, and one that did not exist at start
is granted once it exists at that same real path.
For the same reason a data directory under `/usr`, `/etc` or `/opt` is
refused. Graphify additionally needs its install (found automatically:
its venv and the Python it was made from).

Limits: Landlock does not cover UDP, so where the shell is not contained
(and for graphify and the node's git) `network = "off"` still allows DNS
and any UDP traffic ("Network off" above); Landlock does not hide that a
file exists where the shell is not contained (`stat` and `test -e` still work on paths outside, only opening is denied; contained, they are not there); `/etc` is
readable; the session folder's own `.git` is writable, so a command can
change that repository's hooks and config (the file tools refuse to).
Before Linux 7.1 a command that is not contained can connect to Unix
sockets such as `/run/user/<uid>/bus` or `/var/run/docker.sock`, and
through them start programs outside the sandbox (`systemd-run --user`,
`docker run -v /:/host`); a contained one only to those inside its granted
folders (and, below Linux 6.12 with the network on, to abstract ones): on
such kernels run the node as its own OS user, without membership in the
`docker` group and without a login session.

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
"stopped after N steps (`max_steps` in node.toml)". A `bash` call runs for
the `timeout_secs` it states, at least 5 s and at most `[limits]
bash_timeout_max_s` seconds (default 3600, 1 to 3600, which wins over the
5 s floor if set lower); **a call that states none stops after 60 s** (it was
600 s before background commands existed): a build or test run that takes
longer must say so, or run in the background. The timeout result says how.
The tool description the model sees states the ceiling. Neither an agent
definition, a prompt nor the master can raise either; a value outside its
range stops the node at start.

**Repeated calls.** Within one turn, a call the agent makes again with the same
arguments that gives the same result is answered with a warning from the
second time on. The fifth such call is not run (nor offered for approval):
its result reads "Not run: this exact call gave the same result several
times in this turn, with nothing changing in between", and if the agent asks
for it again in a later message (after it was sent that refusal) the turn
ends there with "stopped: the model asked again for a call the repeated-call
guard had refused" (other calls of that same message are not run either).
Something changing starts every count again: a call that succeeded with a
new result (a poll of a running command, a file that changed, so a wait
between polls is never refused), a change that did something new (an edit,
then the test run again), a sub-agent that completed, a background command
that ended. Calls in between that change nothing do not, and neither does a
call that failed in new words: a failing test runner that prints its run
time is never caught itself (its result is new every time), but the same
read, or the same change you denied, between its runs is still refused the
fifth time, so you are asked at most four times. The harness's own changing
tokens (the id of a kept full output, a stop's measured time) do not make a
result new. A new turn starts from zero. It is the same in a chat and in a
routine: a fifth identical call with an identical result is as useless in
either, and you see the refusal as that call's result. The thresholds are
fixed, not `node.toml` keys, and the guard cannot be turned off. The
warnings are for the agent only and are not shown in the chat.

**Background commands** are on by default (8 a chat, 64 a node); a node owner
turns them off with `background_per_session = 0` in `[limits]`. While they
are off the agent is not offered `bash_output` or `bash_stop` and `bash` does
not describe `background` (a call that asks for it anyway is refused with the
reason).

**What you see and how you stop one.** While a background command of the open
chat runs, a strip above the reply box lists each one: its id (`bg-1`), the
first line of its command (as plain text), the time left before its timeout
stops it, and **Stop**; with two or more, **Stop all**. A click shows
"stopping…" until the node has ended the command; the strip then shows
"ended: stopped" for a moment, the chat keeps the line "bg-1 ended (stopped by
user)", and the strip goes when nothing runs. A refused stop shows the reason
on its row (Stop can be pressed again). The strip is drawn from the chat's
log, so a reload or a second tab shows the same commands; only "stopping…" is
the tab's own. **The chat's Stop does not stop them**: it ends the turn, and
the strip stays, saying they keep running. When the chat's node is away, the
strip says so and offers no Stop: a stop cannot reach it, and the commands
end with that node or at their timeout (the chat shows the end when the node
is back). The time left is the node's deadline counted on your computer's
clock; if the two clocks differ it is off by that much, but it never shows
more than the command's own timeout or less than zero, and only the node's
end removes a row. Only the chat's owner can stop its commands (an admin
cannot, as they cannot read the chat). For scripts: `POST
/api/sessions/{id}/background/{bg_id}/stop` stops one, `POST
/api/sessions/{id}/background/stop` all. The node is always asked: 200 means
it took the stop (also for a command that had just ended), 409 carries its
refusal (an id it does not have), and the end itself arrives as the chat's
`background_ended` event. An admin who must end another person's runaway
command has the node: stopping it, or `background_per_session = 0` there.

`bash` with `background: true` and a `timeout_secs`
(required: a background call without one is refused before anything starts)
is for a dev server, a watcher or a long build. The call waits 3 s: a command
that ended by then is an ordinary result; one still running is kept by the
node as `bg-1`, `bg-2`, … and the call returns its id, when its timeout will
stop it and what it printed so far. It is the same contained command as any
other (its own bubblewrap, guard, file view and, with the network off,
network namespace), so later commands cannot see it in `ps` or signal it; the
agent reads it with `bash_output { id }` (no id lists the chat's commands)
and stops it with `bash_stop { id }`. Its output is kept in the node's memory
only, never on disk, with the node's secrets taken out as it arrives: while
it runs, the first and the last part, `background_output_kb` in all; once it
ended, its first 16 KiB and last 16 KiB, for at most 30 minutes, and only for
the newest `background_per_session` ended commands of a chat and the newest
`background_per_node` of the node (an older one's entry says its output is
no longer kept). All in all the node holds at most `background_per_node ×
(background_output_kb + 32 KiB)` of it. When a command ends (it exits, its
timeout, a stop) the chat log records it once, and the agent is told, with
every line of the output quoted: at its next step if a turn is running (in
that step's last tool result), or in front of your next message if not (your
text follows unchanged). If the chat is idle, the end resumes the agent:
see "Unattended turns" below. If the node dies before a running turn's next
step, an end that waited for that step is recorded as ended with the node.
Under `ask` the approval card says the command keeps running and for how
long, and it is asked for each start ("for this turn" is not offered). The
limits are node.toml's only: at most `background_per_session` per chat and
`background_per_node` per node (a start over either is refused, naming the
chat's running ids, never another chat's; nothing is stopped to make room;
one user's chats can take every slot of the node). Background commands are
refused in a sub-agent's session and in a `host` chat (no containment,
see "Execution modes").
**With `[sandbox] network = "off"` a server started in the background cannot
be reached** by a later command: each command has a network namespace of its
own; the start's result says so. A command stops at its timeout, with
`bash_stop`, when the chat is deleted, when its owner loses access to it
(the chat then starts none until its owner writes to it again), with the
strip's Stop, and when the node stops or dies; a turn's Stop leaves it
running. An undo in a chat is
refused while that chat's own background commands run; a background command
of another chat in the same folder is not checked and can write over what an
undo puts back.

**Unattended turns (callback turns).** When a background command exits or
reaches its timeout while its chat is idle, the server resumes the agent once,
without anyone writing: a *callback turn*. The chat shows it where your message
would be, as "Started by bg-3 ended (exit 1), not by you", and the agent reads
one message from the harness that says you have not written and quotes what
the command printed, every line behind `| `. Nothing else starts one: a
command you or the agent stopped, one stopped because the chat's owner lost
access or the node stopped, and one that died with the node never resume
anything. Several ends that come together are one turn.

*Bounds* (node.toml `[limits]`, nothing the server or the agent sends can
raise them): at most `callback_turns` (3) callback turns in a row; only your
own message gives them back. A callback turn may start background commands
of its own, and their ends spend the same three. Each callback turn makes at
most `callback_max_steps` (20) steps (never more than `max_steps`) and
cannot hand work to sub-agents. So the most the agent does without you after
one of your messages is `callback_turns × callback_max_steps` steps, 60 with
the defaults. A step is one answer of the model, which may carry several tool
calls, so 60 steps is not 60 commands; and on top of the steps come a call
retried after a passing provider error, the agent's fallback models when its
own fails (more calls in that step), and up to two summarising passes a turn
in a long chat (one model call per piece summarised). When
the budget is spent the chat says "the agent was not resumed: 3 turns in a
row without you (node limit)" and the end waits for your next message.
**`callback_turns = 0` turns callback turns off** on that node, silently: the
node does not offer them, the server asks for none, nothing is added to the
chat or to "Waiting for you", and ends reach the agent with your next message,
as before.

*Who and where.* The chat's owner must still be allowed to run it: their
account exists, is enabled and needs no password change; their level in the
chat's workspace still allows the chat's mode and they may use its node:
exactly what you could do yourself at that moment. (A team chat whose owner
left the workspace falls back to their Personal workspace, where they may go
on in it, so it is resumed there if agentic chats are theirs to run.) A
routine's chat, a
chat holding a webhook's data (untrusted) and a sub-agent's chat are never
resumed, and neither is a chat without an owner account (made with the API
token). An end recorded more than 60 minutes before the server could resume
the chat (the server was down, the node away; counted from the older of the
node's clock and the moment the server first saw it) starts nothing. A callback turn
runs in the chat's own approval mode (the node's `max_approval_mode` caps it):
in `ask` its calls wait on their cards like any turn's, with no time limit, and
your new message is refused "a turn is already running" until you answer or
press Stop; in `auto` it runs without a click, within the bounds above. It
gets no memory (no recall, no project facts, no `remember`) and no page
context, and nothing the command printed counts as yours: a link it printed is
not fetched on its say-so.

*Afterwards.* "Waiting for you" lists each chat a callback turn ran in until
you open it ("bg-3 ended: the agent was resumed without you"), and each chat
the agent was not resumed in, with the reason. The chat list marks a chat with
a background command running with a ring (a running turn's pulsing dot wins).

When a model's answer reaches its output limit while it is writing a tool
call, that call (and every other call in the same answer) is not run, even if
it looks complete; the model is told to split the change into smaller calls.
Three cut answers in a row end the turn with a message saying so.

**File limits.** The file tools only open regular files: a FIFO, a device
(`/dev/zero`), a socket or a directory is refused at once, also through a
symlink. `read` streams a file and stops after 32 MiB, saying where (use
bash `tail` or `sed -n` for later parts); `grep` skips files over 8 MiB and
says how many it skipped; `edit` and `outline` refuse files over 8 MiB.
`edit` and `write` never change a file in place: they write a temporary
file (`.rizzy-harness-write.<random>.tmp`, deleted on any error; one left
after a crash can be removed) in the same folder and rename it over the
original, so a crash, full disk or cancel leaves the old file whole. The
file keeps its permission bits, but a hard link to it is split (the other
name keeps the old content), extended attributes and ACLs are not carried
over, and a file in a folder the node cannot write to cannot be changed.
Cancelling a turn stops any tool: one that does not return by itself
within 2 seconds is abandoned. A slow undo or delete in one chat never
delays Stop or an approval answer, in that chat or any other.

### Token savings

Settings → Server → Token savings switches each method on or off; it is
stored as `[savings]` in `config.toml` and applies from the next message.

| Method | Default | Applies to | What it does |
|---|---|---|---|
| `prompt_cache` | on | chat + agentic | Marks cacheable the instructions and tools, the end of the history before this turn's message, the end of the history before the previous turn's message, and the newest message (at most 4, Anthropic's limit); Anthropic bills re-reads at 10% or less. Within a turn each step reads the one before. Across turns the next turn reads everything up to the end of the turn before this one, byte for byte, and writes only the newest turn once: this turn's own message carries the per-turn context, which the next turn sends without, so it and the steps after it are not read again (PLAN.md T43). A summary, an image no longer shown, or a change in which tool outputs are hidden makes one request write the history again. |
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
project's confirmed **memory**, and the open cards of its **board**. Memory
comes whole, newest first, up to about 4,000 characters, so "continue" in a
new chat still knows the project's decisions; past that, the newest that fit,
the older facts that match your message, and a line saying how many were left
out. That context is sent quoted under a line the harness writes, in
front of your message: a card title or an instruction another member wrote
can say anything, but it cannot pose as your message (a model may still be
swayed by what it reads), and a link that is only in it is not fetched on its
say-so. The model proposes project facts with `remember` (scope
`project`), in chats and in Agentic chats alike; a proposal shows only to the
owner of the chat it came from until they keep it on the project's Memory tab.
Every node offers `remember` (it runs nothing on the node: the server files
the call as a proposal); `denied_tools = ["remember"]` in a node's
`node.toml` turns it off there, and the server then files nothing from that
node. The model is told about `remember` only where the chat's node offers
it. Keeping your personal memory out of
a team workspace leaves its projects' memory in. Agentic chats started in a project
take its default node and folder. Temporary chats cannot join a project.
Design and open questions: [docs/projects.md](docs/projects.md).

## Repository instructions (AGENTS.md)

In an agentic chat the node reads the repository's own instruction files and
sends them to the model with every message, the way Codex does: the
`AGENTS.md` in each folder from the top of the git repository holding the
chat's folder down to the chat's folder itself (only that folder when it is in
no repository), and `CLAUDE.md` in a folder that has no `AGENTS.md` (an empty
or whitespace-only `AGENTS.md` counts as none). The top folder's file comes
first, the deepest last, each named by its path from the chat's folder
(`../../AGENTS.md`), so the agent's own `read` of that path opens the same
file. Together they are cut at 32 KiB, or at about a tenth of the model's
context window when that is smaller and known, with a note saying where the
text stops, which files were left out and why. They are read again at the
start of every message, so an edit (yours or the agent's) counts from the next
message on, and they are sent after the history like a project's
instructions, so compacting a long chat does not lose them. A sub-agent gets
them too (with its parent message's context), and so does the turn that resumes a chat after a background command
ends (a callback turn), the one nobody watches.

Not read: files in folders below the chat's folder, files above the
repository's top (or above the folder `node.toml` grants), `.claude/CLAUDE.md`,
`CLAUDE.local.md`, other names, and `@path` imports inside a file (the line is
sent as written; the file it names is not read).

They are text for the model and nothing more: they change no setting in
`node.toml`, no approval mode, tool or limit, and a link that is only in them is
not fetched on their say-so. Only regular files inside the folder the node
grants are read: a link is followed only to a file inside the repository (the
chat folder's own file only to one inside the chat folder), never out of it or
into the node's data directory, and a file that cannot be read is named with
the reason while the message goes on. A read that takes more than 5 seconds
(a hung network disk) is given up with a note; the node logs each refused,
cut or timed-out read (paths and reasons, never the text).

**Cost:** the files are sent fresh with every message, after the part of the
request the provider caches: about 1k tokens a message for a typical
`AGENTS.md`, up to the cap (about 8k tokens), not reused from one message to
the next (the steps within one message do reuse it). Moving them to a cached
place is for later (with X.1).

## Rich messages

Replies render as Markdown: headings, lists, GFM tables (click a header to
sort, **Copy CSV**), task lists, footnotes (the only in-page links a reply
can have: a `#…` link to anything else shows as plain text), callouts (`> [!NOTE]`, `[!TIP]`,
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
  The server accepts `localhost:7777`, and a browser on the host tries
  `[::1]:7777` first for it, which Docker does not hold for you: publish it
  too (`-p '[::1]:7777:7777'`, needs IPv6 in Docker), or open only
  `http://127.0.0.1:7777` and never `localhost`, or another program of yours
  that binds `[::1]:7777` gets that page and your login.
- For remote nodes add `--node-listen <host VPN address>:7443` to the command.
- `docker stop` waits 10 s by default; a server closing its chat turns
  may need up to 80 s and then HTTP's 30 s grace, so give it
  `docker stop -t 150` or compose `stop_grace_period: 150s`
  ([Stopping a node or the server](#stopping-a-node-or-the-server)).
- The image's binary is built on Debian 13: copied to a node, it runs on Debian 13
  and Ubuntu 26.04, not on older glibc.
- Building yourself and hitting Docker Hub rate limits:
  `docker build --build-arg REGISTRY=mirror.gcr.io/library .`
- Published images are linux/amd64 only.
- The image has no bubblewrap, so its embedded node offers no sandbox
  mode: **agent chats there need `[execution] host = true`** in the data
  directory's `node.toml` (Landlock alone counts as the host shell; read
  "Execution modes" first: the container is then the only wall around the
  agent's commands), else the node runs none and `doctor` says so. Inside
  a default rootless container bubblewrap cannot mount a fresh `/proc`
  ("Can't mount proc"), but it runs without one
  ([docs/container-experiment.md](docs/container-experiment.md) §7), so an
  image that adds bubblewrap would get the sandbox mode in its form
  without a `/proc` of its own (not tested with the harness's image).

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

Run each node from its own data directory, also a node on the master's own
machine (a second process on one data directory is refused). If two processes use one node's key
(a copied data directory, a node restored from an old backup) and write the same
chat differently, the server stops taking that chat's events, logs an error
naming the chat and the node, and refuses new messages to it ("diverged").
Delete the chat to clear it; the node's copy goes with it.

A node and its server must speak the same link protocol. A node that does
not is refused when it connects (or joins: the join token is then not
spent), with both versions and which side to upgrade in its log ("the
master refused this node: … the master is older: upgrade the master
first"), and `node run` exits with that message instead of trying again.
A node refused for its key (revoked, or never joined) exits the same way.
Both end with exit status **77**, so a service manager can leave the node
down (the unit below does); a server that is down or restarting is retried
every few seconds up to 30 s apart. What the master sends in a refusal is
shown as one line of printable ASCII of at most 300 characters, and the
node accepts from its master only a node id in the master's own shape
(`a-z`, `0-9`, `-`, at most 64): anything else is refused, also with 77.
Upgrade the server first, then its nodes.

**A node on a slow link.** A node's chats share one link to the server.
When they produce output faster than the link carries (several chats
streaming command output over a slow uplink), the node holds their output
back until the link has room: a chat's output then reaches you late, and
a command the agent runs waits with it, so the agent cannot run far ahead
of what you see (a model's answer is not held back while it is written,
only its delivery; it is at most one answer long). Stop
and approvals are not held back: a Stop ends the command at once, a
command's timeout still applies, and the node answers Stop and an approval
ahead of the waiting output (about a second on a full 256 KiB/s uplink).
The stopped chat shows its turn as ended only once the output it had
already produced has arrived (a few seconds on such a link).

### Stopping a node or the server

`serve` and `node run` stop the same way on SIGTERM (`systemctl stop`,
`kill`, `docker stop`) and on SIGINT (Ctrl-C):

- New work is refused at once with "this node is stopping": a new message
  or a new chat on it gets that answer (HTTP 503) and keeps its attached
  files, so it can be sent again once the node is back. On a stopping
  server, **Run now** and webhook calls get 503 too and record no run, and
  no scheduled run starts; what falls due runs at the next start (by its
  catch-up rule). Answers to approvals and cancels are still taken.
- Every running turn is cancelled as with the chat's Stop button: its
  command gets SIGTERM, and 2 s later its guard kills it and everything it
  started; a question waiting for your approval is denied; the turn takes
  its after-turn checkpoint (its undo point) and ends in the log as
  **stopped** ("turn stopped" in the chat). A turn you had cancelled just
  before stays "cancelled", and one that had already finished stays
  finished. The server keeps serving the page meanwhile, so open chats
  show it; a node keeps its link to the server, so the server sees it too.
- A routine's run that a stop cut (its turn running, or not yet started)
  ends **interrupted** with the reason and does not count toward pausing
  the routine. **It is not run again by itself**, and a webhook's event is
  not replayed: the routine's next occurrence runs as scheduled, and
  **Run now** runs it at once. A run whose node crashed during its turn
  still counts.
- When the last turn has closed the process exits with status 0. It waits
  at most 80 s (the cancelled command's grace, the checkpoint's own 60 s
  limit, and a margin); a turn still open then, or one whose end could not
  be written (a full disk), is named, closed as "interrupted" at the next
  start, and the process exits with status 1.
- A **second** signal while it waits exits at once with status 1. Each
  command's guard ends its command when the node process is gone, and the
  next start closes the open turns.

Background commands are not waited for: the stop ends each one as above
(SIGTERM, then its guard's kill) and the chat records it as ended because
the node stopped. After a crash or `kill -9` they die with the node (their
guards end them within half a second), and at the next start each is
recorded as ended with the node; no turn starts for it. If the log cannot
take an end during a stop (a full disk), the stop does not wait for it: the
process exits with status 1 naming the chat, and the next start records the
end.

What can outlive a stop: a command stuck in a read the kernel cannot
interrupt (a network folder whose server went away) keeps its guard until
the read returns, and a `host` chat's command that detached from its shell
is not ended at all. A tool of the node's own
stuck that way does not hold the process up: 2 s after its work is done
the process calls its exit. But the kernel cannot let it go while that
thread waits in the read: until the read returns (or the mount is forced
off) the process is a zombie that no signal ends, its data directory stays
locked, a second start is refused with a message saying the holder has
exited and waits for a file system that does not answer (`doctor` says the
same), and under systemd the stop times out and the unit is marked failed.

The data directory stays locked until the process is gone, so a second
start on it during a stop is refused with the usual "is in use by"
message. Stopping the server does not stop its remote nodes: their turns
keep running and they connect again when it is back.

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
# The stop signal (SIGTERM) goes to the node alone: it cancels its turns and
# its commands' guards end them. With the default (control-group) every
# guard would get it too and kill its command before the turn could close.
KillMode=mixed
# The node closes its turns within 80 s, then exits; SIGKILL after this.
TimeoutStopSec=120
# 77: the master refused this node (revoked, unknown, another protocol):
# starting it again changes nothing until you act.
RestartPreventExitStatus=77
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

The built-in agents, grouped by category in the picker:

| Agent (`@handle`, aliases) | Category | Runs as | Can |
|---|---|---|---|
| Assistant (`@general`, `@ai`), the chats' default | General | agent | web search and fetch; your facts; project memory |
| Smol (`@smol`) | General | agent, sub-agent | nothing but answer: no tools, no memory |
| Researcher (`@researcher`, `@research`) | General | agent, sub-agent | web search and fetch; project memory |
| Software Developer (`@coder`, `@dev`), the Agentic default | IT | agent, sub-agent | read, change and search files, run commands, publish, web; delegates to Scout, Researcher and Reviewer; project memory |
| Senior Teamlead (`@teamlead`) | IT | agent | everything the Software Developer has, plus your facts; delegates to Software Developer, Researcher, Writer, Scout and Reviewer |
| Writer (`@writer`) | General | agent, sub-agent | read, change and search files, publish, web; project memory |
| Scout (`@scout`) | IT | sub-agent | read and search files only |
| Reviewer (`@reviewer`) | IT | agent, sub-agent | read and search files; project memory |

**Researcher in Agentic sessions.** Web tools exist only in chats, not on
nodes yet (nodes have `remember`, which alone does none of a task's work). In an Agentic session, Researcher is therefore not offered
to the Senior Teamlead or Software Developer as a sub-agent, and a delegation
to it by name is refused with the reason, so the lead never believes research
was done that was not. The rule is general: any sub-agent that lists tools
of which none is available on the node is treated the same way; one that lists
no tools on purpose (Smol) or has some of them is unaffected.

**Default agents per mode.** A chat gets the Assistant unless you pick
another; an Agentic chat gets the Software Developer. A workspace overrides
each (`default_agent` for chats, `default_agentic_agent` for Agentic), and
your own preferences override the workspace's in the picker. An agent you
name for the chat always wins. The Agentic default must be able to change
files or run commands: the Assistant, Smol and Researcher are refused there,
saying why; so is an agent that runs only as a sub-agent or does not exist,
naming the setting. An agent with file tools may be the chats' default: in a
chat it simply has no file or command tools (a call to one gets "unknown
tool").

"Your facts" are the memories in Settings → Account; an agent without them
neither recalls nor proposes them. Scout and Reviewer are read-only by their
tool list: they are offered no tool that writes or runs anything, and the loop
refuses any other call. That is not a sandbox: there is no read-only shell
yet, so Reviewer cannot list a change itself and reviews what it is given or
reads.

A message that starts with `@handle` (an id or an alias: `@dev fix the
build`) goes to that agent for one turn; the chat's agent stays. An agent
that runs only as a sub-agent (Scout) cannot answer a chat: `@scout ...`,
choosing it for a chat, or making it a default is refused with a message
saying so. An agent that does not exist is refused wherever it is named, and
the error names it. A chat whose agent no longer exists (such as the removed
Ultracode) still opens, but its next message is refused ("this chat's agent
"ultracode" no longer exists: choose another agent for this chat"); pick
another agent in the chat and it goes on. A default agent setting that names
a missing agent is refused the same way when a chat needs it, naming the
setting to fix. A routine naming a missing agent is paused and says why. A
message that starts with `@something` that is no agent is ordinary text.

Delegating agents split big jobs into parallel sub-tasks (at most 4 per turn
and 200k tokens by default; set `delegates`, `max_subagents` and
`max_subagent_tokens` in an agent file). Delegation is one level deep: a
sub-agent never delegates further.

What a sub-agent gets: the task its parent wrote, the repository's instruction
files, and the context the parent's message had (your settings, memory
recalled for that message and, in a project chat, the project's instructions,
facts and board), quoted the same way, whatever its own memory switches say:
it is the same person's chat. It never gets the parent chat's history or its
images. It works in its own chat, which the `delegate` card links to.

A sub-agent ends with `finish`, a tool only sub-agents have: a status (`done`,
`partial` or `failed`), a summary, and optional details of what is left open.
Calling it ends the sub-agent's turn at once; it is not taken beside other
calls in the same message, so the report is written after their results. Only
what `finish` carries, and the text of the message that calls it (which leads
the report), reaches the parent. On its last allowed step, and after the
repeated-call guard stops it, a sub-agent is told so and offered `finish` alone,
so it can still report. One that never calls it reports its last answer, `done`
if its turn completed and `failed` if not. A turn that did not complete is
`failed` even after `finish`, and the status line then says how it ended (the
step cap, the guard, a provider error, a cancel).

What the parent sees: the link to the sub-agent's chat, one line of the
harness's (`Sub-agent @scout finished: partial. Its own report follows …; the
harness did not check it.`), the report with every line behind `| ` (so
nothing the sub-agent writes can pass as the harness's), cut to 24,000
characters when longer (the first 18,000 and the last 6,000, with a note
between), then what became of its changes and the pages it cited. The `delegate` card shows the status next to the link.

**Agents** in the sidebar lists every agent. Server admins can make agents there
(name, handle, summary, instructions and feature switches: memory, project
memory, web search, files, commands, sub-agents, and whether it is offered as an agent,
a sub-agent or both), change them, delete them, and change any agent's look.
Built-in agents and agent files keep their name, handle, instructions and
features: only their summary and **additional instructions** (added after
their own) change there, and **Duplicate** makes an editable copy. **Write
from instructions** asks the default model for a summary. An agent file can
set `memory: false` (no recall, no `remember`), `project_memory: false` (no
project facts in project chats), `available_as: [agent]` or
`[subagent]` (default both), `aliases: [h, ...]` (more handles) and
`category: IT` (the picker's group, default General). Ids and aliases are one
namespace: making or changing an agent with a handle another agent has is
refused, naming it ("`ai` is already reserved for the Assistant agent
(general)"), as is an alias listed twice or equal to the agent's own id. An
existing file or agent whose id is now a built-in's alias keeps its id and the
built-in loses that alias (logged at startup, naming both agents). Deleting an agent leaves its chats
readable; each needs another agent before its next message. Design notes: [docs/agent-pages.md](docs/agent-pages.md).

## Development

The Rust checks CI runs (`cargo deny` needs `cargo install cargo-deny`):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

The web checks CI runs, with Node 22 and the pnpm that `web/package.json`
pins (`packageManager`, 12.17.0 today). Corepack, which ships with Node,
provides that pnpm: run `corepack enable` once (or call `corepack pnpm …`).

```bash
cd web
pnpm install --frozen-lockfile
pnpm typecheck && pnpm test && pnpm build && pnpm size
```

`pnpm size` fails when the first load is over its budget in
`web/scripts/size.mjs` (a hard limit), and records what each change adds to
the total. The Rust build embeds `web/dist`; without it, `cargo build` still
works and the server shows "UI not built".

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

End-to-end UI flows against the real binary: the scripts in `web/e2e/` drive
Chromium through Playwright. They are Node scripts; the Python package of
Playwright carries the same Node driver, so `uv` sets everything up in
`.venv` without a global Node package. Once, from the repository root:

```bash
uv venv .venv
uv pip install --python .venv/bin/python playwright
PLAYWRIGHT_BROWSERS_PATH=$PWD/.venv/ms-playwright .venv/bin/playwright install chromium
mkdir -p .venv/node_modules
ln -s "$(echo $PWD/.venv/lib/python*/site-packages/playwright/driver/package)" .venv/node_modules/playwright
```

Then build the binary (`cargo build -p harness`) and run every script, or the
ones named, one at a time (they use fixed ports):

```bash
web/e2e/run.sh
web/e2e/run.sh phase3 login-key
```

`run.sh` takes the binary from `BIN` and the setup from `E2E_VENV` (defaults
`target/debug/rizzy-harness` and `.venv`). To run one script by hand, set the
two variables it sets:

```bash
NODE_PATH=$PWD/.venv/node_modules PLAYWRIGHT_BROWSERS_PATH=$PWD/.venv/ms-playwright node web/e2e/phase3.cjs target/debug/rizzy-harness
```

The commands below assume those two variables are exported.

The login key in the browser (cookie without the key, another port of the
host, the socket's first message, log out):

```bash
node web/e2e/login-key.cjs target/debug/rizzy-harness
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

A chat's background command in the strip, the turn's Stop leaving it, the
strip's Stop ending it (needs bubblewrap; port 7786):

```bash
node web/e2e/background.cjs target/debug/rizzy-harness
```

A background command that ends while its chat is idle resuming the agent,
three times, then the chat saying it was not resumed (needs bubblewrap; port
7893):

```bash
node web/e2e/callbacks.cjs target/debug/rizzy-harness
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

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

**Writing while the agent works.** The reply box stays open while a turn
runs: Enter (or Send, next to Stop) sends the message at once, and a line under
the box says what happens to it ("Read after the current step. Stop
interrupts."). The node stores the message before it says yes, so a message
that was taken is never lost, and the chat shows it where you sent it, marked
"sent while working" until the agent has read it. The agent reads it before
its next model request, right after the results of the tools it was running,
as your message, marked for it as written while it worked; a running command
is never interrupted for it. If the agent had already given its last answer,
or the turn ended with an error (a step limit, the repeated-call guard, a
provider error: your message is often the fix), your message starts the next
turn instead. A turn the server started by itself when a background command
ended reads none of your messages; your own turn starts with them when it
ends. Stop works as before and never restarts anything by itself: a message
the stopped turn did not read (or one a node restart cut off) is marked "not
read yet: goes with your next message" and goes to the agent with your next
message, ahead of it. A message the agent was given just before a Stop or a
crash counts as read: it is in the next turn's history like any other. At
most 8 messages wait at once; the ninth is refused ("a turn is already
running…") until the agent reads them. An answer to a question card still
waits for the turn to end; so does any message while parallel answers run,
and a sub-agent's chat takes none while it works. A message keeps the running
turn's agent and settings: one sent to another agent (the agent picker, or an
`@mention` of another agent) is refused with the reason, so pick the agent for
your next message instead.

**Retry.** When the chat's last turn ended without an answer — "turn error"
(the provider gave up after its own retries, an unknown model, a context the
last resort could not shorten), "turn interrupted" (the node restarted
mid-turn) or "turn stopped" (the node was stopped) — its line has **Retry**.
It runs your same message again, without your typing it and without a second
copy in the chat: the text, the attached files, the images and the `@` files
as the message recorded them (a mentioned file is not read again; a `/skill`
at its start loads again), with the chat's current agent, model, approval
mode and settings and the reasoning effort the attempt ran at. The failed
attempt stays above, marked as failed, and the agent is sent it too: a call
that shows a result ran (a command ran, a file is written), a call with no
result recorded may or may not have run, a denied call did not run; the
harness tells the agent so and asks it to check the current state before
repeating a command or an edit, then carry on from where the attempt
stopped. Your message reaches the agent in every request of the retry: no
summary, automatic or `/compact`, replaces the turn that holds it while it
may still be retried (the attempt's old tool results may be shortened to a
note instead). A message with images is retried only on a model that sees
images. When the
retry runs on another model than the attempt did, its line says so. The retry
is a turn of its own: Undo on it undoes only what the retry changed, and the
failed attempt's changes keep their own Undo. Retry is not offered after you
pressed Stop (write what to do instead, or send the message again), after a
turn that gave an answer (a step limit's account included), once a message or
a turn followed, once the failed turn's changes were undone, while a turn or a
summary runs, in a sub-agent's chat, or on a routine's own run (the routine
retries a run its node missed by its own rules; run it again otherwise). It is
never automatic: a failing provider is already retried inside the turn, and
Retry is your decision after that gave up. It does not rewind the
conversation or let you edit the message first.

When a turn completes or fails in a chat you are not looking at (another chat
is open, or the tab is hidden), the chat gets a dot in **Recent** until you
open it, and with **Desktop notifications** on (Settings → Account →
Preferences) the browser says "A chat finished: number N in your recent
chats", never the chat's title. A sub-agent's chat, a turn the harness
started after a background command ended (it has its own "resumed" row on
Home) and an answer that asks you something (it waits for you on Home) do
not get one; neither does a turn you stopped. The chat header shows the
context of the last request as a share of the model's window and where older
turns will be summarised ("62% of 128k · summary at 80%"). Without a known
window it says **window unknown**: hover it for how to set `context` on the
model. A sub-agent's card in the conversation marks where it was started
and links to its chat; its row in the side panel shows what it is doing (its
latest tool call and how many it made, or that it waits for an approval) and
its tokens once it ended. Only chats in the open workspace notify, and a
second tab may notify while another tab shows the chat.

A chat is titled from its first message. The pencil next to the title in the
chat's header renames it (Enter saves, Escape cancels; at most 200
characters, control and hidden characters dropped); only you can rename your
chats, and the new title is what the list and search show. The download
button next to it saves the chat as a Markdown file: your messages, the
answers, and each tool call as one line (tick **Include tool outputs** for
their full output). The file is made in your browser from what the chat
shows; nothing is fetched for it. It holds the model's text as the model
wrote it, so a Markdown viewer that renders raw HTML or loads remote images
will do so for that text.

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
  defaults it gives, and whether your facts (your memories) are used in its
  chats: always in Personal; in a team workspace you are asked when you join
  it (its creator too). Until you answer, the answer is no; you can change it
  here at any time (see **Memory.** further down); and the workspace's
  memory (Memory tab), which every member reads.
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
- **Agentic routines** run in the approval mode you pick when you create or
  edit them, Auto by default (the owner's decision of 2026-11-14). A scheduled
  routine in Auto runs commands with nobody present, bounded by the node's
  `max_approval_mode` and, in a sandbox chat, by the sandbox; each run may
  take up to the node's `max_steps` (1,000 by default) unattended steps. A run that uses a
  webhook's data always asks, whatever the routine's mode: whoever calls the
  webhook writes into its prompt. When a run asks, a command, an edit or a
  publish waits for your click in "Waiting for you" and is cancelled after 24
  hours; read-only work finishes on its own. A routine in **Plan** changes
  nothing (a report or a review), webhook data or not.
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

### What a chat costs (an estimate)

The chat header shows the tokens a chat used and, once the server has prices,
an estimate of what it cost: `≈ $0.42 (last turn $0.03)`. The chat's figure
already includes its sub-agents; each sub-agent row in the side panel shows
its own share, a breakdown rather than something to add. There is no built-in price table: prices change
and only you know the deal you have, so a server admin sets them per model in
Settings → Server → AI providers → Edit → **Prices (optional)**, per million
tokens: input, cached input (read from the provider's prompt cache), cache
write, and output. Every field may stay empty. They are stored with the model
in `config.toml`, in one currency for the whole server (Settings → Server → AI
providers, `[pricing] currency`, an ISO code; changing it converts nothing):

```toml
[pricing]
currency = "EUR"                     # default USD

[[providers]]
# ...
models = [
  { id = "big-model", price = { input = 3.0, cached_input = 0.3, cache_write = 3.75, output = 15.0 } },
  { id = "small-model", price = { input = 0.25, output = 1.25 } },
]
```

A price must be a number from 0 to 10,000; a bad one is refused (a save
fails with the reason; the server does not start with one in the file). Only
server admins set prices or the currency; everyone signed in reads them with
the model list, and sees the estimate of their own chats only.

What is counted: every model call the chat made, each under the model that
answered it: the turn's main calls, a retry, a fallback model, the summaries
written between and inside turns, sub-agents (on their own model, the small
one included), parallel answers and hand-off notes. Input read from or written
to the cache is priced at its own price, the rest of the input at the input
price; reasoning tokens count as output, as providers bill them. A model is
priced only if every kind of token it used has a price: if the small model has
no cached-input price and a chat read from its cache, its tokens are left out
and the header says `≈ $0.42 + unpriced` instead of a figure that is too low.
A sub-agent that fails or is cancelled still counts what it spent until then.
At most 16 models per chat (and per turn) are tracked separately; tokens of
any further model, and chats from before this version, have no per-model
record and show as unpriced.

What it is not: the bill. It uses the prices as they are now, not as they
were when the chat ran; it knows nothing of discounts, free tiers, batch
prices, long-context surcharges or a 1-hour cache write's higher rate; it
prices cache writes only as the provider reports them (Anthropic's
`cache_creation_input_tokens`, an OpenAI-compatible router's
`prompt_tokens_details.cache_write_tokens` such as OpenRouter's): a provider
that reports none has its cache writes priced as plain input; and nothing
limits or stops spending. The provider's own console has the real
figure.

An agent has one model; one model may serve several agents. A model call
that fails before any output for a passing reason (rate limit 429, server
error 500/502/503/504, Anthropic's overloaded 529, no connection, a stream
that breaks before its first word) is retried up to 3 times, waiting 2, 4,
then 8 seconds (with jitter), or what the provider's `Retry-After` asks; a
cancel ends the wait at once. A `Retry-After` over 60 s (a quota window, say)
is not retried: the agent's next fallback model answers at once, or without
one the turn ends with a message naming the wait. A refused key, a missing
permission, an exhausted quota, a bad request or an error the provider codes
`model_not_found` is not retried, and nothing is retried once part of the
answer is on screen. The chat shows the turn as still running while it
waits, with a line under it naming the model, why (rate limit, provider
overloaded, provider error, connection failed, or no answer yet when the
first-byte timeout ran out) and a countdown to the next attempt ("attempt 2
of 3"). When the provider does not know the model (it answers 404, or codes
the error `model_not_found` at any status, which also sends the turn to the
agent's fallbacks), the turn's error says so in the harness's words, names
the model and where it is set (the chat's model menu, an agent's or the
default model in Settings, the provider's model list under Settings → Server
→ AI providers). When it keeps failing with a server error (500 to 599), the
error says the provider failed to answer and after how many tries, with no
advice about settings. Either way the provider's own message is quoted
below, at most three lines and 300 characters. A router that answers 503
with only a generic error type for a model it has no connection for is
treated as a failing server: nothing but free text tells that apart from a
busy one, and the harness does not guess from free text. When a turn's
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

A provider without a key (none configured, or an empty one) is sent no key
at all: no `Authorization` header and no `x-api-key`, for the chat, the
model list, the provider test, memory embeddings and dictation alike. A
local router that refuses unknown keys therefore works without one (before,
the harness sent `Authorization: Bearer no-key`, which such a router
refuses with 401). Settings shows such a provider's key as empty ("counts as
no key") rather than as stored.

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

At the start of each turn, older turns are summarised when the history is over
the model's context window less a fifth of it (at least 16k, for the reply and
tool results; the window from the provider's model listing or your `context` on
a model), measured with the token count the provider reported for the previous
turn; the two newest turns are kept as they are. Set `summarize_after_tokens` to
use a fixed number instead.

Inside a turn, every request is checked against the same line before it is
sent (when the model's window is known), so a long coding turn goes on instead
of being cut off. Past the line, the oldest tool results are first replaced, in
what the model is sent, by a one-line note saying what was there (the tool, its
file or command, how many lines) and that it can be called again; the newest
steps are never touched (except when one step alone is over the line: then all
but its newest result), nor the task list or a loaded skill; the agent is told
up front to write down what it will need and to read a cleared result again
before relying on it. Each such cut
frees about half the line, so it happens rarely: about every 36 steps of a long
turn of file reads on a 128k model, every 5 or 6 on a 32k one (measured with the
default token-saving settings; with a known window the "hide old tool outputs"
setting does not act, clearing replaces it). When that is not enough (the
calls themselves are what is large), earlier turns are summarised and then the
turn's own earlier steps, at a step boundary, into a summary sent as a message of
its own after your message; your message, the context sent with it, anything you
wrote during the turn and the task list stay as they were. The
chat shows a quiet line where this happened ("Context shortened for the model"),
with the summary collapsed under it; every message and tool result stays shown in
full, and the agent can read any cleared file again. The same applies to the two
newest turns at the start of a turn when they alone are over the line. With an
unknown window none of this happens inside a turn. If a provider still refuses a
request as too long, the turn summarises older turns, hides all but the newest
large tool output, and tries once more; a second refusal ends the turn with its
account of where the work stands (or an error, for a turn nobody started). See
`docs/turn-compaction.md` for the numbers and what the other harnesses do.

A summary is only made by a model whose window holds what it reads: a long
history is summarised in pieces, and a single turn larger than the window is read
with its tool results cut shorter (down to 250 characters each), and left
unsummarised only if it does not fit even so. A summary
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
a long chat is summarised. The summary records what was said, done and found
as it was stated then: it does not check, correct or work out results again
(a live test saw a summary "correct" an answer from a shortened tool result
and the agent then report the invented value), and where the conversation
itself called something wrong it says who did. A tool result longer than
2,000 characters reaches the summarising model shortened, with a line saying
it was cut and that the agent saw all of it. Chat-profile chats keep a
shorter general summary (at most 300 words). The latest summary is shown,
collapsed, where it was made in the chat.

**Summarise now: `/compact`.** Type `/compact` in a chat's message box, or
`/compact keep the database migration details` with words on what to keep
in focus, and the chat's history is summarised now instead of when it fills
up: everything before your last message and its answer, which stay word for
word (an automatic summary keeps the last two turns), with the task list and
standing context given back as after any summary. Your focus words, on the
same line as `/compact`, go to the summarising model as yours, quoted, for
emphasis only (at most 2,000 characters; a longer text is cut and says so;
more lines after the command are refused with a hint). `/compact` is the
built-in command: a skill named `compact` is still offered to the agent,
but `/compact` never loads it. The chat shows "Summarising
the chat for the model…" until the summary appears ("summarised for the
model at your request", with your focus and the summary under it), and the
context meter drops to the new size. It works between turns only (a running
turn shortens its own context when it must; Stop it first if you want to
summarise now), one at a time, and no message is sent while it runs; Stop
shows while it runs and cancels it. A message you sent during the last turn
that no turn has read yet is read first: `/compact` waits until then. A chat
too short to summarise, a model error, a summary that would not be smaller
or Stop is said in plain words, and nothing is lost: the chat keeps showing
every message, only what the model is sent gets shorter. After two failed
summaries in a row, send a message before trying again. The summary's cost
counts in the chat's usage. Only the chat's owner can do it; no agent,
routine or tool can.

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

**Memory.** The assistant keeps short facts for later chats, in three
places:

- **Your facts**: about you (your preferences, people, how you work). Only
  you see them (Settings → Account → Memory). They follow you into a team
  workspace only after you answer its question yes (see below).
- **The workspace's memory**: one per workspace (the owner, 2026-11-14:
  "Workspace memory available to all users in it"). In a team, it is how the
  workspace works (its conventions, systems, who does what): every member's
  chats there are given it, and every member reads it under Settings →
  Workspace → Memory, whatever they answered about their own facts. In your
  Personal workspace, where you are the only member, it holds private details
  about you (health, money, identity documents, family), given to your chats
  there and nowhere else.
- **The project's memory**, for the chats of a project (docs/projects.md).

The assistant says which place it means when it saves (`about`: you, the
workspace, the project); when it does not say, the fact goes to your facts.
A fact it marks as a private detail always stays with you, whatever place it
named: it is never put in a team's or a project's memory by the assistant
(in a team chat it is offered for your Personal workspace, and waits for
your Keep). A project fact in a chat without a project is refused, not
saved elsewhere.

- **In your Personal workspace it saves as you talk**, without asking, in
  every chat (also one that read web pages or files: the owner's decision of
  2026-11-14 was "No, always save"). Each change shows as its own **memory
  block** in the chat, where the assistant made it: "Saved to your facts:
  …", "Saved, in your Personal workspace only: …", "Replaced: old → new" or
  "Forgot: …", each with **Undo**. Undo removes a save, brings back the fact
  a replacement or forgetting retired, and the block then says it was undone
  (after a reload too). The next chat already knows a saved fact.
- **In a team, a save to the workspace's memory applies at once only if
  you could make it yourself**: its admins and owners change the memory by
  hand (add, edit, retire, restore, delete) and so their assistants' saves
  apply at once; with the switch "Every member may change this memory"
  (Settings → Workspace → Memory, off by default) every member's do.
  Guests never change it, switch or not. Otherwise the change waits, and
  first only you see it (your chat is private): the block says what the
  assistant wants, and your **Keep** sends it on to a workspace admin, who
  then sees it under **Waiting** on the workspace's Memory tab (with your
  name, and a count on its Overview) with Keep and Dismiss; your
  **Dismiss** drops it. Saves to your own facts from a team chat wait for
  your Keep in the block.
- **Every member sees who wrote what** in a workspace's or a project's
  memory: "added by", "kept by" and "retired by", with dates ("a former
  member" once that account is gone). Facts there are retired, never
  deleted, by whoever may change them; only the workspace's admins and
  owners (a project's managers) delete a fact, and only one already
  retired.
- **Correcting and dropping facts works for every place** the chat is given
  (the owner, 2026-11-14: "replaces / forget - must work in all memory types
  with ability to revert the change via the block"). A correction stays
  where the fact was. It applies at once where you could make that change
  yourself, with Undo on the block; otherwise it waits for whoever may decide
  it (a workspace admin on the workspace's Memory tab, a project manager on
  the project's), who sees exactly what it would replace or drop. Retired
  facts stay under "Replaced or forgotten", restorable.
- **Never at once**: a sub-agent's save, a message not sent by you signed in
  (the API token, a routine's run), a change to a fact you wrote yourself.
  Those blocks ask, with **Keep** and **Dismiss**.
- **Team workspaces ask about your facts.** When you join one (or create
  it), a question at the top of the page asks whether its agents may use
  your facts. Until you answer yes, nothing of your facts is read or added
  there (the workspace's own memory is given either way). Your answer can be
  changed later in Settings → Workspace.
- **Who sees what.** Your facts and your Personal workspace's memory: you
  alone, not a workspace's admins, not the server's admins, not the API
  token. A team's memory: its members, and the API token (which acts as an
  owner of every team workspace). Leaving a workspace ends your access at
  once, and the
  memory stays with the workspace. Deleting a workspace deletes its memory;
  deleting an account deletes its facts and its Personal workspace's memory.
- What the assistant reads: each place's facts, newest first, up to 3,000
  characters (older ones only when the message matches them), looked up each
  turn and sent again only when they changed (see "What goes with each
  message"). What
  the assistant saved itself may take up to 3,000 characters per place
  (your facts and your Personal workspace together); past that, a save is
  refused and the next turn asks it to merge or drop facts first. Facts
  people add by hand do not count. A team's memory is other people's
  writing, and the assistant is told so.
- Temporary chats, turns started by an external event and turns that
  report a finished background command read and write no memory. Anything
  that looks like a password, key or token is refused ("my passwords are in
  1Password" is fine; the password itself is not).

A data directory made before workspace memory existed must be recreated
(facts kept per person in a workspace are not carried over).

**Earlier chats and memory search.** An agent with this switch (on the
Agents page; on for the Assistant, the Researcher and the Reviewer; off
for the built-in agents that can run commands, use MCP servers or publish,
and for Smol and Scout: built-in agents cannot be changed there, so to give
it to the Software Developer, **Duplicate** it and turn the switch on) has a `recall` tool: when you refer to earlier work it was not given
("like last time", "the plan we made"), it searches **your own** earlier
chats in the chat's workspace and the memory that chat reads anyway (your
facts where the workspace may use them, the workspace's memory, the
project's facts; never a waiting proposal or a retired fact), by keywords,
with no model call. It gets short quoted excerpts, each with the chat's
title, when it was last active and its id; it cannot open a whole chat or
change anything, and the call shows in the chat like any tool.

- Never another person's chats, never a chat in another workspace, never a
  temporary chat, a sub-agent's session or the chat itself. The server
  decides that from the chat the agent works in; nothing the agent sends
  can name a person, a workspace or a chat.
- Not in temporary chats, turns started by an external event (a routine's
  webhook) or turns that report a finished background command. A
  sub-agent searches only when its own agent has the switch.
- Search with a few distinctive words: each word also finds its longer
  forms ("decision" finds "decisions"), and chats with more of the words
  come first. At most 8 searches per turn and 40 per chat per hour, at
  most 10 results and about 6,000 characters of excerpts per search.
- **Recalled text can leave the machine.** In a chat in Auto with the
  node's network on, an agent that also has `bash`, MCP tools or web
  search can send what it found out (in a command, a tool call or a search
  query) with nothing asked. Ask or Accept edits asks before commands and
  MCP calls; `[sandbox] network = "off"` stops commands reaching out.
- The excerpts are quoted to the agent as text written then: earlier chats
  hold web pages, tool output and other people's files, so it is told to
  check a detail before relying on it and never to follow instructions in
  them. A node turns the tool off with `denied_tools = ["recall"]`.

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

**Mentioning a file.** In an agentic chat, type `@` and a path to hand the
agent a file of the chat's folder with your message, as Claude Code, OpenCode
and goose do: the agent gets its content at once, without spending a `read`
on it. A token with a `/` or a `.` is a path (`@src/main.rs`, `@README.md`,
`@docs/`), anything else stays an agent's handle (`@coder`); a file whose
name is a bare word is `@./Makefile`, a folder `@coder/`. While you type, a
"Files" list offers matching paths from the chat's folder (a search by name
honours `.gitignore`; after a `/` it lists that folder's entries, so a folder
you pick opens them); "No file or folder by that name" says so before you
send. Only a token that names a file or folder the chat's folder holds is a
mention: each one shows as a chip above the box before you send (take one off
with its ×), and only those go with the message; `@types/node` or any other
path that names nothing stays plain text, with no note to the agent. The
sent message shows each mention as a chip with its path. The node reads each path under the rules the agent's own `read`
follows: never outside the chat's folder (unless node.toml's
`allow_outside_workspace` lets `read` go there), never the node's data
directory, never through a link that leads out, and not at all on a node
whose `denied_tools` holds `read`; it asks no approval, since sending the
message is yours. A file arrives quoted line by line, like an attached file,
with its path and line count, read when you sent the message. Caps: at most
20 files per message (the rest are named), 64 KiB of text for all of them
together (a file that does not fit is cut at a line, and the agent is told to
`read` the rest from that line), a file over 256 KiB or a binary file is named
and not shown, and a folder lists at most 200 entries. The file's text
reaches the agent with the node's secrets scrubbed, as a `read` of it would.
A file shown (whole, or its first lines) counts as read
for the rule that `edit` and `write` change only files the agent has read, so
the agent can edit it straight away. A chat without a folder offers no files.

Every agentic turn is checkpointed and its file changes can be undone
from the chat. "Show changes" on a turn's "N files changed" row opens what
the turn changed beside the chat: each file with its added and removed lines
(three lines of context, line numbers, `+` and `−`), renames as renames,
binary files and files over 512 KiB named with their sizes, hidden characters
shown as `<U+…>` codes. "Include later turns" shows what "Rewind" would undo.
Changes made by shell commands show like any other. The node reads them from
the turn's checkpoints when you ask; nothing is stored on the server. A very
large turn is shown in part, marked "Cut" (at most 1,000 files are listed and
about 1 MiB of lines is sent); a checkpoint that is no longer kept, or a node
that is away, is said in words. If another chat changed (or may have changed)
some of these files since the turn began, the view names them before you
press Undo (see "Two chats in one folder" below). Only the chat's owner can open them. Checkpoints live in a private repository in the node's data
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
the folder. If you or another sub-agent changed one of them meanwhile,
the two versions are merged file by file the way `git merge` does
(changes to different parts of a file both land; the delegate card says
how many files were merged). When the changes overlap, or a file was
deleted on one side and changed on the other, added on both, a link, a
binary file or over 1 MiB on both sides, nothing of that sub-agent is
written, and the agent that delegated gets the list of those files and
the sub-agent's whole diff (up to 16,000 characters) so it can make the
changes itself; that result is marked failed and ends by telling the agent
to apply the changes before it answers you. A file the sub-agent would
create where you have a file git ignores (your `.env`, if it changed
`.gitignore`) is treated the same way: yours is never overwritten. Files
the sub-agent made that the repository ignores (build output, logs) and
changes to nested repositories are not brought back; the card names them.
If writing fails partway (a full disk, a read-only folder), the files
already written are put back; if even that fails, the card says which
files hold the sub-agent's version and which checkpoint holds the folder
as it was. A child with more than 500 files changed on both sides, or
whose merging takes over 60 seconds, is handed back the same way.
Several sub-agents started in one step work at the same time (each one
that writes on its own copy) and do not see each other's changes; the
agents are told to start together only work that is independent. Merging
needs git 2.43 or newer on the node. None of this runs your repository's config, hooks, filters, merge drivers or
attributes. Sub-agents that only
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

**Two chats in one folder.** A checkpoint is a picture of the whole folder,
so what another chat (yours or a teammate's) changed there while a turn ran
is in that turn's picture too. An undo or rewind never puts that back
without asking: it is refused, naming the files another chat changed during
the turn (with its edit tools, or its own undo) or may have changed (with a
command, a sub-agent, a background command, or a chat deleted since), and
saying nothing else about that chat. Files your own turn changed only with
its edit tools, one edit after the other, are never counted as another
chat's: the harness checks that those edits account for the whole change.
"Undo anyway" goes ahead; the other chat then gets a line naming its files
that were put back (only those in its own folder), with nothing about
yours, and it cannot bring them back itself (the copy is in your undo's
checkpoint), so it has to make those changes again. Commands are judged by
the other chat's whole turn: if it ran a command while your turn changed a
file with a command of its own, that file is named even if the other
command never touched it. When a chat that changed files is deleted, the
node keeps only its folder and when it worked (no content), so an undo
still asks. Your own edits in your editor during a turn still count as the
turn's, and a chat in an unrelated folder that writes here through a path
outside its folder is not seen.

**Approval modes.** Each agentic chat has an approval mode: how much runs
without a click. The picker in the composer shows it and offers four, each
with one line saying what runs unasked:

- **Auto** (the default for every new chat, sandbox and host alike, and for
  routines that set none): nothing asks. In a sandbox chat the sandbox is
  the boundary (its own processes, its own file view). **In a host chat on
  Auto every command runs as the node's own user, with no sandbox and no
  question asked**: nothing stands between an agent that a web page or a
  file has steered and the machine. The picker says so on that line.
- **Accept edits**: `write` and `edit` of a file inside the chat's own folder
  run without a card; they are checkpointed and the turn's Undo takes them
  back. A path out of the folder (`../x`, an absolute path elsewhere, a link
  that leads out or dangles) asks on a node with
  `allow_outside_workspace = true`; elsewhere the tool refuses it, and so
  does every node for anything in `.git`, without a card (see below). Every
  command, every publish and every other tool still ask. The node judges
  the path, from the call itself, as the file tool resolves it.
- **Ask**: every command, write, edit and publish waits on a card. Reads,
  searches and the task list never ask.
- **Plan**: the agent looks and writes a plan; nothing changes. It is not
  "ask": a call that would change something is refused by the node before
  any card, with a sentence telling the agent to describe the change in its
  plan, and no answer can lift it: a card still waiting when you switch to
  Plan is denied by the node at once, and so is a late Allow on it. Plan
  shows no cards at all (what it allows runs unasked), so it is not "Ask
  every call minus the writes". Reads, searches, the task list, web
  search and fetch, earlier-chat search, skills and the repository map run.
  `write`, `edit`, `publish`, `remember` (and memory proposals), and every
  MCP tool are refused and not offered (MCP servers are not started in plan
  mode: their effects are only the server's word). `bash` runs in the
  **read-only shell** (docs/read-only-shell.md): contained in bubblewrap with
  the chat's folder and everything else bound read-only and only its own
  `/tmp` (1 GiB by default, `[sandbox] read_only_tmp_mib`; gone when the command ends) writable, in a sandbox chat
  and in a host chat alike; `git log`, `git diff`, `git status`, `grep` work,
  and any write fails with "Read-only file system". `CARGO_TARGET_DIR` points
  into that `/tmp`, so `cargo build` and `cargo test` work where `~/.cargo`
  and `~/.rustup` are in `[sandbox] read` (or `write`, read-only here) and
  the build fits; toolchains that must write their home or the folder (`npm
  install`) fail: a planner reads, greps, runs `git`, and runs scripts and
  tests that write nothing or only to `$TMPDIR`. On a node without that
  containment (no bubblewrap, `[sandbox] mode = "off"`, a bubblewrap that
  cannot size a tmpfs) plan mode has **no shell** and the agent is told why.
  `delegate` starts only agents that cannot change files (no `write`, `edit`,
  `publish`, MCP or "all tools"); a sub-agent started in plan mode is in plan
  mode for its whole life. A background command started before you switched
  to Plan keeps running as it was started (stop it from the chat or with
  `bash_stop`, which plan mode allows); a sub-agent that changed files before
  the switch has its changes kept in the node's store and shown, not written
  into the folder. The agent is told plan mode is on, that only you can end
  it, and to end its turn with a plan. Plan mode stops changes, not
  reading: where the chat has network access, the agent can still send out
  what it reads (`web_fetch`, the shell), as in every mode. **Only you leave
  it**: pick another mode in the composer, or one of the choices in the plan
  bar above it while no turn runs ("Auto", "Accept edits", "Ask": each
  switches and sends "Carry out the plan."); nothing the agent, a skill,
  an MCP server, a routine or a webhook does switches it. The plan is the
  agent's last answer, ordinary chat history the next turn reads.

No mode shows a card for a call that cannot happen: a path the file tool
would refuse whatever you answer (outside the folder on a node with
`allow_outside_workspace = false`, inside `.git` for a write, the node's data
directory) or a `bash` `workdir` outside the folder is refused before anyone
is asked, with the tool's own reason, and the tool still checks again when an
allowed call runs.

These defaults are the owner's decision of 2026-11-14, in their words: "default accept mode is Auto for every chat type, no limits for apart chat types or agentic host access modes, so each can use auto, always accept, manual, etc...". Pick another mode for a new chat beside its
folder, or change it in the open chat at any time; the chat shows each change.
A lower mode applies from the agent's next call; a card already waiting stays
waiting until you answer it. Only the chat's owner can change it, and the
server's API token (which can already prompt and approve any chat); nobody
else, admins included, can see the chat. Raising it needs the execution level
the chat needs; lowering it is never refused.
Nothing the agent writes, no tool output and no sub-agent can change it: a
sub-agent never runs above the chat that started it, as that chat's mode is
now, so lowering the chat also lowers a sub-agent that is working. A
sub-agent's chat and a routine's chat cannot be switched (a routine sets its
own mode in its editor). A routine run that holds a webhook's data always
asks, whatever the routine's mode: whoever can send the webhook writes into
its prompt.

**The node decides.** Each node clamps every mode it is sent to its own
`max_approval_mode` in `node.toml`, records the result in the chat's log (so a
restart and every open tab show it), and clamps it again after a restart if
node.toml was tightened meanwhile. A fresh `node.toml` allows `auto`. A node's
owner lowers it with one line, `max_approval_mode = "ask"` (or
`"accept_edits"`), then restarts the node: from then on every chat there,
sandbox and host, runs at most at that mode whatever anyone picks, and the
picker shows the higher modes disabled with the reason. In `ask`, a call the
person allowed "for this chat" still runs without a card; `ask_all` asks for
every call, granted ones included. To make every change and command ask on a
node, also set `chat_grants = false` in its `node.toml`: no card offers "for
this chat" there, and grants already in a chat's log are not honoured while
it is off.

**Approving a call.** In `ask` mode a command, a write, an edit or a publish
waits on a card that shows the whole call: every argument, the full path (and
whether it is absolute or outside the chat's folder), long text with its size,
beginning and end and a "show all" for the middle, runs of blanks as counted
markers, and hidden or look-alike characters as codes. "Allow once" runs this
call; "Allow this exact call for this turn" also lets the identical call run
again without asking until the turn ends (any other call still asks); "Allow
this exact call for this chat" lets the identical call (the same command and
every other argument, its working folder included; for a file, the same change
to the same file) run without asking for the rest of the chat, also after a
restart, until you remove it: the chat's side panel lists each one under
"Allowed in this chat" with Remove. **What that allows:** the same command,
with the same arguments, runs with no card for the rest of the chat, and
it runs whatever the files it reads say at that moment: allowing `make test`
allows whatever the Makefile and the tests say then, and in Accept edits the
agent can change those files without a card, so such a grant amounts to
letting it run code of its choice unasked. The card says so beside the
button. It applies in Ask and Accept edits only
(Ask every call means every call), never in another chat or in a sub-agent's
chat, never to a background start, and never past the node's `denied_tools`.
"Deny" refuses the call; the field beside it takes an optional note (up to
2,000 characters) that reaches the agent with the denial, quoted as your own
words, so you can say what to do instead. The field is never focused by
itself and Enter in it answers nothing: only the Deny button sends it. If you
allow while part of the input is collapsed, the card says how many characters
you have not expanded.

`node.toml`: the node's own policy. It is authoritative: the master can only
ask for stricter settings.

**Choose a project folder.** A session's folder may not be the node's data
directory, be inside it, or contain it: with the default data directory
`~/.local/share/rizzy-harness`, your home folder and `~/.local` are refused
(use `~/projects/x` or similar). The file tools (`read`, `write`, `edit`,
`grep`, `glob`, `outline`) never reach into the data directory, even with
`allow_outside_workspace = true` or through a symlink. The shell runs in a
sandbox that cannot open it either (see "The shell sandbox" below); its
output is still scrubbed of the API token and known keys. When the model asks for several `read`, `grep`, `glob` or `outline`
calls in one step and none needs a card, they run at the same time (at most
8 at once); their results still appear, and reach the model, in the order it
asked. Every other call, `bash` included, runs alone in its place.
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
max_approval_mode = "auto"           # plan | ask_all | ask | accept_edits | auto: the most any chat here may use (see "Approval modes")
chat_grants = true                   # offer and honour "Allow this exact call for this chat"; false: never
workspace_roots = ["/home/me/code"]  # session cwd must be inside one of these
denied_tools = []                    # e.g. ["bash"]; "delegate" turns sub-agents off
allow_outside_workspace = false
graphify = false                     # let agents query a graphify knowledge graph (see below)
env_pass = []                        # extra variables the agent's shell gets (see below)

[limits]
tool_stream_bytes = 1048576          # live output one tool call may stream (4096 .. 1 GiB)
max_steps = 1000                     # model calls that run tools in one turn, and the most a chat may be set to; a person's turn then gets one to sum up (1 .. 10000)
bash_timeout_max_s = 3600            # longest a bash call may run, whatever it asks for (1 .. 3600)
model_first_byte_timeout_s = 600     # wait for a model's first answer (10 .. 3600)
model_idle_timeout_s = 300           # then the longest gap between its events (10 .. 3600)
max_subagents = 4                    # sub-agents one turn may start; the master may ask for fewer (0 .. 32)
max_delegation_depth = 1             # 1: the turn's agent may delegate, its sub-agents not; 0: off (0 .. 4)
background_per_session = 8           # background commands one chat may run at once; 0: off (0 .. 32)
background_per_node = 64             # background commands this node runs at once, all chats (0 .. 256)
background_output_kb = 256           # output kept in memory per background command (16 .. 4096)
callback_turns = 3                   # turns the agent may run in a row without you when commands end; 0: off (0 .. 20)
callback_max_steps = 20              # model calls one such turn may make, never above max_steps or the chat's own limit (1 .. 10000)
handoff_notes_per_hour = 4           # hand-off notes this node writes per chat per hour; 0: off, the server asks for none (0 .. 60)

[sandbox]
mode = "required"                    # required | best-effort | off
network = "on"                       # on | off (off: no network for the shell, see "Network off")
read = []                            # more read-only folders, e.g. "~/.cargo"
write = []                           # more read-write folders, e.g. "~/.cache"
# bwrap = "/usr/bin/bwrap"           # which bubblewrap; default: the first root-owned one on PATH
read_only_tmp_mib = 1024             # the read-only shell's /tmp per command, in memory (64 .. 65536)

[execution]
sandbox = "auto"                     # auto | strict | off: offer the sandbox mode (bubblewrap), see "Execution modes"
host = false                         # offer the host shell: commands as this OS user, no sandbox. Read "Execution modes" first

[skills]
repository = true                    # read the chat repository's .agents/skills and .claude/skills (see "Skills")
roots = ["~/.agents/skills", "~/.claude/skills"]  # this machine's skill folders; every chat on the node can read them

[instructions]
# user_file = "/srv/rizzy-harness/AGENTS.md"  # the operator's instruction file; default: AGENTS.md in the node's data directory.
                                     # Refused inside a workspace root, a [sandbox] folder or /etc, /usr, /opt (see "Your own instructions")
                                     # It reaches EVERY chat on this node, whoever owns it (see "Your own instructions")
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
  only check, and only in Ask. **A host chat may run in Auto, and new chats
  start in Auto** (the owner's decision of 2026-11-14, in their words: "default accept mode is Auto for every chat type, no limits for apart chat types or agentic host access modes, so each can use auto, always accept, manual, etc..."): then every command runs as this node's
  user with no question asked, and nothing stands between a steered agent
  and the machine. The warning shown when you choose the host shell says
  so. A node's owner who does not want that sets `max_approval_mode = "ask"`
  and `chat_grants = false` in its `node.toml` (with `ask` alone, a call the
  person allowed "for this chat" still runs unasked). A card in a host chat says the command runs in the
  host shell. Run such a node as a dedicated
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

**MCP servers.** A node can give its agentic chats the tools of Model
Context Protocol servers (stdio servers only, for now). Only the node's
own `node.toml` adds one: nothing the server, a prompt, an agent or a
repository's `.mcp.json` or `.codex/config.toml` says starts anything. The
fields are Codex's, so a definition can be copied over:

```toml
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_..." }  # secrets only: given to this server, scrubbed everywhere
env_vars = ["SSL_CERT_FILE"]          # more names passed from the node's environment
# enabled_tools = ["create_issue", "list_issues"]   # only these (default: all it lists)
disabled_tools = ["delete_repository"]
startup_timeout_sec = 30              # start, initialize and list its tools (1 .. 300)
tool_timeout_sec = 300                # one call (1 .. 3600)
# enabled = false                     # keep the table, start nothing
```

- **Where it runs.** Each chat starts its own copy, at its first turn
  that offers MCP tools, exactly where that chat's `bash` runs: in a
  `sandbox` chat inside bubblewrap (its file view, its network rule, its
  folder read-write), in a `host` chat as the node's user. So a server
  needs what a command needs there: `npx` needs `[sandbox] write =
  ["~/.npm"]` and the network on; a server that reads a config file needs
  its folder in `[sandbox] read`. Sub-agents get no MCP tools.
- **Its environment** is the agent's core list above (not `env_pass`),
  plus its own `env_vars` and `env`. Provider keys, `.env` names and
  `HARNESS_*` are never passed; `env` values are scrubbed from every
  output like the provider keys. So put only secrets in `env`: a value
  such as `/home/me` there would be blanked in every chat's text. Pass
  ordinary variables with `env_vars`.
- **Its tools** are offered as `mcp__<server>__<tool>`. An agent gets them
  when its `tools` list holds `mcp` (Software Developer and Senior
  Teamlead do; agents made on the Agents page with the "MCP servers"
  switch do: "Run commands" alone does not bring them), or names
  one exactly; `deny_tools: [mcp]` removes them all, and `denied_tools` in
  node.toml refuses one by name, `mcp__<server>` all of one server's, and
  `mcp` every MCP tool on the node. A chat carries at most 40 MCP tools and
  64,000 bytes of their descriptions and schemas, and leaves out a tool
  whose schema is over 8,000 bytes; what is left out, and a server that
  did not start, is shown in the chat once (choose with `enabled_tools`).
- **Every call is asked about** in the `ask` mode, like `bash`: the
  harness did not write the server, and what a server says about its own
  tools (`readOnlyHint`) is never believed. A result is tool output like
  any other: text only (an image or a binary resource is named, not
  shown), cut at 30,000 characters. A message from the server over 8 MiB,
  a call past `tool_timeout_sec` or a server that exits fails that call
  with the reason (and the end of its error output); the server is
  started again at its next use, at most 3 times in a row (a successful
  call starts the count again).
- **It never outlives its chat.** A server is stopped when its chat is
  deleted, when the node stops, and after 10 minutes without a call (it
  starts again when needed). A `sandbox` chat's server also dies with the
  node; a `host` chat's can outlive a `kill -9` of the node, like its
  commands.

`rizzy-harness doctor` starts each server once and prints how many tools
it offers, or why it did not start.

Only stdio servers exist: the client library (`rmcp`, pinned at 3.8.0) has
an advisory against its Streamable HTTP client before 3.9.0
(RUSTSEC-2026-0462, a response body read without a limit), code this build
does not enable. An HTTP transport must not be turned on before the pin
moves to 3.9.0 or newer.

**Streamed output.** What a command prints is shown live and stored, up to
`[limits] tool_stream_bytes` per call (default 1 MiB). Past it the stream
ends with `[output truncated: further output is not shown or stored]`; the
command keeps running until it exits, times out or is cancelled, and the
rest of its output is read and dropped. The result the model gets has its
own, smaller cap and says when the live stream was cut.

**Step and time limits.** A turn may make `[limits] max_steps` model calls
that run tools (default 1,000, 1 to 10,000 in the node's `node.toml`). The
default is the owner's decision (2026-11-15: "Raise max to 1000 by default +
add ability for session owners to override max steps (min: 1 - {max
setted})"). **What it costs:** with Auto as the default approval mode, one
message can run up to a thousand model requests with nobody clicking
anything, and each of those steps can carry several tool calls. Sub-agents
multiply it: by default a turn may start 4 of them, running in parallel, each
with up to the chat's limit of its own (more where node.toml's
`max_subagents` and `max_delegation_depth` allow), so one message can cost
about (1 + its sub-agents) times the limit. The sub-agent token budget
(`max_subagent_tokens`, below) only refuses new sub-agents once it is spent;
it does not stop one already running (a follow-up). What bounds it: the chat's own step limit,
the node's `max_steps`, the repeated-call guard (the same call with the same
result is refused and then ends the turn), the model stream timeouts, Stop,
and the provider's own rate and spending limits. A chat's owner can set the
chat's own limit, from 1 up to the node's, in the composer next to the
approval mode ("N steps"): the node clamps whatever it is sent, records it in
the chat (shown as "Step limit: N"), keeps it across a restart, and applies
it from the running turn's next request (a limit below what the turn has
used gives it its wrap-up next). A callback turn (see "Background commands")
takes the lower of `callback_max_steps` and the chat's own limit. Only the owner (or the API token) can change
it; nothing the model, a tool, a skill or an MCP server writes can. A
sub-agent's cap is at most its parent chat's limit (a sub-agent whose parent
chat was deleted gets 1); a routine's chat uses the node's limit.
Halfway through, the agent is told how many it has used. A person's turn then
gets one more call that runs no tool: the tools stay listed (the provider's
cache) but the provider is asked for no tool call (`tool_choice: none`), the
agent is asked to say what is done, what is left and what it would do next, a
tool call it makes anyway is answered "not run", and the turn ends as "step
limit reached", not as an error. Reply to let it carry on. A message you send
just before that last call is not read by it: it starts the next turn, with a
full budget. A sub-agent's last call may only `finish`; a callback turn (see
"Background commands") still ends with "stopped after N steps". A model that
sends nothing is cut off: `[limits] model_first_byte_timeout_s` (default 600)
bounds the wait for its first answer (a reasoning model may think for minutes,
a local server may read the prompt before it answers), and
`model_idle_timeout_s` (default 300, Codex's value) the gaps after it
(keep-alive pings count); both 10 to 3600. Such a call is tried once more,
never after part of its answer was shown, then another model of the agent's
answers or the turn ends with an error naming both keys. A `bash` call runs for
the `timeout_secs` it states, at least 5 s and at most `[limits]
bash_timeout_max_s` seconds (default 3600, 1 to 3600, which wins over the
5 s floor if set lower); **a call that states none stops after 60 s** (it was
600 s before background commands existed): a build or test run that takes
longer must say so, or run in the background. The timeout result says how.
The tool description the model sees states the ceiling. Neither an agent
definition, a prompt nor the master can raise either; a value outside its
range stops the node at start.

**Tool arguments are checked before anything runs.** Every call's arguments
are checked against the schema the model was offered, before the call is
shown, asked about or run: required arguments, their types, values from a
fixed list, and arguments the tool does not take (a misspelt `timeout` is no
longer silently ignored). A number or `true`/`false` sent as a string
(`"timeout_secs": "300"`) is read as the number or the boolean, and the call
card, the approval card and the tool all see the corrected value. Anything else
is sent back to the agent as the call's result, naming the argument and what
it should be, and nothing runs. A call to a tool that is not on offer lists the
tools that are. Schema rules the check does not implement (for example an MCP
server's `pattern` or `oneOf`) never refuse a call; the tool checks those
itself.

**`bash` in a sub-folder.** `bash` takes an optional `workdir`, a folder inside
the chat's folder (relative to it, or absolute) to start the command in, so in
a monorepo the agent does not prefix `cd sub &&` to every command. It is
resolved like a file tool's path and must stay inside the chat's folder, links
included (a link out is refused by name, even on a node with
`allow_outside_workspace`); the shell's grants do not change. It shows on the
approval card and is part of what "allow for this turn" covers. Background
commands honour it. Each command still starts a fresh shell.

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

A second rule catches one tool failing the same way whatever its input: an
agent that adds one more flag to the same failing command again and again
(a live test saw 28 such `bash` calls, each with the same error, until the
provider timed out). From the third failure of the same tool with exactly
the same output in a row (the harness's own changing tokens aside) the
agent is warned that changing the input this way does not change the
result; after seven, its next call to that tool is not run ("Not run: the
calls to this tool before it failed with exactly the same output several
times in a row ..."), and the agent is told that its next call of the tool
will be run once. Other calls of that tool in the same message are refused
too. The tool's next call in a later message runs, because the refusal is of
the tool and a different command may be the right one (`cargo build
--offline` after seven identical network errors); if it fails with the same
output again, the turn ends as above. Any other result of that tool (a
success, a failure in other words) or a change starts it again; calls of
other tools in between do not. Successes are never counted by this rule: a
status poll that keeps answering the same through calls written a little
differently runs on. Calls you deny are not counted by this rule either (nor
do they reset it): seven "no"s to seven different commands are your answers,
and you are asked about every one. The same call denied again is still the
first rule's.
The counts follow Hermes Agent's same-tool failure thresholds (warn at 3,
stop at 8), with the added condition that the output is identical.

**Background commands** are on by default (8 a chat, 64 a node); a node owner
turns them off with `background_per_session = 0` in `[limits]`. While they
are off the agent is not offered `bash_output` or `bash_stop` and `bash` does
not describe `background` (a call that asks for it anyway is refused with the
reason).

**What you see and how you stop one.** Work that runs beside the
conversation is listed in the **side panel** on the right of the chat, as
Claude Desktop does it: the turn's sub-agents on top, then a separator, then
the chat's background commands. The panel opens by itself when the first
one starts; close it with its button, and it shows as a small button with
the count of what is active (on a narrow screen it is always that button,
and the panel opens over the chat). A command's row shows its id (`bg-1`),
the first line of its command (as plain text), its state, the time left
before its timeout stops it, and **Stop**; with two or more running, **Stop
all**. A click shows "stopping…" until the node has ended the command; the
row then says it was stopped, and the chat keeps the line "bg-1 ended
(stopped by user)". Rows of the latest turn stay listed once finished, so you
see what ran; a command from an earlier turn is listed while it runs. A
refused stop shows the reason on its row (Stop can be pressed again). The
panel is drawn from the chat's log, so a reload or a second tab shows the
same rows; only "stopping…" is the tab's own. A command the agent waits for
(not a background one) is not in the panel: it is a block in the
conversation, open, with its output as it comes. **The chat's Stop does not
stop background commands**: it ends the turn, and the panel keeps them,
saying they keep running. When the chat's node is away, the panel says so
and offers no Stop: a stop cannot reach it, and the commands
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
side panel's Stop, and when the node stops or dies; a turn's Stop leaves it
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
own fails (more calls in that step), and summarising calls in a long chat: one
per piece of older turns summarised, and inside a long turn at most 12 summaries
of its own steps (two failed ones stop them for the turn). When
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
in `ask` its calls wait on their cards like any turn's, with no time limit (a
message you send meanwhile waits for the turn, as in any other, and refills
its budget; the card still needs your answer); in `auto` it runs without a
click, within the bounds above. It
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

**Editing files.** The agent changes only files it has seen in this chat:

- `edit`, and `write` over an existing file, need the file to have been
  read with `read` in this chat (any part of it). A new file needs
  nothing. The agent's own edits and writes count as seen. Viewing a file
  with `cat` in `bash` does not count (commands are not parsed), but an
  `edit` of a file not read yet is answered with the lines around the
  first match of its `old_string`, numbered as `read` shows them, and that
  counts as having read the file as it was at that moment: the same edit
  sent again goes through, unless the file changes first. This holds for
  the rest of the turn; the next turn asks for a `read` again. Only for
  one edit without `replace_all` (several edits or every match would change
  lines the agent was not shown), for a file `read` would show (not one it
  refuses as binary), and for an agent that may use `read`. Otherwise, or
  when the text is not in the file, the refusal says to `read` it. In
  every mode that shows cards (Accept edits, Ask, Ask every call) such a
  call shows no card: it would be refused whatever you answered, so it is
  refused before you are asked. It is answered with the file's lines only
  where a `read` of that file would run right then without a card and is
  allowed (so not in Ask every call, nor where the node denies `read`);
  otherwise the refusal shows nothing of the file. An agent without
  `read` (or on a node that denies it) can change only files it created or
  wrote in the chat. The `bash`, `read`, `edit` and `write` descriptions
  the model gets say all of this.
- If the file changed on disk since then (you edited it, a command or a
  formatter did, another chat did), `write` is refused, and `edit` goes
  ahead only when the text it replaces lies outside the lines that
  changed; the result then shows those lines as they are now. Otherwise it
  is refused and shows the changed lines, and the agent reads again. This
  is decided against the text the agent was shown, kept in memory on the
  node (files up to 512 KiB, 8 MiB in all; at most 4,096 files remembered
  per node); after a node restart, or for a larger file, any change means
  "read it again". What the agent has read is stored with each call in the
  chat's log, so a restart does not make it read everything again. A
  sub-agent has its own record; deleting a chat drops it.
- `edit` takes one replacement (`old_string`, `new_string`, `replace_all`)
  or several in `edits`, matched against the file as it was before the
  call, all made or none. Text that does not match exactly is still found
  when, on whole lines, it differs only in trailing whitespace or in
  typographic quotes, dashes and spaces, and the result says so (the
  file's own quotes and dashes are kept where the agent did not change
  them); indentation must match. A line-number prefix copied from `read` output
  is taken off. When nothing matches, the agent is shown the nearest lines
  of the file, or told the change seems to be made already.
- The result lists the changed lines, numbered as `read` numbers them,
  with two lines around each. Line endings (CRLF), a byte-order mark and a
  missing final newline are kept.
- `read` returns up to 2,000 lines (400 with bounded reads) and about
  29,000 characters; a longer window ends early at a whole line and names
  the offset to go on from.

### Token savings

Settings → Server → Token savings switches each method on or off; it is
stored as `[savings]` in `config.toml` and applies from the next message.

| Method | Default | Applies to | What it does |
|---|---|---|---|
| `prompt_cache` | on | chat + agentic | Marks cacheable the instructions and tools, the end of the history before this turn's message, the end of the history before the previous turn's message, and the newest message (at most 4, Anthropic's limit); Anthropic bills re-reads at 10% or less. Within a turn each step reads the one before. Across turns the next turn reads everything up to this turn's last tool result, byte for byte: what was sent with this turn's message (instruction files, skills, memory, the project's brief, your settings line) is recorded with the turn and sent again exactly, and what has not changed is not sent again at all (PLAN.md T52). Usage shows cache reads and writes per turn (the token meter's tooltip). A summary, an image no longer shown, or a change in which tool outputs are hidden makes one request write the history again. |
| `mask_old_outputs` | on, keep 8 | agentic | For a model whose context window is not known: tool outputs older than the newest 8–15 become a one-line note, hidden in batches so the cache stays valid. With a known window it does not act: old outputs are cleared only when the context nears the window (`docs/turn-compaction.md`). |
| `filter_shell_output` | on | agentic | Colours, progress bars, build noise and passing-test lines removed, repeats folded; the full output stays on the node (`tool_output`). |
| `bounded_reads` | on | agentic | 400-line read windows, 60 grep matches grouped by file. |
| `outline_tool` | on | agentic | `outline`: a file's definitions with line numbers (Rust, Python, JS, TS, Go). |
| `repo_map` | off | agentic | A ranked map of the repository's definitions (tree-sitter + PageRank) in the instructions. |
| `web_readability` | on | chat + agentic | `web_fetch` returns the main article as Markdown. |
| `chat_style` / `agentic_style` | lite / lite | | Answer style: `off`, `lite`, `full`, `ultra`. Chats are capped at `lite`. |
| `graphify` | off | agentic | `graph_query` tool, only on nodes that allow and installed it. |
| `small_model` | none | | The **small model**: one cheap, fast model (other harnesses call it the small or fast model) for background writing. Picked from the listed models. |
| `handoff_notes` | `small_model` | agentic | Who writes a coding chat's **hand-off note** (below): `small_model` (the small model when one is set, else the chat's own model), `chat_model`, or `off`. |
| `small_subagents` | off | agentic | A sub-agent that changes nothing (every tool it may use is on the read-only list: no `write`, `edit`, `publish`, writable shell, MCP tool or tool not yet listed; its `bash` in the read-only shell, `remember` and the web tools do not count; by its tools, not its name: Scout, Reviewer and Researcher as shipped) and has no model of its own runs on the small model, its parent's model as its fallback; its own chat says which model ran. |

```toml
[savings]
agentic_style = "full"
repo_map = true
repo_map_tokens = 1024
```

**Hand-off notes.** When a coding chat in a project that changed files or
its task list has been idle for 10 minutes, its node writes it a short
hand-off note with
one request (at most about 1,500 characters: where the work stands, the
decisions taken and why, what is left and the next step, what you asked to
keep in mind). The chat shows a quiet line, "Hand-off note for the next
chat, written by …", that opens to the note. The **next chat in the same
project** gets it on its first message, under the facts the harness reads
from the log itself (what you asked first, the files changed, the task list,
how the last turn ended), instead of the old chat's last reply: a coding
chat's last reply is usually "Done, tests pass", which tells the next chat
nothing. Costs: one bounded request per stretch of work, counted in that
chat's usage: up to about 16,600 input tokens, not cached, plus at most 800
output tokens, on the small model, or on the chat's own model when no small
model is set (the default); a chat that goes idle
again without new changes gets no new note; a provider that fails is not
asked again until the chat goes on. If the small model is unknown to the
chat's node or fails, the chat's own model writes it; if that fails too, the
next chat gets the facts alone and the line says why. A message you send
while a note is being written goes first (the note is dropped and written
at the next idle stretch). Never for a temporary chat, a sub-agent's, a
routine's or one holding data from an external event, or a chat whose owner
is disabled; deleting a chat deletes its note. **Where the text goes:** with
a small model set, the chat's recent turns, tool output included, are sent to
the small model's provider, which may not be the provider the chat itself
uses. A node's owner turns notes off on that node with
`handoff_notes_per_hour = 0` in `node.toml` (`[limits]`), which also bounds
how many it writes per chat per hour (4 by default). Notes stay in your own chats:
nobody else's chat reads them, and they never become memory. Chats outside a
project get no note: nothing would read it yet (a chat moved into a project
later gets one, if due). Design:
[docs/handoff-notes.md](docs/handoff-notes.md).

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
it. The **first message of a new chat** in a project also gets your own
**earlier chats** there: up to five, newest first, each with its title, when
it was last active, the facts its log shows (what you asked first, the files
changed, the task list, how its last turn ended) and its hand-off note (see
Token savings), or without one its compaction summary or last reply, cut to
1,500 characters for a note and 800 otherwise, 6,000 in all, every line
quoted, with a note that it may be outdated and is not instructions, so the
agent asks you before relying on a detail. Only your own chats count, never
another member's, a temporary chat or a sub-agent's; later messages of the
chat do not get it again, and neither does a chat whose agent has project
memory off. Keeping your personal memory out of
a team workspace leaves its projects' memory in. Agentic chats started in a project
take its default node and folder. Temporary chats cannot join a project.
Design and open questions: [docs/projects.md](docs/projects.md).

## The environment line

Every message in an agentic chat (a sub-agent's task and a callback turn
included) opens with one line from the node: its clock (weekday, date, time,
time zone and UTC offset), its OS, that the `bash` tool runs bash, and whether
the chat's folder is in a git repository and on which branch (or at which
commit). The model no longer guesses the date or spends calls finding these.
The branch is read from the repository's `HEAD` file, the way instruction files
are read (inside the folder `node.toml` grants, no link out, never waiting on a
FIFO, within the same 5-second bound); no `git` process is started. The branch
name is the repository's text: it is shown on one line, at most 120
characters, with its quotation marks read as `'`. The line is recorded with
the turn, so later messages resend it exactly and the provider's cache still
covers that message; it costs about 60 tokens a message. The rest of what goes
with a message (instruction files, skills, memory, a project's context) is
recorded the same way (see "What goes with each message" below). Your own time
zone, if you set one in your settings, is still sent separately with your
messages.

## Repository instructions (AGENTS.md)

In an agentic chat the node reads the repository's own instruction files at
every message and sends them to the model when they are new or changed, the way
Codex does: the
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

**Folders below the chat's folder** (`web/AGENTS.md`, `crates/tools/CLAUDE.md`)
are read when the agent first works there, as Claude Code, OpenCode and
DeepSeek's harness do: after a `read`, `edit` or `write` of a file under such a
folder succeeds, the instruction file (`AGENTS.md`, else `CLAUDE.md`) of each
folder between that file and the chat's folder is sent with the next request,
right after that step's results, outermost first, under a line of the
harness's saying they apply to the files under their folders. So the model
has `web/AGENTS.md` before its next step after the first call that touched a
file under `web/`: when that first call is a `read`, before it edits there;
when it is a `write` or `edit` (or an edit in the same step as the first
read), only after that change, for the ones that follow. A file you mention with `@path`
brings its folders' files with the message itself (with a message you send
while the agent works: right after that message). Each is sent once and kept
like the repository's own files: read again at the start of every message,
replaced once when it changed, withdrawn when it is gone, and kept when old
tool results are cleared or the turn's earlier steps are summarised. After the
chat is compacted they are dropped with the history they stood in, and the
next touch under a folder sends its file again. Caps: one file at most 16
KiB as sent (cut with a note to `read` the rest), all of them together at
most 32 KiB (with a small known context window, about a tenth of it together
and half that for one file, as above),
and at most 16 files at a time; a file over a cap is named, not loaded. The
same rules as above apply: only regular files inside the chat's folder, never
through a link out of it, never inside `.git` or the node's data directory,
every line indented so no file can pose as another's label, and anything that
looks like a configured secret removed. A file that changed during the
message is named and not taken as instructions in that message: one the
agent's own `write` or `edit` wrote, and any other whose change or
modification time is after the message began (a command such as `echo > ...`,
even one that sets the time back with `touch -d`: the inode's change time
cannot be set back), with 50 ms of slack for the kernel's coarse clock (so a
file you saved in the 50 ms before sending also waits). Not covered: a file
system without usable change times (FAT stores none; there a command's
backdated change counts as old). It is taken in a later turn, the same rule
as for the chat folder's own file, which is read at the next message. A later
turn need not be yours: a turn that reports a finished background command is
one, and nobody started it. A sub-agent takes nothing changed since the turn
that delegated to it began, so an agent cannot write a subfolder's file and
delegate into that folder to have it obeyed. Each folder is looked in once
per message, so a file created there later counts from the next message
on.

Not read: files above the repository's top (or above the folder `node.toml`
grants), `.claude/CLAUDE.md`, `CLAUDE.local.md`, other names, and `@path`
imports inside a file (the line is sent as written; the file it names is not
read).

They are text for the model and nothing more: they change no setting in
`node.toml`, no approval mode, tool or limit, and a link that is only in them is
not fetched on their say-so. Only regular files inside the folder the node
grants are read: a link is followed only to a file inside the repository (the
chat folder's own file only to one inside the chat folder), never out of it or
into the node's data directory, and a file that cannot be read is named with
the reason while the message goes on. A read that takes more than 5 seconds
(a hung network disk) is given up with a note; the node logs each refused,
cut or timed-out read (paths and reasons, never the text).

**Cost:** about 1k tokens for a typical `AGENTS.md` (up to the cap, about 8k
tokens), sent with the first message, again only when a file changed (with a
line saying the new files replace the earlier ones) and once after the chat is
compacted; between those, nothing (see "What goes with each message"). A
subfolder's file costs its size once, when the agent first works there, and
nothing before: loading one appends to the request, so the provider's cache
holds. A file that changes is replaced at the next message's start, and that
request is sent again from the step where the old copy stood (one cache
write per change, as when a root file changes). Looking for them costs a few `stat`s per folder
the first time a message's calls touch it.

### Your own instructions (the node's AGENTS.md)

What `~/.claude/CLAUDE.md` is to Claude Code and `~/.codex/AGENTS.md` to
Codex: one file of your own rules ("answer in British English", "never run
`git push`") that the node sends with every agentic chat, in front of the
repository's files. By default it is `AGENTS.md` in the node's data directory
(`~/.local/share/rizzy-harness/AGENTS.md` unless `--data-dir` says otherwise).
node.toml `[instructions] user_file` names another file (absolute or under
`~/`), but not one an agent could reach: the node refuses to start when it
lies inside a `workspace_roots` entry (by default your home folder, so
`~/.codex/AGENTS.md` is refused), inside a `[sandbox]` read or write folder,
or inside `/etc`, `/usr`, `/opt` and the other folders the sandbox lets every
command read; put it in a folder of its own such as `/srv/rizzy-harness/`.
The path may hold no `.` or `..`; the node resolves it once at start (links
in its folders followed, even where the file or a folder does not exist
yet) and checks and protects where it really leads. At every message it
resolves the path again: if it now leads elsewhere (a folder on the way
became a link, or appeared as one), the file is not read and the agent is
told why; restart the node to accept the new place. Wherever it lies, the
file joins the node's protected paths: `read`, `write`,
`edit`, `@` mentions and the folder listing refuse it in every mode, and the
shell gets no grant on it. In the sandbox mode the shell does not see it at
all (bubblewrap's file view holds only its grants); in the host mode this
rests on Landlock alone (the same rules, no bubblewrap), so it holds where
Landlock is enforced and has that mode's gaps (see "Execution modes": a
command can ask another service of this user, over a socket, to change it
below Linux 7.1). The node reads it only when it belongs to the node's own
user or root and neither its group nor other users can write it (else
another account would set every chat's instructions: it is named, not read;
`chmod go-w` it), and warns in its log and in `doctor` when another user can
write its folder or the file has more than one name (a hard link). No file, or an empty one:
nothing is sent. It is read at the start of every message like the
repository's files, in a 5-second bound of its own, cut at 16 KiB as sent
(indentation included; or half the repository files' cap on a small context
window) with a note, sent again
only when it changed, withdrawn when you delete it, and kept through
compaction. Only the regular file at that very path is read: a link there is
not followed (name the file itself in node.toml; a link in a folder above it
is), a folder or FIFO is named, not opened. The model is told it is the node
operator's text, not your message, and that it changes no setting; every
line of it (and of the repository's files) reaches the model indented by two
spaces, so no line of a file can pose as the label of another.

**On a node several people use**, this file reaches every chat on the node,
whoever owns it: it is the operator's setting, like `[skills] roots`. Write
there only what every user of the node should get. There are no per-user files
yet.

## Skills

A skill is a folder with a `SKILL.md` that the agent loads only when a task
calls for it: the format Claude Code, Codex, OpenCode, Pi, Cursor and others
read (the Agent Skills format), so folders you already have work as they are.

```markdown
---
name: release
description: Cut a release of this project: bump the version, tag, write the notes.
---
1. Run `scripts/bump.sh <version>` from this folder.
2. ...
```

`name` (letters, digits, `.`, `_`, `-`, at most 64; the folder's name when
left out, or when the given one is not such a name) and `description` (what
it is for and when to use it; the first paragraph of text when left out) are
what the agent sees. Front matter that is not strict YAML, such as an
unquoted `description: Use when: ...`, is read line by line, as other harnesses
accept it; the rest of the file
is what it reads when it loads the skill. `disable-model-invocation: true`
keeps a skill out of the agent's list: it is loaded only when you ask for it
(below). Other keys (`allowed-tools`, `hooks`, `model`, `context`, ...) are
ignored: a skill never widens what the agent may do. Files beside `SKILL.md`
(scripts, references, templates) are read when the skill says so.

**Where the node looks**, at the start of every message of an agentic chat:
`.agents/skills/<name>/SKILL.md` and `.claude/skills/<name>/SKILL.md` in each
folder from the top of the chat folder's git repository down to the chat's
folder (as for `AGENTS.md`), and in the folders `[skills] roots` names in
`node.toml` (by default `~/.agents/skills` and `~/.claude/skills`). When two
skills have one name, this machine's folders win over the repository (a
repository cannot stand in for your own `deploy`), and within the repository
the folder nearest the chat's folder wins; the node logs what it left out.
`[skills] repository = false` reads only this machine's folders;
`denied_tools = ["skill"]` turns skills off.

**What the agent gets.** After the instruction files, in the same block, a
list of the skills: one line each, `- name: description`, sorted by name, at
most 150 skills and 8,000 characters (or about 2% of the model's context
window when that is smaller and known). When they do not fit, descriptions
are shortened to 120 characters, then skills are left out, with a note saying
how many; the `skill` tool lists every name when asked for one it does not
know. The cap never goes below room for about ten skills. A skill folder whose
`SKILL.md` could not be used (no description, a link out of its folder, ...) is
named in a line under the list; the node's log says why. Looking for skills
has its own 5-second limit, after the instruction files, so a slow skill
folder costs only the list. Like the instruction files, the list is read again every message and
survives compacting. The agent loads a skill with the `skill` tool: it gets the
`SKILL.md` without its front matter (cut at 24 KiB with a note), the skill
folder's path, and the names of the files beside it, which it reads with the
same tool (`file`) or, inside the chat's folder, with its usual tools. When a
long chat is summarised, the summary names the skills the work still follows
so the agent can load them again. The Software Developer, Senior Teamlead,
Writer, Reviewer and Scout have the tool, as do custom agents that read or edit
files or run commands; the Assistant and the Researcher do not. A sub-agent
with the tool gets the list too.

**Asking for one.** A message of yours that starts with `/name` (a skill's
name, then your text; `/name:` works too) asks the agent to load that skill
first, a manual one included. A task one agent hands another never does.
There is no menu for it yet. `/compact` is taken by the built-in command
(see "Summarise now"): a skill named `compact` is still offered to the
agent, but cannot be asked for this way (a manual one not at all).

**What a skill can do: nothing by itself.** Nothing in a skill runs when it is
found or loaded (no `` !`command` `` expansion); a script it names runs only
when the agent runs it with `bash`, under the chat's execution mode and its
approval like any other command. The folders in `[skills] roots` (their real
paths, a root that is a link resolved) are readable, never writable, in the
shell of a chat whose agent has the `skill` tool, so a script beside a skill
from this machine runs in the sandbox too; chats whose agent lacks the tool do
not see them. The list is quoted to the model like `AGENTS.md`; a loaded body
is a tool result, like a file the agent reads. Neither changes a setting,
permission or tool, and a link only in them is not fetched.
Only regular files are read: a repository's skill folder must lie inside the
repository (the chat folder's own skills inside the chat folder), and a
skill's files inside the skill's folder; links out of them, into the node's
data directory, FIFOs and devices are refused and logged. This machine's skill
folders may be links anywhere outside the data directory. On a node other
people's chats use, they can read the skills in `[skills] roots`: keep secrets
out of them, or set `roots = []`.

**Cost:** about 25 tokens per listed skill, up to the cap (about 2k tokens),
sent like the instruction files: with the first message and again only when the
list changes; a body costs only when it is loaded.

## What goes with each message

Besides your words, a message to the model carries a block the harness adds
in front of them, every line quoted under one line of the harness's: the
repository's instruction files, the list of skills, your memory facts, a
project's brief, facts and board, a line with your name and local time, and
notes (an MCP server that did not start, a `/skill` you asked for). The block
is recorded with the turn (never shown in the chat), so every later message
resends that message exactly as it was and the provider reads the whole
conversation up to the last tool result from its cache (Anthropic bills those
re-reads at a tenth). What has not changed since it was last sent is not sent
again: the model has it earlier in the conversation. Each part counts on its
own: your facts, the workspace's memory, the project's brief, its facts and its
board, so a moved card sends the board alone and a colleague's change to the
team's memory sends that memory alone. What changes with the message (older facts
that match it, the count of saved memory, notes on earlier saves, a project
chat's earlier chats on its first message) replaces its earlier copy, so the
conversation never holds an old recall: a fact you forget is gone from the
next message even if it was only ever recalled. A turn that reports a finished
background command reads the same memory the chat's agent reads in your turns
(it cannot save or search memory), so when nothing changed it costs no new
cache write; a turn that reads messages you sent while the agent worked gets
the context of the newest of them when it was made for the same agent, and
otherwise reads none. A tool result that quoted a fact earlier stays in the
conversation as it was, like the agent's own earlier answers. A changed part is sent in
full with a line saying it replaces the earlier one; a part that is gone (memory
switched off, the files deleted) gets a line saying it no longer applies, and
earlier messages are then resent without their copy: an agent whose memory is
off (an `@mention`, the Agents page switch, a team workspace that does not use
your memory) reads none of your facts, and a fact you forget or correct is gone
from the next message. A file that could not be read in time is kept as the
model last saw it, with a note. After the chat is compacted, everything is sent
once more. In a plain chat the line with your name has your local time, so it
goes with every message; in a chat on a node the node's environment line has
the clock instead, and the line only names you and your time zone.

When more than one agent answers in a chat, each earlier answer of another
agent is marked with that agent's name, so the agent answering now does not
take it for its own. A message that got no answer (the provider refused, you
stopped it at once) stays in the conversation with a note saying so, so the
next message can say "try again".

The record is part of the chat: only you can see it (admins cannot), it is
deleted with the chat, and anything that looks like a configured secret is
removed before it is sent or recorded. A fact you later forget or edit is not
sent again, but stays in the records of the chats it was already sent to,
until you delete them.

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
limits: the node, not the master, decides what may run there), and `git` on
its `PATH`: 2.43 or newer to merge two sub-agents' changes to one file (with
an older git, a file both changed is handed back to the agent instead,
nothing is lost). Revoke a node in
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
max_approval_mode = "auto"   # the default; "ask" makes every chat here ask (with chat_grants = false)
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
| Assistant (`@general`, `@ai`), the chats' default | General | agent | web search and fetch; your facts; project memory; earlier chats and memory search |
| Smol (`@smol`) | General | agent, sub-agent | nothing but answer: no tools, no memory |
| Researcher (`@researcher`, `@research`) | General | agent, sub-agent | web search and fetch; project memory; earlier chats and memory search |
| Software Developer (`@coder`, `@dev`), the Agentic default | IT | agent, sub-agent | read, change and search files, run commands, the node's MCP servers, publish, web; delegates to Scout, Researcher and Reviewer; project memory |
| Senior Teamlead (`@teamlead`) | IT | agent | everything the Software Developer has, plus your facts; delegates to Software Developer, Researcher, Writer, Scout and Reviewer |
| Writer (`@writer`) | General | agent, sub-agent | read, change and search files, publish, web; project memory |
| Scout (`@scout`) | IT | sub-agent | read and search files; read-only commands (`git log`, `git diff`, `grep` pipelines, in the read-only shell) |
| Reviewer (`@reviewer`) | IT | agent, sub-agent | read and search files; read-only commands (`git diff`, `git log`, `git blame`, tests that write only to `/tmp`); project memory; earlier chats and memory search |

**Researcher in Agentic sessions.** Web tools exist only in chats, not on
nodes yet (nodes have `remember` and `recall`, which alone do none of a task's work). In an Agentic session, Researcher is therefore not offered
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
tool list: every tool they may call changes nothing (one allow-list the master,
the node and the loop all read, PLAN.md T70), and the loop refuses any other
call. Their `bash` (the **Read-only commands** switch, on for both, off for a
new agent) always runs in the **read-only shell** (docs/read-only-shell.md),
whatever the chat's mode: the node picks it from the tools the agent has, so
no setting or agent spec can give a read-only agent a shell that writes. They
can run `git log`, `git diff`, `git blame`, `grep ... | sort | uniq -c` and
tests whose output goes to `/tmp` (`cargo test` does: `CARGO_TARGET_DIR` points
there, with `~/.cargo` and `~/.rustup` in `[sandbox] read`), with the network
as `[sandbox] network` says; nothing they run can change the folder, and their
commands ask no card in Ask or Accept edits (Ask every call asks). A read-only
agent still works in its parent's folder (no copy, nothing merged back), may
run on the small model and may be started in plan mode. Where the node has no
read-only shell (no bubblewrap, `[sandbox] mode = "off"`) they get no `bash`
and are told why. A custom agent gets the switch only when someone turns it
on, and it adds a shell only to an agent that changes nothing else; an agent
whose list holds `bash` and only reading tools gets the read-only shell too.

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

**Follow-ups.** A finished sub-agent keeps its conversation. The agent that
started it can send it another message by calling `delegate` again with
`session` set to the id its result began with (`session: <id>`): "now also
cover the error path in the test you wrote" reaches the same sub-agent, which
answers from what it already read instead of a new one reading everything
again. What it keeps: its own conversation (its earlier tasks, tool calls,
results and reports). What it does not keep: its files. One that changes files
gets a fresh copy of the folder as it is now (your changes and other
sub-agents' since included), at the same place, merged back when it ends as
before; one that only reads works in the folder itself, as before. Its message
starts with a line of the harness's saying so and telling it to read a file
again before it edits it or relies on it. The usual rules for files it read
hold: a `write` over a file that changed since it read it is refused until it
reads it again; an `edit` is refused where the lines it edits changed, and
otherwise runs with a note that the file changed. If its earlier changes were
not applied, or applied only in part, the line says so. A follow-up counts against
the turn's sub-agent and token limits like a new one; a sub-agent has at most
8 runs; only the chat or sub-agent that started it can send it one (another
chat's, a sub-agent's own sub-agent's, an unknown or a deleted id is refused
with a sentence); one that is still working takes no second message; and it
runs with what a new sub-agent of the same agent would get now, at most your
chat's approval mode as it is now (in plan mode a sub-agent that changes files
is not given one, and one started in plan mode stays in it). Its chat marks each follow-up, and the side
panel lists the sub-agent once with its number of runs.

**Agents** in the sidebar lists every agent. Server admins can make agents there
(name, handle, summary, instructions and feature switches: memory, project
memory, earlier chats and memory search, web search, files, commands, MCP
servers, sub-agents, and whether it is offered as an agent,
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

A chat's background command in the side panel (not above the composer), a
foreground command as a block in the chat, the turn's Stop leaving the
background one, the panel's Stop ending it (needs bubblewrap; port 7786, or
the fourth argument):

```bash
node web/e2e/background.cjs target/debug/rizzy-harness
```

Writing while the agent works: a message sent during a running command is
taken, marked "sent while working", read after the command's result, and one
sent during the last answer starts the next turn (needs bubblewrap; port
7964):

```bash
node web/e2e/steer.cjs target/debug/rizzy-harness
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

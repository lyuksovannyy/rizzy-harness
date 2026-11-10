# Round 2, second wave: task descriptions

Each section is the task for one agent. The shared instructions are in
`ROUND2-BRIEF.md` (same folder). Clones are at `.claude/research-repos/<slug>`.

## goose.sessions

Repository: **goose** (`aaif-goose/goose`; Rust, like the reader's harness).
Cluster: **sessions**. Output: `research/round2/goose.sessions.md`. Round 1
file: `research/round1/goose.md`.

Be sure to settle: session storage (the database or files, schema, resume,
fork, export); the server (`goose serve`, formerly `goosed`): its HTTP and ACP
surfaces, how the desktop app and other clients talk to it, authentication
with a shared secret and TLS fingerprint pinning for remote use (find where
the fingerprint is checked and how the secret is generated and compared); the
Agent Client Protocol implementation (as an agent and as a client driving
other agents: which methods, how permission requests and file or terminal
operations are mapped); "Roaming", the experimental peer-to-peer transport
(iroh/QUIC, key cards, revocation), as far as code exists; the scheduler;
extensions as MCP servers (transports, built-in ones, how tools are
discovered); custom distributions.

## openhands.safety

Repository: **openhands** (`OpenHands/OpenHands`). Cluster: **safety**.
Output: `research/round2/openhands.safety.md`. Round 1 file:
`research/round1/openhands.md`.

Round 1 found that much of the engine now lives in a separate agent SDK and
Agent Server; first establish what code is actually in THIS repository (the
application, the server, the canvas UI, enterprise parts) and what is only
referenced from other repositories, and say so plainly. Do not clone other
repositories; list the ones a complete study would need.

The reader plans to run sessions in containers on worker machines, so the
container path matters most. Settle, as far as the code here goes: how a
conversation's container or sandbox is created (the Docker or other runtime
invocation, image selection, mounts, user, network, resource limits, hardening
flags, who can influence them), the "outer server proxy" in front of
per-conversation containers, warm or dormant sandboxes and how one is
activated; the security analysers and confirmation policy (Round 1's review
found two mechanisms and no hard-deny layer: confirm); authentication of the
server and of back ends (the API key model, and that the open-source build has
no user accounts); secrets handling and how provider keys reach the sandbox.

## openhands.agents-sessions

Repository: **openhands** (`OpenHands/OpenHands`). Clusters: **agents and
sessions together**. Output: `research/round2/openhands.agents-sessions.md`.
Round 1 file: `research/round1/openhands.md`.

First establish what code is actually in THIS repository and what is only
referenced from elsewhere (Round 1 found the engine largely moved to a
separate agent SDK and Agent Server); do not clone other repositories, and
list the ones a complete study would need. Cover each topic as far as this
repository's code goes and say "not in this repository" where it stops.

Settle: how the web application talks to back ends (the "Canvas" client with
several independent Agent Server back ends: the protocol, events, how
conversations are stored and resumed, what is scoped per back end);
conversation persistence and the event stream format; running other harnesses
(Claude Code, Codex and others) inside the server over ACP; the
OpenAI-compatible endpoint where a model name is an agent profile; microagents
or skills and how they are triggered; context condensers; delegation between
agents (Round 1's review says the delegate tool is retired: what replaced it);
integrations that start conversations (GitHub, GitLab, Slack resolvers); what
the enterprise directories contain versus the open part.

## cline.safety

Repository: **cline** (`cline/cline`). Cluster: **safety**. Output:
`research/round2/cline.safety.md`. Round 1 file: `research/round1/cline.md`.

Settle: the task loop and tool dispatch; the approval model in code
(auto-approve settings and their defaults per surface, the model-supplied
`requires_approval` flag on commands and whether anything checks it
independently, YOLO mode, the CLI's default, what approving a sub-agent spawn
covers); Plan versus Act mode and whether Plan is enforced (Round 1 notes a
model can still write through a shell one-liner); command execution (shell
integration, timeouts, output handling, background commands); whether any
sandbox exists; hooks and what a failing hook does; `.clineignore` and other
file-access rules and how they are enforced for the shell; enterprise remote
configuration and which surfaces it reaches; secret storage.

## cline.agents-sessions

Repository: **cline** (`cline/cline`). Clusters: **agents and sessions
together**. Output: `research/round2/cline.agents-sessions.md`. Round 1 file:
`research/round1/cline.md`.

Agents side: system prompt assembly per model family and the tool definitions;
the edit tools (search-and-replace blocks or other formats: the matching,
tolerance, failure handling, the diff view); context management (auto-compact
or condense, the truncation strategy, the new-task hand-off, the focus chain
or task list); sub-agents (`spawn_agent` or equivalent: context, tools,
limits, results).

Sessions side: **checkpoints** (Round 1 says a shadow git repository: find the
implementation, where the repository lives, when a checkpoint is taken, what
restore does, the guards for large repositories or nested git directories);
task history storage and resume; the **hub and spoke runtime** (a per-machine
hub daemon that routes events and approvals between clients and keeps sessions
alive: the protocol, what a client can do, whether there is any
authentication: Round 1 found none documented); the **desktop SSH remote**
(uploading a helper, starting a hub on the remote host, the tunnel, host-key
handling); the core and host split and its protocol; the MCP client and
marketplace; rules, workflows and skills loading.

## zed

Repository: **zed** (`zed-industries/zed`; a large Rust editor: stay inside
the agent-related crates). Focused scope, not a whole cluster. Output:
`research/round2/zed.md`. Round 1 file: `research/round1/zed-acp.md`.

Find the agent crates first (the agent panel and thread, tools, tool
permissions, sandboxing, the ACP client and agent servers, context servers for
MCP). Then settle:

- **Checkpoints.** Round 1 records that per-prompt Git checkpoints were
  removed in 1.27.0 because they slowed or hung responses in large
  repositories. Find what remains in the code (is the mechanism gone,
  disabled, or replaced by something such as per-edit undo or an action log)
  and anything that shows how it worked and why it was slow. The reader's own
  harness takes a checkpoint per turn in a shadow git repository, so this is
  the most valuable thing you can find.
- **Tool permissions**: the per-tool pattern lists (always allow, deny,
  confirm), how a terminal command is split into sub-commands before matching,
  the precedence.
- **Sandboxing** of the terminal and fetch tools (Seatbelt, Bubblewrap, the
  Windows path): how the policy is built and what happens when it is
  unavailable.
- **The ACP client**: how Zed launches and talks to external agents, how
  permission requests, file reads and writes and terminals are served by the
  client, how agents are found in the registry.
- **Edit tools** and how agent edits are tracked and reviewed.
- **Compaction or context limits** for threads.
- If the ACP protocol's schema or a crate for it is vendored in this
  repository, describe the session, prompt, tool-call and permission messages
  from it; otherwise say which repository holds it.

## warp

Repository: **warp** (`warpdotdev/warp`; a large Rust terminal client under
AGPL; the server, control plane and cloud orchestration are closed services).
Focused scope. Output: `research/round2/warp.md`. Round 1 file:
`research/round1/warp.md`.

First establish what agent logic actually lives in the open client and what is
only a call to Warp's services: where the agent loop runs (client or server),
what the client decides locally. Then settle, as far as the code goes: command
permissions on the client (allow and deny lists as regular expressions,
autonomy settings, and the "run until completion" path that Round 1's review
found bypasses the user's denylist: find the exact conditions); how the client
hosts third-party CLI agents (Claude Code, Codex, OpenCode) and orchestrates
them as child agents, including the per-agent inbox or message bus if its code
is here; local-to-cloud hand-off (what state is snapshotted); secret
redaction; the self-hosted worker protocol if any client side of it exists
here (otherwise say which repository holds the worker); MCP client handling;
codebase indexing. Be strict about saying "not in this repository" rather than
inferring server behaviour.

## opencodex

Repository: **opencodex** (`lidge-jun/opencodex`; a provider gateway with a
web dashboard, not an agent harness). Focused scope. Output:
`research/round2/opencodex.md`. Round 1 file: `research/round1/opencodex.md`.

The reader is deciding whether their central server should proxy model calls
for worker machines, and how to keep provider keys off those machines, so this
gateway is studied for exactly that. Settle: the request path of a proxied
model call (accepting Responses, Chat Completions and Anthropic Messages
requests, translation, streaming back): where it buffers, what it costs per
stream, any limits and back-pressure; how provider credentials are stored (at
rest, in memory), how clients authenticate to the gateway, and how account
pools and quota-aware selection work; routing "combos" (the strategies, and
the rule that fail-over happens only before output starts); the bounded loops
the gateway does run itself (the web-search sidecar, the compaction
summariser, the vision call) and how they are bounded; **Remote Workspace**
(experimental): the agent on a hub, an executor on another machine that
connects outbound over WebSocket and runs files and bubblewrap-sandboxed
commands: the pairing with signed handshakes, the message set, how the
bubblewrap invocation is built, what the executor refuses; telemetry (on by
default since v2.100.0: what is sent); the dashboard's authentication.

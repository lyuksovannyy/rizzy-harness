# X.27 step 4: the push tool (design, stage 0)

Status: design and experiments only; nothing here is built yet.
Row: [`X.27.md`](X.27.md). Threat rows it touches: T19, T83, T86, and a new one (proposed T101).
Experiment logs: `.claude/handoff-2026-11-15/b-push/notes.md` (git-ignored; the decisive lines are quoted below).

## 1. Decisions this note starts from

The owner's (not reopened): the agent's own shell never sees the token; the node holds a forge credential in memory only; the agent pushes only through a tool the node runs, behind its own per-agent switch and the approval path; a push asks every time; the chat owner's own connection pushes; node.toml lists the hosts the node clones from and pushes to; tokens are sealed on the master (T83); a host chat gets no extra limits, only an honest sentence about what is weaker.

The session lead's for v1 (the owner may overrule): the destination is only the project's recorded `source` (host and name); the tool pushes one branch, by name, default the branch the folder is on, and exactly the commit the card showed; never a force push, a delete, a tag, more than one ref, `--mirror`/`--all` or a push option; any branch, the default one included; a card every time, refused in plan mode and wherever nobody can answer; no pull request in this change.

## 2. What exists (read at `1b2a3cef`)

**The clone's token path** (`crates/node/src/forge_clone.rs`, T86).
The master sends `project.clone` with host, `owner/name` and, for a private repository, login and token (`wire_secret`).
The node checks the host against `[forge] hosts` exactly as written (`policy.rs` `ForgePolicy::allows`, `valid_forge_host`), the name by `valid_full_name`, login and token by shape, and builds `https://<host>/<owner/name>.git` itself.
The token goes into a close-on-exec pipe that becomes git's standard input; the only credential helper is a shell function that answers `get` only for that scheme, host and path, finds its git by the pipe's inode among at most six parents and reads the token from `/proc/<git>/fd/0` (`helper`, `token_pipe`).
The command is `git::hardened` (cleared environment, `GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL=/dev/null`, `protocol.allow=never`, `credential.helper=` reset, hooks off) plus `clone_config` (one protocol, `http.followRedirects=false`, `http.proxy=`, no bundle URIs, `http.proactiveAuth=basic` from git 2.46), an empty `HOME` of its own, a timeout and a size cap, the process group killed on either; git's error text cut to 300 characters with the token and its Basic form removed (`clean_error`).
All of it is reusable for a push; the helper and the pipe move to a shared module.

**Services the master answers for a node** (PLAN.md §5, the invariant "the master runs no tool").
`NodeToMaster::{Recall, Attached, Docs, Board, Skill, Connector}` each carry `req_id`, `session_id` and the request alone; the master takes the scope from the session (`crates/master/src/lib.rs` around line 1716, e.g. `connectors::call::handle`).
The closest precedent is the connector call (`crates/master/src/connectors/call.rs`, T85): it is answered only when the requesting node runs the session, the open turn is one the master itself started with the owner's own signed-in send (`connectors/grant.rs`: callback turns, API-token, webhook and foreign-marked chats never grant), the open turn's agent has the switch, and per-turn and per-hour counts allow it; the owner's sealed token is opened with the key proven (`Master::connection_token`, `forge.rs`).

**The approval path** (`crates/node/src/session.rs` `approve_from`, about line 6473).
Order: node policy `denied_tools` (denied, recorded); plan mode refuses every tool not on `READ_ONLY_TOOLS`; `mode.needs_approval(risk)` false means allow without a card (so **Auto allows every mutating call unasked**); accept-edits for edits inside the folder; a chat grant (`AllowChat`) with the same call key allows unasked; then the card (`ApprovalRequested` with `turn_allow`, `chat_allow`, `reason`, `preview`).
The one precedent for "one click is one call" is the background start: `turn_allow: false` and `chat_allow: false` (`is_background_start`); but it is still skipped in Auto.
There is no "always ask" today; the push needs one (section 6).

**Per-agent switches** (`crates/agents/src/lib.rs` `Features`): each capability is a field (`recall`, `board`, `office`, `background_subagents` …), off when absent unless stated, turned into tool names by `Features::tools`; built-ins list their tools in `crates/agents/builtin/*.md`; a file agent may say `tools: all` (`ToolSelection::All` allows every name).

**`[forge]` in node.toml** (`policy.rs` `ForgePolicy`): `clone` (default true) and `hosts` (default `github.com`, `gitlab.com`), `deny_unknown_fields`; **`push` is not a key yet**.

**What the master knows of a project's source:** `projects.source_host` and `source_name`, written once at the clone (T86).
It does **not** keep the default branch (the forge's answer at the clone has it, `forge_repos::Repo::default_branch`, but it is not stored) nor the connection used for the clone nor the commit cloned.

**The user repository as files** (`crates/node/src/git/user_repo.rs`, T19): `head_label` reads `HEAD` and one ref (loose, no link followed, bounded; else `packed-refs`, at most 1 MiB); `UserRepo::read` refuses to borrow an object store that is a link or has `info/alternates` or `info/http-alternates`.

## 3. The hard problem: pushing from an agent-controlled `.git`

T19's rule: no git process of the node runs with the user's `.git` as its git directory.
A `git push` run there reads `.git/config`; `-c` on the command line beats a key but cannot remove one, and `url.<x>.insteadOf`/`pushInsteadOf` and `include.path` cannot be unset from the command line at all.

**Experiment E1 (control, git run in the folder):** inotify shows `OPEN folder/.git/config`, `HEAD`, `refs/heads/main`; the hostile `insteadOf` rewrote the URL to the thief server and only the credential helper's host check kept the token back (`fatal: unable to get password from user`).
Ruled out.

Three ways that never use the folder as git's repository were tried with a loopback smart-HTTP server (`git http-backend`, Basic auth, made-up token), a second "thief" server, and a folder whose `.git/config` set `include.path`, `insteadOf`, `pushInsteadOf`, `pushurl`, `receivepack`, `http.proxy`, `http.extraHeader`, `http.sslVerify=false`, `credential.helper`, `core.hooksPath`, `core.sshCommand`, `core.gitProxy`, `core.fsmonitor`, `core.alternateRefsCommand`, `push.pushOption`, `push.followTags`, `push.recurseSubmodules`, `gpg.program`, `uploadpack.packObjectsHook`, plus marker hooks.

**A. Staging repository with `objects/info/alternates` → the folder's `.git/objects` (the brief's proposal).**
E1: pushed, no marker ran, thief untouched, and the only files git opened in the folder were seven loose objects.
**E2 breaks it:** with `folder/.git/objects/info/alternates` naming another repository on the node, the push of that repository's commit succeeded: `* 1ff791c…:refs/heads/leak [new branch]`, git opened `other/.git/objects/{0f,1f,a6}/…`, and the forge now serves `OTHER-REPO-SECRET-CANARY`.
`UserRepo::read`'s check refuses such a store when it looks, but the agent's background command can write the file after the check (the residual T19 accepts for checkpoints, where it only affects the same folder; here it sends another person's or another project's code to a forge with this person's token).
E3: git also parses the borrowed store's `info/commit-graph` and `pack/multi-pack-index` (`fatal: multi-pack-index signature … does not match`; `error: commit-graph signature … does not match` with `OPEN …/objects/info/commit-graph`): agent-written metadata that decides what git believes is reachable.

**B. The same inside a file-system jail** (E9, bubblewrap standing in for the node's `sandbox-exec` helper): the chain cannot be opened (`unable to normalize alternate object path`, `fatal: bad object`), the folder's own commit still pushes.
It closes the chain only where the node has an enforced sandbox (`[sandbox] mode` not `off`, a kernel with Landlock), and git still parses the commit-graph and midx.

**C. Copy (hard-link) the objects into a node-owned staging repository by our own code (chosen).**
The node makes a bare repository in `<data>/projects-staging/push-<random>` (`git init --bare --template= --object-format=<sha1|sha256>`, the format from the pinned id's length) and walks the folder's `.git/objects` itself: only `??/<38 or 62 hex>` and `pack/pack-<hex>.pack` / `.idx`, regular files only (`O_NOFOLLOW`/`lstat`), the directory itself a real directory and not a link (`user_repo`'s `plain`), each hard-linked into the staging repository; nothing else of `objects/` is taken (`info/alternates`, `info/http-alternates`, `info/packs`, `commit-graph`, `multi-pack-index`, `.bitmap`, `.rev`, `.keep`, `.promisor`, `.mtimes`).
Hard links always work for a cloned project: `policy.rs` already refuses a `[projects] dir` that is not on the data directory's file system (the clone's no-replace move needs one).
E2 with C: `fatal: bad object 1ff791c…`, nothing pushed, nothing of the other repository opened.
E3 with C: pushed, nothing in the folder opened by git, thief empty, no marker ran, the gitlink pushed as a tree entry and nothing run for it.
Cost: one `link()` per file (a few files after `git gc`; a store of 100 000 loose objects is 100 000 links: to be measured on the USB disk in the build; a cap on the number of files, with a message to run `git gc`, bounds it).

**Why not fetch from the folder into the staging repository:** `git fetch <folder path>` runs `upload-pack` with the folder as its git directory, which reads the folder's config (`uploadpack.*`, `core.*`) — the T19 rule again.

**What git still reads that the agent wrote, with C:** the bytes of loose objects and of pack and index files (git's parsers; the same residual T19 accepts), and the commit id (read by our code from the refs as files, then given as 40 or 64 hex).
A hard-linked file can be rewritten by the agent while git reads it: the result is a pack the forge refuses or a corrupt object in the agent's own push, nothing else.
The push also runs inside the node's helper jail where the node has one (read-write on the staging repository only, network on: `Sandbox::helper` today passes `--no-network` when the sandbox's network is off, so the push needs a variant that keeps the network), so git cannot open any other file of the node user even if it had a reason to.

**The push command** (E0, E1, E5, E8 with this exact line): in the staging repository, `git::hardened` plus `protocol.https.allow=always` and every other protocol `never`, `http.followRedirects=false`, `http.proxy=`, `credential.useHttpPath=true`, `credential.interactive=false`, `http.proactiveAuth=basic` (git ≥ 2.46), `push.recurseSubmodules=no`, `push.gpgSign=false`, `push.followTags=false`, `push.pushOption=`, `advice.pushUpdateRejected=false`, `core.bigFileThreshold=1m`, `pack.windowMemory=64m`, `pack.deltaCacheSize=16m`, the clone's empty `HOME`, and
`git push --porcelain --no-verify --no-follow-tags --recurse-submodules=no -- https://<host>/<owner/name>.git <40|64 hex>:refs/heads/<branch>`.
The source side is a hex id, never a ref name; the destination is always fully spelled `refs/heads/…`; there is no `+`, `--force`, `--delete`, `--tags`, `--mirror`, `--all`, `-o` or remote name in it, so none of those can be asked for.
- E5: a forge answering 302 to another host: `The requested URL returned error: 302`, the other host got no request.
- E8: `!	a3d7e8c…:refs/heads/feature	[rejected] (non-fast-forward)`, exit 1: reported as the porcelain line (git's `hint:` lines tell the agent to `git pull`, which the tool cannot do: dropped).
- E4: a 200 MB zero blob (about 1 MB on disk) pushed with a peak of 198 MB RSS by git's defaults and 16 MB with `core.bigFileThreshold=1m` and the pack memory limits: those three keys are in the command.
Bounds: `[limits] clone_timeout_s` for the whole push (the process group killed), the stderr read capped at 64 KiB and cut to 300 characters with the token, `login:token` and its base64 forms removed and control and format characters dropped; the staging repository removed afterwards; one push at a time per node; the staging folder is emptied at start already (T86).

## 4. Which commit, which branch

The tool's only argument is `branch` (optional).
Without it, the branch is the one `HEAD` names (`user_repo::head_label`); a detached `HEAD` or an unborn branch gets a plain sentence.
With it, the name must pass `user_repo::branch_ref`'s rule (components of `[A-Za-z0-9-_.+@]`, none empty, `.`/`..`, starting with `.` or ending `.lock`), at most 255 bytes, no `@{`, which is stricter than `git check-ref-format` and leaves no `:`, `*`, `^`, `~`, space or leading `-`; it is resolved as `refs/heads/<branch>` by `user_repo::resolve` (loose file, no link followed, else `packed-refs`).
The local branch and the forge branch have the same name in v1.
The commit read then is the pinned commit: it is on the card, and after the person allows, the ref is read again; if it moved, nothing is pushed and the tool says so (the agent can call it again, and a new card is shown).
The commit must exist in the staging repository after the link step (`git cat-file -t <hex>` in the staging repository says `commit`), else a plain sentence.
`refs/remotes/*` in the folder are agent-written and are not read.

## 5. The token's path

1. **The offer (per turn, no secret).** When the master sends a turn it decides, next to the connectors (`offer_connectors`), whether this turn may push: the chat is a root chat of a project with a `source`; the send is the owner's own signed-in message or Retry (`grant.rs`'s rule, but **not** a routine run: a routine has nobody to answer); the open turn's agent has the push switch, named explicitly in its tool list (`tools: all` does not include it); the owner has a connection to the source host and the host is still allowed by the server's `[[forge.hosts]]`; the node announced `forge-push`.
   Then the top spec carries `push_target: { host, full_name, connection_id, login }` (no secret; the connection id is the row's id, the login the forge's account name the master stored).
   With several connections to that host the newest is used and the card names its login (a choice in the UI can follow).
2. **The node offers `push`** only when `push_target` is there, `[forge] push` is on and the host is in `[forge] hosts` (else the tool is absent, and the system prompt says nothing), the session has no parent (not a sub-agent), it is not a routine's chat (`no_callbacks`), the turn is not a callback turn (the open turn's `callback` flag in `session.rs`), and the folder is a plain repository (`user_repo::find`).
   A call in any other state is refused with one plain sentence; the model never receives the target's connection id.
3. **The card** (section 6), **before any network call** and before the token is asked for.
4. **The request.** After the person's Allow, the node re-reads the ref (section 4) and sends `forge.push_credential { req_id, session_id, approval_id }`.
5. **The master answers** only when all of these hold, else `error` and no token: the requesting node runs the session; the session is a root chat whose open turn is granted (`grant.rs`) and not a callback turn; **the master itself relayed an Allow for this `approval_id` in this session from the chat's owner** (recorded in `respond_approval` when the mirror shows that `approval_requested` was for the tool `push`; each such approval is used once and expires after 10 minutes; a "for this turn/chat" answer is never offered and is not accepted); the session's project still has a `source`, the connection `push_target` named is still the owner's and still for that host, and the host still allowed; the key is proven (`connection_token`); one request in flight per session and at most 20 pushes per chat per hour.
   Then `forge.push_credential_result { req_id, username, token }` (`wire_secret`), never logged, never an event, no audit line holding it; the master writes one audit line "push credential issued" (user, connection id, session, host, name; no token).
6. **The node** checks the token's shape (`valid_token`), gives it to one git process through the pipe and helper of section 3, and drops it (`Secret`, unwiped memory as T86 says).

**Attacks, and what stops each.**
- A hostile node asks for a session it does not run: the owner check (as every service).
- A hostile node asks for a session it runs with no click: refused; the click is recorded by the master, not reported by the node (this is stronger than the connector call, where the node's log is trusted for the turn).
- A node that runs the session and is compromised can still show a misleading card and then use one real click for its own push: the token it gets is the one it would get for the honest push; the master's card rendering of host and name from its own `source` (section 6) limits what the card can lie about.
- A host not in the project's source: the request names no host; the master takes it from the project.
- A prompt-injected agent calling the tool in a loop: every call is a card (no grant can cover it), a card without an answer allows nothing, 20 per chat per hour on the master.
- Another member's chat in the same workspace project: their own connection pushes, or none; chats and connections are private.
- An admin's session: the admin's own connection only; never another person's.
- The token in an event, a log line, an audit row, an error text, argv, the environment or `/proc/<pid>/cmdline`: never written there (the pipe); git's errors scrubbed as the clone's; the master's result frame is a `wire_secret`; tests canary every one of these (section 9).
- The forge echoing a URL with credentials: the URL never has one; the scrub removes the token in all its forms anyway.
- Redirects: off (E5).
- The source host resolving to a private address: the node connects to what its node.toml lists, as the clone does (T86 does no address check on the node either; the master's check is for its own API calls); TLS with the system's roots, `http.sslVerify` and `sslCAInfo` cannot be changed (no repository config, no `GIT_SSL_*` in the cleared environment).
- The branch name: section 4's rule; hostile branch names are a test.

## 6. The card

**Every time.** A new risk kind, `ToolRisk::External` ("acts as the person on another service"), for which `ApprovalMode::needs_approval` is true in every mode but plan (plan refuses the tool: it is not read-only); `approve_from` skips the chat-grant lookup for it and sends `turn_allow: false`, `chat_allow: false`; a `"for this turn"`/`"for this chat"` answer is taken as a single allow (as today for a background start); node policy `denied_tools = ["push"]` refuses it.
This is the first tool that asks in Auto; Auto's promise ("never asks") changes, and README must say so in the Auto paragraph.

**What it shows**, all known without the network: `reason` = "push to `<host>/<owner/name>`, branch `<branch>`, as `<login>`" (host, name and login from `push_target`, i.e. from the master); `preview` = the pinned commit's short id and subject (agent-written text, at most one line of 200 characters, control and bidirectional characters removed, rendered as inert text like every preview) and, when the branch is the folder's current one, nothing more.
The commit's subject is read in the staging repository (`git cat-file commit <hex>`, bounded) after the link step, which needs no network.
**Not shown:** "N commits ahead": the only local reference point is `refs/remotes/origin/<branch>`, which the agent writes; a number the agent can fake does not belong on a security card.
After the push, the result says what the forge did, from git's porcelain line, which is the server's answer: `[new branch]`, `old..new`, `[up to date]` or the rejection.
**The default branch** cannot be marked today: the master did not store it at the clone (section 2).
Recommendation: a migration (number 7) adding `projects.source_branch`, written at the clone from the forge's answer it already has, labelled "the default branch when the project was cloned"; in a second pull request.
**In the UI:** the existing approval card with the push's own icon and the destination drawn from the master's `source`, not from the node's `reason` (the master knows the session's project); no new `ui:` block.

## 7. A host chat

In a host chat the agent's shell is the node's user, unconfined.
It can run its own `git push` with whatever the machine holds (ssh keys, a credential store, `gh`), which the tool neither stops nor sees.
While the tool's git runs, a background command of that chat can read `/proc/<git>/fd/0` and take the connection's token (T86's residual; a sandboxed shell cannot, since Landlock denies ptrace access to processes outside its domain, which the build stage proves with a test).
It can also write the node's staging folder.
So in a host chat the tool adds a card, a fixed destination and the token never being given to the shell on purpose; it does not keep the token from a hostile agent there.
README says this in one paragraph; nothing is refused for it (the owner's decision of 2026-11-20).

## 8. The other harnesses

Claude Code's cloud sessions (research `round1/claude-code.md`, "GitHub proxy"): the credential never enters the VM, a proxy swaps a scoped credential for the real token, refuses branch deletion and tag pushes, does not limit branches and limits the API to the session's repositories.
Its auto mode pushes to any branch of the working repository without asking (deploy-named branches judged apart; "Pushes and PRs run without prompt unless blocked").
Codex refuses project configuration that would redirect credentials (`round1/codex.md`, appendix "Config key blocklist").
Ours copies the proxy's shape (the token outside the agent's reach, branch pushes only, no tags or deletes, one repository) without a TLS proxy, and asks every time where Claude Code does not.
What we skip: a git proxy that lets the agent's own `git push` work (it needs TLS termination or a credential placeholder; the tool is smaller and fits "the node decides").

## 9. Work plan

**core:** `forge::PUSH_TOOL = "push"`, `PushTarget`, `AgentSpec::push_target` (serde default, skipped when empty); `ToolRisk::External` and its rule in `ApprovalMode::needs_approval`; not on `READ_ONLY_TOOLS`.
**protocol (additive):** `CAP_FORGE_PUSH = "forge-push"`; `NodeToMaster::PushCredential { req_id, session_id, approval_id }`; `MasterToNode::PushCredentialResult { req_id, username?, token? (wire_secret), error? }`; a redaction test like `project_clone_carries_names_and_a_redacted_token`.
**node:** `[forge] push` (bool, see open question 1); the capability; `forge_git.rs` (the pipe, the helper, the scrub and the bounds moved out of `forge_clone.rs`); `forge_push.rs` (branch rule, ref read, staging and link, the jail with network, the push, the result text in `[harness]` lines with the forge's words quoted by `quoted_line`); the tool in the session's tool set with the offer rules of §5.2; `approve_from`'s `External` path; an audit line (host, name, branch, commit, result).
**master:** `forge_push.rs` (the offer next to `offer_connectors`, the request handler, the approval record in `respond_approval`, limits, audit line); README; no HTTP route.
**agents:** `Features::push` ("Push to the project's repository"), off when absent, its tool only by explicit name; which built-ins get it is open question 2.
**web:** the switch on the Agents page (an existing lazy chunk); the push card's icon and destination line in `ToolCard`/the approval card; the README paragraph.
**docs:** README "Pushing to the repository" (what is pushed, the card every time, Auto, host chats); `X.27.md` step 4; a new threat row (T101, "a person's forge token used for a push"), T86's residual line about pushes updated; `PLAN.md` index line; `ADOPTION_PLAN.md` index unchanged unless X.27's first sentence changes.
**No schema change** in the first pull request.

**Tests (named; each fails on today's code by not existing or by the behaviour it names):**
- node: `push_reads_no_file_of_the_folder_but_objects` (hostile config, include, hooks, insteadOf, proxy, helper, receivepack: markers absent, thief empty), `an_alternates_chain_never_reaches_another_repository` (E2: `bad object`, the canary not on the forge), `commit_graph_midx_promisor_and_http_alternates_are_not_copied`, `a_gitlink_and_gitmodules_run_nothing`, `a_redirect_is_not_followed_and_the_other_host_gets_nothing`, `a_rejected_push_is_reported_in_the_forges_words_without_hints`, `the_token_is_in_no_argv_environ_event_log_or_file` (a stalled push's `/proc`, TRACE logs, the data directory), `a_moved_branch_is_not_pushed`, `a_hostile_branch_name_is_refused` (`:`, `*`, `..`, `@{`, leading `-`, `+main`, 4 000 bytes), `a_detached_or_unborn_head_says_so`, `a_bomb_blob_stays_under_the_memory_bound`, `the_push_times_out_and_kills_its_group`, `push_is_not_offered_to_a_sub_agent_a_routine_or_a_callback_turn`, `push_asks_in_auto_and_offers_no_grant`, `plan_mode_refuses_push`, `node_policy_without_push_offers_nothing_and_announces_nothing`.
- master: `a_push_credential_needs_a_click_the_master_relayed` (no click; a click for another tool; a second use; an expired click), `another_node_gets_no_credential`, `another_members_and_an_admins_session_get_their_own_or_none`, `a_routine_api_token_webhook_or_callback_turn_gets_no_target`, `a_removed_host_or_connection_or_source_refuses`, `twenty_one_pushes_in_an_hour_refuse_the_last`, `the_token_is_in_no_frame_log_audit_or_answer_but_the_result`.
- agents: `push_is_its_own_switch_and_not_in_all`.
- web: the card shows the destination from the master and no "allow for this chat/turn".
- e2e: `web/e2e/push.cjs` (turn the switch on, a push card, Allow, the fake forge has the branch).

**Size:** L. Node about 600 lines plus 700 of tests, master about 300 plus 400, core and protocol about 120, web about 120, docs about 150.
**Split:** PR 1 = core, protocol, node, master, agents switch (backend), README, threat row; PR 2 = the Agents page switch and the card's polish in the UI, `push.cjs`, and the default-branch migration (7) if the owner wants the marker.

## 10. Open questions

1. `[forge] push` default: **on** (same `hosts` list as the clone; each push is still a card and needs the agent's switch), or off until a node owner turns it on? Recommendation: on, matching `clone = true` and "usable for coding first".
2. Which built-ins get the push switch: recommendation **Software Developer and Senior Teamlead** (they already commit through `bash`), the others off.
3. The default-branch marker needs migration 7 (`projects.source_branch`); recommendation: yes, in PR 2.
4. Several connections to one host: the newest pushes and the card names it; a picker later. Recommendation: accept for v1.
5. After a rejected push the agent cannot fetch the forge's branch with the token. A `fetch` tool (same mechanics, into `refs/remotes/<source>/…` of a node-owned repository, then objects linked back) is the natural next step; recommendation: a separate row, not this change.
6. Auto mode asking for the first time: the README must say "Auto never asks, except before a push". Recommendation: accept (the owner's "asks every time").

# Pushing a project's branch to its forge (X.27 step 4)

Status: built (branch `feat/push-tool`, 2026-11-24).
Row: [`plan/X.27.md`](plan/X.27.md). Threat row: [T101](threats/T101.md); it builds on T19 (no git process in the user's repository), T83 (sealed connections) and T86 (the clone's token path).
Experiment logs of stage 0: `.claude/handoff-2026-11-15/b-push/notes.md` (git-ignored; the decisive lines are quoted here).

## 1. Decisions

The owner's: the agent's own shell never sees the token; the node holds a forge credential in memory only; the agent pushes only through a tool the node runs, behind its own per-agent switch and the approval path; a push asks every time; the chat owner's own connection pushes; node.toml lists the hosts the node clones from and pushes to; tokens are sealed on the master (T83); a host chat gets no extra limits, only an honest sentence about what is weaker.

The session lead's (2026-11-24; each may be overruled by the owner):
1. The destination is only the project's recorded `source` (host and `owner/name`). The tool takes no URL, no remote and no repository name. A project without a `source` (a hand-made folder) is never offered the tool.
2. One branch, by name, default the branch the folder is on; it goes to the forge branch of the same name; exactly the commit the card showed.
3. Never a force push, a delete, a tag, more than one ref, `--mirror`/`--all` or a push option. A rejection is reported in the forge's words.
4. Any branch, the default one included. The card shows the branch name plainly; it does not mark the default branch (the master did not record it at the clone; recording it is a later change).
5. A card every time, in every approval mode, Auto included; no "for this turn" or "for this chat"; refused in plan mode; never where nobody can answer (routines, callback turns, sub-agents).
6. No pull request, no fetch: later changes.
7. `[forge] push` is on by default for the hosts in `[forge] hosts`, like `clone`; `push = false` turns it off.
8. The agent switch "Push to the repository" is on for the built-in Software Developer and Senior Teamlead, off for every other built-in; a custom or file agent gets the tool only by naming `push`, never through `tools: all`.
9. With several connections to the source host, the newest pushes and the card names its login.

## 2. What existed

The clone's token path (`crates/node/src/forge_clone.rs`, T86): a close-on-exec pipe at git's standard input, a shell credential helper that answers only that scheme, host and path and finds its git by the pipe's inode, the hardened git base (`crates/node/src/git.rs`), `http.followRedirects=false`, an empty home, a scrub of git's errors.
The services the master answers for a node (`NodeToMaster::{Recall, Board, Connector, …}`), with the scope taken from the session alone.
The approval path (`session.rs` `approve_from`), in which Auto allowed every mutating call without a card.
Per-agent switches (`crates/agents/src/lib.rs` `Features`).
The project's `source` on the master (`projects.source_host`, `source_name`).

## 3. The hard problem: pushing from an agent-controlled `.git`

A `git push` run in the folder reads its `.git/config`; `-c` beats a key but cannot remove `url.<x>.insteadOf`, `pushInsteadOf` or `include.path`.
Stage 0's control run (git in the folder) opened `.git/config`, and the hostile `insteadOf` sent the push to the thief server; only the helper's host check kept the token back.

The brief's first proposal, a node-made bare repository whose `objects/info/alternates` names the folder's object store, failed experiment E2: with the folder's own `objects/info/alternates` chained to another repository on the node, the push of that repository's commit succeeded and the forge then served `OTHER-REPO-SECRET-CANARY`. The check that refuses such a store when the node looks is racy (a background command can write the file afterwards). git also parsed the borrowed store's `commit-graph` and `multi-pack-index` (E3).

**Built: copy by hard link** (`crates/node/src/forge_push.rs`).
The node makes a bare repository in `<data>/projects-staging/push-<random>` by writing `HEAD`, `config` and the folders itself (sha1 or sha256 from the pinned id's length), and walks the folder's repository **by descriptor** (review F1): the chat's folder is opened once with `O_DIRECTORY | O_NOFOLLOW`, checked to be the session's pinned directory (device and inode of `FolderPin`), and held for the whole push, both walks and both reads of the ref; `.git`, `objects`, each `??` folder and `pack` are opened with `openat(…, O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)` relative to the one before and listed through their own descriptors; only `??/<hex>` and `pack/pack-<hex>.{pack,idx}` are taken, each checked with `fstatat(…, AT_SYMLINK_NOFOLLOW)` to be a regular file, linked with `linkat` (no follow) into the node's folder, and checked again through the node's side to be a regular file of the same device and inode as the one checked.
A folder that is a link where a directory was (a `.git`, `objects`, `pack`, `??` or ref folder swapped in mid-walk) refuses the push; a file swapped for a link between its check and its link is caught by the check after the link.
The first version checked `.git/objects` once by path and resolved every path again; the review won that race and pushed another repository's commit, and the deterministic race tests (`a_swapped_*_is_not_followed`) failed on it for `.git`, `objects`, `pack` and a `??` folder.
The only path resolved is the folder's own, once, and the descriptor opened must be the pinned inode, so a parent folder swapped before that open refuses the push.
Nothing else of `objects/` is taken: no `info/alternates`, `info/http-alternates`, `info/packs`, `commit-graph`, `multi-pack-index`, `.bitmap`, `.rev`, `.keep`, `.promisor`.
Where a link is refused (another owner's files under `fs.protected_hardlinks`, another file system), the file is copied instead: opened with `openat(…, O_NOFOLLOW | O_NONBLOCK)` from its folder's descriptor, a regular file of the same device and inode as checked on the opened descriptor, its size from that descriptor, the copies together within `[limits] clone_max_mb`.
A cloned project is always on the staging folder's file system (`policy.rs` refuses a `[projects] dir` elsewhere), so links are the normal case.
More than 200,000 listed entries in the object store (objects or not: every name `names()` reads counts, review N2), or a walk longer than 120 s (the clock read at every entry), is refused with the advice to run `git gc` in the folder; a pack folder of 300,000 junk names is refused after 68 ms, the test process's peak memory going from 17 MB to 33 MB.
Measured on this machine's USB disk (`measure_the_link_step`, run by hand): 100,102 loose objects linked in 1.4 s and removed in 0.2 s; the copy fallback over the same store was refused at the 120 s bound (the check then ran every 1,000 files, so it ended at 135 s; it now runs every 100); a store of 2.2 GB in four objects linked in 0.3 ms, and its copy was refused at `clone_max_mb` (2048) after 35 s.

**What git still reads that the agent wrote:** the bytes of the loose objects and packs (git's parsers, the residual T19 accepts), and the commit id, which the node read from the refs as files and passes as 40 or 64 hex.
**A linked file the agent rewrites** after the link shares its inode: git reads the new bytes. The forge names every object by the hash of what it received (`index-pack`) and refuses a ref update whose objects it cannot all reach (receive-pack's connectivity check), so the rewrite can only make the push fail; the forge never gets the pinned id with other content, nor another commit (`a_rewritten_object_only_fails_the_push`, against `git http-backend`). A forge that skipped those checks would be a broken git server; that is what this relies on.

**The command**, in the node's repository, through the node's helper jail where the node has one (`Sandbox::helper_net`: read-write on the push's own folder and the system folders only, the network open whatever the agent's sandbox says):
the hardened base plus `protocol.https.allow=always` and every other protocol `never`, `http.followRedirects=false`, `http.proxy=`, `credential.useHttpPath=true`, `credential.interactive=false`, `http.proactiveAuth=basic` (git 2.46 and later), `push.recurseSubmodules=no`, `push.gpgSign=false`, `push.followTags=false`, `push.pushOption=`, `push.negotiate=false`, the push `advice.*` keys off, `core.bigFileThreshold=1m`, `pack.windowMemory=64m`, `pack.deltaCacheSize=16m`, `pack.threads=2`, `transfer.bundleURI=false`, the push's own empty home, `GIT_NO_REPLACE_OBJECTS=1`, and
`git --git-dir <node repository> push --porcelain --no-verify --no-follow-tags --recurse-submodules=no -- https://<host>/<owner/name>.git <hex>:refs/heads/<branch>`.
No `+`, `--force`, `--delete`, `--tags`, `--mirror`, `--all`, `-o` or remote name can be asked for.
A 200 MB zero blob (about 1 MB on disk) peaked at 198 MB of the push's memory with git's defaults and 16 MB with the three memory keys (stage 0, E4).
Bounds: `[limits] clone_timeout_s` for the whole push (the process group killed), git's output read to 64 KiB, its error cut to 300 characters; one push at a time on a node.
git's text goes through the shell's terminal-escape stripper (`harness_tools::filter::AnsiStripper`), then loses the token, `login:token` and its base64 forms and every `hint:` line, then the node's secret scrubber.

**What the walk stops, and what it does not.** It stops the node from following a link: a folder or file of the repository swapped for, or planted as, a link out of the chat's folder is never followed, so the node never reads another repository through the folder. It does not stop the agent from putting another repository's objects into the folder itself, as a copy or as a hard link where the agent may make one: those are the folder's files, and they are pushed, to the project's own source only. So any repository content the agent can already read can be pushed there.
Who can hard-link what: a host chat runs as the node's user and can hard-link any file of that user on the same file system (`fs.protected_hardlinks` lets a user link files they own or may read and write), or copy any file it can read; a sandbox chat can link only within what its sandbox lets it write, since Landlock gives the right to link across folders (`REFER`) only with read-write grants (`crates/tools/src/sandbox.rs`, read from the code) and the containment shows no file outside its grants; `a_sandboxed_shell_cannot_hard_link_outside_files_into_its_folder` (`crates/cli/tests/push_token_reach.rs`) runs it, with Landlock alone and contained: linking the data directory's token or another folder's file into the chat's folder fails, while the same link unconfined succeeds.

## 4. Which commit, which branch

`branch` is the tool's only argument (`harness_core::forge::valid_branch`: 1 to 255 bytes of `/`-separated parts of `[A-Za-z0-9._+@-]`, none empty, starting with `.`, `-` or `+`, ending in `.lock` or `.`, holding `..` or `@{`; stricter than `git check-ref-format`).
Without it, the branch `HEAD` names (`user_repo::head_label`); a detached `HEAD` or an unborn branch is a plain sentence.
The commit is read through the same folder descriptor: `HEAD`, each component of `refs/heads/<branch>` (`O_DIRECTORY | O_NOFOLLOW`; a component that is a link refuses) and the ref file (`O_NOFOLLOW | O_NONBLOCK`, a regular file, bounded), else `packed-refs` the same way.
`refs/remotes/*` in the folder are agent-written and never read.

## 5. Order and the token's path

1. **The offer** (master, `crates/master/src/forge_push.rs` `offer`, per turn it sends): only a project chat's own turn its owner sent signed in as themselves; not the API token's, a routine run's, a webhook's (untrusted) or a callback turn; not a sub-agent's, a temporary, a forked or a marked (foreign) chat; not parallel answers or a list of called agents; the turn's agent names `push`; the project's source host is still allowed on the server; the owner has a connection for it; the node announced `forge-push`. The target (`host`, `full_name`, `connection_id`, `login`; no secret) goes in the turn's `MasterContext::push` and is kept for the chat until the next turn replaces or clears it.
2. **The node keeps it** (`session_push.rs` `push_for_turn`) only for an agentic chat of its own that is not a sub-agent's or a routine's, in a turn that is not a callback, where node.toml pushes to that host and has a staging folder; otherwise `push` is added to the turn's denied tools and the model is never offered it. A turn that only reads queued messages gets none.
3. **Before the card:** the commit from the refs; a first node repository of links; the commit's subject from it (`git cat-file commit`, one line, cleaned, 200 characters).
4. **The card** (§6).
5. **After the Allow:** the ref read again (a different commit refuses: "the branch moved after the card was shown"); the objects listed and linked again into a fresh node repository; the commit checked to be there.
6. **The request:** `forge.push_credential { req_id, session_id, approval_id }`.
7. **The master answers** with the token only when: the requesting node runs the session; the session is a top-level, non-temporary chat; no other request of the chat is being answered; **the master itself relayed an Allow of that very card from the chat's owner signed in** (recorded by the approval route before it relays the answer, when the mirror's `approval_requested` of that id names the tool `push`), used once, within ten minutes; the chat's target is kept and its owner unchanged; the open turn is not a callback and does not withhold `push`, and its agent has the push switch; no one else acted in the chat; the owner's account exists, is not disabled, and their level still lets the chat run (`agentic_blocked`); the project's source is still the target; the host is still allowed; the connection still the owner's and for that host; fewer than 20 credentials in the chat's last hour; the key proven. Then `forge.push_credential_result { username, token }` (a redacted secret on the wire) and one audit line `forge.push_credential` (user, session, connection, host, name; never the token).
8. **The node** gives the token to the one `git push` through the pipe and helper and drops it; one audit line `forge.push` (session, host, name, branch, commit, linked and copied counts, the result).

What this binding buys: a node, even one that runs the session, gets no token without a person's real click on a card the page drew as a push card of that chat: the master takes a click only when the approval id appears on exactly one `approval_requested` in the chat's log, its call id on exactly one `tool_call_started`, both name `push`, and the card is not answered yet (review F2, N1: a reused approval id or a call announced twice could make the card the page shows stand for another), and the same check runs again when the credential is asked for; an approval id or call id outside the harness's id rule (1 to 128 of `[A-Za-z0-9_-]`, `harness_core::valid_event_id`) names no card (review N4: an id such as `u1#x` beside a real `u1` was sent by the page unencoded and reached `u1`): the page encodes every id it puts into an `/api/` path, draws such a card invalid, and the approval route answers 404 for such an id before relaying anything; and each approval id gets one token, ever (F3). The page draws a push card's destination from the master (`GET /api/sessions/{id}/push-target`), never from the node's text, and draws a card as invalid, Deny only, whatever its tool, when its approval names another tool than its call or when its approval id or call id was announced more than once (the Rust view and `fold.ts` alike).
**What a compromised node gets** that runs the session: a bearer token of the connection, as wide as the scope the person gave it on the forge (every repository and API that token reaches), for one real click on a push card; never another person's. It can show a card of its own making apart from the destination line (the branch, commit and subject are the folder's, which the node reads).

## 6. The card

`ToolRisk::External`: `ApprovalMode::needs_approval` is true in every mode but plan (which refuses the tool). The loop asks the host nothing for such a tool: the tool asks for its own card while it runs (`Session::ask_external`), once it can show what it will do. The card offers no "for this turn" or "for this chat" (`turn_allow` and `chat_allow` false), and any allow answer counts as this one call; node.toml's `denied_tools` refuses it.
Each part of the card and where it comes from:

- **Destination line**, from the master: "Pushes to `<host>/<owner/name>` as `<login>` (as the server knows it)".
- **"From the folder (the agent's repository)"**: the approval's `preview`, written by the node from the folder before the card: `branch:` and `commit: <12 hex> <subject>`, plain text. It is the only node-written part drawn, because only the node can read the folder; what it says is bound as below.
- **Not drawn**: the approval's `reason` ("push branch `<branch>` to `<host>/<owner/name>` as `<login>`", the node's text; review R6 of the pull request tool): it stays in the event for the log, the page does not show it on a forge card.
- **Collapsed text**: if the preview is long enough to be cut, Allow is replaced by "Show all to allow" until it was shown once, as on the pull request's card (`docs/forge-pr.md` §4).

A card without a target from the master, or whose approval names another tool than its call, offers only Deny.
Not shown: how many commits are ahead (the only local reference is agent-written), whether the branch is the default one (not recorded).
**What on the card is bound to what:** the destination line (host, `owner/name`, login) is the master's, from the project's recorded source and the connection it named for the turn; the branch and the commit id are the folder's refs as the node read them before the card, read again after the Allow and pushed only if unchanged; the subject is shown only when the commit's bytes hash to that id (`git hash-object` over what `cat-file` returned; review F6), otherwise "(subject not shown: the commit could not be verified)"; what is pushed is the commit with that id, which the forge checks by hash.
The result is the forge's porcelain line: `[new branch]`, `old..new`, `[up to date]` or the rejection, quoted line by line under `[harness]` lines.

## 7. What is weaker in a host chat, and on a shared node

In a host chat the agent's shell is the node's user, unconfined. It can run its own `git push` with whatever the machine holds (ssh keys, a credential store, `gh`), which the tool neither stops nor sees.
**Any** host chat on a node, of any person, runs as the node's user: while any push runs on that node, another person's push from a sandbox chat included, it can read the token from the push's git: from the pipe at `/proc/<git>/fd/0` until the helper has read it (a short window), and after that from the memory of `git` and `git-remote-https` (`/proc/<pid>/mem`, same-user ptrace rules). It can also read the node's own key and data directory, so it is a compromised node (§5). So on a node that allows host mode for more than one person, the push tool is only as private as those people trust each other: turn pushing off there (`[forge] push = false`) or offer no host mode on a shared node.
A sandboxed shell cannot read the token (`a_sandboxed_shell_cannot_read_the_pushs_token`: Landlock alone and inside the containment). The push's git runs with a core-file limit of 0, so no core dump holds the token.
So in a host chat the tool adds the card, the fixed destination and the token never being handed to the shell; it does not keep the token from a hostile agent there.

## 8. The other harnesses

Claude Code's cloud sessions (research `round1/claude-code.md`, "GitHub proxy"): the credential never enters the VM; a proxy swaps a scoped credential for the real token, refuses branch deletion and tag pushes, does not limit branches, and limits the API to the session's repositories. Its auto mode pushes to any branch of the working repository without asking (deploy-named branches judged apart).
Codex refuses project configuration that would redirect credentials (`round1/codex.md`).
Ours keeps the proxy's shape (the token outside the agent's reach, branch pushes only, no tags or deletes, one repository) without a TLS proxy, and asks every time where Claude Code does not.
Not copied: a git proxy that lets the agent's own `git push` work (it needs TLS termination or a credential placeholder).

## 9. Where it is

| Part | Files |
|---|---|
| Types | `crates/core/src/forge.rs` (`PushTarget`, `PushHost`, `valid_branch`), `policy.rs` (`ToolRisk::External`), `agent.rs` (`push` never through `all`), `backend.rs` (`MasterContext::push`, `TurnExtra::push`, `TurnHost::push`) |
| Link | `crates/protocol/src/lib.rs`: `CAP_FORGE_PUSH`, `forge.push_credential`, `forge.push_credential_result` |
| Tool | `crates/tools/src/push.rs`; the loop's External rule in `crates/agent-native/src/lib.rs` `authorize` |
| Node | `crates/node/src/forge_push.rs` (mechanics), `session_push.rs` (offer, card, order, credential), `policy.rs` (`[forge] push`), the folder walk by descriptor in `forge_push.rs` |
| Master | `crates/master/src/forge_push.rs`, the approval route in `http.rs`, the offer in `Master::prompt`; `crates/store` `approval_tool` |
| Switch | `crates/agents/src/lib.rs` (`Features::push`), `builtin/coder.md`, `teamlead.md`; `web/src/components/AgentPage.tsx` |
| Card | `web/src/components/ApprovalChoice.tsx` (the label) |

Tests: `crates/node/src/forge_push_tests.rs` (mechanics), `forge_push_runtime_tests.rs` (through the node's runtime), `crates/master/tests/logs/forge_push_http.rs`, `crates/cli/tests/push_token_reach.rs`, `crates/agent-native/src/plan_tests.rs` (`an_external_tool_is_asked_by_the_tool_never_by_the_loop`), `crates/agents/tests/agents.rs` (`pushing_is_its_own_switch_and_never_comes_with_all`), `crates/core` (`forge`, `policy`, `agent`), `crates/protocol`, `web/src/components/approvalpreview.test.tsx`.

## 10. Left for later

Opening a pull request (a second tool, on the master's side); a fetch of the forge's branch after a rejection; recording and marking the default branch (a migration); binding a hand-made folder to a forge repository; a picker among several connections to one host; the master drawing the card's destination from its own data rather than the node's text.

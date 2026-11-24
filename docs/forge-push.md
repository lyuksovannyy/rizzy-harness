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
The node makes a bare repository in `<data>/projects-staging/push-<random>` by writing `HEAD`, `config` and the folders itself (sha1 or sha256 from the pinned id's length), and walks the folder's `.git/objects` with its own code: only `??/<hex>` and `pack/pack-<hex>.{pack,idx}`, regular files by `lstat`, the store a real directory with no link on its path; each file hard-linked, and the link checked to be a regular file on the same device afterwards (a symbolic link swapped in between is refused, never followed).
Nothing else of `objects/` is taken: no `info/alternates`, `info/http-alternates`, `info/packs`, `commit-graph`, `multi-pack-index`, `.bitmap`, `.rev`, `.keep`, `.promisor`.
Where a link is refused (another owner's files under `fs.protected_hardlinks`, another file system), the file is copied instead: opened with `O_NOFOLLOW | O_NONBLOCK`, a regular file on the descriptor, the copies together within `[limits] clone_max_mb`.
A cloned project is always on the staging folder's file system (`policy.rs` refuses a `[projects] dir` elsewhere), so links are the normal case.
More than 200,000 object files, or a walk longer than 120 s, is refused with the advice to run `git gc` in the folder.
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

## 4. Which commit, which branch

`branch` is the tool's only argument (`harness_core::forge::valid_branch`: 1 to 255 bytes of `/`-separated parts of `[A-Za-z0-9._+@-]`, none empty, starting with `.`, `-` or `+`, ending in `.lock` or `.`, holding `..` or `@{`; stricter than `git check-ref-format`).
Without it, the branch `HEAD` names (`user_repo::head_label`); a detached `HEAD` or an unborn branch is a plain sentence.
The commit is read by `user_repo::branch_commit` (loose ref with no link followed, else `packed-refs`).
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

What this binding buys: a node, even one that runs the session, gets no token without a person's real click on a push card of that chat. A compromised node that runs the session can still show a misleading card text and spend one real click on a push of its own choosing to the same repository (T101's residual).

## 6. The card

`ToolRisk::External`: `ApprovalMode::needs_approval` is true in every mode but plan (which refuses the tool). The loop asks the host nothing for such a tool: the tool asks for its own card while it runs (`Session::ask_external`), once it can show what it will do. The card offers no "for this turn" or "for this chat" (`turn_allow` and `chat_allow` false), and any allow answer counts as this one call; node.toml's `denied_tools` refuses it.
`reason`: "push branch `<branch>` to `<host>/<owner/name>` as `<login>`"; `preview` (shown as plain text under "What will be pushed"): `repository:`, `branch:`, `as:` and `commit: <12 hex> <subject>`. Host, name and login are the master's target; the subject is the agent's text, inert.
Not shown: how many commits are ahead (the only local reference is agent-written), whether the branch is the default one (not recorded).
The result is the forge's porcelain line: `[new branch]`, `old..new`, `[up to date]` or the rejection, quoted line by line under `[harness]` lines.

## 7. What is weaker in a host chat

In a host chat the agent's shell is the node's user, unconfined. It can run its own `git push` with whatever the machine holds (ssh keys, a credential store, `gh`), which the tool neither stops nor sees. While the tool's git runs, a background command of that chat can read `/proc/<git>/fd/0` and take the connection's token (`push_token_reach.rs`'s calibration does exactly that unconfined). It can also write the node's staging folder. So in a host chat the tool adds the card, the fixed destination and the token never being handed to the shell; it does not keep the token from a hostile agent there.
A sandboxed shell cannot read the token (`a_sandboxed_shell_cannot_read_the_pushs_token`: Landlock alone and inside the containment).

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
| Node | `crates/node/src/forge_push.rs` (mechanics), `session_push.rs` (offer, card, order, credential), `policy.rs` (`[forge] push`), `git/user_repo.rs` (`branch_commit`) |
| Master | `crates/master/src/forge_push.rs`, the approval route in `http.rs`, the offer in `Master::prompt`; `crates/store` `approval_tool` |
| Switch | `crates/agents/src/lib.rs` (`Features::push`), `builtin/coder.md`, `teamlead.md`; `web/src/components/AgentPage.tsx` |
| Card | `web/src/components/ApprovalChoice.tsx` (the label) |

Tests: `crates/node/src/forge_push_tests.rs` (mechanics), `forge_push_runtime_tests.rs` (through the node's runtime), `crates/master/tests/logs/forge_push_http.rs`, `crates/cli/tests/push_token_reach.rs`, `crates/agent-native/src/plan_tests.rs` (`an_external_tool_is_asked_by_the_tool_never_by_the_loop`), `crates/agents/tests/agents.rs` (`pushing_is_its_own_switch_and_never_comes_with_all`), `crates/core` (`forge`, `policy`, `agent`), `crates/protocol`, `web/src/components/approvalpreview.test.tsx`.

## 10. Left for later

Opening a pull request (a second tool, on the master's side); a fetch of the forge's branch after a rejection; recording and marking the default branch (a migration); binding a hand-made folder to a forge repository; a picker among several connections to one host; the master drawing the card's destination from its own data rather than the node's text.

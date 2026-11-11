# Gate 3 fix review 6, 2026-11-11: the remaining findings, and the gate

Last read. Reviewed: `fix/R.4-remaining-findings` (pull request 44; fix commits
`eb670f6`, `9053622`, `1e3b702`, `59cbd26`, `7e13faa`, `0fd64f4`, `2e949ee`,
`9972b6d`, `d9232d6`). Read detached, no product change.

## 1. Verdict

**R.4 is fit to merge.** No finding of Medium or above. Two Low, three Notes.

| Item | Result |
|---|---|
| R2-1 (cookie alone takes socket places, shows liveness) | Closed, with one Low residual the author states (R4-2). |
| R2-3 (logout without the key) | Closed. |
| R1d-1 (sandbox extras marked `system`) | Closed while the node runs; one Low gap across a restart (R4-1). |
| P-6, R2-2, L-7, S-5, N-3, N-4, N-5, the routine race | Closed (by reading). |

## 2. New findings

- **R4-1. Low, proven by test.** A `[sandbox]` extra is bound to where its path
  resolves when the node starts (`Extra::new`, `real(p)`), and its identity is
  taken at each command, not at start as the description says. While the node
  runs, an extra replaced by a link is left out. The link stays on disk, and at
  the next start the extra resolves through it: the link's target is then
  granted to every session with an identity. Reachable only where a session can
  write the extra's parent (an extra inside a session's folder or inside another
  `write` extra); the data directory stays refused by the start-up check. Test:
  `review_r4_1_an_extra_swapped_before_a_restart_is_granted_after_it`
  (`crates/tools/tests/review_gate3_extras.rs` in the patch). Fix: at start,
  refuse an extra whose last component is a link when its parent lies inside a
  workspace root or another `write` extra; or record each extra's identity in
  the data directory at first start and refuse a different one later.
- **R4-2. Low, by reading; the author states it.** Sockets waiting for a key
  are capped at 64 and the oldest is pushed out (`ws.rs`, `Sockets::wait`). A
  local process that opens 64 upgrades inside the one round trip the page needs
  to send its key pushes the page's socket out (1013); the page retries. It
  needs no login, only a cookie-shaped header and a forged `Origin`. Sustaining
  it means tens of thousands of upgrades a second on loopback. No per-source
  limit is possible there.
- **N-1.** A socket's key check now uses the login (`auth::socket_login` calls
  `login_with_key`, which slides the expiry), where T2 says an open tab does not
  keep a login alive. It slides once per connect, not per message.
- **N-2.** A create that times out and never lands leaves its
  `create-intent:<sid>` row (`master/src/lib.rs`, `create_session`). One small
  row per such create.
- **N-3.** `PLAN.md` says extras are bound "to the identity taken then"; the
  code binds the path at start and the identity per command. Correct the
  sentence with R4-1.

## 3. What I checked and found sound

- **R2-1.** The upgrade answers 101 to any cookie-shaped value and 401 to none,
  with no lookup (`ws.rs`, `upgrade`); every failure after that ends in 4401,
  and 4403/4429 are sent only after the right key, so no status or frame
  distinguishes a live cookie. A place among the user's eight is taken only
  after the key (`register` after `key_first`). A waiting socket lives at most
  `KEY_WAIT` (5 s). A page on another port cannot open the socket: its `Origin`
  is refused by `guard` before the handler.
- **Favicon.** `/api/favicon` needs the key, or the cookie with a link
  (`link_principal`); the path is in `linkable`, and the MAC covers the path and
  the whole query, so a link holder cannot change `url`; expiry and per-login
  binding are the rules read in fix review 2.
- **R2-3.** `logout` deletes the row of the cookie it is given and answers the
  same for any cookie. A cross-origin page cannot trigger it (state-changing,
  cookie-carrying, `Origin` not allowed: 403 in `guard`); a non-browser holder
  of the cookie can, and the harm is that one login ending.
- **P-6.** The intent is keyed by the session id the master minted and names
  the node, owner and workspace; `claim_late` claims only when the event's node
  is the intent's node. A node cannot pick another user's pending create: it
  knows only ids sent to it, and each id carries its own requester.
  `claim_session` still drops a parent of another owner, and now runs after the
  event when the ack came first, which also closes fix review 1's R3-1 ordering.
- **R1d-1 while running.** `Extra::grant` requires the path to canonicalize to
  what it did at start and passes an identity the wrapper re-checks; an extra
  that equals or contains a workspace root is refused at start
  (`check_extras_against_roots`). An extra that does not exist yet grants
  nothing until a real directory is there.
- **N-4.** `MasterCert::load_or_create` makes a pair only when both files are
  missing; any other read error, or half a pair, stops with the file and what to
  do. **N-3.** `check_data_dir` and `check_private` compare the owner with the
  effective uid and name both; a service user that owns its own data directory
  passes. **S-5.** `--` precedes the question; the group is killed on timeout.
- **The routine gate.** `queueing` is held by the tick only around the drain of
  the in-memory queue, and by the retry path across one store call; never
  across a wait on a node or a person; no ordering that can deadlock.
- **The recorded items** in `PLAN.md` say what is accepted and why, in plain
  words. None hides a path to a shell or to another user's chat that this
  review did not already rate Low or Note.

## 4. What I did not check

No full suites, `clippy` or `cargo deny`; the author's new tests were read, not
run, except my own probe. `0fd64f4` (audit rate limit) and `d9232d6` beyond the
sentences on accepted items were skimmed. No browser.

## 5. Proof

`docs/security-review/proofs/2026-11-gate3-fixes-6.patch` adds one test; it
passes on the branch, and passing is the gap.

```bash
git checkout --detach fix/R.4-remaining-findings
git apply docs/security-review/proofs/2026-11-gate3-fixes-6.patch
cargo test -p harness-tools --test review_gate3_extras
```

## 6. Final verdict for Gate 3

**On the code as it will be once this branch merges, the node link, the
approval path and the sandbox with the tools are fit to pass Gate 3.** Every
finding of Medium or above from the first review and from the five fix reviews
is closed and its proof now fails; what is left is Low, Note, or recorded.

By passing the gate the owner accepts, in my judgement:

1. **A process an agent leaves running outlives its command** (`setsid`; T.8 not
   built). Most of what this review found leaned on it. The fixes removed what
   such a process could reach; they did not remove the process.
2. **The sandbox is Landlock, and only where the kernel has it.** With
   `mode = "off"`, or `best-effort` on an old kernel, the agent's shell and the
   node's own git run unconfined. UDP and DNS are open with `network = "off"`;
   pathname Unix sockets are open before ABI 9.
3. **The static API token is every user** (T29).
4. **The approval card states what is not expanded; it does not make anyone
   read it** (W-4), and a person may approve a call they did not read.
5. **A registered node is trusted with its own sessions**: it can freeze them,
   and a copied node key can do the same (T7).
6. **The workspace `agentic` role is not a boundary, and a project's `settings`
   role steers other members' agents** (P-4, P-5).
7. **C parsers (tree-sitter) run in the process that holds the node key** (N-1
   of fix review 2).
8. **R4-1 and R4-2 above**, and that `write` and `edit` are bounded by path
   checks and the click, not by the sandbox.
9. **What was never run:** a real browser (the cookie-and-port premise, the CSP,
   the card's layout), `cargo deny` here, and two node processes on one data
   directory.

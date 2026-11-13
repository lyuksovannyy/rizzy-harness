# N.10a part 1 review, 2026-11-13: execution modes on the node

Reviewed: `feat/N.10a-node-modes`. First review at `5c8ec79`
(`git diff origin/main...HEAD`, 62 files): five reviewers, one area each
(the node's ceiling; a session's mode over its life; how each mode's
commands run; the master's rule; the UI, the tests and the documents), all
read-only, and one skeptic per finding who re-traced it from the code. 30
findings, all confirmed, none refuted; many are the same defect seen from
several areas, so the list below is deduplicated to 18. Then one independent
reviewer re-checked the fixes at `06f74ca` with proofs against the real
binary (`crates/cli/tests/support/review_n10a/`), and found seven more.
Machine: Arch Linux, kernel 7.2.15 (Landlock ABI 9), bubblewrap 0.13.1.

## 1. Verdict

**Fit after fixes.** The node's side held: it offers only what its own
self-test and `node.toml` allow, refuses anything else whatever the master
sends, and a session's mode cannot be changed after creation. The one high
finding was on the master: the interim rule for the unconfined shell
admitted every account. The fixes then opened two medium issues of their
own, both found by the re-check and closed.

## 2. Findings of the first review

| Id | Severity | What | Decided |
|---|---|---|---|
| H1 | **High** | The interim rule "`host` for the workspace's owner or a server admin" admitted everyone: every account owns its Personal workspace, any account may create a team, and nothing ties a remote node to a workspace | `host` is for server admins only, on every node, until part 2 |
| L-a | Low | A create without a mode got `host` on a node that offers only `host` | `host` is only ever an explicit request: the node gives `sandbox` or refuses; the master always sends a mode |
| L-b | Low | One failed bubblewrap probe at start removed the sandbox mode until a restart (the later step-up was removed by this change) | The start's probes are tried three times, one second apart |
| L-c | Low | `strict` offered the sandbox mode with gaps the code itself names (the shared-/proc form, a write entry holding a socket directory or a PATH folder) | `strict` refuses those |
| L-d | Low | A host chat was told "the network is off" where nothing stops TCP | Its own gap, `gap:host:net-open`, with its own words |
| L-e | Low | The hello carried only the network gaps; a sandbox chat below Landlock ABI 9 or in the shared-/proc form showed a plain "sandbox" | `gap:sandbox:sockets` and `gap:sandbox:proc` announced and worded |
| L-f | Low | The backend held a working host confinement on nodes with `host = false`; one check stood between a session claiming `host` and a shell | The backend gets a refusing confinement for a mode the node does not offer |
| L-g | Low | A node that does not know modes still ran chats; the master never checked the hello | The master refuses an agentic create when the hello does not list the mode |
| L-h | Low | The rule ran only at create: a demoted admin kept their host chats | The mode is mirrored onto the master's session row and checked on every later action, failing closed |
| L-i | Low | `/api/nodes` showed every node's modes and gaps to every account | Blanked for accounts without the agentic grant |
| L-j | Low | In the UI a confirmed `host` choice followed an automatic switch to another node | The choice is bound to the node it was confirmed for |
| L-k | Low | The routine runner's always-sandbox rule had no test | Test added |
| L-l, L-m | Low | Stale threat-row references; the README contradicted the note about bubblewrap in a rootless container | Corrected |
| N-a | Note | `graph_query` starts graphify outside bubblewrap in a sandbox chat (older code) | Not fixed here: documented in the README, the note and T39 |
| N-b | Note | The "mode no longer offered" refusal was tested only for a prompt | Restore, callback and artifact staging added |
| N-c | Note | A host chat's model was offered background commands that are always refused | A host session gets no background table |
| N-d | Note | Nothing recorded who was given a host chat | One audit line per request for `host`, granted or refused |
| N-e | Note | The approval card, the only check in a host chat, did not say so; and in `auto` there is no card | The card says it is the host shell, and **a host session never runs in `auto`** (clamped to `ask` at create and at load) |

## 3. Re-check of the fixes, `5c8ec79..06f74ca`

H1 closed: on the real binary a plain account was refused `host` in its
Personal workspace (active and by id), in a team it made itself, as a
workspace admin and when naming the local node (`host_rule.py`). L-a to
L-f, L-i, L-j, N-c and N-e closed. Asking for `auto` with
`max_approval_mode = "auto"` recorded `ask`.

| Id | Severity | What |
|---|---|---|
| RN-1 | **Medium** | The revocation sweep answered once per (owner, workspace, node); since L-h the answer also depends on the chat's mode. A demoted admin with a host chat and a sandbox chat: with the sandbox chat newer, the running host turn was not stopped; in the other order the sandbox turn was stopped for nothing. Shown on the real binary (`sweep_memo.py`). New prompts and approvals were refused either way |
| RN-2 | **Medium**, a regression from L-g | After a server restart a routine whose node had not said hello yet was blocked and paused instead of retried, and a person's create got 409 with a wrong reason (`routine_unknown_node.py`). The tests' fixture had been changed in a way that hid it |
| RN-3 | Low | The master's new pre-check replaced the node's own refusal, so the person no longer read the remedy |
| RN-4 | Low | "Every request for `host` is audited" was not true for requests refused by an earlier check; the line lacked the node and the session |
| RN-5 | Low | Four sentences in the docs and comments were now false |
| RN-6 | Note | A data directory made before `sessions.exec_mode` does not open (`old_data_dir.py`); nothing said so |
| RN-7 | Note | The start's retry triples a slow probe failure; a bubblewrap that hangs still hangs the start (older behaviour) |

Tests the re-check found weak: the demotion test ran no turn, so it never
exercised the sweep; the "silent node" would have refused by itself; the
`strict` test returns green below Landlock ABI 9, which is CI's kernel; the
L-f wiring in `cli/src/main.rs` is tested only through its helper. The
builder had not run the Rust tests against the code before the fixes; the
re-check confirmed the "failed before" claims by reading the old code, and
named three tests that pass on the old code too (coverage, not
failing-first).

## 4. After the re-check (the co-owner's closing)

Fixed on the branch at `e8213b7`, tests first:

- **RN-1.** The chat's mode is part of the sweep's key.
  `a_demotion_stops_the_host_chat_and_only_the_host_chat` (both chats with a
  running turn, both orders) failed with "host not cancelled" before.
  `sweep_memo.py` now shows the host turn ended and the sandbox turn running
  in every order.
- **RN-2.** The mode check runs only for a node with a live link; a node
  that is away is `NodeOffline` as before. The routine fixture announces no
  mode again, and the offline test failed on the code before.
  `routine_unknown_node.py` shows `pending` with the retry reason in both
  cases.
- **RN-3, RN-4, RN-5.** The refusal names what the node offers and what to
  do on the node; one audit line for every request for `host`, written after
  the create with the node as resolved and the session id; the texts
  corrected.
- **RN-6, RN-7.** Said in the README and the note. No compatibility code:
  an earlier data directory must be removed or recreated.

The whole suite at `e8213b7` (sandbox and FUSE required): 1396 passed, 1
failed, 10 ignored; the failure is the timing-dependent
`callbacks_real::an_idle_end_resumes_the_agent_once_and_two_ends_make_one_turn`
under load, 4 of 4 when run alone.

Open after this branch:

- `graph_query` outside the containment in a sandbox chat (N-a).
- No test reaches the L-f wiring in the binary crate.
- The `strict` write-entry test does not run in CI (Landlock ABI 8 there).
- A node cannot know which workspaces it serves, so "refuse `host` on a node
  that serves several" is not built; it belongs with the node's attached
  list (C.12, N.1). Until then server admins only.
- Not exercised: Landlock below ABI 9 and the shared-/proc form (this
  kernel has neither); the published image; a second remote node.
- `set_agent` and `select_variant` have no gate; they start no work. Part 2
  gives them one.

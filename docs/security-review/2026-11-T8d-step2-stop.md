# T.8d step 2 review, 2026-11-13: the person's stop, the strip, default ON

Reviewed: `feat/T.8d-step2-stop` at `678f4c9` (`git diff ed9083c..HEAD`):
`4e2c2d6`, `2a35c51`, `a7f900a`, `9f833f9`, `678f4c9`. Nothing pushed. Read
in full, then attacked on the branch's own build. No product or builder
test source was changed; the two step-1 proofs the builder marked outdated
(`review_t8d_proofs.rs` T8D-4, `person_cannot_stop.py`) are updated to
assert step 2, and new proofs are added under
`crates/cli/tests/support/review_t8d/` prefixed `step2_`. Machine: Arch
Linux, kernel 7.2.15, bubblewrap 0.13.1, the repository on a rotational USB
disk. Format and severity as in the step-1 record.

The builder's suites at this head, run whole:

```text
cargo test -p harness-master --test background_stop_http    6 passed
HARNESS_REQUIRE_BWRAP=1 HARNESS_REQUIRE_FUSE=1 cargo test -p harness --test background    31 passed, 2 ignored, 60.2 s
web vitest (whole)    24 files, 205 passed
web/e2e/background.cjs (Playwright, real binary)    strip shows id+command+time, second tab, turn's Stop leaves it, strip's Stop kills it, no page errors
```

## 1. Verdict

**Fit after fixes.** The step-2 deliverables — the owner-only stop route and
the strip — held every attack I could reach. Authorisation is parity-exact:
another member, a workspace admin and a server admin all get the same `404
unknown session`, byte-for-byte and in the same time, as for a chat that does
not exist; a cookie without its login key is `401`, a wrong/missing/foreign
`Origin` or `Host` is `403` from the same guard the neighbouring routes use;
a cross-origin `POST` and its preflight are refused. Every kill path ended
the command with **exactly one** end and freed it: `setsid`, `nohup`, a
double fork, a daemon in its own session, a process that ignores `SIGTERM`,
and one stuck in an uninterruptible FUSE read; so did eight-at-once with one
Stop all, every stop/own-end/timeout race, and a node or master killed,
frozen or restarted mid-command. No id was ever reused across a restart, and
the "already ended" shortcut cannot be made to lie. With the defaults off
(`contain = "off"`, no bubblewrap, `background_per_session = 0`, `network =
"off"`) a background start is still refused, and a sub-agent still gets no
table.

Two weaknesses sit at the edges, neither in the route's own logic:

| Id | Severity | What | Proof |
|---|---|---|---|
| S2-1 | **Medium** (T2/P-1 premise; surfaced by the default flip) | The master binds `127.0.0.1` only by default but allows `Host`/`Origin` `localhost:<port>` and `[::1]:<port>`. With background commands now ON by default, a steered agent's command binds `[::1]:<master-port>` (the master never occupies it); `localhost` resolves to `::1` first, so `http://localhost:<port>` — an origin the master calls its own — is served by the agent, with the user's host-scoped session cookie. | `step2_defaults.py` §4 |
| S2-2 | **Low–Medium** (the "light process"/shared-master cost) | The stop's store read (`Store::background_events`) filters `kind` with no index on it: the query plan is a backward scan of the session's whole event log, held under the single mirror `Mutex`. A chat with a long transcript and few background events makes every stop scan it. | `step2_scan.py` |

Asked for before the default flips on in anger: **S2-1** (the master should
bind every loopback address it lists as an allowed origin, or stop allowing
the ones it does not bind). **S2-2** is a one-line index. Neither touches the
route's authorisation or its kill, which are sound; if the owner judges S2-1
out of this task's scope (it is older code that the default flip merely
exposes), the route and strip are independently fit to merge.

How to run the proofs (temp folders under `target/review_t8d_step2/`, ports
the kernel picks, markers `sleep 9790xx`–`9795xx`):

```text
RIZZY=$PWD/target/debug/rizzy-harness python3 -I crates/cli/tests/support/review_t8d/step2_authz.py
RIZZY=$PWD/target/debug/rizzy-harness python3 -I crates/cli/tests/support/review_t8d/step2_stop.py [escapers|races|first_wait|stuck|all]
RIZZY=$PWD/target/debug/rizzy-harness python3 -I crates/cli/tests/support/review_t8d/step2_restart.py [remote|embedded|all]
RIZZY=$PWD/target/debug/rizzy-harness python3 -I crates/cli/tests/support/review_t8d/step2_defaults.py
python3 -I crates/cli/tests/support/review_t8d/step2_scan.py
crates/cli/tests/support/review_t8d/run_rust_proofs.sh t8d4_step2
python3 -I crates/cli/tests/support/review_t8d/person_cannot_stop.py
```

## 2. Findings

### S2-1. Medium. A default, possibly-injected agent can squat a loopback origin the master calls its own

**The invariant it bears on.** T2 / Gate 3 P-1: "Browsers send a cookie to
every port of its host … so another local server, such as one the agent
started on `127.0.0.1:8000`, gets the cookie … The login therefore also has
a random 256-bit login key … per origin with the port." The defence rests on
the agent's local server being a **different origin** from the real UI, so it
holds the cookie but never the login key. The default flip to ON
(`background_per_session = 8`) gives a prompt-injected agent in an `auto`
chat an unattended way to break that premise on one spelling of the host.

**The mechanism.** `allowed_hosts(bind, …)` (`crates/cli/src/main.rs:226`)
lists `127.0.0.1:<port>`, `localhost:<port>` **and** `[::1]:<port>` whenever
the bind is loopback or unspecified, but the server listens on the one bind
address (`127.0.0.1` by default; the banner advertises `http://127.0.0.1:…`).
`[::1]:<port>` is never occupied, so a contained background command may bind
it, and `getaddrinfo("localhost")` returns `::1` first on this machine. A page
fetched from `http://localhost:<port>` then reaches the **agent's** listener,
on an origin (`localhost:<port>`) the master's own guard accepts and that
shares its `localStorage` with any login the user made via `localhost`.

**Reproduction.** `step2_defaults.py` §4 (the agent's command binds the
master's own port on `[::1]`; the master is on `127.0.0.1`):

```text
4. the master listens at ['127.0.0.1:32895']; before: http://localhost:32895/api/setup/status -> HTTP 404 (the master)
   the agent's background command binds [::1]:32895: running in the background as bg-1 …; listeners on that port now: ['127.0.0.1:32895', '[::1]:32895']
   http://localhost:32895/… is now answered by: THE AGENT'S COMMAND (http://127.0.0.1:32895/… by: the master)
   the master takes Host: localhost:32895 (it is one of its allowed origins): 200/404
```

and, independently, that `localhost` resolves to `::1` first here
(`getaddrinfo("localhost", …)[0] -> ('::1', …)`).

**What an attacker gains.** With the default config and an `auto` (or
`ask`-then-approved-once) chat, a steered agent occupies `[::1]:<master-port>`
indefinitely (no race: the master never binds it). A user who opens the UI as
`http://localhost:<port>` — a spelling the master itself advertises as valid
— is served the agent's page, which receives the session cookie and runs in
the `localhost:<port>` origin: it can read that origin's `localStorage` (the
login key, if the user ever logged in there) and otherwise phish. Stopping is
harmless, but this is a session-hijack primitive the P-1 design was written
to deny, newly reachable by default.

**Scope note.** This is not a flaw in the stop route (its CSRF/Origin guard
is the neighbours' own; S2-1 is reachable without it). It is older
bind/allowed-origin code that step 2's default flip turns into a default-config
exposure (task item #2, #5). An operator who had turned the feature on in step
1 had the same exposure; nobody did, because it shipped off.

**The smallest fix.** The master binds **every** loopback address it lists as
an allowed origin (add `[::1]:<port>` and `127.0.0.1:<port>` to the listener
set when the bind is loopback/unspecified), so the port is occupied and cannot
be squatted; or it stops listing `localhost`/`[::1]` origins it does not
actually serve. Either closes the shadow without touching the route.

**The test.** `step2_defaults.py` §4 turned round: after the fix, the agent's
bind of `[::1]:<master-port>` fails (address in use), and `http://localhost:<port>`
is answered by the master.

### S2-2. Low–Medium. The stop's store read scans the whole session log under the mirror lock

**The claim it bears on.** §12 of the note: the master answers from
"`Store::background_events`, the newest 4096 … events of the chat, **one
bounded read**", "a node that floods them costs one bounded read." The read
is bounded in **output** (4096 rows), not in **scan**.

**The mechanism.** `background_events`
(`crates/store/src/lib.rs:1480`) is
`… WHERE session_id = ?1 AND kind IN ('background_started','background_ended')
ORDER BY seq DESC LIMIT 4096`. There is no index on `kind`; the `events` table
is `PRIMARY KEY (session_id, seq) WITHOUT ROWID`. The plan is a backward scan
of the session's events by `seq`, filtering `kind` in memory, and it runs
inside `mirror.call`, which holds the master's single SQLite `Mutex`
(`Arc<Mutex<Connection>>`): while it runs, no other session's read or write on
the master proceeds.

**Reproduction.** `step2_scan.py` (the exact schema and query; a chat that
started background commands early, then ran a long time):

```text
plan: ['SEARCH events USING PRIMARY KEY (session_id=?)']
tail of     10000 ordinary events after 8 background ones: the stop's read took  0.4 ms, returned 8 rows
tail of    100000 ordinary events after 8 background ones: the stop's read took  3.7 ms, returned 8 rows
tail of   1000000 ordinary events after 8 background ones: the stop's read took 35.9 ms, returned 8 rows
tail of   3000000 ordinary events after 8 background ones: the stop's read took 109.7 ms, returned 8 rows
```

**What an attacker gains.** Linear in the session's log length, held under the
lock that serialises every user's master I/O. Only the chat's owner can
trigger it (authorisation holds), but a single owner with a very long chat who
spams Stop/Stop all degrades the **shared** master for everyone. Normal chats
(thousands of events) are sub-millisecond, so this is a tail risk, not a
routine one.

**The smallest fix.** A covering index `events(session_id, kind, seq)` (the
`IN (…)` then uses it directly, newest-first, touching only background events),
or build the running set from the fold/mirror that already exists rather than
re-scanning the log.

**The test.** `step2_scan.py` after the index: the plan uses the index and the
3M-tail read stays flat.

## 3. Attacks that held

### 3.1 Authorisation of the route (`step2_authz.py`)

A team workspace: alex (server admin, workspace owner), wanda (workspace
admin), olga (member), bob (the owner, member with the agentic grant). Bob's
chat runs two background commands.

```text
olga, a member of the same workspace       one/all  404 {"error":"unknown session"} | 404 {"error":"unknown session"}
wanda, a workspace admin                    one/all  404 {"error":"unknown session"} | 404 {"error":"unknown session"}
alex, server admin and the workspace owner  one/all  404 {"error":"unknown session"} | 404 {"error":"unknown session"}
signed out                                  401 {"error":"not logged in"}
bob's cookie without its login key          401 {"error":"not logged in"}
bob's cookie with olga's login key          401        bob's key with olga's cookie   401
bob, no Origin                              403 origin required
bob, Origin of another port of this host    403 origin not allowed
bob, Origin https:// of the same host       403 origin not allowed
bob, Origin null / http://evil.example      403 origin not allowed
bob, Host evil.example                      403 host not allowed
a wrong API token (± bob's cookie)          401 {"error":"not logged in"}
the commands still run: True
OPTIONS from a cross-origin page: 403 origin not allowed; no Access-Control-* headers
GET/DELETE/PUT/PATCH/HEAD by bob: 405
timing (400 each): bob's chat median 0.41 ms (p10 0.32, p90 0.50) | no such chat median 0.42 ms (p10 0.34, p90 0.51)
```

Every non-owner — admins included — gets the same `404` and body as for a
nonexistent chat, with no timing difference, so the route confirms nothing
about another user's chat (existence, node, running commands). The CSRF/Origin
guard is `auth::needs_origin`/`origin_allowed`, the same the neighbours use;
cross-origin `POST` and preflight are refused. Path/id tricks:

```text
bg-+1, bg--1, bg-1<space>, BG-1, bg-1%00, bg-%EF%BC%91 (fullwidth 1), bg-1e0, bg-0x1, bg-1000000000, bg-<5000 nines>, ..%2F..%2Fcancel, bg-1%2Fstop  -> 400 bad id
bg-01, bg-0, bg-999999999, bg-3 (owner)   -> 409 no background command … (it has bg-1, bg-2)   [nothing stopped]
the same odd ids / chat id by a non-owner  -> 404 unknown session (the id check never runs for non-owners)
chat id upper/no-dashes/{…}/urn:uuid:      -> owner 409 no such bg … | non-owner 404   [nothing stopped]
the owner stops bg-1: 200 its process is gone: True; bg-2 runs: True
the API token (machine owner), Stop all: 200; bg-2 is gone: True
```

Access revoked, each with a command running and no turn:

```text
removed from the workspace: the chat is his Personal one again, where he may still run agentic chats; his stop works (stopped_by_user). [by design]
the agentic grant taken away: the sweep stopped it (access_revoked); his later stop of that id is the "already ended: … owner lost access" 200; a new prompt is refused 403.
the chat deleted: a stop of it -> 404 unknown session.
the account disabled: the sweep left nothing; his stop afterwards -> 401 not logged in.
```

The one difference from "chats are private" worth stating: removing bob from
the workspace returns the chat to his Personal workspace, where he still owns
it and may run agentic work, so his background commands keep running and he can
still stop them — correct (he did not lose access, only that workspace).

### 3.2 The kill ends everything, exactly once (`step2_stop.py`)

```text
escapers (setsid, nohup, double fork, a daemon in its own session, TERM ignored): 7 processes; Stop bg-1: 200
   left after 0.01 s: 0; the end: reason stopped_by_user, confirmed ended; the other command (bg-2) still runs: True
   the same stop again: 200 "bg-1 had already ended: it was stopped by the person"
   nine starts with the defaults: 8 run (the 9th refused, names bg-1..bg-8); Stop all: 8 ends stopped_by_user, left 0 after 0.42 s
24 stops of bg-1 and 8 Stop all at once: {200 stopping: 32}; processes left 0; ends {bg-1: stopped_by_user, bg-2: stopped_by_user}
16 stops aimed at the command's own exit or its timeout: a mix of stopped_by_user / exited / timed_out, some "already ended"; starts without exactly one end: 0
Stop all 1 s into the turn's second start (log shows bg-1): 200 stopping [bg-1]; the command starting (bg-2) and a third (bg-3) started after the Stop all survive the still-running turn; a second Stop all, the turn over, ends all three; left 0
the turn's Stop then Stop all, and Stop all then the turn's Stop, 1 s into a start: both leave 0 processes; exactly one end each
stuck: a command in an uninterruptible FUSE read; Stop: 200; end after 4.2 s reason stopped_by_user, confirmed still_alive; the agent's list says "killed, but NOT ended: a process is still alive … waits on I/O the kernel cannot interrupt; its guard keeps killing"; released by the mount, the process is gone
```

Stop all does not reach a command started by the still-running turn **after**
the Stop all (that start is the turn's, admitted after the close was not set):
a person who presses Stop all while a turn is mid-flight must press it again
once the turn ends, which the strip's header ("keep running when the turn
stops") and its persistent Stop all make available. A stuck read behaves as the
step-1 FUSE case: the end is recorded `still_alive` and the guard keeps killing;
nothing escapes, and the slot is accounted as R-1 fixed it.

### 3.3 Node/master away, frozen, killed, restarted (`step2_restart.py`)

Real `serve --node-listen` + real `node run`, the node's defaults:

```text
1. the node frozen (SIGSTOP), 300 stops of bg-1 at once: all 504 "node did not answer in time" after 15 s; the server meanwhile RSS 65->83 MiB, descriptors 26->326, another request of the same user still 200; on SIGCONT the queued stops are carried out, the command ends stopped_by_user, descriptors back to 26
2. the node killed (-9): its command's processes die with it; the log still shows bg-2 running, Stop -> 503 "the node … is away"; Stop all -> 503; on restart bg-2 is node_restarted, a stop of it is the "already ended" 200; a new start is bg-3 (ids never reused)
3. the master killed (-9) mid-turn: the node's commands run on, incl. one started while the master was away; the master back, the log catches up (bg-4), Stop all ends bg-3+bg-4, left 0
4. the node stopped (SIGTERM, exit 0): the command ends node_stopping; a stop of it afterwards is the "already ended" 200
5-embedded: the server with its own node killed (-9) and restarted: the command is node_restarted, a stop is "already ended"; a new start is bg-3, never an id again, its stop works
```

300 held stops of a frozen node each occupy a request to the 15 s ack timeout
(≈300 descriptors, +18 MiB), all released after, and a concurrent request from
the same user is unaffected: no unbounded pile-up (task #7). No id was reused
across any restart; every command had exactly one end. The "already ended"
shortcut (task #4) can only report an id that has a `background_ended` in the
window; a *running* command has none, and ids are per-session monotonic (after
a restart `next = max+1`, and all previously-running get `node_restarted`
first), so it cannot answer "ended" for something the node still runs. The
4096-row window is used only to answer the stop; the strip is a fold of the
full log over the WebSocket, so a long-running command pushed out of the window
still shows and is still relayed to the node (Stop all is always sent).

### 3.4 Default ON, and what still refuses (`step2_defaults.py`)

```text
1. node.toml with the folder only (the node's default mode is ask): a background start parks for a card whose reason says it KEEPS RUNNING for up to its timeout and the network is on; "for this turn" is NOT offered; the identical start asks again; the timeout is clamped to the ceiling (asked 99999 -> 3600 s). The chat's row in the list of chats has no "running"/background field; /api/attention has no item for it (step 3).
2. max_approval_mode = "auto": nine starts in one turn, no click, 8 run (the node cap is 8 a chat), the listener answers another process of this machine, its timeout is the node's ceiling; a second chat fills 8 more (eight chats fill the node's 64); Stop all in both ends them.
3. [sandbox] contain = "off": refused "background commands need the shell's containment …"; nothing runs.
   [sandbox] bwrap = <missing>: NOT CONTAINED (Landlock alone) -> same refusal; nothing runs.
   [limits] background_per_session = 0: "background commands are off on this node"; nothing runs.
   [sandbox] network = "off": the listener cannot bind; nothing runs; no listener.
```

With the defaults and `auto`, a steered agent can leave up to 8 commands a
chat × 64 a node running with the network on, each up to the node's
`bash_timeout_max_s` (3600 s default), and the person sees them in the strip
(and can Stop/Stop all) but has no attention item or sidebar marker yet (step
3, as documented). Every containment-off path refuses a start, so step 1's
"off was the only safe default" items reduce to S2-1 (above) under default-on.
Sub-agents remain refused by construction: `background_table` returns `None`
when `parent.is_some()` (`crates/node/src/session.rs:98`), unchanged by the
default; the step-1 proof `a_sub_agent_cannot_start_list_read_or_stop` still
passes. Routine/untrusted chats run only in `ask`/`ask_all`
(`routines/runner.rs`), so a start still parks for a click.

### 3.5 Untrusted text into the UI

Read of `BackgroundStrip.tsx`, `lib/background.ts` and `backgroundStrip.css`,
plus the strip's vitest file (24 files, 205 passed). Everything the strip
shows that comes from the model or the node is React **text**, run through
`showHidden` (bidi/zero-width/control marked as `<U+…>`): the command head
(`title` attribute too), the node label, and the node's refusal shown on the
row (`backgroundstrip.test.tsx` asserts a trailing `U+202E` becomes
`<U+202E>`). No value is HTML, a URL, or a class name; the one `aria-label`,
`Stop ${bg_id}`, uses the server-validated `bg-<digits>` id. The timeline's
`background_ended` tail is `showHidden` inside `<pre>`. A command head that
imitates the strip's labels ("ended: stopped", "node away") lands in the
command column (`.bg-cmd`), a separate cell from the state (`.bg-state`), so it
cannot pose as the row's own status. The route's error bodies carry only static
strings or node-generated `bg-<n>` ids (the node's refusal names the chat's own
ids), never attacker free text.

### 3.6 On the three decisions

- **(a) an admin cannot stop another user's runaway command.** Agree, with a
  caveat. "Chats are private to their owner, for admins too" is a stated
  invariant and the route honours it exactly (3.1). The node owner's levers
  (stop the node, set `background_per_session = 0`) are coarse — a single
  runaway in someone else's chat has no per-command kill short of them — but a
  per-command admin override would breach the invariant for a rare case, and
  the timeout ceiling bounds the blast. Acceptable as decided; worth a README
  line that the node owner's only in-product stop for another user's command is
  node-wide.
- **(b) "stopping…" is per tab and not retried.** Agree. The click state is
  cosmetic; the authority is the fold, so a second tab and a reload show the
  real end, and a failed stop re-enables Stop. A missing retry costs at most one
  extra click.
- **(c) time left trusts the browser clock within [0, timeout].** Agree.
  `msLeft` clamps to `[0, timeout_s]` and nothing acts on it — only the log's
  end removes a row — so a skewed clock mis-states a label but cannot hide or
  falsely end a command. Fetching server time would claim an exactness the
  deadline does not have (node vs master clocks also differ).

## 4. Not tested

- The end-to-end browser consequence of S2-1 (login-key theft / phishing): I
  proved the origin shadow, the cookie delivery and the `localhost`→`::1`
  resolution, not a full key exfiltration in a driven browser.
- Real providers (none contacted); 64 commands flooding at once; the largest
  bounds (`background_output_kb = 4096`, `background_per_node = 256`); a remote
  node with its link down at a stop beyond the cases in `step2_restart.py`.
- `cargo clippy`, `cargo fmt --check`, `cargo deny`, `pnpm typecheck`,
  `pnpm build` and `pnpm size` were **not** run by me (no network for a clean
  install; this machine has no cargo-deny). Of the Rust suites I ran only
  `background_stop_http`, `background`, and the review proofs; of the web, only
  vitest and the Playwright e2e.

## 5. Left clean

Every `step2_*` script kills what it started by pid and prints `cleanup
killed: nothing`. After the last run: no process with a `sleep 9790xx`–`9795xx`
marker or `review_t8d_step2` in its command line; no `t8e-stall` line in
`/proc/self/mountinfo` (one stale FUSE mount left by the builder's `background`
suite — under a `tempfile` dir, not a `step2_` script — was unmounted by its
exact path with `fusermount3 -u -z`). The owner's server
(`serve --data-dir /home/lebron/test-harness`) was never signalled, and its
data directory and port never opened; nothing outside this machine was
contacted. `step2_scan.py` builds and removes its own SQLite file and touches
no product database.

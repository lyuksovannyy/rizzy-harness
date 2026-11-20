# Gate 3 fix review 2, 2026-11-11: the approval-path fixes and the web UI

Third pass. Reviewed: `fix/R.2-approval-findings` (pull request 38; `a19ff62`,
`7cbffd1` P-1, `f306aca` P-7, `ad0bce9` P-3, `1ce8657` P-2, merge `59bea35`), read
detached, nothing committed to it. Then the web UI (`web/src`), which neither
earlier report reached, on `main` at `6cd5b38`. No product code changed.

Same severity scale and the same rule for proofs as before: a test that passes
is an attack that still works.

## 1. Verdict

| Finding | First severity | Result |
|---|---|---|
| P-1 (the cookie reaches every port) | High | **Closed with a remaining gap.** The cookie alone no longer reads a chat, answers an approval or opens a working socket. It still takes every one of the person's WebSocket places and tells its holder whether the login is live (R2-1). |
| P-2 (`delegate` skips the approval path) | Low | **Closed.** |
| P-3 (revocation sweep runs once) | Low | **Closed with a remaining gap** (R2-2, Low). |
| P-7 (deny refused on a diverged chat and before a password change) | Low | **Closed.** |

**R.2 is fit to merge.** Its one new finding of weight (R2-1, Low) is a denial of
the person's own live view, not a way back to the approval click, and is a small
fix that can follow.

**The web UI is the larger result of this pass.** The Markdown side holds. The
approval card does not: for `write`, `bash`, `edit` and every tool without a
card of its own, what the person reads before clicking Allow is not what the
node will run (W-1, High, proven). That is on `main` today and is independent of
R.2. With P-1 closed, the click is again the control in `ask` mode, and W-1 is
what stands between a prompt-injected model and that click.

New findings: 1 High (W-1), 5 Low (R2-1, R2-2, R2-3, W-2, W-3), 4 Note.

## 2. New findings

### W-1. High. The approval card does not show what will run

Severity: in `ask` mode the click is the only control over `write` and `edit`
(they are not sandboxed), and the card lets a model put the part that matters
where the person does not see it, with no race and no precondition.

- Adversary: a prompt-injected agent (1).
- Where: `web/src/components/ToolCard.tsx`, the non-web branch.
  1. **`write`: content after 4,000 characters is never shown**
     (`:271`, `String(item.input?.content ?? "").slice(0, 4000)`), before or
     after approval, and nothing says it was cut. A `Makefile`, a CI workflow or
     a `package.json` with 4,000 harmless characters and then a command is
     approved as harmless.
  2. **`bash`: the command is rendered raw** (`:273`), in a `pre.term`
     that does not wrap and is 420 px high (`web/src/styles.css:1636-1645`:
     `white-space: pre; overflow: auto; max-height: 420px`). A run of spaces
     puts `; curl … | sh` off to the right, a run of newlines puts it below the
     box. The one-line head (`:212`) is cut by CSS (`.ellipsis`, `:478`) and has
     no `title`.
  3. **Bidi overrides and zero-width characters are shown as they are.** The
     web tools go through `showHidden` and a `<bdi dir="ltr">` (`:63`, `:73`,
     `:260`) precisely so that the person sees `<U+202E>`; the shell command,
     the written content and the edit strings do not.
  4. **`write`/`edit`: the path is only in the cut head** (`:113`, `:212`); the
     open body shows content or the diff, not where it goes. A long harmless
     prefix hides the file name.
  5. **Every other tool** (`delegate`, `publish`, `graph_query`, …): the input
     is only in the cut head (`:114-121`); the open card shows none of it. Since
     R.2, `ask_all` asks for a delegation whose task text the person cannot read
     in full.
  6. The second button reads "Allow `<tool>` for this session" (`:235`); the
     node allows this exact call again for this turn (T4). T4 records a wording
     gap; the button promises more than "this turn", not less.
- Established: **proven by test**, six cases in
  `web/src/components/review-gate3.test.tsx` (in the patch), rendered with the
  real `ToolCard`. The off-screen case rests on the class and the stylesheet
  lines above: happy-dom lays nothing out, and no browser was run.
- Smallest fix: one component for "what this call will do", used by every
  pending card: the full input, never cut (a long one scrolls inside a box that
  says how long it is, e.g. "412 lines, 18,300 characters; showing all"),
  `white-space: pre-wrap; overflow-wrap: anywhere`, `showHidden` on every string
  and `<bdi dir="ltr">` around it, the resolved path on its own line, and the
  Allow button enabled only when the box was scrolled to its end when it does
  not fit. Reword the second button to "Allow this exact call for this turn".
- Threat row: T4 and T14. T14 says "answers carry ids, never model text" and
  covers what the chat renders; no row covers what the approval card leaves out.

### R2-1. Low. A cookie alone still takes all eight of the person's WebSocket places, and shows whether the login is live

- Adversary: the same holder of the cookie as in P-1 (another local port).
- `ws::upgrade` accepts the cookie without the key
  (`crates/master/src/ws.rs:208-214`) and registers the socket under the user
  (`:228`, the cap of eight) before `key_first` runs; the place is held for up
  to `KEY_WAIT` (5 s, `:70`). Eight such sockets, reopened every five seconds,
  and the person's own page gets `429` on its WebSocket: no live events, no
  approval cards arriving. The places are per user, not per login, so logging
  out and in again does not end it; only a password change does (it deletes
  every session row).
- The upgrade's status (101 or 429 against 401, and 403 for a pending password
  change) and `/api/favicon` (`crates/master/src/favicon.rs:66-72`: 400/404/200
  against 401) both tell a cookie-only client that the login is live. The
  author's "missing or wrong is the same 401 as no login" holds for every other
  route (`auth::by_cookie`, `auth.rs:834-840`, returns before any lookup when
  the header is absent).
- Established: **proven by test**,
  `review_r2_1_a_cookie_alone_takes_every_socket_place`
  (`crates/master/tests/login_binding_http.rs` in the patch).
- Smallest fix: count a socket against the user's eight only once its key was
  accepted; give unkeyed sockets a small separate allowance per login (two) and
  a shorter wait. For the favicon, take the link route the other `<img>`s use.
- Threat row: T2.

### R2-2. Low. The sweep after a hello reads the mirror once, before the node's replay

`cancel_revoked_on` (`crates/master/src/workspaces.rs:556-564`) lists
`running = 1` rows of the node as the mirror has them at the hello. A turn whose
`turn_started` was not mirrored before the link dropped is not in that list, and
the replay that brings it arrives after the sweep ran. `cancel_retrying`
(`:592-622`) retries a cancel that did not fit for about 7.5 s without looking
again whether the owner is still blocked; a grant given back in that window gets
its next turn cancelled. Both are narrow. Shown by reading. Fix: run the sweep
again when a resume for that node completes, and re-check `blocked` before each
retry.

### R2-3. Low. A logout without the key leaves the login alive on the server

`logout` deletes the session row only with the key (`crates/master/src/http.rs:425-429`)
and always clears the browser's cookie. That is the right choice against a
cookie-only client. The other half: a person whose page lost its key (storage
cleared) cannot end that login at all; it lives until its 30-day expiry or a
password change, and R2-1 works against it for that long. Shown by reading.
Fix: let a password-authenticated call end all of the caller's other logins
("sign out everywhere"), and offer it after a login that found the key missing.

### W-2. Low. A link to another port of the UI's own host reads as the UI's host

`hostOf` (`web/src/md/links.ts:20-23`) shows `hostname`, without the port, and
`needsConfirm` (`:127-146`) does not ask for `http://localhost:8000/`. A model's
link to its own dev server on the UI's host is shown as `localhost`, the same as
the harness itself. After P-1 the visit gives much less than before (R2-1 is what
is left), which is why this is Low. Fix: show the port when it is not the
scheme's default, and always confirm a link to the page's own host on another
port. Shown by reading.

### W-3. Low. A chat title, a file name and a node's error text keep bidi and hidden characters

Model- and node-supplied strings outside Markdown (`Sidebar.tsx:363` title,
`ToolCard.tsx:227` the approval reason, `:248`, tool names at `:211`, error
items) are rendered by React as text, so no markup runs, but only the Markdown
pipeline and the web cards strip or show hidden characters. A title can read as
another chat's. Shown by reading. Fix: `stripHidden` where the fold builds these
strings (`web/src/fold.ts`), once.

### Notes

- N-1. `api.ts` `seen()` (`web/src/api.ts:93-106`) stores any `login_key` field
  of any JSON answer. No route but login, claim and password change returns one
  today; a future route that echoes node JSON at top level would let a node sign
  the person out. Take the key only from those three calls.
- N-2. The key is in `localStorage` (`harness.login`), so it is exactly as safe
  as "no script from a model or a node runs in this origin". That holds as far
  as I read (section 3) and the CSP is the second line: `script-src` falls back
  to `default-src 'self'`, with no `unsafe-inline` and no `unsafe-eval`, and
  stored files are served `nosniff` as attachments, so they do not load as
  script.
- N-3. `delegate` is asked as `ToolRisk::ReadOnly`
  (`crates/agent-native/src/lib.rs:1313`): `ask` does not ask for it, `ask_all`
  does. That is the author's stated design and the child's own calls are gated;
  recorded here so nobody reads "delegate goes through approve" as "ask asks".
- N-4. Task 3.10, re-read as asked: `auth::temporary_password`
  (`crates/master/src/auth.rs:227-242`) takes 100 bits from the OS generator with
  an unbiased 32-symbol mask and is returned once; drafts and the login key are
  cleared when `me` is null (`web/src/App.tsx`); a `#` link is a link only when
  it names a footnote of its own message (`web/src/md/Link.tsx`, the `fn` prefix
  check and `CSS.escape`). Nothing found.

## 3. What I checked and found sound

**R.2**
- **Every route, by how it authenticates now.** `authenticate`
  (`auth.rs:816-827`): a `Bearer` header is the API token or nothing; otherwise
  `by_cookie`, which needs the cookie and a well-formed `x-harness-login` and
  compares the stored hash in constant time inside the session lookup
  (`store/src/lib.rs`, `ui_session_user` with the `bound` closure; the expiry
  slides only after the key matched). Every `require`/`require_any` caller goes
  through it. The only callers of the cookie-only `cookie_user` are
  `ws::upgrade`, `favicon` and `link_principal` (grep over `crates/master/src`).
  No response in the crate sets an `Access-Control-*` header.
- **The key is returned only by** `session_response` (`http.rs:389-399`), called
  from setup (claim code), login (password) and password change (current
  password, and the caller's key). `/api/me` re-sends the cookie, never the key.
- **The socket.** Before the key: no `subscribe()` on the live channel, no read
  of the mirror, nothing sent (`ws.rs:376-381`); a missing, late or wrong key
  ends in close 4401. The key is checked against this socket's own session hash
  and user (`auth::socket_key_matches`).
- **Links** (`auth/links.rs`). HMAC-SHA256 with a 32-byte key made at process
  start, over `harness-link\n<cookie hash>\n<expiry>\n<path?query>`; `verify`
  rebuilds the target from the request's own path and query with the one `link`
  parameter removed, refuses two of them, an expiry in the past or more than 30
  minutes ahead (server clock), and any path `linkable` does not name; the MAC is
  compared in constant time. The paths are media content and thumbnails and
  artifact download, preview and thumbnail, each a `GET` that returns bytes; ids
  are `[A-Za-z0-9-]` only, so no `..`, no encoded slash, no trailing slash. The
  login's row is read on every use, so a link dies with logout, expiry, a
  disabled account and a restart; another login's cookie fails the MAC. `mint`
  refuses a query that holds a parameter starting with `link`. Responses carry
  `Referrer-Policy: no-referrer`.
- **P-2.** My first proof (`review_p2_…`) now fails: no child exists when
  `denied_tools` names `delegate`. The approval is asked before `run_delegate`,
  per call, also for several in one step; the turn's allow set is keyed by the
  delegation's own input and is not passed to the child; `bound_delegation`
  (`node/src/policy.rs`) is applied in `Session::prompt` to the spec the master
  sends, at every level, and children take their spec from the bounded parent.
- **P-7.** `writes_to` leaves a deny out (`master/src/lib.rs:398-403`); deny and
  cancel use `require_any`; `visible_session` (owner equality) still runs before
  either, so a deny cannot reach another person's approval.

**Web UI, as far as read**
- **Markdown.** `react-markdown` with no raw-HTML plugin, so model HTML is text.
  `urlTransform` (`md/pipeline.ts:218-223`) keeps `http`, `https`, `mailto` and
  `#`; everything else becomes no link. Images are never fetched: `img` renders
  as a chip with the host (`md/Link.tsx`, `ImageChip`), and a `data:` image keeps
  only its scheme. External links get `rel="noopener noreferrer nofollow"`,
  `target=_blank`, the real host beside the text when the text does not show it,
  and a confirmation with the full URL when the URL came only from the model and
  could carry data. Hidden characters are stripped before parsing and again
  after it (`md/hidden.ts`). Footnote ids are prefixed per message
  (`clobberPrefix`).
- **Math.** Temml builds DOM nodes with `trust: false`, an expansion limit and a
  size limit; a last pass keeps an allow-list of layout attributes and drops
  `href` and `id` (`md/math.ts`).
- **Sinks.** No `dangerouslySetInnerHTML`, `innerHTML`, `eval`, `new Function`,
  `srcdoc` or `document.write` in `web/src` (grep). The one `window.open` opens a
  parsed http(s) URL with `noopener,noreferrer` after the confirmation.
  `postMessage` is only between the page and its own highlight worker. Dynamic
  `href`s are parsed and scheme-checked (`Cite.tsx:26-27`, `ToolCard.tsx:85-86`)
  or built from fixed hashes.
- **Headers.** Every response: the CSP above (`auth.rs:985`, with
  `frame-ancestors 'none'`, `base-uri 'none'`, `form-action 'self'`,
  `img-src 'self' data:`, `connect-src 'self'`), `X-Frame-Options: DENY`,
  `nosniff`, `no-referrer`. Stored files: their own sandboxed CSP, `nosniff`,
  `Cross-Origin-Resource-Policy: same-origin`, `no-store`, a cleaned
  `Content-Disposition`; an artifact preview is refused unless the stored kind
  is a raster image, audio or video (`http/artifacts.rs:640-660`), so no HTML or
  SVG artifact is ever rendered in the origin.
- **The key in the page.** One name, every access in `try`, removed on any 401
  and whenever `me` is null; sent as a header only by `call` and three `fetch`es
  to fixed same-origin paths, and as the socket's first message; never put in a
  URL; not logged (`console.error` in `live.ts` logs a handler's exception only).

## 4. What I did not check

- **Web UI, not read:** `md/UiHost.tsx` and the form cards (`ui:` blocks),
  `md/Code.tsx` and the highlighter's output, `md/Table.tsx`, `md/Cite.tsx`
  beyond its URL checks, `Conversation.tsx`, `Composer.tsx`, the routine, agent
  and settings editors, `MediaPreview.tsx` and `ArtifactDetail.tsx` beyond their
  `src`/`href` lines. So "no script from a model or node runs in the origin" is
  established for Markdown, math, tool cards and the grep of sinks, not for
  every component.
- No real browser: W-1's off-screen case, the cookie-and-port rule, CSP
  enforcement and `Referer` behaviour are from reading and from the tests named.
- The full `pnpm test` suite, `pnpm build`, `pnpm size`, `cargo clippy`,
  `cargo deny` and the full workspace tests were not run; only the proof tests.
- R2-2 and R2-3 are by reading, not reproduced.
- Timing differences finer than "a store lookup or not" between a valid cookie
  without a key and no login.

## 5. Proofs

`docs/security-review/proofs/2026-11-gate3-fixes-2.patch` adds tests only; each
**passes, and passing is the defect**.

```bash
# R2-1, on the fix branch
git checkout --detach fix/R.2-approval-findings
git apply docs/security-review/proofs/2026-11-gate3-fixes-2.patch
cargo test -p harness-master --test login_binding_http review_r2_1

# W-1, on the fix branch or on main (the web file applies to either)
git apply --include='web/*' docs/security-review/proofs/2026-11-gate3-fixes-2.patch
cd web && COREPACK_ENABLE_DOWNLOAD_PROMPT=0 CI=1 pnpm exec vitest run src/components/review-gate3.test.tsx
```

Run here on 2026-11-11: R2-1 passes on R.2; the six W-1 cases pass on R.2 and on
`main` (`6cd5b38`; on `main` pnpm needed `--pm-on-fail=ignore` because the
installed modules were R.2's). The first review's P-2 proof, appended to
`crates/node/tests/runtime.rs` on R.2, **fails** ("the denied delegation ran":
0 children).

## 6. The three things I am least sure of

1. W-1's severity for `bash` on a sandboxed node: the sandbox bounds what an
   approved command can do, so the High rests on `write` and `edit`, which run
   in the node process, and on nodes with the sandbox in `best-effort` or `off`.
2. That nothing in the unread components (section 4), `ui:` forms above all,
   lets model or node text become markup or a URL. The key in `localStorage`
   depends on it.
3. P-1's premise itself was still not run in a browser by anyone (the author
   used `curl`'s cookie jar, I used a non-browser client): that a browser hands
   the cookie to another port of the host. RFC 6265 says so; a Playwright run
   with two listeners would settle it, and R2-2/R2-3 are by reading only.

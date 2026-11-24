# Authentication by construction (FIX_PLAN 4.1, second half)

Goal: on every route, the caller is identified before anything of the request is read, and a new route cannot forget it.
Status: stage 0 (design and spike), 2026-11-24.
The spike is kept: it already makes the route walk's `BODY_FIRST` and `QUERY_FIRST` lists empty.

## What authenticates a request today

Read at `59c8b3c9` (branch `test/route-walk`).

| Mechanism | Where | Needs from the request | Returns | Uses |
|---|---|---|---|---|
| `auth::authenticate` | `auth.rs:822` | headers: `Authorization: Bearer` (the API token, constant time) or the session cookie plus `x-harness-login`, one indexed lookup (`by_cookie`, `auth.rs:842`) | `Option<Principal>` | called by `require_any`, `require_bytes`, `favicon.rs:68`, `artifacts.rs:498` (to tell a link download), `ws.rs:261` |
| `require_any` | `http.rs:379` | headers | 401 "not logged in" | 8 calls in handlers (`me`, the password change, approval mode, chat allows, step limit, and others that must work during a pending password change) |
| `require` | `http.rs:386` | headers | `require_any`, then 403 "change your password first" | 111 calls in handlers |
| `require_admin` | `http.rs:397` | headers | `require`, then 403 "admins only" | 34 calls |
| `require_bytes` (`links.rs:60`) and `auth::link_principal` (`auth.rs:891`) | byte routes | headers and the URI: the key or token as above, or a live login's cookie plus a link minted for that login and this exact path and query (`auth/links.rs:159`; `linkable` at `auth/links.rs:40` limits links to media content and thumbs, artifact download, preview and thumb, the latest download, a made file, a profile picture and the favicon) | 401 / 403 as `require` | 7 calls, plus `favicon.rs` by hand |
| `require_agent_editor` | `agent_edits.rs:31` | headers and the query (`?scope=server` needs an admin, `?workspace=` the workspace's settings permission) | principal and place | 4 calls, each after `require` |
| `visible_session` / `acting_session` / `agentic_gate` | `http.rs:419,438,463` | the path's chat id and a principal | 404 for a chat that is not the caller's, the T85 mark, 403 for a blocked mode | authorisation, not authentication: they take a `Principal`, never headers |
| the Origin guard | `auth.rs:930` | headers | 403 host / origin not allowed, origin required | an outer layer on every request, unchanged |
| the webhook secret | `hooks.rs:87,179` | the body limit first (by design, T-row in the module), then the trigger id and its bearer secret | 401 "unknown webhook or wrong secret" | public route |
| setup, login, logout | `http.rs` | the body (claim code, password) or the cookie alone (logout, R2-3) | their own answers | public routes |
| the WebSocket | `ws.rs:252` | headers, then its first message | 401, or a socket that waits for its key | public route |

By hand: 160 calls in handlers (153 of `require`, `require_any` and `require_admin`, 7 of `require_bytes`) plus the favicon's own, in 46 files (`http.rs` 33, `http/workspaces.rs` 16, `artifacts.rs` 14, `routines.rs`, `providers.rs` and `memory.rs` 11 each, the rest 1 to 6); counted with `git grep -E "(^|[^.a-z_])(require|require_any|require_admin|require_bytes)\((&|m,|master,)"` at `59c8b3c9`, less the two calls inside the helpers themselves and `workspaces.rs`'s unrelated `Access::require`.
No route authenticates differently depending on its body except the public ones (setup's claim code, login, the webhook secret): the route walk proved that every other route answers 401 to every credential-less variant once its input parsed.

## Candidates, attacked

**1. Extractors only (the plan's wording).**
`Caller`, `User`, `Admin` as `FromRequestParts`, each doing the lookup itself, placed first in every handler.
It works: axum runs the arguments in order and every `FromRequestParts` before the one body extractor (`axum-0.8.9/src/handler/mod.rs:221`).
But it fixes a route only when its handler is edited, and the order among the parts extractors is the author's to get right: a `Query<T>` or `Path<(String, u64)>` written before the extractor is the attachment upload's finding again.
Forgetting it is caught only at test time, by the route walk.
Three routers (public, user, admin) do not help as written: a router cannot tell a handler's arguments, and the role is not always a property of the route (`require_agent_editor` decides from the query whether an admin is needed).

**Compile-time enforcement (B1), tried.**
It is expressible without `unsafe` and without a macro: axum's `Handler<T, S>` encodes a handler's arguments as `T = (M, T1, …, Tn)`, so a trait `FirstIs<A>` implemented for `(M, A)`, `(M, A, B)`, … (one hand-written impl per arity, 16 in all) and a pass-through `fn checked<A, H, T, S>(h: H) -> H where H: Handler<T, S>, T: FirstIs<A>` make `get(checked::<Admin, _, _, Master>(handler))` fail to compile when `handler` does not take `Admin` first.
The spike compiled it for `storage.rs` and got a clear error for a wrong first argument (E0631, "expected function signature `fn(caller::Admin, _) -> _`, found `fn(caller::User, State<Master>) -> _`"; log in the stage-0 notes).
It is not worth having: the check is opt-in per registration, so forgetting `checked` is the same mistake as forgetting the extractor, and nothing but a source-reading test could insist on it.
The spike code was removed.

**2. One layer that identifies before any extractor, extractors that only read what it found.**
`http::caller::identify` is a `route_layer` on every non-public route (`http::private`).
It runs after routing and before the handler's first extractor, so 401 comes before the path, the query and the body, whatever order the handler's arguments are in.
It looks the caller up once (the API token, a login with its key, or for a GET or HEAD a cookie with a link) and stores it in the request's extensions.
`Caller`, `User`, `Admin` and `ByteCaller` read it, cannot fail for a lookup reason, and add only the route's rule with today's words ("change your password first", "admins only").
An extractor on a route outside the layer finds nothing and answers 401, so that mistake shuts the route instead of opening it.
A route cannot forget to be authenticated: being on the private router is being authenticated.
The only way out is `http::public`, five lines in one function, and the route walk's `PUBLIC` list fails any route there that is not written down with its reason.

**3. Both** is option 2: the layer gives "401 before anything", the extractors give the typed principal without a second lookup.

**Verdict: do it differently from the plan's wording: option 2/3, not three routers.**
Evidence from the spike: one layer and a split of `router_with` into `public()` and `private()` emptied both known-finding lists, all 86 routes at once, with no handler touched (`every_route_answers_401_without_credentials` failed on the old code with 468 findings on 86 routes, and passes now).

### Is "401 before the body" enough, or must 404 for another person's chat also come first?

It is enough.
A member who sends a malformed body to another person's chat gets 400 or 422 before the handler's `visible_session` answers 404.
That answer depends only on the body, never on the id: the same malformed body gets the same answer for their own chat, another person's and one that does not exist, so it confirms nothing (T10's promise is that an id is not confirmed).
With a valid body, all three that are not the caller's get the same 404 as today.
Moving the 404 into the layer would mean the layer parses each route's path and looks up the chat: route knowledge in middleware, for no information gained.

## What an unauthenticated upload costs now

Nothing of its body is read.
The layer answers before the body extractor and the unread body is dropped.
hyper 1.12 then drains what is already buffered (`hyper-1.12.0/src/proto/h1/conn.rs:876`, `poll_drain_or_close_read`): a body that arrived whole is consumed by its framing and the connection is kept for the next request; a body that has not arrived is not read, and the read side is closed, so the connection ends after the 401.
The drained body is never read as a request: `a_refused_body_is_never_read_as_a_request` sends a refused POST whose body is itself a request, with `Content-Length` and with chunked framing, then a real pipelined request, and gets exactly two answers (the review's probe found the same for `Expect`, both framings at once and a GET with a body).
`Expect: 100-continue` is answered with the 401 and no `100 Continue`, since hyper sends that only when the body is first polled (`conn.rs:412`).
Test: `an_unauthenticated_upload_is_refused_unread` (a 1 GB `Content-Length`, with and without `Expect`): on the old code the server answered `100 Continue` and kept the connection open waiting for the body; now 401 and a close, since the body had not arrived.
Per-route body limits (`DefaultBodyLimit` layers) are unchanged: they only tell the body extractor its limit, so they now apply to identified callers only.

## Behaviour that changes, and what does not

Unchanged for identified callers: status codes and sentences (the extractors use the old words), the Origin guard (an outer layer, runs first as before), the request time limits (an outer route layer), the throttles (keyed in the handlers after the principal, untouched), the API token's rights, and the byte routes (`a_link_counts_only_for_the_byte_routes`: a link gets a made file's download past the layer, the same link on another URL is 401, a link on a PUT to a linkable URL is 401).
Changes: an unauthenticated caller now gets 401 where it got 400, 413, 415 or 422.
A deliberate change in which error wins (decision 2 below): once a handler converts, a signed-in caller who lacks the right (a member on an admin route, a login with a pending password change) and also sent a bad body gets the 403 where it got 400, 413, 415 or 422, because the extractor runs before the body.
A member no longer learns an admin route's fields that way.
The member walk lists the handlers still in the old order (72 routes for one caller kind or another today); the list shrinks as they convert.
Until a handler converts, it still calls `require` after the layer: two lookups per request (the second is a read, the login's expiry moves at most once a minute), which goes away with the conversion.

## Splitting the work

1. **The layer** (this spike, ~+140 lines in `http/caller.rs`, +38/-17 in `http.rs`, the tests): fixes every route at once; the walk's lists are empty from here on, so the later steps cannot be proven by those lists shrinking.
2. **Convert the handlers by area**, one pull request each, mechanical: the `require*` call and the `headers`/`uri` arguments go, an extractor comes first in the arguments.
   Per route it is about -2 lines (the spike: `storage.rs` +6/-8 for three routes, `made_files.rs` +5/-5 for one byte route).
   Suggested slices, by call count: `http.rs` (33); workspaces, levels, users, plugins, skills (29); the byte routes: artifacts, media, made files, profile, links, favicon (23 and the favicon); providers, pricing, storage, usage, memory (27); routines, projects, agent edits (20; `require_agent_editor` takes a `User` instead of headers); the 25 one- and two-call chat files (28).
   Helpers that take headers to call `require` (`media::caller`, `profile::account`, `require_agent_editor`) are changed to take the principal.
   Proof per step: the area's suites and the walk stay green, and the count of `require` calls goes down.
3. **Delete `require`, `require_any`, `require_admin` and `require_bytes`** in the last step, so the compiler proves "no handler calls `require*` by hand" (the plan's done criterion).

## Decisions (session lead, 2026-11-24; the owner may overrule any)

1. The layer and the four extractors replace the plan's three routers; FIX_PLAN 4.1 says so.
2. Once handlers convert, a signed-in caller without the right gets 403 before a bad body's 400 or 422 (above); T2 says so.
3. A member walk lands with the layer (`crates/master/tests/it/member_walk_http.rs`), below.
4. A link authorises a GET or HEAD only, in the layer and in `ByteCaller` (`a_link_passes_the_byte_extractor_only_to_read` for the extractor alone, which the layer otherwise hides).

## The member walk and the agreement walk

`member_walk_http.rs` reads the route table as the route walk does, resolves each registration to its handler function (through closures, `use` imports and helpers in the same file), and reads that function's checks: `require_admin` or `Admin` is admin-only, `require_agent_editor` needs an admin for `?scope=server`, `require_any` or `Caller` works during a pending password change, `require_bytes`, `ByteCaller` or `link_principal` take a link.
A handler it cannot find, or one with no check at all, fails the test.
The routes it reads as admin-only (43 today), as open to a login with a pending password change (`ANY_LOGIN`, 9) and as taking a link (`BYTE_ROUTES`, 10) must each equal a written list, so a handler that loses its admin check when it converts fails until someone changes that list on purpose; nothing under `/api/admin/` may be open.
Then every route, every registered method, as:
a member (empty, small JSON and 1 MB bodies: 403 or 404 on admin-only routes, never 401, never "admins only" elsewhere, never 500);
a login whose password must change (403 "change your password first" except where `require_any` lets it in);
a disabled account's still-stored login and an expired login (401);
a member's cookie with a link on every write (401);
and the API token, the machine owner's root key with admin rights and every session (`auth.rs`): never 401, never "admins only", never "change your password first".
Each admin-only route also gets a body it parses (`VALID_BODIES`, from its body type's required fields), so a member's request reaches the handler's own admin check even while that check still comes after the body: all 43 answer a member 403 or 404, none is left unreached, and an admin route added without such a body fails.
Only checks on a handler's straight-line path count: one inside `if`, `else`, a loop or a braced closure, or after a top-level `return`, does not (tested on written shapes); what a lexer cannot see, a branch dead by value or a `match` arm never taken, is left to the golden file and to the walk asking the server.
Proven able to fail: an `Admin` extractor that let members through made it fail on `/api/admin/storage`.
The "admin" in module docs was tried as a second source and dropped: 83 open routes match it, because the docs say "admins included" of rules that keep admins out of other people's things; the `ADMIN_ONLY` snapshot does that job.

**The golden file.** `tests/it/auth_golden.txt` (4,296 lines, 262 KB) holds what `require*` answered before the layer, one line per request: caller kind, method, path, body (none, empty, `{}`), status and error sentence.
It was written by `write_the_auth_golden` (`HARNESS_WRITE_GOLDEN=1 cargo test -p harness-master --features testing --test it write_the_auth_golden`) against `router_before_identify`, with `storage.rs` and `made_files.rs` put back to their bodies before conversion (from `59c8b3c9`) for the run, so every line is an answer of `require*`; two runs gave the same file.
Kinds: nobody, a wrong token, a made-up cookie, a member's cookie alone, a foreign page, a member, an admin, a login that must change its password, a disabled account's login, an expired login, the API token, and a cookie with a link (its own on a byte route, carried elsewhere), with HEAD on the byte routes.
`the_layer_answers_as_require_did` sends the same requests to the router with the layer and allows only two differences, matched by rule: a caller who is nobody gets 401 where a body error came first (896 lines today), and decision 2, a member or a login that must change its password gets 403 "admins only" or "change your password first" where a body error came first (2 today, the converted `PUT /api/admin/storage`).
Every other change fails it: widening `storage.rs`'s `mine` from `User` to `Caller` failed with `now must-change GET /api/me/storage none 200 / was ... 403 change your password first` (the review's mutation E, which the member walk alone missed).
`router_before_identify` and the writer exist only with the crate's `testing` feature, which no shipped build turns on; once a handler converts it fails closed there (the extractors find no principal), so the file cannot be regenerated after conversion begins and is frozen.

`the_origin_guard_answers_first` pins the guard's place: another site's write with no credentials is 403 "origin not allowed", a cookie write without `Origin` is 403 "origin required", the page's own GET and login pass through.
`the_public_router_is_exactly_the_public_list` reads `http::public()`: moving `GET /api/me` there made it fail while the route walk stayed green (the handler still checks its caller), which is why it exists.

## Review (2026-11-24, `r-auth-first`) and what was done

The independent review found no way past the layer that the old code refused, link principals refused by `Caller`, `User` and `Admin`, and no request smuggling.
Its findings, each closed in this pull request:
- A route that gets wider access during conversion passed the member walk (mutation E): the golden file, and the `ANY_LOGIN` and `BYTE_ROUTES` lists beside `ADMIN_ONLY`, now catch it (both fail on mutation E).
- The admin check of unconverted admin routes with a body was never asked over HTTP: `VALID_BODIES` reaches all 43.
- A dead check counted as a check: only straight-line calls count now (above).
- The layer read the store for every private GET with a cookie and no key (`cookie_user` ran before the link's MAC): `link_principal` now checks the link first, and `a_link_is_checked_before_the_store_is_read` counts the store reads (it failed with 4 reads before the change, 0 after).
- "The connection is closed after the 401" was true only for a body not yet buffered: reworded above and in T2, with the no-desync test.
- `router_before_identify` was `pub` in shipped builds: now behind the `testing` feature.
- Mutations A (the layer takes a cookie alone as a link) and C (a link on every method) were each caught by one test: `the_layer_takes_a_link_only_to_read` (a unit test of the layer's own verdict, seen by a handler that does not check again) is the second catch for both; for A the golden test (a member's cookie alone on the made-file download: 404 where it was 401) and the store-read test fail too.
  No walk over the real routes can catch C, since every handler on a linkable URL that writes checks again; that is the defence in depth, and the unit test is the catch.

## Rules for the conversion pull requests

- Keep `the_layer_answers_as_require_did` green with only its two difference classes; never edit `auth_golden.txt` (the only allowed edit is deleting the lines of a route that is removed).
- Edit `ADMIN_ONLY`, `ANY_LOGIN`, `BYTE_ROUTES` or `VALID_BODIES` only with a line of reason in the pull request.
- The route walk, the member walk and the area's own suites stay green; the count of `require*` calls goes down.
- A route added after the golden is listed by the golden test, not compared; it is covered by the route walk and the member walk, and should be written with the extractors from the start.

## Open questions

- None from stage 0; the four decisions above stand unless the owner says otherwise.

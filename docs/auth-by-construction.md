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
The layer answers before the body extractor, the unread body is dropped, and hyper 1.12 then drains only what is already buffered and otherwise closes the read side (`hyper-1.12.0/src/proto/h1/conn.rs:876`, `poll_drain_or_close_read`), so the connection is closed after the 401 instead of reading the rest.
`Expect: 100-continue` is answered with the 401 and no `100 Continue`, since hyper sends that only when the body is first polled (`conn.rs:412`).
Test: `an_unauthenticated_upload_is_refused_unread` (a 1 GB `Content-Length`, with and without `Expect`): on the old code the server answered `100 Continue` and kept the connection open waiting for the body; now 401 and a close.
Per-route body limits (`DefaultBodyLimit` layers) are unchanged: they only tell the body extractor its limit, so they now apply to identified callers only.

## Behaviour that changes, and what does not

Unchanged for identified callers: status codes and sentences (the extractors use the old words), the Origin guard (an outer layer, runs first as before), the request time limits (an outer route layer), the throttles (keyed in the handlers after the principal, untouched), the API token's rights, and the byte routes (`a_link_counts_only_for_the_byte_routes`: a link gets a made file's download past the layer, the same link on another URL is 401, a link on a PUT to a linkable URL is 401).
Changes: an unauthenticated caller now gets 401 where it got 400, 413, 415 or 422.
After the handlers convert, one more, which needs a yes: a signed-in caller who is not allowed (a member on an admin route, a login with a pending password change) and also sent a bad body gets the 403 where it got 400 or 422, because the extractor runs before the body.
That is the better order (a member no longer learns an admin route's fields) but it is a change in which error wins.
Until a handler converts, it still calls `require` after the layer: two lookups per request (the second is a read, the login's expiry moves at most once a minute), which goes away with the conversion.

## Splitting the work

1. **The layer** (this spike, ~+140 lines in `http/caller.rs`, +38/-17 in `http.rs`, the tests): fixes every route at once; the walk's lists are empty from here on, so the later steps cannot be proven by those lists shrinking.
2. **Convert the handlers by area**, one pull request each, mechanical: the `require*` call and the `headers`/`uri` arguments go, an extractor comes first in the arguments.
   Per route it is about -2 lines (the spike: `storage.rs` +6/-8 for three routes, `made_files.rs` +5/-5 for one byte route).
   Suggested slices, by call count: `http.rs` (33); workspaces, levels, users, plugins, skills (29); the byte routes: artifacts, media, made files, profile, links, favicon (23 and the favicon); providers, pricing, storage, usage, memory (27); routines, projects, agent edits (20; `require_agent_editor` takes a `User` instead of headers); the 25 one- and two-call chat files (28).
   Helpers that take headers to call `require` (`media::caller`, `profile::account`, `require_agent_editor`) are changed to take the principal.
   Proof per step: the area's suites and the walk stay green, and the count of `require` calls goes down.
3. **Delete `require`, `require_any`, `require_admin` and `require_bytes`** in the last step, so the compiler proves "no handler calls `require*` by hand" (the plan's done criterion).

## Open questions

- The 403-before-the-body order for signed-in callers who lack the right (above): recommend yes.
- A forgotten `Admin` on an admin handler is authorisation, not this fix, and neither design catches it at compile time; recommend a member walk next to the route walk (a plain member's login against every `/api/admin/*` and `/api/users*` route, expecting 403), cheap and source-driven like the route walk.
- Keep the layer's GET/HEAD rule for links, or let `ByteCaller` alone decide: recommend keeping both (the layer refuses a link on any write even if a handler later takes the wrong extractor).

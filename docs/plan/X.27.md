# X.27 **sec**

## Adds

**Steps 1, 2 and 3 of 4 done (2026-11-18, 2026-11-19); step 4 (push) left, so the row is not Done:** (1) a person's own connection by a pasted token, sealed at rest (C.5's sealing), for `github.com`, `gitlab.com` and hosts the admin lists in `config.toml`, under Plugins → Connectors (README "Connections", PLAN.md T83); (2) the API only, no screen: `GET /api/connections/{id}/repositories?q&page` lists, from the master, the repositories the owner's connection can see (GitHub `/user/repos`, filtered here for a search word since GitHub has none on that endpoint; GitLab `/projects?membership=true`; Gitea/Forgejo `/repos/search?uid=`), names and branches validated, 30 per page, 10 pages, cached a minute, limited per person and to 2 in flight on the server (`crates/master/src/forge_repos.rs`, PLAN.md T83). (3, API and node, 2026-11-19, PLAN.md T86, README "Cloning a repository") `POST /api/projects` with `folder_on`, `connection_id` and `full_name`: the master checks the caller's own connection and asks the forge for that repository by name (one call), then sends `project.clone` (only to a node that announced `forge-clone:<limit>`; the token only for a private repository); the node clones only from its node.toml's `[forge] hosts`, builds the URL itself, gives the token to that one git through a pipe read by a credential helper that answers only for that host and path (not §4.7's `extraHeader`, which leaked on a redirect and through `/proc/<git>/environ` in the experiment), refuses redirects, and builds the clone in its staging folder within `[limits] clone_timeout_s` and `clone_max_mb`; no submodules, no LFS; the project records `source` (host, name). (3, the screen, 2026-11-19, branch `feat/forge-clone-screen`) the Project popup's "From a repository" (`web/src/components/RepoPicker.tsx`, a lazy chunk): the person's connection, the list with a search that waits about 300 ms, cancels the request before and takes our own 429 as still loading, GitHub's within-page search said in words with "Search the next 100" on request only, forge text as one line of inert text; `POST /api/projects` with a clone answers `202` at once with the project pending (`project_clones`, not a project: no route that takes a project id finds it), drawn as cloning in the popup and the sidebar, a failure with its reason and Dismiss for its starter only, every running clone failed at the master's start (PLAN.md T77, T86; README "Cloning a repository").
Left: (4) push.
**Requirements on step 3's screen (review of PR 163, L6):** `POST /api/projects` with a clone blocks for the whole clone (up to the node's `clone_timeout_s`, plus 60 s) with no progress, and a reverse proxy will cut it while the project still appears later: the screen's pull request must make the request return at once with a "cloning" state and deliver the result as an event; plus the within-page search text, the debounce and the cancel listed below.
**A requirement on step 3's screen:** when an answer says `search: "within_page"` (GitHub), the screen must say that it searched the N most recently updated repositories (100 per page) and offer to search the next ones; it must not read further pages on its own, and a repository past the 1,000 most recently updated is never found by a GitHub search.
A search-as-you-type picker must wait until the person stops typing and cancel its previous request before it sends the next (one listing per person is in flight at a time; a cancelled request frees the slot).
(The owner, 2026-11-16, same message.)
**Forge accounts and repositories in the Project popup:** "ability to select an repository from supported forge platforms (like github, gitlab, gitea, forejo, + self-hosted variants where applicapable and ability to authorize own account from one of the platforms to parse private repos and push as auth user)".
This is N.6 (Connections) and `docs/nodes-credentials-environments.md` §4.7 pulled ahead of N.1 to N.5, on today's model.
What existed before step 1: nothing of it.
The node's git refused every transport (`protocol.allow=never`, `credential.helper=`, `crates/node/src/git.rs`), no code can clone, fetch or push; provider keys are a server-wide `secrets.toml` (0600), not a per-user store, and the sealing of C.5 is not built; the master's outbound client allows https to public addresses only, with same-origin redirects.
To build, in this order: (1) a per-user connection by a pasted access token (works on every forge, self-hosted too, and needs no registered application; from the forges' own documentation: device authorisation exists on GitHub and on GitLab from 17.9 and needs a client id, Gitea and Forgejo offer the authorisation-code flow only), sealed at rest; (2) listing the user's repositories through the forge's API from the master, for hosts an admin allowed; (3) a clone on the node into a project's folder (X.26); (4) a push as that user.
Device flow can follow where a client id is configured.

## Safeguards and abuse tests

The token never appears in argv, a URL, an event or a log line: §4.7's path (the URL given explicitly, never the repository's `remote.*`; `[forge] hosts` and, for step 4, `[forge] push` in node.toml).
Established by the experiment before step 3 (2026-11-18): an `http.<url>.extraHeader` in git's environment reaches a redirect's other host and sits in `/proc/<git>/environ`, so the clone uses a pipe and a host-checking credential helper with redirects off (T86); a process of the node's user that reads `/proc` can still reach the pipe while a clone runs.
A push is its own per-agent switch and asks every time; the agent's own shell never sees the token.
Abuse tests: a repository whose `remote.*` or `.gitmodules` points elsewhere, a redirect to another host, a forge host on a private address, a token of user A used by a session of user B or by an admin, a prompt-injected agent asking for a push, a hostile master asking a node for a host that is not in its list.

## Invariants

No secret in an event, a URL or a log line; the node decides; chats and connections are private to their owner, for admins too; the master never runs tools (the node clones and pushes).

## Podman / Node

No / Yes

## Depends

X.26. **Decisions (2026-11-17).**
Five questions were put to the owner with a recommendation each.
The owner answered one in their own words, on whether the agent's own `git push` may see the token: "No, git credentials are managed by the node in memory so agent wants to push or access something private through the credential? use tools rather than give agent it self the token".
So: the node holds a forge credential in memory only (never written to the node's disk, never in the sandbox's environment or files); the agent reaches a private repository or pushes only through tools the node runs with that credential, each behind its own per-agent switch and the approval path.
The other four stand as recommended until overruled: a pasted access token first (it works on all four forges and self-hosted ones with no registered application), device sign-in later where a client id exists; the chat owner's own connection pushes (§4.7 wrote the workspace's account for autosave only); allowed forge hosts are listed twice: by the server admin for listing repositories from the master, and in node.toml for clone and push; sealed storage on the master (C.5) is built first, not a plain 0600 file.

## Size

**L**


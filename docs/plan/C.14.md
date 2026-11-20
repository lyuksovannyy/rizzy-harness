# C.14

## Decision

Node update and drain (owner's direction and answers, 2026-11-12).
**Note: `docs/node-update.md`** (revised 2026-11-12 after the answers, recorded in its §2): §1 what exists and why the removed SSH upload is not back, §3 `update_mode` (`off`, `notify` by default, `restart`, `auto`) and `update_verify` (`"release_hash"` by default, `"none"`), §4 build identity, the master's own build and the cache for other targets, §5 the endpoint on the node link, §6 verification, install, rollback, the trust given up per `update_verify` and the release hash, §7 the drain, §8 protocol, events, `local`, supervisors and the draft threat row T35, §9 tests, §10 build order, no open question

## What the note settles

The node pulls a compatible build (the master's own) from its master over the authenticated link, only when its own `node.toml` says `auto`; the master never pushes.
With `update_verify = "release_hash"` the node fetches the binary's hash from the project's GitHub release itself (a URL it builds from its own `node.toml` and a plain version) and installs only on a match, compared before and after the download; with `"none"`, for untagged and self-built servers, a compromised master runs code on those nodes, stated as the price.
No fallback from `"release_hash"` to `"none"`.
An offline signing key is planned, as a third value `"release_signature"` that then becomes the default (note §6.5).
Staged and preflighted while serving, renamed in after the drain, rolled back on a failed handshake.
The drain as decided, started also by a stop signal.
Before the first tag: same target and admin-placed builds only.

## Tests it commits to

The note's §9: a node not in `auto` never requests a build; the master sends nothing unasked; the endpoint refuses without node authentication, a revoked node and another target; wrong hash, target, identity or an older build refused with nothing installed; a build whose hash differs from the release's refused before any download; no URL or hash source taken from the master; a version with path characters refused;
GitHub unreachable leaves `notify` and never skips the check; a self-built master's build refused under `"release_hash"`; `auto` without `update_verify` checks the release and an unknown value refuses to start; a failed download causes no drain; a failed first handshake rolls back; a SIGTERM drains and a second forces; plus the drain tests (pause and continue, the five-minute stop, the forced cancel, restores, events, routines, abuse).


# N.9 **sec**

## Note phase

`docs/node-update.md` §10

## Adds beyond the note

Steps 2 to 8 after Gate 3, on A.19: build identity; the full drain (note, `drain_answer`, events, Continue, routines); the requirement and `off`, `notify`, `restart`; the build endpoint on the link and the master's own build; the node side of `auto` (fetch, verify, preflight, install after the drain, rollback, `update_verify = "none"`); `update_verify = "release_hash"` (the hash fetch, the release workflow's `.bin.sha256` asset, immutable releases on, from the first tag); the cache (`node-build add`; the release fetch after the first tag); the offline signing key is planned as its own task after the first tag, not built here

## Abuse tests beyond the note's

The note's §9; with N.1 the spool flush and a notice per attached workspace, with N.4 containers stopped and reconciled

## Invariants

Node decides (`update_mode`, `update_verify`; the trust given up in `auto` is the owner's yes of 2026-11-12, written into `PLAN.md` T5); the node pulls, the master never pushes; one writer

## Podman / Node

No / Yes

## Depends

A.19, C.14

## Size

**L** (about five weeks in all)


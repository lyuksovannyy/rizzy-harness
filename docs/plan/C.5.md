# C.5

## Decision

Credentials, pairing, aliases (note amendments)

## What the note settles

§4.1: AES-GCM associated data binds `workspace_id` and `name` (Warp's idea, not its code).
**Sealing built (2026-11-18, X.27 step 1, `crates/master/src/sealed.rs`, PLAN.md T83):** the associated data binds the table, the owner and the row's key fields, used now for `user_connections`; workspace secrets will use the same module. §4.2: strict fallback as `Option<Vec<_>>`; per-slot timeout and effort; slot names as validated strings; a slot failure never fails the turn; a slot on another provider needs its own grant route. §4.4: burn the code before any check; the master's code endpoint refuses foreign `Origin`, gives one failure reason, limits by peer. §9: "fallback" becomes the explicit `--systemd-creds` flag.
Pairing is written per attachment (C.12). `05` §6.6; `09` §6.4.

## Tests it commits to

Sealed row copied across workspaces fails to open; empty strict list fails with a reason; rejected attempt burns the code; failed title slot leaves the turn running.


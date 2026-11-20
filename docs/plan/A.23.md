# A.23 **sec**

## Fixes

Independent review of the project-MCP change (proof `review_scrubber_breaks_other_users_events`): with `[mcp_servers.*] env` values `approval_mode`, `message_id`, `function`, another user's `session_created` was stored with a `"[REDACTED]"` key, later events did not decode and the node did not start again (`Corrupt("… has no session_created")`). Its review (r-scrubber): kept fields matched by bare name kept outside text (a call id in `continued_by`, a repository path in a section's key), the cost had no bound (7.8 s on a 2.2 MB event), scrubbing was not idempotent, map keys could merge

## Change

**The scrubber keeps the schema** (PLAN.md T3). `SecretSet::scrub_event` learns, once per kind and path, what each position is from the event's own types (field names, tags, enum values and typed ids never change; below a `serde_json::Value` all is free), and scrubs all text except an explicit allowlist of kept positions by kind and path (`KEPT`), each named with the code that reads it back.
The allowlist fails closed: a forgotten position, a new field included, is scrubbed.
Kept positions are never changed, in whole or part (a path prefix such as `HOME` would otherwise break the session).
Keys never merge, keep their order; marker parts are not secrets; the person's message is scrubbed once.
Rejected: a list of FREE fields per kind (a forgotten field would go unscrubbed), whole-value matches only in kept positions (path prefixes), dropping word-like env values at load (a short password can look like a word).

## Done when

**Done (branch `fix/scrubber-keeps-the-schema`, PR 153):** `crates/core/src/secret/schema.rs`; tests `crates/core/tests/scrub_event.rs` (every kind under every schema word against a hand-written oracle, the allowlist against every kind's text positions, one class per path across instances, a key in every free position, call ids and `continued_by` alike, section keys, usage by model, colliding keys, scrubbing twice, random secret sets on the fold fixture), `crates/node/tests/scrubber_schema.rs` (the reviewer's sequence with admin-written values, restart, the message stored as sent, key order with `preserve_order`), `crates/agent-native/tests/section_keys.rs`; cost `crates/core/tests/scrub_cost.rs`.


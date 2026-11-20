# X.18

## Adds

What a chat costs, estimated from prices the admin sets (the owner, 2026-11-15: the useful things other harnesses ship, "no guessing", usability first)

## Safeguards and abuse tests

**Done on branch `feat/cost-estimate`** (PLAN.md 2026-11-15 entry): each model call's usage is stamped with its model in `ModelRouter`, and `Usage.by_model` (at most 16 models) keeps per-model sums through retries, fallbacks, summaries, sub-agents, parallel answers and hand-off notes; prices per model per million tokens (input, cached input, cache write, output, each optional) and one server currency, set by server admins in Settings → AI providers, validated and refused when bad, stored with the model in `config.toml`, read by everyone with the model list; the chat header shows "≈ $0.42 (last turn $0.03)", "+ unpriced" when a model lacks a price for a class it used, and the side panel shows each sub-agent's; always labelled an estimate at current prices, not the bill.
No built-in price table (OpenHands' is stale the moment models change);
Claude Code and OpenCodex estimate locally from a table, Oh My Pi counts unpriced requests apart.
The dead `budget` agent field is removed.
Admin totals per person and model are built (Settings, Server, Usage; `GET /api/admin/usage`: numbers only, never per chat, computed from the log with no schema change).
Not done: spending limits (a check before a turn starts), filling prices from a provider (no listing we use has a documented price field).

## Invariants

One writer per session (the breakdown is in the turn's own event); nothing secret in an event

## Podman / Node

No / Yes

## Depends

X.16

## Size

**M**


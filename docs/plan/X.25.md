# X.25 **sec** (T73)

## Adds

The model's thinking kept and sent back the way each provider needs, so thinking models can use tools (PLAN.md R8; research: goose resends once without replayed thinking blocks when rejected, `research/round2/synth/09-subagents-models-retry.md` "goose"; Hermes classifies `thinking_signature` failures; OpenCode, Pi and goose drop thinking from summaries, `07-context-compaction-memory.md`)

## Safeguards and abuse tests

**Done on branch `feat/thinking-in-history`** (README "Thinking", PLAN.md T73): each step's thinking in `assistant_message.thinking` (Anthropic signed and redacted blocks exactly as received and in order, plain `reasoning_content` text), sent back only to the model that wrote it: Anthropic blocks while the request's prefix before them is unchanged (a SHA-256 recorded with them, mirroring Anthropic's prefix check), plain text always; scrubbed or oversized thinking kept for the person and never sent; a refused replay resent once without and recorded (`thinking_withheld`); one collapsed "Thinking" row per step in the chat.
Effort for Anthropic models stays off (R8: not checked live).


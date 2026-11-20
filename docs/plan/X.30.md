# X.30 **sec** (T75)

## Adds

Streamed text without the secret scrubber's lag (measured 2026-11-16: with the 64-character API token known, a provider sending one token every 3 s showed no text for 21.9 s, then the whole answer; `.claude/handoff-2026-11-15/b-slowprovider/observation.md` (c)).
None of the surveyed harnesses holds a stream back to scrub split secrets (research round 1 appendix item 10), so there is nothing to copy

## Safeguards and abuse tests

**Done on branch `fix/scrubber-hold-back`** (PLAN.md T75, T3): `StreamScrubber` holds back only text that could begin a secret: a constant `longest − 1` bytes (at most 30) for secrets under 32 bytes; for longer ones only from a place that agrees with a secret's first 4 bytes.
Ordinary text comes out of the piece that brought it.
The hold reads no more than those 4 bytes of a long secret, so where a stream cuts tells a watcher at most them (the exact hold would have given a byte-by-byte oracle: 39 of 40 bytes in 1398 streams).
No budget, no setting.
Also fixed: a secret that begins a longer one no longer lets the longer one's rest through (the held text is raw, never re-scrubbed).


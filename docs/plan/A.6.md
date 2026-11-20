# A.6

## Fixes

Exec 5; A5 rewritten

## Change

**Tell the model a call was cut off for length.** `Completion` carries the finish reason (read on docs.rs what genai 0.7.2 exposes per adapter).
On a length stop every call in the message is refused with "output limit reached; split the change", without the fragment; three in a row end the turn. `01` §6.4.

## Done when

Test D9: a `write` ends mid-JSON with `finish: length`; the error names the limit and holds no fragment; three in a row end the turn; one fixture per adapter (Anthropic, OpenAI, OpenAI Responses, Gemini) shows whether any returns a cut call as a valid object, which must also be refused.
**S**

## Lands on

PR 7


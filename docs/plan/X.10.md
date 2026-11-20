# X.10

## Adds

Tool-call checks (the owner, 2026-11-14)

## Safeguards and abuse tests

**Done on branch `feat/tool-call-checks`:** every call's arguments checked in the loop against the schema the model was offered, before the tool card, approval and run (`crates/tools/src/args.rs`): `type`, `properties`, `required`, `items`, `enum`, unknown keys where the schema lists `properties` and does not open the object; numeric strings and `"true"`/`"false"` repaired (Pi's repair-then-validate; Oh My Pi and DeepSeek validate too; `01` Pi §2, §4.1); an error names the field and type and is the call's result; keywords not implemented never refuse (MCP schemas). `unknown tool` lists the tools on offer. `bash` takes `workdir` (OpenCode, DeepSeek; R1 "Shell execution model"), resolved like a file tool's path, inside the session folder, part of the approval key, honoured by background commands.

## Invariants

`agent-native/tests/tool_call_checks.rs`, `tools/src/args.rs`, `cli/tests/background.rs` (background `workdir` in the sandbox).
Not done: `workdir` keyed by its real path rather than its spelling (a different spelling asks again, which is stricter, not looser).

## Podman / Node

Node decides

## Depends

No / No

## Size


## Column 8

**S**


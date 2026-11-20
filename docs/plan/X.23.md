# X.23 **sec** (T71)

## Adds

Images through `read` (research gap list "images through `read` (M)"; X.22 is the highest id in flight, so this is X.23)

## Safeguards and abuse tests

**Done on branch `feat/read-images`** (README "Images" under the file tools, docs/media.md "Images `read` returns", PLAN.md T71): `read` of a PNG, JPEG, WebP or GIF (told by its first bytes, never its name; SVG stays text) returns the image to a model listed with vision, as Claude Code, OpenCode, Oh My Pi and Cursor do (`research/round1/synth/01-loop-tools-editing-shell.md` "Read-tool capabilities and limits"); a model without vision gets a line, never bytes. 3.5 MiB a file, so its base64 fits Anthropic's 5 MB per image (Pi caps the base64 at 4.5 MB, OpenCode at 5 MiB; Claude Code refuses only over 128 MB and downscales, OpenCode and Oh My Pi take files up to 20 MiB and shrink them), the same cap for an attached image, and 8,000 px a side; no resizing (the upload path resizes in the browser, and the node has no image decoder: refused with a note instead, where Claude Code, OpenCode and Pi downscale).
The log holds a reference; the bytes read are kept with the chat in `session_images`, as the person's images are (OpenCode keeps image blobs content-addressed, `research/round2/opencode.sessions.md`; Oh My Pi moves images over 1 KiB to a blob store, `research/round2/synth/01-loop-dispatch-durability.md`), so a request rebuilds byte for byte.
A request shows the newest 4 (5 MiB together), with clearing and summaries dropping old ones (OpenCode's `boundImages` replaces images oldest-first past 25 MiB, Claude Code prunes old images in batches, DeepSeek harness puts placeholders in for the oldest: `research/round2/opencode.agents.md`, `research/round1/claude-code.md` 1166, `research/round2/deepseek-harness.agents.md` 159); 4 a step, 20 a turn.
A turn holds only its window's bytes in memory.
A request the provider refuses (400/413/422, not an overflow) is sent once more without its images; if that works, `images_withheld` keeps those calls' images out from then on (review of PR 130: one bad image had wedged the chat).
A summary is told an image was read (goose writes `[image: mime]`, `research/round2/goose.agents.md` 138).
Not done: resizing, PDFs, a thumbnail in the tool card, a vision bridge for text-only models (OpenCodex's sidecar), images through `@path` (still "a binary is named, not shown").


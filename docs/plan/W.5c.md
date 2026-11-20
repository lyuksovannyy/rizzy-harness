# W.5c (done)

## Covers

Rename and export a chat (2026-11-14)

## Change

The owner renames a chat in its header: a master-side update of the row and the search index (`PUT /api/sessions/{id}/title`, cleaned and capped), no event, since the node and the model never read the title; the log keeps the first.
Export as Markdown, built in the browser from the fold the chat shows, tool calls one line each, outputs only when ticked, in a fence they cannot close; lazy-loaded.
Seven harnesses export a session (S5 "Export of a session").

## Done when, and size

The store and HTTP tests (owner only, admins get 404, cleaned, capped, searchable after an index rebuild), the title's and the builder's component tests.
**S**


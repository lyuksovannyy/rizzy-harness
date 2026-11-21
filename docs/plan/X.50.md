# X.50

## Adds

Change an agent's model and its fallbacks from the agent's own page (the Models block was read-only: "Chosen per agent in Settings → AI provider").
Where the viewer may change the chain in the scope the page is opened in (a server admin at the server's scope; in a workspace whoever holds its `settings` permission, the one `PATCH /api/workspaces/{id}` and the agent edits ask for), the block is an editor: a main model or none of its own, up to 8 fallbacks in order, unavailable models marked, a Save that shows the server's answer.
No new route: the server's `PUT /api/admin/agent-models` with only this agent's entry, a workspace's `PATCH` with its settings read again just before and sent whole, so another agent's chain or another setting changed meanwhile survives.
A new agent's form has the same block and saves the chain right after the agent is made; if that fails the page says the agent exists and its models were not saved, keeps what was typed and offers a retry.
A Personal workspace has no settings: its server admin edits the server's chain, others read who sets it.
Not compared with the other harnesses: the owner asked for the same editor the Settings pages already have, on the agent's page.

## Safeguards and abuse tests

The server decides, as before: the page hides what the viewer may not do and the routes refuse it (403, 400 unknown model, 409).
`web/src/components/agentmodels.test.tsx`: the editor per scope and per kind of agent, the read-only view and its hint, main model, none of its own, fallbacks add, remove and reorder, the limit of 8, the repeat rule, unavailable models, each route's body, a stale-copy save, refusals that lose nothing, the create flow and its half-failure, a Personal workspace for an admin and a member.

## Invariants

The node decides; chats are private to their owner; nothing the model writes is trusted

## Podman / Node

No / No

## Depends

X.33

## Size

**S**

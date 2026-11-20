# X.29 **sec** (T74)

## Adds

Account menu, Profile and Personalisation (the owner's testing, 2026-11-16, with a sketch; the reference for the rows was another product's Personalization page: memory, a base style and four finer choices, custom instructions; Claude.ai and ChatGPT both give a profile and standing instructions for every chat)

## Safeguards and abuse tests

**Done on branch `feat/account-menu-profile-personalisation`** (README "Profile and Personalisation", PLAN.md T74, docs/agent-pages.md): the account menu's Workspace settings moved out of the Workspaces flyout into the main menu under Profile and Personalisation, icons on every item, Settings and Log out side by side at the foot (Log out in the destructive colour);
Profile with a banner and an avatar (cropped and re-encoded in the browser, checked by bytes and header on the server, in the blob store, the avatar served to who shares a workspace), About you (280 characters, in the settings line);
Personalisation with five reply choices (fixed server sentences) and personal instructions (1,024 characters), one standing section of the context block for the owner's own chats; a per-agent "Personalisation" switch, on by default;
Workspace settings gains an Agents tab (the Agents page's list in the workspace's scope).
Not taken from the reference: reference photos, voice, record history, per-feature search switches (here those are the agent's).


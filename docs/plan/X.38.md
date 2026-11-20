# X.38 **sec** (T88)

## Adds

Git works unattended in an agent's shell, with the person's identity (the readiness trial of 2026-11-19, item 3: `git commit` failed for lack of an identity in 4 of 4 sandbox coding chats, each agent invented another one, and every git command in a host chat warned and failed on `~/.gitconfig`).
The owner, 2026-11-19, asked "which identity should commits made by agents in the harness carry?": "My own name and e-mail".

## Safeguards and abuse tests

**Done on branch `fix/git-works-unattended`** (PLAN.md T88, T74, README "What an agent's commits are signed with", docs/nodes-credentials-environments.md §4.7).
The Profile page gets an optional **E-mail for commits** (checked by the master and again by the node); the chat owner's login, display name and that e-mail travel with each prompt, retry and callback, and the node gives the agent's commands a default identity (git's lowest configuration level, so an identity in the repository, `git -c` or the person's readable `~/.gitconfig` wins); without an e-mail, `<login>@users.noreply.rizzy.invalid`.
A sub-agent follows the root chat's owner.
Host chats: Landlock's denial of the home folder is deliberate and stays; git is given an empty global configuration where it cannot read the person's.
The node's own git is untouched; the address is in no event, audit or log line.
**S.**


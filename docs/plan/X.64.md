# X.64 **sec** (T19)

## Adds

The branch and the state of the work tree of a coding chat's folder, in its header (the owner's direction, 2026-11-14: usable for coding first; Claude Desktop and Codex show the branch beside the session, and here a person could not tell which branch a chat was on, or whether it had left changes, before sending a message or asking for a commit).
A small chip after "project · node · how commands run": the branch name, or "detached at abc1234", and, when files differ from the last commit, a count ("3 changed").
Nothing for a folder that is in no git repository the node reads, for a node that is away or older than the server, and for a node that denies `read` in `node.toml`.
It is read when the chat is opened, again when a turn ends, and on a click; it is never polled; a failed read keeps what is shown.
The chip's hover and screen-reader text spell it out in full.
The branch name is the repository's own text (whoever controls the repository wrote it): the node cuts it at 200 bytes and replaces control, line-separator and bidirectional characters, the master bounds it again, and the page cuts it to 80 characters with an ellipsis and draws it as text through the same hidden-character treatment as every other untrusted text; it is never a URL, a link or markup.
**What is counted.** Files that differ from the commit HEAD names (changed tracked files, staged or not, one entry per path, deletions included) plus untracked files no ignore rule hides, where a folder that holds nothing else is one entry (what `git status` lists); at most 1,000, shown "999+".
Untracked files count because a coding agent's main work is making new files.
The whole repository is counted, not only the chat's folder inside it, as `git status` run in a subfolder lists the whole repository; a chat in a subfolder shows the repository's branch.
It is not `git status`: the user's global ignore file is not followed (X.31, as for checkpoints), a file an attribute filter would change counts by its bytes, and a submodule or a nested repository is one entry.
The count is absent (not zero) when it cannot be told: a repository with no commit yet (the branch is still shown), a node with no checkpoint store or with `checkpoints_keep = 0`, a user index over 64 MiB, a timeout.
One typed request over the link, `session.git_state` (`req_id`, `session_id`; it names nothing else), answered by one `session.git_state_result` (the state, or none, or `busy`), only to a node that announced `git-state`; additive, no event kind, no schema change.
**No git process runs in the person's repository** (T19): the branch and the commit are read from the repository's files by our own code (`user_repo::head_label`: a plain `.git` directory, no gitfile, link or `commondir`), and the count is asked of the node's checkpoint shadow with a private copy of the user's index, as the published file's "dirty" flag is: `update-index --refresh`, `diff-index --name-only -z` and `ls-files --others --exclude-standard --directory -z`, each with the node's hardened environment and overrides, a 5 s timeout and 12 s for the three; `diff-index` passes `--ignore-submodules=all` (a gitlink in the user's index otherwise runs the nested repository's filter, T19), so changes inside a nested repository are not counted and a broken nested config leaves the count out; the two listings are read as they stream and cut at 1,000 names, killing git.
A linked worktree or a submodule (`.git` is a file), a bare repository and a corrupt `.git` show nothing: they are not read as repositories anywhere in the node (T19).
`GET /api/sessions/{id}/git` (`crates/master/src/http/git_state.rs`, waiting in `crates/master/src/git_state.rs`, 16 s, two places per account and 32 in all): the waiting place is held until the node answers or 16 s pass, even if the page aborted (a retry cannot keep the node's two places busy); owner only through `visible_session`; anyone else, admins and workspace admins included, gets the 404 of a chat that does not exist and the node is not asked; so does a plain chat.
The answer is `null` for everything the header shows nothing for; only "too many asks" is a 503, and the page keeps the chip it has.
The chip is a lazy component of the conversation chunk (the first load did not grow).
Measured (an ignored test, `git::state::tests::a_hundred_thousand_untracked_files_are_bounded`, warm cache on a tmpfs): 100,000 untracked files in a new folder are one entry and take 5 ms in all; 100,000 beside a tracked file are listed one by one and take 33 ms, the count capped at 1,000; 20,000 in a folder is an ordinary unit test; a slower disk is bounded by the timeouts, not by the tree.
Not covered: ahead and behind counts, the upstream, a stash count, a merge in progress, a worktree of a linked checkout, a branch name past 200 bytes shown whole, and a refresh when a person commits outside the chat while it sits open (the click does that).

## Safeguards and abuse tests

See [T19](../threats/T19.md).

## Podman / Node

No / Yes (the node reads; the master only relays)

## Depends

X.42

## Size

**M**

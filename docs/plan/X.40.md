# X.40

## Adds

Push from a host chat: the recipe and doctor's check (a trial found "there is no way to push or open a pull request from a chat").
Found by experiment with a throwaway key, `ssh-agent` and loopback `sshd`: a host chat pushes over SSH with `env_pass = ["SSH_AUTH_SOCK"]`, `[sandbox] write` on the agent's socket file (`read` is not enough) and `[sandbox] read` on `~/.ssh/known_hosts`; `gh` reads a folder named by `GH_CONFIG_DIR` once it is in `read`, and `GH_TOKEN`/`GITHUB_TOKEN` are passed by `env_pass`.
No defect found in the product, so no code path changed.
Not done: the push tool of X.27 step 4 (the approval card for a push, a token the agent never holds), the sandbox mode (no home folder and no socket there), `gh pr create` against a real forge, a token in the system keyring, a forge not yet in `known_hosts`.

## Safeguards and abuse tests

**Done on branch `feat/push-from-host-chat`** (README "Push and open pull requests from a chat"). `rizzy-harness doctor` prints a "push from a chat" block (`crates/cli/src/push_check.rs`): the agent socket (passed, set, reachable), `known_hosts`, `~/.gitconfig`, a `gh` token variable and `gh`'s folder, each with the node.toml entry that would change it; from node.toml and the node's environment only.
No threat row: nothing a command can reach changed.


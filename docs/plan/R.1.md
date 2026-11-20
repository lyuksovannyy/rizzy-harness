# R.1 **sec**

## Findings

S-1 (Critical), S-2 (High), S-3 (High), S-4, S-6

## The change

**Nothing the agent's shell writes may steer the node's own git or move a session's folder.**
Sub-agent snapshots and the patch are taken in a harness-owned git directory with the hardened settings, never under the user's repository config; the child's grant on the main `.git` is narrowed; a session's folder is pinned by canonical path, device and inode and refused when it changed; the checkpoint root is found by walking up for `.git`, never taken from git configuration, and goes through the same checks as a session folder; a restore refuses protected folders.

## Done when

The review's proofs for S-1, S-2 and S-3 fail (the attack no longer works), on the real sandbox where they need it.


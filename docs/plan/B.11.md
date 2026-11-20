# B.11

## Experiment

Containers in place of the Landlock sandbox for agent shells (the owner, 2026-11-12: a separate branch with quick Podman tests first).
Rootless Podman against bubblewrap against today's `sandbox-exec`, same tests: time to start and per `exec`; whether a `setsid` child dies with the container or namespace, on cancel and on `kill -9` of the node; no network with `--network=none` (UDP and DNS included); what a session can see of another's folder; memory per idle session; what it needs from the machine (`subuid`, a user runtime directory, user namespaces); what happens inside the published image, where rootless Podman cannot nest.

## Outcome that changes a decision

Decides: Podman alone, bubblewrap alone, or Podman for environments with bubblewrap for plain sessions; and what a node without either does (refuse the shell, never run unconfined).
The node's own helpers (shadow git, the PDF parser) keep Landlock either way: a container per checkpoint costs too much.
Feeds T.8 and N.4. **Done and decided 2026-11-12 (O12): bubblewrap for plain sessions, Podman for environments, Landlock kept.**

## Size

**M**


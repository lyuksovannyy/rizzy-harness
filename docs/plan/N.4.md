# N.4 **sec**

## Note phase

4 environments

## Adds beyond the note

Tests D14, D15 first (D14 answered by B.11: rerun as a test);
C.6's argv and checks; C.8's binds if chosen.
B.11's corrections (note §4.5, §4.6): `--network=pasta:--map-guest-addr,none` for "on", and a test that `host.containers.internal` reaches no host service; the mount checked by device and inode from inside after start, the container removed on a mismatch; cancel's test uses a `setsid` child, which needs the container stopped; the node's unit must not set `NoNewPrivileges=yes`; about 26 MB per session for pasta in the RSS budget; refused inside the published image

## Abuse tests beyond the note's

C.6's tests; identical behaviour inside and out

## Invariants

Node decides; loopback; RSS with 8 containers

## Podman / Node

**Yes** / Yes

## Depends

N.2, C.6, C.8

## Size

**L**


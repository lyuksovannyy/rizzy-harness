# T.8f review, 2026-11-12: the node's link to its own image

Reviewed: `feat/T.8f-image-link` at `ad8b0e5`, only what it adds to
`feat/T.8c-mount-view` (`git diff 09183aa..HEAD`: `b4124d8`, `ffbeb58`,
`478e365`, `ad8b0e5`). Read in full, then attacked on the branch's own build.
No product or test source changed; only this report and the scripts under
`crates/cli/tests/support/review_t8f/` were added. Machine: Arch Linux, kernel
7.2.15, bubblewrap 0.13.1 (not setuid; binds by descriptor), ext4,
`fs.protected_hardlinks=1`, an `/etc/subuid` entry for the user. The binds by
path (bubblewrap below 0.10.0: Ubuntu 24.04's 0.9.0, Debian 12's, CI's) were
exercised through `with_binds(Binds::Path)` on 0.13.1. Format and severity
scale as in `2026-11-T8c-mount-view.md`.

## 1. Verdict

**With bubblewrap 0.10.0 or newer the change does what it says, with one
defect of its own (T8F-1). With an older bubblewrap two of its sentences are
not true (T8F-2, T8F-3).**

What held, with proof: the link is the image the node runs and never the
file now at the binary's path; the guard binds it only while it is its own
image; in the binds by descriptor a link replaced at any moment is refused by
the guard or by bubblewrap, 0 of 500 commands ran another file; no session
can see, write, replace or remove the link's name; the content of the image
cannot be changed through any name while the node runs (`ETXTBSY`); both
fallbacks (`EXDEV`, `EPERM`) were made real here and behave as T.8c with the
reason in the log and in `doctor`; a second node makes the first refuse;
`doctor` leaves a running node's link alone.

What did not hold:

| Id | Severity | What | Proof |
|---|---|---|---|
| T8F-1 | **Low** (the change's own mechanism; fix before merge) | Every restart of a binary that was not rebuilt leaves `node-image.new-<pid>` in the data directory: `rename(2)` does nothing when both names are already the same file. Each build that was started twice then stays on disk whole after it is superseded (178 MB here), for ever; the names grow without bound. | `g_a_restart_leaves_its_temporary_link` |
| T8F-2 | **Low** (claim 3; T8C-N2 carried, and understated there) | Binds by path: every contained command inherits an `O_PATH` descriptor of the link. Through it one command sets the image's mode to 000 (also `setxattr`, `utime`), for a user-owned binary anywhere, not only in a build tree. Every later command of every session fails to start, and so does a restart of the node, until the owner restores the mode. README: "a hard link no session can reach". | `a_what_a_command_can_do_to_the_image` |
| T8F-3 | **Low** (claims 2 and 5 with the link; claim 4's fallback) | Binds by path: after the guard's check nothing looks at what was mounted. (a) With the link, a replacement of the link landing in that window binds the other file: 21 of 200 commands under continuous replacement ran it as `sandbox-exec`. Only something that writes the data directory can do it (a second node's start, A.20), so not a session. (b) In the fallback the bound path is the binary's own, and in a build tree a session writes it: with two real contained commands of two sessions, the session's file ran as `sandbox-exec` before Landlock in 4 of 200 tries. (b) is T.8c's behaviour, which T.8f keeps "exactly"; T.8f closes it where the link is made (0 of 200). | `c_a_link_replaced_after_the_guards_check`, `h_the_fallback_by_path_in_a_build_tree` |
| T8F-4 | Note (claim 5 holds; a cost) | A second start of ANOTHER build on the same data directory replaces the link before it fails on the port that is taken. The running node then refuses every contained command until restarted. This is `cargo build`, then `serve` typed again while the old node runs. A start that cannot make its link (`EPERM`, …) removes the running node's link the same way. | `d_a_second_node_and_doctor` |
| T8F-5 | Note (the attacker's "lock the node out") | In a build tree the link shares its inode, so its mode, with a name the session can write. `chmod 000` on any name of it stops every command of every session, in both bind modes. T.8f removes the accident (a rebuild), not the attack. The documents do not say so. | `b_a_build_tree_session_and_the_running_image` |
| T8F-6 | Note (claim 4, "nothing is left") | A temporary link left by a start that died between the link and the rename is never removed (only this pid's is). A data directory that cannot be written keeps an earlier start's link at the name. A node later set to `mode = "off"` never removes it (reading). | `e_what_a_start_finds_and_leaves` E4, E5 |
| T8F-7 | Note | Test gaps: no restart on a data directory that already holds the link (T8F-1 went unseen); the guard's refusal is tested only in `(Full, Fd)`; nothing replaces the link while a command starts; claim 3's test does not look at inherited descriptors; no real `EPERM`. | section 5 |
| T8F-8 | Note, not run | `explain` gives `EPERM` one cause ("not this user's own file"). A file system without hard links (vfat, exfat) and an immutable file answer `EPERM` too; the log would name the wrong reason. The behaviour is the fallback either way. | reading; no such file system here |
| T8F-9 | Note | After an upgrade the node runs the old image silently until restarted (before T.8f the refusal forced the restart). Nothing in the log or in `doctor` says "the node runs another build than the one on disk". | reading |

Verdict per claim: 1 holds (with T8F-1's leftover); 2 holds by descriptor,
**not by path**; 3 holds for the name, **not for the inode by path**; 4
holds (corners T8F-6); 5 holds by descriptor, **not by path**; 6: both
places where the fix does not apply are stated, three sentences claim more
than is true.

One thing the owner should know before reading on: **on this machine the
fix does not apply to the node it was written for.** The project is on
`/dev/sda1`, the home directory on `/dev/nvme0n1p2`. A node run from the
build tree with its data directory in the home directory gets `EXDEV` and
T.8c's refusal after the first rebuild. README and PLAN state the case in
general; neither says it is the usual one for a build tree on its own disk.

## 2. Findings

### T8F-1. Low. A restart leaves its temporary link, and old builds stay on disk

**The claim it breaks.** The plan row: "made under a temporary name, checked
by device and inode, renamed over whatever was there". `image.rs`: "The name
is replaced at every start".

**The mechanism.** `settle` renames `node-image.new-<pid>` over `node-image`.
POSIX: if both names are links to the same file, `rename` does nothing and
succeeds. After a restart of an unchanged binary the old `node-image` is
that file, so the temporary name stays. `check(at)` passes (it is the image),
so the start reports `Linked`. Only a later start with the same pid would
remove it. After the next upgrade the leftover names are the old build's only
names and keep its blocks.

**Reproduction** (`run_rust_proofs.sh g_a_restart`; a real `serve`, stopped
and started):

```
make, first: true; the data directory: ["node-image"]
make, again (a restart): true; the data directory: ["node-image", "node-image.new-1849932"]
start 1 (pid 1849953): command: "RAN\n[exit code: 0]"; node-image*: ["node-image (inode 4065612, 178738720 bytes)"]
start 2 (pid 1850164): command: "RAN\n[exit code: 0]"; node-image*: ["node-image (inode 4065612, 178738720 bytes)", "node-image.new-1850164 (inode 4065612, 178738720 bytes)"]
start 3 (pid 1850220): command: "RAN\n[exit code: 0]"; node-image*: ["node-image (inode 4065612, 178738720 bytes)", "node-image.new-1850164 (inode 4065612, 178738720 bytes)", "node-image.new-1850220 (inode 4065612, 178738720 bytes)"]
after an upgrade (the old build was inode 4065612, the new one is 4065589) and a start: node-image*: ["node-image (inode 4065589, 178738720 bytes)", "node-image.new-1850164 (inode 4065612, 178738720 bytes)", "node-image.new-1850220 (inode 4065612, 178738720 bytes)"]
the old build has no name but those left behind: 178738720 bytes kept on disk by them
```

**Smallest fix.** In `place`, remove the temporary name after `settle`
whatever it returned (after a rename that moved it there is nothing to
remove). With it, at start, remove every `node-image.new-*` that is not this
start's (T8F-6's crashed start).

**The test that should exist.** `make` twice with the same file: the data
directory holds `node-image` and nothing else. The existing unit test asserts
the count only after a folder was in the way.

### T8F-2. Low. By path, a command holds a descriptor of the link and can change the image's mode

**The claim it breaks.** Claim 3 in its plain sense, README's "a hard link no
session can reach", and T8C-N2's "no new capability".

**The mechanism.** With an older bubblewrap the guard hands bubblewrap
`/proc/self/fd/<n>`, a descriptor that is not close-on-exec; bubblewrap does
not close it, `sandbox-exec` does not, the command inherits it. It is opened
on the host's mount, not on the read-only bind, so the bind does not cover
it. Landlock does not rule on `chmod`, `utimensat` or `setxattr`. The owner
of the file may change its mode. With T.8f the descriptor names
`<data dir>/node-image`; without the link it names the binary. Same inode,
same result. Writing and truncating are refused (`ETXTBSY`), a third link
into the session's or the temp folder is refused (`EXDEV`: another mount).

**Reproduction** (`run_rust_proofs.sh a_what`; the binary is OUTSIDE every
grant, as an installed user-owned binary is):

```
==== Fd, Linked ====
view image: nlink=2 mode=0o755
view path: chmod 000: refused (EROFS)
view path: open for writing: refused (ETXTBSY)
view path: link into the temp folder: refused (EXDEV)
no inherited descriptor of the image
on the host: the binary's mode before 755, after 755; size 178738720
the next command: NEXT-COMMAND-RAN
==== Path, Linked ====
view image: nlink=2 mode=0o755
inherited descriptor 5 IS the image; it names: node-image
view path: chmod 000: refused (EROFS)
fd: open for writing: refused (ETXTBSY)
fd: truncate: refused (ETXTBSY)
fd: link into the temp folder (same file system as the data directory): refused (EXDEV)
fd: link into the session folder: refused (EXDEV)
fd: setxattr user.review: DONE
fd: utime: DONE
fd: chmod 000: DONE
on the host: the binary's mode before 755, after 0; size 178738720
the next command: failed to spawn bash: Permission denied (os error 13)
another session's command: failed to spawn bash: Permission denied (os error 13)
==== Path, no link (T.8c's view) ====
inherited descriptor 5 IS the image; it names: rizzy-harness
fd: chmod 000: DONE
on the host: the binary's mode before 755, after 0; size 178738720
the next command: failed to spawn bash: Permission denied (os error 13)
```

The binary at its path has mode 000 afterwards: the node cannot be restarted
either until the owner runs `chmod`. Not a root-owned binary (the session's
user does not own it).

**Smallest fix.** By path, do not give bubblewrap a descriptor: give it the
path the guard checked (the link's; without a link, the path
`/proc/self/exe` names, after the "(deleted)" check), and open the guard's
own descriptor close-on-exec. bubblewrap resolves `/proc/self/fd/<n>` to that
same path before it mounts, so nothing is lost. Or `sandbox-exec` closes
every descriptor above 2 before it starts the command.

**The test that should exist.** In `a_contained_command_cannot_touch_the_link`,
by path: the command lists `/proc/self/fd` and none has the image's device
and inode; the host's mode of the link is unchanged after
`chmod 000 /proc/self/fd/*`. (`mount_view`'s
`no_descriptor_of_a_folder_reaches_the_command` asserts only "not a folder".)

### T8F-3. Low. By path, nothing checks what was mounted after the guard's check

**The claims it breaks.** Claim 2 ("the node's image or nothing, in both bind
modes"), claim 5 ("never makes it bind another build"), PLAN's "rather than
bind another build"; and, for the fallback, the start-up warning's "a
contained command's guard and sandbox-exec are the node's own running image
and are not affected".

**The mechanism.** bubblewrap turns `/proc/self/fd/<n>` into a path
(`realpath`) and mounts that path after every other mount of the view. By
descriptor it then compares what it mounted with the descriptor ("Race
condition binding dirfd") and the command is refused. By path there is no
comparison, and the image is `sandbox-exec` itself, so the check
`sandbox-exec` makes for the grants cannot stand in.

**(a) With the link** (`run_rust_proofs.sh c_a_link`; a thread replaces the
link with another file and back, 0 to 7 ms apart, while commands run):

```
Path: bubblewrap's refusal: bwrap: Can't find source path /proc/self/fd/5: No such file or directory
Path: round 12: THE OTHER FILE RAN AS sandbox-exec; the command's output: "RAN\n\n[exit code: 0]"; its record: "ran as sandbox-exec\n"
Path: 200 commands, 330 swaps of the link: ran from the node's image 75; refused by the guard 89; refused by bubblewrap 15; THE OTHER FILE RAN AS sandbox-exec 21; other outcomes 0
Fd: bubblewrap's refusal: bwrap: Race condition binding dirfd: File exists
Fd: 200 commands, 314 swaps of the link: ran from the node's image 71; refused by the guard 91; refused by bubblewrap 38; THE OTHER FILE RAN AS sandbox-exec 0; other outcomes 0
```

Who can do it: only something that writes the data directory. No session
can (section 3). A second node's start does one replacement; it must land
in about a millisecond of a command's set-up. What then runs is the other
node's build, not an attacker's file. Unlikely and bounded, and still not
"never".

**(b) Without the link, in a build tree** (`run_rust_proofs.sh h_the`; both
actors are contained commands of two sessions of one node; the first waits,
spins a random time and runs `mv -f imp rizzy-harness` once; the second is
`echo RAN`):

```
Path, link false: pair 93: THE SESSION'S FILE RAN AS sandbox-exec; the command's output "RAN\n\n[exit code: 0]"; its record Some("ran as sandbox-exec")
Path, link made: false: 200 pairs: the command ran from the node's image 177; refused 19; THE SESSION'S FILE RAN AS sandbox-exec 4; other 0
Path, link made: true: 200 pairs: the command ran from the node's image 200; refused 0; THE SESSION'S FILE RAN AS sandbox-exec 0; other 0
Fd, link made: false: 200 pairs: the command ran from the node's image 174; refused 26; THE SESSION'S FILE RAN AS sandbox-exec 0; other 0
```

One try per life of the node (after the rename the image has no path and
every command is refused), about 2 % here. What it gains: the command runs
in the view and the PID namespace, under its guard, without Landlock (its
TCP rules and scoping) and without `sandbox-exec`'s check of the grants by
path. Needs: an older bubblewrap, a build tree, no link, `contain = "auto"`
(`required` refuses such a node). This is T.8c's code, not T.8f's; T.8c's
review did not have it. It is here because claim 4 keeps it and because the
middle line is the proof that the link closes it.

**Smallest fix.** Either a check after the mount: bubblewrap's `--block-fd`
and `--info-fd` (in 0.13.1; to be confirmed on the oldest version supported), the guard compares
`/proc/<child>/root/.rizzy-harness/rizzy-harness` with its own image and
only then lets the command go. Or say it: in PLAN's T9 and the README, that
by path the image is checked before the mount only; in the start-up warning,
that with an older bubblewrap and no link a contained command IS affected.
For (a), A.20's lock ends it.

**The test that should exist.** Both proofs with the count asserted zero by
path (they are the tests; they need no root).

### T8F-4. Note. A failed second start stops the running node's commands

`d_a_second_node_and_doctor` (two copies of the binary, one data directory):

```
B started on A's port: the server ended (...): ... Error: binding 127.0.0.1:40611 / Address already in use (os error 98)
after B's failed start the link is: B's image
A, after B's failed start: sandbox-guard: the node's link to its own image (<root>/data/node-image) is not the image this node runs: it was replaced since the node started (a second node started on this data directory does that); restart the node. The command was not started
```

Claim 5 holds: A refuses, it does not bind B. But B never ran. In a build
tree this is `cargo build` followed by `serve` typed while the old node is
up; T.8c lost the node at the rebuild, T.8f loses it one step later. **Fix I
would accept:** none before A.20; or make the link after the listener is
bound. **Test:** the proof, once A.20 exists.

### T8F-5. Note. In a build tree one `chmod` still stops the node

`b_a_build_tree_session_and_the_running_image` (a real `serve` from
`ws/target/debug`):

```
1 (content, while the node runs): third-link-made
nlink=3 mode=755
bash: line 1: target/debug/rizzy-harness: Text file busy
bash: line 1: .keep: Text file busy
cp: cannot create regular file 'target/debug/rizzy-harness': Text file busy
OSError: [Errno 26] Text file busy: '.keep'
2 (a rebuild): rebuilt
3 (after the rebuild): AFTER-THE-REBUILD
4 (chmod 000 on the session's own link to the image): chmod=0
  on the host: the node's link has mode 0
5 (the next command, any session): failed to spawn bash: Permission denied (os error 13)
6 (mode restored on the host): AFTER-THE-OWNER-RESTORED-THE-MODE
```

The content is safe: the node runs the inode, so the kernel refuses every
open for writing and every truncate through any name. The mode is not. A
session that kept its own link (`ln`, before or after the rebuild) can do it
at any time. **Fix:** a sentence in README and PLAN: the link keeps a
rebuild from stopping the node; a session that can write the binary's folder
can still stop it, as before. A copy would end it and was ruled out.

### T8F-6 to T8F-9. Notes

- **T8F-6.** `e_what_a_start_finds_and_leaves`:
  `E4 a crashed start's temporary link (another pid): after a start (Linked) and a doctor (Possible) the data directory holds ["node-image", "node-image.new-1949932"]`;
  `E5 an unwritable data directory with an earlier link: NotMade("… cannot be written (EACCES: Permission denied)"); … the name holds Some("an earlier build")`.
  The left link is not bound (the guard gets no `--image`), so it is a wrong
  sentence, not a risk. Fix with T8F-1.
- **T8F-8, T8F-9.** As in the table. For T8F-9 `doctor` could compare
  `node-image` with its own image and say when they differ.
- **`doctor`'s own names.** Its trial removes whatever is at
  `node-image.doctor-<its pid>` and `node-image.new-<its pid>`, a folder with
  what is in it (`E8 … the data directory holds ["node-image"]`). Only
  something that writes the data directory can put a thing there.

## 3. What I attacked and what held

**The link's creation.** Read every step of `place`, `settle`, `check`,
`remove`. No step uses the binary's path. `e_what_a_start_finds_and_leaves`:

```
E1 a FIFO at the name: Linked; now a regular file with the image's inode: true
E1 a dangling symlink at the name: Linked; the image's inode: true
E2 a folder with links out at the name: Linked; outside still holds ["kept"] = "kept"
E3 a symlink at the temporary name: Linked; its target: "kept"
E3 a folder at the temporary name: Linked
E6 the data directory through a symlink: Linked("<root>/data-link/node-image")
E7 a binary replaced at its path that has another name: Linked; the link is the held image: true; is the file now at the path: false
```

E7 is cargo's layout (`target/debug/x` is a hard link of `deps/x-<hash>`):
the held inode is linked, not the new file. A session cannot write the
temporary name or the fixed name: both are in the data directory itself, not
in `sandbox-tmp/<session>`. A planted `node-image.new-<pid>` is removed
without being followed; a name taken between the removal and the link gives
`EEXIST` and the fallback. `remove_dir_all` did not follow the links in E2
(std's `remove_dir_all` does not follow links since 1.58.1).

**The guard's check.** Section 2, T8F-3. By descriptor: 0 of 500 commands
(300 + 200 in two runs) ran another file under continuous replacement. The
name can be replaced only from the data directory; see the next paragraph.

**Who can write the data directory.** Suite test
`a_contained_command_cannot_touch_the_link` passes in both bind modes
(`link=absent`, `Read-only file system`, the listing refused). On a real
node (`i_a_command_runs_the_harness_itself`), from the session's temp folder,
which is inside the data directory and on the link's file system:

```
in its own data directory:
Invalid cross-device link                       (ln /.rizzy-harness/rizzy-harness $TMPDIR/k1)
bash: line 1: /.rizzy-harness/rizzy-harness: Permission denied   (open for reading)
Read-only file system                           (chmod 000)
mountinfo lines naming node-image: 1
the image in the view: links=2 mode=755
on the host: the link's mode 755; node-image* in the data directory: ["node-image"]
```

The command learns that a second name exists and the data directory's path
(`mountinfo`; it knew the path from `TMPDIR`). Nothing else.

**A third link, writing the inode.** T8F-2 and T8F-5. A third link is
possible only where the session already has a name for the binary in its own
folder (the build tree). It gives nothing but the mode (T8F-5): the content
is held by `ETXTBSY` as long as the node, or any guard, runs the inode.

**Identity by device and inode.** The guard compares with its own
`/proc/self/exe`, which it is running: the file system cannot give that
number to another file while the comparison is made. `inode_reuse.py`:

```
held and unlinked: inode 4065575, links now 0; /proc/self/fd names '…/image (deleted)'
while it is held: 3000 files created, 0 with its device and inode
after it was closed: file 0 got its device and inode
```

A node whose image has no name at start: `nlink == 0`, not linked (suite
test 2, and the kernel's own `ENOENT`). A rename that exchanges two files
moves names, not inodes: the link stays the image.

**The fallbacks, real** (`real_fallbacks.sh`; a copy of the binary owned by
uid 100000 through `newuidmap`; a tmpfs data directory in a private mount
namespace):

```
EPERM: the binary: owner uid 100000 (this user is 1000), mode 755, device 2049; the data directory's device 2049
doctor: … bound through the binary's own path, as no link node-image could be made in the data directory (the harness binary is not this user's own file and fs.protected_hardlinks is on (an installed binary)): after a rebuild or upgrade of the binary under the running node, every contained command is refused until the node is restarted) …
after doctor, node-image* in the data directory: []
after the start, node-image* in the data directory: []
first command: FIRST-RAN
after the upgrade: sandbox-guard: the harness binary this node runs was deleted or replaced since the node started (<bin>/rizzy-harness (deleted)); restart the node (it could not keep its image by a link in its data directory: the file view line of its log, or doctor, says why). The command was not started

EXDEV: the binary: owner uid 1000 (this user is 1000), mode 755, device 2049; the data directory's device 97
link_image: NotMade("the data directory <data> is on another file system than the harness binary (a hard link cannot cross file systems)")
doctor: … (the data directory <data> is on another file system than the harness binary (a hard link cannot cross file systems)) …
after the start, node-image* in the data directory: []
first command: FIRST-RAN
after the upgrade: sandbox-guard: the harness binary this node runs was deleted or replaced … The command was not started
```

The refusal names the binary's path and, with the link, the link's path.
Both are known to the session already (`TMPDIR`, `mountinfo`). Nothing else
is in them.

**The second node.** T8F-4's proof, continued:

```
with B running the link is: B's image
A, with B running (A's image is inode 4065659, B's 4065662): sandbox-guard: the node's link to its own image (…) is not the image this node runs … The command was not started
B: B-RAN
inode=4065662
A, after B stopped: sandbox-guard: … The command was not started
```

Two starts racing (`e9_two_starts_racing`, 3000 rounds, one shared pid, which
is harsher than two processes): every outcome is "linked and the name holds
my image", "linked and the name holds the other's or nothing" (the guard
refuses) or "not made". None leaves a third file at the name.

**`doctor`** against a running node, with the node's build and with another:

```
after both doctors: the link is still A's image: true; new names in the data directory: []
A, after the doctors: A-RAN-2
```

**The suites.** `image_link` 7 of 7, `mount_view` 15 of 15 (one needs a
short `TMPDIR`, T8C-N1), `harness-tools` `image::` 4 of 4.

## 4. The documents

Both places where the fix does not apply are stated, in README and in PLAN:
the data directory on another file system; a binary the user does not own
(`/usr/local/bin`) under `fs.protected_hardlinks`. Sentences that claim more
than is true:

- README: "a hard link no session can reach". By path a command holds a
  descriptor of it and can change its mode (T8F-2).
- PLAN T9: "the first node then refuses its contained commands … rather than
  bind another build". By path, not always (T8F-3a).
- `check_binary_reach`'s warning (unchanged text, now beside T.8f): "a
  contained command's guard and sandbox-exec are the node's own running
  image and are not affected". By path without a link, not always (T8F-3b).
- The plan row and `image.rs`: "renamed over whatever was there", "nothing is
  left at the name" (T8F-1, T8F-6).

Missing: that a build tree on another disk than the data directory is the
fallback (this machine); that a session in a build tree can still stop the
node (T8F-5).

## 5. The tests

| Claim | Test | Fails without the change | Tests the claim |
|---|---|---|---|
| 1 | `a_binary_replaced_before_the_link_is_not_used`, unit `a_replaced_file…`, `a_link_to_another_file_is_refused` | on the stub, yes | yes; not a restart over an existing link (T8F-1) |
| 2 | `a_link_left_by_an_earlier_start_is_never_what_runs` | yes | `(Full, Fd)` only, and only a replacement between commands; by path and during a start: nothing (T8F-3) |
| 3 | `a_contained_command_cannot_touch_the_link` | yes | the name, both modes; not the inherited descriptor (T8F-2) |
| 4 | `where_no_link_can_be_made_the_view_is_as_before` | yes | the `EACCES` seam; `EXDEV` only where the temp folder is on another file system; no `EPERM` (both run here, section 3) |
| 5 | second half of the claim 2 test | yes | a replaced file, not a second node (run here, holds by descriptor) |
| 6 | `doctor_names_the_link_and_leaves_it_alone` | yes | yes |

"All failing on the stub" is true and weak: the stub returns `NotMade`, so
each fails at its first `Linked`, which a wrong implementation would pass.

## 6. What I could not test, and why

- A real bubblewrap below 0.10.0, and a setuid one: path mode is 0.13.1 with
  `with_binds`.
- A file system without hard links, overlayfs (the module is not loaded and I
  load none), btrfs subvolumes, a bind-mounted data directory.
- A root-owned binary: the `EPERM` proof uses a subordinate uid, which the
  kernel's rule treats the same.
- `ENOSPC`, `EMLINK` (65000 links to the image from a build tree would send
  the next start to the fallback): read, not run.
- **Not proven:** `open_image` decides "this is my own binary" by a second
  `current_exe()` comparison. A binary replaced between `start`'s and that
  one (microseconds, at the node's start) would be opened by path, and
  `link_image` would then link the new file. Nothing a session can time; a
  proof needs a hook between the two calls. Opening `/proc/self/exe` once in
  `main` would settle it.
- Mutations of the product source (I change none): which test catches a
  guard without its comparison is argued in section 5, not run.

## 7. Reproduction

```
export CARGO_TARGET_DIR=…/target
export TMPDIR=$CARGO_TARGET_DIR/review-t8f/tmp     # on the build's file system; not /tmp
D=crates/cli/tests/support/review_t8f
sh $D/run_rust_proofs.sh                # all; or a name: a_what b_a_build c_a_link d_a_second e_what e9 g_a_restart h_the i_a_command
REVIEW_T8F_ROUNDS=300 sh $D/run_rust_proofs.sh c_a_link
sh $D/real_fallbacks.sh                 # EPERM and EXDEV, without root
python3 $D/inode_reuse.py 3000
```

The whole file: `10 passed … finished in 154.48s`. `run_rust_proofs.sh`
copies the proofs to `crates/cli/tests/` for the run and removes the copy.
Every node the proofs start is their own child, killed by pid and reaped
when its test ends.

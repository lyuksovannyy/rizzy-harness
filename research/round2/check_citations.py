#!/usr/bin/env python3
"""Check every GitHub permalink in a Round 2 file against the pinned local clones.

Usage: python3 research/round2/check_citations.py <file.md> [more files]

A citation must look like
  https://github.com/<org>/<repo>/blob/<40-hex-sha>/<path>#L<start>-L<end>
(or #L<line>). For each one this checks: the repository is one of the pinned
clones, the SHA is the pinned one, the path exists in the clone, and the line
range lies inside the file. It also reports GitHub blob links that are not
pinned to a commit. It does not judge whether the lines say what the text
claims: that is the reviewer's job.
"""
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CLONES = ROOT / ".claude" / "research-repos"

repos = {}
for line in (HERE / "00-repos.tsv").read_text().splitlines():
    parts = line.split("\t")
    if len(parts) >= 3 and parts[2] != "CLONE-FAILED":
        repos[parts[1].lower()] = (parts[0], parts[2])

PINNED = re.compile(
    r"https://github\.com/([\w.-]+/[\w.-]+)/blob/([0-9a-f]{40})/([^\s#)>\]`]+)(?:#L(\d+)(?:-L(\d+))?)?"
)
UNPINNED = re.compile(r"https://github\.com/[\w.-]+/[\w.-]+/(?:blob|tree)/(?![0-9a-f]{40}/)[^\s)>\]`]+")

line_counts = {}


def count_lines(p):
    if p not in line_counts:
        with open(p, "rb") as f:
            line_counts[p] = sum(1 for _ in f)
    return line_counts[p]


status = 0
for name in sys.argv[1:]:
    text = Path(name).read_text(encoding="utf-8")
    ok, bad, no_range = 0, [], 0
    for m in PINNED.finditer(text):
        repo, sha, path, a, b = m.groups()
        path = path.rstrip(".,;:")
        entry = repos.get(repo.lower())
        if entry is None:
            bad.append((m.group(0), "repository is not one of the pinned clones"))
            continue
        slug, pinned = entry
        if sha != pinned:
            bad.append((m.group(0), f"commit is not the pinned one ({pinned[:12]})"))
            continue
        local = CLONES / slug / path
        if not local.is_file():
            bad.append((m.group(0), "path does not exist at the pinned commit"))
            continue
        if a is None:
            no_range += 1
            ok += 1
            continue
        start, end = int(a), int(b or a)
        n = count_lines(local)
        if start < 1 or end < start or end > n:
            bad.append((m.group(0), f"lines {start}-{end} are outside the file ({n} lines)"))
            continue
        ok += 1
    unpinned = sorted(set(UNPINNED.findall(text)))
    print(f"{name}: {ok} citations valid ({no_range} without a line range), "
          f"{len(bad)} invalid, {len(unpinned)} GitHub links not pinned to a commit")
    for url, why in bad:
        print(f"  INVALID {why}: {url}")
    for url in unpinned[:20]:
        print(f"  UNPINNED {url}")
    if bad:
        status = 1
sys.exit(status)

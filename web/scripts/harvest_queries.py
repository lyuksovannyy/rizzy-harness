#!/usr/bin/env python3
"""Collect common search queries from Google's autocomplete for the chat
background wall. Run by hand, rarely: the results are committed, so the app
never calls Google at runtime.

    python3 -I web/scripts/harvest_queries.py [--min 1000]

Writes web/public/wall/queries.txt (general queries) and
web/public/wall/topics/<topic>.txt (queries per interest topic), one query
per line. Politeness: one request at a time, a pause between requests, stop
on any HTTP error (e.g. 429).
"""

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ENDPOINT = "https://suggestqueries.google.com/complete/search"
PAUSE_S = 0.4
OUT = Path(__file__).resolve().parent.parent / "public" / "wall"

LETTERS = "abcdefghijklmnopqrstuvwxyz"

# Everyday query starts; each is also expanded with " a" .. " z".
GENERAL_PREFIXES = [
    "how to", "what is", "what are", "why is", "why do", "why does", "when is", "when does",
    "where is", "who is", "can i", "can you", "is it", "how much", "how many", "how long",
    "how do i", "what time", "what does", "best way to", "weather", "time in", "how to make",
    "how to cook", "recipe for", "symptoms of", "meaning of", "difference between",
    "convert", "how far is", "cheap flights to", "things to do in", "how to get",
]

# Interest topics for weighting the wall by the user's history.
TOPICS = {
    "software-development": ["python how to", "javascript how to", "how to learn programming", "react", "rust programming",
                             "git how to", "docker", "sql query", "typescript", "how to code", "linux command", "api"],
    "robotics": ["how to build a robot", "robot arm", "robotics engineer", "robotics for beginners", "arduino",
                 "raspberry pi", "ros2", "servo motor", "stepper motor", "how to build a drone", "3d printing",
                 "industrial robot"],
    "careers": ["how to get a job", "job interview", "resume", "cover letter", "remote jobs", "salary", "how to ask for a raise",
                "linkedin", "career change"],
    "health": ["symptoms of", "how to lose weight", "how much water", "how to sleep", "vitamin", "is it healthy to"],
    "travel": ["cheap flights to", "things to do in", "visa for", "best time to visit", "hotels in"],
    "money": ["how to save money", "how to invest", "mortgage", "credit score", "inflation", "stock"],
    "food": ["recipe for", "how to cook", "how long to boil", "how to bake", "vegan"],
    "science": ["why is the sky", "how does", "what is the speed of", "black hole", "how many planets"],
    "language": ["translate", "meaning of", "how to pronounce", "synonym for", "how to say"],
    "home": ["how to clean", "how to fix", "how to remove", "how to grow", "diy"],
}

# Nothing on the wall about sex, or about named people dying or being arrested.
BLOCK = re.compile(
    r"\b(porn|sex|sexy|nude|nudes|naked|xxx|onlyfans|hentai|nsfw|escort|fetish|dick|pussy|boobs|"
    r"dead|died|death|dies|killed|murder|arrested|arrest|shooting|suicide|obituary|funeral|"
    r"leaked|scandal|affair|divorce|girlfriend|boyfriend|wife|husband|net worth|age of|"
    # Politics and drugs: divisive or awkward on a decorative wall.
    r"trump|biden|harris|obama|maga|democrat\w*|republican\w*|election|congress|senate|"
    r"gaza|israel|hamas|putin|zelensky|weed|marijuana|cocaine|meth)\b",
    re.I,
)


def fetch(q: str, hl: str = "en", gl: str = "us") -> list[str]:
    url = ENDPOINT + "?" + urllib.parse.urlencode({"client": "firefox", "hl": hl, "gl": gl, "q": q})
    req = urllib.request.Request(url, headers={"User-Agent": "rizzy-harness query harvester"})
    with urllib.request.urlopen(req, timeout=15) as r:
        data = json.loads(r.read().decode("utf-8", "replace"))
    out = data[1] if isinstance(data, list) and len(data) > 1 and isinstance(data[1], list) else []
    return [s for s in out if isinstance(s, str)]


def clean(s: str) -> str | None:
    s = " ".join(s.split()).strip().lower()
    if not 6 <= len(s) <= 60 or len(s.split()) < 2:
        return None
    if any(ord(c) < 32 for c in s) or BLOCK.search(s):
        return None
    if not re.fullmatch(r"[a-z0-9 '&+.,?!:/-]+", s):
        return None
    return s


def harvest(
    seeds: list[str], seen: set[str], target: int | None, label: str, per_seed: int = 10
) -> list[str]:
    got: list[str] = []
    for i, seed in enumerate(seeds):
        if target and len(got) >= target:
            break
        try:
            results = fetch(seed)
        except (urllib.error.URLError, TimeoutError, ValueError) as e:
            print(f"stopping {label}: {e}", file=sys.stderr)
            break
        taken = 0
        for s in results:
            c = clean(s)
            if c and c not in seen and taken < per_seed:
                seen.add(c)
                got.append(c)
                taken += 1
        if i % 25 == 0:
            print(f"{label}: {i + 1}/{len(seeds)} requests, {len(got)} queries", file=sys.stderr)
        time.sleep(PAUSE_S)
    return got


def expand(prefixes: list[str]) -> list[str]:
    seeds = []
    for p in prefixes:
        seeds.append(p)
        seeds.extend(f"{p} {c}" for c in LETTERS)
    return seeds


def refilter(path: Path) -> None:
    """Apply the current filter to an already written list."""
    if path.exists():
        kept = [q for q in path.read_text(encoding="utf-8").splitlines() if q and clean(q)]
        path.write_text("\n".join(kept) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--min", type=int, default=1000, help="general queries to collect at least")
    ap.add_argument("--refilter", action="store_true", help="only re-apply the filter to the written lists")
    ap.add_argument("--topics-only", action="store_true", help="keep queries.txt, rebuild the topic lists")
    args = ap.parse_args()
    if args.refilter:
        for p in [OUT / "queries.txt", *sorted((OUT / "topics").glob("*.txt"))]:
            refilter(p)
        return 0
    seen: set[str] = set()
    OUT.mkdir(parents=True, exist_ok=True)
    if args.topics_only:
        general = (OUT / "queries.txt").read_text(encoding="utf-8").splitlines()
        seen.update(general)
    else:
        general = harvest(expand(GENERAL_PREFIXES), seen, args.min + 200, "general")
        if len(general) < args.min:
            print(f"only {len(general)} general queries (wanted {args.min})", file=sys.stderr)
        (OUT / "queries.txt").write_text("\n".join(general) + "\n", encoding="utf-8")
    topics_dir = OUT / "topics"
    topics_dir.mkdir(exist_ok=True)
    for name, prefixes in TOPICS.items():
        seeds = []
        # Breadth first: every prefix, then the prefixes with a letter, so a
        # topic is not just variations of its first prefix.
        seeds.extend(prefixes)
        for c in "abcdhimprstw":
            seeds.extend(f"{p} {c}" for p in prefixes)
        q = harvest(seeds, seen, 120, name, per_seed=4)
        (topics_dir / f"{name}.txt").write_text("\n".join(q) + "\n", encoding="utf-8")
    print(f"general {len(general)}; topics written to {topics_dir}", file=sys.stderr)
    return 0 if len(general) >= args.min else 1


if __name__ == "__main__":
    sys.exit(main())

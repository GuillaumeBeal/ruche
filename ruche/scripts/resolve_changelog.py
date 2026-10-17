#!/usr/bin/env python3
"""Resolve merge conflicts in CHANGELOG.md by keeping the entries of both sides.

Fork merges conflict on CHANGELOG.md whenever both sides add an entry at the
top of the same section. Each conflict block is replaced by the entries of
both sides, ours first, with an entry present on both sides kept once. Two
versions of the same entry (same bold title) are reduced to the one in
upstream/main when it has one, else to ours.

Usage (from the repository root, during a merge):
    python3 ruche/scripts/resolve_changelog.py [--upstream upstream/main]
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHANGELOG = ROOT / "CHANGELOG.md"
TITLE = re.compile(r"^\* \*\*(.+?)\*\*")


def entry_title(line: str) -> str | None:
    match = TITLE.match(line)
    return match.group(1) if match else None


def merge_block(ours: list[str], theirs: list[str], upstream: set[str]) -> list[str]:
    merged: list[str] = []
    index_by_title: dict[str, int] = {}

    for line in ours + theirs:
        if line in merged:
            continue
        title = entry_title(line)
        if title is None:
            merged.append(line)
            continue
        if title not in index_by_title:
            index_by_title[title] = len(merged)
            merged.append(line)
        elif line in upstream:
            merged[index_by_title[title]] = line

    return merged


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--upstream", default="upstream/main", help="ref whose wording wins for duplicate entries")
    args = parser.parse_args()

    upstream_text = subprocess.run(
        ["git", "show", f"{args.upstream}:CHANGELOG.md"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout
    upstream = set(upstream_text.split("\n"))

    lines = CHANGELOG.read_text().split("\n")
    output: list[str] = []
    blocks = 0
    index = 0

    while index < len(lines):
        if not lines[index].startswith("<<<<<<< "):
            output.append(lines[index])
            index += 1
            continue

        separator = next(i for i in range(index, len(lines)) if lines[i] == "=======")
        end = next(i for i in range(separator, len(lines)) if lines[i].startswith(">>>>>>> "))
        output.extend(merge_block(lines[index + 1 : separator], lines[separator + 1 : end], upstream))
        blocks += 1
        index = end + 1

    CHANGELOG.write_text("\n".join(output))
    print(f"Resolved {blocks} conflict block(s) in CHANGELOG.md.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Check that the Ruche fork only changes the files listed in ruche/FORK_SURFACE.txt.

Compares HEAD with its merge base on the upstream branch, lists every changed
path (renames are reported as a deletion plus an addition, so both names must
be allowed), and fails when one of them matches no glob of the surface file.
It also warns when the fork changes more files than the agreed budget.

Usage:
    python3 ruche/scripts/fork_surface.py [--upstream upstream/main] [--budget 30]
"""

import argparse
import fnmatch
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SURFACE_FILE = ROOT / "ruche" / "FORK_SURFACE.txt"


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def load_patterns() -> list[str]:
    patterns = []
    for line in SURFACE_FILE.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            patterns.append(line)
    return patterns


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--upstream", default="upstream/main", help="upstream ref to compare with")
    parser.add_argument("--budget", type=int, default=30, help="warn above this many changed files outside ruche/")
    args = parser.parse_args()

    base = git("merge-base", "HEAD", args.upstream).strip()
    changed = [path for path in git("diff", "--name-only", "--no-renames", base, "HEAD").splitlines() if path]
    patterns = load_patterns()
    outside = [path for path in changed if not any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)]

    # The fork-only tree never conflicts with upstream, so it does not count against the budget.
    budgeted = [path for path in changed if not path.startswith("ruche/")]
    print(
        f"Fork surface: {len(changed)} file(s) differ from {args.upstream} (merge base {base[:10]}), "
        f"{len(budgeted)} outside ruche/."
    )
    if len(budgeted) > args.budget:
        print(f"warning: more than {args.budget} changed files; see section 9 of ruche/docs/plan-demarrage-fork.md")

    if outside:
        print("error: these files are changed but not listed in ruche/FORK_SURFACE.txt:")
        for path in outside:
            print(f"  {path}")
        print("Move the change upstream, or add the path to ruche/FORK_SURFACE.txt deliberately.")
        return 1

    print("All changed files are within the declared fork surface.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

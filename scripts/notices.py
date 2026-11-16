#!/usr/bin/env python3
"""Write crates/morph-desktop/notices.tsv: every crate built into the desktop
shell (normal dependencies, on Linux, macOS and Windows) with its version,
license and repository. The About page's "Open-source notices" shows it.

    scripts/notices.py          # rewrite the file
    scripts/notices.py --check  # fail when it is out of date (CI)
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "morph-desktop" / "notices.tsv"
TARGETS = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"]


def reachable(target):
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target], cwd=ROOT))
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    start = next(p["id"] for p in meta["packages"] if p["name"] == "morph-desktop")
    seen, todo = set(), [start]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] in (None, "normal") for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    return {pkgs[i]["name"] + "\t" + pkgs[i]["version"]: pkgs[i] for i in seen if pkgs[i]["source"]}


def render():
    found = {}
    for t in TARGETS:
        found.update(reachable(t))
    lines = ["# crate\tversion\tlicense\trepository (scripts/notices.py writes this file)"]
    for key in sorted(found, key=str.lower):
        p = found[key]
        lic = (p.get("license") or ("see " + p["license_file"] if p.get("license_file") else "unknown")).replace("\t", " ")
        lines.append(f"{key}\t{lic}\t{p.get('repository') or ''}")
    return "\n".join(lines) + "\n"


def main():
    text = render()
    if "--check" in sys.argv:
        if not OUT.exists() or OUT.read_text() != text:
            print(f"{OUT.relative_to(ROOT)} is out of date: run scripts/notices.py", file=sys.stderr)
            return 1
        return 0
    OUT.write_text(text)
    print(f"{OUT.relative_to(ROOT)}: {len(text.splitlines()) - 1} crates")
    return 0


if __name__ == "__main__":
    sys.exit(main())

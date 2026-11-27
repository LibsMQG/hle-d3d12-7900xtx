# Author: libs (hle-d3d12-amd)
"""Read-only: which pipelines the title created (pipeline book in <cache>/pipelines) hold a geometry,
hull or domain stage, and which hold shaders whose containers carry given names (for example shaders the translator refused), and
whether runs drew with them (early-pipelines.txt).
Usage: python -I book_stages.py <pipelines dir>... [--watch <name substring>...]"""
import argparse
import collections
import re
import struct
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="backslashreplace")
STAGES = ["vertex", "pixel", "domain", "hull", "geometry", "compute", "amplification", "mesh"]
NAME = re.compile(rb"\b(?:GS|VS|PS|CS|HS|DS|MS|AS|ES|LS)_[A-Za-z0-9_]{3,80}")


def shaders(d):
    out = {}
    raw = (d / "known-shaders.bin").read_bytes()
    at = 0
    while at + 37 <= len(raw):
        stage = raw[at]
        h = raw[at + 1:at + 33]
        size = struct.unpack_from("<I", raw, at + 33)[0]
        body = raw[at + 37:at + 37 + size]
        if len(body) < size:
            break
        names = sorted({m.group(0).decode() for m in NAME.finditer(body)})
        out[h] = (STAGES[stage] if stage < 8 else str(stage), names, size)
        at += 37 + size
    return out


def pipelines(d):
    raw = (d / "known-pipelines.bin").read_bytes()
    at = 0
    while at + 4 <= len(raw):
        size = struct.unpack_from("<I", raw, at)[0]
        e = raw[at + 4:at + 4 + size]
        if len(e) < size:
            break
        key = e[:16].hex()
        n = e[16]
        st = []
        for i in range(n):
            o = 17 + 33 * i
            st.append((STAGES[e[o]] if e[o] < 8 else str(e[o]), e[o + 1:o + 33]))
        yield key, st
        at += 4 + size


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+", help="<cache>/pipelines folders")
    ap.add_argument("--watch", nargs="*", default=[], help="substrings of container names to count")
    opts = ap.parse_args()
    WATCH, DIRS = opts.watch, opts.dirs
else:
    WATCH, DIRS = [], []

for arg in DIRS:
    d = Path(arg)
    sh = shaders(d)
    early = set((d / "early-pipelines.txt").read_text(encoding="utf-8", errors="replace").split()) if (d / "early-pipelines.txt").exists() else set()
    print(f"=== {d}: {len(sh)} shaders, early (drawn with) {len(early)}")
    by_stage = collections.Counter(v[0] for v in sh.values())
    print("  shaders by stage:", dict(by_stage))
    combos = collections.Counter()
    hits = collections.defaultdict(lambda: [0, 0, set()])
    total = 0
    for key, st in pipelines(d):
        total += 1
        combo = "+".join(s for s, _ in st)
        drawn = key in early
        combos[(combo, drawn)] += 1
        names = set()
        for s, h in st:
            names.update(sh.get(h, ("?", [], 0))[1])
        for w in WATCH:
            if any(w in n for n in names):
                hits[w][0] += 1
                hits[w][1] += drawn
                hits[w][2].add(combo)
        if any(s in ("geometry", "hull", "domain") for s, _ in st):
            hits["(any geometry/hull/domain stage)"][0] += 1
            hits["(any geometry/hull/domain stage)"][1] += drawn
            hits["(any geometry/hull/domain stage)"][2].add(combo)
    print(f"  pipelines {total}; stage combinations (combo, drawn with): ")
    for (c, dr), n in sorted(combos.items(), key=lambda kv: -kv[1]):
        print(f"     {n:7d}  {c}{'  [drawn with]' if dr else ''}")
    for w, (n, dr, cs) in hits.items():
        print(f"  {w}: {n} pipelines, {dr} drawn with; combos {sorted(cs)}")
    gs_names = collections.Counter()
    for h, (s, names, size) in sh.items():
        if s == "geometry":
            gs_names.update(n.split("_")[0] + "_" + n.split("_")[1] if "_" in n else n for n in names)
    print("  names in geometry-stage containers:", gs_names.most_common(12))

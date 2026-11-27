# Author: libs (hle-d3d12-amd)
"""Read-only: the pipelines of the book holding a shader whose container names include <name substring>
(or whose shader hashes are given), with the 16-digit hash, the matching names and whether runs drew with them.
Usage: python -I book_trace.py <pipelines dir> <name substring> [hash16...]"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import book_stages as b  # noqa: E402  (imported, its own loop runs over no folders)

d, sub = Path(sys.argv[1]), sys.argv[2]
want = set(sys.argv[3:])
sh = b.shaders(d)
early_file = d / "early-pipelines.txt"
early = set(early_file.read_text(encoding="utf-8").split()) if early_file.exists() else set()
found = set()
for key, st in b.pipelines(d):
    rows = []
    hit = False
    for s, h in st:
        stage, names, size = sh.get(h, ("?", [], 0))
        h16 = h.hex()[:16]
        if h16 in want or any(sub in n for n in names):
            hit = True
            found.add(h16)
        rows.append(f"{s} {h16} {','.join(n for n in names if sub in n)[:120]}")
    if hit:
        print(("DRAWN " if key in early else "      ") + key[:12] + "  " + " | ".join(rows))
print("given hashes in the book:", {w: (w in found) for w in want})
names = {}
for h, (s, ns, size) in sh.items():
    for n in ns:
        if sub in n:
            names.setdefault(h.hex()[:16], set()).add(n)
print(f"containers naming {sub} in the shader book:", len(names))
for h16, ns in sorted(names.items()):
    print("  ", h16, sorted(ns)[:4], "(given)" if h16 in want else "")

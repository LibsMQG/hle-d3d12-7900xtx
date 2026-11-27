# Author: libs (hle-d3d12-amd)
"""Print chosen sections of VIctory timeline summaries (.txt), first N lines each. Read only.
Usage: python -I tl_sections.py <n> <file.txt> [...]"""
import sys

sys.stdout.reconfigure(encoding="utf-8")
n = int(sys.argv[1])
WANT = ("the chain back through all frames", "by thread, per frame", "threads, per frame: running")
for path in sys.argv[2:]:
    lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
    print(f"===== {path.split(chr(92))[-1]}: {lines[0]}")
    frames = [l for l in lines if l.startswith("frame of ")]
    print("  frames:", "; ".join(l.split(",")[0].replace("frame of ", "") for l in frames))
    for i, l in enumerate(lines):
        if any(l.startswith(w) for w in WANT):
            print("  --", l)
            for x in lines[i + 1:i + 1 + n]:
                if not x.strip():
                    break
                print("   ", x)

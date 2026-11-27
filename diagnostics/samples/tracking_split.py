# Author: libs (hle-d3d12-amd)
"""Read only: in a sampler CSV, the samples whose leaf is page tracking (GetWriteWatch, VirtualProtect, VirtualQuery,
the exception handler), per thread and per chain of our frames. Usage: python -I tracking_split.py <csv>"""
import collections
import csv
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
LEAF = [("GetWriteWatch", "GetWriteWatch"), ("ProtectVirtualMemory", "VirtualProtect"), ("QueryVirtualMemory", "VirtualQuery"),
        ("KiUserExceptionDispatch", "exception"), ("RtlDispatchException", "exception")]
per_thread = collections.Counter()
total = collections.Counter()
chains = collections.Counter()
names = {}


def ours(stack):
    frames = [f for f in stack.split(" <- ") if "victory_gpu::" in f or "victory_hle::" in f]
    out = []
    for f in frames[:5]:
        f = re.sub(r"<.*?>", "", f).replace("victory.exe!", "").replace("victory_gpu::", "").replace("victory_hle::", "hle::")
        out.append(f[:50])
    return " <- ".join(out) or "(no frame of ours)"


with open(sys.argv[1], encoding="utf-8", errors="replace", newline="") as fh:
    for row in csv.DictReader(fh):
        total[row["tid"]] += 1
        if row["thread"]:
            names[row["tid"]] = row["thread"]
        text = row["function"] + " " + row["stack"]
        kind = next((k for key, k in LEAF if key in row["function"] or key in row["stack"].split(" <- ")[0]), None)
        if kind is None and "exception" not in text.lower():
            continue
        if kind is None:
            kind = "exception path"
        per_thread[(row["tid"], kind)] += 1
        chains[(row["tid"], kind, ours(row["stack"]))] += 1

print("thread, kind: samples (share of that thread's samples)")
for (tid, kind), n in per_thread.most_common(25):
    print(f"  {tid:>6} {names.get(tid, ''):<16} {kind:<16} {n:6}  {100 * n / total[tid]:5.1f}%")
print("\ntop chains:")
for (tid, kind, chain), n in chains.most_common(30):
    print(f"  {tid:>6} {kind:<14} {n:5} {100 * n / total[tid]:5.1f}%  {chain}")

# Author: libs (hle-d3d12-amd)
"""Timeline trace: where the records threads (victory-records-N) and the heap-sync thread wait, and who woke them,
summed over the trace; and their landing jobs. Read only.
Usage: python -I records_waits.py <trace.json>"""
import collections
import json
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
ev = json.load(open(sys.argv[1], encoding="utf-8"))["traceEvents"]
names = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}
want = {t for t, n in names.items() if n.startswith("victory-records") or n == "victory-heap-sync"}
lo = min(e["ts"] for e in ev if e.get("ph") == "i" and e.get("name") == "a frame ends")
hi = max(e["ts"] for e in ev if e.get("ph") == "i" and e.get("name") == "a frame ends")
print(f"window between the first and last frame end: {(hi - lo) / 1000:.1f} ms")
waits = collections.Counter()
count = collections.Counter()
busy = collections.Counter()
for e in ev:
    if e.get("ph") != "X" or e["tid"] not in want:
        continue
    a, b = max(e["ts"], lo), min(e["ts"] + e["dur"], hi)
    if b <= a:
        continue
    who = "heap-sync" if names[e["tid"]] == "victory-heap-sync" else "records"
    if e.get("cat") == "wait":
        at = e.get("args", {}).get("at", "?")
        at = " < ".join(re.sub(r"<.*", "", f) for f in at.split(" < ")[:2])
        woke = e.get("args", {}).get("by", e.get("args", {}).get("woken", ""))
        waits[(who, at[:110], str(woke)[:40])] += b - a
        count[(who, at[:110], str(woke)[:40])] += 1
    else:
        busy[(who, e.get("cat", "-"), e["name"][:60])] += b - a
print("\nspans (time inside, ms):")
for k, v in busy.most_common(10):
    print(f"  {v / 1000:8.2f}  {k[0]:9} {k[1]:7} {k[2]}")
print("\nwaits (ms, count): where")
for k, v in waits.most_common(14):
    print(f"  {v / 1000:8.2f} {count[k]:6d}  {k[0]:9} {k[1]}  {k[2]}")

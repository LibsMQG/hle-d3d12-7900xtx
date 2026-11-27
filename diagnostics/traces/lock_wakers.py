# Author: libs (hle-d3d12-amd)
"""Timeline trace: for the waits on one lock (its address), who woke them and in what span of ours the waker was,
summed by waiter kind, waker kind and the waker's innermost span. Read only.
Usage: python -I lock_wakers.py <trace.json> <lock address hex>"""
import bisect
import collections
import json
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
ev = json.load(open(sys.argv[1], encoding="utf-8"))["traceEvents"]
lock = int(sys.argv[2], 16)
names = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}


def kind(tid):
    n = names.get(tid, "?")
    if n.startswith("victory-image-"):
        return "title"
    return re.sub(r"-\d+$", "-N", n)


starts = {}
finishes = collections.defaultdict(list)
spans = collections.defaultdict(list)
waits = []
for e in ev:
    ph = e.get("ph")
    if ph == "s" and e.get("cat") == "wake":
        starts[e["id"]] = (e["tid"], e["ts"])
    elif ph == "f" and e.get("cat") == "wake":
        finishes[e["tid"]].append((e["ts"], e["id"]))
    elif ph == "X":
        if e.get("cat") == "wait":
            if int(e.get("args", {}).get("object", "0"), 16) == lock:
                waits.append(e)
        elif e.get("cat") in ("Span", "Kernel"):
            spans[e["tid"]].append((e["ts"], e["ts"] + e["dur"], e["name"]))
for l in finishes.values():
    l.sort()
for l in spans.values():
    l.sort()


def innermost(tid, t):
    best = None
    for a, b, n in spans.get(tid, []):
        if a > t:
            break
        if a <= t <= b and (best is None or a >= best[0]):
            best = (a, n)
    return best[1] if best else "(outside our spans)"


total = collections.Counter()
count = collections.Counter()
unmatched = 0.0
for w in waits:
    end = w["ts"] + w["dur"]
    fl = finishes.get(w["tid"], [])
    i = bisect.bisect_left(fl, (end - 30, -1))
    match = None
    while i < len(fl) and fl[i][0] <= end + 30:
        match = fl[i][1]
        break
    if match is None or match not in starts:
        unmatched += w["dur"]
        continue
    wt, wts = starts[match]
    key = (kind(w["tid"]), kind(wt), innermost(wt, wts))
    total[key] += w["dur"]
    count[key] += 1
print(f"{len(waits)} waits on {lock:#x}, {sum(w['dur'] for w in waits) / 1000:.1f} ms; no waker found for {unmatched / 1000:.1f} ms")
print("   ms   waits  waiter      <- woken by     in span")
for k, v in total.most_common(20):
    print(f"{v / 1000:7.1f} {count[k]:6d}  {k[0]:<11} <- {k[1]:<14} {k[2][:70]}")

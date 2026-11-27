# Author: libs (hle-d3d12-amd)
"""Timeline trace: what lies inside the spans named <name> on one thread: nested spans by name, waits by place,
kernel calls, and the time covered by none of them; per frame of the trace's frames. Read only.
Usage: python -I inside_spans.py <trace.json> <thread name> <span name> [<span name>...]"""
import collections
import json
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
path, thread, names = sys.argv[1], sys.argv[2], sys.argv[3:]
ev = json.load(open(path, encoding="utf-8"))["traceEvents"]
tnames = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}
tid = next(t for t, n in tnames.items() if n == thread)
frames = [e["ts"] for e in ev if e.get("ph") == "i" and e.get("name") == "a frame ends" and e.get("tid") is not None]
lo, hi = min(frames), max(frames)
nframes = max(1, len([f for f in frames if lo < f <= hi]))
mine = sorted((e for e in ev if e.get("ph") == "X" and e["tid"] == tid), key=lambda e: (e["ts"], -e["dur"]))
for name in names:
    outer = [e for e in mine if e["name"] == name and lo <= e["ts"] <= hi]
    total = sum(e["dur"] for e in outer)
    inner = collections.Counter()
    covered = 0.0
    for o in outer:
        a, b = o["ts"], o["ts"] + o["dur"]
        # Direct children: events inside, not inside another child.
        kids = [e for e in mine if e is not o and a <= e["ts"] and e["ts"] + e["dur"] <= b + 0.01]
        kids.sort(key=lambda e: (e["ts"], -e["dur"]))
        top, end = [], -1.0
        for k in kids:
            if k["ts"] >= end:
                top.append(k)
                end = k["ts"] + k["dur"]
        for k in top:
            if k.get("cat") == "wait":
                at = k.get("args", {}).get("at", "?")
                at = " < ".join(re.sub(r"<.*", "", f) for f in at.split(" < ")[:2])
                key = f"wait: {at[:90]}"
            elif k.get("cat") == "Kernel":
                key = f"kernel: {k['name']}"
            else:
                key = f"span: {k['name'][:80]}"
            inner[key] += k["dur"]
            covered += k["dur"]
    print(f"== {name}: {len(outer)} spans, {total / 1000 / nframes:.2f} ms a frame over {nframes} frames")
    for k, v in inner.most_common(14):
        print(f"   {v / 1000 / nframes:7.2f} ms  {k}")
    print(f"   {(total - covered) / 1000 / nframes:7.2f} ms  (in none of those: our own work)")

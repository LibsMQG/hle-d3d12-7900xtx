"""Timeline trace: inside one span of the submitting thread (the n-th longest of a name), what every thread did:
per thread, time in our spans by name (outermost within the window), waits, and running outside spans. Read only.
Usage: python -I one_flush.py <trace.json> <span name> [rank 0]"""
import collections
import json
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
path, name = sys.argv[1], sys.argv[2]
rank = int(sys.argv[3]) if len(sys.argv) > 3 else 0
ev = json.load(open(path, encoding="utf-8"))["traceEvents"]
tnames = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}
submit = next(t for t, n in tnames.items() if n == "victory-submit")
target = sorted((e for e in ev if e.get("ph") == "X" and e["tid"] == submit and e["name"] == name), key=lambda e: -e["dur"])[rank]
a, b = target["ts"], target["ts"] + target["dur"]
print(f"{name}: {target['dur'] / 1000:.2f} ms")
per = collections.defaultdict(collections.Counter)
for e in ev:
    if e.get("ph") != "X":
        continue
    o = min(e["ts"] + e["dur"], b) - max(e["ts"], a)
    if o <= 0:
        continue
    n = tnames.get(e["tid"], str(e["tid"]))
    if not (n.startswith("victory-wide") or n == "victory-submit" or n.startswith("victory-under")):
        continue
    if e.get("cat") == "wait":
        at = e.get("args", {}).get("at", "?")
        key = "wait " + " < ".join(re.sub(r"<.*", "", f) for f in at.split(" < ")[:2])[:70]
    elif e.get("cat") == "Kernel":
        key = "kernel " + e["name"]
    else:
        key = e["name"][:60]
    per[n][key] += o
for n in sorted(per):
    c = per[n]
    print(f"-- {n}")
    for k, v in c.most_common(7):
        print(f"   {v / 1000:6.2f} ms  {k}")

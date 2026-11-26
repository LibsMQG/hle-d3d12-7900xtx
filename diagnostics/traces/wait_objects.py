"""Timeline trace: for lock waits at a given place, the objects (lock addresses) waited on: how many distinct,
their span, the busiest ones, and per thread kind. Read only.
Usage: python -I wait_objects.py <trace.json> <substring of 'at'> [...]"""
import collections
import json
import sys

sys.stdout.reconfigure(encoding="utf-8")
ev = json.load(open(sys.argv[1], encoding="utf-8"))["traceEvents"]
subs = sys.argv[2:]
names = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}
for sub in subs:
    objs = collections.Counter()
    time = collections.Counter()
    who = collections.Counter()
    for e in ev:
        if e.get("ph") != "X" or e.get("cat") != "wait":
            continue
        a = e.get("args", {})
        if sub not in a.get("at", ""):
            continue
        o = int(a.get("object", "0"), 16)
        objs[o] += 1
        time[o] += e["dur"]
        n = names.get(e["tid"], "?")
        who[n.rstrip("0123456789").rstrip("-")] += e["dur"]
    if not objs:
        print(f"== '{sub}': none")
        continue
    lo, hi = min(objs), max(objs)
    print(f"== '{sub}': {sum(objs.values())} waits, {sum(time.values()) / 1000:.1f} ms, {len(objs)} distinct objects, span {hi - lo:#x} bytes from {lo:#x}")
    for o, t in time.most_common(8):
        print(f"   {o:#x} (+{o - lo:#x}): {objs[o]} waits, {t / 1000:.2f} ms")
    print("   by thread kind:", ", ".join(f"{k} {v / 1000:.1f} ms" for k, v in who.most_common(6)))

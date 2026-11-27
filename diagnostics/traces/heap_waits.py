# Author: libs (hle-d3d12-7900xtx)
"""Timeline trace: each wait of the submitting thread for the heaps' records, and what the heap-sync thread
(and the records threads) did meanwhile. Read only.
Usage: python -I heap_waits.py <trace.json> [max waits shown]"""
import collections
import json
import sys

sys.stdout.reconfigure(encoding="utf-8")
path = sys.argv[1]
show = int(sys.argv[2]) if len(sys.argv) > 2 else 12
ev = json.load(open(path, encoding="utf-8"))["traceEvents"]
names = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}
tid_of = collections.defaultdict(list)
for t, n in names.items():
    tid_of[n].append(t)
submit = tid_of["victory-submit"][0]
sync = tid_of["victory-heap-sync"][0]
records = {t for n, ts in tid_of.items() if n.startswith("victory-records") for t in ts}
xs = [e for e in ev if e.get("ph") == "X"]
by_tid = collections.defaultdict(list)
for e in xs:
    by_tid[e["tid"]].append(e)
for l in by_tid.values():
    l.sort(key=lambda e: e["ts"])

# What the heap-sync thread records, by name and category.
tot = collections.Counter()
cnt = collections.Counter()
for e in by_tid[sync]:
    k = (e.get("cat", "-"), e["name"])
    tot[k] += e["dur"]
    cnt[k] += 1
frames = [e for e in ev if e.get("ph") == "i" and e.get("name") == "a frame ends"]
span_ms = (max(e["ts"] + e.get("dur", 0) for e in xs) - min(e["ts"] for e in xs)) / 1000
print(f"heap-sync thread {sync}: events by kind over {span_ms:.0f} ms of trace")
for k, v in tot.most_common(14):
    print(f"   {v / 1000:8.2f} ms in {cnt[k]:5d}  {k[0]:8} {k[1][:90]}")

waits = [e for e in by_tid[submit] if e["name"] == "submit: waiting for the heaps' records"]
print(f"\nsubmit waits for the heaps' records: {len(waits)}, {sum(w['dur'] for w in waits) / 1000:.2f} ms in all")


def overlap(e, a, b):
    return max(0.0, min(e["ts"] + e["dur"], b) - max(e["ts"], a))


for w in sorted(waits, key=lambda w: -w["dur"])[:show]:
    a, b = w["ts"], w["ts"] + w["dur"]
    print(f"\n-- wait at {a / 1000:.2f} ms, {w['dur'] / 1000:.2f} ms")
    rounds = [e for e in by_tid[sync] if e["name"].startswith("the heaps' records") and e["ts"] < b and e["ts"] + e["dur"] > a - 20000]
    for r in rounds:
        print(f"   round {r['name'][19:]:<18} from {(r['ts'] - a) / 1000:+7.2f} ms, {r['dur'] / 1000:6.2f} ms, ends {(r['ts'] + r['dur'] - b) / 1000:+6.2f} vs the wait's end")
    inside = collections.Counter()
    for e in by_tid[sync]:
        if e["name"].startswith("the heaps' records"):
            continue
        o = overlap(e, a, b)
        if o > 0:
            inside[(e.get("cat", "-"), e["name"])] += o
    for k, v in inside.most_common(6):
        print(f"      heap-sync meanwhile: {v / 1000:6.2f} ms  {k[0]:8} {k[1][:80]}")
    rec = collections.Counter()
    for t in records:
        for e in by_tid[t]:
            o = overlap(e, a, b)
            if o > 0:
                rec[(e.get("cat", "-"), e["name"])] += o
    for k, v in rec.most_common(3):
        print(f"      records threads meanwhile: {v / 1000:6.2f} ms  {k[0]:8} {k[1][:70]}")

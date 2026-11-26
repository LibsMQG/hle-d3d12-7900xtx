"""Timeline trace: the durations of one span name on one thread, within the recorded frames: the longest ones, the
sum per frame, and the share of the longest tenth. Read only.
Usage: python -I span_sizes.py <trace.json> <thread name> <span name> [top n]"""
import json
import sys

sys.stdout.reconfigure(encoding="utf-8")
path, thread, name = sys.argv[1:4]
top = int(sys.argv[4]) if len(sys.argv) > 4 else 12
ev = json.load(open(path, encoding="utf-8"))["traceEvents"]
tnames = {e["tid"]: e["args"]["name"] for e in ev if e.get("ph") == "M" and e.get("name") == "thread_name"}
tid = next(t for t, n in tnames.items() if n == thread)
frames = sorted(e["ts"] for e in ev if e.get("ph") == "i" and e.get("name") == "a frame ends")
lo, hi = frames[0], frames[-1]
spans = sorted((e for e in ev if e.get("ph") == "X" and e["tid"] == tid and e["name"] == name and lo <= e["ts"] <= hi), key=lambda e: -e["dur"])
total = sum(e["dur"] for e in spans)
n = len(frames) - 1
print(f"{name} on {thread}: {len(spans)} spans, {total / 1000 / max(1, n):.2f} ms a frame over {n} frames")
tenth = spans[: max(1, len(spans) // 10)]
print(f"the longest tenth ({len(tenth)}): {sum(e['dur'] for e in tenth) / max(1, total) * 100:.0f}% of the time")
for e in spans[:top]:
    print(f"   {e['dur'] / 1000:7.2f} ms at {(e['ts'] - lo) / 1000:8.1f} ms into the frames")

# Author: libs (hle-d3d12-7900xtx)
"""Sums the VICTORY_CPU_STATS lines of a log over its last reports, weighted by
frames, and the frame lines of the same stretch. Read only.
Usage: python -I tools/cpustats.py <log> [last reports, default 8]"""
import collections
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
WAIT = re.compile(r"waits before reading what flights write, over (\d+) frames: (.+?): ([\d.]+) a frame, ([\d.]+) ms a frame")
LONG = re.compile(r"cpu stats, longest: (.+?) copy (0x[0-9a-f]+), last range (0x[0-9a-f]+\+0x[0-9a-f]+): ([\d.]+) a frame, ([\d.]+) ms")
QUERY = re.compile(r"VirtualQuery, (.+?): ([\d.]+) askings a frame, ([\d.]+) calls, ([\d.]+) ms a frame, regions of (\d+) MB")
FRAME = re.compile(r" ([\d.]+) ms/frame ")
FILLS = re.compile(r"cpu stats, fills (of upload memory and table constant views, made|waited for at the send, first thing met) \(a frame\): (.+?)(?:; VICTORY_FILL_STICKY=(\d), (\d+) copies sticky)?$")

reports = []  # one per say(): waits, longest, queries
current = None
frames_ms = []
with open(sys.argv[1], encoding="utf-8", errors="replace") as log:
    for line in log:
        if "cpu stats" not in line and "ms/frame" not in line:
            continue
        line = ANSI.sub("", line)
        if m := FRAME.search(line):
            frames_ms.append((line[11:19], float(m[1])))
            continue
        if m := WAIT.search(line):
            if current is None or current["queries"]:
                current = {"time": line[11:19], "frames": 0, "waits": [], "longest": [], "queries": []}
                reports.append(current)
            current["frames"] = int(m[1])
            current["waits"].append((m[2], float(m[3]), float(m[4])))
        elif current is not None and (m := LONG.search(line)):
            current["longest"].append((m[1], m[2], m[3], float(m[4]), float(m[5])))
        elif current is not None and (m := FILLS.search(line.rstrip())):
            which = "made" if m[1].endswith("made") else "waited"
            if m[2] != "none":
                for item in m[2].split(", "):
                    name, value = item.rsplit(" ", 1)
                    current.setdefault(which, []).append((name, float(value)))
            if m[4] is not None:
                current["sticky"] = (m[3], int(m[4]))
        elif current is not None and (m := QUERY.search(line)):
            current["queries"].append((m[1], float(m[2]), float(m[3]), float(m[4]), float(m[5])))

reports = [r for r in reports if r["frames"]]
if not reports:
    sys.exit("no 'cpu stats' wait reports in this log")
last = reports[-(int(sys.argv[2]) if len(sys.argv) > 2 else 8):]
total = sum(r["frames"] for r in last)
since = last[0]["time"]
ms = sorted(v for t, v in frames_ms if t >= since)
print(f"{len(last)} reports from {since} to {last[-1]['time']} UTC, {total} frames; frame time median {ms[len(ms) // 2] if ms else '?'} ms")

waits = collections.defaultdict(lambda: [0.0, 0.0])
queries = collections.defaultdict(lambda: [0.0, 0.0, 0.0, 0.0])
longest = collections.defaultdict(lambda: [0.0, 0.0, "", 0])
for r in last:
    for kind, n, t in r["waits"]:
        waits[kind][0] += n * r["frames"]
        waits[kind][1] += t * r["frames"]
    for name, *values in r["queries"]:
        for i, v in enumerate(values):
            queries[name][i] += v * r["frames"]
    for kind, copy, rng, n, t in r["longest"]:
        e = longest[(kind, copy)]
        e[0] += n * r["frames"]
        e[1] += t * r["frames"]
        e[2] = rng
        e[3] += 1
print("\nwaits, per frame:")
for kind, (n, t) in sorted(waits.items(), key=lambda kv: -kv[1][1]):
    print(f"  {kind:<44} {n / total:5.2f} waits  {t / total:7.1f} ms")
print(f"  {'TOTAL':<44} {sum(v[0] for v in waits.values()) / total:5.2f} waits  {sum(v[1] for v in waits.values()) / total:7.1f} ms")
print("\nlongest causes (a floor: only each report's top six are listed):")
for (kind, copy), (n, t, rng, seen) in sorted(longest.items(), key=lambda kv: -kv[1][1])[:8]:
    print(f"  {kind:<40} copy {copy:<13} range {rng:<22} {n / total:5.2f}  {t / total:6.1f} ms  (in {seen}/{len(last)} reports)")
for which, title in (("made", "fills and table constant views made"), ("waited", "fills waited for at the send (first thing met)")):
    if not any(which in r for r in last):
        continue
    sums = collections.defaultdict(float)
    for r in last:
        for name, v in r.get(which, []):
            sums[name] += v * r["frames"]
    print(f"\n{title}, per frame:")
    for name, v in sorted(sums.items(), key=lambda kv: -kv[1]):
        print(f"  {name:<44} {v / total:7.2f}")
if "sticky" in last[-1]:
    print(f"\nVICTORY_FILL_STICKY={last[-1]['sticky'][0]}, copies sticky at the end: {last[-1]['sticky'][1]}")
print("\nVirtualQuery in watch, per frame:")
for name, (asked, calls, t, mb) in queries.items():
    print(f"  {name:<24} {asked / total:7.1f} askings  {calls / total:7.1f} calls  {t / total:6.2f} ms  regions {mb / total:7.0f} MB")

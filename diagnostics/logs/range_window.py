"""Means over a UTC window of the per-range refresh lines ("copied from A+L ..."), the ten costliest ranges by
time, and the refresh line of the submitting thread. Read only.
Usage: python -I range_window.py <log> <HH:MM:SS> <HH:MM:SS>"""
import collections
import re
import statistics
import sys

sys.stdout.reconfigure(encoding="utf-8")
log, a, b = sys.argv[1:4]
TIME = re.compile(r"T(\d\d:\d\d:\d\d)\.")
RANGE = re.compile(
    r"copied from (0x[0-9a-f]+)\+(0x[0-9a-f]+) \(memory (0x[0-9a-f]+)\): ([\d.]+)x/frame, ([\d.]+) MB/frame, ([\d.]+) ms/frame; "
    r"([\d.]+) written blocks found a frame, ([\d.]+) MB of them copied, ([\d.]+) ms asking, ([\d.]+) ms copying"
)
PREP = re.compile(r"refreshes ([\d.]+) in (\d+) flushes of (\d+), (\d+) of them known to copy nothing")
FRAME = re.compile(r"victory_gpu::host: [\d.]+ fps, ([\d.]+) ms/frame")
ranges = collections.defaultdict(list)
prep, frames, lines = [], [], 0
for line in open(log, encoding="utf-8", errors="replace"):
    m = TIME.search(line)
    if not m or not (a <= m.group(1) <= b):
        continue
    if (r := RANGE.search(line)):
        lines += 1
        ranges[(r.group(1), r.group(2), r.group(3))].append([float(x) for x in r.groups()[3:]])
    elif (p := PREP.search(line)):
        prep.append([float(x) for x in p.groups()])
    elif (f := FRAME.search(line)):
        frames.append(float(f.group(1)))
print(f"window {a}-{b}: {lines} range lines, {len(prep)} preparing lines, frames median {statistics.median(frames) if frames else 0:.1f} ms")
if prep:
    c = list(zip(*prep))
    print(f"submit thread refreshes: {statistics.mean(c[0]):.1f} ms a frame in {statistics.mean(c[1]):.0f} flushes of {statistics.mean(c[2]):.0f}, "
          f"{statistics.mean(c[3]):.0f} known to copy nothing")
names = ["x/frame", "MB/frame", "ms/frame", "written blocks", "MB copied", "ms asking", "ms copying"]
rows = []
for key, vals in ranges.items():
    # A range that is not among the top ten in a line counts as 0 there.
    n = len(vals)
    mean = [sum(v[i] for v in vals) / max(1, len(prep) or n) for i in range(7)]
    rows.append((key, n, mean))
rows.sort(key=lambda r: -r[2][2])
print(f"{'range':<34} {'kind':>8} {'lines':>5} " + " ".join(f"{n:>14}" for n in names))
for (start, size, kind), n, mean in rows[:12]:
    print(f"{start + '+' + size:<34} {kind:>8} {n:>5} " + " ".join(f"{v:14.2f}" for v in mean))

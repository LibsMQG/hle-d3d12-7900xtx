# Author: libs (hle-d3d12-amd)
"""Means per time window of the heap-records lines and CPU_STATS lines of a VIctory log. Read only.
Usage: python -I windows_stats.py <log> <name=HH:MM:SS-HH:MM:SS> [...]"""
import re
import statistics
import sys

sys.stdout.reconfigure(encoding="utf-8")
log = sys.argv[1]
wins = []
for w in sys.argv[2:]:
    name, span = w.split("=")
    a, b = span.split("-")
    wins.append((name, a, b))
TIME = re.compile(r"T(\d\d:\d\d:\d\d)\.")
REC = re.compile(
    r"per frame ([\d.]+) changed heap pages, (\d+) slots looked at, ([\d.]+) whose record was new and was mirrored, "
    r"([\d.]+) new ones of draws' tables left to the draws, ([\d.]+) that waited for a draw first, ([\d.]+) ms \(([\d.]+) ms waited for\).*?"
    r"([\d.]+) MB read; ([\d.]+) ms asking, ([\d.]+) reading and comparing, ([\d.]+) making"
)
REC_NAMES = ["pages", "slots looked", "mirrored", "left to draws", "waited for a draw", "round ms", "submit waited ms", "MB read", "asking ms", "reading ms", "making ms"]
ALL = re.compile(r"waits for flights over \d+ frames: ([\d.]+) ms a frame in all")
KIND = re.compile(r"waits for flights over \d+ frames: ([^:]+(?:\([^)]*\))?): ([\d.]+) a frame, ([\d.]+) ms a frame")
FOLLOW = re.compile(r"waits followed to their end over \d+ frames: (.+?): ([\d.]+) a frame, ([\d.]+) of them with read bytes shaders changed meanwhile, ([\d.]+) KB")
data = {n: {"rec": [], "all": [], "kind": {}, "follow": {}} for n, _, _ in wins}
with open(log, encoding="utf-8", errors="replace") as f:
    for line in f:
        m = TIME.search(line)
        if not m:
            continue
        t = m.group(1)
        for n, a, b in wins:
            if not (a <= t <= b):
                continue
            d = data[n]
            if (r := REC.search(line)):
                d["rec"].append([float(x) for x in r.groups()])
            elif (r := ALL.search(line)):
                d["all"].append(float(r.group(1)))
            elif (r := FOLLOW.search(line)):
                d["follow"].setdefault(r.group(1), []).append((float(r.group(2)), float(r.group(3)), float(r.group(4))))
            elif (r := KIND.search(line)):
                d["kind"].setdefault(r.group(1).strip(), []).append(float(r.group(3)))
for n, a, b in wins:
    d = data[n]
    print(f"== {n} {a}-{b}")
    if d["rec"]:
        cols = list(zip(*d["rec"]))
        print(f"  heap records ({len(d['rec'])} lines, per frame, mean): " + ", ".join(f"{k} {statistics.mean(c):.1f}" for k, c in zip(REC_NAMES, cols)))
    if d["all"]:
        print(f"  flight waits in all ({len(d['all'])} lines): mean {statistics.mean(d['all']):.1f} ms a frame")
    lines = max(1, len(d["all"]))
    for k, v in sorted(d["kind"].items(), key=lambda kv: -sum(kv[1])):
        print(f"    {k}: {sum(v) / lines:.2f} ms a frame (mean over the window's lines)")
    for k, v in d["follow"].items():
        n_, w_, kb = (sum(x[i] for x in v) / len(v) for i in range(3))
        print(f"    followed {k}: {n_:.2f} a frame, {w_:.2f} with bytes changed, {kb:.1f} KB")

"""Means per UTC window of the ASYNC_SUBMIT line ('submissions prepared by a thread of ours') and the frame lines. Read only.
Usage: python -I line_stats.py <log> <name=HH:MM:SS-HH:MM:SS> [...]"""
import re
import statistics
import sys

sys.stdout.reconfigure(encoding="utf-8")
log = sys.argv[1]
wins = [(w.split("=")[0], *w.split("=")[1].split("-")) for w in sys.argv[2:]]
TIME = re.compile(r"T(\d\d:\d\d:\d\d)\.")
LINE = re.compile(
    r"per frame \(VICTORY_ASYNC_SUBMIT\): ([\d.]+) in ([\d.]+) presents, ([\d.]+) in the line when one was added \(at most (\d+)\); "
    r"that thread worked ([\d.]+) ms; threads of the title waited ([\d.]+) ms for room in the line, ([\d.]+) ms at presents, ([\d.]+) ms in ([\d.]+) drains"
)
FRAME = re.compile(r"victory_gpu::host: [\d.]+ fps, ([\d.]+) ms/frame \(([\d.]+) preparing")
data = {w[0]: {"line": [], "frame": []} for w in wins}
with open(log, encoding="utf-8", errors="replace") as f:
    for line in f:
        m = TIME.search(line)
        if not m:
            continue
        t = m.group(1)
        for name, a, b in wins:
            if a <= t <= b:
                if (r := LINE.search(line)):
                    data[name]["line"].append([float(x) for x in r.groups()])
                elif (r := FRAME.search(line)):
                    data[name]["frame"].append((float(r.group(1)), float(r.group(2))))
for name, a, b in wins:
    d = data[name]
    print(f"== {name} {a}-{b}: {len(d['frame'])} frame lines, {len(d['line'])} line stats")
    if d["frame"]:
        print(f"   ms/frame median {statistics.median(x[0] for x in d['frame']):.1f}, preparing median {statistics.median(x[1] for x in d['frame']):.1f}")
    if d["line"]:
        c = list(zip(*d["line"]))
        print(f"   submissions {statistics.mean(c[0]):.1f}/frame, in line {statistics.mean(c[2]):.1f}; our thread worked {statistics.mean(c[4]):.1f} ms; "
              f"title waited: room {statistics.mean(c[5]):.1f} ms, at presents {statistics.mean(c[6]):.1f} ms, drains {statistics.mean(c[7]):.1f} ms in {statistics.mean(c[8]):.2f}")

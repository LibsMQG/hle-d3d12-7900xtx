"""Start of a run: from the first frame line with >1000 draws, how long until frames settle (median of 5 lines < 130 ms),
and the texture MB uploaded meanwhile. Read only.
Usage: python -I start_phase.py <log> [<log>...]"""
import re
import statistics
import sys
from datetime import datetime

sys.stdout.reconfigure(encoding="utf-8")
RX = re.compile(r"(\d\d:\d\d:\d\d\.\d+)Z.*?([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing.*?textures ([\d.]+) ms ([\d.]+) MB.*?(\d+) draws/frame")
for path in sys.argv[1:]:
    rows = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            if "victory_gpu::host:" not in line:
                continue
            m = RX.search(line)
            if m:
                t = datetime.strptime(m.group(1)[:15], "%H:%M:%S.%f")
                rows.append((t, float(m.group(3)), float(m.group(6)), int(m.group(7))))
    start = next((i for i, r in enumerate(rows) if r[3] > 1000), None)
    if start is None:
        print(path, "no scene")
        continue
    settle = next((i for i in range(start, len(rows) - 5) if statistics.median(r[1] for r in rows[i:i + 5]) < 130), None)
    end = settle if settle is not None else len(rows) - 1
    secs = (rows[end][0] - rows[start][0]).total_seconds()
    mb = sum(r[2] for r in rows[start:end])
    worst = max(r[1] for r in rows[start:end + 1])
    print(f"{path.split(chr(92))[-1]}: scene at {rows[start][0]:%H:%M:%S}, settled after {secs:.0f} s "
          f"({end - start} frame lines, worst {worst:.0f} ms, texture MB/frame summed over the lines {mb:.0f})")

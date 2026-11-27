# Author: libs (hle-d3d12-amd)
"""Frame blocks of a VIctory log between two UTC times (HH:MM:SS), e.g. a walk: count, median, p90 and max ms/frame,
median draws, card wait; and the VICTORY_DISK_STATS reports in the same window (MB/s from disk). Read only.
Usage: python -I tools/window_frames.py <log> <from HH:MM:SS> <to HH:MM:SS>"""
import re
import statistics
import sys

sys.stdout.reconfigure(encoding="utf-8")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
FRAME = re.compile(r"([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing.*?; ([\d.]+) on the GPU.*?(\d+) draws/frame")
DISK = re.compile(r"dstorage: last ([\d.]+) s")
FIELD = re.compile(r"(requests|mb_per_s|busy_workers|bcpack_mb)=([\d.]+)")

log, start, end = sys.argv[1], sys.argv[2], sys.argv[3]
frames, disk = [], []
with open(log, encoding="utf-8", errors="replace") as f:
    for line in f:
        line = ANSI.sub("", line)
        t = line[11:19]
        if not (start <= t <= end):
            continue
        if m := FRAME.search(line):
            frames.append((t, float(m[2]), float(m[3]), float(m[4]), int(m[5])))
        elif (m := DISK.search(line)) and "INFO" in line:
            d = dict((k, float(v)) for k, v in FIELD.findall(line))
            disk.append((t, float(m[1]), d))
if not frames:
    sys.exit("no frame blocks in that window")
ms = sorted(r[1] for r in frames)
p90 = ms[min(len(ms) - 1, int(round(0.9 * (len(ms) - 1))))]
print(f"{len(frames)} blocks {frames[0][0]}-{frames[-1][0]} UTC")
print(f"ms/frame: median {statistics.median(ms):.1f}, p90 {p90:.1f}, max {ms[-1]:.1f}, min {ms[0]:.1f}")
print(f"preparing median {statistics.median(r[2] for r in frames):.1f}, card median {statistics.median(r[3] for r in frames):.1f}, "
      f"draws median {statistics.median(r[4] for r in frames):.0f}")
if disk:
    secs = sum(d[1] for d in disk)
    mb = sum(d[2].get("mb_per_s", 0) * d[1] for d in disk)
    req = sum(d[2].get("requests", 0) for d in disk)
    print(f"disk (VICTORY_DISK_STATS): {len(disk)} reports over {secs:.0f} s, {mb / secs:.1f} MB/s on average, "
          f"peak {max(d[2].get('mb_per_s', 0) for d in disk):.1f} MB/s, {req / secs:.1f} requests/s")
    print("note: a report comes only when a read finishes after 5 s; seconds without reads stretch the next one.")
else:
    print("disk: no VICTORY_DISK_STATS report in the window")

"""Sums the VICTORY_CHANGED_BYTES lines of a diagnostic log over its last
reports, per reservation, weighted by frames. Read only.
Usage: python -I tools/changed.py <log> [last reports, default 8]"""
import collections
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
HEAD = re.compile(r"^(\S+).*changed bytes over (\d+) frames; looking took ([\d.]+) ms")
ROW = re.compile(
    r"changed bytes: reservation (0x[0-9a-f]+) protect (0x[0-9a-f]+) (to-card|pc-only): per frame ([\d.]+) copies, "
    r"copied ([\d.]+) MB, differ ([\d.]+) MB, in 256 B blocks ([\d.]+) MB, in 4 KiB blocks ([\d.]+) MB, not read ([\d.]+) MB"
)

reports = []  # (time, frames, looking ms, rows)
with open(sys.argv[1], encoding="utf-8", errors="replace") as log:
    for line in log:
        if "changed bytes" not in line:
            continue
        line = ANSI.sub("", line)
        head = HEAD.search(line)
        if head:
            reports.append((head[1][11:19], int(head[2]), float(head[3]), []))
            continue
        row = ROW.search(line)
        if row and reports:
            reports[-1][3].append(row.groups())

if not reports:
    sys.exit("no 'changed bytes' reports in this log")
last = reports[-(int(sys.argv[2]) if len(sys.argv) > 2 else 8):]
frames = sum(r[1] for r in last)
sums = collections.defaultdict(lambda: [0.0] * 6)
for _, n, _, rows in last:
    for start, protect, side, *values in rows:
        total = sums[(start, protect, side)]
        for i, value in enumerate(values):
            total[i] += float(value) * n
print(f"{len(last)} reports ({last[0][0]}-{last[-1][0]} UTC), {frames} frames; the extra look took "
      f"{sum(r[2] * r[1] for r in last) / frames:.0f} ms a frame, summed over threads")
print("per frame, MB:")
print(f"{'reservation':>13} {'protect':>8} {'side':>8} {'copies':>7} {'copied':>8} {'differ':>8} {'256 B':>8} {'4 KiB':>8} {'unread':>7}")
grand = collections.defaultdict(lambda: [0.0] * 6)
for (start, protect, side), v in sorted(sums.items(), key=lambda kv: -kv[1][1]):
    a = [x / frames for x in v]
    for i in range(6):
        grand[side][i] += a[i]
    print(f"{start:>13} {protect:>8} {side:>8} {a[0]:>7.0f} {a[1]:>8.2f} {a[2]:>8.3f} {a[3]:>8.3f} {a[4]:>8.3f} {a[5]:>7.2f}")
for side, a in grand.items():
    share = lambda x: f"{100 * x / a[1]:.1f}%" if a[1] else "-"
    print(f"\nTOTAL {side}: copied {a[1]:.1f} MB/frame; differ {a[2]:.2f} ({share(a[2])}); "
          f"256 B blocks {a[3]:.2f} ({share(a[3])}); 4 KiB blocks {a[4]:.2f} ({share(a[4])}); unread {a[5]:.2f}")

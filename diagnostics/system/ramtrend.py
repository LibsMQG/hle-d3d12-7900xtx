"""The game's memory over time, minute by minute, from counters.ps1 CSVs.
The CSVs hold some decimals with a comma (locales with a decimal comma), so the columns after
commit_total_mb are read from the end of the line. Read only.
Usage: python -I tools/ramtrend.py <counters.csv> [...]"""
import collections
import sys

sys.stdout.reconfigure(encoding="utf-8")
for path in sys.argv[1:]:
    minutes = collections.OrderedDict()
    with open(path, encoding="ascii", errors="replace") as f:
        next(f)
        for line in f:
            c = line.strip().split(",")
            if len(c) < 11:
                continue
            try:
                row = (int(c[6]), int(c[7]), int(c[8]), int(c[-2]), int(c[-1]))
            except ValueError:
                continue
            minutes.setdefault(c[0][:5], []).append(row)
    print(f"== {path.split(chr(92))[-1]}  (GB, median of each minute)")
    print(f"{'time':>6} {'working set':>12} {'private':>9} {'system commit':>14} {'VRAM':>7} {'shared GPU':>11}")
    for minute, rows in minutes.items():
        mid = [sorted(r[i] for r in rows)[len(rows) // 2] / 1024 for i in range(5)]
        print(f"{minute:>6} {mid[0]:>12.1f} {mid[1]:>9.1f} {mid[2]:>14.1f} {mid[3]:>7.1f} {mid[4]:>11.1f}")

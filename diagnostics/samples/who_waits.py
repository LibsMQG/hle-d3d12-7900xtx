# Author: libs (hle-d3d12-7900xtx)
"""Per thread: wait samples whose stack goes through a given substring. Read only.
Usage: python -I who_waits.py <csv> <substring> [substring...]"""
import collections
import csv
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, subs = sys.argv[1], sys.argv[2:]
WAIT = re.compile(r"WaitOnAddress|WaitForAlertByThreadId|WaitForSingleObject")
total = collections.Counter()
hit = collections.Counter()
name = {}
for row in csv.DictReader(open(path, encoding="utf-8", errors="replace", newline="")):
    t = row["tid"]
    name[t] = row["thread"] or "-"
    total[t] += 1
    if WAIT.search(row["function"]) and any(s in row["stack"] for s in subs):
        hit[t] += 1
print("tid    thread                      samples  waits-through  share")
for t, v in hit.most_common(25):
    print(f"{t:>6} {name[t][:26]:<26} {total[t]:>8} {v:>14} {100 * v / total[t]:6.1f}%")
print("sum of shares (x frame ms = ms of waiting summed over threads):", round(sum(100 * v / total[t] for t, v in hit.items()), 1), "%")

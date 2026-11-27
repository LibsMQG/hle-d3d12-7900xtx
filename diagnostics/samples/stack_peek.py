"""Print a few raw stacks of a thread's lock-wait samples from a sampler CSV. Read only.
Usage: python -I stack_peek.py <csv> <tid> [n]"""
import collections
import csv
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, tid = sys.argv[1], sys.argv[2]
n = int(sys.argv[3]) if len(sys.argv) > 3 else 4
depth = collections.Counter()
shown = 0
with open(path, encoding="utf-8", errors="replace", newline="") as f:
    for row in csv.DictReader(f):
        if row["tid"] != tid:
            continue
        stack = row["stack"].split(";") if row["stack"] else []
        depth[len(stack)] += 1
        if "WaitOnAddress" in row["function"] and shown < n:
            shown += 1
            print(f"--- leaf {row['function']} depth {len(stack)}")
            for fr in stack[:30]:
                print("   ", fr[:160])
print("stack depth histogram:", sorted(depth.items())[:20])

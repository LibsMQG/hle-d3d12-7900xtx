# Author: libs (hle-d3d12-amd)
"""Raw stacks of wait samples whose stack has `needle_lock` then `needle_fn`, for some threads. Read only.
Usage: python -I raw_mutex_stacks.py <csv> <needle_lock> <needle_fn> <n> <tid> [tid...]"""
import collections
import csv
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, lock, fn, n = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
tids = set(sys.argv[5:])
shown = 0
shapes = collections.Counter()
for row in csv.DictReader(open(path, encoding="utf-8", errors="replace", newline="")):
    if row["tid"] not in tids:
        continue
    st = row["stack"]
    if lock in st and fn in st:
        frames = [x.strip() for x in st.split(" <- ")]
        i = next(k for k, f in enumerate(frames) if fn in f)
        shapes[" <- ".join(f[:70] for f in frames[max(0, i - 4): i + 2])] += 1
        if shown < n:
            shown += 1
            print(f"--- leaf {row['function']}+{row['offset']}")
            for f in frames[: i + 3]:
                print("   ", f[:150])
print("\nshapes around the function (count):")
for k, v in shapes.most_common(8):
    print(f"{v:5d}  {k}")

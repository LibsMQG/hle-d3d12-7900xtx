"""Sampler CSV: one thread's samples whose stack goes through one of the given functions: by leaf, and by the first
two frames of ours, busy and waiting apart. Shares are of the thread's samples. Read only.
Usage: python -I refresh_leaves.py <csv> <tid> <substring> [substring...]"""
import collections
import csv
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, tid, subs = sys.argv[1], sys.argv[2], sys.argv[3:]
WAIT = re.compile(r"WaitOnAddress|WaitForAlertByThreadId|WaitForSingleObject|WaitForMultipleObjects")
total = 0
hit = 0
leaves = collections.Counter()
ours = collections.Counter()
for row in csv.DictReader(open(path, encoding="utf-8", errors="replace", newline="")):
    if row["tid"] != tid:
        continue
    total += 1
    stack = row["stack"]
    if not any(s in stack for s in subs):
        continue
    hit += 1
    frames = [row["function"]] + [x.strip() for x in stack.split(" <- ") if x.strip()]
    wait = bool(WAIT.search(row["function"]))
    leaf = re.sub(r"\+0x[0-9a-f]+$", "", row["function"])
    leaf = re.sub(r"<.*", "", leaf.replace("victory.exe!", ""))[:70]
    mine = [re.sub(r"<.*", "", f.replace("victory.exe!", "").replace("victory_gpu::", "")) for f in frames if "victory_gpu::" in f][:2]
    kind = "wait" if wait else "busy"
    leaves[(kind, leaf)] += 1
    ours[(kind, " < ".join(mine))] += 1
print(f"{total} samples of the thread, {hit} ({100 * hit / max(1, total):.1f}%) through {subs}")
print("-- by leaf")
for (k, l), v in leaves.most_common(18):
    print(f"  {100 * v / total:5.1f}%  {k}  {l}")
print("-- by our first two frames")
for (k, l), v in ours.most_common(18):
    print(f"  {100 * v / total:5.1f}%  {k}  {l[:120]}")

# Author: libs (hle-d3d12-7900xtx)
"""Group a thread's samples by the first frames of ours above the leaf. Read only.
Stacks in the sampler CSV are one field, frames joined by ' <- ' (leaf first).
Usage: python -I lock_callers.py <csv> <tid> [only-waits 0/1] [frames 2]"""
import collections
import csv
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, tid = sys.argv[1], sys.argv[2]
only_waits = len(sys.argv) > 3 and sys.argv[3] == "1"
keep = int(sys.argv[4]) if len(sys.argv) > 4 else 2
WAIT = re.compile(r"WaitOnAddress|WaitForAlertByThreadId|WaitForSingleObject|WaitForMultipleObjects")
LOCK = re.compile(r"(RwLock::(read|write)_contended|Mutex::lock_contended|futex::Mutex|Condvar::wait|parking|Once)")
total = 0
groups = collections.Counter()
locks = collections.Counter()
for row in csv.DictReader(open(path, encoding="utf-8", errors="replace", newline="")):
    if row["tid"] != tid:
        continue
    total += 1
    frames = [row["function"]] + [x.strip() for x in row["stack"].split(" <- ") if x.strip()]
    is_wait = bool(WAIT.search(row["function"]))
    if only_waits and not is_wait:
        continue
    lock = next((m.group(1) for fr in frames if (m := LOCK.search(fr))), "-")
    ours = [re.sub(r"^victory\.exe!", "", fr) for fr in frames if "victory_gpu::" in fr or "victory_hle::" in fr]
    ours = [re.sub(r"<.*", "", fr) for fr in ours][:keep]
    key = (lock if is_wait else "busy") + " | " + " < ".join(ours or ["(none of ours)"])
    groups[key] += 1
    if is_wait:
        locks[lock] += 1
print(f"{total} samples; waits by primitive:", dict(locks))
for k, v in groups.most_common(30):
    print(f"{100 * v / total:5.1f}%  {k[:170]}")

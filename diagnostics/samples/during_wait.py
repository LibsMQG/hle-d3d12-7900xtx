# Author: libs (hle-d3d12-7900xtx)
"""At the ticks where thread A's stack holds `needle`, what thread B is doing (its leaf kind and first frames of ours). Read only.
Usage: python -I during_wait.py <csv> <tid A> <needle> <tid B> [frames 3]"""
import collections
import csv
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, ta, needle, tb = sys.argv[1:5]
keep = int(sys.argv[5]) if len(sys.argv) > 5 else 3
WAIT = re.compile(r"WaitOnAddress|WaitForAlertByThreadId|WaitForSingleObject|WaitForMultipleObjects")
LOCK = re.compile(r"(RwLock::(read|write)_contended|Mutex::lock_contended|futex::Mutex|Condvar::wait)")
a_hit = set()
b_at = {}
for row in csv.DictReader(open(path, encoding="utf-8", errors="replace", newline="")):
    t = row["t_ms"]
    if row["tid"] == ta and needle in row["stack"]:
        a_hit.add(t)
    elif row["tid"] == tb:
        b_at[t] = row


def kind(row):
    frames = [row["function"]] + [x.strip() for x in row["stack"].split(" <- ") if x.strip()]
    is_wait = bool(WAIT.search(row["function"]))
    lock = next((m.group(1) for fr in frames if (m := LOCK.search(fr))), "-") if is_wait else "busy"
    ours = [re.sub(r"<.*", "", re.sub(r"^victory\.exe!", "", fr)) for fr in frames if "victory_gpu::" in fr]
    leaf = re.sub(r"\+0x.*", "", row["function"])[:40] if not is_wait and not ours[:1] else ""
    return f"{lock} | " + " < ".join(ours[:keep] or [leaf or "(none of ours)"])


groups = collections.Counter(kind(b_at[t]) for t in a_hit if t in b_at)
n = sum(groups.values())
print(f"{len(a_hit)} ticks with A in '{needle}'; B seen at {n}")
for k, v in groups.most_common(18):
    print(f"{100 * v / max(1, n):5.1f}%  {k[:180]}")

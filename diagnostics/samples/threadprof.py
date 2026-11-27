# Author: libs (hle-d3d12-7900xtx)
"""For chosen threads of a sampler CSV: where each sample's wall time goes.
Every sample counts the same (a share of the thread's wall time), whether the
thread was on a core or blocked. For each, the leaf function and the nearest
frames of our own module on the stack. Read only.
Usage: python -I tools/threadprof.py <samples.csv> <tid> [tid...]"""
import collections
import csv
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
csv.field_size_limit(10**8)
path, wanted = sys.argv[1], set(sys.argv[2:])
samples = collections.defaultdict(list)
with open(path, encoding="utf-8", errors="replace", newline="") as f:
    for row in csv.DictReader(f):
        if row["tid"] in wanted:
            samples[row["tid"]].append(row)


def short(name):
    name = re.sub(r"<.*>", "", name)
    return name.replace("victory.exe!", "").replace("victory_gpu::", "").replace("victory_hle::", "hle::")[:90]


KERNEL = [
    ("ReadVirtualMemory", "read title memory (ReadProcessMemory)"),
    ("WriteVirtualMemory", "write title memory (NtWriteVirtualMemory)"),
    ("ProtectVirtualMemory", "change page protection (VirtualProtect)"),
    ("QueryVirtualMemory", "ask what pages are (VirtualQuery)"),
    ("GetWriteWatch", "ask which pages were written (GetWriteWatch)"),
    ("WaitForAlertByThreadId", "blocked on a lock or condition"),
    ("RtlWaitOnAddress", "blocked on a lock or condition"),
    ("DelayExecution", "sleeping"),
    ("WaitForSingleObject", "blocked on an event (card, thread or handle)"),
    ("WaitForMultipleObjects", "blocked on events"),
    ("D3DKMT", "graphics kernel call"),
    ("NtGdiDdDDI", "graphics kernel call"),
]
for tid, rows in samples.items():
    n = len(rows)
    span = (float(rows[-1]["t_ms"]) - float(rows[0]["t_ms"])) / 1000
    kinds, whys = collections.Counter(), collections.defaultdict(collections.Counter)
    for r in rows:
        leaf = r["function"]
        kind = next((label for key, label in KERNEL if key in leaf), None)
        if kind is None:
            module = r["module"].lower()
            if module == "victory.exe":
                kind = "our own code"
            elif module.startswith("victory-image"):
                kind = "game code"
            elif "amd" in module or module.startswith("d3d12") or module.startswith("dxgi"):
                kind = "driver / Direct3D code"
            else:
                kind = "other: " + module
        kinds[kind] += 1
        own = [short(x) for x in re.split(r"[|;]", r["stack"] or "") if "victory.exe!" in x or "victory_gpu::" in x]
        if r["module"].lower() == "victory.exe":
            own = [short(leaf)] + own
        own = [o for i, o in enumerate(own) if i == 0 or o != own[i - 1]]
        whys[kind][" < ".join(own[:3]) or "(no frame of ours in the walk)"] += 1
    print(f"\n######## thread {tid} {rows[0]['thread']}: {n} samples over {span:.0f} s")
    for kind, count in kinds.most_common(12):
        print(f"{100 * count / n:5.1f}%  {kind}")
        if count / n >= 0.04:
            for why, c in whys[kind].most_common(5):
                print(f"           {100 * c / n:5.1f}%  {why}")

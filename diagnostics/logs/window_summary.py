"""Summary numbers of one window of a run (for example standing still in one spot). Read only.
Usage: python -I window_summary.py <log> <from HH:MM:SS UTC> <to HH:MM:SS UTC> [counters.csv]

From the log, inside the window: the submitting thread's work and the title's wait at presents
(VICTORY_ASYNC_SUBMIT line), the card's memory in use (memory housekeeping line), the process's
working set (pool line). From counters.csv (system/counters.ps1), if given: working set, private
bytes, commit, the GPU's dedicated and shared memory for the process, 3D engine use.
Frames: run logs/window_frames.py on the same window."""
import csv
import re
import statistics
import sys

sys.stdout.reconfigure(encoding="utf-8")
log, start, end = sys.argv[1], sys.argv[2], sys.argv[3]
counters = sys.argv[4] if len(sys.argv) > 4 else None
ansi = re.compile(r"\x1b\[[0-9;]*m")
worked, presents, card, ws = [], [], [], []
with open(log, encoding="utf-8", errors="replace") as f:
    for line in f:
        line = ansi.sub("", line)
        m = re.match(r"\d{4}-\d\d-\d\dT(\d\d:\d\d:\d\d)", line)
        if not m or not (start <= m.group(1) <= end):
            continue
        if "that thread worked" in line:
            a = re.search(r"that thread worked ([\d.]+) ms", line)
            b = re.search(r"([\d.]+) ms at presents", line)
            if a:
                worked.append(float(a.group(1)))
            if b:
                presents.append(float(b.group(1)))
        if "memory housekeeping: card" in line:
            c = re.search(r"card (\d+) MB in use", line)
            if c:
                card.append(int(c.group(1)))
        w = re.search(r"working set (\d+) MB", line)
        if w:
            ws.append(int(w.group(1)))


def med(v, fmt="{:.1f}"):
    return fmt.format(statistics.median(v)) + f" (n {len(v)})" if v else "-"


print(f"window {start}-{end} UTC of {log}")
print(f"submitting thread worked, ms a frame: median {med(worked)}")
print(f"title waited at presents, ms a frame: median {med(presents)}")
print(f"card memory in use (log), MB: median {med(card, '{:.0f}')}")
print(f"working set (log), MB: median {med(ws, '{:.0f}')}")
if counters:
    rows = []
    with open(counters, encoding="utf-8", errors="replace", newline="") as f:
        for r in csv.DictReader(f):
            rows.append(r)
    for key, label in (("working_set_mb", "working set MB"), ("private_mb", "private bytes MB"), ("commit_total_mb", "system commit MB"),
                       ("gpu_dedicated_mb", "GPU dedicated (VRAM) MB"), ("gpu_shared_mb", "GPU shared MB"), ("gpu_3d_pct", "GPU 3D engine %"),
                       ("proc_cpu_pct", "process CPU %")):
        v = [float(r[key]) for r in rows if r.get(key) not in (None, "", "NA")]
        print(f"counters {label}: median {med(v, '{:.0f}')}")

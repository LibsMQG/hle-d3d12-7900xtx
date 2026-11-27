"""VICTORY_AB run with one switch (any): phase medians per side, each ON phase against the mean
of its OFF neighbours, and the counters of the lines logged inside each phase (5 s or more after its start). Read only.
Usage: python -I ab_fault.py <log> <first phase> <last phase>"""
import re
import statistics
import sys
from datetime import datetime

sys.stdout.reconfigure(encoding="utf-8")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
TS = re.compile(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+)Z")
PHASE = re.compile(r"A/B phase (\d+) bits (\d+) on \[[^\]]*\]: (\d+) frames, mean ([\d.]+) ms, median ([\d.]+), p90 ([\d.]+), (\d+) draws")
RX = {
    "asking": re.compile(r"asking about title memory per frame, all threads: (\d+) kernel askings over (\d+) MB \(([\d.]+) ms.*?(\d+) blocks made read-only, (\d+) opened by ourselves; ([\d.]+) ms waiting for the guards, ([\d.]+) ms changing protections"),
    "copies": re.compile(r"copystat: copies per frame: .*?(\d+) guard faults"),
    "recording": re.compile(r"copystat: recording per frame .*?: (\d+) list calls ([\d.]+) ms .*?guards ([\d.]+) ms"),
    "landing": re.compile(r"of the threads' time ([\d.]+) ms waiting for the title's bytes going in, ([\d.]+) ms opening or noting guards"),
    "line": re.compile(r"that thread worked ([\d.]+) ms; threads of the title waited ([\d.]+) ms for room in the line, ([\d.]+) ms at presents"),
    "records": re.compile(r"per frame ([\d.]+) changed heap pages.*?([\d.]+) ms \(([\d.]+) ms waited for\)"),
    "frame": re.compile(r"victory_gpu::host: [\d.]+ fps, ([\d.]+) ms/frame \(([\d.]+) preparing"),
    "framelog": re.compile(r"render thread: ran ([\d.]+) ms \(user ([\d.]+), kernel ([\d.]+)\).*?submit thread: at work ([\d.]+) ms \(user ([\d.]+), kernel ([\d.]+)\); (\d+) submissions, preparing ([\d.]+), waiting for the card ([\d.]+).*?card: idle ([\d.]+) ms"),
}
NAMES = {
    "asking": ["kernel askings", "MB asked", "asking ms", "blocks made read-only", "blocks opened by ourselves", "waiting for the guards ms (askers)", "changing protections ms"],
    "copies": ["guard faults"],
    "recording": ["list calls", "list calls ms", "recording: guards ms"],
    "landing": ["landing: waiting for title bytes ms", "landing: opening or noting guards ms"],
    "line": ["submit thread worked ms", "title waited for room ms", "title waited at presents ms"],
    "records": ["heap pages", "heap rounds ms", "submit waited for rounds ms"],
    "frame": ["frame ms (frame lines)", "preparing ms"],
    "framelog": ["render thread ran ms", "render user ms", "render kernel ms", "submit at work ms", "submit user ms", "submit kernel ms", "submissions", "submit preparing ms", "submit waiting for card ms", "card idle ms"],
}
log, first, last = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
phases, lines = [], []
for raw in open(log, encoding="utf-8", errors="replace"):
    line = ANSI.sub("", raw.rstrip("\n"))
    m = TS.match(line)
    if not m:
        continue
    t = datetime.fromisoformat(m.group(1)[:26])
    if p := PHASE.search(line):
        phases.append((t, int(p.group(1)), int(p.group(2)), int(p.group(3)), float(p.group(5)), float(p.group(6)), int(p.group(7))))
        continue
    for kind, rx in RX.items():
        if c := rx.search(line):
            lines.append((t, kind, [float(x) for x in c.groups()]))
            break


def phase_of(t):
    for i in range(1, len(phases)):
        start, end = phases[i - 1][0], phases[i][0]
        if start < t <= end and (t - start).total_seconds() >= 5.0:
            return phases[i]
    return None


chosen = [p for p in phases if first <= p[1] <= last]
for b in (0, 1):
    s = [p for p in chosen if p[2] == b]
    if not s:
        continue
    meds = [p[4] for p in s]
    print(f"side {'ON ' if b else 'OFF'}: phases {[p[1] for p in s]}, medians {meds}, median of medians {statistics.median(meds):.2f}, "
          f"p90s median {statistics.median(p[5] for p in s):.1f}, draws {statistics.mean(p[6] for p in s):.0f}")
by_number = {p[1]: p for p in phases}
rel = []
for p in chosen:
    if p[2] == 1 and p[1] - 1 in by_number and p[1] + 1 in by_number and by_number[p[1] - 1][2] == 0 and by_number[p[1] + 1][2] == 0:
        ref = (by_number[p[1] - 1][4] + by_number[p[1] + 1][4]) / 2
        rel.append(100 * (p[4] - ref) / ref)
if rel:
    print(f"ON vs the mean of its OFF neighbours: {', '.join(f'{r:+.1f}%' for r in rel)}; median {statistics.median(rel):+.1f}%, mean {statistics.mean(rel):+.1f}%")
acc = {0: {}, 1: {}}
for t, kind, g in lines:
    p = phase_of(t)
    if p is None or not (first <= p[1] <= last):
        continue
    for name, v in zip(NAMES[kind], g):
        acc[p[2]].setdefault(name, []).append(v)
print(f"\n{'counter (mean of the lines in each side)':<42} {'OFF':>9} {'ON':>9} {'ON-OFF':>9}  n(off/on)")
for kind in NAMES:
    for n in NAMES[kind]:
        a, b = acc[0].get(n, []), acc[1].get(n, [])
        ma = statistics.mean(a) if a else float("nan")
        mb = statistics.mean(b) if b else float("nan")
        print(f"{n:<42} {ma:9.2f} {mb:9.2f} {mb - ma:+9.2f}  {len(a)}/{len(b)}")


"""VICTORY_AB run with two switches (bit 0 and bit 1): phase medians per combination, each phase with a
switch on against the mean of its nearest neighbours with that switch off and the other one the same,
and the counters of the lines logged inside each phase (5 s or more after its start), per combination.
Read only. Usage: python -I ab2.py <log> <first phase> <last phase> [name of bit 0] [name of bit 1]"""
import re
import statistics
import sys
from datetime import datetime

sys.stdout.reconfigure(encoding="utf-8")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
TS = re.compile(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+)Z")
PHASE = re.compile(r"A/B phase (\d+) bits (\d+) on \[[^\]]*\]: (\d+) frames, mean ([\d.]+) ms, median ([\d.]+), p90 ([\d.]+), (\d+) draws")
RX = {
    "line": re.compile(r"that thread worked ([\d.]+) ms; threads of the title waited ([\d.]+) ms for room in the line, ([\d.]+) ms at presents"),
    "records": re.compile(r"per frame ([\d.]+) changed heap pages.*?([\d.]+) ms \(([\d.]+) ms waited for\)"),
    "frame": re.compile(r"victory_gpu::host: [\d.]+ fps, ([\d.]+) ms/frame \(([\d.]+) preparing"),
    "asking": re.compile(r"asking about title memory per frame, all threads: (\d+) kernel askings over (\d+) MB \(([\d.]+) ms.*?(\d+) blocks made read-only, (\d+) opened by ourselves; ([\d.]+) ms waiting for the guards, ([\d.]+) ms changing protections"),
    "copies": re.compile(r"copystat: copies per frame: .*?(\d+) guard faults"),
    "p5": re.compile(r"renderer port 5 \(VICTORY_PORT_CBV=1\) per frame: (\d+) tables' constant buffers read by the card in place \(([\d.]+) KB not copied.*?copied as before: (\d+) not in place, (\d+) not on 16 bytes"),
    "pass": re.compile(r"the pass over the heap's views per frame: ([\d.]+) ms looking at slots, ([\d.]+) their copies' spans and refreshes, ([\d.]+) textures;.*?([\d.]+) ms waited for at the lists' close"),
    "ask": re.compile(r"preparing per frame, the submitting thread: ([\d.]+) ms asking the write watch \(([\d.]+) of them finding what to ask\)"),
    "dynamic": re.compile(r"dynamic slots: (\d+) tables asked, (\d+) refused, ([\d.]+) ms searching"),
}
NAMES = {
    "line": ["submit thread worked ms", "title waited for room ms", "title waited at presents ms"],
    "records": ["heap pages", "heap rounds ms", "submit waited for rounds ms"],
    "frame": ["frame ms (frame lines)", "preparing ms"],
    "asking": ["kernel askings", "MB asked", "asking ms", "blocks made read-only", "blocks opened by ourselves", "askers waiting for guards ms", "changing protections ms"],
    "copies": ["guard faults"],
    "p5": ["port 5: constant buffers in place", "port 5: KB not copied", "port 5: copied, not in place", "port 5: copied, not on 16 bytes"],
    "pass": ["heap pass: looking at slots ms", "heap pass: copies' spans+refreshes ms", "heap pass: textures ms", "heap pass: waited at lists' close ms"],
    "ask": ["submit: asking the write watch ms", "submit: finding what to ask ms"],
    "dynamic": ["dynamic tables asked", "dynamic tables refused", "dynamic slots searching ms"],
}
log, first, last = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
names = (sys.argv[4] if len(sys.argv) > 4 else "bit0", sys.argv[5] if len(sys.argv) > 5 else "bit1")
phases, lines = [], []
for raw in open(log, encoding="utf-8", errors="replace"):
    line = ANSI.sub("", raw.rstrip("\n"))
    m = TS.match(line)
    if not m:
        continue
    t = datetime.fromisoformat(m.group(1)[:26])
    if p := PHASE.search(line):
        phases.append((t, int(p.group(1)), int(p.group(2)) & 3, int(p.group(3)), float(p.group(5)), float(p.group(6)), int(p.group(7))))
        continue
    for kind, rx in RX.items():
        if c := rx.search(line):
            lines.append((t, kind, [float(x) for x in c.groups()]))
            break

chosen = [p for p in phases if first <= p[1] <= last]
label = lambda b: "+".join(n for i, n in enumerate(names) if b >> i & 1) or "none"
print(f"phases {first}-{last}: {len(chosen)}")
for b in range(4):
    s = [p for p in chosen if p[2] == b]
    if s:
        meds = [p[4] for p in s]
        print(f"  {label(b):<28} phases {[p[1] for p in s]}, medians {meds}, median {statistics.median(meds):.2f}, p90 median {statistics.median(p[5] for p in s):.1f}, draws {statistics.mean(p[6] for p in s):.0f}")
by_number = {p[1]: p for p in phases}
for k in (0, 1):
    rel = []
    for p in chosen:
        if not p[2] >> k & 1:
            continue
        want = p[2] & ~(1 << k)
        prev = next((by_number[n] for n in range(p[1] - 1, p[1] - 8, -1) if n in by_number and by_number[n][2] == want), None)
        nxt = next((by_number[n] for n in range(p[1] + 1, p[1] + 8) if n in by_number and by_number[n][2] == want), None)
        if prev and nxt:
            ref = (prev[4] + nxt[4]) / 2
            rel.append(100 * (p[4] - ref) / ref)
    if rel:
        print(f"{names[k]} on vs its neighbours with it off (other switch the same): {', '.join(f'{r:+.1f}%' for r in rel)}; median {statistics.median(rel):+.1f}%, mean {statistics.mean(rel):+.1f}%")


def phase_of(t):
    for i in range(1, len(phases)):
        start, end = phases[i - 1][0], phases[i][0]
        if start < t <= end and (t - start).total_seconds() >= 5.0:
            return phases[i]
    return None


acc = {b: {} for b in range(4)}
for t, kind, g in lines:
    p = phase_of(t)
    if p is None or not (first <= p[1] <= last):
        continue
    for name, v in zip(NAMES[kind], g):
        acc[p[2]].setdefault(name, []).append(v)
print(f"\n{'counter (mean of the lines in each combination)':<44}" + "".join(f"{label(b):>16}" for b in range(4)))
for kind in NAMES:
    for n in NAMES[kind]:
        vals = [statistics.mean(acc[b][n]) if acc[b].get(n) else float("nan") for b in range(4)]
        print(f"{n:<44}" + "".join(f"{v:16.2f}" for v in vals))

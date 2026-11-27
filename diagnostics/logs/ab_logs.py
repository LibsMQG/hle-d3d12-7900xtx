# Author: libs (hle-d3d12-amd)
"""Compare two VIctory run logs, same scene: python -I ab_logs.py <A.log> <B.log> [--last N] [--min-draws N]
Uses each log's last N steady blocks (default 14, about 70 s) with at least --min-draws draws a frame and under
400 ms/frame. Read only."""
import argparse, re, statistics as s
p = argparse.ArgumentParser()
p.add_argument('a'); p.add_argument('b')
p.add_argument('--last', type=int, default=14)
p.add_argument('--min-draws', type=int, default=4500)
o = p.parse_args()
PAT = re.compile(r"([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing: copies ([\d.]+) ms ([\d.]+) MB, textures ([\d.]+) ms ([\d.]+) MB; ([\d.]+) on the GPU; ([\d.]+) copying back ([\d.]+) MB\), (\d+) draws/frame, dropped (\d+) without")
K = ['fps', 'ms', 'prep', 'copies', 'copyMB', 'texms', 'texMB', 'gpu', 'back', 'backMB', 'draws', 'dropped']
def load(path):
    t = re.sub(r'\x1b\[[0-9;]*m', '', open(path, encoding='utf-8', errors='replace').read())
    rows, wait, idle, bad = [], None, None, 0
    for l in t.splitlines():
        if ' ERROR ' in l or 'panicked' in l or 'device removed' in l: bad += 1
        m = re.search(r"([\d.]+) ms of that while the submitting thread was working.*?stood waiting for the card ([\d.]+) ms", l)
        if m: idle, wait = float(m[1]), float(m[2])
        m = PAT.search(l)
        if m:
            r = dict(zip(K, map(float, m.groups())))
            r.update(t=l[11:19], cardwait=wait, gpuidle=idle)
            rows.append(r)
    return [r for r in rows if r['ms'] < 400 and r['draws'] >= o.min_draws][-o.last:], bad
(a, ea), (b, eb) = load(o.a), load(o.b)
print(f"A: {len(a)} blocks {a[0]['t'] if a else '-'}-{a[-1]['t'] if a else '-'}, errors/panics/device removed: {ea}")
print(f"B: {len(b)} blocks {b[0]['t'] if b else '-'}-{b[-1]['t'] if b else '-'}, errors/panics/device removed: {eb}")
print(f"{'median':9} {'A':>8} {'B':>8} {'B-A':>8}   A range        B range")
for k in K + ['cardwait', 'gpuidle']:
    va, vb = ([r[k] for r in x if r[k] is not None] for x in (a, b))
    if va and vb:
        ma, mb = s.median(va), s.median(vb)
        print(f"{k:9} {ma:8.1f} {mb:8.1f} {mb-ma:+8.1f}   {min(va):6.1f}-{max(va):<7.1f} {min(vb):6.1f}-{max(vb):<7.1f}")

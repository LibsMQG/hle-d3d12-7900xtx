"""Where the frame goes: medians of steady heavy-scene blocks. python -I model.py <log>... Read only."""
import re
import statistics as st
import sys

for L in sys.argv[1:]:
    rows = []
    cur = {}
    for l in open(L, encoding='utf-8', errors='replace'):
        def g(p):
            m = re.search(p, l)
            return float(m[1]) if m else None
        for k, p in (
            ('flights', r'submissions sent ahead per frame: ([\d.]+)'),
            ('settles', r'submissions sent ahead per frame: [\d.]+ in ([\d.]+) settles'),
            ('wait', r'([\d.]+) ms waiting for the card in them'),
            ('landing', r'was ([\d.]+) ms landing in all'),
            ('idle', r'nothing of ours queued ([\d.]+) ms'),
            ('busy%', r'the card about ([\d.]+)% busy'),
            ('idle_while_us', r'([\d.]+) ms of that while the submitting thread was working'),
            ('hashpass', r'per frame: ([\d.]+) hash passes'),
            ('cmppass', r'([\d.]+) compare passes'),
            ('pushMB', r'parts \(([\d.]+) MB\) pushed down'),
            ('upMB', r'per frame: ([\d.]+) MB sent up'),
            ('cmpPieces', r'ranges, ([\d.]+) pieces compared on the card'),
            ('snapMB', r'\(([\d.]+) MB snapshotted\), of them'),
        ):
            v = g(p)
            if v is not None:
                cur[k] = v
        m = re.search(r'([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing: copies ([\d.]+) ms ([\d.]+) MB, textures ([\d.]+) ms ([\d.]+) MB; ([\d.]+) on the GPU; ([\d.]+) copying back ([\d.]+) MB\), (\d+) draws/frame', l)
        if m:
            f = dict(zip(['fps', 'ms', 'prep', 'copies', 'copyMB', 'tex', 'texMB', 'gpuwait', 'back', 'backMB', 'draws'], map(float, m.groups())))
            h = re.search(r'heap buffers [\d.]+ passes ([\d.]+) ms', l)
            if h:
                f['heap'] = float(h[1])
            f.update(cur)
            if f['draws'] >= 4300 and f['ms'] < 400:
                rows.append(f)
            cur = {}
    rows = rows[-18:]
    print('==', L.split('/')[-1], len(rows), 'blocks')
    if rows:
        keys = sorted({k for r in rows for k in r})
        print('  ' + '  '.join(f"{k}={st.median([r[k] for r in rows if k in r]):.1f}" for k in keys))
        gb = [r['ms'] * r['busy%'] / 100 for r in rows if 'busy%' in r]
        if gb:
            print(f'  card busy = {st.median(gb):.0f} ms/frame')

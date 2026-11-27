# Author: libs (hle-d3d12-7900xtx)
"""Per-stats-block table of a VIctory run log: python frames.py <log> [last_n]"""
import re, sys, statistics as s
t = re.sub(r'\x1b\[[0-9;]*m', '', open(sys.argv[1], encoding="utf-8", errors="replace").read())
last = int(sys.argv[2]) if len(sys.argv) > 2 else 0
rows = []; remap = (0, 0); refresh = 0; idle = (0, 0); wait = 0; queue = (0, 0)
for l in t.splitlines():
    m = re.search(r"changes of title memory.s mapping per frame: ([\d.]+), [\d.]+ ms of them waiting for reads in place, ([\d.]+) ms under way", l)
    if m: remap = (float(m[1]), float(m[2]))
    m = re.search(r"texture refreshes per frame, all threads: ([\d.]+) ms", l)
    if m: refresh = float(m[1])
    m = re.search(r"the card.s clock per frame: nothing of ours queued ([\d.]+) ms .*?, ([\d.]+) ms of that while the submitting thread was working.*?stood waiting for the card ([\d.]+) ms", l)
    if m: idle = (float(m[1]), float(m[2])); wait = float(m[3])
    m = re.search(r"textures picked by index, the queue: (\d+) slots, (\d+) of them due", l)
    if m: queue = (int(m[1]), int(m[2]))
    m = re.search(r"([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing: copies ([\d.]+) ms ([\d.]+) MB, textures ([\d.]+) ms ([\d.]+) MB; ([\d.]+) on the GPU; ([\d.]+) copying back.*?(\d+) draws/frame, dropped (\d+) without.*?heap buffers [\d.]+ passes ([\d.]+) ms \(marked: \d+ slots looked at ([\d.]+) ms, \d+ copies refreshed ([\d.]+) ms, textures ([\d.]+) ms", l)
    if m:
        rows.append(dict(t=l[11:19], fps=float(m[1]), ms=float(m[2]), prep=float(m[3]), copies=float(m[4]), texmb=float(m[7]),
            gpu=float(m[8]), back=float(m[9]), draws=int(m[10]), dropped=int(m[11]), heaptex=float(m[15]), remaps=remap[0], remapms=remap[1],
            refresh=refresh, subwork=idle[1], notsub=idle[0] - idle[1], due=queue[1]))
sel = rows[-last:] if last else rows
print("%-8s %5s %6s %6s %5s %5s %5s %5s %5s %6s %6s %6s %6s %5s" % ("time", "fps", "ms", "heapTx", "prep", "texMB", "gpu", "back", "refr", "remaps", "remMs", "subWrk", "notSub", "draws"))
for r in sel:
    print("%-8s %5.1f %6.0f %6.0f %5.0f %5.0f %5.0f %5.0f %5.0f %6.0f %6.0f %6.0f %6.0f %5d" % (r['t'], r['fps'], r['ms'], r['heaptex'], r['prep'], r['texmb'], r['gpu'], r['back'], r['refresh'], r['remaps'], r['remapms'], r['subwork'], r['notsub'], r['draws']))
scene = [r for r in sel if r['ms'] > 40]
if scene:
    med = lambda k: s.median([r[k] for r in scene])
    print("\nmedian of %d in-scene blocks: %.0f ms/frame (%.1f fps); heapTx %.0f, prep %.0f, texMB %.0f, gpu %.0f, back %.0f, refresh %.0f, remaps %.0f (%.0f ms), notSub %.0f" % (
        len(scene), med('ms'), 1000 / med('ms'), med('heaptex'), med('prep'), med('texmb'), med('gpu'), med('back'), med('refresh'), med('remaps'), med('remapms'), med('notsub')))

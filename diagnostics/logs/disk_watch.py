# Author: libs (hle-d3d12-7900xtx)
"""Follows the newest run-*.log in a folder while the game runs and prints, once a minute, the DirectStorage
report (VICTORY_DISK_STATS=1): MB/s, requests, busy and reading workers, BCPack MB and workers, staging waits,
queue. Says when the world is reached (first frame line with >= 1000 draws) and warns when files are opened
but no disk line ever comes (counter not on). Reads files only. Stop with Ctrl+C.
Usage: python -I disk_watch.py <logs dir> [--every 60]"""
import argparse
import glob
import os
import re
import time

ap = argparse.ArgumentParser()
ap.add_argument("logs", help="folder holding the run-*.log files")
ap.add_argument("--every", type=float, default=60.0, help="seconds between summaries")
args = ap.parse_args()

RX = re.compile(r"requests=(\d+) mb_per_s=([\d.]+) busy_workers=([\d.]+) reading_workers=([\d.]+) staging_waits=(\d+) "
                r"bcpack_mb=(\d+) bcpack_workers=([\d.]+) waiting=(\d+)")
FPS = re.compile(r"([\d.]+) fps, ([\d.]+) ms/frame .*?\), (\d+) draws")


def say(s):
    print(s, flush=True)


cur, pos, acc, seen, world, warned, opened = None, 0, [], False, False, False, 0
t0 = last = time.time()
first = True
while True:
    files = sorted(glob.glob(os.path.join(args.logs, "run-*.log")), key=os.path.getmtime)
    if files and files[-1] != cur:
        cur, pos, acc, seen, world, warned, opened, t0 = files[-1], 0, [], False, False, False, 0, time.time()
        if first:  # a run already going: follow from its end
            pos, seen, world = os.path.getsize(cur), True, True
        say(("following from the current end: " if first else "new log: ") + os.path.basename(cur))
        first = False
    if cur:
        with open(cur, encoding="utf-8", errors="replace") as fh:
            fh.seek(pos)
            data = fh.read()
            pos = fh.tell()
        for line in data.splitlines():
            if "dstorage: file opened" in line:
                opened += 1
            m = RX.search(line)
            if m:
                seen = True
                acc.append(tuple(float(x) for x in m.groups()))
            f = FPS.search(line)
            if f and int(f.group(3)) >= 1000 and not world:
                world = True
                say(f"world reached ({f.group(2)} ms/frame)")
        if not seen and opened > 20 and not warned and time.time() - t0 > 90:
            warned = True
            say(f"WARNING: {opened} files opened but no disk counter line: VICTORY_DISK_STATS is not on in this run")
        if acc and time.time() - last >= args.every:
            n = len(acc)
            say("last %ds (%s): disk %.1f MB/s mean, max %.1f; requests %d; workers busy %.2f (reading %.2f); "
                "BCPack %d MB, workers %.2f; staging waits %d; queued at most %d" % (
                    n * 5, "world" if world else "loading", sum(a[1] for a in acc) / n, max(a[1] for a in acc),
                    int(sum(a[0] for a in acc)), sum(a[2] for a in acc) / n, sum(a[3] for a in acc) / n,
                    int(sum(a[5] for a in acc)), sum(a[6] for a in acc) / n, int(sum(a[4] for a in acc)),
                    int(max(a[7] for a in acc))))
            acc, last = [], time.time()
    time.sleep(2)

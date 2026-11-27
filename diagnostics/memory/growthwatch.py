# Author: libs (hle-d3d12-7900xtx)
"""growthwatch: follows a running game's log (and memread's .jsonl) and prints one line per event, for someone watching the run:
when moving starts, a status every few minutes, when the growth of the card's memory looks stopped, the re-check
45 s later, releases of texture copies, and the game closing. Reads files only; touches nothing.

usage: python -I growthwatch.py <game log> [--memread-dir DIR] [--status-every 180] [--flat-mb 60] [--recheck 45]
       python -I growthwatch.py <old game log> --replay      (runs the same logic over a finished log, no waiting)
"""
import argparse, glob, json, os, re, subprocess, sys, time

ap = argparse.ArgumentParser()
ap.add_argument('log')
ap.add_argument('--memread-dir', default=os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out'))
ap.add_argument('--status-every', type=int, default=180)
ap.add_argument('--flat-mb', type=int, default=60)      # growth over 60 s below this = flat
ap.add_argument('--recheck', type=int, default=45)      # seconds before re-checking a flat stretch
ap.add_argument('--min-move', type=int, default=150)   # seconds of moving before a flat stretch counts
ap.add_argument('--replay', action='store_true')
a = ap.parse_args()

T = re.compile(r'2026-\d\d-\d\dT(\d\d):(\d\d):(\d\d)')
VRAM = re.compile(r"video memory as the system counts it.*?card's own: budget \d+ MB, in use (\d+) MB")
HOUSE = re.compile(r'memory housekeeping: card (\d+) MB in use.*?texture copies alive (\d+) MB, let go for room (\d+) MB')
UPL = re.compile(r'uploads by cause, last 5 s: (\d+) uploads (\d+) MB;.*?first \d+ \((\d+) MB\)')
ANSI = re.compile(r'\x1b\[[0-9;]*m')


def hms(s):
    return '%02d:%02d:%02d' % (s // 3600, s % 3600 // 60, s % 60)


class Watch:
    def __init__(self):
        self.vram, self.copies, self.upl, self.private = [], [], [], []
        self.letgo = None; self.drive_t = None; self.flat_t = None; self.state = 'loading'; self.last_status = 0
        self.flat_v = self.flat_c = 0; self.clock = None

    def say(self, msg):
        stamp = hms(self.clock) + ' UTC' if self.clock is not None else time.strftime('%H:%M:%S')
        print(stamp + ' ' + msg, flush=True)

    def line(self, line):
        line = ANSI.sub('', line)
        m = T.search(line)
        if not m:
            return None
        s = int(m.group(1)) * 3600 + int(m.group(2)) * 60 + int(m.group(3))
        if (v := VRAM.search(line)):
            self.vram.append((s, int(v.group(1))))
        elif (h := HOUSE.search(line)):
            self.copies.append((s, int(h.group(2))))
            lg = int(h.group(3))
            if self.letgo is not None and lg - self.letgo >= 50:
                self.clock = s
                self.say('RELEASE: texture copies let go for room %d MB in all (+%d), copies alive %d MB, card %d MB'
                         % (lg, lg - self.letgo, int(h.group(2)), int(h.group(1))))
            if self.letgo is None or lg - self.letgo >= 50:
                self.letgo = lg
        elif (u := UPL.search(line)):
            self.upl.append((s, (int(u.group(2)), int(u.group(3)))))
        return s

    @staticmethod
    def at(series, t):
        v = None
        for s, x in series:
            if s <= t:
                v = x
            else:
                break
        return v

    def evaluate(self):
        if not (self.vram and self.copies and self.upl):
            return
        now = self.vram[-1][0]; self.clock = now
        v_now, c_now = self.vram[-1][1], self.copies[-1][1]
        v_60, c_60 = self.at(self.vram, now - 60), self.at(self.copies, now - 60)
        up_mb, first_mb = self.upl[-1][1]
        p = self.private[-1][1] if self.private else None
        mem = ', game private %.1f GB, commit left %.1f GB' % p if p else ''
        if self.state == 'loading':  # the load ends when the burst of first-time uploads is over (3 samples under 30 MB/5 s)
            last_first = [x[1][1] for x in self.upl[-3:]]
            if v_now > 5000 and len(last_first) == 3 and max(last_first) < 30:
                self.state = 'standing'; self.loaded_t = now; self.told = False
                self.say('LOADED, standing: VRAM %d MB, copies alive %d MB%s' % (v_now, c_now, mem))
            return
        if self.state == 'standing' and not self.told and now - self.loaded_t >= 120:
            self.told = True
            self.say('STOOD 2 MIN after the load: you can start moving. VRAM %d MB, copies alive %d MB%s'
                     % (v_now, c_now, mem))
        if self.state == 'standing':  # compared only with what the card held after the load
            recent_first = [x[1][1] for x in self.upl[-3:]]
            v_ref = self.at(self.vram, max(now - 60, self.loaded_t))
            grown = v_now - v_ref if v_ref is not None else 0
            if (len(recent_first) == 3 and min(recent_first) >= 15 and grown > 150) or grown > 300:
                self.drive_t = now; self.state = 'growing'; self.last_status = now
                self.say('MOVING STARTED: VRAM %d MB, copies alive %d MB%s' % (v_now, c_now, mem))
            return
        if v_60 is None or c_60 is None:
            return
        dv, dc = v_now - v_60, c_now - c_60
        first_60 = [x[1][1] for x in self.upl if x[0] > now - 60]
        new_rate = sum(first_60) / max(1, len(first_60))
        why = ('new textures still arrive (%.0f MB/5 s first-time): the card holds still anyway' % new_rate if new_rate >= 10
               else 'few new textures arrive (%.0f MB/5 s first-time): known area or stopped?' % new_rate)
        line = ('VRAM %d MB (%+d in 60 s), copies alive %d MB (%+d)%s, uploads %d MB/5 s (first %d)'
                % (v_now, dv, c_now, dc, mem, up_mb, first_mb))
        mins = (now - self.drive_t) // 60
        if self.state == 'growing' and now - self.drive_t >= a.min_move and dv < a.flat_mb and dc < a.flat_mb:
            self.state = 'candidate'; self.flat_t = now; self.flat_v, self.flat_c = v_now, c_now
            self.say('PLATEAU? flat for 60 s after %d min of moving; %s: %s' % (mins, why, line))
        elif self.state == 'candidate' and now - self.flat_t >= a.recheck:
            if v_now - self.flat_v < a.flat_mb and c_now - self.flat_c < a.flat_mb:
                self.state = 'confirmed'
                self.say('PLATEAU CONFIRMED %d s later; %s: %s' % (now - self.flat_t, why, line))
            else:
                self.state = 'growing'
                self.say('NOT A PLATEAU, grew again (+%d MB VRAM since): %s' % (v_now - self.flat_v, line))
        elif self.state == 'confirmed' and v_now - self.flat_v > 200:
            self.state = 'growing'
            self.say('GROWING AGAIN (+%d MB VRAM since the plateau): %s' % (v_now - self.flat_v, line))
        if now - self.last_status >= a.status_every:
            self.last_status = now
            self.say('status, %d min of moving: %s' % (mins, line))


def game_running():
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq victory.exe', '/NH'], capture_output=True, text=True).stdout
    return 'victory.exe' in out.lower()


w = Watch()
if a.replay:
    last = None
    for l in open(a.log, encoding='utf-8', errors='replace'):
        s = w.line(l)
        if s is not None and (last is None or s - last >= 5):
            last = s
            w.evaluate()
    print('replay done: VRAM %s MB, copies %s MB' % (w.vram[-1][1] if w.vram else '?', w.copies[-1][1] if w.copies else '?'))
    sys.exit(0)

print(time.strftime('%H:%M:%S') + ' watching ' + os.path.basename(a.log), flush=True)
pos, jpos, jfile, quiet = 0, 0, None, 0
while True:
    with open(a.log, encoding='utf-8', errors='replace') as f:
        f.seek(pos)
        chunk = f.read()
        pos = f.tell()
    for l in chunk.splitlines():
        w.line(l)
    files = glob.glob(os.path.join(a.memread_dir, 'memread-*-ram-mov.jsonl'))
    jf = max(files, key=os.path.getmtime) if files else None
    if jf != jfile:
        jfile, jpos = jf, 0
    if jfile:
        with open(jfile, encoding='utf-8') as f:
            f.seek(jpos)
            while True:
                l = f.readline()
                if not l.endswith('\n'):
                    break
                jpos = f.tell()
                r = json.loads(l)
                if r.get('kind') == 'fast':
                    h, mi, se = r['t'].split(':')
                    w.private.append((int(h) * 3600 + int(mi) * 60 + int(se),
                                      (r['private_gb'], r['commit_limit_gb'] - r['commit_total_gb'])))
    w.evaluate()
    quiet = 0 if chunk else quiet + 5
    if quiet >= 30 and not game_running():
        w.say('GAME CLOSED. Last: VRAM %s MB, copies alive %s MB, let go for room %s MB'
              % (w.vram[-1][1] if w.vram else '?', w.copies[-1][1] if w.copies else '?', w.letgo))
        sys.exit(0)
    time.sleep(5)

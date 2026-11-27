# Author: libs (hle-d3d12-7900xtx)
"""memread: reads victory.exe's memory from outside, while it runs. No rebuild, no
change to the game: only VirtualQueryEx / QueryWorkingSetEx / GetMappedFileName /
GetPerformanceInfo on the process, at below-normal priority.

Question it answers: of the commit the game holds, how much is
  - the title's own reservations (address space only, free),
  - memory the title asked to be committed (its own VirtualAlloc with commit),
  - the title's physical pages (sections, committed on first use by us; seen
    once through our lasting whole-section views, "ours_mapped"),
  - our own memory (write-combined = upload heaps / driver, plain private),
and how much of each is really in RAM (resident, sampled every 64 KiB).
Plus the system commit limit/total/peak over time (does the pagefile grow?)
and the commit held outside every process.

Usage:
  python -I memread.py                        attach to the next victory.exe started
  python -I memread.py --attach-now           attach to a running victory.exe
  python -I memread.py --only-tag mytest --log-dir <logs dir>
                                              attach only to the run whose launcher
                                              log is named run-*-mytest*.log in <logs dir>
                                              (any other victory.exe is left alone)
Follows the run until it exits, then exits. Output: --out (default: out/ next to this script).
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import datetime as dt
import glob
import json
import os
import subprocess
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)

PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
PROCESS_VM_READ = 0x0010
SYNCHRONIZE = 0x00100000
MEM_COMMIT, MEM_RESERVE, MEM_FREE = 0x1000, 0x2000, 0x10000
MEM_PRIVATE, MEM_MAPPED, MEM_IMAGE = 0x20000, 0x40000, 0x1000000
PAGE_WRITECOMBINE = 0x400
TITLE_TOP = 1 << 40  # the title's memory is placed below 1 TiB (xmem.rs address_range)
TITLE_LOW = 0x10000
STEP = 64 << 10
CHUNK = 65536  # residency samples per QueryWorkingSetEx call
GB = 1 << 30
MB = 1 << 20
NAME = "victory.exe"


class MBI(ctypes.Structure):
    _fields_ = [
        ("BaseAddress", ctypes.c_ulonglong),
        ("AllocationBase", ctypes.c_ulonglong),
        ("AllocationProtect", wt.DWORD),
        ("PartitionId", wt.WORD),
        ("RegionSize", ctypes.c_size_t),
        ("State", wt.DWORD),
        ("Protect", wt.DWORD),
        ("Type", wt.DWORD),
    ]


class WSEX(ctypes.Structure):
    _fields_ = [("VirtualAddress", ctypes.c_ulonglong), ("Attributes", ctypes.c_ulonglong)]


class PERF(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD),
        ("CommitTotal", ctypes.c_size_t),
        ("CommitLimit", ctypes.c_size_t),
        ("CommitPeak", ctypes.c_size_t),
        ("PhysicalTotal", ctypes.c_size_t),
        ("PhysicalAvailable", ctypes.c_size_t),
        ("SystemCache", ctypes.c_size_t),
        ("KernelTotal", ctypes.c_size_t),
        ("KernelPaged", ctypes.c_size_t),
        ("KernelNonpaged", ctypes.c_size_t),
        ("PageSize", ctypes.c_size_t),
        ("HandleCount", wt.DWORD),
        ("ProcessCount", wt.DWORD),
        ("ThreadCount", wt.DWORD),
    ]


class PMC(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD),
        ("PageFaultCount", wt.DWORD),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
        ("PrivateUsage", ctypes.c_size_t),
    ]


k32.OpenProcess.restype = wt.HANDLE
k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k32.CloseHandle.argtypes = [wt.HANDLE]
k32.VirtualQueryEx.restype = ctypes.c_size_t
k32.VirtualQueryEx.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.POINTER(MBI), ctypes.c_size_t]
k32.K32QueryWorkingSetEx.restype = wt.BOOL
k32.K32QueryWorkingSetEx.argtypes = [wt.HANDLE, ctypes.c_void_p, wt.DWORD]
k32.K32GetPerformanceInfo.restype = wt.BOOL
k32.K32GetPerformanceInfo.argtypes = [ctypes.POINTER(PERF), wt.DWORD]
k32.K32GetProcessMemoryInfo.restype = wt.BOOL
k32.K32GetProcessMemoryInfo.argtypes = [wt.HANDLE, ctypes.POINTER(PMC), wt.DWORD]
k32.K32GetMappedFileNameW.restype = wt.DWORD
k32.K32GetMappedFileNameW.argtypes = [wt.HANDLE, ctypes.c_void_p, wt.LPWSTR, wt.DWORD]
k32.K32EnumProcesses.restype = wt.BOOL
k32.K32EnumProcesses.argtypes = [ctypes.POINTER(wt.DWORD), wt.DWORD, ctypes.POINTER(wt.DWORD)]
k32.WaitForSingleObject.restype = wt.DWORD
k32.WaitForSingleObject.argtypes = [wt.HANDLE, wt.DWORD]
k32.GetCurrentProcess.restype = wt.HANDLE
k32.SetPriorityClass.argtypes = [wt.HANDLE, wt.DWORD]


def now():
    return dt.datetime.now(dt.timezone.utc).strftime("%H:%M:%S")


def find_pids():
    out = subprocess.run(
        ["tasklist", "/FO", "CSV", "/NH", "/FI", f"IMAGENAME eq {NAME}"],
        capture_output=True, text=True, errors="replace").stdout
    pids = []
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) > 1 and parts[0].lower() == NAME.lower():
            try:
                pids.append(int(parts[1]))
            except ValueError:
                pass
    return pids


def tagged_log(tag, since, dirs):
    """The newest launcher log run-*-<tag>*.log in the --log-dir folders born at or after `since`."""
    best = None
    for d in dirs:
        for f in glob.glob(os.path.join(d, f"run-*-{tag}*.log")):
            st = os.stat(f)
            born = getattr(st, "st_birthtime", st.st_ctime)
            if born >= since and (best is None or born > best[0]):
                best = (born, f)
    return best[1] if best else None


def perf():
    p = PERF()
    p.cb = ctypes.sizeof(p)
    k32.K32GetPerformanceInfo(ctypes.byref(p), p.cb)
    ps = p.PageSize
    return {
        "commit_total_gb": p.CommitTotal * ps / GB,
        "commit_limit_gb": p.CommitLimit * ps / GB,
        "commit_peak_gb": p.CommitPeak * ps / GB,
        "phys_avail_gb": p.PhysicalAvailable * ps / GB,
        "phys_total_gb": p.PhysicalTotal * ps / GB,
    }


def pmc(h):
    m = PMC()
    m.cb = ctypes.sizeof(m)
    if not k32.K32GetProcessMemoryInfo(h, ctypes.byref(m), m.cb):
        return None
    return {
        "private_gb": m.PrivateUsage / GB,
        "ws_gb": m.WorkingSetSize / GB,
        "peak_ws_gb": m.PeakWorkingSetSize / GB,
        "peak_pagefile_gb": m.PeakPagefileUsage / GB,
    }


def all_private():
    """Sum of every readable process's private commit (GB), processes, unreadable ones."""
    arr = (wt.DWORD * 8192)()
    got = wt.DWORD()
    if not k32.K32EnumProcesses(arr, ctypes.sizeof(arr), ctypes.byref(got)):
        return None, 0, 0
    total, failed, count = 0.0, 0, got.value // 4
    for i in range(count):
        p = arr[i]
        if p == 0:
            continue
        hh = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, False, p) or \
            k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, p)
        if not hh:
            failed += 1
            continue
        q = pmc(hh)
        k32.CloseHandle(hh)
        if q:
            total += q["private_gb"]
        else:
            failed += 1
    return total, count, failed


def mapped_name(h, addr):
    buf = ctypes.create_unicode_buffer(520)
    n = k32.K32GetMappedFileNameW(h, ctypes.c_void_p(addr), buf, 520)
    return buf.value if n else None


def classify(m):
    """Bucket of a committed or reserved region."""
    a = m.BaseAddress
    wc = (m.Protect | m.AllocationProtect) & PAGE_WRITECOMBINE
    if TITLE_LOW <= a < TITLE_TOP:
        if m.Type == MEM_PRIVATE:
            return "title_private"      # title's own VirtualAlloc (and placeholders when reserved)
        if m.Type == MEM_MAPPED:
            return "title_views"        # views of physical pages (alias the sections)
        return "title_image"
    if m.Type == MEM_IMAGE:
        return "image"
    if m.Type == MEM_MAPPED:
        return "ours_mapped"            # our lasting whole-section views, and mapped files
    return "ours_wc" if wc else "ours_private"


# Per-allocation detail only where it is few and telling (title views are
# ~100,000 one-page views: aggregate only).
DETAIL = ("title_private", "ours_mapped")


def walk(h):
    m = MBI()
    a = 0
    buckets = {}
    allocs = {}   # (bucket, AllocationBase) -> [size, committed, resident]
    regions = 0
    n = 0
    arr = (WSEX * CHUNK)()
    meta = []     # (bucket, alloc or None, weight) per entry of arr

    def flush():
        nonlocal n
        k = len(meta)
        if not k:
            return
        if k32.K32QueryWorkingSetEx(h, arr, ctypes.sizeof(WSEX) * k):
            for j in range(k):
                if arr[j].Attributes & 1:
                    b, al, w = meta[j]
                    buckets[b]["resident"] += w
                    if al is not None:
                        allocs[(b, al)][2] += w
        n += k
        meta.clear()

    top = 0x7FFF_FFFF_0000
    while a < top:
        if k32.VirtualQueryEx(h, ctypes.c_void_p(a), ctypes.byref(m), ctypes.sizeof(m)) == 0:
            break
        size = m.RegionSize
        if size == 0:
            break
        regions += 1
        if m.State != MEM_FREE:
            b = classify(m)
            e = buckets.setdefault(b, {"committed": 0, "reserved": 0, "resident": 0})
            al = None
            if b in DETAIL:
                al = m.AllocationBase
                rec = allocs.setdefault((b, al), [0, 0, 0])
                rec[0] += size
            if m.State == MEM_COMMIT:
                e["committed"] += size
                if al is not None:
                    allocs[(b, al)][1] += size
                base = m.BaseAddress
                off = 0
                while off < size:
                    j = len(meta)
                    arr[j].VirtualAddress = base + off
                    arr[j].Attributes = 0
                    meta.append((b, al, min(STEP, size - off)))
                    if len(meta) == CHUNK:
                        flush()
                    off += STEP
            else:
                e["reserved"] += size
        a = m.BaseAddress + size
    flush()
    return buckets, allocs, regions, n


def fmt_buckets(buckets):
    order = ["title_private", "title_views", "ours_mapped", "ours_wc", "ours_private", "image", "title_image"]
    rows = []
    for b in order + [k for k in buckets if k not in order]:
        if b in buckets:
            e = buckets[b]
            rows.append(f"  {b:14s} committed {e['committed']/GB:7.2f} GB  resident {e['resident']/GB:6.2f} GB  reserved-only {e['reserved']/GB:7.2f} GB")
    return "\n".join(rows)


def top_allocs(allocs, bucket, k):
    return sorted(((v, a) for (b, a), v in allocs.items() if b == bucket and v[1]), key=lambda x: -x[0][1])[:k]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "out"))
    ap.add_argument("--every", type=float, default=60.0, help="seconds between full walks")
    ap.add_argument("--fast", type=float, default=5.0, help="seconds between cheap commit samples")
    ap.add_argument("--first", type=float, default=20.0, help="seconds after attach of the first walk")
    ap.add_argument("--only-tag", default="", help="attach only to the run whose launcher log is run-*-TAG*.log")
    ap.add_argument("--log-dir", action="append", default=[], help="folder of launcher logs searched by --only-tag (repeatable)")
    ap.add_argument("--attach-now", action="store_true", help="attach to a running victory.exe instead of the next one")
    ap.add_argument("--name", default="victory.exe", help="process image name (testing)")
    ap.add_argument("--once", action="store_true", help="one walk, then exit (testing)")
    args = ap.parse_args()
    if args.only_tag and not args.log_dir:
        ap.error("--only-tag needs --log-dir")
    global NAME
    NAME = args.name
    k32.SetPriorityClass(k32.GetCurrentProcess(), 0x4000)  # BELOW_NORMAL
    os.makedirs(args.out, exist_ok=True)

    ignored = set() if args.attach_now else set(find_pids())
    what = f"tagged {args.only_tag}" if args.only_tag else "any"
    print(f"{now()} UTC waiting for a new {NAME} ({what}; ignoring {sorted(ignored)})", flush=True)
    pid, logname = None, None
    while pid is None:
        for p in find_pids():
            if p in ignored:
                continue
            if not args.only_tag:
                pid = p
                break
            seen = time.time()
            found = None
            while found is None and time.time() - seen < 60 and p in find_pids():
                found = tagged_log(args.only_tag, seen - 300, args.log_dir)
                if found is None:
                    time.sleep(3)
            if found:
                pid, logname = p, found
                break
            ignored.add(p)
            print(f"{now()} left alone: pid {p} (no new log tagged {args.only_tag}: another run)", flush=True)
        if pid is None:
            time.sleep(2)
    h = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | SYNCHRONIZE, False, pid)
    if not h:
        print(f"OpenProcess failed: {ctypes.get_last_error()}", flush=True)
        return 1
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    tag = f"-{args.only_tag}" if args.only_tag else ""
    txt = open(os.path.join(args.out, f"memread-{stamp}{tag}.txt"), "w", encoding="utf-8")
    jl = open(os.path.join(args.out, f"memread-{stamp}{tag}.jsonl"), "w", encoding="utf-8")

    def say(s):
        txt.write(s + "\n")
        txt.flush()
        print(s, flush=True)

    say(f"{now()} UTC attached pid {pid}; game log {logname}; walk every {args.every:.0f} s, commit sample every {args.fast:.0f} s; times are UTC like the game log")
    next_walk = time.monotonic() + args.first
    peak = {"commit_total_gb": 0.0, "private_gb": 0.0, "min_left_gb": 1e9}
    while k32.WaitForSingleObject(h, 0) != 0:  # WAIT_OBJECT_0 = exited
        p = perf()
        q = pmc(h)
        if q:
            left = p["commit_limit_gb"] - p["commit_total_gb"]
            peak["commit_total_gb"] = max(peak["commit_total_gb"], p["commit_total_gb"])
            peak["private_gb"] = max(peak["private_gb"], q["private_gb"])
            if left < peak["min_left_gb"]:
                peak["min_left_gb"] = left
                peak["min_left_at"] = now()
                peak["limit_at_min"] = p["commit_limit_gb"]
            jl.write(json.dumps({"t": now(), "kind": "fast", **p, **q}) + "\n")
            jl.flush()
        if time.monotonic() >= next_walk:
            w0 = time.monotonic()
            buckets, allocs, regions, n = walk(h)
            took = time.monotonic() - w0
            if k32.WaitForSingleObject(h, 0) == 0:
                break
            others, procs, unread = all_private()
            maps = []
            for v, a in top_allocs(allocs, "ours_mapped", 400):
                nm = mapped_name(h, a)
                maps.append((a, v, nm))
            sec = [x for x in maps if not x[2]]
            fil = [x for x in maps if x[2]]
            say(f"\n{now()} walk: {regions} regions, {n} residency samples, {took:.1f} s")
            say(f"  system: commit {p['commit_total_gb']:.2f} / limit {p['commit_limit_gb']:.2f} GB (peak {p['commit_peak_gb']:.2f}), RAM free {p['phys_avail_gb']:.2f} of {p['phys_total_gb']:.1f} GB")
            if others is not None:
                say(f"  all processes: private {others:.2f} GB in {procs} processes ({unread} not readable); commit outside any process {p['commit_total_gb'] - others:.2f} GB (the game's sections are in there)")
            if q:
                say(f"  process: private {q['private_gb']:.2f} GB, working set {q['ws_gb']:.2f} GB (peak {q['peak_ws_gb']:.2f})")
            say(fmt_buckets(buckets))
            say(f"  ours_mapped split: pagefile sections {sum(x[1][1] for x in sec)/GB:.2f} GB committed ({sum(x[1][2] for x in sec)/GB:.2f} resident) in {len(sec)}; "
                f"mapped files {sum(x[1][1] for x in fil)/GB:.2f} GB ({sum(x[1][2] for x in fil)/GB:.2f} resident) in {len(fil)}")
            say("  title_private, biggest allocations:")
            for v, a in top_allocs(allocs, "title_private", 10):
                say(f"    {a:#014x} size {v[0]/MB:9.0f} MB committed {v[1]/MB:8.0f} MB resident {v[2]/MB:8.0f} MB")
            say("  ours_mapped, biggest (section = pagefile-backed; whole-section views have the section's size):")
            for a, v, nm in maps[:10]:
                say(f"    {a:#014x} size {v[0]/MB:9.0f} MB committed {v[1]/MB:8.0f} MB resident {v[2]/MB:8.0f} MB  {os.path.basename(nm) if nm else 'section'}")
            jl.write(json.dumps({"t": now(), "kind": "walk", "took_s": took, "regions": regions, "buckets": buckets,
                                 "system": p, "process": q, "others_private_gb": others, "procs": procs, "unread": unread,
                                 "title_allocs": [[hex(a), *v] for v, a in top_allocs(allocs, "title_private", 200)],
                                 "ours_mapped_allocs": [[hex(a), *v, nm] for a, v, nm in maps]}) + "\n")
            jl.flush()
            next_walk = time.monotonic() + args.every
            if args.once:
                break
        time.sleep(args.fast)
    exited = k32.WaitForSingleObject(h, 0) == 0
    say(f"\n{now()} {'victory.exe exited' if exited else 'stopped'}. Peaks seen: system commit {peak['commit_total_gb']:.2f} GB, process private {peak['private_gb']:.2f} GB, "
        f"least commit left {peak['min_left_gb']:.2f} GB at {peak.get('min_left_at')} (limit then {peak.get('limit_at_min', 0):.2f} GB)")
    txt.close()
    jl.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())

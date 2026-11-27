"""Micro-benchmark, own process only: cost of GetWriteWatch (succeeding on a write-watched reservation, failing on
plain memory) and of VirtualProtect per 64 KiB block vs one call for a run of blocks. Allocates ~350 MB, frees it.
Python call overhead (~0.3-1 us) is included in every number."""
import ctypes
import time
from ctypes import wintypes as w

k = ctypes.WinDLL("kernel32", use_last_error=True)
k.VirtualAlloc.restype = ctypes.c_void_p
k.VirtualAlloc.argtypes = [ctypes.c_void_p, ctypes.c_size_t, w.DWORD, w.DWORD]
k.VirtualFree.argtypes = [ctypes.c_void_p, ctypes.c_size_t, w.DWORD]
k.VirtualProtect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, w.DWORD, ctypes.POINTER(w.DWORD)]
k.GetWriteWatch.argtypes = [w.DWORD, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p),
                            ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(w.DWORD)]
MEM_COMMIT, MEM_RESERVE, MEM_WRITE_WATCH, MEM_RELEASE = 0x1000, 0x2000, 0x200000, 0x8000
RW, RO = 0x04, 0x02
SIZE = 174 << 20
BLOCK = 64 << 10

watched = k.VirtualAlloc(None, SIZE, MEM_COMMIT | MEM_RESERVE | MEM_WRITE_WATCH, RW)
plain = k.VirtualAlloc(None, SIZE, MEM_COMMIT | MEM_RESERVE, RW)
assert watched and plain
ctypes.memset(watched, 1, SIZE)
ctypes.memset(plain, 1, SIZE)
pages = SIZE // 4096
buf = (ctypes.c_void_p * pages)()
gran = w.DWORD()


def gww(base, size, n):
    count = ctypes.c_size_t(pages)
    t = time.perf_counter()
    fails = 0
    for _ in range(n):
        count.value = pages
        fails += k.GetWriteWatch(1, base, size, buf, ctypes.byref(count), ctypes.byref(gran)) != 0
    return (time.perf_counter() - t) / n * 1e6, fails


def noop(n):
    t = time.perf_counter()
    for _ in range(n):
        k.GetCurrentProcessId()
    return (time.perf_counter() - t) / n * 1e6


print(f"python call overhead: {noop(20000):.2f} us")
gww(watched, SIZE, 1)  # reset after the memset
for size, label in ((BLOCK, "64 KiB"), (4 << 20, "4 MiB"), (SIZE, "174 MiB")):
    us, f = gww(watched, size, 2000)
    print(f"GetWriteWatch reset, watched, clean, {label}: {us:.2f} us/call (fails {f})")
    us, f = gww(plain, size, 2000)
    print(f"GetWriteWatch reset, plain memory (fails), {label}: {us:.2f} us/call (fails {f})")
# Dirty pages then ask: cost grows with pages reported.
ctypes.memset(watched, 2, 32 << 20)
us, f = gww(watched, SIZE, 1)
print(f"GetWriteWatch reset, 174 MiB with 32 MiB written: {us:.0f} us (one call)")

old = w.DWORD()
blocks = 512
t = time.perf_counter()
for rep in range(5):
    for b in range(blocks):
        k.VirtualProtect(plain + b * BLOCK, BLOCK, RO, ctypes.byref(old))
    for b in range(blocks):
        k.VirtualProtect(plain + b * BLOCK, BLOCK, RW, ctypes.byref(old))
per_block = (time.perf_counter() - t) / (5 * 2 * blocks) * 1e6
t = time.perf_counter()
for rep in range(200):
    k.VirtualProtect(plain, blocks * BLOCK, RO, ctypes.byref(old))
    k.VirtualProtect(plain, blocks * BLOCK, RW, ctypes.byref(old))
per_run = (time.perf_counter() - t) / 400 * 1e6
print(f"VirtualProtect one block (64 KiB) per call: {per_block:.2f} us/call")
print(f"VirtualProtect one call for {blocks} blocks (32 MiB): {per_run:.2f} us/call = {per_run / blocks:.3f} us/block")
k.VirtualFree(watched, 0, MEM_RELEASE)
k.VirtualFree(plain, 0, MEM_RELEASE)

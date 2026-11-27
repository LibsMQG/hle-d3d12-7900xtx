# Author: libs (hle-d3d12-amd)
"""Run a program of ours until it crashes and print where: python -I dbg.py <runs> <exe> [args...]
Only starts and reads the process it started (debug API of Windows)."""
import ctypes as C
import ctypes.wintypes as W
import os
import struct
import sys

k = C.WinDLL('kernel32', use_last_error=True)
d = C.WinDLL('dbghelp', use_last_error=True)
DWORD64 = C.c_uint64


class STARTUPINFO(C.Structure):
    _fields_ = [('cb', W.DWORD), ('a', W.LPWSTR), ('b', W.LPWSTR), ('c', W.LPWSTR), ('x', W.DWORD * 8), ('w1', W.WORD), ('w2', W.WORD),
                ('r', C.c_void_p), ('i', W.HANDLE), ('o', W.HANDLE), ('e', W.HANDLE)]


class PI(C.Structure):
    _fields_ = [('hp', W.HANDLE), ('ht', W.HANDLE), ('pid', W.DWORD), ('tid', W.DWORD)]


class EXREC(C.Structure):
    _fields_ = [('code', W.DWORD), ('flags', W.DWORD), ('rec', C.c_void_p), ('addr', C.c_void_p), ('n', W.DWORD), ('pad', W.DWORD),
                ('info', DWORD64 * 15)]


class EXDBG(C.Structure):
    _fields_ = [('rec', EXREC), ('first', W.DWORD)]


class U(C.Union):
    _fields_ = [('ex', EXDBG), ('raw', C.c_byte * 160)]


class EV(C.Structure):
    _fields_ = [('code', W.DWORD), ('pid', W.DWORD), ('tid', W.DWORD), ('u', U)]


class SYM(C.Structure):
    _fields_ = [('size', W.ULONG), ('type', W.ULONG), ('res', DWORD64 * 2), ('idx', W.ULONG), ('sz', W.ULONG), ('mod', DWORD64),
                ('flags', W.ULONG), ('val', DWORD64), ('addr', DWORD64), ('reg', W.ULONG), ('scope', W.ULONG), ('tag', W.ULONG),
                ('nl', W.ULONG), ('mx', W.ULONG), ('name', C.c_char * 1024)]


class LINE(C.Structure):
    _fields_ = [('size', W.DWORD), ('key', C.c_void_p), ('line', W.DWORD), ('file', C.c_char_p), ('addr', DWORD64)]


class ADDR(C.Structure):
    _fields_ = [('off', DWORD64), ('seg', W.WORD), ('mode', W.DWORD)]


class FRAME(C.Structure):
    _fields_ = [('pc', ADDR), ('ret', ADDR), ('fp', ADDR), ('sp', ADDR), ('bs', ADDR), ('fte', C.c_void_p), ('params', DWORD64 * 4),
                ('far', W.BOOL), ('virt', W.BOOL), ('res', DWORD64 * 3), ('kd', C.c_byte * 112)]


k.OpenThread.restype = W.HANDLE
k.WaitForDebugEvent.argtypes = [C.POINTER(EV), W.DWORD]
k.ReadProcessMemory.argtypes = [W.HANDLE, C.c_void_p, C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)]
d.SymInitializeW.argtypes = [W.HANDLE, W.LPCWSTR, W.BOOL]
d.SymFromAddr.argtypes = [W.HANDLE, DWORD64, C.POINTER(DWORD64), C.POINTER(SYM)]
d.SymGetLineFromAddr64.argtypes = [W.HANDLE, DWORD64, C.POINTER(W.DWORD), C.POINTER(LINE)]
d.SymFunctionTableAccess64.restype = C.c_void_p
d.SymFunctionTableAccess64.argtypes = [W.HANDLE, DWORD64]
d.SymGetModuleBase64.restype = DWORD64
d.SymGetModuleBase64.argtypes = [W.HANDLE, DWORD64]
d.StackWalk64.argtypes = [W.DWORD, W.HANDLE, W.HANDLE, C.POINTER(FRAME), C.c_void_p, C.c_void_p, C.c_void_p, C.c_void_p, C.c_void_p]


def name(h, a):
    s = SYM()
    s.size = 88
    s.mx = 1000
    off = DWORD64()
    base = d.SymGetModuleBase64(h, a)
    mn = C.create_unicode_buffer(300); C.windll.psapi.GetModuleFileNameExW(W.HANDLE(h), C.c_void_p(base), mn, 300); t = f'{mn.value}+{a - base:#x}' if base else f'{a:#x} (no module)'
    if d.SymFromAddr(h, a, C.byref(off), C.byref(s)):
        t = mn.value.split(chr(92))[-1] + '!' + s.name.decode(errors='replace') + f'+{off.value:#x}'
    ln = LINE()
    ln.size = C.sizeof(LINE)
    dis = W.DWORD()
    if d.SymGetLineFromAddr64(h, a, C.byref(dis), C.byref(ln)) and ln.file:
        t += '  [' + '/'.join(ln.file.decode(errors='replace').replace('\\', '/').split('/')[-3:]) + f':{ln.line}]'
    return t


def stack(h, tid):
    th = k.OpenThread(0x1FFFFF, False, tid)
    raw = C.create_string_buffer(1232 + 16)
    ctx = (C.addressof(raw) + 15) & ~15
    C.memset(ctx, 0, 1232)
    C.c_uint32.from_address(ctx + 0x30).value = 0x10001F
    k.GetThreadContext.argtypes = [W.HANDLE, C.c_void_p]
    if not k.GetThreadContext(th, ctx):
        print('no context', C.get_last_error())
        return
    q = lambda o: C.c_uint64.from_address(ctx + o).value
    f = FRAME()
    f.pc.off, f.pc.mode = q(0xF8), 3
    f.sp.off, f.sp.mode = q(0x98), 3
    f.fp.off, f.fp.mode = q(0xA0), 3
    print('  registers rcx %#x rdx %#x r8 %#x r9 %#x rax %#x' % (q(0x80), q(0x88), q(0xB8), q(0xC0), q(0x78)))
    fa = C.cast(d.SymFunctionTableAccess64, C.c_void_p)
    mb = C.cast(d.SymGetModuleBase64, C.c_void_p)
    for i in range(34):
        if not d.StackWalk64(0x8664, h, th, C.byref(f), ctx, None, fa, mb, None) or not f.pc.off:
            break
        print(f'  #{i:2}', name(h, f.pc.off)[:230])


def run(cmd):
    si = STARTUPINFO()
    si.cb = C.sizeof(si)
    pi = PI()
    null = open(os.devnull, 'w')
    if not k.CreateProcessW(None, C.create_unicode_buffer(cmd), None, None, False, 0x2 | 0x08000000, None, None, C.byref(si), C.byref(pi)):
        raise OSError(C.get_last_error())
    ev = EV()
    crashed = False
    while True:
        if not k.WaitForDebugEvent(C.byref(ev), 90000):
            print('no event for 90 s: ended')
            k.TerminateProcess(pi.hp, 1)
            break
        how = 0x00010002
        if ev.code == 1:
            r = ev.u.ex.rec
            if r.code == 0x80000003:
                how = 0x00010002
            else:
                how = 0x80010001
                if not ev.u.ex.first:
                    crashed = True
                    print(f'second chance: code {r.code:#x} at {r.addr or 0:#x}, kind {r.info[0]} (0 read, 1 write, 8 run), address {r.info[1]:#x}, thread {ev.tid}')
                    d.SymSetOptions(0x10 | 0x2 | 0x4)
                    d.SymInitializeW(pi.hp, None, True)
                    stack(pi.hp, ev.tid)
                    sys.stdout.flush()
                    k.TerminateProcess(pi.hp, 1)
        elif ev.code == 5:
            k.ContinueDebugEvent(ev.pid, ev.tid, 0x00010002)
            break
        k.ContinueDebugEvent(ev.pid, ev.tid, how)
    k.CloseHandle(pi.hp)
    k.CloseHandle(pi.ht)
    return crashed


os.environ['_NO_DEBUG_HEAP'] = '1'
n = int(sys.argv[1])
cmd = ' '.join(f'"{a}"' for a in sys.argv[2:])
found = 0
for i in range(n):
    if run(cmd):
        found += 1
        print('--- run', i)
        sys.stdout.flush()
        if found >= 3:
            break
print('runs', i + 1, 'crashes', found)

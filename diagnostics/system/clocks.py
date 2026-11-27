# Author: libs (hle-d3d12-amd)
"""Read the card's sensors through AMD's own library (read only): python -I clocks.py [seconds] [step] [out.csv]"""
import ctypes as C
import sys
import time

adl = C.CDLL(r'C:\Windows\System32\atiadlxx.dll')
keep = []
MALLOC = C.CFUNCTYPE(C.c_void_p, C.c_int)


def alloc(n):
    b = C.create_string_buffer(n)
    keep.append(b)
    return C.addressof(b)


cb = MALLOC(alloc)
ctx = C.c_void_p()
assert adl.ADL2_Main_Control_Create(cb, 1, C.byref(ctx)) == 0


class Info(C.Structure):
    _fields_ = [('size', C.c_int), ('index', C.c_int), ('udid', C.c_char * 256), ('bus', C.c_int), ('dev', C.c_int), ('fn', C.c_int),
                ('vendor', C.c_int), ('name', C.c_char * 256), ('display', C.c_char * 256), ('present', C.c_int), ('exist', C.c_int),
                ('path', C.c_char * 256), ('path2', C.c_char * 256), ('pnp', C.c_char * 256), ('os', C.c_int)]


class Sensor(C.Structure):
    _fields_ = [('supported', C.c_int), ('value', C.c_int)]


class Log(C.Structure):
    _fields_ = [('size', C.c_int), ('s', Sensor * 256)]


n = C.c_int()
adl.ADL2_Adapter_NumberOfAdapters_Get(ctx, C.byref(n))
infos = (Info * n.value)()
adl.ADL2_Adapter_AdapterInfo_Get(ctx, infos, C.sizeof(infos))
seen = {}
for i in infos:
    if i.present and i.bus not in seen:
        seen[i.bus] = (i.index, i.name.decode(errors='replace'))
print(seen)
# Sensor places from AMD's public header (ADL_PMLOG_*): the names that matter here.
NAMES = {1: 'gfxclk', 2: 'memclk', 3: 'socclk', 8: 'temp_edge', 19: 'activity_gfx', 20: 'activity_mem', 21: 'gfx_voltage', 25: 'temp_hotspot?', 74: 'bus_speed', 75: 'bus_lanes'}


def read(idx):
    log = Log()
    log.size = C.sizeof(log)
    if adl.ADL2_New_QueryPMLogData_Get(ctx, idx, C.byref(log)) != 0:
        return None
    return log


idx = None
for bus, (i, name) in seen.items():
    log = read(i)
    if log and any(log.s[k].supported for k in range(256)):
        idx = i
        print('adapter', name, 'sensors', sum(1 for k in range(256) if log.s[k].supported))
        if len(sys.argv) < 2:
            for k in range(256):
                if log.s[k].supported:
                    print('  ', k, NAMES.get(k, ''), log.s[k].value)
        break
if idx is not None and len(sys.argv) >= 2:
    end = time.time() + float(sys.argv[1])
    step = float(sys.argv[2]) if len(sys.argv) > 2 else 0.5
    out = open(sys.argv[3], 'w') if len(sys.argv) > 3 else None
    rows = []
    while time.time() < end:
        log = read(idx)
        row = [time.strftime('%H:%M:%S')] + [log.s[k].value for k in (1, 2, 19, 73)]
        rows.append(row)
        if out:
            out.write(','.join(map(str, row)) + '\n')
            out.flush()
        time.sleep(step)
    for c, t in ((1, 'gfxclk MHz'), (2, 'memclk MHz'), (3, 'gfx activity %'), (4, 'board power W')):
        v = sorted(r[c] for r in rows)
        print(f'{t}: min {v[0]} median {v[len(v) // 2]} max {v[-1]} over {len(v)} samples')
adl.ADL2_Main_Control_Destroy(ctx)

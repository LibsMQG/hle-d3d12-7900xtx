"""Groups what the title asked for memory and resources, from a diagnostic log
run with VICTORY_LOG="info,victory_hle::modules::xmem=debug,victory_gpu::resource=debug".
Read only. Usage: python -I tools/intent.py <log> [--map out.tsv]"""
import collections
import re
import sys

sys.stdout.reconfigure(encoding="utf-8")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
FIELD = re.compile(r"(\w+)=([^ ]+)")
MB = 1 << 20

alloc = collections.defaultdict(lambda: [0, 0])  # (protect, kind, flags, own base) -> count, bytes
regions = []  # (start, size, protect, kind, flags) of reservations the title made
pages = collections.defaultdict(lambda: [0, 0])  # flags -> calls, pages
addressed = {}  # address -> (dimension, width, layout)
asked = collections.Counter()
other = collections.defaultdict(lambda: collections.Counter())

with open(sys.argv[1], encoding="utf-8", errors="replace") as log:
    for line in log:
        if " DEBUG " not in line and "xmem" not in line:
            continue
        line = ANSI.sub("", line)
        if "XMemVirtualAlloc size=" in line:
            f = dict(FIELD.findall(line))
            key = (f["asked_protect"], f["asked_kind"], f["flags"], "new" if f["base"] == "0x0" else "inside")
            alloc[key][0] += 1
            alloc[key][1] += int(f["size"])
            at = int(f["at"], 16)
            if at:
                regions.append((at, int(f["size"]), f["asked_protect"], f["asked_kind"], f["flags"]))
        elif "XMemAllocatePhysicalPages" in line and "n=" in line:
            f = dict(FIELD.findall(line))
            pages[f.get("flags", "?")][0] += 1
            pages[f.get("flags", "?")][1] += int(f.get("n", 0))
        elif "resource at an address" in line:
            f = dict(FIELD.findall(line))
            asked["resource at an address"] += 1
            addressed[int(f["address"], 16)] = (f.get("dimension"), int(f.get("width", 0)), f.get("layout"))
        else:
            for name in ("committed resource", '"heap"', " heap ", "page pool", "placed resource"):
                if name in line:
                    f = dict(FIELD.findall(line))
                    key = " ".join(f"{k}={v}" for k, v in f.items() if k in ("dimension", "heap", "kind", "flags"))
                    other[name.strip()][key] += 1
                    break

print("== XMemVirtualAlloc, by what the title asked (protect, kind, XMem flags)")
print(f"{'protect':>10} {'kind':>12} {'flags':>8} {'base':>7} {'calls':>7} {'MB asked':>10}")
for (protect, kind, flags, base), (n, size) in sorted(alloc.items(), key=lambda kv: -kv[1][1]):
    print(f"{protect:>10} {kind:>12} {flags:>8} {base:>7} {n:>7} {size / MB:>10.0f}")
print("\n== XMemAllocatePhysicalPages by flags (64 KiB pages)")
for flags, (calls, n) in pages.items():
    print(f"flags {flags}: {calls} calls, {n} pages = {n / 16:.0f} MB")
for name, counter in other.items():
    print(f"\n== {name}")
    for key, n in counter.most_common(12):
        print(f"{n:>7}  {key}")

print("\n== resources the title made at an address (latest per address), by the reservation they lie in")
regions.sort()
starts = [r[0] for r in regions]
import bisect


def region_of(address):
    i = bisect.bisect_right(starts, address) - 1
    while i >= 0:
        start, size, protect, kind, flags = regions[i]
        if start <= address < start + size:
            return protect, flags
        if address - start > 1 << 36:
            break
        i -= 1
    return "not in a logged reservation", "-"


table = collections.defaultdict(lambda: [0, 0])
for address, (dimension, width, layout) in addressed.items():
    key = (*region_of(address), dimension, layout)
    table[key][0] += 1
    table[key][1] += width if dimension == "1" else 0
print(f"calls in all: {asked['resource at an address']}, distinct addresses: {len(addressed)}")
print(f"{'protect':>28} {'flags':>8} {'dim':>4} {'layout':>7} {'count':>7} {'buffer MB':>10}")
for (protect, flags, dimension, layout), (n, size) in sorted(table.items(), key=lambda kv: -kv[1][0])[:30]:
    print(f"{protect:>28} {flags:>8} {dimension:>4} {layout:>7} {n:>7} {size / MB:>10.0f}")

# ---- the repeated copies and compares of the last statistics block, by reservation
COPIED = re.compile(r"copied from (0x[0-9a-f]+)\+(0x[0-9a-f]+) \(memory 0x[0-9a-f]+\): ([\d.]+)x/frame, ([\d.]+) MB/frame, ([\d.]+) ms")
COMPARED = re.compile(r"compared (0x[0-9a-f]+)\+(0x[0-9a-f]+): ([\d.]+)x/frame, ([\d.]+) MB/frame, ([\d.]+) MB/frame of it written back")
blocks = {"copied": [[]], "compared": [[]]}
with open(sys.argv[1], encoding="utf-8", errors="replace") as log:
    for line in log:
        if "copies per frame:" in line:
            for b in blocks.values():
                b.append([])
        elif " copied from 0x" in line or " compared 0x" in line:
            line = ANSI.sub("", line)
            for kind, rx in (("copied", COPIED), ("compared", COMPARED)):
                m = rx.search(line)
                if m:
                    blocks[kind][-1].append(m.groups())

print("\n== reservations (start, end, asked protect, XMem flags)")
for start, size, protect, kind, flags in regions:
    print(f"{start:#014x} - {start + size:#014x}  {size / MB:>8.0f} MB  protect {protect:>8}  flags {flags}")
for kind in ("copied", "compared"):
    filled = [b for b in blocks[kind] if b]
    if not filled:
        continue
    last = filled[-1]
    title = "ranges asked to be refreshed (size checked, NOT bytes copied)" if kind == "copied" else "ranges compared after shaders wrote (and bytes written back)"
    print(f"\n== last block: largest {title}")
    for row in last:
        at = int(row[0], 16)
        protect, flags = region_of(at)
        tail = f"  ms/frame {row[4]}" if kind == "copied" else f"  written back {row[4]} MB/frame"
        print(f"{row[0]:>13}+{row[1]:<10} {row[2]:>5}x {float(row[3]):>9.2f} MB/frame{tail}   protect {protect} flags {flags}")

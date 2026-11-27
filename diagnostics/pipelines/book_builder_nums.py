# Author: libs (hle-d3d12-7900xtx)
"""Read-only: numbers (order in known-pipelines.bin) of the pipelines whose only shader is a container
naming XboxBvhBuilder (the ray-tracing structure builder), comma-separated, plus counts on stderr."""
import struct
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import book_stages as b  # noqa: E402

d = Path(sys.argv[1])
raw = (d / "known-shaders.bin").read_bytes()
builder = set()
at = 0
while at + 37 <= len(raw):
    size = struct.unpack_from("<I", raw, at + 33)[0]
    if b"XboxBvhBuilder" in raw[at + 37:at + 37 + size]:
        builder.add(raw[at + 1:at + 33])
    at += 37 + size
nums = [n for n, (key, st) in enumerate(b.pipelines(d)) if len(st) == 1 and st[0][1] in builder]
print(",".join(map(str, nums)))
print(f"builder containers {len(builder)}, pipelines {len(nums)}", file=sys.stderr)

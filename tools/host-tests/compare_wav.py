#!/usr/bin/env python3
"""Compare two float WAV files sample by sample. Exit 0 only if bit-identical and not silent."""
import struct, sys

def read(path):
    data = open(path, "rb").read()
    assert data[:4] == b"RIFF" and data[8:12] == b"WAVE", path
    pos, fmt, frames = 12, None, None
    while pos + 8 <= len(data):
        cid, size = data[pos:pos + 4], struct.unpack("<I", data[pos + 4:pos + 8])[0]
        body = data[pos + 8:pos + 8 + size]
        if cid == b"fmt ":
            fmt = struct.unpack("<HHIIHH", body[:16])
        elif cid == b"data":
            frames = body
        pos += 8 + size + (size & 1)
    tag, ch, rate, _, _, bits = fmt
    assert bits == 32, f"{path}: expected 32-bit float, got {bits}"
    return ch, rate, frames

a, b = read(sys.argv[1]), read(sys.argv[2])
assert a[:2] == b[:2], f"format differs: {a[:2]} vs {b[:2]}"
fa = struct.unpack(f"<{len(a[2]) // 4}f", a[2])
fb = struct.unpack(f"<{len(b[2]) // 4}f", b[2])
peak = max(abs(x) for x in fa)
diff = sum(1 for x, y in zip(fa, fb) if x != y) + abs(len(fa) - len(fb))
print(f"channels={a[0]} rate={a[1]} samples={len(fa)} peak={peak:.4f} differing={diff}")
if peak < 1e-3:
    sys.exit("FAIL: signal is silent")
if diff:
    sys.exit("FAIL: output differs")
print("PASS: bit-identical pass-through")

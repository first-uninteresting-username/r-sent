#!/usr/bin/env python3
"""Generates the example images (no third-party modules needed).

- diagram.png:      opaque 480x270 diagram (boxes and an arrow)
- transparent.png:  320x320 RGBA ring with a soft, partly transparent edge
- gradient.ff.bz2:  256x64 farbfeld gradient, compressed (decoded through the bunzip2 filter)
"""
import bz2
import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))


def png(path, w, h, pixels, alpha):
    """pixels: function (x, y) -> tuple of 3 or 4 ints."""
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            raw.extend(pixels(x, y))

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6 if alpha else 2, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", ihdr))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
        f.write(chunk(b"IEND", b""))


def diagram(x, y):
    bg = (250, 250, 245)
    ink = (30, 30, 40)
    boxes = [(30, 95, 150, 175, (90, 150, 220)), (330, 95, 450, 175, (230, 140, 60))]
    for x0, y0, x1, y1, col in boxes:
        if x0 <= x < x1 and y0 <= y < y1:
            if x < x0 + 4 or x >= x1 - 4 or y < y0 + 4 or y >= y1 - 4:
                return ink
            return col
    # Arrow shaft and head between the boxes.
    if 150 <= x < 300 and 131 <= y < 139:
        return ink
    if 300 <= x < 330 and abs(y - 135) <= (330 - x) * 0.8:
        return ink
    return bg


def ring(x, y):
    cx = cy = 160
    d = math.hypot(x - cx, y - cy)
    a = max(0.0, 1.0 - abs(d - 110) / 40)
    hue = math.atan2(y - cy, x - cx)
    r = int(127 + 127 * math.cos(hue))
    g = int(127 + 127 * math.cos(hue + 2.094))
    b = int(127 + 127 * math.cos(hue + 4.189))
    return (r, g, b, int(255 * a))


def farbfeld(path, w, h):
    data = bytearray(b"farbfeld" + struct.pack(">II", w, h))
    for y in range(h):
        for x in range(w):
            v = x * 257
            data.extend(struct.pack(">HHHH", v, 65535 - v, (y * 1024) & 0xFFFF, 65535))
    with open(path, "wb") as f:
        f.write(bz2.compress(bytes(data)))


if __name__ == "__main__":
    png(os.path.join(HERE, "diagram.png"), 480, 270, diagram, alpha=False)
    png(os.path.join(HERE, "transparent.png"), 320, 320, ring, alpha=True)
    farbfeld(os.path.join(HERE, "gradient.ff.bz2"), 256, 64)

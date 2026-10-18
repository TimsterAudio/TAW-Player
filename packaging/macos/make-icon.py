"""Draws the Player's icon (the one src/icon.rs draws at 64 px) as a PNG
of any size, with only Python's standard library, for the macOS .icns.

    python3 make-icon.py 1024 icon_512x512@2x.png
"""

import struct
import sys
import zlib

MARGIN = 100 / 1024
STRIPE = [(0xD8, 0x43, 0x2A), (0xEC, 0x7B, 0x24), (0xEF, 0xC2, 0x3B), (0xE7, 0xE0, 0xC9)]


def sample(x, y):
    """Colour and coverage at (x, y) on src/icon.rs's 64-unit grid."""
    dx, dy = abs(x - 32.0) - 24.0, abs(y - 32.0) - 24.0
    outside = (max(dx, 0.0) ** 2 + max(dy, 0.0) ** 2) ** 0.5 - 7.5
    if outside > 0.0:
        return None
    c = (0x17, 0x17, 0x17)
    if 50.0 <= y < 54.0 and 8.0 <= x < 56.0:
        c = STRIPE[min(int((x - 8.0) // 12.0), 3)]
    half = (x - 22.0) / 24.0 * 15.0
    if 22.0 <= x < 46.0 and abs(y - 27.0) < 15.0 - half:
        c = (0xFF, 0x5B, 0x36)
    return c


def draw(size, ss=3):
    rows = []
    for py in range(size):
        row = bytearray([0])
        for px in range(size):
            r = g = b = n = 0
            for sy in range(ss):
                for sx in range(ss):
                    # macOS icons leave a margin: the badge fills the
                    # middle 824 of 1024.
                    x = ((px + (sx + 0.5) / ss) / size - MARGIN) / (1 - 2 * MARGIN) * 64.0
                    y = ((py + (sy + 0.5) / ss) / size - MARGIN) / (1 - 2 * MARGIN) * 64.0
                    c = sample(x, y)
                    if c:
                        r += c[0]
                        g += c[1]
                        b += c[2]
                        n += 1
            if n:
                row += bytes((r // n, g // n, b // n, n * 255 // (ss * ss)))
            else:
                row += bytes(4)
        rows.append(bytes(row))
    return b"".join(rows)


def png(size, data):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    head = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")


if __name__ == "__main__":
    n = int(sys.argv[1])
    with open(sys.argv[2], "wb") as f:
        f.write(png(n, draw(n)))

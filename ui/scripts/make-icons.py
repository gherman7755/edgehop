#!/usr/bin/env python3
"""Draws the EdgeHop icons (resources/*.png) without any imaging library: `python3 scripts/make-icons.py`.

The mark is a double-headed arrow: control passes across an edge in both directions.
"""
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "resources"
SS = 4  # samples per pixel along each axis


def png(path, width, height, rgba):
    raw = b"".join(b"\x00" + bytes(rgba[y * width * 4:(y + 1) * width * 4]) for y in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def inside_polygon(x, y, poly):
    result = False
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            result = not result
        j = i
    return result


def arrow_polygons(scale):
    """The double arrow in a 0..1 square, as polygons scaled to `scale` pixels."""
    t = 0.07  # half thickness of the shaft
    head = 0.17
    left, right, mid = 0.17, 0.83, 0.5
    shaft = [(left + head * 0.6, mid - t), (right - head * 0.6, mid - t), (right - head * 0.6, mid + t), (left + head * 0.6, mid + t)]
    head_l = [(left, mid), (left + head, mid - head), (left + head, mid + head)]
    head_r = [(right, mid), (right - head, mid - head), (right - head, mid + head)]
    return [[(px * scale, py * scale) for px, py in poly] for poly in (shaft, head_l, head_r)]


def rounded_rect(x, y, size, radius):
    cx = min(max(x, radius), size - radius)
    cy = min(max(y, radius), size - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius**2


def render(size, shade):
    """`shade(u, v, in_mark, in_shape)` returns an (r, g, b, a) tuple for the point (u, v) in 0..1."""
    polys = arrow_polygons(size)
    pixels = bytearray()
    for py in range(size):
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(SS):
                for sx in range(SS):
                    x = px + (sx + 0.5) / SS
                    y = py + (sy + 0.5) / SS
                    mark = any(inside_polygon(x, y, p) for p in polys)
                    color = shade(x / size, y / size, mark, rounded_rect(x, y, size, size * 0.225))
                    a = color[3]
                    acc[0] += color[0] * a
                    acc[1] += color[1] * a
                    acc[2] += color[2] * a
                    acc[3] += a
            n = SS * SS
            a = acc[3]
            if a == 0:
                pixels += bytes((0, 0, 0, 0))
            else:
                pixels += bytes((round(acc[0] / a), round(acc[1] / a), round(acc[2] / a), round(a / n)))
    return pixels


def app_icon(u, v, mark, shape):
    if not shape:
        return (0, 0, 0, 0)
    if mark:
        return (255, 255, 255, 255)
    # indigo to teal, top-left to bottom-right
    t = (u + v) / 2
    return (round(79 + (20 - 79) * t), round(70 + (184 - 70) * t), round(229 + (166 - 229) * t), 255)


def tray_template(u, v, mark, shape):
    return (0, 0, 0, 255) if mark else (0, 0, 0, 0)


def tray_color(u, v, mark, shape):
    if not shape:
        return (0, 0, 0, 0)
    return (255, 255, 255, 255) if mark else (67, 97, 238, 255)


for name, size, shade in [
    ("icon.png", 512, app_icon),
    ("trayTemplate.png", 18, tray_template),
    ("trayTemplate@2x.png", 36, tray_template),
    ("tray.png", 32, tray_color),
]:
    png(OUT / name, size, size, render(size, shade))
    print("wrote", OUT / name)

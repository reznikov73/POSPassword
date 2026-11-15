#!/usr/bin/env python3
"""Рисует иконки расширения «Хранитель»: навесной замок на синем фоне.

Без внешних зависимостей: пиксели пишутся вручную, PNG собирается zlib'ом.
"""
import zlib
import struct
import os

def build_png(size: int) -> bytes:
    """Собирает PNG размером size×size с рисунком замка."""
    px = [[(0, 0, 0, 0) for _ in range(size)] for _ in range(size)]

    def put(x, y, rgba):
        if 0 <= x < size and 0 <= y < size:
            px[y][x] = rgba

    # Фон: скруглённый квадрат насыщенного синего.
    r = size * 0.22
    blue = (38, 92, 180, 255)
    edge = max(2, size // 16)
    for y in range(size):
        for x in range(size):
            # Скругление: считаем расстояние до углов.
            inside = True
            for cx, cy in ((r, r), (size - r, r), (r, size - r), (size - r, size - r)):
                if (x < r or x > size - r) and (y < r or y > size - r):
                    dx, dy = x - cx, y - cy
                    if dx * dx + dy * dy > r * r:
                        inside = False
            if inside:
                # Тонкая рамка чуть светлее.
                border = (
                    x < edge or y < edge or x >= size - edge or y >= size - edge
                )
                px[y][x] = (74, 126, 208, 255) if border else blue

    # Дужка замка: кольцо из белых пикселей в верхней половине.
    cy = size * 0.38
    rx, ry = size * 0.21, size * 0.19
    white = (255, 255, 255, 255)
    for y in range(size):
        for x in range(size):
            if y > size * 0.62:
                continue
            d = ((x - size / 2) / rx) ** 2 + ((y - cy) / ry) ** 2
            if 0.55 <= d <= 1.0:
                put(x, y, white)

    # Корпус: белый прямоугольник в нижней части.
    top, bottom = int(size * 0.52), int(size * 0.82)
    left, right = int(size * 0.24), int(size * 0.76)
    for y in range(top, bottom):
        for x in range(left, right):
            put(x, y, white)

    # Замочная скважина: синяя точка и щель.
    kx, ky = size / 2, size * 0.63
    kr = max(1, size * 0.05)
    for y in range(size):
        for x in range(size):
            if (x - kx) ** 2 + (y - ky) ** 2 <= kr * kr:
                put(x, y, blue)
    for y in range(int(ky), int(size * 0.74)):
        for x in range(int(kx - kr), int(kx + kr + 1)):
            put(x, y, blue)

    # Сборка PNG.
    raw = b""
    for row in px:
        raw += b"\x00" + b"".join(bytes(c) for c in row)

    def chunk(tag: bytes, data: bytes) -> bytes:
        block = tag + data
        return (
            struct.pack(">I", len(data)) + block + struct.pack(">I", zlib.crc32(block))
        )

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main():
    base = os.path.join(os.path.dirname(__file__), "..", "extension", "icons")
    os.makedirs(base, exist_ok=True)
    for size in (16, 48, 128):
        path = os.path.join(base, f"{size}.png")
        with open(path, "wb") as fh:
            fh.write(build_png(size))
        print(f"{path}: {os.path.getsize(path)} байт")


if __name__ == "__main__":
    main()

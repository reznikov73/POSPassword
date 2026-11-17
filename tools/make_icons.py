#!/usr/bin/env python3
"""Рисует иконки «Хранителя»: белый щит с замочной скважиной на синем.

Нужен Pillow. Скрипт создаёт:
  extension/icons/16.png, 48.png, 128.png   — для расширения браузера
  installer/hranitel.ico                    — для установщика
  apps/vault-win/icon.ico                  — встраивается в exe (build.rs)

Запуск: python tools/make_icons.py
"""

from __future__ import annotations

import os

from PIL import Image, ImageDraw

# Размер «холста»: рисуем крупно, уменьшаем с Lanczos — так края гладкие.
S = 1024

TOP = (58, 123, 240)      # светлее сверху
BOTTOM = (23, 55, 139)    # темнее снизу
WHITE = (255, 255, 255, 255)


def vertical_gradient(size: int) -> Image.Image:
    image = Image.new("RGB", (1, size))
    pixels = image.load()
    for y in range(size):
        t = y / max(1, size - 1)
        pixels[0, y] = tuple(
            round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3)
        )
    return image.resize((size, size), Image.BILINEAR)


def rounded_mask(size: int, radius: float) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return mask


def shield_mask(size: int) -> Image.Image:
    """Щит: скруглённый верх, плавное сужение к острию и отступ от края."""
    import math

    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)

    cx = size / 2
    half = size * 0.300
    top = size * 0.200
    radius = size * 0.085
    shoulder = size * 0.500
    tip = size * 0.780

    points = []

    # Левый верхний угол: дуга от 180° к 270°.
    corner_cx, corner_cy = cx - half + radius, top + radius
    for step in range(13):
        angle = math.pi + (math.pi / 2) * step / 12
        points.append(
            (corner_cx + radius * math.cos(angle), corner_cy + radius * math.sin(angle))
        )

    # Правый верхний угол: от 270° к 360°.
    corner_cx = cx + half - radius
    for step in range(13):
        angle = 1.5 * math.pi + (math.pi / 2) * step / 12
        points.append(
            (corner_cx + radius * math.cos(angle), corner_cy + radius * math.sin(angle))
        )

    # Правая сторона вниз и кривая к острию.
    points.append((cx + half, shoulder))
    steps = 64
    for step in range(1, steps + 1):
        t = step / steps
        points.append(
            (cx + half * (1 - t) ** 1.25, shoulder + (tip - shoulder) * t)
        )

    # Левая кривая обратно вверх.
    for step in range(steps - 1, -1, -1):
        t = step / steps
        points.append(
            (cx - half * (1 - t) ** 1.25, shoulder + (tip - shoulder) * t)
        )

    draw.polygon(points, fill=255)
    return mask


def keyhole_mask(size: int) -> Image.Image:
    """Замочная скважина: круг и сужающийся книзу паз."""
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)

    cx = size / 2
    cy = size * 0.385
    r = size * 0.080
    draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=255)

    top_w = size * 0.052
    bottom_w = size * 0.036
    bottom = size * 0.570
    draw.polygon(
        [
            (cx - top_w, cy + r * 0.15),
            (cx + top_w, cy + r * 0.15),
            (cx + bottom_w, bottom),
            (cx - bottom_w, bottom),
        ],
        fill=255,
    )
    return mask


def render(size: int) -> Image.Image:
    background = vertical_gradient(size)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.paste(background, (0, 0), rounded_mask(size, size * 0.22))

    shield = shield_mask(size)
    # Скважина «прорезается» в щите: вычитаем её из маски.
    hole = keyhole_mask(size)
    shield = Image.composite(Image.new("L", (size, size), 0), shield, hole)

    canvas.paste(Image.new("RGBA", (size, size), WHITE), (0, 0), shield)
    return canvas


def main() -> None:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    big = render(S)

    def save_png(rel_path: str, size: int) -> None:
        path = os.path.join(root, rel_path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        big.resize((size, size), Image.LANCZOS).save(path, "PNG", optimize=True)
        print("png:", rel_path, size)

    for size in (16, 48, 128):
        save_png(f"extension/icons/{size}.png", size)

    ico_sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    for rel_path in ("installer/hranitel.ico", "apps/vault-win/icon.ico"):
        path = os.path.join(root, rel_path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        big.resize((256, 256), Image.LANCZOS).save(path, format="ICO", sizes=ico_sizes)
        print("ico:", rel_path)


if __name__ == "__main__":
    main()

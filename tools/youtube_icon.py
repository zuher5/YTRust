"""Generates the app icon: the official YouTube icon.

    python tools/youtube_icon.py

The geometry is the standard YouTube icon (the same artwork as
assets/youtube.svg: a 24 x 16.91 rounded rectangle, #FF0000, with a white play
triangle), drawn large and scaled down for smooth edges.

Writes:
  assets/streamtube.ico  Windows: window, taskbar, shortcuts, installer
  assets/youtube.png     256 px, Linux menu icon (embedded in the binary)
"""
import os

from PIL import Image, ImageDraw

OUT = os.path.join(os.path.dirname(__file__), "..", "assets")
S = 1024  # drawn large, scaled down for smooth edges
RED = (255, 0, 0, 255)

# the icon's own aspect: full width, height = 24 * 16.91 / 24 of the canvas
BADGE_H = S * 16.91 / 24
TOP = (S - BADGE_H) / 2
RADIUS = S * 3.016 / 24
K = S / 24


def draw():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, TOP, S - 1, TOP + BADGE_H], radius=RADIUS, fill=RED)
    # play triangle, triangle points from the SVG (9.545,15.568) (15.818,12) (9.545,8.432)
    tri = [(9.545 * K, 15.568 * K), (15.818 * K, 12 * K), (9.545 * K, 8.432 * K)]
    d.polygon(tri, fill=(255, 255, 255, 255))
    return img


def main():
    img = draw()
    img.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "youtube.png"))
    img.resize((256, 256), Image.LANCZOS).save(
        os.path.join(OUT, "streamtube.ico"),
        sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print("wrote assets/youtube.png and assets/streamtube.ico")


if __name__ == "__main__":
    main()

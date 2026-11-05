"""A made-up planet map for texture tests and the user guide: testdata/images/planet.jpg.

    python3 scripts/gen_texture.py [out]

An equirectangular image (1024 x 512, longitude -180..180 left to right, latitude 90..-90 top
to bottom): continents from a sum of random waves on the sphere (so the map wraps without a
seam), shaded by height, ice caps, and a graticule every 30 degrees.
"""
import os, sys
import numpy as np
from PIL import Image

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "testdata", "images", "planet.jpg")
W, H = 1024, 512
lon = (np.arange(W) + 0.5) / W * 2 * np.pi - np.pi
lat = np.pi / 2 - (np.arange(H) + 0.5) / H * np.pi
LON, LAT = np.meshgrid(lon, lat)
P = np.stack([np.cos(LAT) * np.cos(LON), np.cos(LAT) * np.sin(LON), np.sin(LAT)], -1)
rng = np.random.default_rng(7)
h = np.zeros((H, W))
for octave in range(7):
    k = 2.5 * 1.9 ** octave
    for _ in range(10):
        d = rng.normal(size=3)
        d /= np.linalg.norm(d)
        h += np.sin(k * P @ d + rng.uniform(0, 2 * np.pi)) / (1.8 ** octave)
h = (h - h.mean()) / h.std()
sea = h < 0.25
img = np.zeros((H, W, 3))
deep, shallow = np.array([0.05, 0.15, 0.45]), np.array([0.15, 0.45, 0.75])
low, mid, high = np.array([0.25, 0.55, 0.2]), np.array([0.6, 0.55, 0.3]), np.array([0.5, 0.45, 0.4])
t = np.clip((h + 2.0) / 2.25, 0, 1)[..., None]
img = np.where(sea[..., None], deep * (1 - t) + shallow * t, 0)
u = np.clip((h - 0.25) / 2.0, 0, 1)[..., None]
land = np.where(u < 0.5, low * (1 - 2 * u) + mid * 2 * u, mid * (2 - 2 * u) + high * (2 * u - 1))
img = np.where(sea[..., None], img, land)
ice = np.abs(np.degrees(LAT)) > 72 + 4 * np.sin(3 * LON)
img[ice] = [0.92, 0.95, 0.98]
lat_d, lon_d = np.degrees(LAT), np.degrees(LON)
grid = (np.abs(((lat_d + 15) % 30) - 15) < 0.35) | (np.abs(((lon_d + 15) % 30) - 15) < 0.35)
img[grid] = img[grid] * 0.5 + 0.5
Image.fromarray((img * 255).astype(np.uint8)).save(out, quality=88)
print(out, os.path.getsize(out))

"""Reference columnar AM grains: testdata/synthetic/columns_reference.json.

    python3 scripts/columns_reference.py <AM-Orientation-Shift-TOOLKIT folder>

Runs the toolkit's am_build_campaign.generate_columns on two small builds (constant and
lognormal widths) with a NumPy generator wrapped so that every random number it draws is
recorded, in order, as a uniform ('r') or a standard normal ('n') value. auraFX's Columnar
Grains replays the same numbers and must give the same label volume voxel for voxel.
"""
import json, os, sys
import numpy as np

sys.path.insert(0, sys.argv[1])
import am_build_campaign as B


class Recorder:
    """The four Generator calls generate_columns makes, logged as their underlying draws."""

    def __init__(self, seed):
        self.g = np.random.default_rng(seed)
        self.log = []

    def random(self):
        v = float(self.g.random())
        self.log.append(["r", v])
        return v

    def standard_normal(self):
        v = float(self.g.standard_normal())
        self.log.append(["n", v])
        return v

    def uniform(self, lo, hi):
        return lo + (hi - lo) * self.random()

    def normal(self, loc, scale):
        return loc + scale * self.standard_normal()


def rle(a):
    """Run lengths of a label array in C order: [value, count, value, count, ...]."""
    out, start = [], 0
    for i in range(1, len(a) + 1):
        if i == len(a) or a[i] != a[start]:
            out += [int(a[start]), i - start]
            start = i
    return out


cases = []
for name, mic, seed in [
    ("constant widths", {"grain_width_um": 50.0, "grain_height_um": 150.0, "height_sigma": 0.5, "column_tilt_sd_deg": 6.0}, 7),
    ("lognormal widths", {"grain_width_um": 40.0, "grain_height_um": 120.0, "height_sigma": 0.6, "column_tilt_sd_deg": 8.0, "width_sigma": 0.3, "width_height_exponent": 0.5}, 11),
]:
    cfg = {"geometry": {"box_um": [200.0, 160.0, 300.0], "voxel_um": 5.0}, "microstructure": mic}
    rec = Recorder(seed)
    labels, grains, grid = B.generate_columns(cfg, rec)
    cases.append({
        "name": name,
        "box_um": cfg["geometry"]["box_um"],
        "voxel_um": 5.0,
        "microstructure": mic,
        "shape_zyx": list(labels.shape),
        "labels_rle": rle(labels.ravel()),
        "draws": rec.log,
        "grains": int(labels.max()) + 1,
        "base_um": grains["base_um"].tolist(),
        "top_um": grains["top_um"].tolist(),
    })
    print(name, labels.shape, int(labels.max()) + 1, "grains", len(rec.log), "draws")
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "synthetic", "columns_reference.json")
os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w") as f:
    json.dump(cases, f)
print("wrote", out, os.path.getsize(out), "bytes")

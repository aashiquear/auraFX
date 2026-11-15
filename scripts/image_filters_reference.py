#!/usr/bin/env python3
"""testdata/images/filters_reference.json: scipy.ndimage results on a seeded 10 x 8 x 6 random
volume (x fastest), for the volume image filters: median (3 x 3 x 3, edges repeated), Gaussian
(sigma 1, cut at 4 sigma, edges repeated), binary erosion and dilation of v > 0.5 with the
6-connected cross (outside counts as set for erosion, unset for dilation), opening (erosion then
dilation) and closing (dilation then erosion) built from those, and Otsu thresholds (one with 128
bins, two with 64) found by exhaustive search over the histogram's bin edges in numpy.

    python3 scripts/image_filters_reference.py
"""
import itertools
import json
import os

import numpy as np
from scipy import ndimage

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "images", "filters_reference.json")
nx, ny, nz = 10, 8, 6
a = np.random.default_rng(42).random((nz, ny, nx))
st = ndimage.generate_binary_structure(3, 1)
mask = a > 0.5
ero = lambda m: ndimage.binary_erosion(m, structure=st, border_value=1)
dil = lambda m: ndimage.binary_dilation(m, structure=st, border_value=0)


def otsu(v, levels, bins):
    lo, hi = v.min(), v.max()
    w = (hi - lo) / bins
    h = np.minimum(((v - lo) / w).astype(int), bins - 1)
    hist = np.bincount(h.ravel(), minlength=bins) / v.size
    centres = lo + (np.arange(bins) + 0.5) * w
    best, cut = -1, None
    for c in itertools.combinations(range(1, bins), levels):
        edges = (0,) + c + (bins,)
        s = 0.0
        for p, q in zip(edges[:-1], edges[1:]):
            w0 = hist[p:q].sum()
            if w0 > 0:
                m = (hist[p:q] * centres[p:q]).sum()
                s += m * m / w0
        if s > best + 1e-15:
            best, cut = s, c
    return [lo + k * w for k in cut]


ref = {
    "nx": nx, "ny": ny, "nz": nz,
    "input": a.ravel().tolist(),
    "median": ndimage.median_filter(a, size=3, mode="nearest").ravel().tolist(),
    "gaussian": ndimage.gaussian_filter(a, sigma=1, mode="nearest", truncate=4.0).ravel().tolist(),
    "mask": mask.astype(float).ravel().tolist(),
    "erode": ero(mask).astype(float).ravel().tolist(),
    "dilate": dil(mask).astype(float).ravel().tolist(),
    "open": dil(ero(mask)).astype(float).ravel().tolist(),
    "close": ero(dil(mask)).astype(float).ravel().tolist(),
    "otsu": otsu(a, 1, 128)[0],
    "otsu2": otsu(a, 2, 64),
}
os.makedirs(os.path.dirname(OUT), exist_ok=True)
json.dump(ref, open(OUT, "w"))
print("wrote", os.path.normpath(OUT))

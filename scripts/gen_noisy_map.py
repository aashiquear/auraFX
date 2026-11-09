#!/usr/bin/env python3
"""testdata/ebsd/noisy_map.ang: ni_map.ang with 8 % of its points (seeded) spoilt: a random
orientation and a confidence index of 0.02, for the clean-up filters' guide picture.

    python3 scripts/gen_noisy_map.py
"""
import os

import numpy as np

D = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "ebsd")
lines = open(os.path.join(D, "ni_map.ang")).read().splitlines()
rng = np.random.default_rng(11)
out = []
for l in lines:
    if l.startswith("#") or not l.strip():
        out.append(l)
        continue
    t = l.split()
    if rng.random() < 0.08:
        t[0], t[1], t[2] = (f"{rng.random() * 2 * np.pi:.5f}", f"{np.arccos(2 * rng.random() - 1):.5f}", f"{rng.random() * 2 * np.pi:.5f}")
        t[6] = "0.020"
    out.append(" ".join(t))
open(os.path.join(D, "noisy_map.ang"), "w").write("\n".join(out) + "\n")
print("wrote", os.path.normpath(os.path.join(D, "noisy_map.ang")))

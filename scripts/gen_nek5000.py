#!/usr/bin/env python3
"""Write testdata/nek5000/: box.nek5000 and two data files (box0.f00001 with coordinates, box0.f00002
without) of two 3 x 3 x 3 spectral elements side by side (element e spans x in [e, e + 1], y and z
in [0, 1]), single precision, little endian. Fields: velocity (x, y, z), pressure x + y and
temperature 100 + 10 step + z; times 0.1 and 0.2.

    python3 scripts/gen_nek5000.py
"""
import os
import struct

import numpy as np

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "nek5000")
N, NELT = 3, 2


def points(e):
    g = np.array([0.0, 0.5, 1.0])
    z, y, x = np.meshgrid(g, g, g, indexing="ij")  # x fastest
    return (x + e).ravel(), y.ravel(), z.ravel()


def write(step, coords):
    rd = "XUPT" if coords else "UPT"
    head = f"#std 4 {N:2d} {N:2d} {N:2d} {NELT:10d} {NELT:10d} {0.1 * step:20.13E} {step:9d} {0:6d} {1:6d} {rd}"
    b = head.ljust(132).encode()
    b += struct.pack("<f", 6.54321)
    b += struct.pack(f"<{NELT}i", *range(1, NELT + 1))
    blocks = []
    if coords:
        for e in range(NELT):
            x, y, z = points(e)
            blocks += [x, y, z]
    for e in range(NELT):
        x, y, z = points(e)
        blocks += [x, y, z]
    for e in range(NELT):
        x, y, z = points(e)
        blocks.append(x + y)
    for e in range(NELT):
        x, y, z = points(e)
        blocks.append(100 + 10 * step + z)
    for blk in blocks:
        b += np.asarray(blk, "<f4").tobytes()
    open(os.path.join(OUT, f"box0.f{step:05d}"), "wb").write(b)


def main():
    os.makedirs(OUT, exist_ok=True)
    open(os.path.join(OUT, "box.nek5000"), "w").write("filetemplate: box%01d.f%05d\nfirsttimestep: 1\nnumtimesteps: 2\n")
    write(1, True)
    write(2, False)
    print("wrote", os.path.normpath(OUT))


main()

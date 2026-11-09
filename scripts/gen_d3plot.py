#!/usr/bin/env python3
"""Write testdata/d3plot/d3plot (and a family file d3plot01): an LS-DYNA result in single precision,
little endian, NDIM 4: 13 nodes; two hexahedra side by side, a tetrahedron (a hexahedron with
repeated nodes) on top, one beam and one shell; three states (two in d3plot, one in d3plot01) at
times 0, 1e-3, 2e-3 with the nodes moved by x += 0.1 s z, velocities (s, 0, 0), solid stress
XX = 100 e + s and plastic strain 0.01 s, beam axial force s, every shell value s.

Also testdata/d3plot/bar/d3plot: a 12 x 3 x 3 bar of hexahedra (one material) bending over 11
states: tip deflection 0.1 t^2 x^2, stress XX -E z * curvature, for the guide's picture.

    python3 scripts/gen_d3plot.py
"""
import os
import struct

import numpy as np

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "d3plot")


def words(vals, kinds):
    b = b""
    for v, k in zip(vals, kinds):
        b += struct.pack("<i", int(v)) if k == "i" else struct.pack("<f", float(v))
    return b


def main():
    os.makedirs(OUT, exist_ok=True)
    # Nodes: a 3 x 2 x 2 block (x 0..2, y 0..1, z 0..1) and an apex at (0.5, 0.5, 2).
    nodes = [(x, y, z) for z in (0, 1) for y in (0, 1) for x in (0, 1, 2)] + [(0.5, 0.5, 2.0)]
    numnp = len(nodes)
    n = lambda x, y, z: 1 + x + 3 * (y + 2 * z)
    hexes = [[n(e, 0, 0), n(e + 1, 0, 0), n(e + 1, 1, 0), n(e, 1, 0), n(e, 0, 1), n(e + 1, 0, 1), n(e + 1, 1, 1), n(e, 1, 1)] for e in (0, 1)]
    tet = [n(0, 0, 1), n(1, 0, 1), n(0, 1, 1), 13, 13, 13, 13, 13]
    solids = hexes + [tet]
    maxint, nv3d, nv1d = 3, 7, 6
    nv2d = maxint * (6 + 1) + 8 + 4
    ctrl = [0] * 64
    kinds = ["i"] * 64
    ctrl[14] = 971.0
    kinds[14] = "f"
    ctrl[15] = 4          # NDIM
    ctrl[16] = numnp      # NUMNP
    ctrl[17] = 6          # ICODE
    ctrl[18] = 6          # NGLBV
    ctrl[19] = 0          # IT
    ctrl[20] = 1          # IU
    ctrl[21] = 1          # IV
    ctrl[22] = 0          # IA
    ctrl[23] = len(solids)
    ctrl[24] = 1
    ctrl[27] = nv3d
    ctrl[28] = 1          # NEL2
    ctrl[29] = 1
    ctrl[30] = nv1d
    ctrl[31] = 1          # NEL4
    ctrl[32] = 1
    ctrl[33] = nv2d
    ctrl[36] = maxint
    ctrl[43:47] = [1000, 1000, 1000, 1000]
    ctrl[51] = 3          # NMMAT
    title = b"auraFX d3plot test".ljust(40)
    head = title + words(ctrl[10:], kinds[10:])
    geo = words([c for p in nodes for c in p], ["f"] * 3 * numnp)
    for s in solids:
        geo += words(s + [1], ["i"] * 9)
    geo += words([n(0, 0, 0), n(2, 0, 0), n(0, 1, 0), 0, 0, 2], ["i"] * 6)   # beam, material 2
    geo += words([n(0, 0, 0), n(1, 0, 0), n(1, 1, 0), n(0, 1, 0), 3], ["i"] * 5)  # shell, material 3

    def state(s):
        t = 1e-3 * s
        b = words([t] + [10.0 * s, 1.0, 11.0 * s, s, 0, 0], ["f"] * 7)
        b += words([c for (x, y, z) in nodes for c in (x + 0.1 * s * z, y, z)], ["f"] * 3 * numnp)
        b += words([c for _ in nodes for c in (s, 0.0, 0.0)], ["f"] * 3 * numnp)
        for e in range(len(solids)):
            b += words([100 * e + s, 0, 0, 0, 0, 0, 0.01 * s], ["f"] * nv3d)
        b += words([s, 0, 0, 0, 0, 0], ["f"] * nv1d)
        b += words([s] * nv2d, ["f"] * nv2d)
        return b

    open(os.path.join(OUT, "d3plot"), "wb").write(head + geo + state(0) + state(1) + words([-999999.0], ["f"]))
    open(os.path.join(OUT, "d3plot01"), "wb").write(state(2) + words([-999999.0], ["f"]))
    print("wrote", os.path.normpath(OUT))


def bar():
    folder = os.path.join(OUT, "bar")
    os.makedirs(folder, exist_ok=True)
    nx, ny, nz = 12, 3, 3
    nodes = [(i * 1.0, j * 0.5, k * 0.5) for k in range(nz + 1) for j in range(ny + 1) for i in range(nx + 1)]
    nid = lambda i, j, k: 1 + i + (nx + 1) * (j + (ny + 1) * k)
    solids = [[nid(i, j, k), nid(i + 1, j, k), nid(i + 1, j + 1, k), nid(i, j + 1, k), nid(i, j, k + 1), nid(i + 1, j, k + 1), nid(i + 1, j + 1, k + 1), nid(i, j + 1, k + 1)]
              for k in range(nz) for j in range(ny) for i in range(nx)]
    ctrl = [0] * 64
    kinds = ["i"] * 64
    ctrl[14] = 971.0
    kinds[14] = "f"
    ctrl[15], ctrl[16], ctrl[17], ctrl[18] = 4, len(nodes), 6, 6
    ctrl[20], ctrl[21] = 1, 1
    ctrl[23], ctrl[24], ctrl[27] = len(solids), 1, 7
    ctrl[51] = 1
    head = b"auraFX bending bar".ljust(40) + words(ctrl[10:], kinds[10:])
    geo = words([c for p in nodes for c in p], ["f"] * 3 * len(nodes))
    for sld in solids:
        geo += words(sld + [1], ["i"] * 9)
    out = head + geo
    for st in range(11):
        t = st / 10
        b = words([t, 0, 0, 0, 0, 0, 0], ["f"] * 7)
        b += words([c for (x, y, z) in nodes for c in (x, y, z - 0.02 * t * t * x * x)], ["f"] * 3 * len(nodes))
        b += words([c for (x, y, z) in nodes for c in (0.0, 0.0, -0.04 * t * x * x)], ["f"] * 3 * len(nodes))
        for idx in range(len(solids)):
            k = idx // (nx * ny)
            i = idx % nx
            zc = (k + 0.5) * 0.5 - 0.75
            sxx = -2.0e5 * zc * 0.04 * t * t * (1 - i / nx)
            b += words([sxx, 0, 0, 0, 0, 0, max(0.0, abs(sxx) - 300) / 2.0e5], ["f"] * 7)
        out += b
    open(os.path.join(folder, "d3plot"), "wb").write(out + words([-999999.0], ["f"]))


main()
bar()

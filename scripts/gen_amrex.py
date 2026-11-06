"""Small AMReX plotfiles: testdata/amr/plt_3d and testdata/amr/plt_2d.

    python3 scripts/gen_amrex.py [outdir]

Written as AMReX's WriteMultiLevelPlotfile does (Header, Level_n/Cell_H VisMF headers in the
"Version_v1" form with min/max, Cell_D_* files of FABs with their text headers, little-endian
doubles, variables one after another, x fastest). Two levels: level 0 split into two boxes,
level 1 (ratio 2) one box over part of the domain; variables density, temperature and the
vector amrexvec_velocity_x/_y(/_z), which readers combine into "velocity". plt_3d_f32 is the 3-D
file with single-precision data.
"""
import os, sys
import numpy as np

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "amr")

def write(name, dim, real="f8"):
    root = os.path.join(out, name)
    os.makedirs(root, exist_ok=True)
    lo, hi = np.array([-1.0, -1.0, 0.0][:dim]), np.array([1.0, 1.0, 2.0][:dim])
    n0 = np.array([16, 16, 16][:dim])
    ratio = 2
    dx0 = (hi - lo) / n0
    vel = ["amrexvec_velocity_x", "amrexvec_velocity_y", "amrexvec_velocity_z"][:dim]
    names = ["density"] + vel + ["temperature"]
    # Boxes (inclusive cell index ranges per axis).
    lev0 = [(np.zeros(dim, int), np.r_[7, n0[1:] - 1]), (np.r_[8, np.zeros(dim - 1, int)], n0 - 1)]
    lev1 = [(np.r_[10, 8, 4][:dim], np.r_[25, 23, 19][:dim])]
    levels = [(dx0, lev0), (dx0 / ratio, lev1)]
    def field(c):
        x, y = c[0], c[1]
        z = c[2] if dim == 3 else 0.0 * x
        r2 = x * x + y * y + (z - 1.0) ** 2
        return [np.exp(-4 * r2), -y, x] + ([0.1 * z] if dim == 3 else []) + [300 + 50 * np.sin(3 * x) * np.cos(2 * y)]
    with open(os.path.join(root, "Header"), "w") as h:
        h.write("HyperCLaw-V1.1\n%d\n" % len(names))
        for n in names:
            h.write(n + "\n")
        h.write("%d\n%.17g\n%d\n" % (dim, 1.25, len(levels) - 1))
        h.write(" ".join("%.17g" % v for v in lo) + " \n")
        h.write(" ".join("%.17g" % v for v in hi) + " \n")
        h.write("%d \n" % ratio)
        doms = []
        for l, (dx, _) in enumerate(levels):
            n = n0 * ratio ** l
            doms.append("((" + ",".join("0" for _ in range(dim)) + ") (" + ",".join(str(v - 1) for v in n) + ") (" + ",".join("0" for _ in range(dim)) + "))")
        h.write(" ".join(doms) + " \n")
        h.write(" ".join("10" for _ in levels) + " \n")
        for dx, _ in levels:
            h.write(" ".join("%.17g" % v for v in dx) + " \n")
        h.write("0\n0\n")
        for l, (dx, boxes) in enumerate(levels):
            h.write("%d %d %.17g\n%d\n" % (l, len(boxes), 1.25, 10))
            for blo, bhi in boxes:
                for k in range(dim):
                    h.write("%.17g %.17g\n" % (lo[k] + blo[k] * dx[k], lo[k] + (bhi[k] + 1) * dx[k]))
            h.write("Level_%d/Cell\n" % l)
    desc = "((8, (64 11 52 0 1 12 0 1023)),(8, (8 7 6 5 4 3 2 1)))" if real == "f8" else "((8, (32 8 23 0 1 9 0 127)),(4, (4 3 2 1)))"
    for l, (dx, boxes) in enumerate(levels):
        d = os.path.join(root, "Level_%d" % l)
        os.makedirs(d, exist_ok=True)
        offsets, mins, maxs = [], [], []
        with open(os.path.join(d, "Cell_D_00000"), "wb") as f:
            for blo, bhi in boxes:
                axes = [lo[k] + (np.arange(blo[k], bhi[k] + 1) + 0.5) * dx[k] for k in range(dim)]
                grid = np.meshgrid(*axes, indexing="ij")
                vals = [np.asarray(v, float) * np.ones_like(grid[0]) for v in field(grid)]
                offsets.append(f.tell())
                tup = lambda a: "(" + ",".join(str(int(v)) for v in a) + ")"
                f.write(("FAB %s(%s %s %s) %d\n" % (desc, tup(blo), tup(bhi), tup(np.zeros(dim)), len(vals))).encode())
                for v in vals:
                    f.write(np.ascontiguousarray(v.transpose()).astype("<" + real).tobytes())  # x fastest
                mins.append([v.min() for v in vals])
                maxs.append([v.max() for v in vals])
        with open(os.path.join(d, "Cell_H"), "w") as h:
            h.write("1\n0\n%d\n0\n(%d 0\n" % (len(names), len(boxes)))
            for blo, bhi in boxes:
                h.write("((%s) (%s) (%s))\n" % (",".join(map(str, blo)), ",".join(map(str, bhi)), ",".join("0" for _ in range(dim))))
            h.write(")\n%d\n" % len(boxes))
            for o in offsets:
                h.write("FabOnDisk: Cell_D_00000 %d\n" % o)
            h.write("\n%d,%d\n" % (len(boxes), len(names)))
            for m in mins:
                h.write("".join("%.17g," % v for v in m) + "\n")
            h.write("\n%d,%d\n" % (len(boxes), len(names)))
            for m in maxs:
                h.write("".join("%.17g," % v for v in m) + "\n")
    print(root)

write("plt_3d", 3)
write("plt_2d", 2)
write("plt_3d_f32", 3, "f4")

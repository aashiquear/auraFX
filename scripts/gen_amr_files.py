"""Small FLASH and Enzo AMR test files: testdata/amr/.

    pvpython scripts/gen_amr_files.py [outdir]

Written with h5py (inside ParaView's Python) in the layouts VTK's FLASH and Enzo readers read:

flash3d_hdf5_plt_cnt_0007  FLASH 3 (file format version 9, "sim info", "integer scalars",
    "real scalars", bounding boxes padded to 3-D): two root blocks of 4x4x4 cells, the first
    refined into eight; variables dens, pres, velx (cell-centered functions of x, y, z).
flash3d_ffv7_hdf5_chk_0007 the same grid in FLASH 2's layout (file format version 7, "simulation
    parameters").
flash2d_hdf5_chk_0003      FLASH 2 (file format version 7, "simulation parameters"): one root
    block of 8x8 cells refined into four, one of those into four again (three levels);
    variables dens, temp.
enzo/DD0003/data0003       Enzo: parameter file (TopGridRank 3, InitialTime, DataLabel and a
    CGS conversion factor for Density), .hierarchy (a 8^3 root grid, two level-1 grids, one
    level-2 grid inside the first), .boundary, and data0003.cpu0000 (Grid0000000N groups with
    float32 Density and TotalEnergy over the active cells, z slowest).
"""
import os, sys
import numpy as np
import h5py

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "amr")
os.makedirs(out, exist_ok=True)

def field(name, x, y, z):
    return {"dens": 1.0 + x + 2 * y + 3 * z, "pres": np.sin(3 * x) * np.cos(2 * y) + z, "velx": x * y - z,
            "temp": 100.0 + 10 * x - 5 * y, "Density": 2.0 + x * x + y - z, "TotalEnergy": 0.5 + x + y * z}[name]

def cells(lo, hi, n):
    """Cell centers of a block (z, y, x order, x fastest), n = (nx, ny, nz)."""
    c = [lo[k] + (np.arange(n[k]) + 0.5) * (hi[k] - lo[k]) / n[k] for k in range(3)]
    z, y, x = np.meshgrid(c[2], c[1], c[0], indexing="ij")
    return x, y, z

def compound_scalars(names_values, vtype):
    dt = np.dtype([("name", "S80"), ("value", vtype)])
    return np.array([(n.ljust(80).encode(), v) for n, v in names_values], dtype=dt)

# ------------------------------------------------------------------ FLASH 3, 3-D
nb = 4
blocks = []  # (lo, hi, level, node type, parent (1-based or -1), children)
blocks.append(([0, 0, 0], [0.5, 0.5, 0.5], 1, 2))
blocks.append(([0.5, 0, 0], [1.0, 0.5, 0.5], 1, 1))
for k in range(8):
    o = [0.25 * (k & 1), 0.25 * ((k >> 1) & 1), 0.25 * ((k >> 2) & 1)]
    blocks.append((o, [o[0] + 0.25, o[1] + 0.25, o[2] + 0.25], 2, 1))
n = len(blocks)
p = os.path.join(out, "flash3d_hdf5_plt_cnt_0007")
with h5py.File(p, "w") as f:
    si = np.dtype([("file format version", "<i4"), ("setup call", "S400"), ("file creation time", "S80"), ("flash version", "S80"), ("build date", "S80"),
                   ("build dir", "S80"), ("build machine", "S80"), ("cflags", "S400"), ("fflags", "S400"), ("setup time stamp", "S80"), ("build time stamp", "S80")])
    f["sim info"] = np.array([(9, b"./setup test", b"today", b"FLASH 4.6", b"", b"", b"", b"", b"", b"", b"")], dtype=si)
    f["integer scalars"] = compound_scalars([("nxb", nb), ("nyb", nb), ("nzb", nb), ("globalnumblocks", n), ("nstep", 7), ("dimensionality", 3)], "<i4")
    f["real scalars"] = compound_scalars([("time", 0.25), ("dt", 0.01)], "<f8")
    f["unknown names"] = np.array([[b"dens"], [b"pres"], [b"velx"]], dtype="S4")
    f["refine level"] = np.array([b[2] for b in blocks], dtype="<i4")
    f["node type"] = np.array([b[3] for b in blocks], dtype="<i4")
    f["bounding box"] = np.array([[[b[0][d], b[1][d]] for d in range(3)] for b in blocks], dtype="<f8")
    f["coordinates"] = np.array([[(b[0][d] + b[1][d]) / 2 for d in range(3)] for b in blocks], dtype="<f8")
    f["block size"] = np.array([[b[1][d] - b[0][d] for d in range(3)] for b in blocks], dtype="<f8")
    gid = -np.ones((n, 15), dtype="<i4")
    gid[2:, 6] = 1          # parent of the fine blocks: block 1 (1-based)
    gid[0, 7:15] = np.arange(3, 11)
    f["gid"] = gid
    f["processor number"] = np.zeros(n, dtype="<i4")
    for v in ("dens", "pres", "velx"):
        data = np.zeros((n, nb, nb, nb))
        for i, b in enumerate(blocks):
            x, y, z = cells(b[0], b[1], (nb, nb, nb))
            data[i] = field(v, x, y, z)
        f[v] = data.astype("<f4" if v == "velx" else "<f8")

# The same grid in FLASH 2's layout (file format version 7, "simulation parameters", bounding
# boxes and coordinates with as many entries as dimensions).
p2 = os.path.join(out, "flash3d_ffv7_hdf5_chk_0007")
with h5py.File(p, "r") as a, h5py.File(p2, "w") as b:
    for k in a.keys():
        if k not in ("sim info", "integer scalars", "real scalars"):
            b[k] = a[k][()]
    b["file format version"] = np.array([7], dtype="<i4")
    sp = np.dtype([("total blocks", "<i4"), ("time", "<f8"), ("timestep", "<f8"), ("redshift", "<f8"), ("number of steps", "<i4"), ("nxb", "<i4"), ("nyb", "<i4"), ("nzb", "<i4")])
    b["simulation parameters"] = np.array([(n, 0.25, 0.01, 0.0, 7, 4, 4, 4)], dtype=sp)

# ------------------------------------------------------------------ FLASH 2, 2-D
nb = 8
blocks = [([0, 0, 0], [1.0, 1.0, 0], 1, 2)]
for k in range(4):
    o = [0.5 * (k & 1), 0.5 * ((k >> 1) & 1), 0]
    blocks.append((o, [o[0] + 0.5, o[1] + 0.5, 0], 2, 2 if k == 3 else 1))
for k in range(4):
    o = [0.5 + 0.25 * (k & 1), 0.5 + 0.25 * ((k >> 1) & 1), 0]
    blocks.append((o, [o[0] + 0.25, o[1] + 0.25, 0], 3, 1))
n = len(blocks)
p = os.path.join(out, "flash2d_hdf5_chk_0003")
with h5py.File(p, "w") as f:
    f["file format version"] = np.array([7], dtype="<i4")
    sp = np.dtype([("total blocks", "<i4"), ("time", "<f8"), ("timestep", "<f8"), ("redshift", "<f8"), ("number of steps", "<i4"), ("nxb", "<i4"), ("nyb", "<i4"), ("nzb", "<i4")])
    f["simulation parameters"] = np.array([(n, 1.5, 0.1, 0.0, 3, nb, nb, 1)], dtype=sp)
    f["unknown names"] = np.array([[b"dens"], [b"temp"]], dtype="S4")
    f["refine level"] = np.array([b[2] for b in blocks], dtype="<i4")
    f["node type"] = np.array([b[3] for b in blocks], dtype="<i4")
    f["bounding box"] = np.array([[[b[0][d], b[1][d]] for d in range(2)] for b in blocks], dtype="<f8")
    f["coordinates"] = np.array([[(b[0][d] + b[1][d]) / 2 for d in range(2)] for b in blocks], dtype="<f8")
    gid = -np.ones((n, 9), dtype="<i4")
    f["gid"] = gid
    for v in ("dens", "temp"):
        data = np.zeros((n, 1, nb, nb))
        for i, b in enumerate(blocks):
            x, y, z = cells(b[0], [b[1][0], b[1][1], 1.0], (nb, nb, 1))
            data[i] = field(v, x, y, 0 * z)
        f[v] = data

# ------------------------------------------------------------------ Enzo, 3-D
d = os.path.join(out, "enzo", "DD0003")
os.makedirs(d, exist_ok=True)
grids = [  # (left, right, cells, parent index (0 = root level), level)
    ([0, 0, 0], [1, 1, 1], [8, 8, 8], 0, 0),
    ([0.25, 0.25, 0.25], [0.75, 0.5, 0.5], [8, 4, 4], 1, 1),
    ([0.5, 0.625, 0.5], [0.875, 0.875, 0.75], [6, 4, 4], 1, 1),
    ([0.375, 0.3125, 0.3125], [0.625, 0.4375, 0.4375], [8, 4, 4], 2, 2),
]
with open(os.path.join(d, "data0003"), "w") as f:
    f.write("InitialCycleNumber  = 3\nInitialTime         = 1.5\nTopGridRank         = 3\nTopGridDimensions   = 8 8 8\n"
            "DataLabel[0]              = Density\nDataLabel[1]              = TotalEnergy\n"
            "#DataCGSConversionFactor[0] = 2.500000e+00\n#DataCGSConversionFactor[1] = 1.000000e+00\n")
with open(os.path.join(d, "data0003.boundary"), "w") as f:
    f.write("BoundaryRank         = 3\n")
# Hierarchy in Enzo's order (WriteDataHierarchy): a grid and its pointer to the next grid of
# its level, that grid (recursively), then the pointer to its first subgrid and the subgrids.
nxt_this = {1: 0, 2: 3, 3: 0, 4: 0}
nxt_next = {1: 2, 2: 4, 3: 0, 4: 0}
def grid_text(i):
    l, r, c, _, _ = grids[i - 1]
    g = 3
    return (f"Grid = {i}\nTask              = 0\nGridRank          = 3\nGridDimension     = {' '.join(str(x + 2 * g) for x in c)}\n"
            f"GridStartIndex    = {g} {g} {g}\nGridEndIndex      = {' '.join(str(g + x - 1) for x in c)}\n"
            f"GridLeftEdge      = {' '.join(repr(float(x)) for x in l)}\nGridRightEdge     = {' '.join(repr(float(x)) for x in r)}\n"
            f"Time              = 1.5\nSubgridsAreStatic = 0\nNumberOfBaryonFields = 2\nFieldType = 0 1\n"
            f"BaryonFileName = ./DD0003/data0003.cpu0000\nCourantSafetyNumber    = 0.300000\n"
            f"NumberOfParticles   = 0\nGravityBoundaryType = 0\nPointer: Grid[{i}]->NextGridThisLevel = {nxt_this[i]}\n\n")
def write(i):
    t = grid_text(i)
    if nxt_this[i]:
        t += write(nxt_this[i])
    t += f"Pointer: Grid[{i}]->NextGridNextLevel = {nxt_next[i]}\n\n"
    if nxt_next[i]:
        t += write(nxt_next[i])
    return t
txt = write(1)
with open(os.path.join(d, "data0003.hierarchy"), "w") as f:
    f.write(txt)
with h5py.File(os.path.join(d, "data0003.cpu0000"), "w") as f:
    for i, (l, r, c, _, _) in enumerate(grids, start=1):
        g = f.create_group(f"Grid{i:08d}")
        x, y, z = cells(l, r, c)
        for v in ("Density", "TotalEnergy"):
            g[v] = field(v, x, y, z).astype("<f4")
for root, _, files in os.walk(out):
    for fn in sorted(files):
        if "flash" in fn or "enzo" in root:
            print(os.path.join(root, fn), os.path.getsize(os.path.join(root, fn)))

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
flash3d_hdf5_part_0007, flash3d_ffv7_hdf5_part_0007   the 3-D FLASH grid with 40 tracer
    particles (FLASH 3: "particle names" + "tracer particles" table; FLASH 2: a compound
    "particle tracers" dataset).
enzo/DD0004                 the Enzo dump with particles in grids 1 and 4 (positions, velocity,
    mass, index, type).
chombo3d.hdf5               Chombo (cell-centered): two levels, three boxes, components density
    and energy (functions of x, y, z).
chombo2d.hdf5               Chombo 2-D: three levels, one ghost cell around every box
    (outputGhost), prob_lo and vec_dx as compound attributes.
chombo3d_node.hdf5          Chombo 3-D node-centered (data_centering 7): values at the nodes.
chombo4d.hdf5               Chombo 4-D (x, y, vpar, mu): one level, five boxes, components f, g.
chombo2d_particles.hdf5     Chombo 2-D with 30 particles (/particles: position_x, position_y,
    mass, particle_id).
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
# ------------------------------------------------------------------ Chombo
box3 = np.dtype([("lo_i", "<i4"), ("lo_j", "<i4"), ("lo_k", "<i4"), ("hi_i", "<i4"), ("hi_j", "<i4"), ("hi_k", "<i4")])
box2 = np.dtype([("lo_i", "<i4"), ("lo_j", "<i4"), ("hi_i", "<i4"), ("hi_j", "<i4")])
iv3 = np.dtype([("intvecti", "<i4"), ("intvectj", "<i4"), ("intvectk", "<i4")])
iv2 = np.dtype([("intvecti", "<i4"), ("intvectj", "<i4")])
dv2 = np.dtype([("x", "<f8"), ("y", "<f8")])

def chombo(path, dim, levels, ghost, prob_lo=None, vec_dx=False, node=False, particles=None):
    """levels: [(dx, [(lo, hi), ...]), ...] with lo/hi cell indices of length dim."""
    comps = [("density", lambda x, y, z: 1 + x + 2 * y + 3 * z), ("energy", lambda x, y, z: x * y - z + 0.5)]
    lo0 = prob_lo or [0.0] * dim
    with h5py.File(path, "w") as f:
        f.attrs["num_levels"] = np.int32(len(levels))
        f.attrs["max_level"] = np.int32(len(levels) - 1)
        f.attrs["num_components"] = np.int32(len(comps))
        for i, (n, _) in enumerate(comps):
            f.attrs[f"component_{i}"] = np.bytes_(n)
        f.attrs["time"] = 0.75
        f.attrs["iteration"] = np.int32(12)
        if node:
            f.attrs["data_centering"] = np.int32(7)
        if particles is not None:
            pg = f.create_group("particles")
            for k, v in particles.items():
                pg[k] = v
        if prob_lo:
            f.attrs["prob_lo"] = np.array(tuple(prob_lo), dtype=dv2)
        g = f.create_group("Chombo_global")
        g.attrs["SpaceDim"] = np.int32(dim)
        g.attrs["testReal"] = 0.0
        bt, it = (box3, iv3) if dim == 3 else (box2, iv2)
        n0 = None
        for l, (dx, boxes) in enumerate(levels):
            L = f.create_group(f"level_{l}")
            if vec_dx:
                L.attrs["vec_dx"] = np.array((dx, dx), dtype=dv2)
            else:
                L.attrs["dx"] = dx
            L.attrs["dt"] = 0.01
            L.attrs["time"] = 0.75
            L.attrs["ref_ratio"] = np.int32(2)
            n = int(round(1.0 / dx)) - 1
            L.attrs["prob_domain"] = np.array(tuple([0] * dim + [n] * dim), dtype=bt)
            L["boxes"] = np.array([tuple(lo) + tuple(hi) for lo, hi in boxes], dtype=bt)
            data, offs = [], [0]
            for lo, hi in boxes:
                # Cell centers, or the nodes (one more per axis) of node-centered data.
                c = [lo0[k] + (np.arange(lo[k] - ghost, hi[k] + ghost + 1 + (1 if node else 0)) + (0.0 if node else 0.5)) * dx for k in range(dim)]
                if dim == 2:
                    y, x = np.meshgrid(c[1], c[0], indexing="ij")
                    z = 0 * x
                else:
                    z, y, x = np.meshgrid(c[2], c[1], c[0], indexing="ij")
                for _, fn in comps:
                    data.append(fn(x, y, z).ravel())
                offs.append(offs[-1] + len(comps) * x.size)
            L["data:datatype=0"] = np.concatenate(data)
            L["data:offsets=0"] = np.array(offs, dtype="<i8")
            da = L.create_group("data_attributes")
            da.attrs["comps"] = np.int32(len(comps))
            da.attrs["objectType"] = np.bytes_("FArrayBox")
            da.attrs["outputGhost"] = np.array(tuple([ghost] * dim), dtype=it)
            da.attrs["ghost"] = np.array(tuple([ghost] * dim), dtype=it)

chombo(os.path.join(out, "chombo3d.hdf5"), 3, [(0.125, [((0, 0, 0), (7, 7, 7))]), (0.0625, [((4, 4, 4), (11, 9, 7)), ((10, 10, 8), (13, 13, 11))])], 0)
chombo(os.path.join(out, "chombo3d_node.hdf5"), 3, [(0.125, [((0, 0, 0), (7, 7, 7))]), (0.0625, [((4, 4, 4), (11, 9, 7))])], 0, node=True)
pr = np.random.default_rng(11)
pp = pr.random((30, 2))
chombo(os.path.join(out, "chombo2d_particles.hdf5"), 2, [(0.125, [((0, 0), (7, 7))]), (0.0625, [((2, 2), (9, 7))])], 0,
       particles={"position_x": pp[:, 0], "position_y": pp[:, 1], "mass": 1 + pr.random(30), "particle_id": np.arange(30, dtype="<i4")})
chombo(os.path.join(out, "chombo2d.hdf5"), 2,
       [(0.125, [((0, 0), (7, 7))]), (0.0625, [((2, 2), (9, 7)), ((8, 10), (13, 15))]), (0.03125, [((6, 6), (13, 11))])], 1, prob_lo=[-0.5, 0.25], vec_dx=True)

# 4-D Chombo (two configuration and two velocity axes, as gyrokinetic codes write it): one
# level, five boxes; two (i, j) boxes each split in velocity, the second not covering k = 3.
box4 = np.dtype([(n, "<i4") for n in ("lo_i", "lo_j", "lo_k", "lo_l", "hi_i", "hi_j", "hi_k", "hi_l")])
iv4 = np.dtype([(n, "<i4") for n in ("intvecti", "intvectj", "intvectk", "intvectl")])
dv4 = np.dtype([(n, "<f8") for n in ("x", "y", "z", "u")])
dx4 = (0.25, 0.5, 0.1, 0.2)
comps4 = [("f", lambda x, y, v, m: 1 + x + 10 * y + 100 * v + 1000 * m), ("g", lambda x, y, v, m: x * y - v + m * m)]
boxes4 = [((0, 0, 0, 0), (3, 5, 1, 2)), ((0, 0, 2, 0), (3, 5, 3, 2)),
          ((4, 0, 0, 0), (7, 5, 1, 1)), ((4, 0, 0, 2), (7, 5, 2, 2)), ((4, 0, 2, 0), (7, 5, 2, 1))]
with h5py.File(os.path.join(out, "chombo4d.hdf5"), "w") as f:
    f.attrs["num_levels"] = np.int32(1)
    f.attrs["max_level"] = np.int32(0)
    f.attrs["num_components"] = np.int32(len(comps4))
    for i, (n, _) in enumerate(comps4):
        f.attrs[f"component_{i}"] = np.bytes_(n)
    f.attrs["time"] = 2.5
    f.attrs["iteration"] = np.int32(3)
    g = f.create_group("Chombo_global")
    g.attrs["SpaceDim"] = np.int32(4)
    g.attrs["testReal"] = 0.0
    L = f.create_group("level_0")
    L.attrs["vec_dx"] = np.array(dx4, dtype=dv4)
    L.attrs["dt"] = 0.01
    L.attrs["time"] = 2.5
    L.attrs["ref_ratio"] = np.int32(2)
    L.attrs["prob_domain"] = np.array((0, 0, 0, 0, 7, 5, 3, 2), dtype=box4)
    L["boxes"] = np.array([tuple(lo) + tuple(hi) for lo, hi in boxes4], dtype=box4)
    data, offs = [], [0]
    for lo, hi in boxes4:
        c = [(np.arange(lo[k], hi[k] + 1) + 0.5) * dx4[k] for k in range(4)]
        m, v, y, x = np.meshgrid(c[3], c[2], c[1], c[0], indexing="ij")
        for _, fn in comps4:
            data.append(fn(x, y, v, m).ravel())
        offs.append(offs[-1] + len(comps4) * x.size)
    L["data:datatype=0"] = np.concatenate(data)
    L["data:offsets=0"] = np.array(offs, dtype="<i8")
    da = L.create_group("data_attributes")
    da.attrs["comps"] = np.int32(len(comps4))
    da.attrs["objectType"] = np.bytes_("FArrayBox")
    da.attrs["outputGhost"] = np.array((0, 0, 0, 0), dtype=iv4)
    da.attrs["ghost"] = np.array((0, 0, 0, 0), dtype=iv4)

# ------------------------------------------------------------------ particles
rng = np.random.default_rng(7)
npart = 40
pos = rng.random((npart, 3)) * [1.0, 0.5, 0.5]
vel = rng.random(npart) - 0.5
mass = 1.0 + rng.random(npart)
# FLASH 3: "particle names" (24-character names) and "tracer particles" (a column per name).
with h5py.File(os.path.join(out, "flash3d_hdf5_plt_cnt_0007"), "r") as a, h5py.File(os.path.join(out, "flash3d_hdf5_part_0007"), "w") as b:
    for k in a.keys():
        b[k] = a[k][()]
    names = ["tag", "posx", "posy", "posz", "velx", "mass"]
    # As FLASH writes them: 24-character space-padded names in a 24-byte C string type (no NUL),
    # which VTK reads without conversion.
    tid = h5py.h5t.C_S1.copy()
    tid.set_size(24)
    tid.set_strpad(h5py.h5t.STR_NULLTERM)
    ds = b.create_dataset("particle names", (len(names), 1), dtype=h5py.Datatype(tid))
    ds.id.write(h5py.h5s.ALL, h5py.h5s.ALL, np.array([[n.ljust(24).encode()] for n in names], dtype="S24"), mtype=tid)
    b["tracer particles"] = np.column_stack([np.arange(1, npart + 1), pos[:, 0], pos[:, 1], pos[:, 2], vel, mass]).astype("<f8")
# FLASH 2: a compound "particle tracers" dataset.
with h5py.File(os.path.join(out, "flash3d_ffv7_hdf5_chk_0007"), "r") as a, h5py.File(os.path.join(out, "flash3d_ffv7_hdf5_part_0007"), "w") as b:
    for k in a.keys():
        b[k] = a[k][()]
    pt = np.dtype([("particle_x", "<f8"), ("particle_y", "<f8"), ("particle_z", "<f8"), ("particle_velx", "<f8"), ("mass", "<f8"), ("tag", "<i4")])
    b["particle tracers"] = np.array([(pos[i, 0], pos[i, 1], pos[i, 2], vel[i], mass[i], i + 1) for i in range(npart)], dtype=pt)
# Enzo with particles: the DD0003 grids, particles in grids 1 and 4.
d4 = os.path.join(out, "enzo", "DD0004")
os.makedirs(d4, exist_ok=True)
for src, dst in (("data0003", "data0004"), ("data0003.boundary", "data0004.boundary")):
    open(os.path.join(d4, dst), "w").write(open(os.path.join(d, src)).read())
h = open(os.path.join(d, "data0003.hierarchy")).read().replace("DD0003/data0003", "DD0004/data0004")
counts = {1: 25, 4: 6}
parts = h.split("Grid = ")
for i in counts:
    for k, p in enumerate(parts):
        if p.startswith(f"{i}\n"):
            parts[k] = p.replace("NumberOfParticles   = 0", f"NumberOfParticles   = {counts[i]}\nParticleFileName = ./DD0004/data0004.cpu0000")
open(os.path.join(d4, "data0004.hierarchy"), "w").write("Grid = ".join(parts))
with h5py.File(os.path.join(d, "data0003.cpu0000"), "r") as a, h5py.File(os.path.join(d4, "data0004.cpu0000"), "w") as b:
    for g in a.keys():
        a.copy(g, b)
    for i, n in counts.items():
        l, r = grids[i - 1][0], grids[i - 1][1]
        p = np.array(l) + rng.random((n, 3)) * (np.array(r) - np.array(l))
        g = b[f"Grid{i:08d}"]
        for k, ax in enumerate("xyz"):
            g[f"particle_position_{ax}"] = p[:, k].astype("<f8")
        g["particle_velocity_x"] = (rng.random(n) - 0.5).astype("<f4")
        g["particle_mass"] = (1 + rng.random(n)).astype("<f8")
        g["particle_index"] = (np.arange(n) + 100 * i).astype("<i8")
        g["particle_type"] = np.array([1 + (k % 2) for k in range(n)], dtype="<i4")

for root, _, files in os.walk(out):
    for fn in sorted(files):
        if "flash" in fn or "enzo" in root or "chombo" in fn:
            print(os.path.join(root, fn), os.path.getsize(os.path.join(root, fn)))

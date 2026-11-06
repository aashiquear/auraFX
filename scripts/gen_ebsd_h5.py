"""EBSD HDF5 test files: testdata/ebsd/*.h5ebsd, *.h5 and testdata/dream3d/am_bench_subset.dream3d.

    pvpython scripts/gen_ebsd_h5.py [AM Bench .dream3d file]

Needs h5py (ParaView's Python has it). Run under a memory cap: (ulimit -v 8000000; timeout 600 pvpython ...).
Made from the text maps of scripts/gen_ebsd.py, in the layouts EbsdLib reads and writes:
- ni_stack.h5ebsd: a TSL stack of four 70 x 50 slices cut from ni_map.ang at shifted windows
  (window corners in ebsd_h5_expected.json), 0.5 um apart, with sample (180 degrees about y)
  and Euler (90 degrees about z) transformations recorded at the root.
- ti_stack.h5ebsd: an HKL stack of two copies of ti_map.ctf (angles in degrees, as stored).
- ni_map_edax.h5: an EDAX OIM file with two scans: "Scan 1" from ni_map.ang and "Scan 2"
  from two_phase.ang (angles in radians, IQ and CI columns).
- two_phase_bruker.h5: a Bruker Esprit file from two_phase.ang (angles in degrees, phases by
  space group: 225 and 194), its points written in a shuffled order with their X BEAM / Y BEAM.
- am_bench_subset.dream3d (only when the AM Bench file is given): a 48 x 40 x 6 window of
  NIST's AM Bench 2022 IN718 EBSD volume "9 - EBSD_2um_OrientationAnalysis_2deg.dream3d"
  (DREAM.3D 6.6 output), keeping its cell arrays, ensemble data and the features present in the
  window (renumbered from 1, feature 0 kept), with every HDF5 attribute DREAM.3D wrote.
"""
import json, os, sys
import numpy as np
import h5py

here = os.path.dirname(os.path.abspath(__file__))
td = os.path.join(here, "..", "testdata")
out = os.path.join(td, "ebsd")
vstr = h5py.string_dtype()


def read_ang(p):
    rows = [l.split() for l in open(p) if l.strip() and not l.startswith("#")]
    return np.array(rows, float)


def read_ctf(p):
    lines = open(p).read().splitlines()
    k = next(i for i, l in enumerate(lines) if l.startswith("Phase\tX"))
    return np.array([l.split("\t") for l in lines[k + 1:] if l.strip()], float)


def sds(g, name, value):
    g.create_dataset(name, data=np.array([value.encode()], dtype=object), dtype=vstr)


ni = read_ang(os.path.join(out, "ni_map.ang"))          # 80 x 60
two = read_ang(os.path.join(out, "two_phase.ang"))      # 60 x 40
ti = read_ctf(os.path.join(out, "ti_map.ctf"))          # 70 x 50
expected = {}

# ---------------------------------------------------------------- TSL .h5ebsd stack
corners = [(4, 4), (6, 3), (9, 4), (8, 6)]   # (column, row) of each slice's window in ni_map
nx, ny = 70, 50
with h5py.File(os.path.join(out, "ni_stack.h5ebsd"), "w") as f:
    f.attrs["FileVersion"] = np.int32(5)
    f.attrs["EbsdLibVersion"] = "auraFX test file"
    sds(f, "Manufacturer", "TSL")
    for k, v in [("ZStartIndex", 0), ("ZEndIndex", 3), ("Max X Points", nx), ("Max Y Points", ny), ("Stacking Order", 0)]:
        f.create_dataset(k, data=np.int64(v))
    for k, v in [("Z Resolution", 0.5), ("X Resolution", 0.5), ("Y Resolution", 0.5), ("SampleTransformationAngle", 180.0), ("EulerTransformationAngle", 90.0)]:
        f.create_dataset(k, data=np.float32(v))
    f.create_dataset("SampleTransformationAxis", data=np.array([0, 1, 0], np.float32))
    f.create_dataset("EulerTransformationAxis", data=np.array([0, 0, 1], np.float32))
    f.create_dataset("Index", data=np.array([str(z).encode() for z in range(4)], dtype=object), dtype=vstr)
    full = ni.reshape(60, 80, -1)
    for z, (c0, r0) in enumerate(corners):
        w = full[r0:r0 + ny, c0:c0 + nx].reshape(-1, ni.shape[1])
        h = f.create_group(f"{z}/Header")
        for k, v in [("XSTEP", 0.5), ("YSTEP", 0.5), ("TEM_PIXperUM", 1.0), ("x-star", 0.5), ("y-star", 0.5), ("z-star", 0.5), ("WorkingDistance", 15.0)]:
            h.create_dataset(k, data=np.float32(v))
        for k, v in [("NCOLS_ODD", nx), ("NCOLS_EVEN", nx), ("NROWS", ny)]:
            h.create_dataset(k, data=np.int32(v))
        for k, v in [("GRID", "SqrGrid"), ("OPERATOR", "auraFX"), ("SAMPLEID", ""), ("SCANID", ""), ("OriginalFile", "ni_map.ang")]:
            sds(h, k, v)
        p = h.create_group("Phases/1")
        p.create_dataset("Phase", data=np.int32(1))
        sds(p, "MaterialName", "Nickel")
        sds(p, "Formula", "Ni")
        p.create_dataset("Symmetry", data=np.int32(43))
        p.create_dataset("LatticeConstants", data=np.array([3.524, 3.524, 3.524, 90, 90, 90], np.float32))
        p.create_dataset("NumberFamilies", data=np.int32(0))
        d = f.create_group(f"{z}/Data")
        x = (w[:, 3] - w[0, 3]).astype(np.float32)
        y = (w[:, 4] - w[0, 4]).astype(np.float32)
        for name, col in [("Phi1", 0), ("Phi", 1), ("Phi2", 2), ("Image Quality", 5), ("Confidence Index", 6), ("SEM Signal", 8), ("Fit", 9)]:
            d.create_dataset(name, data=w[:, col].astype(np.float32))
        d.create_dataset("X Position", data=x)
        d.create_dataset("Y Position", data=y)
        d.create_dataset("PhaseData", data=w[:, 7].astype(np.int32))
expected["ni_stack.h5ebsd"] = {"corners": corners, "nx": nx, "ny": ny}

# ---------------------------------------------------------------- HKL .h5ebsd stack
with h5py.File(os.path.join(out, "ti_stack.h5ebsd"), "w") as f:
    f.attrs["FileVersion"] = np.int32(5)
    sds(f, "Manufacturer", "HKL")
    for k, v in [("ZStartIndex", 1), ("ZEndIndex", 2), ("Max X Points", 70), ("Max Y Points", 50), ("Stacking Order", 1)]:
        f.create_dataset(k, data=np.int64(v))
    for k, v in [("Z Resolution", 0.4), ("X Resolution", 0.4), ("Y Resolution", 0.4)]:
        f.create_dataset(k, data=np.float32(v))
    for z in (1, 2):
        h = f.create_group(f"{z}/Header")
        for k, v in [("XCells", 70), ("YCells", 50), ("Mag", 1000), ("KV", 20)]:
            h.create_dataset(k, data=np.int32(v))
        for k, v in [("XStep", 0.4), ("YStep", 0.4), ("TiltAngle", 70.0)]:
            h.create_dataset(k, data=np.float32(v))
        for k, v in [("Prj", "auraFX test map"), ("JobMode", "Grid"), ("OriginalFile", "ti_map.ctf")]:
            sds(h, k, v)
        p = h.create_group("Phases/1")
        p.create_dataset("LatticeConstants", data=np.array([2.95, 2.95, 4.68, 90, 90, 120], np.float32))
        sds(p, "PhaseName", "Titanium")
        lg = p.create_dataset("LaueGroup", data=np.int32(9))
        lg.attrs["Name"] = "Hexagonal-High 6/mmm"
        p.create_dataset("SpaceGroup", data=np.int32(194))
        d = f.create_group(f"{z}/Data")
        names = ["Phase", "X", "Y", "Bands", "Error", "Euler1", "Euler2", "Euler3", "MAD", "BC", "BS"]
        for k, name in enumerate(names):
            ints = name in ("Phase", "Bands", "Error", "BC", "BS")
            d.create_dataset(name, data=ti[:, k].astype(np.int32 if ints else np.float32))

# ---------------------------------------------------------------- EDAX OIM .h5
def edax_scan(f, scan, a, nx, ny, step, phases):
    g = f.create_group(f"{scan}/EBSD")
    h = g.create_group("Header")
    h.create_dataset("Step X", data=np.float32(step))
    h.create_dataset("Step Y", data=np.float32(step))
    h.create_dataset("nColumns", data=np.int32(nx))
    h.create_dataset("nRows", data=np.int32(ny))
    sds(h, "Grid Type", "SqrGrid")
    pc = h.create_group("Pattern Center Calibration")
    for k in ("x-star", "y-star", "z-star"):
        pc.create_dataset(k, data=np.float32(0.5))
    for n, (name, sym, lat) in enumerate(phases, 1):
        p = h.create_group(f"Phase/{n}")
        sds(p, "MaterialName", name)
        sds(p, "Formula", "")
        p.create_dataset("Symmetry", data=np.int32(sym))
        p.create_dataset("NumberFamilies", data=np.int32(0))
        for k, v in zip(["a", "b", "c", "alpha", "beta", "gamma"], lat):
            p.create_dataset(f"Lattice Constant {k}", data=np.float32(v))
    d = g.create_group("Data")
    for name, col in [("Phi1", 0), ("Phi", 1), ("Phi2", 2), ("X Position", 3), ("Y Position", 4), ("IQ", 5), ("CI", 6), ("SEM Signal", 8), ("Fit", 9)]:
        d.create_dataset(name, data=a[:, col].astype(np.float32))
    d.create_dataset("Phase", data=a[:, 7].astype(np.int32))


with h5py.File(os.path.join(out, "ni_map_edax.h5"), "w") as f:
    sds(f, "Manufacturer", "EDAX")
    sds(f, " Version", "OIM Analysis 8.6.0 x64")
    edax_scan(f, "Scan 1", ni, 80, 60, 0.5, [("Nickel", 43, [3.524, 3.524, 3.524, 90, 90, 90])])
    edax_scan(f, "Scan 2", two, 60, 40, 2.0, [("Iron (Gamma)", 43, [3.6, 3.6, 3.6, 90, 90, 90]), ("Titanium (Alpha)", 62, [2.95, 2.95, 4.68, 90, 90, 120])])

# ---------------------------------------------------------------- Bruker Esprit .h5
rng = np.random.default_rng(3)
perm = rng.permutation(len(two))
b = two[perm]
with h5py.File(os.path.join(out, "two_phase_bruker.h5"), "w") as f:
    sds(f, "Manufacturer", "Bruker Nano")
    sds(f, "Version", "Esprit 2.3")
    g = f.create_group("Scan 0/EBSD")
    h = g.create_group("Header")
    for k, v in [("NCOLS", 60), ("NROWS", 40), ("NPoints", 2400)]:
        h.create_dataset(k, data=np.int32(v))
    for k, v in [("XSTEP", 2.0), ("YSTEP", 2.0), ("ZOffset", 0.0)]:
        h.create_dataset(k, data=np.float64(v))
    sds(h, "Grid Type", "isometric")
    for n, (name, it, lat) in enumerate([("Iron (Gamma)", 225, [3.6, 3.6, 3.6, 90, 90, 90]), ("Titanium (Alpha)", 194, [2.95, 2.95, 4.68, 90, 90, 120])], 1):
        p = h.create_group(f"Phases/{n}")
        sds(p, "Name", name)
        sds(p, "Formula", "")
        p.create_dataset("IT", data=np.int32(it))
        p.create_dataset("Setting", data=np.int32(1))
        p.create_dataset("LatticeConstants", data=np.array(lat, np.float32))
    d = g.create_group("Data")
    for name, col in [("phi1", 0), ("PHI", 1), ("phi2", 2)]:
        d.create_dataset(name, data=np.degrees(b[:, col]).astype(np.float32))
    d.create_dataset("Phase", data=b[:, 7].astype(np.int32))
    d.create_dataset("X BEAM", data=np.rint(b[:, 3] / 2.0).astype(np.int32))
    d.create_dataset("Y BEAM", data=np.rint(b[:, 4] / 2.0).astype(np.int32))
    d.create_dataset("MAD", data=b[:, 9].astype(np.float32))
    d.create_dataset("RadonQuality", data=b[:, 5].astype(np.float32))
    d.create_dataset("NIndexedBands", data=np.where(b[:, 7] > 0, 9, 0).astype(np.int32))

with open(os.path.join(out, "ebsd_h5_expected.json"), "w") as fp:
    json.dump(expected, fp)

# ---------------------------------------------------------------- .dream3d window
if len(sys.argv) > 1:
    src = h5py.File(sys.argv[1], "r")
    dc = src["DataContainers/EBSDDataContainer"]
    sv = dc["CellData/SampleVoxels"]
    z0, nz, wy, wx = 260, 6, 40, 48
    sl = sv[z0, :, :, 0].astype(np.int64)
    c = np.pad(sl.cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    win = c[wy:, wx:] - c[:-wy, wx:] - c[wy:, :-wx] + c[:-wy, :-wx]
    y0, x0 = np.unravel_index(win.argmax(), win.shape)
    print("window", z0, y0, x0, "fraction in sample", win.max() / (wx * wy))
    os.makedirs(os.path.join(td, "dream3d"), exist_ok=True)
    with h5py.File(os.path.join(td, "dream3d", "am_bench_subset.dream3d"), "w") as f:
        for k, v in src.attrs.items():
            f.attrs[k] = v
        f.create_group("DataContainerBundles")
        o = f.create_group("DataContainers/EBSDDataContainer")
        g = o.create_group("_SIMPL_GEOMETRY")
        for k, v in dc["_SIMPL_GEOMETRY"].attrs.items():
            g.attrs[k] = v
        origin = dc["_SIMPL_GEOMETRY/ORIGIN"][()]
        spacing = dc["_SIMPL_GEOMETRY/SPACING"][()]
        g.create_dataset("DIMENSIONS", data=np.array([wx, wy, nz], np.int64))
        g.create_dataset("ORIGIN", data=(origin + spacing * np.array([x0, y0, z0])).astype(np.float32))
        g.create_dataset("SPACING", data=spacing)

        def copy_attrs(dst, srcobj, tuples):
            for k, v in srcobj.attrs.items():
                dst.attrs[k] = v
            if tuples is not None:
                dst.attrs["TupleDimensions"] = np.array(tuples, np.uint64)
                if "Tuple Axis Dimensions" in srcobj.attrs:
                    names = "xyz"
                    dst.attrs["Tuple Axis Dimensions"] = ",".join(f"{names[i]}={t}" for i, t in enumerate(tuples)).encode()

        cell = o.create_group("CellData")
        copy_attrs(cell, dc["CellData"], [wx, wy, nz])
        fid = dc["CellData/FeatureIds"][z0:z0 + nz, y0:y0 + wy, x0:x0 + wx]
        present = np.unique(fid)
        present = present[present > 0]
        newid = np.zeros(int(fid.max()) + 1, np.int32)
        newid[present] = np.arange(1, len(present) + 1)
        for name in ["EulerAngles", "Phases", "FeatureIds", "SampleVoxels", "IPFColorZ", "IPFColorX", "KernelAverageMisorientations"]:
            a = dc["CellData/" + name][z0:z0 + nz, y0:y0 + wy, x0:x0 + wx]
            if name == "FeatureIds":
                a = newid[a]
            d = cell.create_dataset(name, data=a)
            copy_attrs(d, dc["CellData/" + name], [wx, wy, nz])
        ens = o.create_group("CellEnsembleData")
        copy_attrs(ens, dc["CellEnsembleData"], None)
        for name, d in dc["CellEnsembleData"].items():
            e = ens.create_dataset(name, data=d[()], dtype=d.dtype)
            copy_attrs(e, d, None)
        feat = o.create_group("CellFeatureData")
        copy_attrs(feat, dc["CellFeatureData"], [len(present) + 1])
        rows = np.r_[0, present]
        for name, d in dc["CellFeatureData"].items():
            if d.attrs.get("ObjectType", b"").startswith(b"NeighborList"):
                continue
            e = feat.create_dataset(name, data=np.stack([d[int(r)] for r in rows]))
            copy_attrs(e, d, [len(rows)])
    print("features in the window", len(present))

for n in sorted(os.listdir(out)):
    print(n, os.path.getsize(os.path.join(out, n)))

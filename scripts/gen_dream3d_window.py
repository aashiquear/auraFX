"""Cut a window out of a large .dream3d image volume, keeping its features: a small test file.

    pvpython scripts/gen_dream3d_window.py SOURCE OUT x0 y0 z0 nx ny nz [arrays]

Needs h5py (ParaView's Python has it); run under a memory cap:
(ulimit -v 8000000; timeout 600 pvpython ...). The window is cells x0..x0+nx-1 (and so on) of the
first image data container. Cell arrays listed in `arrays` (comma-separated; default: all) are
kept; feature arrays keep the rows of the features present, renumbered from 1 in the order of
their old ids (row 0 kept), and FeatureIds is renumbered alike. The window's original ids are
kept as the feature array OriginalFeatureIds. Ensemble arrays are copied. Every attribute
DREAM.3D wrote is kept, with the tuple dimensions updated.

testdata/dream3d/am_bench_features.dream3d was made with
    SOURCE = "9 - EBSD_1um_OrientationAnalysis_2deg.dream3d" (NIST AM Bench 2022 IN718, the
    1 um serial-section EBSD volume segmented by DREAM.3D 6.6: misorientation 2 degrees within
    SampleVoxels, Fill Bad Data 7, Minimum Size 7, then sizes, centroids, shapes, average
    orientations, reference misorientations, surface features and kernel average
    misorientations with kernel 2 2 1)
    window 181 135 262 64 64 20,
    arrays EulerAngles,Phases,SampleVoxels,FeatureIds,KernelAverageMisorientations,FeatureReferenceMisorientations
"""
import os, sys
import numpy as np
import h5py

src, out = sys.argv[1], sys.argv[2]
x0, y0, z0, nx, ny, nz = map(int, sys.argv[3:9])
keep = sys.argv[9].split(",") if len(sys.argv) > 9 else None


def copy_attrs(dst, so, tuples):
    for k, v in so.attrs.items():
        dst.attrs[k] = v
    if tuples is not None:
        dst.attrs["TupleDimensions"] = np.array(tuples, np.uint64)
        if "Tuple Axis Dimensions" in so.attrs:
            dst.attrs["Tuple Axis Dimensions"] = ",".join(f"{'xyz'[i]}={t}" for i, t in enumerate(tuples)).encode()


s = h5py.File(src, "r")
dcs = s["DataContainers"]
name = next(n for n in dcs if "_SIMPL_GEOMETRY" in dcs[n] and dcs[n]["_SIMPL_GEOMETRY"].attrs.get("GeometryTypeName", b"") == b"ImageGeometry")
dc = dcs[name]
os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
with h5py.File(out, "w") as f:
    for k, v in s.attrs.items():
        f.attrs[k] = v
    f.create_group("DataContainerBundles")
    o = f.create_group(f"DataContainers/{name}")
    g = o.create_group("_SIMPL_GEOMETRY")
    copy_attrs(g, dc["_SIMPL_GEOMETRY"], None)
    origin = dc["_SIMPL_GEOMETRY/ORIGIN"][()]
    spacing = dc["_SIMPL_GEOMETRY/SPACING"][()]
    g.create_dataset("DIMENSIONS", data=np.array([nx, ny, nz], np.int64))
    g.create_dataset("ORIGIN", data=(origin + spacing * np.array([x0, y0, z0])).astype(np.float32))
    g.create_dataset("SPACING", data=spacing)
    cell_name = next(n for n in dc if dc[n].attrs.get("AttributeMatrixType", [0])[0] == 3)
    feat_name = next((n for n in dc if dc[n].attrs.get("AttributeMatrixType", [0])[0] == 7), None)
    ens_name = next((n for n in dc if dc[n].attrs.get("AttributeMatrixType", [0])[0] == 11), None)
    cell = o.create_group(cell_name)
    copy_attrs(cell, dc[cell_name], [nx, ny, nz])
    fid = dc[cell_name]["FeatureIds"][z0:z0 + nz, y0:y0 + ny, x0:x0 + nx]
    present = np.unique(fid)
    present = present[present > 0]
    newid = np.zeros(int(fid.max()) + 1, np.int32)
    newid[present] = np.arange(1, len(present) + 1)
    for an, d in dc[cell_name].items():
        if keep and an not in keep:
            continue
        a = d[z0:z0 + nz, y0:y0 + ny, x0:x0 + nx]
        if an == "FeatureIds":
            a = newid[a]
        e = cell.create_dataset(an, data=a)
        copy_attrs(e, d, [nx, ny, nz])
    if ens_name:
        ens = o.create_group(ens_name)
        copy_attrs(ens, dc[ens_name], None)
        for an, d in dc[ens_name].items():
            e = ens.create_dataset(an, data=d[()], dtype=d.dtype)
            copy_attrs(e, d, None)
    if feat_name:
        feat = o.create_group(feat_name)
        rows = np.r_[0, present]
        copy_attrs(feat, dc[feat_name], [len(rows)])
        for an, d in dc[feat_name].items():
            if d.attrs.get("ObjectType", b"").startswith(b"NeighborList"):
                continue
            e = feat.create_dataset(an, data=np.stack([d[int(r)] for r in rows]))
            copy_attrs(e, d, [len(rows)])
        e = feat.create_dataset("OriginalFeatureIds", data=rows.astype(np.int32).reshape(-1, 1))
        copy_attrs(e, dc[feat_name]["NumElements"], [len(rows)])
print(out, os.path.getsize(out), "features", len(present))

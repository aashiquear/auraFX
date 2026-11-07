"""SROD and GND reference values: testdata/crystal/am_reference.json.

    pvpython scripts/am_reference.py <AM-Orientation-Shift-TOOLKIT folder>

(pvpython for h5py; under `ulimit -v 8000000`.) The AM orientation toolkit's
am_ebsd3d_analysis on testdata/dream3d/am_bench_features.dream3d (a 64 x 64 x 20 window of the
NIST AM Bench 2022 IN718 1 um volume with DREAM.3D's FeatureIds): `analyse` with the plate top
4 um above the window's bottom, bins of 4 um, grains of at least 200 um^3, reference volume
50 um^3 in at most 5 sections, bins of at least 20 um^3, interface 4 um; and `nye_gnd` (kernel
3, b = 0.254 nm). Grain g of the toolkit is feature g + 1 of the file.
"""
import json, os, sys
import numpy as np
import h5py

sys.path.insert(0, sys.argv[1])
import am_ebsd3d_analysis as A

here = os.path.dirname(os.path.abspath(__file__))
with h5py.File(os.path.join(here, "..", "testdata", "dream3d", "am_bench_features.dream3d"), "r") as f:
    dc = f["DataContainers/EBSDDataContainer"]
    cd = dc["CellData"]
    eul = cd["EulerAngles"][()].astype(np.float64)
    ph = cd["Phases"][()][..., 0]
    fid = cd["FeatureIds"][()][..., 0].astype(np.int64)
    sp = dc["_SIMPL_GEOMETRY/SPACING"][()].astype(float)
    org = dc["_SIMPL_GEOMETRY/ORIGIN"][()].astype(float)
nz = eul.shape[0]
mask = (ph > 0) & (fid > 0)
plate = 4.0
zc = (np.arange(nz) + 0.5) * sp[2] - plate
V = dict(euler=eul, mask=mask, zc=zc, spacing=(float(sp[0]), float(sp[1]), float(sp[2])), feature=np.where(mask, fid - 1, -1))
rules = dict(segmentation_deg=5.0, min_volume_um3=200.0, reference_volume_um3=50.0, reference_sections=5,
             min_bin_volume_um3=20.0, interface_um=4.0, bin_um=4.0)
R = A.analyse(V, rules, lambda *a: None)
gr, bins = R["grains"], R["bins"]
out = {
    "plate_top_above_bottom": plate, "origin": org.tolist(), "spacing": sp.tolist(), "rules": rules,
    "grains": {"feature": (gr["grain"] + 1).tolist(), "class": [str(c) for c in gr["cls"]], "base": gr["z_base_um"].tolist(),
               "top": gr["z_top_um"].tolist(), "n_bins": gr["n_bins"].tolist(), "first": gr["first_bin_srod_deg"].tolist(),
               "max": gr["max_bin_srod_deg"].tolist(), "rate": gr["rate_deg_per_bin"].tolist(), "voxel_mean": gr["voxel_srod_mean_deg"].tolist()},
    "bins": {"feature": (bins["grain"] + 1).tolist(), "bin": bins["bin"].tolist(), "srod": bins["srod_deg"].astype(float).tolist(),
             "n_vox": bins["n_vox"].tolist(), "vox_mean": bins["vox_mean"].astype(float).tolist(), "p50": bins["p50"].astype(float).tolist()},
}
srod = R["srod_field"].ravel()
keep = np.nonzero(np.isfinite(srod))[0][::29]
out["srod_voxels"] = {"index": keep.tolist(), "value": srod[keep].astype(float).tolist()}
q = A.euler_to_quat(eul.reshape(-1, 3)).reshape(eul.shape[:3] + (4,))
lab = np.where(mask, fid - 1, -1)
N = A.nye_gnd(q, lab, (float(sp[0]), float(sp[1]), float(sp[2])), burgers_m=2.54e-10, kernel=3, z_from=0, min_neighbours=6)
rho = N["rho_voxel_m2"].ravel()
idx = np.nonzero(np.isfinite(rho))[0][::19]
out["nye"] = {"mean": N["mean_m2"], "n": N["n_voxels"], "index": idx.tolist(), "value": rho[idx].astype(float).tolist(),
              "nan_count": int(np.isnan(rho).sum())}
def clean(x):
    """NaN as null (JSON has no NaN)."""
    if isinstance(x, float) and x != x:
        return None
    if isinstance(x, dict):
        return {k: clean(v) for k, v in x.items()}
    if isinstance(x, list):
        return [clean(v) for v in x]
    return x


dst = os.path.join(here, "..", "testdata", "crystal", "am_reference.json")
with open(dst, "w") as fh:
    json.dump(clean(out), fh)
print("wrote", dst, len(gr["grain"]), "grains", len(bins["bin"]), "bins; classes", {c: int((gr["cls"] == c).sum()) for c in set(gr["cls"])}, "Nye mean", N["mean_m2"], N["n_voxels"])

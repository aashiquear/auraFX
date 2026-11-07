"""Crystal-mechanics reference values: testdata/crystal/mechanics_reference.json.

    pvpython scripts/mechanics_reference.py <AM-Orientation-Shift-TOOLKIT folder>

(pvpython for h5py; under `ulimit -v 8000000`.) For the first 2000 indexed cells of
testdata/dream3d/am_bench_subset.dream3d, the toolkit's am_build_campaign gives the
full-constraint Taylor factor (taylor_factor), the largest Schmid factor (schmid_max) of the 12
FCC {111}<110> systems and the directional Young's modulus (youngs_modulus, C11 250, C12 160,
C44 120 GPa: its IN718 constants) along the build direction z and the transverse x.
"""
import json, os, sys
import numpy as np
import h5py

sys.path.insert(0, sys.argv[1])
import am_build_campaign as B
import am_texture as T

here = os.path.dirname(os.path.abspath(__file__))
with h5py.File(os.path.join(here, "..", "testdata", "dream3d", "am_bench_subset.dream3d"), "r") as f:
    cd = f["DataContainers/EBSDDataContainer/CellData"]
    eul = cd["EulerAngles"][()].astype(float).reshape(-1, 3)
    ph = cd["Phases"][()].reshape(-1)
e = eul[ph > 0][:2000]
g = T.euler_to_matrices(e)
systems = B.fcc_slip_systems()
out = {"euler": e.tolist()}
for name, axis in (("z", np.array([0.0, 0.0, 1.0])), ("x", np.array([1.0, 0.0, 0.0]))):
    out["taylor_" + name] = B.taylor_factor(g, axis, systems).tolist()
    out["schmid_" + name] = B.schmid_max(g, axis, systems).tolist()
    out["youngs_" + name] = B.youngs_modulus(g, axis, 250.0, 160.0, 120.0).tolist()
dst = os.path.join(here, "..", "testdata", "crystal", "mechanics_reference.json")
with open(dst, "w") as fh:
    json.dump(out, fh)
print("wrote", dst, len(e), "orientations; mean Taylor z", np.mean(out["taylor_z"]))

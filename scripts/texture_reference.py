"""Reference values for texture analysis: testdata/crystal/texture_reference.json.

    pvpython scripts/texture_reference.py <AM-Orientation-Shift-TOOLKIT folder>

(pvpython for h5py; run under `ulimit -v 8000000`.) The orientations are the indexed cells of
testdata/dream3d/am_bench_subset.dream3d (NIST AM Bench 2022 IN718, cubic m-3m). The toolkit's
am_texture and am_validation_metrics give:
- pole figures {100}, {110}, {111} on its 41 x 41 stereographic grid (kernel 7.5 deg, 2 deg bins);
- the ODF kernel means (before dividing by the uniform value) on the phi2 = 0, 45, 65 sections
  (10 deg steps), its Monte Carlo uniform value, and its texture index and ODF maximum;
- component fractions (15 deg cones);
- grain-boundary chord lengths (10 deg) of the first slice, along both map directions.
"""
import json, os, sys
import numpy as np
import h5py

tk = sys.argv[1]
sys.path.insert(0, tk)
import am_texture as T
import am_validation_metrics as V

here = os.path.dirname(os.path.abspath(__file__))
src = os.path.join(here, "..", "testdata", "dream3d", "am_bench_subset.dream3d")
with h5py.File(src, "r") as f:
    cd = f["DataContainers/EBSDDataContainer/CellData"]
    eul = cd["EulerAngles"][()].astype(float)        # (z, y, x, 3)
    ph = cd["Phases"][()][..., 0]
    geom = f["DataContainers/EBSDDataContainer/_SIMPL_GEOMETRY"]
    spacing = geom["SPACING"][()].astype(float)
ok = ph > 0
e = eul[ok]                                            # z-, y-, x-major order of the cells
g = T.euler_to_matrices(e)
X, Y, inside, D = T.stereo_grid(41)
pf = {name: T.pole_figure(g, hkl, D).ravel().tolist() for name, hkl in (("100", (1, 0, 0)), ("110", (1, 1, 0)), ("111", (1, 1, 1)))}
sections = {}
for s in (0.0, 45.0, 65.0):
    gg, p1, P = T.euler_grid(s, 10.0)
    qg = T.matrices_to_quaternions(gg).reshape(-1, 4)
    sections[str(int(s))] = T._kernel_mean(qg, g, np.radians(T.SIGMA_DEG)).tolist()
J, fmax = T.texture_index(g)
# Chords of slice 0: rows along y (the map's second direction), columns along x.
G0 = T.euler_to_matrices(eul[0].reshape(-1, 3)).reshape(eul.shape[1], eul.shape[2], 3, 3)
chords = V.chords_from_map(G0, ph[0] > 0, (float(spacing[0]), float(spacing[1])), threshold_deg=10.0, line_spacing_um=float(spacing[1]))
out = {
    "source": "testdata/dream3d/am_bench_subset.dream3d, indexed cells in file order",
    "n": int(len(e)),
    "sigma_deg": T.SIGMA_DEG,
    "stereo_grid": 41,
    "pole_figures": pf,
    "section_step_deg": 10.0,
    "odf_kernel_means": sections,
    "uniform_expectation_mc": T.uniform_odf_expectation(),
    "texture_index": J,
    "odf_max": fmax,
    "fractions": T.fractions(g),
    "chords_first": chords["first"].tolist(),
    "chords_second": chords["second"].tolist(),
    "spacing": spacing.tolist(),
}
dst = os.path.join(here, "..", "testdata", "crystal", "texture_reference.json")
with open(dst, "w") as fh:
    json.dump(out, fh)
print("wrote", dst, out["n"], "orientations; J", J, "max", fmax, "E", out["uniform_expectation_mc"], "chords", len(out["chords_first"]), len(out["chords_second"]))

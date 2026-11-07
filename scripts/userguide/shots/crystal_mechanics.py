# Crystal Mechanics on a measured volume: the AM Bench 2022 IN718 window
# (testdata/dream3d/am_bench_features.dream3d) colored by the full-constraint Taylor factor for
# loading along the build direction z (IN718's {111}<110> slip systems).
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_features.dream3d")
d.hide()
m = d.crystal_mechanics(material="IN718", direction=(0, 0, 1))
m.set(color_by="TaylorFactor", cmap="Viridis")
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

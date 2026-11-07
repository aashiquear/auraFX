# Pole figures of a measured texture: the {100}, {110} and {111} pole figures of the AM Bench
# 2022 IN718 window (testdata/dream3d/am_bench_subset.dream3d), stereographic, sample X right
# and Y up, colored by multiples of random.
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_subset.dream3d")
d.hide()
pf = d.pole_figure()
pf.set(color_by="MRD", cmap="Viridis")
afx.annotate("{100}            {110}            {111}", "upper center", 18)
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

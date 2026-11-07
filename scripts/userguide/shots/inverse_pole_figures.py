# Inverse pole figures of the AM Bench 2022 IN718 window
# (testdata/dream3d/am_bench_subset.dream3d) for sample X, Y and Z on the cubic standard
# triangle ([001] lower left, [101] lower right, [111] top), colored by multiples of random.
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_subset.dream3d")
d.hide()
i = d.inverse_pole_figure()
i.set(color_by="MRD", cmap="Viridis")
afx.annotate("X                         Y                         Z", "upper center", 18)
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

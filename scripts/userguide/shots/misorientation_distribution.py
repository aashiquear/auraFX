# Misorientation distribution of the AM Bench 2022 IN718 window
# (testdata/dream3d/am_bench_subset.dream3d) in the Charts panel: disorientation angles from 5
# degrees up between neighbouring points, between random pairs of points, and for random
# orientations (the uniform cubic distribution), as densities per degree.
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_subset.dream3d")
d.set(color_by="IPFColorZ")
m = d.misorientation_distribution(min_angle=5, bin=1)
x = m.array("bin_center")
afx.plot(x, m.array("Neighbours"), label="Neighbours", title="Misorientation distribution", xlabel="Disorientation angle (degrees)", ylabel="Density per degree", clear=True)
afx.plot(x, m.array("Random pairs"), label="Random pairs")
afx.plot(x, m.array("Uniform"), label="Uniform")
d.activate()
afx.view("iso"); afx.reset_camera()
afx.show_panel("Charts")

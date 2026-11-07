# SROD of a measured volume: the AM Bench 2022 IN718 window
# (testdata/dream3d/am_bench_features.dream3d, 2.12 um sections along the build direction z),
# each cell colored by how far its orientation has turned from its grain's starting
# orientation (degrees), with bins of 4 um and the plate top 4 um above the window's bottom.
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_features.dream3d")
d.hide()
s = d.srod(layer=4, plate_top=129.96, min_volume=200, reference_volume=50, min_bin_volume=20)
s.set(color_by="SROD", cmap="Inferno")
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

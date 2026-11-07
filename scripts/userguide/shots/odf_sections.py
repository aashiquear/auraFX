# ODF sections of the AM Bench 2022 IN718 window (testdata/dream3d/am_bench_subset.dream3d) at
# phi2 = 0, 45 and 65 degrees (phi1 0..360 across, Phi 0..90 down), next to its inverse pole
# figures for X, Y and Z (not drawn here).
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_subset.dream3d")
d.hide()
o = d.odf_sections()
o.set(color_by="ODF", cmap="Viridis")
afx.annotate("phi2 = 0, 45, 65 degrees (top to bottom)", "upper left", 16)
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

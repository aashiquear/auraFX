# Overlay Mesh of a measured microstructure: the AM Bench 2022 IN718 window
# (testdata/dream3d/am_bench_features.dream3d, FeatureIds) meshed on its own voxel grid, one
# block per grain, the grain boundaries smoothed and wrapped in a layer of elements.
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_features.dream3d")
d.hide()
m = d.overlay_mesh(array="FeatureIds")
m.set(color_by="ElementBlock", cmap="Turbo", representation="Surface With Edges", edge_color=(0.15, 0.15, 0.15))
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

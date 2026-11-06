# A .dream3d microstructure: a 48 x 40 x 6 window of NIST's AM Bench 2022 IN718 EBSD volume
# (testdata/dream3d/am_bench_subset.dream3d, cut by scripts/gen_ebsd_h5.py), drawn with the
# IPF colors stored in the file (IPFColorZ, drawn as their own colors) and its cell edges.
import aurafx as afx
d = afx.load(ROOT + "/testdata/dream3d/am_bench_subset.dream3d")
d.set(color_by="IPFColorZ", representation="Surface With Edges", edge_color=(0.15, 0.15, 0.18))
afx.view([-0.8, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

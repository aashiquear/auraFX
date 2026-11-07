# Grains in 3-D: a 64 x 64 x 20 window of NIST's AM Bench 2022 IN718 1 um EBSD volume
# (testdata/dream3d/am_bench_features.dream3d, from scripts/gen_dream3d_window.py), segmented
# at 2 degrees within the sample mask, small defects and features under 7 cells cleaned up,
# colored by feature.
import aurafx as afx
v = afx.load(ROOT + "/testdata/dream3d/am_bench_features.dream3d", arrays="EulerAngles, Phases, SampleVoxels")
v.hide()
seg = v.segment_features(tolerance=2, mask="SampleVoxels", shuffle=True)
seg.hide()
clean = seg.fill_bad_data(min_defect=7).minimum_size(min_cells=7)
clean.hide()
col = clean.feature_colors()
col.set(color_by="FeatureColors")
for n in afx.nodes():
    n.hide()
col.show(); col.activate()
afx.view([-0.7, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

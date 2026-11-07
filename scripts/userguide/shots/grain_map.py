# Grains of an EBSD map: the synthetic nickel scan (testdata/ebsd/ni_map.ang, from
# scripts/gen_ebsd.py) segmented at 5 degrees, each feature in its own color, with the
# boundaries between features drawn as black lines on top.
import aurafx as afx
m = afx.load(ROOT + "/testdata/ebsd/ni_map.ang")
m.hide()
seg = m.segment_features(tolerance=5)
seg.hide()
col = seg.feature_colors()
col.set(color_by="FeatureColors")
gb = seg.feature_boundaries()
gb.set(color_by=None, solid_color=(0.05, 0.05, 0.05), representation="Wireframe", line_width=2)
m.hide(); seg.hide()
col.show(); gb.show(); col.activate()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

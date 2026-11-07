# Per-feature results in the Spreadsheet: the nickel map (testdata/ebsd/ni_map.ang) segmented
# at 5 degrees, with sizes, shapes, average orientations and neighbours, shown as the Feature
# Table.
import aurafx as afx
m = afx.load(ROOT + "/testdata/ebsd/ni_map.ang")
s = m.segment_features(tolerance=5).feature_sizes().feature_centroids().feature_shapes().average_orientations().feature_neighbors()
t = s.feature_table()
col = s.feature_colors()
col.set(color_by="FeatureColors")
for n in afx.nodes():
    n.hide()
col.show()
t.activate()
afx.show_panel("Spreadsheet")
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()

# Tutorial: a 3-D EBSD volume from the file to grain statistics and texture.
#   aurafx --script examples/ebsd/ebsd_analysis.py
# Uses the AM Bench nickel-alloy window shipped in testdata/dream3d. Segments the grains at a
# 3-degree tolerance, cleans them, measures sizes and shapes, colors the grains by the inverse
# pole figure key, draws the pole figures and writes the grain table as CSV.
import os
import aurafx as afx

root = os.path.dirname(os.path.abspath(__file__))
vol = afx.load(os.path.join(root, "../../testdata/dream3d/am_bench_subset.dream3d"))
vol.hide()

grains = vol.segment_features(tolerance=3, mask="SampleVoxels").fill_bad_data(min_defect=7).minimum_size(min_cells=7)
grains.hide()
measured = grains.feature_sizes().feature_shapes().average_orientations()
measured.orientation_colors(direction=[0, 0, 1])        # IPF-Z colors, drawn in the view

table = measured.feature_table()
table.save("grains.csv")
print("grain table written: grains.csv")

pf = vol.pole_figure(families="100, 110, 111")           # {100} {110} {111} pole figures
pf.send_to_view("right")
afx.reset_camera()

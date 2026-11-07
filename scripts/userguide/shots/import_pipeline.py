# Pipeline import: the segmentation steps of the AM Bench serial-section pipeline
# (testdata/dream3d/am_bench_segmentation.json) built onto the AM Bench window
# (testdata/dream3d/am_bench_subset.dream3d); a whole-window shot, so the pipeline browser shows
# the auraFX filters the steps became. The last IPF color step is drawn.
import aurafx as afx
v = afx.load(ROOT + "/testdata/dream3d/am_bench_subset.dream3d")
v.hide()
steps = afx.import_pipeline(ROOT + "/testdata/dream3d/am_bench_segmentation.json", v)
for n in steps[:-1]:
    n.hide()
last = steps[-1]
last.show()
last.activate()
last.set(color_by="IPFColorZ")
afx.view([0.3, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()

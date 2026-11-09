import aurafx as afx
# A bending bar from an LS-DYNA d3plot: the last state, colored by axial stress, with its time.
b = afx.load(ROOT + "/testdata/d3plot/bar/d3plot")
b.set(color_by="Stress", representation="Surface With Edges")
b.color_by("Stress", component=0, cmap="Cool to Warm")
afx.time(10)
afx.time_label(True)
afx.view("-y"); afx.reset_camera()
afx.screenshot(OUT)

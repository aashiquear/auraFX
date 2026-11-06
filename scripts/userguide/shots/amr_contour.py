# AMR Contour: the density isosurface of an AMReX plotfile (scripts/gen_amrex.py), a sphere
# crossing from the fine box into the coarse ones without cracks, colored by temperature,
# with the boxes' outlines.
import aurafx as afx
d = afx.load(ROOT + "/testdata/amr/plt_3d")
c = d.amr_contour(array="density", values=[0.3])
c.set(color_by="temperature", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.03, 0.03, 0.05))
d.show()
d.set(representation="Outline", color_by=None, solid_color=(0.15, 0.15, 0.15))
d.info(); c.info()
afx.view([0.7, -1.0, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=900)

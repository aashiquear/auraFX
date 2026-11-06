# AMR: an AMReX plotfile (scripts/gen_amrex.py) as the outlines of its boxes (two coarse boxes
# and one finer one), with a slice through both levels colored by density.
import aurafx as afx
d = afx.load(ROOT + "/testdata/amr/plt_3d")
s = d.slice(origin=(0, 0.05, 1.0), normal=(0, 1, 0))
s.set(color_by="density", cmap="Inferno")
d.show()
d.set(representation="Outline", color_by=None, solid_color=(0.15, 0.15, 0.15))
d.info(); s.info()
afx.view([0.7, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=900)

# A STEP file opened as a CAD Model: the solid is the base feature, built by the exact (B-rep)
# kernel with named faces, colored by face; the Kernel is in the Properties.
import aurafx as afx
m = afx.load(ROOT + "/testdata/cad/plate_with_hole.step")
m.cad.hole(diameter=5, depth=20, name="Second hole", at=(-10, -5, 12))
m.set(color_by="FaceId", cmap="Turbo", resolution=96)
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
m.activate()
afx.show_panel("Properties")

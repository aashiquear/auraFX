# Edit in CAD on a Geometry source: an angle bracket made with geometry commands becomes the
# first body of a CAD part (an imported body), and a hole and a rib are added to it with the
# CAD environment's features.
import aurafx as afx
g = afx.mesh.geometry()
g.cmd("brick x 40 y 24 z 8")
g.cmd("brick x 8 y 24 z 24; move body 2 x -16 z 16; unite body 1 2")
body = g.result(resolution=96)
body.update()
c = afx.cad_from(body, name="Bracket")
c.update()
e = c.edit_cad()
e.hole(diameter=7, depth=30, at=(8, 0, 4), name="Bore")
e.box(size=(14, 4, 14), at=(-7, 0, 11), name="Rib")
e.evaluate()
e.select("Bore")
afx.view([0.8, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
c.set(color_by="FaceId", cmap="Turbo")
afx.hide_panel("Information")

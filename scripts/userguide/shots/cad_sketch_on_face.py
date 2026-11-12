# A sketch that stays on its face: a boss drawn on the top face of a plate (by the face's name)
# after the plate was made thicker, and the list of faces its sketch can be moved to (the
# model tree's "Pick another face…").
import aurafx as afx
m = afx.cad_model(name="Plate with a boss")
m.update()
e = m.edit_cad()
e.clear("Plate with a boss")
e.parameter("t", "6 mm")
e.box(size=(40, 30, "t"), at=(0, 0, "t / 2"), name="Plate")
e.extrude(e.sketch(face="Plate z+").circle(r=6, center=(8, 0)), depth=5, name="Boss")
e.extrude(e.sketch(face="Plate z+").rectangle(8, 8, center=(-10, 0)), depth=2, op="cut", name="Pocket")
e.parameter("t", "12 mm")
e.evaluate()
e.select("Boss")
e.pick_face_for("Boss")
afx.view([0.8, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")
afx.hide_panel("Information")

# Picking edges: the Modify tab's "Fillet edges…" lists the edges of the exact part (with the
# faces on either side); the chosen ones become a Fillet feature.
import aurafx as afx
m = afx.cad_model(name="Bracket block")
m.cad.hole(diameter=8, depth=20, name="Bore", at=(5, 3, 12))
m.update()
e = m.edit_cad()
e.evaluate()
e.show_tab("Modify")
e.open_picker("fillet")
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")

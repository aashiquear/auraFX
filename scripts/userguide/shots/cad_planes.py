# A reference plane turned 30 degrees above the top plane with a block sketched on it, and the
# New reference plane form filled in for a second plane: the 3D view shows the plane the form
# would make (filled, strong) and the plane it starts from (light) before Make plane is pressed.
import aurafx as afx
m = afx.cad_model(name="Tilted block")
m.cad.box(size=(40, 30, 10), name="Plinth", at=(0, 0, -5))
m.cad.plane(name="Tilted", from_plane="XY", offset=20, angle=30, about="x")
m.cad.extrude(m.cad.sketch(plane="Tilted").rectangle(20, 10), depth=5, name="Block")
m.update()
e = m.edit_cad()
e.evaluate()
e.show_tab("Sketch")
e.plane_form(start="YZ", offset=12, angle=20, about="y", name="Side cut")
afx.view([0.9, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()

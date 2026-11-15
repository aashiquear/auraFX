# Sketching over the part: a sketch on the top of a plate with a bore and a boss. The part shows
# underneath the grid; the From the part tool has taken the rim of the bore as a circle and the
# front edge of the plate as a line, and the edge under the cursor (the foot of the boss) is
# marked with what it would become.
import aurafx as afx
m = afx.cad_model(name="Plate with a boss")
m.update()
e = m.edit_cad()
e.clear("Plate with a boss")
e.box(size=(60, 40, 8), at=(0, 0, 4), name="Plate")
e.hole(diameter=12, depth=8, name="Bore", at=(14, 2, 8))
e.cylinder(radius=7, height=10, at=(-14, 4, 8), name="Boss")
e.evaluate()
e.sketch_new(plane="XY", offset=8)
e.sketch_tool("project")
e.sketch_take_edge(20, 2)
e.sketch_take_edge(0, -20)
e.sketch_cursor(-7.2, 4.5)
afx.hide_panel("Information")

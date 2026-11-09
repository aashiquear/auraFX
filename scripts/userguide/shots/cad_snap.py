# The sketch canvas while drawing a line: snapping with a guide line from a far corner, and a
# dimension's value open for typing (as after a double-click on its label).
import aurafx as afx
m = afx.cad_model(name="Snapping")
m.update()
e = m.edit_cad()
e.evaluate()
e.sketch_new(plane="XY", items=[{"rectangle": {"center": [0, 0], "w": 40, "h": 30}}, {"circle": {"center": [45, 25], "r": 6}}])
e.sketch_dim_edit("first")
e.sketch_cursor(45.3, -15.2, line_from=0)

# The sketch canvas: the L-bracket template's sketch, dimensioned by the part's dimensions
# (length, height, t). The constraint and dimension tools are in the ribbon; the constraints
# with their values and expressions are listed on the right.
import aurafx as afx
m = afx.cad_model(template="bracket", name="L-bracket")
m.update()
e = m.edit_cad()
e.evaluate()
e.sketch_edit("Bracket")

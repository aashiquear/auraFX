# The CAD environment on an empty part: the Next strip at the top of the model tree offers the
# first steps, and the ribbon opens on the Sketch tab.
import aurafx as afx
m = afx.cad_model(name="Part")
m.update()
e = m.edit_cad()
e.remove("Base")        # the default plate, taken away: an empty part
e.show_tab("Sketch")

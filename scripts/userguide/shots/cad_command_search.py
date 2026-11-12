# The CAD command search (S in the 3D view, or Ctrl+K): typing "fil" lists the fillet commands
# and the ribbon tab each is on.
import aurafx as afx
m = afx.cad_model(name="Mounting plate")
m.update()
e = m.edit_cad()
e.hole(diameter=6, depth=12, name="Corner hole", at=(14, 9, 8))
e.evaluate()
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.show_panel("cad search fil")

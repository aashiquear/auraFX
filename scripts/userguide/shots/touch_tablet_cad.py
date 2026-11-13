# shot: interface=tablet window=1024x768 safe_area=24,20,0,0
# The CAD environment on a tablet held wide: the ribbon rows scroll sideways.
import aurafx as afx
m = afx.cad_model(name="Mounting plate")
m.update()
e = m.edit_cad()
e.parameter("pocket", "width / 4")
e.hole(diameter=6, depth=12, name="Corner hole", at=(14, 9, 8))
e.hole(diameter="pocket", depth=4, kind="counterbore", cb_diameter=14, cb_depth=3, name="Centre bore", at=(0, 0, 8))
e.fillet(0.8)
e.evaluate()
e.select("Centre bore")
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")
# The tablet build has no Python console: put it away here too, as a tablet shows the layout.
afx.hide_panel("Python")

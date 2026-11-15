# Sweeps and lofts from sketches kept with the part: a round profile swept along a bent path
# (the handle) and a foot lofted from a square to a circle. The model tree lists the kept
# sketches; the feature panel of the sweep chooses its profile and its path.
import aurafx as afx
m = afx.cad_model(name="Handle")
m.update()
e = m.edit_cad()
e.clear("Handle")
e.sketch_add(e.sketch(plane="YZ").circle(r=3), name="Profile")
e.sketch_add(e.sketch(plane="XY").path([(0, 0), (40, 0), (40, 25)]), name="Path")
e.sweep("Profile", "Path", name="Handle")
e.sketch_add(e.sketch(offset=-14).rectangle(22, 22), name="Foot base")
e.sketch_add(e.sketch(offset=-3).circle(r=5), name="Foot top")
e.loft(["Foot base", "Foot top"], name="Foot")
e.evaluate()
e.show_tab("Features")
e.select("Handle")
afx.view([0.8, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")
afx.hide_panel("Information")

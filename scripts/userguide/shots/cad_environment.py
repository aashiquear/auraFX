# The CAD environment open on a part: the ribbon, the feature tree, the properties of the
# selected feature and the timeline. The pipeline is paused; the part is built from its features.
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

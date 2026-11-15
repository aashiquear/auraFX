# The downstream check: a plate with a bored hole, an Extract Set of the hole's wall and two
# cuts below it. The hole is renamed and the plate narrowed; Apply asks what to do about what
# the nodes below refer to.
import aurafx as afx
m = afx.cad_model(name="Plate")
m.cad.hole(diameter=8, depth=20, name="Bore", at=(0, 0, 12))
m.update()
wall = m.extract_set(set="Bore side"); wall.set(name="Bore wall")
cut = m.clip(origin=(-12, 0, 4), normal=(1, 0, 0)); cut.set(name="Clip near the left end")
sl = m.slice(origin=(10, 0, 4), normal=(1, 0, 0)); sl.set(name="Slice near the right end")
e = m.edit_cad()
e.rename("Bore", "Pin hole")
e.parameter("width", 16)
e.evaluate()
m.set(color_by="FaceId", cmap="Turbo")
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
e.ask_apply()

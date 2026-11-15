# CAD Model in the window: a plate with a bored hole, its dimensions and a second configuration
# in the Properties, colored by face.
import aurafx as afx
m = afx.cad_model(name="Bracket plate")
m.cad.parameter("hole", "width / 5")
m.cad.hole(diameter="hole", depth=30, name="Bore", at=(0, 0, 12))
m.cad.fillet(0.6, feature="Base")
m.cad.configuration("wide", width=70, thickness=6)
m.set(color_by="FaceId", cmap="Turbo", resolution=96)
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
m.activate()
afx.show_panel("Properties")

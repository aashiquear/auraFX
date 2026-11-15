# CAD Model (no file needed): the flange template (a plate with a bolt circle of counterbored
# holes and a center bore), colored by face number.
import aurafx as afx
m = afx.cad_model(template="flange")
m.set(color_by="FaceId", cmap="Turbo", resolution=96)
afx.view([0.7, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

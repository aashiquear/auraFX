# The starter parts of Files > New from template (and afx.cad_model(template=...)): enclosure,
# flange, knob, threaded rod, vase and bracket, in two rows, each colored by face number.
import aurafx as afx
names = ["enclosure", "flange", "knob", "threaded rod", "vase", "bracket"]
for i, n in enumerate(names):
    m = afx.cad_model(template=n)
    m.set(color_by="FaceId", cmap="Turbo", resolution=80)
    m.hide()
    col, row = i % 3, i // 3
    s = m.transform(translate=(col * 130, -row * 130, 0))
    s.set(color_by="FaceId", cmap="Turbo")
afx.view([0.05, -0.45, 1.0], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

# The Inspect tab of the CAD environment: volume, surface, size, center of volume, the mass for a
# density, and the area of the cross-section at a height.
import aurafx as afx
m = afx.cad_model(template="bracket")
m.update()
e = m.edit_cad()
e.evaluate()
e.show_tab("Inspect")
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")

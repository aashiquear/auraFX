# Grain Realism (no file needed): the default columnar grains as plain columns (left) and
# with realism 0.8 and 40-unit layers (right): competitive growth thins the grains out with
# height, some columns stop at each layer, the columns lean one way and the other, fine
# grains sit on the layer boundaries and the boundaries are relaxed. Both colored by the
# inverse pole figure key for X.
import aurafx as afx
plain = afx.columnar_grains()
plain.hide()
a = plain.orientation_colors(direction=(1, 0, 0))
a.hide()
before = a.transform()
real = afx.columnar_grains(realism=0.8, layer=40)
real.hide()
b = real.orientation_colors(direction=(1, 0, 0))
b.hide()
after = b.transform(translate=(380, 0, 0))
plain.hide(); a.hide(); real.hide(); b.hide()
before.show(); after.show(); after.activate()
before.set(color_by="IPFColor")
after.set(color_by="IPFColor")
afx.view([0.25, -1.0, 0.35], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

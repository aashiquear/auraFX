# Two-Point Statistics (no file needed): overlapping spheres of radius 5 at a volume fraction
# of 0.3 on 96 x 96 x 96 cells, and the chart of their two-point probability measured on the
# cells (S2) with the exact curve the model keeps (Expected).
import aurafx as afx
s = afx.overlapping_spheres(cells=[96, 96, 96], radius=5, fraction=0.3)
s.hide()
solid = s.threshold(array="Phases", lower=2)
solid.set(color_by=None, solid_color=(0.82, 0.52, 0.28))
t = s.two_point_statistics(periodic=True, reach=24)
afx.hide_panel("Charts")
t.plot("S2", "Expected", x="Distance", style="markers", title="Two-point probability of overlapping spheres")
solid.activate()
afx.view("iso"); afx.reset_camera()

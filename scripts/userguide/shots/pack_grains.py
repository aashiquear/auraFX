# Pack Grains (no file needed): a 80-cube of grains packed from goal statistics (diameters
# lognormal around 14 cells, elongated along x: b/a and c/a about 0.5), with 4 % precipitates
# of a second phase placed on the grain boundaries (the small features between them); every
# feature in its own color.
import aurafx as afx
g = afx.pack_grains(cells=[80, 80, 80], esd_mu=2.64, esd_sigma=0.15, ba=[10, 10], ca=[10, 10], aligned=True)
g.hide()
p = g.insert_precipitates(fraction=0.04, esd_mu=1.4, at_boundaries=True)
p.hide()
c = p.feature_colors()
c.set(color_by="FeatureColors")
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

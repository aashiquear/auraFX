# Laguerre grains (no file needed): 300 grains on a 64-cube grid, seed radii spread
# lognormally (log sigma 0.4), periodic, random orientations, colored by the inverse pole
# figure key for Z.
import aurafx as afx
v = afx.voronoi_grains(cells=[64, 64, 64], grains=300, size_sigma=0.4, periodic=True)
v.hide()
c = v.orientation_colors()
c.set(color_by="IPFColor")
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

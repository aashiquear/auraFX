# Grain boundaries: 60 Voronoi grains (no file), their boundary surfaces smoothed and colored
# by the inverse pole figure key of each boundary plane; a turntable.
import aurafx as afx
v = afx.voronoi_grains(cells=[40, 40, 40], grains=60); v.hide()
s = v.quick_surface_mesh(); s.hide()
m = s.smooth_boundaries(iterations=20); m.hide()
c = m.face_orientation_colors()
c.set(color_by="FaceIPFColors")
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=20, orbit=80)

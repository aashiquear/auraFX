# Crystal plasticity: the PINNACLE result of a 30-grain plate pulled to 2 % strain
# (testdata/solver/pinnacle_plate_fine, about 5 000 elements), colored by the equivalent plastic strain with the grain
# boundaries, the displacement field scaled up from 0 to 9 times.
import aurafx as afx
d = ROOT + "/testdata/solver/pinnacle_plate_fine/"
res = afx.load(d + "fields_final.vtu"); res.hide()
pts = afx.load(d + "orientations_points.csv"); pts.hide()
g = res.assign_grains(pts, method="Nearest point", boundaries=True); g.hide()
w = g.warp_by_vector(vectors="displacement", scale=0.0)
w.set(color_by="equivalent_plastic_strain", cmap="Inferno")
b = w.extract_set(set="grain boundaries"); w.show()   # a new child hides its parent
b.set(color_by=None, representation="Wireframe", solid_color=(0.92, 0.92, 0.92), line_width=2)
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=15, frames=45, width=720, height=400, cue=lambda t: w.set(scale=9.0 * t))

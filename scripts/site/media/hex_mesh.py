# Meshing: eight spheres packed in a cube and meshed with conforming hexahedra (no file); the
# inclusions with three walls of the box; a turntable.
import aurafx as afx
m = afx.inclusion_mesh(count=8, radius=0.075, cells=24, seed=7); m.hide()
incl = m.threshold(array="ElementBlock", lower=50)
incl.set(color_by=None, solid_color=(0.85, 0.35, 0.25), representation="Surface With Edges", edge_color=(0.15, 0.05, 0.05))
for name in ("x-", "y+", "z-"):
    w = m.extract_set(set=name)
    w.set(color_by=None, solid_color=(0.78, 0.8, 0.84), representation="Surface With Edges", edge_color=(0.35, 0.37, 0.42))
afx.view([1.0, -0.7, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=20, orbit=80)

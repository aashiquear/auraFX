# Inclusion Mesh (no file needed): eight spheres of radius 0.075 packed in a unit cube, meshed
# with conforming hexahedra (24 cells along a side). Shown: the inclusion elements (block 100,
# kept by Threshold on ElementBlock) with their edges, and three walls of the box (the side sets
# x-, y+ and z-, from Extract Set) with the background grid.
import aurafx as afx
m = afx.inclusion_mesh(count=8, radius=0.075, cells=24, seed=7)
incl = m.threshold(array="ElementBlock", lower=50)
incl.set(color_by=None, solid_color=(0.85, 0.35, 0.25), representation="Surface With Edges", edge_color=(0.15, 0.05, 0.05))
walls = []
for name in ("x-", "y+", "z-"):
    w = m.extract_set(set=name)
    w.set(color_by=None, solid_color=(0.78, 0.8, 0.84), representation="Surface With Edges", edge_color=(0.35, 0.37, 0.42))
    walls.append(w)
for n in afx.nodes():
    n.hide()
incl.show()
for w in walls:
    w.show()
incl.activate()
afx.view([1.0, -0.7, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

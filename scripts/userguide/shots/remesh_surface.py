import aurafx as afx
# A torus remeshed: triangles at one size (left) and quadrilaterals sized by curvature (right).
t = afx.geometry(commands="torus major 1 minor 0.35", resolution=64)
a = t.remesh_surface(size=0.12)
a.set(representation="Surface With Edges", solid_color=[0.8, 0.82, 0.9])
b = t.remesh_surface(size=0.4, tolerance=0.004, element="Quadrilaterals").transform(translate=[2.9, 0, 0])
b.set(color_by="Quality", cmap="Viridis", representation="Surface With Edges")
t.hide()
afx.view([0.2, -1.0, 0.9]); afx.reset_camera()
afx.screenshot(OUT)

# Surface operations: a line swept round an axis (Rotational Extrusion, three quarters of a
# turn), a coarse sphere rounded by Loop subdivision and colored by its Gaussian curvature, and
# a line swept along a vector into a ribbon (Linear Extrusion).
import aurafx as afx
line = afx.line(point1=(0.6, 0, 0), point2=(0.25, 0, 1.2), resolution=12)
line.hide()
vase = line.rotational_extrusion(angle=270, resolution=48)
vase.set(solid_color=(0.35, 0.65, 0.95), representation="Surface With Edges", edge_color=(0.15, 0.2, 0.3))
base = afx.sphere(radius=0.6, theta=6, phi=5); base.hide()
smooth = base.subdivide(scheme="Loop", levels=3); smooth.hide()
curv = smooth.curvature(kind="Gaussian"); curv.hide()
ball = curv.transform(translate=(1.8, 0, 0.6))
ball.set(color_by="Gauss_Curvature", cmap="Viridis")
rib = afx.line(point1=(3.2, -0.5, 0.0), point2=(3.2, 0.5, 1.2), resolution=12)
rib.hide()
sheet = rib.linear_extrusion(vector=(1.0, 0.0, 0.0), scale=1.0)
sheet.set(solid_color=(0.95, 0.6, 0.25), representation="Surface With Edges", edge_color=(0.3, 0.2, 0.1))
afx.view([0.5, -1.0, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

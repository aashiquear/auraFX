# Plate Mesh (no file needed): a 40 x 20 x 1 plate of unit cells with a 45-degree elliptical
# hole (semi-axes 2 and 4) meshed by a conforming ring, colored by block (plate, ring), seen
# from above.
import aurafx as afx
p = afx.plate_mesh(size=(40, 20, 1), cell=1.0, hole="Ellipse", a=2, b=4, angle=45)
p.set(color_by="ElementBlock", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

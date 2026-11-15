# From a STEP file to a mesh: the file opens as a CAD Model, Overlay Mesh fills it with
# hexahedra, and every face of the file is a side set of the mesh (Extract Set draws each in
# its own color over the mesh).
import aurafx as afx
m = afx.load(ROOT + "/testdata/cad/plate_with_hole.step")
m.hide()
mesh = m.overlay_mesh(cell=2.5)
mesh.hide()
palette = [(0.9, 0.3, 0.2), (0.2, 0.6, 0.9), (0.3, 0.75, 0.35), (0.95, 0.75, 0.2), (0.7, 0.4, 0.85), (0.2, 0.8, 0.8), (0.95, 0.5, 0.15)]
for k in range(1, 8):
    s = mesh.extract_set(set="plate_with_hole face %d" % k)
    s.set(solid_color=palette[k - 1], representation="Surface With Edges", edge_color=(0.2, 0.2, 0.25), name="face %d" % k)
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

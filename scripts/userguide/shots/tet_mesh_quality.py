# Hexahedra to tetrahedra (no file needed): a plate with a hole meshed with hexahedra, split
# into conforming tetrahedra (12 per hexahedron) and colored by each tetrahedron's minimum
# dihedral angle from Mesh Quality.
import aurafx as afx
p = afx.plate_mesh(size=(40, 20, 4), cell=2.0, hole="Circle", a=4)
p.hide()
t = p.convert_elements(to="Hexahedra to tetrahedra")
t.hide()
q = t.mesh_quality()
q.set(color_by="MinimumDihedralAngle", cmap="Viridis", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0.4, -1.0, 1.2], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

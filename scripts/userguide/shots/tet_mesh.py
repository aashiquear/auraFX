# Tetrahedral meshing of a part built with commands (no file needed): a plate with a hole
# and a ball beside it, filled with tetrahedra of size 0.1 by the Geometry source's Tet mesh
# output, colored by each tetrahedron's minimum dihedral angle (Mesh Quality).
import aurafx as afx
g = afx.geometry(output="Tet mesh", commands="""brick x 4 y 2 z 1
cylinder radius 0.5 height 2
subtract body 2 from body 1
sphere radius 0.7
move body 3 x 3.2
mesh body 1 3 size 0.1
""")
g.hide()
q = g.mesh_quality()
q.set(color_by="MinimumDihedralAngle", cmap="Viridis", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0.3, -0.7, 1.0], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

# Mesh commands (no file needed): a turned brick and a cylinder meshed with hexahedra by
# 'mesh body' commands, the cylinder (an O-grid swept along its axis) in its own block.
import aurafx as afx
g = afx.geometry(output="Hex mesh", commands="""brick x 2 y 1 z 1
rotate body 1 angle 30 about z
cylinder radius 0.5 height 2
move body 2 x 2.6
mesh body 1 size 0.2
mesh body 2 size 0.1
block 2 add body 2 name 'rod'
sideset 1 add face 7 name 'rod surface'
""")
g.set(color_by="ElementBlock", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0.6, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

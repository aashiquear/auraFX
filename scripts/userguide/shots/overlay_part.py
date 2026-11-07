# Overlay meshing of a part built with commands (no file needed): a plate with a hole and a
# ball pressed into its end, meshed together on one 0.1 grid; the ball and the plate are two
# blocks that meet node to node.
import aurafx as afx
g = afx.geometry(output="Hex mesh", commands="""brick x 4 y 2 z 1
cylinder radius 0.5 height 2
subtract body 2 from body 1
sphere radius 0.7
move body 3 x 2.1
mesh body 1 3 size 0.1 scheme overlay
""")
g.set(color_by="ElementBlock", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0.3, -0.7, 1.0], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

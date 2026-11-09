# Webcuts (no file needed): a brick cut by a plane along its own axis into two structured bricks
# (they meet node to node on the cut), and a second brick with a core webcut out by a cylinder,
# meshed on one overlay grid; colored by block, with element edges.
import aurafx as afx
g = afx.mesh.geometry()
g.cmd("brick x 3 y 1 z 1; webcut body 1 with plane xplane offset 0.4")
g.cmd("brick x 1.6 y 1.6 z 1; move body 3 x 3.4; webcut body 3 with cylinder radius 0.45 axis z center 3.4 0 0")
g.cmd("imprint body 1 2; mesh body 1 2 size 0.1; mesh body 3 4 size 0.08")
mesh = g.result(output="Hex mesh")
mesh.set(color_by="ElementBlock", cmap="Turbo", representation="Surface With Edges", edge_color=(0.15, 0.15, 0.15))
afx.view([0.55, -1.0, 0.75], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)

import aurafx as afx
# A ball and a rounded block, each meshed with the mapped scheme as one structured block.
g = afx.geometry(commands="""sphere radius 1
superquadric x 1.4 y 0.8 z 0.8 e1 0.5 e2 0.5
move body 2 x 3
mesh body 1 2 size 0.18 scheme mapped""", output="Hex mesh")
g.set(color_by="ElementBlock", cmap="Cool to Warm", representation="Surface With Edges")
afx.view([0.3, -1.0, 0.6]); afx.reset_camera()
afx.screenshot(OUT)

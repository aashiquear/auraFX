import aurafx as afx
# A brick webcut along an L-shaped curve swept along z, meshed with graded intervals.
g = afx.geometry(commands="""brick x 2 y 2 z 1
outline 9 polyline 0.2 2 0.2 0.2 2 0.2
webcut body 1 with sweep outline 9
mesh body 1 2 size 0.08""", output="Hex mesh")
g.set(color_by="ElementBlock", cmap="Cool to Warm", representation="Surface With Edges")
b = afx.geometry(commands="brick x 2 y 1 z 1\nmesh body 1 size 0.1 intervals 16 6 6 bias 4 1 2 cluster both", output="Hex mesh").transform(translate=[2.8, 0, 0])
b.set(representation="Surface With Edges", solid_color=[0.8, 0.82, 0.9])
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

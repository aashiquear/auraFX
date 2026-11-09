import aurafx as afx
# A plate with a hole paved with quadrilaterals refined near the hole (colored by quality), and
# the same face swept into three layers of hexahedra beside it.
face = afx.face_mesh(commands="outline 1 rectangle x 4 y 2 center 2 1\noutline 2 circle radius 0.5 center 2 1",
                     outer=1, holes="2", size=0.15, near=0.05, growth=0.3)
sw = face.sweep_quads(direction=(0, 0, 1), distance=0.5, layers=3)
plate = sw.transform(translate=[0, -2.6, 0])
sw.hide()
plate.set(representation="Surface With Edges", solid_color=[0.75, 0.8, 0.9])
face.show()
face.set(color_by="Quality", cmap="Viridis", representation="Surface With Edges")
afx.view([0.3, -1.0, 1.4]); afx.reset_camera()
afx.screenshot(OUT)

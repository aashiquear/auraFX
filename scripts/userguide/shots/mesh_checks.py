import aurafx as afx
# Nine hexahedra: one turned inside out, one squashed flat; Validate Cells marks them.
src = afx.programmable_source(script="""
import numpy as np
pts = []
for k in range(2):
    for j in range(4):
        for i in range(4):
            pts.append((i, j, k))
pts = np.array(pts, float)
pts[1 * 1 + 4 * 1 + 16 * 1] = (1, 1, 0.0)
cells = []
for j in range(3):
    for i in range(3):
        a = i + 4 * j
        cells.append([a, a + 1, a + 5, a + 4, a + 16, a + 17, a + 21, a + 20])
cells[8] = cells[8][4:] + cells[8][:4]
output = afx.make_grid(pts, cells, 12)
""")
v = src.validate_cells()
v.set(color_by="ValidityState", cmap="Turbo", representation="Surface With Edges")
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

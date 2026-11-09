import aurafx as afx
# A vortex pair on a plane, filled with evenly spaced streamlines.
src = afx.programmable_source(script="""
import numpy as np
n = 121
output = afx.make_image((n, n, 1), spacing=(2.0 / (n - 1), 2.0 / (n - 1), 1.0), origin=(-1.0, -1.0, 0.0))
p = output.points
x, y = p[:, 0], p[:, 1]
v = np.zeros((len(x), 3))
for cx, s in ((-0.4, 1.0), (0.4, -1.0)):
    r2 = (x - cx) ** 2 + y ** 2 + 0.02
    v[:, 0] += -s * y / r2
    v[:, 1] += s * (x - cx) / r2
output.point_data["V"] = v
output.point_data["speed"] = np.linalg.norm(v, axis=1)
""")
src.set(color_by="speed", cmap="Grayscale", opacity=0.3)
s = src.evenly_spaced_streamlines_2d(vectors="V", separation=0.06)
s.set(color_by="speed", cmap="Viridis", line_width=2)
src.show()
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT)

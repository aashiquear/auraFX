import aurafx as afx
# A ball given as cell volume fractions on a 24^3 grid: the mixed cells and the rebuilt interface.
src = afx.programmable_source(script="""
import numpy as np
n, s, r = 24, 4, 0.6
h = 2.0 / n
output = afx.make_image((n + 1, n + 1, n + 1), spacing=(h, h, h), origin=(-1.0, -1.0, -1.0))
c = (np.arange(n) + 0.5) * h - 1.0
o = ((np.arange(s) + 0.5) / s - 0.5) * h
X, Y, Z = np.meshgrid(c, c, c, indexing='ij')
f = np.zeros((n, n, n))
for a in o:
    for b in o:
        for e in o:
            f += ((X + a) ** 2 + (Y + b) ** 2 + (Z + e) ** 2 <= r * r)
output.cell_data["ball"] = (f / s ** 3).transpose(2, 1, 0).ravel()
""")
mixed = src.threshold(array={"assoc": "Cells", "name": "ball"}, lower=0.001, upper=0.999)
mixed.set(representation="Wireframe", color_by="ball", opacity=0.25)
m = src.material_interface(arrays="ball")
m.color_by(None)
m.set(solid_color=(0.95, 0.55, 0.2))
mixed.show()
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

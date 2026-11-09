import aurafx as afx
# A box mesh with its inner nodes disturbed (left) and after Winslow smoothing (right), sliced.
m = afx.box_mesh(size=(1, 1, 1), cells=(10, 10, 10))
j = m.programmable_filter(script="""
import numpy as np
p = inputs[0].points.copy()
rng = np.random.default_rng(3)
inner = np.all((p > 1e-9) & (p < 1 - 1e-9), axis=1)
p[inner] += (rng.random((inner.sum(), 3)) - 0.5) * 0.06
output.points = p
""")
w = j.smooth_mesh(scheme="Winslow", iterations=30)
for k, n in enumerate((j, w)):
    s = n.slice(origin=[0.5, 0.5, 0.5], normal=[0, 0, 1])
    if k:
        s = s.transform(translate=[1.3, 0, 0])
    s.set(representation="Wireframe", solid_color=[0.15, 0.2, 0.35])
m.hide(); j.hide(); w.hide()
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT)

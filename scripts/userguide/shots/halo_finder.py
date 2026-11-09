import aurafx as afx
# Particles: a sparse background and five clumps; friends-of-friends halos colored by HaloId.
src = afx.programmable_source(script="""
import numpy as np
rng = np.random.default_rng(3)
p = [rng.random((3000, 3))]
for c, n, w in (((0.2, 0.3, 0.3), 900, 0.03), ((0.7, 0.2, 0.6), 700, 0.025), ((0.5, 0.75, 0.4), 600, 0.03), ((0.8, 0.8, 0.8), 400, 0.02), ((0.3, 0.7, 0.8), 300, 0.02)):
    p.append(np.array(c) + w * rng.standard_normal((n, 3)))
p = np.vstack(p)
output = afx.make_points(p)
""")
h = src.halo_finder(linking=0.2, min_members=50)
h.set(representation="Points", point_size=4, color_by="HaloId", cmap="Turbo")
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

# Grain growth: the Potts-model run of examples/grain_growth/potts.py (run it first), the
# grain ids over 300 Monte Carlo sweeps.
import aurafx as afx
d = afx.load(ROOT + "/examples/grain_growth/grains_0000.vtk")
d.set(color_by="grain_id", cmap="Turbo")
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
n = afx.time()[1]
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=12, frames=n, width=560, height=560, cue=lambda t: afx.time(round(t * (n - 1))))

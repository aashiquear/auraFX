# Phase field: spinodal decomposition from examples/phase_field/cahn_hilliard.py (run it first),
# the concentration over the 61 outputs.
import aurafx as afx
d = afx.load(ROOT + "/examples/phase_field/ch_00000.vtk")
d.set(color_by="concentration", cmap="Viridis", range=(0, 1))
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
n = afx.time()[1]
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=12, frames=n, width=560, height=560, cue=lambda t: afx.time(round(t * (n - 1))))

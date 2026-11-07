# Mechanics: the crushed can (testdata/can.ex2, 44 time steps of an explicit dynamics run),
# colored by the equivalent plastic strain, the time steps played once.
import aurafx as afx
c = afx.load(ROOT + "/testdata/can.ex2")
c.set(color_by="EQPS", cmap="Turbo")
afx.view("iso"); afx.reset_camera()
n = afx.time()[1]
afx.save_animation(OUT, fps=14, frames=n, width=720, height=450, cue=lambda t: afx.time(round(t * (n - 1))))

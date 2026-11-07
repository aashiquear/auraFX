# Mechanics: the crushed can (testdata/can.ex2, 44 time steps of an explicit dynamics run),
# colored by the equivalent plastic strain, the time steps played once.
import aurafx as afx
c = afx.load(ROOT + "/testdata/can.ex2")
# A fixed color range over all steps (the last step reaches about 1.8), seen from the side so
# the can under the plate stays in view as it crumples.
c.set(color_by="EQPS", cmap="Turbo", range=(0.0, 1.5))
n = afx.time()[1]
# Fit the camera at a middle step so both the falling plate and the crushed can stay in frame.
afx.time(n // 2)
afx.view([1.0, -0.75, 0.12], up=[0, 0, 1]); afx.reset_camera()
afx.save_animation(OUT, fps=14, frames=n, width=720, height=450, cue=lambda t: afx.time(round(t * (n - 1))))

# Particles: a LAMMPS dump of a melting FCC crystal (testdata/fcc_melt.dump, 20 frames), the
# atoms colored by their potential energy.
import aurafx as afx
d = afx.load(ROOT + "/testdata/fcc_melt.dump")
d.set(representation="Points", point_size=5, color_by="c_pe", cmap="Plasma")
afx.view([0.5, -1.0, 0.6], up=[0, 0, 1]); afx.reset_camera()
n = afx.time()[1]
afx.save_animation(OUT, fps=8, frames=n, width=720, height=450, cue=lambda t: afx.time(round(t * (n - 1))))

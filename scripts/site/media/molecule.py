# Chemistry: a molecule (testdata/molecules/test.cml) as balls and sticks; a turntable.
import aurafx as afx
m = afx.load(ROOT + "/testdata/molecules/test.cml")
m.set(atom_scale=0.22)
afx.view([0.15, -0.35, 1.0], up=[0, 1, 0]); afx.reset_camera()
afx.save_animation(OUT, fps=20, orbit=80)

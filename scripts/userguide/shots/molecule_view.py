# Molecules: the test molecule (testdata/molecules/test.cml, from scripts/gen_cml.py) as it
# opens: balls and sticks colored by element, double bonds as two sticks; Properties shows
# the molecule settings.
import aurafx as afx
m = afx.load(ROOT + "/testdata/molecules/test.cml")
m.set(atom_scale=0.22)
m.activate(); m.info()
afx.view([0.15, -0.35, 1.0], up=[0, 1, 0]); afx.reset_camera()

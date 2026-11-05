import aurafx as afx
a = afx.load(ROOT + "/testdata/lagrange_annulus.vtu")
a.set(representation="Surface With Edges", nonlinear_subdivision=3)
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT, width=800)

# The Save Screenshot window with a 16 : 9 picture on a gradient chosen.
import aurafx as afx
c = afx.load(ROOT + "/testdata/can.ex2")
c.set(color_by="EQPS")
afx.time(30)
afx.view([0.6, -1.0, 0.4], up=[0, 0, 1]); afx.reset_camera()
afx.show_panel("screenshot example")

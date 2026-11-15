# A picture at 16 : 9 on a gradient background (the view on the screen keeps its own).
import aurafx as afx
c = afx.load(ROOT + "/testdata/can.ex2")
c.set(color_by="EQPS")
afx.time(30)
afx.view([0.6, -1.0, 0.4], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=1280, height=720, background=[(0.93, 0.95, 0.98), "#6b8cbd"])

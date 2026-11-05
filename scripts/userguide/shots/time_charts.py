import aurafx as afx
c = afx.load(ROOT + "/testdata/can.ex2")
c.set(color_by="EQPS")
afx.time(20); afx.time_label(True)
afx.over_time(c, "EQPS", stat="max")
afx.view("iso"); afx.reset_camera()
afx.show_panel("Charts")

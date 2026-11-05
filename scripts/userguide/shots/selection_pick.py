import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
d.set(color_by="Temp")
afx.select(d, list(range(2000, 2600)), "cells")
afx.view("iso"); afx.reset_camera()

import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
d.set(color_by="Temp")
k = d.clip(type="Box", box_min=(-3, -3, -2), box_max=(3, 3, 4)); k.set(color_by="Temp")
d.hide(); afx.view("iso"); afx.reset_camera()

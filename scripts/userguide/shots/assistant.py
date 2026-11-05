import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
d.set(color_by="Temp"); afx.view("iso"); afx.reset_camera()
afx.show_panel("Assistant")

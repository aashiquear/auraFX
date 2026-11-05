import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
s = d.slice(normal=(0, 1, 0)); s.set(color_by="Temp")
afx.view("iso"); afx.reset_camera()
afx.show_panel("Python")

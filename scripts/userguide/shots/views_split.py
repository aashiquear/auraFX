import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
d.set(color_by="Temp")
afx.split_view("right")
s = d.slice(normal=(1, 0, 0)); s.set(color_by="V", representation="Surface LIC", lic_vectors="V")
afx.active_view(0); d.show(); s.hide()
afx.active_view(1); d.hide(); s.show()
afx.link_views(True); afx.view("iso"); afx.reset_camera()

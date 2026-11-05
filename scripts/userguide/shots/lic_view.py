import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
s = d.slice(normal=(1, 0, 0)); s.set(color_by="Temp", representation="Surface LIC", lic_vectors="V")
d.hide(); afx.view("+x"); afx.reset_camera()
afx.screenshot(OUT, width=800)

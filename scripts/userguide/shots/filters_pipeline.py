import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
s = d.slice(normal=(0, 1, 0)); s.set(color_by="Temp")
c = d.contour(array="Temp", values=[400.0, 600.0]); c.set(color_by="Temp", opacity=0.6)
k = d.clip(normal=(1, 0, 0)); k.set(color_by="Pres", representation="Surface With Edges")
d.hide(); afx.view("iso"); afx.reset_camera()

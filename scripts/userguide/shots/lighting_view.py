import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
c = d.contour(array="Temp", values=[400.0]); c.set(color_by="Temp")
d.hide()
afx.plane(origin=(-12, -12, -10.5), point1=(12, -12, -10.5), point2=(-12, 12, -10.5))
afx.lighting(shadows=True, ssao=True, fxaa=True)
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT, width=1000)

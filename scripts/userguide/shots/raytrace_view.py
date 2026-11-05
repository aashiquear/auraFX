import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
c = d.contour(array="Temp", values=[400.0]); c.set(color_by="Temp", metallic=1.0, roughness=0.25)
d.hide()
afx.plane(origin=(-12, -12, -10.5), point1=(12, -12, -10.5), point2=(-12, 12, -10.5))
afx.lighting(environment="sky", show_environment=True, ray_tracing=True, ray_samples=128)
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT, width=1000)

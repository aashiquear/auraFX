import aurafx as afx
c = afx.load(ROOT + "/testdata/can.ex2")
c.set(color_by="EQPS")
afx.key_camera(0.0); afx.view("+y"); afx.key_camera(1.0, interp="ease in-out")
c.key("opacity", 0.0, 1.0); c.key("opacity", 1.0, 0.3)
afx.animation_time(0.4)
afx.show_panel("animation")

# Flow and heat: the heated disk (testdata/disk_out_ref.ex2): streamlines of the velocity as
# tubes colored by temperature over a translucent temperature contour; a turntable.
import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2"); d.hide()
c = d.contour(array="Temp", values=[400.0]); c.set(color_by="Temp", opacity=0.35)
s = d.stream_tracer(seeds=60); s.hide()
t = s.tube(radius=0.12); t.set(color_by="Temp", cmap="Inferno")
afx.view([0.7, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()
afx.save_animation(OUT, fps=20, orbit=80)

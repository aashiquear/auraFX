# The main window on a small result (testdata/disk_out_ref.ex2): the pipeline with a contour and
# stream lines as tubes, their properties, the 3D view, the Python console and the assistant.
# The same picture is the README's (copied to docs/images/screenshot.png).
import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2"); d.hide()
c = d.contour(array="Temp", values=[400.0]); c.set(color_by="Temp", opacity=0.35)
s = d.stream_tracer(seeds=60); s.hide()
t = s.tube(radius=0.12); t.set(color_by="Temp", cmap="Inferno")
t.activate()
afx.view([0.7, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()

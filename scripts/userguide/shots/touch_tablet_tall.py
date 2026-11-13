# shot: interface=tablet window=768x1024 safe_area=24,20,0,0
# The Tablet interface, held tall: the 3D view on top, two groups of tabs below; the menus that do
# not fit the width are in the last menu.
import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2"); d.hide()
c = d.contour(array="Temp", values=[400.0]); c.set(color_by="Temp", opacity=0.35)
s = d.stream_tracer(seeds=60); s.hide()
t = s.tube(radius=0.12); t.set(color_by="Temp", cmap="Inferno")
t.activate()
afx.view([0.7, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()
# The tablet build has no Python console: put it away here too, as a tablet shows the layout.
afx.hide_panel("Python")

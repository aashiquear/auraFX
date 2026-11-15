# shot: interface=phone window=390x844 safe_area=47,34,0,0
# The Phone interface, held tall: the menu button, the tool row, the 3D view and one group of tabs.
# The empty strips at the top and the bottom are the ones a phone keeps for its status bar and its
# home indicator (here set with AURAFX_SAFE_AREA).
import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2"); d.hide()
c = d.contour(array="Temp", values=[400.0]); c.set(color_by="Temp", opacity=0.35)
s = d.stream_tracer(seeds=60); s.hide()
t = s.tube(radius=0.12); t.set(color_by="Temp", cmap="Inferno")
t.activate()
afx.view([0.7, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()

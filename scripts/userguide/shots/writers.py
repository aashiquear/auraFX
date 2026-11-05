# The can crash written as an EnSight Gold case and as Xdmf (with every time step), then
# opened again side by side: EnSight on the left, Xdmf on the right.
import aurafx as afx, os, shutil
d = afx.load(ROOT + "/testdata/can.ex2")
out = "/tmp/aurafx_guide_writers"
shutil.rmtree(out, ignore_errors=True)
os.makedirs(out)
d.save(out + "/can.case", all_times=True)
d.save(out + "/can.xmf", all_times=True)
e = afx.load(out + "/can.case")
x = afx.load(out + "/can.xmf")
d.hide()
for n in (e, x):
    n.set(color_by="EQPS", cmap="Viridis")
afx.split_view("right")
afx.active_view(0); e.show(); x.hide()
afx.active_view(1); x.show(); e.hide()
afx.link_views(True)
afx.time(43)
x.info()          # waits for the data
afx.view([0.3, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()

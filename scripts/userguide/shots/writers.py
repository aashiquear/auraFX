# The can crash written as EnSight Gold, Xdmf and CGNS (with every time step), then opened
# again: EnSight on the left, Xdmf top right, CGNS bottom right (each split_view splits the
# view made by the one before).
import aurafx as afx, os, shutil
d = afx.load(ROOT + "/testdata/can.ex2")
out = "/tmp/aurafx_guide_writers"
shutil.rmtree(out, ignore_errors=True)
os.makedirs(out)
for ext in ("case", "xmf", "cgns"):
    d.save(out + "/can." + ext, all_times=True)
nodes = [afx.load(out + "/can." + ext) for ext in ("case", "xmf", "cgns")]
d.hide()
for n in nodes:
    n.set(color_by="EQPS", cmap="Viridis")
afx.split_view("right")
afx.split_view("down")
for v in range(3):
    afx.active_view(v)
    for k, n in enumerate(nodes):
        n.show() if k == v else n.hide()
afx.link_views(True)
afx.time(43)
for n in nodes:
    n.info()          # waits for the data
afx.view([0.3, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()

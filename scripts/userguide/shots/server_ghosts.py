# A parallel server with four processes ("This computer"): the gradient of the temperature in
# testdata/disk_out_ref.ex2, computed piece by piece with ghost cells, cut open by a clip.
import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2", host="local", processes=4)
g = d.gradient(array="Temp")
c = g.clip(normal=[1, 0, 0])
c.set(color_by="Temp_gradient", cmap="Inferno")
d.hide(); g.hide()
afx.view([-1.0, -0.5, 0.35], up=[0, 0, 1]); afx.reset_camera()

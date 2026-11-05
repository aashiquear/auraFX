import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
d.set(color_by="Temp", cmap="Inferno", cmap_bands=8)
afx.view("iso"); afx.reset_camera()
afx.show_panel("colormap")

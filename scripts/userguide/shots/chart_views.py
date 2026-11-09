import aurafx as afx
# Two chart views beside the 3D view, each plotting its own object.
src = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
src.set(color_by="Temp")
line = src.plot_over_line(point1=[0, 0, -10], point2=[0, 0, 10])
hist = src.histogram(array="Temp", bins=30)
afx.hide_panel("Charts")
a = line.plot("Temp", "Pres", x="arc_length", style="markers", title="Along the axis")
b = hist.plot("Temp_count", style="bars", title="Temperature histogram")
src.activate()
afx.view("iso"); afx.reset_camera()

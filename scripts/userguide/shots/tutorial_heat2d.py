# The heat diffusion example (examples/heat2d, its five committed output files) opened as a
# time series: the temperature at the third step, with its maximum charted over time; a
# whole-window shot showing the time toolbar and the Charts panel.
import aurafx as afx
d = afx.load(ROOT + "/examples/heat2d/heat_0000.vtk")
d.set(color_by="temperature", cmap="Inferno")
afx.time(2); afx.time_label(True)
afx.over_time(d, "temperature", stat="max")
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.show_panel("Charts")

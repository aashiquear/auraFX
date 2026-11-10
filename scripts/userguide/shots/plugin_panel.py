import aurafx as afx
# A plugin panel defined in Python (as a plugin file would), opened from Windows › Plugin panels.
@afx.panel("Scale tools")
def scale_tools(ui):
    ui.label("Scale the selected object")
    f = ui.slider("factor", 2.0, 0.1, 10.0)
    axis = ui.choice("axes", ["all", "x", "y", "z"])
    if ui.button("Apply"):
        s = [f if axis in ("all", a) else 1 for a in "xyz"]
        afx.active().transform(scale=s)
c = afx.cone()
afx.view("iso"); afx.reset_camera()
afx.show_panel("plugin Scale tools")

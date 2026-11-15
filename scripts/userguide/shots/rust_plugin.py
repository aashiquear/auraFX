import aurafx as afx, os, subprocess
# The example Rust plugin (crates/aurafx-plugin-example), built and loaded; its Spiral source,
# scaled by its Scale Points filter; the Plugins window lists it.
subprocess.run(["cargo", "build", "--release", "--quiet", "-p", "aurafx-plugin-example"], cwd=ROOT, check=True)
lib = os.path.join(ROOT, "target", "release", "libaurafx_plugin_example.so")
afx.load_plugin(lib)
s = afx.spiral(points=400, turns=4)
s.set(color_by="Angle", cmap="Viridis", line_width=3)
b = s.scale_points(factor=1.5, center=[0, 0, 0])
b.set(color_by="Angle", cmap="Plasma", line_width=3)
afx.view("iso"); afx.reset_camera()
afx.show_panel("plugins")

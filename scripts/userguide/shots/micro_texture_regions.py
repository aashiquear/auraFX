import aurafx as afx
# Alpha-titanium map: grains, then micro-texture regions (c axes within 20 degrees).
m = afx.load(ROOT + "/testdata/ebsd/ti_map.ctf")
g = m.segment_features(tolerance=5)
r = g.identify_micro_texture_regions(tolerance=20)
r.set(color_by="MTRIds", cmap="Turbo")
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT)

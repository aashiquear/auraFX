import aurafx as afx
# A noisy map (8 % of the points spoilt) and the same map after Neighbor Orientation Correlation.
m = afx.load(ROOT + "/testdata/ebsd/noisy_map.ang")
c = m.orientation_colors()
clean = m.neighbor_orientation_correlation(confidence="Confidence Index", min_confidence=0.1, tolerance=5, level=2).orientation_colors()
clean.set(color_by="IPFColor")
clean.send_to_view("right")
afx.active_view(0)
c.show()
c.set(color_by="IPFColor")
afx.link_views(True)
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT, views="3d")

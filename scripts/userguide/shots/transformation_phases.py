import aurafx as afx
# Voronoi grains with twin plates inserted in half of them, colored by feature.
v = afx.voronoi_grains(cells=[64, 64, 32], grains=40, seed=2)
t = v.insert_transformation_phases(parent_phase=1, new_phase=2, fraction=0.6, thickness=2.5, angle=60, seed=4)
t.set(color_by="Phases", cmap="Cool to Warm")
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

import aurafx as afx
# Points sampled on a cone, a turned and moved copy, and the copy aligned back by ICP.
target = afx.cone().point_sample_surface(samples=3000, seed=2)
moved = target.transform(rotate=[0, 25, 15], translate=[0.4, 0.2, 0])
aligned = afx.iterative_closest_point(moved, target, iterations=100)
target.set(solid_color=[0.55, 0.55, 0.55], point_size=4)
moved.set(solid_color=[0.95, 0.55, 0.15], point_size=4)
aligned.set(solid_color=[0.15, 0.45, 0.95], point_size=3)
for n in (target, moved, aligned):
    n.show()
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

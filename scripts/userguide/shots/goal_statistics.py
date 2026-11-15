import aurafx as afx
# A packed volume and the Goal Statistics panel editing it (goal curves over the measured bars).
g = afx.pack_grains(cells=[48, 48, 48], esd_mu=2.0, esd_sigma=0.2, ba=[12, 4], ca=[10, 6], calibrate=False, seed=3)
g.set(color_by="FeatureIds", cmap="Turbo")
afx.view("iso"); afx.reset_camera()
g.activate()
afx.show_panel("goal statistics")

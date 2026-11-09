# The Simulations panel on the DAMASK template with a Voronoi microstructure selected (no file
# needed): "Write case" makes grid.vti, material.yaml and load.yaml from it; a whole-window shot.
import aurafx as afx
v = afx.voronoi_grains(cells=[32, 32, 32], grains=40)
v.set(color_by="FeatureIds", cmap="Turbo")
v.activate()
afx.view([1.0, -0.8, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.simulation_template("damask")

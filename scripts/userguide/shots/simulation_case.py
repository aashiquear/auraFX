# The Simulations panel on the PINNACLE crystal-plasticity template, with a grain mesh selected
# (Plate Mesh with 30 Voronoi grains, no file needed); a whole-window shot.
import aurafx as afx
plate = afx.plate_mesh()
plate.hide()
poly = plate.assign_grains(grains=30, seed=1, blocks=True)
poly.set(color_by="grain_id", cmap="Turbo", representation="Surface With Edges", edge_color=(0.2, 0.2, 0.2))
poly.activate()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.simulation_template("pinnacle")

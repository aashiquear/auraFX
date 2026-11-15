# The Simulations panel on the JAX-FEM template, with a grain mesh selected (a box of hexahedra
# with 20 Voronoi grains, no file needed); a whole-window shot.
import aurafx as afx
box = afx.box_mesh(size=(4, 2, 1), cells=(24, 12, 6))
box.hide()
poly = box.assign_grains(grains=20, seed=1)
poly.set(color_by="grain_id", cmap="Turbo", representation="Surface With Edges", edge_color=(0.2, 0.2, 0.2))
poly.activate()
afx.view("iso"); afx.reset_camera()
afx.simulation_template("jaxfem")

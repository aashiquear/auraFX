# Tutorial: from geometry to a solver-ready polycrystal, in one script.
#   aurafx --script examples/polycrystal/polycrystal_case.py
# Builds a plate with a hole, meshes it with hexahedra, puts 60 Voronoi grains on the mesh
# (one element block per grain, a side set on the grain boundaries), checks the element
# quality, saves the mesh and the orientations, and writes a crystal-plasticity case for a
# MOOSE application (change "moose" to "pinnacle" for PINNACLE). Nothing here needs a solver;
# afx.run_case(...) would run it when one is installed.
import os
import aurafx as afx

out = os.path.abspath("polycrystal_out")
os.makedirs(out, exist_ok=True)

part = afx.geometry(output="Hex mesh", commands="""
brick x 4 y 2 z 0.5
cylinder radius 0.4 height 1
subtract body 2 from body 1
mesh body 1 size 0.1 scheme overlay
""")
part.hide()
poly = part.assign_grains(grains=60, blocks=True, boundaries=True, seed=3)
poly.set(color_by="grain_id", cmap="Turbo", representation="Surface With Edges")

quality = poly.mesh_quality(output="Summary table")
quality.hide()
print(quality.array("Min", "field") if "Min" in quality.arrays()["field"] else "quality table ready")

poly.save(os.path.join(out, "plate.e"))      # one block per grain, side sets per face
poly.save(os.path.join(out, "plate.tex"))    # one orientation per grain
case = afx.simulation_case(poly, "moose", os.path.join(out, "moose"), material="Nickel", axis="x", strain=0.02, steps=50)
print("case files:", [os.path.basename(f) for f in case["files"]])
print("run with:", case["command"])
afx.view([0.3, -0.8, 0.6], up=[0, 0, 1]); afx.reset_camera()

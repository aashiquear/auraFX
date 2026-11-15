# The PINNACLE result committed under testdata/solver/pinnacle_plate (user guide pictures):
# the plate with a hole of Plate Mesh (its defaults: 100 x 50 x 1, cells of 2.5, a hole of
# diameter 5), 30 Voronoi grains (seed 1), pulled along x to 2 % strain in 40 steps.
# Run with: CASE_DIR=<folder> aurafx --script scripts/cases/pinnacle_plate.py   (needs `pinnacle`)
# CELL=1 gives the finer plate of testdata/solver/pinnacle_plate_fine (5060 elements; about half
# an hour on a laptop), the one in the project page's clip and the guide's Sample pipelines.
import aurafx as afx, os
folder = os.path.abspath(os.environ.get("CASE_DIR", "pinnacle_plate"))
plate = afx.plate_mesh(cell=float(os.environ.get("CELL", "2.5")))
poly = plate.assign_grains(grains=30, seed=1, blocks=True)
jid = afx.run_case(poly, "pinnacle", folder, strain=0.02, steps=40, wait=True, auto_load=False)
open(os.path.join(folder, "state.txt"), "w").write(afx.job(jid)["state"] + "\n")

"""Reference meshes from hexaform (the user's Python hex mesher) for auraFX's structured mesher.

    <hexaform>/.venv/bin/python scripts/hexaform_reference.py <hexaform repository>

Builds a 12 x 12 x 12 background grid on the unit cube with four inclusions placed explicitly
(no random packing): a sphere, a rotated ellipsoid, a superquadric and a spherical-harmonic
grain, each in its own macro-box (placements in four_shapes.json). hexaform's `assemble`
meshes them (default template options) as HEX8 and HEX27; the raw arrays (points, cells,
block ids, side sets, node sets, particle_id) go to four_shapes_hex8.json.gz and
four_shapes_hex27.json.gz, and hexaform's Exodus writer writes four_shapes_hex8.e.
"""
import gzip, json, os, sys
import numpy as np

repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "src"))
from hexaform.background.cartesian import CartesianGrid
from hexaform.packing.rsa import PackedInclusion, box_cells_for_shape
from hexaform.shapes.library import Sphere, Ellipsoid, Superquadric, SphericalHarmonicGrain
from hexaform.assembly.builder import assemble
from hexaform.io import exodus

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "hexaform")
os.makedirs(out, exist_ok=True)
M = 12
grid = CartesianGrid.cube(1.0, M)
h = grid.h
c, s = np.cos(0.5), np.sin(0.5)
rot = np.array([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]])
coeffs = {(2, 0): 0.1, (3, 1): -0.05, (2, -2): 0.04}
specs = [
    ("sphere", {"radius": 0.12}, [1, 1, 1]),
    ("ellipsoid", {"semi_axes": [0.15, 0.1, 0.08], "rotation": rot.tolist()}, [6, 1, 1]),
    ("superquadric", {"semi_axes": [0.1, 0.1, 0.12], "exponent_xy": 4.0, "exponent_z": 3.0}, [1, 6, 6]),
    ("harmonic", {"radius": 0.11, "coefficients": [[l, m, a] for (l, m), a in coeffs.items()]}, [6, 6, 6]),
]


def make(kind, p, centre):
    if kind == "sphere":
        return Sphere(centre, p["radius"])
    if kind == "ellipsoid":
        return Ellipsoid(centre, np.array(p["semi_axes"]), np.array(p["rotation"]))
    if kind == "superquadric":
        return Superquadric(centre, np.array(p["semi_axes"]), p["exponent_xy"], p["exponent_z"])
    return SphericalHarmonicGrain(centre, p["radius"], {(l, m): a for l, m, a in p["coefficients"]})


inclusions = []
setup = []
for pid, (kind, p, lo) in enumerate(specs, start=1):
    k = box_cells_for_shape(make(kind, p, np.zeros(3)), h, 1.35)
    lo = np.array(lo)
    centre = grid.origin + (lo + k / 2.0) * h
    shape = make(kind, p, centre)
    inclusions.append(PackedInclusion(shape, lo, k, pid))
    setup.append({"kind": kind, "params": p, "box_lo": lo.tolist(), "k": k.tolist(), "centre": centre.tolist()})
with open(os.path.join(out, "four_shapes.json"), "w") as f:
    json.dump({"divisions": M, "length": 1.0, "inclusions": setup}, f, indent=1)

for et in ("HEX8", "HEX27"):
    mesh = assemble(grid, inclusions, element_type=et)
    data = {
        "points": mesh.points.ravel().tolist(),
        "cells": mesh.cells.ravel().tolist(),
        "nodes_per_element": int(mesh.cells.shape[1]),
        "block_id": mesh.block_id.tolist(),
        "block_names": {str(k): v for k, v in mesh.block_names.items()},
        "side_sets": {k: [e.tolist(), s_.tolist()] for k, (e, s_) in mesh.side_sets.items()},
        "node_sets": {k: v.tolist() for k, v in mesh.node_sets.items()},
        "particle_id": mesh.element_fields["particle_id"].tolist(),
    }
    with gzip.open(os.path.join(out, f"four_shapes_{et.lower()}.json.gz"), "wt") as f:
        json.dump(data, f)
    if et == "HEX8":
        exodus.write(mesh, os.path.join(out, "four_shapes_hex8.e"))
    print(et, mesh.n_nodes, "nodes", mesh.n_elements, "elements", {k: len(e) for k, (e, _) in mesh.side_sets.items()})
for n in sorted(os.listdir(out)):
    print(n, os.path.getsize(os.path.join(out, n)))

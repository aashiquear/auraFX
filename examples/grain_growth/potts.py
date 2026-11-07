"""Grain growth with the Potts model: a 2-D lattice of grain ids (from Voronoi seeds) evolves by
Monte Carlo spin flips that lower the boundary energy, written as VTK files every few sweeps.

Run it from auraFX's Simulations panel (template: Python pipeline, input: potts.py), or:
    afx.submit("python3 -u potts.py", workdir="examples/grain_growth",
               outputs="grains_*.vtk", auto_load=True)
Color the result by grain_id with a banded or categorical color map and play the steps.

    python3 potts.py [N] [grains] [sweeps] [every] [kT]      (defaults 256, 600, 300, 5, 0.3)
"""
import sys
import numpy as np

N = int(sys.argv[1]) if len(sys.argv) > 1 else 256
Q = int(sys.argv[2]) if len(sys.argv) > 2 else 600
sweeps = int(sys.argv[3]) if len(sys.argv) > 3 else 300
every = int(sys.argv[4]) if len(sys.argv) > 4 else 5
kT = float(sys.argv[5]) if len(sys.argv) > 5 else 0.3

rng = np.random.default_rng(3)
# Voronoi seeds give the starting grains (ids 1..Q).
seeds = rng.uniform(0, N, (Q, 2))
yy, xx = np.mgrid[0:N, 0:N]
S = np.empty((N, N), dtype=np.int32)
for j in range(0, N, 32):
    d = (xx[j:j + 32, :, None] - seeds[None, None, :, 0]) ** 2 + (yy[j:j + 32, :, None] - seeds[None, None, :, 1]) ** 2
    S[j:j + 32] = d.argmin(axis=2) + 1

OFFS = [(dy, dx) for dy in (-1, 0, 1) for dx in (-1, 0, 1) if (dy, dx) != (0, 0)]


def neighbors(a):
    return [np.roll(np.roll(a, dy, 0), dx, 1) for dy, dx in OFFS]


def unlike(spin, nbrs):
    return sum((spin != n).astype(np.int8) for n in nbrs)


mask = ((yy + xx) % 2 == 0)      # checkerboard sublattices: sites of one color do not touch


def write(sweep):
    name = f"grains_{sweep:04d}.vtk"
    with open(name, "w") as f:
        f.write("# vtk DataFile Version 3.0\nPotts grain growth\nASCII\nDATASET STRUCTURED_POINTS\n")
        f.write(f"DIMENSIONS {N + 1} {N + 1} 1\nORIGIN 0 0 0\nSPACING 1 1 1\nCELL_DATA {N * N}\n")
        f.write("SCALARS grain_id int 1\nLOOKUP_TABLE default\n")
        np.savetxt(f, S, fmt="%d")
    print(f"sweep {sweep}: wrote {name}, {len(np.unique(S))} grains", flush=True)


for sweep in range(sweeps + 1):
    if sweep % every == 0:
        write(sweep)
    for sub in (mask, ~mask):
        nb = neighbors(S)
        pick = np.stack(nb, 0)[rng.integers(0, 8, (N, N)), yy, xx]   # a random neighbor's id
        dE = unlike(pick, nb) - unlike(S, nb)
        accept = (dE <= 0) | (rng.random((N, N)) < np.exp(-np.maximum(dE, 0) / kT))
        S = np.where(sub & accept, pick, S)

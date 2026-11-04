"""Tiny 2-D heat diffusion 'simulation' that writes VTK image files each output step.

Run it from auraFX's Simulations panel (template: Python pipeline, input: heat.py) or:
    afx.submit("python3 -u heat.py", workdir="examples/heat2d", outputs="heat_*.vtk", auto_load=True)
"""
import math, sys

N = int(sys.argv[1]) if len(sys.argv) > 1 else 64
steps, every, alpha = 400, 50, 0.24
T = [[0.0] * N for _ in range(N)]
for j in range(N):
    for i in range(N):
        r = math.hypot(i - N * 0.35, j - N * 0.5)
        T[j][i] = 100.0 if r < N * 0.12 else 0.0

def write(k):
    name = f"heat_{k:04d}.vtk"
    with open(name, "w") as f:
        f.write("# vtk DataFile Version 3.0\nheat\nASCII\nDATASET STRUCTURED_POINTS\n")
        f.write(f"DIMENSIONS {N} {N} 1\nORIGIN 0 0 0\nSPACING 1 1 1\nPOINT_DATA {N*N}\nSCALARS temperature float 1\nLOOKUP_TABLE default\n")
        for row in T:
            f.write(" ".join(f"{v:.4f}" for v in row) + "\n")
    print(f"step {k}: wrote {name}, max T = {max(max(r) for r in T):.3f}", flush=True)

for k in range(steps + 1):
    if k % every == 0:
        write(k)
    new = [row[:] for row in T]
    for j in range(1, N - 1):
        for i in range(1, N - 1):
            new[j][i] = T[j][i] + alpha * (T[j][i+1] + T[j][i-1] + T[j+1][i] + T[j-1][i] - 4 * T[j][i])
    # Hot boundary on the right edge.
    for j in range(N):
        new[j][N - 1] = 50.0
    T = new
print("done", flush=True)

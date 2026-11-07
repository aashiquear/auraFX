"""Spinodal decomposition: the Cahn-Hilliard equation on a periodic 2-D grid, solved with a
semi-implicit spectral scheme in NumPy, writing a VTK file every few steps.

Run it from auraFX's Simulations panel (template: Python pipeline, input: cahn_hilliard.py), or:
    afx.submit("python3 -u cahn_hilliard.py", workdir="examples/phase_field",
               outputs="ch_*.vtk", auto_load=True)
Then play the time steps, or afx.save_animation("spinodal.mp4", fps=12).

    python3 cahn_hilliard.py [N] [steps] [every]      (defaults 128, 3000, 50)
"""
import sys
import numpy as np

N = int(sys.argv[1]) if len(sys.argv) > 1 else 128
steps = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
every = int(sys.argv[3]) if len(sys.argv) > 3 else 50
dt, kappa, W, M = 0.5, 1.0, 1.0, 1.0

rng = np.random.default_rng(7)
c = 0.5 + 0.02 * rng.standard_normal((N, N))
k = 2 * np.pi * np.fft.fftfreq(N)
k2 = k[:, None] ** 2 + k[None, :] ** 2
denom = 1.0 + dt * M * kappa * k2 ** 2


def write(step):
    name = f"ch_{step:05d}.vtk"
    with open(name, "w") as f:
        f.write("# vtk DataFile Version 3.0\nCahn-Hilliard concentration\nASCII\nDATASET STRUCTURED_POINTS\n")
        f.write(f"DIMENSIONS {N} {N} 1\nORIGIN 0 0 0\nSPACING 1 1 1\nPOINT_DATA {N * N}\n")
        f.write("SCALARS concentration float 1\nLOOKUP_TABLE default\n")
        np.savetxt(f, c, fmt="%.4f")
    print(f"step {step}: wrote {name}, c in [{c.min():.3f}, {c.max():.3f}]", flush=True)


for n in range(steps + 1):
    if n % every == 0:
        write(n)
    mu = 2 * W * c * (1 - c) * (1 - 2 * c)            # bulk chemical potential
    c = np.real(np.fft.ifft2((np.fft.fft2(c) - dt * M * k2 * np.fft.fft2(mu)) / denom))

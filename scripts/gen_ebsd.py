"""Small synthetic EBSD maps for the tests and the user guide: testdata/ebsd/.

    python3 scripts/gen_ebsd.py [outdir]

- ni_map.ang: a TSL/EDAX .ang map of nickel (cubic m-3m), 80 x 60 points on a square grid with a
  0.5 um step, 14 grains (Voronoi cells of seeded random points) with random orientations, a
  small orientation gradient inside each grain, image quality and confidence index (low near
  grain boundaries). The phase column is 0, as single-phase scans write it.
- two_phase.ang: 60 x 40 points, 2 um step, an austenite phase (cubic m-3m) and an alpha
  titanium phase (hexagonal 6/mmm), some points not indexed (phase 0, CI -1).
- ti_map.ctf: an Oxford .ctf map of alpha titanium (hexagonal 6/mmm, Laue group 9), 70 x 50
  points, 0.4 um step, angles in degrees with the hexagonal a axis along x rotated by -30
  degrees on phi2 (Oxford's convention), some points not indexed (phase 0, error 3).
- ebsd_expected.json: for each file, the grain of every point and, for a sample of points, the
  Euler angles as written and the expected values after reading (radians; phi2 + 30 degrees
  for the hexagonal phase of the .ctf file).
"""
import json, os, sys
import numpy as np

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "ebsd")
os.makedirs(out, exist_ok=True)


def random_euler(rng, n):
    """Uniform random orientations as Bunge angles (phi1, Phi, phi2) in radians."""
    return np.c_[rng.uniform(0, 2 * np.pi, n), np.arccos(rng.uniform(-1, 1, n)), rng.uniform(0, 2 * np.pi, n)]


def grains(nx, ny, n, rng):
    seeds = np.c_[rng.uniform(0, nx, n), rng.uniform(0, ny, n)]
    jj, ii = np.mgrid[0:ny, 0:nx]
    d = (ii[..., None] + 0.5 - seeds[:, 0]) ** 2 + (jj[..., None] + 0.5 - seeds[:, 1]) ** 2
    g = d.argmin(-1)
    s = np.sort(d, -1)
    edge = np.sqrt(s[..., 1]) - np.sqrt(s[..., 0])  # distance (in cells) to the nearest boundary
    return g, edge


def ang_header(phases, step, nx, ny):
    h = ["# TEM_PIXperUM          1.000000", "# x-star                0.500000", "# y-star                0.500000",
         "# z-star                0.500000", "# WorkingDistance       15.000000", "#"]
    for k, (name, formula, sym, lat) in enumerate(phases, 1):
        h += [f"# Phase {k}", f"# MaterialName  \t{name}", f"# Formula     \t{formula}", "# Info \t\t", f"# Symmetry              {sym}",
              "# LatticeConstants      " + " ".join(f"{v:.3f}" for v in lat), "# NumberFamilies        2",
              "# hklFamilies   \t 1  1  1 1 0.000000 1", "# hklFamilies   \t 2  0  0 1 0.000000 1",
              "# ElasticConstants \t0.000000 0.000000 0.000000 0.000000 0.000000 0.000000", "# Categories0 0 0 0 0 ", "#"]
    h += ["# GRID: SqrGrid", f"# XSTEP: {step:.6f}", f"# YSTEP: {step:.6f}", f"# NCOLS_ODD: {nx}", f"# NCOLS_EVEN: {nx}", f"# NROWS: {ny}",
          "#", "# OPERATOR: \tauraFX", "#", "# SAMPLEID: \t", "#", "# SCANID: \t", "#"]
    return "\n".join(h) + "\n"


expected = {}

# ------------------------------------------------------------------ ni_map.ang
rng = np.random.default_rng(42)
nx, ny, step = 80, 60, 0.5
g, edge = grains(nx, ny, 14, rng)
eul = random_euler(rng, 14)
jj, ii = np.mgrid[0:ny, 0:nx]
# A small gradient inside each grain: phi1 turns by up to 2 degrees across the map.
e = eul[g].copy()
e[..., 0] = (e[..., 0] + np.radians(2.0) * ii / nx) % (2 * np.pi)
iq = 3000 + 1500 * np.tanh(edge / 2.0) + rng.normal(0, 40, g.shape)
ci = np.clip(0.1 + 0.85 * np.tanh(edge / 1.5) + rng.normal(0, 0.03, g.shape), 0.02, 1.0)
fit = np.clip(0.4 + 0.6 * (1 - np.tanh(edge / 2.0)), 0, 3)
lines = []
for j in range(ny):
    for i in range(nx):
        lines.append(f"{e[j, i, 0]:9.5f} {e[j, i, 1]:9.5f} {e[j, i, 2]:9.5f} {i * step:12.5f} {j * step:12.5f} {iq[j, i]:8.1f} {ci[j, i]:6.3f}  0 {800 + g[j, i]:6d} {fit[j, i]:6.3f}")
with open(os.path.join(out, "ni_map.ang"), "w") as f:
    f.write(ang_header([("Nickel", "Ni", 43, [3.524, 3.524, 3.524, 90, 90, 90])], step, nx, ny))
    f.write("\n".join(lines) + "\n")
written = np.round(e, 5)
sample = [(i, j) for j in range(0, ny, 7) for i in range(0, nx, 9)]
expected["ni_map.ang"] = {"nx": nx, "ny": ny, "step": step, "grain": g.ravel().tolist(),
                          "points": [{"i": i, "j": j, "euler": written[j, i].tolist(), "phase": 1, "ci": round(float(ci[j, i]), 3)} for i, j in sample]}

# ------------------------------------------------------------------ two_phase.ang
rng = np.random.default_rng(7)
nx, ny, step = 60, 40, 2.0
g, edge = grains(nx, ny, 10, rng)
eul = random_euler(rng, 10)
phase_of = np.where(np.arange(10) % 3 == 0, 2, 1)  # every third grain is alpha titanium
ph = phase_of[g]
bad = (edge < 0.35) & (rng.uniform(size=g.shape) < 0.6)
lines = []
for j in range(ny):
    for i in range(nx):
        if bad[j, i]:
            lines.append(f"{4 * np.pi:9.5f} {4 * np.pi:9.5f} {4 * np.pi:9.5f} {i * step:12.5f} {j * step:12.5f} {0.0:8.1f} {-1.0:6.3f}  0 {0:6d} {180.0:6.3f}")
        else:
            a = eul[g[j, i]]
            lines.append(f"{a[0]:9.5f} {a[1]:9.5f} {a[2]:9.5f} {i * step:12.5f} {j * step:12.5f} {2500.0:8.1f} {0.9:6.3f} {ph[j, i]:2d} {1000:6d} {0.5:6.3f}")
with open(os.path.join(out, "two_phase.ang"), "w") as f:
    f.write(ang_header([("Iron (Gamma)", "Fe", 43, [3.6, 3.6, 3.6, 90, 90, 90]), ("Titanium (Alpha)", "Ti", 62, [2.95, 2.95, 4.68, 90, 90, 120])], step, nx, ny))
    f.write("\n".join(lines) + "\n")
written = np.round(eul, 5)
sample = [(i, j) for j in range(0, ny, 5) for i in range(0, nx, 7)]
expected["two_phase.ang"] = {"nx": nx, "ny": ny, "step": step, "grain": g.ravel().tolist(),
                             "points": [{"i": i, "j": j, "euler": (written[g[j, i]].tolist() if not bad[j, i] else [round(4 * np.pi, 5)] * 3), "phase": int(0 if bad[j, i] else ph[j, i])} for i, j in sample]}

# ------------------------------------------------------------------ ti_map.ctf
rng = np.random.default_rng(11)
nx, ny, step = 70, 50, 0.4
g, edge = grains(nx, ny, 12, rng)
eul = random_euler(rng, 12)          # the orientations with a1 = [2-1-10] along x
oxford = eul.copy()
oxford[:, 2] = (oxford[:, 2] - np.radians(30.0)) % (2 * np.pi)   # as Oxford writes them
bad = (edge < 0.3) & (rng.uniform(size=g.shape) < 0.5)
mad = np.clip(0.3 + 0.5 * (1 - np.tanh(edge)), 0, 2)
rows = []
for j in range(ny):
    for i in range(nx):
        if bad[j, i]:
            rows.append(f"0\t{i * step:.4f}\t{j * step:.4f}\t0\t3\t0.0000\t0.0000\t0.0000\t0.0000\t0\t0")
        else:
            a = np.degrees(oxford[g[j, i]])
            rows.append(f"1\t{i * step:.4f}\t{j * step:.4f}\t{8 + g[j, i] % 3}\t0\t{a[0]:.4f}\t{a[1]:.4f}\t{a[2]:.4f}\t{mad[j, i]:.4f}\t{120 + g[j, i]}\t{150 + 2 * g[j, i]}")
head = ["Channel Text File", "Prj\tauraFX test map", "Author\t[Unknown]", "JobMode\tGrid", f"XCells\t{nx}", f"YCells\t{ny}",
        f"XStep\t{step}", f"YStep\t{step}", "AcqE1\t0", "AcqE2\t0", "AcqE3\t0",
        "Euler angles refer to Sample Coordinate system (CS0)!\tMag\t1000\tCoverage\t100\tDevice\t0\tKV\t20\tTiltAngle\t70\tTiltAxis\t0",
        "Phases\t1", "2.95;2.95;4.68\t90;90;120\tTitanium\t9\t194\t\t\t",
        "Phase\tX\tY\tBands\tError\tEuler1\tEuler2\tEuler3\tMAD\tBC\tBS"]
with open(os.path.join(out, "ti_map.ctf"), "w") as f:
    f.write("\n".join(head + rows) + "\n")
sample = [(i, j) for j in range(0, ny, 6) for i in range(0, nx, 8)]
pts = []
for i, j in sample:
    if bad[j, i]:
        pts.append({"i": i, "j": j, "euler": [0.0, 0.0, 0.0], "phase": 0})
    else:
        w = np.radians(np.round(np.degrees(oxford[g[j, i]]), 4))
        aligned = [float(w[0]), float(w[1]), float((w[2] + np.radians(30.0)) % (2 * np.pi))]
        pts.append({"i": i, "j": j, "euler": aligned, "phase": 1})
expected["ti_map.ctf"] = {"nx": nx, "ny": ny, "step": step, "grain": g.ravel().tolist(), "points": pts}

with open(os.path.join(out, "ebsd_expected.json"), "w") as f:
    json.dump(expected, f)
for n in sorted(os.listdir(out)):
    print(n, os.path.getsize(os.path.join(out, n)))

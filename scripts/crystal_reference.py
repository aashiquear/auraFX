"""Reference values for the orientation mathematics: testdata/crystal/reference.json.

    python3 scripts/crystal_reference.py [AM-Orientation-Shift-TOOLKIT folder]

Computed independently of auraFX with numpy and scipy (1.17):
- conversions: Bunge angles -> quaternion and orientation matrix through
  scipy.spatial.transform.Rotation ('ZXZ' intrinsic rotation, transposed: the orientation matrix
  is the passive sample -> crystal rotation);
- disorientation angles for every Laue class by brute force over both sides of the symmetry
  group, with the groups from scipy's Rotation.create_group (C1, C2 about y, D2, C4, D4, C3, D3,
  C6, D6, T, O);
- inverse pole figure colors of cubic m-3m and hexagonal 6/mmm orientations, the direction being
  reduced to the standard triangle by sorting components (cubic) or folding the azimuth
  (hexagonal) instead of searching the group.
If the folder of the AM orientation toolkit is given, its cubic disorientation
(am_validation_metrics.disorientation_deg) is checked against the brute force first.
"""
import json, os, sys
import numpy as np
from scipy.spatial.transform import Rotation as R

rng = np.random.default_rng(2026)
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "crystal", "reference.json")


def euler_list(n):
    e = np.c_[rng.uniform(0, 2 * np.pi, n), np.arccos(rng.uniform(-1, 1, n)), rng.uniform(0, 2 * np.pi, n)]
    special = [[0, 0, 0], [0.7, 0, 0], [1.1, np.pi, 0.3], [np.pi / 4, np.arccos(1 / np.sqrt(3)), 0], [0, np.pi / 2, np.pi / 4]]
    return np.r_[np.array(special, float), e]


def g_of(e):
    """Orientation matrix (sample -> crystal) of Bunge angles."""
    return R.from_euler("ZXZ", e).as_matrix().transpose(0, 2, 1)


GROUPS = {
    "Triclinic -1": R.identity(1), "Monoclinic 2/m": R.create_group("C2", axis="Y"), "Orthorhombic mmm": R.create_group("D2"),
    "Tetragonal 4/m": R.create_group("C4"), "Tetragonal 4/mmm": R.create_group("D4"), "Trigonal -3": R.create_group("C3"),
    "Trigonal -3m": R.create_group("D3"), "Hexagonal 6/m": R.create_group("C6"), "Hexagonal 6/mmm": R.create_group("D6"),
    "Cubic m-3": R.create_group("T"), "Cubic m-3m": R.create_group("O"),
}


def disorientation(g1, g2, ops):
    d = g2 @ g1.T
    best = np.pi
    for a in ops:
        for b in ops:
            m = a @ d @ b.T
            best = min(best, np.arccos(np.clip((np.trace(m) - 1) / 2, -1, 1)))
    return best


def color(eta, chi, eta0, eta1, chimax):
    f = np.clip(chi / chimax, 0, 1)
    b = np.clip((eta - eta0) / (eta1 - eta0), 0, 1)
    rgb = np.sqrt(np.array([1 - f, (1 - b) * f, b * f]))
    return rgb / rgb.max()


def ipf_cubic(c):
    a = np.sort(np.abs(c / np.linalg.norm(c)))  # y <= x <= z in the triangle [001]-[101]-[111]
    y, x, z = a
    eta, chi = np.arctan2(y, x), np.arccos(z)
    return color(eta, chi, 0, np.pi / 4, np.arccos(np.sqrt(1 / (2 + np.tan(eta) ** 2))))


def ipf_hex(c):
    c = c / np.linalg.norm(c)
    if c[2] < 0:
        c = -c
    eta = np.arctan2(c[1], c[0]) % (np.pi / 3)
    if eta > np.pi / 6:
        eta = np.pi / 3 - eta
    return color(eta, np.arccos(np.clip(c[2], -1, 1)), 0, np.pi / 6, np.pi / 2)


E = euler_list(25)
G = g_of(E)
Q = R.from_euler("ZXZ", E).inv().as_quat()  # (x, y, z, w) of the rotation whose matrix is g
Q = np.c_[Q[:, 3], Q[:, :3]]
Q[Q[:, 0] < 0] *= -1
ref = {"conversions": [{"euler": e.tolist(), "quat": q.tolist(), "matrix": g.ravel().tolist()} for e, q, g in zip(E, Q, G)]}

ref["disorientation"] = {}
E1, E2 = euler_list(12)[5:], euler_list(12)[5:]
G1, G2 = g_of(E1), g_of(E2)
for name, grp in GROUPS.items():
    ops = grp.as_matrix()
    ref["disorientation"][name] = [{"e1": a.tolist(), "e2": b.tolist(), "angle": disorientation(g1, g2, ops)} for a, b, g1, g2 in zip(E1, E2, G1, G2)]

if len(sys.argv) > 1:
    sys.path.insert(0, sys.argv[1])
    import am_validation_metrics as vm
    tk = np.radians(vm.disorientation_deg(G1, G2))
    bf = np.array([r["angle"] for r in ref["disorientation"]["Cubic m-3m"]])
    print("toolkit cubic disorientation, largest difference (rad):", np.abs(tk - bf).max())
    assert np.abs(tk - bf).max() < 1e-9

E3 = euler_list(40)
ref["ipf"] = []
for e, g in zip(E3, g_of(E3)):
    for d in ([0, 0, 1], [1, 0, 0], [0, 1, 0]):
        c = g @ np.array(d, float)
        ref["ipf"].append({"euler": e.tolist(), "direction": d, "cubic": ipf_cubic(c).tolist(), "hexagonal": ipf_hex(c).tolist()})

os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w") as f:
    json.dump(ref, f)
print(out, os.path.getsize(out))

# The GBCD of a synthetic volume rich in twins (no file needed): 80 Voronoi grains on a
# 48-cube, each cut into lamellae 3 cells thick along one of its {111} planes, alternating
# between the grain's orientation and its Sigma 3 twin (60 degrees about that [111]). Every
# lamella boundary is a coherent twin. After smoothing the staircase boundaries (60 Taubin
# passes), the GBCD section for Sigma 3 (boundary planes in multiples of random,
# stereographic, crystal [001] at the centre) peaks at (111).
import aurafx as afx
import numpy as np

v = afx.voronoi_grains(cells=[48, 48, 48], grains=80, seed=2)
v.update()
ids = np.asarray(v.array("FeatureIds")).astype(np.int64).ravel()
eul = np.asarray(v.array("EulerAngles"), dtype=np.float64).reshape(-1, 3)


def bunge(e):
    """Orientation matrix (sample -> crystal) of Bunge angles."""
    c1, s1, c, s, c2, s2 = np.cos(e[0]), np.sin(e[0]), np.cos(e[1]), np.sin(e[1]), np.cos(e[2]), np.sin(e[2])
    return np.array([[c1 * c2 - s1 * s2 * c, s1 * c2 + c1 * s2 * c, s2 * s],
                     [-c1 * s2 - s1 * c2 * c, -s1 * s2 + c1 * c2 * c, c2 * s],
                     [s1 * s, -c1 * s, c]])


def euler(g):
    Phi = np.arccos(np.clip(g[2, 2], -1, 1))
    if abs(np.sin(Phi)) > 1e-9:
        return np.array([np.arctan2(g[2, 0], -g[2, 1]) % (2 * np.pi), Phi, np.arctan2(g[0, 2], g[1, 2]) % (2 * np.pi)])
    return np.array([np.arctan2(g[0, 1], g[0, 0]) % (2 * np.pi), Phi, 0.0])


def rot(axis, a):
    k = np.asarray(axis, float) / np.linalg.norm(axis)
    K = np.array([[0, -k[2], k[1]], [k[2], 0, -k[0]], [-k[1], k[0], 0]])
    return np.eye(3) + np.sin(a) * K + (1 - np.cos(a)) * K @ K


n = 48
ii, jj, kk = np.meshgrid(np.arange(n), np.arange(n), np.arange(n), indexing="ij")
centres = np.stack([ii.ravel(order="F"), jj.ravel(order="F"), kk.ravel(order="F")], 1) + 0.5
new_ids = np.zeros_like(ids)
new_eul = np.zeros_like(eul)
next_id = 1
for f in np.unique(ids):
    m = ids == f
    g = bunge(eul[np.argmax(m)])
    c = np.array([1.0, 1.0, 1.0]) / np.sqrt(3)
    gt = rot(c, np.pi / 3) @ g
    nrm = g.T @ c                       # the twin plane's normal in the sample
    layer = np.floor(centres[m] @ nrm / 3.0).astype(np.int64)
    layer -= layer.min()
    new_ids[m] = next_id + layer
    e0, e1 = euler(g), euler(gt)
    new_eul[m] = np.where((layer % 2 == 0)[:, None], e0, e1)
    next_id += int(layer.max()) + 1
v.add_array("FeatureIds", new_ids.astype(np.int32), "cells")
v.add_array("EulerAngles", new_eul.astype(np.float32), "cells")
v.hide()
s = v.quick_surface_mesh()
s.hide()
m = s.smooth_boundaries(iterations=60)
m.hide()
g = m.gbcd_section(angle=60, axis=(1, 1, 1), tolerance=5)
g.set(color_by="MRD", cmap="Viridis")
afx.annotate("Sigma 3 (60 deg about [111]): boundary planes, multiples of random", "upper left", 16)
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)

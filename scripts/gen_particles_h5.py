"""Write testdata/particles/: an H5Part file (h5py) and openPMD series (openpmd_api, HDF5 backend,
one with the iterations as groups of one file, one with a file per iteration). Run with the Python
of ParaView 6.1.1, which has h5py and openpmd_api:

    (ulimit -v 8000000; timeout 180 pvpython scripts/gen_particles_h5.py)

Particle k of step s sits at (k, 2k, 3k) + s * (0.5, 0, 0) with id k and energy 10 k + s; the
mesh value at grid point (i, j, k) of iteration s is 100 k + 10 j + i + 1000 s (on a 4 x 3 x 2
grid, spacing 0.5 x 1 x 2, offset 1 2 3).
"""
import os

import h5py
import numpy as np
import openpmd_api as io

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "particles")
N = 6


def h5part():
    with h5py.File(os.path.join(OUT, "steps.h5part"), "w") as f:
        for s in range(3):
            g = f.create_group(f"Step#{s}")
            g.attrs["TimeValue"] = 0.25 * s
            k = np.arange(N, dtype=np.float64)
            g["x"] = k + 0.5 * s
            g["y"] = 2 * k
            g["z"] = 3 * k
            g["id"] = np.arange(N, dtype=np.int64)
            g["energy"] = 10 * k + s


def field(s):
    k, j, i = np.meshgrid(np.arange(2), np.arange(3), np.arange(4), indexing="ij")
    return (100 * k + 10 * j + i + 1000 * s).astype(np.float64)


def openpmd(name, encoding):
    series = io.Series(os.path.join(OUT, name), io.Access.create)
    series.set_attribute("software", "auraFX test generator")
    for s in (0, 10):
        it = series.iterations[s]
        it.time = 0.5 * s
        it.time_unit_SI = 1e-15
        rho = it.meshes["rho"][io.Mesh_Record_Component.SCALAR]
        m = it.meshes["rho"]
        m.axis_labels = ["z", "y", "x"]
        m.grid_spacing = [2.0, 1.0, 0.5]
        m.grid_global_offset = [3.0, 2.0, 1.0]
        m.grid_unit_SI = 1.0
        data = field(s // 10)
        rho.reset_dataset(io.Dataset(data.dtype, data.shape))
        rho.store_chunk(data)
        e = it.particles["electrons"]
        k = np.arange(N, dtype=np.float64)
        for c, v in (("x", k), ("y", 2 * k), ("z", 3 * k)):
            pc = e["position"][c]
            pc.reset_dataset(io.Dataset(v.dtype, v.shape))
            pc.store_chunk(np.ascontiguousarray(v))
            off = e["positionOffset"][c]
            off.reset_dataset(io.Dataset(np.dtype("float64"), [N]))
            off.make_constant(0.5 * (s // 10) if c == "x" else 0.0)
        en = e["energy"][io.Record_Component.SCALAR]
        ev = 10 * k + s // 10
        en.reset_dataset(io.Dataset(ev.dtype, ev.shape))
        en.store_chunk(ev)
        series.flush()
    series.close()


def main():
    os.makedirs(OUT, exist_ok=True)
    h5part()
    openpmd("group_based.h5", "groupBased")
    openpmd("file_%T.h5", "fileBased")
    print("wrote", os.path.normpath(OUT))


main()

#!/usr/bin/env python3
"""Write testdata/rotating.pvd: a time series of 3D image data with a rigid rotation about z
whose angular speed grows with time, w(t) = 1 + t. A particle released at time 0 at angle a0
is at angle a0 + t + t^2/2 at time t (radius unchanged), which the temporal filter tests use.

Velocity is linear in space and in time, so trilinear and linear-in-time interpolation are
exact. Arrays: point vector "velocity", point scalar "T" = x + t, cell scalar "id"."""
import os

DIMS = (21, 21, 3)
ORIGIN = (-1.0, -1.0, -0.1)
SPACING = (0.1, 0.1, 0.1)
TIMES = [0.25 * k for k in range(9)]  # 0 .. 2

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata")
os.makedirs(os.path.join(root, "rotating"), exist_ok=True)
ext = f"0 {DIMS[0]-1} 0 {DIMS[1]-1} 0 {DIMS[2]-1}"
entries = []
for k, t in enumerate(TIMES):
    w = 1.0 + t
    vel, temp = [], []
    for kk in range(DIMS[2]):
        for j in range(DIMS[1]):
            for i in range(DIMS[0]):
                x = ORIGIN[0] + i * SPACING[0]
                y = ORIGIN[1] + j * SPACING[1]
                vel += [-w * y, w * x, 0.0]
                temp.append(x + t)
    ncells = (DIMS[0] - 1) * (DIMS[1] - 1) * (DIMS[2] - 1)
    fmt = lambda v: " ".join(f"{x:.9g}" for x in v)
    name = f"rotating/rot_{k}.vti"
    with open(os.path.join(root, name), "w") as f:
        f.write('<?xml version="1.0"?>\n<VTKFile type="ImageData" version="1.0" byte_order="LittleEndian">\n')
        f.write(f'  <ImageData WholeExtent="{ext}" Origin="{fmt(ORIGIN)}" Spacing="{fmt(SPACING)}">\n')
        f.write(f'    <Piece Extent="{ext}">\n      <PointData Scalars="T" Vectors="velocity">\n')
        f.write(f'        <DataArray type="Float64" Name="velocity" NumberOfComponents="3" format="ascii">{fmt(vel)}</DataArray>\n')
        f.write(f'        <DataArray type="Float64" Name="T" format="ascii">{fmt(temp)}</DataArray>\n')
        f.write('      </PointData>\n      <CellData>\n')
        f.write(f'        <DataArray type="Int32" Name="id" format="ascii">{" ".join(str(c) for c in range(ncells))}</DataArray>\n')
        f.write('      </CellData>\n    </Piece>\n  </ImageData>\n</VTKFile>\n')
    entries.append(f'<DataSet timestep="{t:g}" file="{name}"/>')
with open(os.path.join(root, "rotating.pvd"), "w") as f:
    f.write('<?xml version="1.0"?>\n<VTKFile type="Collection" version="0.1"><Collection>' + "".join(entries) + "</Collection></VTKFile>\n")
print("wrote", len(TIMES), "steps")

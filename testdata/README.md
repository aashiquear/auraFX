# Test data

Small files are committed. Larger ones are ignored by git and can be recreated:

| File | How to get it |
|---|---|
| `can.ex2`, `bake.e` | Copy from a ParaView install: `share/paraview-*/examples/` |
| `can_nc4.exo`, `disk_out_ref_nc4.exo` | NetCDF-4 (HDF5, zlib) copies of the ParaView Exodus samples, made with the netCDF4 Python module (`disk_out_ref_nc4.exo` is committed) |
| `fcc_melt.dump` | `python3 scripts/gen_lammps_dump.py` |
| VTK samples (`*.vt?`, `*.vtk`, `series/`, `group/`) | Written by ParaView 6.1.1 (`pvpython`), committed |
| `lagrange_annulus.vtu`, `bezier_annulus.vtu`, `lagrange_shapes.vtu`, `quadratic_shapes.vtu` | `python3 scripts/gen_higher_order.py` (curved Lagrange/Bezier cells; ParaView reference values in `crates/aurafx-filters/tests/filters.rs`), committed |
| `rotating.pvd`, `rotating/` | `python3 scripts/gen_rotating_flow.py` (rotation with speed 1 + t; exact particle paths known), committed |

Tests that need a missing file skip themselves.

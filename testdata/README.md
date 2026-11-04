# Test data

Small files are committed. Larger ones are ignored by git and can be recreated:

| File | How to get it |
|---|---|
| `can.ex2`, `bake.e` | Copy from a ParaView install: `share/paraview-*/examples/` |
| `fcc_melt.dump` | `python3 scripts/gen_lammps_dump.py` |
| VTK samples (`*.vt?`, `*.vtk`, `series/`, `group/`) | Written by ParaView 6.1.1 (`pvpython`), committed |

Tests that need a missing file skip themselves.

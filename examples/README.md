# Examples

Small, self-contained examples. The simulations are plain Python with NumPy; run them from the
Simulations panel (template **Python pipeline**) or with `afx.submit`, and auraFX opens the
result files when they finish. The tutorials are auraFX scripts: `aurafx --script <file>`.

| Folder | What it shows |
|---|---|
| `heat2d/` | 2-D heat diffusion writing VTK files; the simplest Python-pipeline run (sample outputs included) |
| `phase_field/` | Spinodal decomposition (Cahn-Hilliard, spectral NumPy solver): a phase-field time series |
| `grain_growth/` | Potts-model grain growth on a 2-D lattice: grain ids over Monte Carlo sweeps |
| `polycrystal/` | Tutorial: geometry → hexahedral mesh → Voronoi grains → quality → Exodus, `.tex` and a crystal-plasticity case |
| `ebsd/` | Tutorial: an EBSD volume → grains → sizes, shapes, IPF colors, pole figures, a CSV table |

```python
afx.submit("python3 -u cahn_hilliard.py", workdir="examples/phase_field", outputs="ch_*.vtk", auto_load=True)
afx.submit("python3 -u potts.py", workdir="examples/grain_growth", outputs="grains_*.vtk", auto_load=True)
```

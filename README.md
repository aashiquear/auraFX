<p align="center">
  <img src="assets/branding/logo-wordmark.png" alt="auraFX" width="360">
</p>

<p align="center">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue" alt="License: MIT/Apache-2.0"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/language-Rust-orange" alt="Language: Rust"></a>
  <a href="https://www.python.org"><img src="https://img.shields.io/badge/python-3.10%2B-3776AB" alt="Python 3.10+"></a>
</p>

**auraFX** is an open-source scientific visualization, analysis and simulation workbench
written in Rust. It reads the files your solvers write (Exodus II, VTK, LAMMPS dumps and
more), renders them on any GPU through Vulkan/Metal/DX12, and puts three things ParaView
does not have right next to the 3D view:

* an **AI assistant** (Claude, any OpenAI-compatible API, or local/cloud models through Ollama)
  that sees your pipeline and writes runnable Python for you,
* an **embedded Python** with a one-line-per-idea API (`afx.load(...).slice(...).color_by(...)`),
* a **simulation runner** that launches LAMMPS, MOOSE, OpenFOAM or your own Python pipeline
  locally, on a remote GPU box over SSH, or through Slurm, and opens the results when the run ends.

The goal is ParaView's breadth with a UI a new user can drive without a manual. The full
plan, with every ParaView 6.1.1 reader, filter and view mapped to a phase, is in
[ROADMAP.md](ROADMAP.md).

<p align="center"><img src="docs/images/screenshot.png" alt="auraFX main window" width="900"></p>

---

## What works today

| Area | Status |
|---|---|
| **Readers** | Exodus II (NetCDF-3 and NetCDF-4/HDF5, time steps, nodal/element/global variables, blocks, displacements), VTK legacy and XML (`.vtk .vtu .vtp .vti .vtr .vts .vtt`, parallel, `.vtm`, `.pvd`), OpenFOAM cases (ASCII/binary/gzip, decomposed, boundary patches), LAMMPS dumps (multi-frame, gzip) and data files, XYZ / extended XYZ, STL, PLY, OBJ, CSV |
| **Writers** | VTK XML (all types + multiblock), legacy VTK, **Exodus II** (blocks, variables, all time steps), CSV, STL, PLY — all verified readable by ParaView |
| **Filters (57 + 8 sources)** | Particle Tracer / Path / Streak Line, Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator, Append Datasets, Append Attributes, Resample With Dataset (multi-input), Slice, Clip, Contour, Threshold, Calculator, Warp, Glyph, Stream Tracer, Tube, Plot Over Line, Probe, Resample To Image, Histogram, Descriptive Statistics, Integrate Variables, Gradient, Find Data, Extract Selection, Connectivity, Clean, Surface Normals, Smooth, Shrink, Reflect, Extract Block, Cell Size, Mesh Quality, Elevation, Triangulate, Tetrahedralize, Mask Points, Temporal Statistics, Extract Subset, OpenFOAM Boundary Patches and more |
| **Rendering** | wgpu (Vulkan / Metal / DX12 / GL), MSAA, crease-angle shading, surface / edges / wireframe / points / outline / **GPU volume rendering**, 18 color maps with previews plus a **color map editor** (histogram, drag markers, bands, reverse, your own presets, ParaView `.json` import/export), log scale, scalar bar, translucency, volumes depth-merged with surfaces, X-ray selection highlight, data axes grid, text and time annotations, headless rendering |
| **Interaction** | Several 3D views side by side or stacked with linked cameras; pick values on surfaces; rectangle selection of visible cells (or through); ruler; interactive slice/clip plane widget; camera presets; time player |
| **UI** | Dark, light or system theme; Windows menu with panel toggles and layout presets; command palette (Ctrl+K); undo/redo; `.afx` session files and last-session restore; charts; spreadsheet; keyframe animation (camera and filter values on a timeline) and export (GIF, MP4, PNG) |
| **Python** | Embedded interpreter with the `afx` module, NumPy in and out, console with completion, `--script`, trace export |
| **AI** | Ollama (honours `OLLAMA_HOST`), Anthropic, OpenAI-compatible; streaming; pipeline-aware; **agent mode** runs code and iterates on results |
| **Simulations** | Local, SSH and Slurm jobs; NVIDIA/AMD GPU selection; parameter sweeps; live logs; auto-open results; **live reload** of files a running solver writes; GPU monitor; remote file browser |

## Installing

Prebuilt packages are attached to each [release](https://github.com/aashiquear/auraFX/releases).
Desktop packages bundle their own Python 3.12 with NumPy, so the Python console works without
any setup.

| Platform | Package | One-line install |
|---|---|---|
| Linux (x86_64, aarch64) | `.tar.gz`, `.AppImage` | `curl -fsSL https://raw.githubusercontent.com/aashiquear/auraFX/main/scripts/install.sh \| sh` |
| macOS (Apple silicon, Intel) | `.dmg` | same command as Linux |
| Windows 10/11 (x64) | `-setup.exe` installer, portable `.zip` | `irm https://raw.githubusercontent.com/aashiquear/auraFX/main/scripts/install.ps1 \| iex` |
| Android 8+ | `.apk` (viewer build) | download and open the APK |
| iOS / iPadOS 15+ | unsigned `.ipa` (viewer build) | sign and install with Xcode, AltStore or Sideloadly |

The mobile builds are viewers: they open, filter, render and chart data, chat with the AI
assistant and submit jobs to remote machines, but they do not embed Python. Packages are built by
[`.github/workflows/package.yml`](.github/workflows/package.yml) using the scripts in
`scripts/package/`; you can run those scripts locally too.

## Building

Requirements: Rust 1.85+, Python 3.10+ with development headers (`python3-dev`), a GPU driver
with Vulkan (Linux/Windows) or Metal (macOS).

```bash
git clone https://github.com/aashiquear/auraFX
cd auraFX
cargo build --release
./target/release/aurafx                     # start the app
./target/release/aurafx run.e               # open a file
./target/release/aurafx --script setup.py   # run a Python script at startup
```

The embedded Python is the one PyO3 finds at build time (`PYO3_PYTHON=/path/to/python3` to
choose another, e.g. a conda env with SciPy/PyTorch). Install packages into that interpreter
with `pip` and they are available in the console.

On Linux, `scripts/install-desktop.sh` adds auraFX to your application menu with its icon.

## Python in thirty seconds

```python
m = afx.load("impact_run.e")                     # Exodus, VTK, LAMMPS, STL, CSV ...
s = m.slice(normal=[0, 1, 0]).color_by("stress", cmap="Inferno")
c = m.contour(values=[300.0])                    # any filter: node.<filter>(**params)
afx.over_time(m, "stress", stat="max")           # plots max stress for every time step
T = m.array("temperature")                       # NumPy array
m.add_array("T_celsius", T - 273.15)
afx.plot(range(len(T)), T, label="T")            # Charts panel
afx.view("iso"); afx.screenshot("slice.png")
afx.save_animation("run.gif")                    # every time step, or orbit=72 for a turntable
afx.ask("find where the plate yields and plot the max stress over time", agent=True)

# Run a simulation on a remote GPU machine and open the result when it finishes
afx.submit("lmp -sf gpu -pk gpu 1 -in in.melt", host="rocm-server", workdir="~/runs/melt",
           gpu="amd:0", outputs="dump.melt", auto_load=True)
```

`afx.filters()` lists every filter with its parameters; `afx.help_filter("clip")` explains one.
The Python panel's **Trace** button converts what you did with the mouse into this API.

## AI assistant

Pick a provider in the Assistant panel (or Settings):

* **Ollama**: local or cloud models; the server URL comes from `OLLAMA_HOST` or Settings.
* **Anthropic**: set `ANTHROPIC_API_KEY` or paste a key in Settings.
* **OpenAI-compatible**: OpenAI, OpenRouter, vLLM, LM Studio, Groq… set the base URL and key.

The assistant receives a text description of your pipeline (array names, ranges, bounds,
time steps), never the raw data. Code it writes runs only when you click **Run code**,
unless you enable auto-run.

## Architecture

```
crates/
  aurafx-data      data model: arrays, attributes, cells, image/rectilinear/structured/poly/unstructured grids, tables, multiblock, time series
  aurafx-io        readers: Exodus (pure-Rust NetCDF-3), VTK legacy + XML, LAMMPS, STL, CSV
  aurafx-filters   filters and sources with typed parameter schemas (one registry drives UI, Python and AI)
  aurafx-pipeline  pipeline graph, background execution, time, display properties, color maps
  aurafx-render    wgpu renderer, camera, tessellation, headless rendering
  aurafx-python    embedded CPython (PyO3) and the `aurafx` module
  aurafx-ai        streaming chat clients for Ollama, Anthropic and OpenAI-compatible APIs
  aurafx-jobs      job runner (local / SSH / Slurm), GPU discovery, solver templates
  aurafx-app       the egui desktop application
```

More detail in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Testing

```bash
cargo test --workspace
```

Reader tests use files in `testdata/` (VTK samples written by ParaView 6.1.1). Larger samples
are not committed; see [testdata/README.md](testdata/README.md). Rendering has an offscreen
test that draws an Exodus file on the GPU and checks the image.

## Contributing

Contributions from the scientific computing, visualization and AI communities are welcome.
See [CONTRIBUTING.md](CONTRIBUTING.md). Pick an unchecked item in the roadmap, open an issue,
and send a pull request.

## License

auraFX is dual-licensed under either the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option. The bundled Phosphor icon font
is MIT-licensed (see `assets/fonts/`).

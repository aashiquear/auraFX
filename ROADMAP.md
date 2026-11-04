# auraFX Roadmap

auraFX is a Rust-native scientific visualization and analysis workstation. The goal is
feature parity with ParaView 6.1.1 (293 filters, 266 sources/readers, 46 writers, 101
view/representation types, 206 file-type groups; full inventory in
`docs/reference/paraview-6.1.1-catalog.json`) delivered through a UI that a first-time
user can drive without a manual, plus four things ParaView does not have:

1. A first-class AI assistant (cloud API, subscription, or local Ollama) that can see the
   pipeline, write and run analysis code, and explain results.
2. An embedded Python runtime with an `aurafx` module designed for one-liners
   (`mesh = afx.load("run.e"); afx.slice(mesh, normal="y").show()`).
3. A simulation runner: launch, monitor and post-process your own simulation suites
   (LAMMPS, MOOSE, OpenFOAM, custom scripts) locally or over SSH with GPU scheduling.
4. Cross-vendor GPU acceleration (Vulkan/Metal/DX12 through wgpu for everything,
   CUDA/ROCm optional for heavy kernels).

Legend: `[ ]` not started, `[~]` partly done (see notes in the "Status" section), `[x]` done. Phases are ordered so that each
one ships something usable on its own.

---

## 0. Foundations

### 0.1 Workspace and tooling
- [x] Cargo workspace with one crate per subsystem (see `docs/ARCHITECTURE.md`).
- [x] App icon and branding assets (`assets/icons`, `assets/branding`).
- [~] CI: build + test on Linux/macOS/Windows, clippy, rustfmt (workflow written, not yet run on GitHub).
- [ ] Packaging: AppImage, `.deb`, `.dmg`, MSIX, Flatpak; Linux `.desktop` + icon install.
- [ ] Crash reporting and structured logs (`tracing`), log panel in the UI.
- [~] Settings system (TOML in platform config dir), settings UI with search.
- [ ] Plugin ABI (dynamic Rust crates + Python plugins) and plugin manager UI.

### 0.2 Data model (`aurafx-data`)
Mirror of VTK's data object hierarchy, column-oriented, zero-copy where possible.
- [x] Typed arrays: i8..i64, u8..u64, f32, f64, with N components; string arrays.
- [x] Attribute sets: point data, cell data, field data; active scalars/vectors/normals/tcoords/tensors; global IDs; ghost arrays.
- [x] Datasets: `ImageData`, `RectilinearGrid`, `StructuredGrid`, `PolyData`, `UnstructuredGrid`, `Table`, `PointCloud`/particles.
- [ ] Composite: `MultiBlock`, `PartitionedDataSet`, `PartitionedDataSetCollection`, overlapping and non-overlapping AMR, `HyperTreeGrid`, `Molecule`, `CellGrid`.
- [x] Cell types: all VTK linear types (vertex, line, polyline, tri, quad, polygon, tet, hex, wedge, pyramid, pentagonal/hexagonal prism, polyhedron).
- [ ] Higher-order: quadratic, Lagrange and Bezier cells with tessellation.
- [~] Time: `TimeSeries` wrapper with lazy per-step loading, time-step metadata, time arrays.
- [ ] Selections: IDs, global IDs, pedigree IDs, frustum, location, block, thresholds, query.
- [ ] Spatial structures: point locator, cell locator, BVH/octree, kd-tree (used by probe, interpolate, picking).
- [ ] Out-of-core/mmap-backed arrays for files larger than RAM.
- [ ] Unit and physical-quantity metadata on arrays.

### 0.3 Pipeline engine (`aurafx-pipeline`)
- [x] Demand-driven DAG: sources -> filters -> representations, with dirty tracking and memoized outputs.
- [~] Per-node progress and cancellation; background execution off the UI thread.
- [~] Time-aware execution (request a time step, temporal filters that need multiple steps).
- [ ] Streaming/pieces for huge datasets; ghost-cell generation.
- [x] Undo/redo of every pipeline and property change.
- [~] State files (`.afx` JSON/TOML) and ParaView `.pvsm` import (best effort).
- [~] Python trace: every UI action recorded as an equivalent `aurafx` Python call.

---

## 1. Readers (file formats)

Priority is driven by the stated use-cases (Exodus, VTU/VTK, LAMMPS) and then by breadth.
Pure-Rust readers are preferred; HDF5/NetCDF-4 use the C libraries behind a feature flag.

### 1.1 Tier 1 (ship first)
- [x] VTK legacy `.vtk` (ASCII and binary; all dataset types).
- [x] VTK XML: `.vtu`, `.vtp`, `.vti`, `.vtr`, `.vts`, `.vtt` (inline, appended, base64; raw; zlib and LZ4 block compression). `.vtm`, `.pvtu/.pvtp/...` parallel pieces and `.pvd` time collections.
- [x] LAMMPS dump (text; `ITEM: ATOMS` with arbitrary columns, triclinic boxes, multi-frame), plus `.dump.gz`.
- [~] LAMMPS binary dump, LAMMPS data/structure files, XYZ/extXYZ, `.lammpstrj`.
- [~] Exodus II (NetCDF-3 classic/64-bit offset in pure Rust; NetCDF-4/HDF5 via `hdf5` feature): nodes, blocks, side sets, node sets, nodal/element/global variables, time steps, displacement application, name maps.
- [~] CSV/TSV with delimiter detection, header sniffing, and "table to points/structured grid".
- [~] STL, OBJ, PLY, OFF, glTF/GLB.
- [ ] PNG/JPEG/TIFF image stacks and series as `ImageData`.
- [ ] VTKHDF (`.vtkhdf`), HDF5 generic browser.

### 1.2 Tier 2 (simulation ecosystem)
- [~] OpenFOAM (`.foam` case, ASCII/binary, decomposed and reconstructed, time directories).
- [ ] EnSight Gold (`.case`), Tecplot ASCII/binary, CGNS, Fluent `.cas/.msh`, Nastran/BDF, ANSYS `.inp`, LS-DYNA `d3plot`, Silo, Xdmf2/3, PLOT3D, Nek5000, MFIX, ADIOS2 BP4/BP5, openPMD, Fides, Conduit/Catalyst.
- [ ] NetCDF CF/MPAS/CAM/POP, GDAL raster/vector, DEM, SEG-Y.
- [ ] Molecular: PDB, Gaussian Cube, CML, VASP (POSCAR/CHGCAR/OUTCAR), XYZ molecules.
- [ ] Cosmology: Gadget, GenericIO, Cosmo, Enzo/FLASH AMR, AMReX plotfiles.
- [ ] Medical/imaging: DICOM, NRRD, MetaImage (`.mhd`), MRC, OME-TIFF, NIfTI.
- [ ] Point clouds: LAS/LAZ (PDAL), PTS, H5Part.
- [ ] CAD: STEP, IGES (via OpenCASCADE feature), VRML, Alembic, USD, OpenVDB.
- [ ] Every remaining reader in the catalog, tracked per entry in `docs/reference/format-matrix.md`.

### 1.3 Writers
- [~] VTK XML (all dataset types, parallel pieces, `.pvd`), VTK legacy, VTKHDF.
- [~] Exodus II, CGNS, EnSight, Xdmf, OpenVDB, GenericIO, Fides.
- [ ] CSV, STL, PLY, OBJ, glTF, Houdini `.geo`.
- [~] Images: PNG/JPEG/TIFF/BMP/EXR, screenshot with resolution override and transparent background.
- [ ] Animation: PNG sequence, MP4/WebM (ffmpeg feature), GIF.
- [ ] Scene export: glTF, OBJ, X3D, VRML, POV-Ray, PDF/SVG vector (GL2PS-equivalent), WebGL/HTML interactive scene.
- [ ] Extracts and Catalyst-style in-situ scripts.

---

## 2. Filters (full ParaView parity list)

Each group below mirrors a ParaView menu category. Every ParaView filter name from the
catalog is listed; those marked with a star are the ones that ship in the first usable build.

### 2.1 Common (toolbar) ★
- [~] Calculator ★ (expression language with vector/scalar/tensor functions, per-point and per-cell)
- [~] Contour ★ (marching cubes/tetrahedra, multiple isovalues, compute normals/gradients, generate triangles)
- [~] Clip ★ (plane, box, sphere, cylinder, cone, annulus, scalar, crinkle, inside-out)
- [~] Slice ★ (plane, box, sphere, cylinder, multiple offsets, crinkle, merge points; Axis-Aligned Slice; Slice With Plane; Slice Along PolyLine; Slice Generic Dataset)
- [~] Threshold ★ (between/below/above, all-points/any-point, component modes)
- [ ] Extract Subset ★ (VOI, sample rate, include boundary; Extract Subset With Seed)
- [~] Glyph ★ (arrow, cone, sphere, cylinder, line, 2D glyph, custom source; scale/orient by array; masking strategies), Glyph With Custom Source, Tensor Glyph, Normal Glyphs
- [ ] Stream Tracer ★ (RK2/RK4/RK45, point cloud and line seeds, surface streamlines, vorticity), Stream Tracer With Custom Source, Evenly Spaced Streamlines 2D
- [~] Warp By Vector ★, Warp By Scalar ★
- [ ] Group Datasets, Extract Block, Extract Level
- [~] Plot Over Line ★, Plot Data, Plot Data Over Time, Plot Selection Over Time, Plot Global Variables Over Time, Plot On Intersection Curves, Plot On Sorted Lines, Plot Over Lines From Custom Source

### 2.2 Data analysis
- [~] Histogram ★, Histogram 2D, Descriptive Statistics, Contingency Statistics, Multicorrelative Statistics, Principal Component Analysis, K-Means, Compute Quartiles, Extract Statistical Model Tables, Generate Statistics, Integrate Variables ★, Min Max, Scatter Plot, Table FFT, FFT Of Selection Over Time, Temporal Statistics ★, Compute Derivatives, Gradient ★, Gradient Magnitude, Curvature, Elevation, Mesh Quality, Cell Quality, Boundary Mesh Quality, Cell Size, Polyline Length, Count Cell Faces, Count Cell Vertices, Validate Cells, Overlapping Cells Detector, Vortex Cores, Yield Criteria, Tensor Principal Invariants, Surface Flow, Minkowski Filter, OpenTURNS: OT Density Map, OT Kernel Smoothing

### 2.3 Alphabetical / geometry and topology
- [ ] Append Attributes, Append Datasets ★, Append Geometry, Append Reduce, Append Molecule, Append Selections
- [~] Cell Centers ★, Cell Data to Point Data ★, Point Data to Cell Data ★, Field Data to Attribute, Pass Arrays ★, Rename Arrays, Extract Component ★, Merge Vector Components, Extract Attributes, Coordinates, Global Point And Cell Ids, Point And Cell Ids, Process Ids, Block Ids, Overlapping AMR Level Ids, Random Attributes, Random Vectors
- [~] Clean ★, Clean to Grid, Clean Cells to Grid, Merge Blocks ★, Convert To Multi Block, Convert To Partitioned DataSet Collection, Convert AMR dataset to Multiblock, Convert To Point Cloud, Convert To Cell Grid, Convert Polyhedral Cells, Convert Into Molecule, Convert Selection, Image Data To AMR, Image Data To Uniform Grid, Image Data to Point Set, Rectilinear Data to Point Set, Table To Points ★, Table To Structured Grid, Transpose Table, Points Matching Transform
- [~] Connectivity ★, Rectilinear Grid Connectivity, Extract Surface ★, Extract Edges, Feature Edges ★, Feature Edges Region Ids, Extract Region Surface, Extract Enclosed Points, Extract Cells By Region, Extract Cells By Type, Extract Cells Along Line, Extract Cells Along Lines (custom source), Extract Location, Extract Ghost Cells, Ghost Cells, Remove Ghost Information, Extract Selection ★, Extract Time Steps, Extract Particles Over Time, Extract AMR Blocks, Extract CTH Parts, Geometry Filter, Outline ★, Outline Corners, Outline Curvilinear DataSet, Outline Generic DataSet, Bounding Ruler
- [~] Transform ★, Axis-Aligned Transform, Reflect, Axis-Aligned Reflect, Shrink, Explode DataSet, Crop, Image Shrink, Align Image Origins, Stitch Image Data With Ghosts
- [ ] Triangulate ★, Tetrahedralize ★, Tessellate, Tessellate Generic Dataset, Delaunay 2D ★, Delaunay 3D ★, Decimate ★, Quadric Decimation, Quadric Clustering, Decimate Polyline, Subdivide, Loop Subdivision, Smooth ★, Triangle Strips, Reverse Sense, Surface Normals ★, Surface Tangents, Surface Vectors, Deflect Normals, PCA Normal Estimation, Ribbon, Tube ★, Linear Extrusion, Linear Cell Extrusion, Rotational Extrusion, Volume Of Revolution, Clip Closed Surface, Iso Volume ★, Mask Points, Distribute Points, Generate Quadrature Points, Generate Quadrature Scheme Dictionary, Interpolate to Quadrature Points, Finite Element Field Distributor, Compute Sides
- [ ] Texture Map to Plane/Cylinder/Sphere, Perlin Noise, Spatio Temporal Harmonics
- [~] Resample To Image ★, Resample With Dataset ★, Resample To Line, Resample AMR, Adaptive Resample To Image, Gaussian Resampling, Probe Location ★, Point Interpolator (Gaussian/Shepard/Voronoi/Linear kernels), Point Line/Plane/Volume/Dataset Interpolator, SPH Interpolator (+ Line/Plane/Volume/Dataset), Point Set To Octree Image, Octree Image to Point Set, Median, Equalizer Filter
- [ ] Particle Tracer, Particle Path, Streak Line, Legacy Particle Path, Legacy Streak Line, In Situ Particle Path, Temporal Particles To Pathlines
- [ ] Temporal Interpolator, Temporal Shift Scale, Temporal Snap-to-TimeStep, Temporal Cache, Temporal Array Operator, Temporal Smoothing, Merge Time, Group Time Steps, Generate Time Steps, Force Time, Synchronize Time, Critical Time, Date To Numeric, Time Step Progress Bar, Animate Modes
- [ ] Hyper Tree Grid: Axis Reflection, Cell Centers, Depth Limiter, Evaluate Coarse, Feature Edges, Generate Fields, Geometry Filter, Ghost Cells Generator, Redistribute, To Dual Grid, To Unstructured Grid, Visible Leaves Size
- [ ] AMR: Connectivity, Contour, Dual Clip, Fragment Integration, Fragments Filter, Material Interface Filter, Intersect Fragments, Youngs Material Interface
- [ ] Cell Grid: Centers, Elevation, Probe, To Unstructured Grid, Transform, Warp
- [ ] Molecules: Compute Molecule Bonds, Molecule To Lines, EAM Project To Sphere
- [ ] Cosmology: ANL Halo Finder, ANL Subhalo Finder, LANL FOF/SOD Halo Finder
- [ ] Parallel/distributed: D3, Redistribute DataSet, Partition Balancer, Reduction Filter, Aggregate Dataset, Client Server Move Data, MPI Move Data, Pass Through, Force Static Mesh, Voronoi Merge Connected
- [ ] Annotation: Annotate Attribute Data, Annotate Global Data, Annotate Selection, Annotate Time Filter, Environment Annotation, Python Annotation, Programmable Annotation, Text, 3D Text, Logo, Ruler, Protractor
- [~] Programmable: Programmable Filter ★ (Python), Programmable Source, Python Calculator ★, Live Programmable Source, Color By Array, OME TIFF Channel Calculator, Threshold Table, Field Arrays From File

### 2.4 Sources
- [ ] Geometric: Sphere ★, Cone ★, Cylinder ★, Box ★, Plane ★, Line ★, Disk, Ellipse, Arrow, Axes, Superquadric, Point Source, PolyLine Source, PolyPoint Source, Spline Source, Outline Source
- [ ] Synthetic data: Wavelet ★, Mandelbrot, Fast Uniform Grid, Hierarchical Fractal, Hyper Tree Grid (+Random), AMR Gaussian Pulse, Data Object Generator, Unstructured Cell Types, Partitioned DataSet Collection Source, Cell Grid Source, Spatio Temporal Harmonics Uniform Grid, Time Source, Trivial Producer
- [ ] Selection sources (ID, global ID, pedigree ID, location, frustum, threshold, value, block, query, composite/hierarchical ID)

### 2.5 Filter infrastructure
- [~] Every filter has: typed properties, defaults, validation, documentation, search keywords, Python binding, undo entry, and progress reporting.
- [ ] GPU implementations (wgpu compute) for: contour, clip, slice, threshold, gradient, resample, glyph, stream tracer, particle advection, connectivity, histogram.
- [ ] Custom filter authoring: Python (`@afx.filter`) and Rust plugins.

---

## 3. Rendering and views

### 3.1 Render view (`aurafx-render`, wgpu)
- [x] Camera: perspective/orthographic, trackball/terrain/joystick styles, reset/fit, standard views (+X,-X,...), isometric, roll, zoom to box, zoom to data, camera undo/redo, saved camera links.
- [x] Surface rendering with flat/Gouraud/PBR shading; backface handling.
- [x] Scalar coloring with color maps (ColorBrewer, viridis family, ParaView presets, imported `.json/.xml`), range modes (auto, custom, time-wide), log scale, discrete, NaN/above/below colors, opacity transfer functions, per-component or magnitude, categorical.
- [~] Representations: Surface, Surface With Edges, Wireframe, Points, Outline, Feature Edges, Point Gaussian (splats: sphere, gaussian, custom shader), 3D Glyphs, Volume, Slice (image), Slice (unstructured), Multi-slice, Orthographic slice, Molecule (ball/stick/VDW), Hyper Tree Grid, Cell Grid, Polar axes, Data labels, Selection highlight, Texture.
- [~] Volume rendering: GPU ray casting, smart/fixed-point, 2D transfer functions, shade, gradient opacity, multi-component, blend modes (composite, MIP, MinIP, additive, isosurface), cropping, clipping planes, unstructured-grid projected tetra / ray casting.
- [ ] Lighting: key/fill/head/back light kit, custom lights with widgets, shadows, ambient occlusion (SSAO), Eye-Dome Lighting, tone mapping, FXAA/MSAA/TAA, depth peeling for correct transparency.
- [ ] Surface LIC, ray tracing (OSPRay/Embree- or Rust path tracer, optional), environment maps/skyboxes, materials editor.
- [ ] Axes grid (data, custom bounds, labels, units), orientation axes, center-of-rotation marker, scalar bars (position, titles, number format, custom labels), legends, logos, text, interactive 2D annotations.
- [ ] Widgets: implicit plane, box, sphere, cylinder, cone, annulus, frustum, line, polyline, spline, handle/point, distance, protractor, ruler, slider, light, coordinate frame, four-lines, contour, display-sized plane, text, logo, equalizer, progress bar.
- [~] Selection: by click/rubber band/polygon on points/cells/blocks, through-frustum, by query (`field > value`, nearest, etc.), grow/shrink, invert, hover tooltips and interactive labels, "Find Data" panel.
- [ ] Picking tools: point probe, coordinate readout, interactive cell/point info.
- [ ] Stereo (anaglyph, crystal eyes, interlaced, side-by-side), VR (OpenXR) later.
- [ ] Huge data: LOD, frustum/occlusion culling, instanced glyphs, GPU-resident meshes, mesh streaming, remote render with image streaming.

### 3.2 Other views
- [~] Line chart ★ (multiple series, log axes, styles, legends, markers, selection link), Bar chart, Point chart, XY bar chart, Histogram view, Box chart / Quartile chart, Plot matrix (scatter matrix), Parallel coordinates, Image chart, Comparative views (render/line chart), Slice view, Orthographic slice view, Spreadsheet view ★ (point/cell/field/row data, sort, filter, column visibility, precision, export CSV), Python view (matplotlib embedded), Spreadsheet of selection.
- [~] Layouts: split horizontally/vertically, tabs, link cameras, link views, link selections, maximize, save/restore layouts, multiple windows/monitors.

### 3.3 Animation and time
- [~] Time toolbar (play, step, loop, time value entry, snap to steps, real-time, sequence modes), time inspector.
- [ ] Animation view: keyframes for any property (ramp, linear, exponential, sinusoid, boolean), camera paths (orbit, path, interpolate), Python cue, comparative cues.
- [~] Save animation at any resolution/format with frame-rate control; stride; preview.

---

## 4. User experience (the thing that must be better than ParaView)

Design principles: progressive disclosure, no modal "Apply" button by default (auto-apply
with cancel), everything searchable, every action scriptable, every panel optional.

- [x] Modern docking layout (egui): pipeline browser, properties, viewport, Python console, AI chat, log, charts. Default layouts for "Visualize", "Analyze", "Script", "Simulate".
- [x] Command palette (Ctrl+K / Ctrl+Space): fuzzy search across filters, sources, actions, settings, recent files, and AI prompts.
- [~] Smart "Open" with drag-and-drop, format auto-detect, time-series grouping (`run_*.vtu`), preview thumbnails and metadata before load, recent/favorite files and folders, remote file browser over SSH.
- [~] Auto-apply with background execution and progress; cancel any time; "Apply" mode available for heavy data.
- [~] Properties panel: grouped, searchable, with inline help, units, sliders bound to data ranges, live preview, "advanced" toggle, per-filter presets, copy/paste properties between nodes.
- [~] Pipeline browser as a graph with inline eye toggles, color-by dropdown, representation dropdown, rename, duplicate, group, disable, comments. Also a node-graph editor view.
- [~] Contextual quick actions in the viewport (right-click: slice here, clip here, probe here, color by...).
- [ ] Color map editor with live histogram, drag handles, presets gallery, color-blind safe set, favorites, import/export.
- [ ] Onboarding: sample datasets gallery, guided tours, tooltips with screenshots, "Explain this filter" via AI.
- [ ] Keyboard-first: fully rebindable shortcuts, vim-style view navigation optional.
- [~] Themes: dark/light/high-contrast, scalable UI (HiDPI), font size.
- [ ] Session recovery after crash, autosave of state.
- [ ] Accessibility: screen-reader labels, keyboard navigation, color-blind color maps by default.
- [ ] Localization framework.
- [~] Notifications/toasts for background jobs and errors, with "show details" and "ask AI why".

---

## 5. Python (`aurafx-python`, PyO3)

- [x] Embedded interpreter (system Python by default, venv/conda selectable in settings); `aurafx` module exposed to it.
- [x] Interactive console panel with history, completion, and a script editor with run/run-selection.
- [~] `aurafx` API designed for brevity:
  - `afx.load(path)`, `afx.sources.sphere()`, `mesh.slice(normal=..)`, `mesh.clip(..)`, `mesh.contour(..)`, `mesh.threshold(..)`, `mesh.warp(..)`, `.show()`, `.color_by("stress")`, `.camera(view="iso")`, `.screenshot("a.png", size=(1920,1080))`, `.animate(...)`.
  - Every filter and property is reachable by the same name as the UI (snake_case).
  - Zero-copy NumPy views of arrays (`mesh.point_data["disp"]` returns an ndarray), `to_pandas()`/`to_polars()`, `from_numpy(...)`.
  - Helpers: `afx.fit(x, y, model)`, `afx.stats(...)`, `afx.plot(...)` (opens a chart view), `afx.export(...)`.
- [~] Python trace recorder (UI -> script), script "macros" toolbar, scheduled/batch runs (`aurafx --script run.py`), headless mode (`aurafx-batch`).
- [ ] Bundled scientific stack management: detect/install numpy, scipy, pandas, polars, matplotlib, h5py, netCDF4, scikit-learn, torch (CUDA/ROCm wheels), with a UI for environments.
- [ ] Programmable filter/source with NumPy and typed dataset wrappers.
- [ ] Jupyter kernel exposure (drive auraFX from a notebook) and notebook panel inside the app.
- [ ] Custom Python plugins (reader, filter, panel) with hot-reload.

---

## 6. AI assistant (`aurafx-ai`)

- [x] Provider abstraction: Anthropic API, OpenAI-compatible API, Ollama (local and cloud models), with streaming.
- [~] Subscription/OAuth sign-in for hosted providers; API keys stored in the OS keyring.
- [x] Chat panel with Markdown rendering, code blocks with "Run in Python console", conversation history per project.
- [~] Context awareness: the model sees pipeline state, selected dataset summaries (arrays, ranges, bounds, time steps), camera, recent errors and logs, without uploading raw data unless asked.
- [ ] Tools the model can call: load file, add filter, set property, color by, take screenshot (and view it), run Python, read a chart's data, search docs, run a simulation job.
- [~] Modes: "Explain", "Do it for me" (agentic with confirmation per step), "Write a script", "Debug this error".
- [ ] Natural-language filter creation ("show me regions where von Mises stress > 300 MPa").
- [ ] Anomaly and feature detection helpers (outliers in fields, mesh quality problems, time-series change points).
- [ ] Prompt templates and user-defined skills; privacy controls (local-only mode, redaction).
- [ ] Vision: send rendered frames to multimodal models for interpretation.

---

## 7. Simulation runner (`aurafx-jobs`)

- [~] Job model: command, working dir, environment, inputs, outputs, resources (CPU, GPU count/vendor, memory), dependencies between jobs, templates per solver.
- [~] Local executor with live stdout/stderr streaming, log search, kill/pause, resource monitoring (CPU, GPU via NVML/ROCm-SMI).
- [~] Templates: LAMMPS, MOOSE, OpenFOAM, Abaqus/CalculiX, GROMACS, custom shell/Python.
- [~] Python pipeline runs: define a pipeline in Python (pre -> run -> post), run as a job, auto-load outputs when done.
- [x] Parameter sweeps and ensembles with a results table and comparative views.
- [~] Remote executor over SSH: connect (key, agent, password, jump hosts, 2FA prompts), remote file browser, sync inputs/outputs (rsync-style), run under nohup/tmux, Slurm/PBS/LSF submission and queue monitoring, reconnect to running jobs.
- [ ] Remote rendering/data server mode: run `aurafx-server` on the GPU box, stream geometry or images to the local client.
- [x] Watch folders: auto-refresh when the solver writes a new step.

---

## 8. GPU compute (`aurafx-gpu`)

- [x] wgpu device management: adapter choice (AMD, NVIDIA, Intel, software), features, limits, memory stats in UI.
- [ ] Compute kernels in WGSL for filters listed in 2.5, array math (calculator JIT to WGSL), reductions, histograms, sorting.
- [ ] Optional vendor backends behind features: `cuda` (cudarc) and `rocm` (HIP) for kernels where WGSL is too limited (fp64, large shared memory), and for interop with PyTorch tensors.
- [ ] GPU memory budgeting, fallbacks to CPU (rayon), benchmark panel.

---

## 9. Distributed and remote (parity with pvserver)

- [ ] Client/server split: `aurafx-server` (data + render) over TCP/SSH tunnel with the same protocol as local.
- [ ] Multi-process data parallel (MPI optional; Rust-native work distribution first), ghost cells, D3 redistribution, compositing (sort-last), tile displays.
- [ ] Collaboration: shared sessions with multiple clients.
- [ ] In-situ: Catalyst/Conduit endpoint, live visualization of running jobs.

---

## 10. Quality, docs, community

- [~] Golden-image render tests, filter regression tests against VTK outputs, reader conformance suite with sample files for every format.
- [ ] Benchmarks vs ParaView (load time, filter time, frame rate) published in docs.
- [ ] User guide, filter reference (auto-generated from property metadata), Python API docs, tutorials, example gallery.
- [ ] Contributing guide, code of conduct, issue templates, plugin cookbook.

---

## Delivery phases

| Phase | Target | Contents |
|---|---|---|
| P0 Skeleton | done | Workspace, data model, legacy VTK + VTU/VTP + LAMMPS readers, wgpu render view, docking UI, command palette, Python console, AI chat (Ollama/Anthropic/OpenAI) |
| P1 Daily driver | in progress | Exodus, PVD/time series, core ★ filters, color maps + scalar bar, spreadsheet + line chart, screenshots, state files, settings |
| P2 Analysis | | Statistics/data-analysis filters, selection + Find Data, Python API breadth, Python trace, programmable filter, charts breadth |
| P3 Simulation | | Job runner local + SSH, templates, sweeps, watch folders, remote file browser, GPU monitoring |
| P4 Rendering depth | | Volume rendering, widgets, lighting/SSAO/EDL, animation + keyframes, exporters, LIC, ray tracing |
| P5 Breadth | | Remaining readers/writers/filters from the catalog, AMR/HTG/molecule/cell-grid types, higher-order cells |
| P6 Scale | | Client/server, distributed, in-situ, collaboration, out-of-core |
| P7 Polish | | Plugins, packaging on all OSes, accessibility, localization, docs and benchmarks |

---

## Status (2026-11-04)

Done in iteration 2 (on top of iteration 1 below): light/dark/system themes; Windows menu with
panel toggles and layout presets; undo/redo; `.afx` state files and last-session restore;
writers (VTK XML, legacy, CSV, STL, PLY); picking, rectangle selection, Extract Selection, Find
Data; 18 more filters including Stream Tracer and Tube; Temporal Statistics; GPU volume
rendering; animation export; interactive plane widget; AI agent mode; NetCDF-4 Exodus;
OpenFOAM, XYZ, LAMMPS data, PLY and OBJ readers; live file watching; parameter sweeps; CI.

Known gaps worth doing next: boundary patches and decomposed cases in the OpenFOAM reader;
selection of visible cells only (needs an ID buffer); volume rendering that depth-merges with
opaque geometry; higher-order cell tessellation; client/server and distributed execution;
plugin system; packaging (AppImage, installers); Slurm and CI runs not yet exercised on real
infrastructure.

## Status (2026-11-03, iteration 1)

Built in the first iteration, all exercised by tests or by driving the real app:

* **Readers:** Exodus II (NetCDF-3; NetCDF-4/HDF5 Exodus not yet), VTK legacy, VTK XML (all
  serial types, parallel pieces, multiblock, PVD), LAMMPS text dumps (multi-frame, gzip),
  STL, CSV.
* **Filters (27):** slice, clip, contour, threshold, calculator, warp by vector/scalar, glyph,
  extract surface, extract edges, feature edges, outline, transform, cell centers, merge
  blocks, cell↔point data, plot over line, probe location, resample to image, histogram,
  descriptive statistics, integrate variables, gradient, plus 8 sources.
* **Rendering:** wgpu, MSAA, surface/edges/wireframe/points/outline, color maps, scalar bar,
  orientation axes, translucency, headless rendering and screenshots.
* **UI:** docking layout, command palette, auto-apply, time player, charts, spreadsheet,
  information, log, toasts, recent files, drag and drop, settings, Python trace export.
* **Python:** embedded interpreter, `afx` module, NumPy interop, console, `--script`.
* **AI:** Ollama, Anthropic and OpenAI-compatible providers with streaming and pipeline context.
* **Simulations:** local / SSH / Slurm runner, GPU selection and monitoring, templates,
  remote file browser, auto-open results.

Items marked `[~]` above have a working core with known gaps, for example: no undo yet, no
selection/picking yet, AI "do it for me" tool-calling not yet (the assistant writes code the
user runs), subscription/OAuth login not yet (API keys and Ollama work), Slurm path untested
against a live cluster.

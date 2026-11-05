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
- [~] Packaging: AppImage, tar.gz + `install.sh`, `.dmg`, Windows installer + zip, Android APK, unsigned iOS IPA (done); `.deb`, MSIX, Flatpak (todo).
- [~] Crash reporting and structured logs (`tracing`), log panel in the UI. (Logs and log panel done; crash reporting todo.)
- [~] Settings system (TOML in platform config dir), settings UI with search.
- [~] Plugin ABI (dynamic Rust crates + Python plugins) and plugin manager UI. (Python plugins and the Plugins window done; Rust plugins todo.)

### 0.2 Data model (`aurafx-data`)
Mirror of VTK's data object hierarchy, column-oriented, zero-copy where possible.
- [x] Typed arrays: i8..i64, u8..u64, f32, f64, with N components; string arrays.
- [x] Attribute sets: point data, cell data, field data; active scalars/vectors/normals/tcoords/tensors; global IDs; ghost arrays.
- [x] Datasets: `ImageData`, `RectilinearGrid`, `StructuredGrid`, `PolyData`, `UnstructuredGrid`, `Table`, `PointCloud`/particles.
- [~] Composite (MultiBlock done): `MultiBlock`, `PartitionedDataSet`, `PartitionedDataSetCollection`, overlapping and non-overlapping AMR, `HyperTreeGrid`, `Molecule`, `CellGrid`.
- [x] Cell types: all VTK linear types (vertex, line, polyline, tri, quad, polygon, tet, hex, wedge, pyramid, pentagonal/hexagonal prism, polyhedron).
- [x] Higher-order: Lagrange and Bezier cells of any order (curve, triangle, quad, tet, hex, wedge; per-cell `HigherOrderDegrees`, rational Bezier weights) and quadratic cells (complete, serendipity 8/20/15-node, quadratic-linear, cubic line), drawn curved and cut/integrated through their nodes like VTK (checked against ParaView 6.1.1). Pyramids (quadratic, Lagrange, Bezier) are still cut through their corners only.
- [~] Time: `TimeSeries` wrapper with lazy per-step loading, time-step metadata, time arrays.
- [~] Selections (IDs, rectangle/frustum, visible-only, threshold query done): IDs, global IDs, pedigree IDs, frustum, location, block, thresholds, query.
- [~] Spatial structures (cell locator for probe and stream tracer done): point locator, cell locator, BVH/octree, kd-tree (used by probe, interpolate, picking).
- [ ] Out-of-core/mmap-backed arrays for files larger than RAM.
- [ ] Unit and physical-quantity metadata on arrays.

### 0.3 Pipeline engine (`aurafx-pipeline`)
- [x] Demand-driven DAG: sources -> filters -> representations, with dirty tracking and memoized outputs.
- [x] Multi-input filters: extra inputs per node, scheduled and invalidated like the main input, kept in undo and state files, cycles rejected.
- [~] Per-node progress and cancellation; background execution off the UI thread.
- [x] Time-aware execution: temporal filters read their input at any time (the upstream pipeline is re-evaluated on the worker thread), add or remap time steps, keep state between runs (particles advance incrementally) and skip re-running when their result does not depend on time.
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
- [x] OpenFOAM (`.foam` case, ASCII/binary, decomposed and reconstructed, time directories, boundary patches with boundary values; verified against ParaView).
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
- [~] Exodus II (done: element blocks, nodal/element variables, all time steps; verified in ParaView), CGNS, EnSight, Xdmf, OpenVDB, GenericIO, Fides.
- [~] CSV, STL, PLY (done), OBJ, glTF, Houdini `.geo`.
- [~] Images: PNG/JPEG/TIFF/BMP/EXR, screenshot with resolution override and transparent background.
- [~] Animation: PNG sequence, MP4 (ffmpeg), GIF (done); WebM.
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
- [~] Extract Subset ★ (VOI, sample rate, include boundary done, verified against ParaView; Extract Subset With Seed todo)
- [~] Glyph ★ (arrow, cone, sphere, cylinder, line, 2D glyph, custom source; scale/orient by array; masking strategies), Glyph With Custom Source, Tensor Glyph, Normal Glyphs
- [~] Stream Tracer ★ (RK2/RK4/RK45, point cloud and line seeds, surface streamlines, vorticity), Stream Tracer With Custom Source, Evenly Spaced Streamlines 2D
- [~] Warp By Vector ★, Warp By Scalar ★
- [~] Group Datasets, Extract Block (done), Extract Level
- [~] Plot Over Line ★, Plot Data, Plot Data Over Time, Plot Selection Over Time, Plot Global Variables Over Time, Plot On Intersection Curves, Plot On Sorted Lines, Plot Over Lines From Custom Source

### 2.2 Data analysis
- [~] Histogram ★, Histogram 2D, Descriptive Statistics, Contingency Statistics, Multicorrelative Statistics, Principal Component Analysis, K-Means, Compute Quartiles, Extract Statistical Model Tables, Generate Statistics, Integrate Variables ★, Min Max, Scatter Plot, Table FFT, FFT Of Selection Over Time, Temporal Statistics ★ (live filter), Compute Derivatives, Gradient ★, Gradient Magnitude, Curvature, Elevation, Mesh Quality, Cell Quality, Boundary Mesh Quality, Cell Size, Polyline Length, Count Cell Faces, Count Cell Vertices, Validate Cells, Overlapping Cells Detector, Vortex Cores, Yield Criteria, Tensor Principal Invariants, Surface Flow, Minkowski Filter, OpenTURNS: OT Density Map, OT Kernel Smoothing

### 2.3 Alphabetical / geometry and topology
- [~] Append Attributes, Append Datasets ★, Append Geometry (done, checked against ParaView), Append Reduce, Append Molecule, Append Selections
- [~] Cell Centers ★, Cell Data to Point Data ★, Point Data to Cell Data ★, Field Data to Attribute, Pass Arrays ★, Rename Arrays, Extract Component ★, Merge Vector Components, Extract Attributes, Coordinates, Global Point And Cell Ids, Point And Cell Ids, Process Ids, Block Ids, Overlapping AMR Level Ids, Random Attributes, Random Vectors
- [~] Clean ★, Clean to Grid, Clean Cells to Grid, Merge Blocks ★, Convert To Multi Block, Convert To Partitioned DataSet Collection, Convert AMR dataset to Multiblock, Convert To Point Cloud, Convert To Cell Grid, Convert Polyhedral Cells, Convert Into Molecule, Convert Selection, Image Data To AMR, Image Data To Uniform Grid, Image Data to Point Set, Rectilinear Data to Point Set, Table To Points ★, Table To Structured Grid, Transpose Table, Points Matching Transform
- [~] Connectivity ★, Rectilinear Grid Connectivity, Extract Surface ★, Extract Edges, Feature Edges ★, Feature Edges Region Ids, Extract Region Surface, Extract Enclosed Points, Extract Cells By Region, Extract Cells By Type, Extract Cells Along Line, Extract Cells Along Lines (custom source), Extract Location, Extract Ghost Cells, Ghost Cells, Remove Ghost Information, Extract Selection ★, Extract Time Steps, Extract Particles Over Time, Extract AMR Blocks, Extract CTH Parts, Geometry Filter, Outline ★, Outline Corners, Outline Curvilinear DataSet, Outline Generic DataSet, Bounding Ruler
- [~] Transform ★, Axis-Aligned Transform, Reflect, Axis-Aligned Reflect, Shrink, Explode DataSet, Crop, Image Shrink, Align Image Origins, Stitch Image Data With Ghosts
- [~] Triangulate ★, Tetrahedralize ★, Tessellate (done: uniform levels instead of ParaView's chord-error refinement), Tessellate Generic Dataset, Delaunay 2D ★, Delaunay 3D ★, Decimate ★, Quadric Decimation, Quadric Clustering, Decimate Polyline, Subdivide, Loop Subdivision, Smooth ★, Triangle Strips, Reverse Sense, Surface Normals ★, Surface Tangents, Surface Vectors, Deflect Normals, PCA Normal Estimation, Ribbon, Tube ★, Linear Extrusion, Linear Cell Extrusion, Rotational Extrusion, Volume Of Revolution, Clip Closed Surface, Iso Volume ★, Mask Points, Distribute Points, Generate Quadrature Points, Generate Quadrature Scheme Dictionary, Interpolate to Quadrature Points, Finite Element Field Distributor, Compute Sides
- [ ] Texture Map to Plane/Cylinder/Sphere, Perlin Noise, Spatio Temporal Harmonics
- [~] Resample To Image ★, Resample With Dataset ★ (done, matches ParaView point by point), Resample To Line, Resample AMR, Adaptive Resample To Image, Gaussian Resampling, Probe Location ★, Point Interpolator (Gaussian/Shepard/Voronoi/Linear kernels), Point Line/Plane/Volume/Dataset Interpolator, SPH Interpolator (+ Line/Plane/Volume/Dataset), Point Set To Octree Image, Octree Image to Point Set, Median, Equalizer Filter
- [~] Particle Tracer, Particle Path, Streak Line (done; exact on an analytic flow, where ParaView 6.1.1 is 0.25 rad off), Legacy Particle Path, Legacy Streak Line, In Situ Particle Path, Temporal Particles To Pathlines
- [~] Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator (done, checked against ParaView), Temporal Snap-to-TimeStep, Temporal Cache, Temporal Smoothing, Merge Time, Group Time Steps, Generate Time Steps, Force Time, Synchronize Time, Critical Time, Date To Numeric, Time Step Progress Bar, Animate Modes
- [ ] Hyper Tree Grid: Axis Reflection, Cell Centers, Depth Limiter, Evaluate Coarse, Feature Edges, Generate Fields, Geometry Filter, Ghost Cells Generator, Redistribute, To Dual Grid, To Unstructured Grid, Visible Leaves Size
- [ ] AMR: Connectivity, Contour, Dual Clip, Fragment Integration, Fragments Filter, Material Interface Filter, Intersect Fragments, Youngs Material Interface
- [ ] Cell Grid: Centers, Elevation, Probe, To Unstructured Grid, Transform, Warp
- [ ] Molecules: Compute Molecule Bonds, Molecule To Lines, EAM Project To Sphere
- [ ] Cosmology: ANL Halo Finder, ANL Subhalo Finder, LANL FOF/SOD Halo Finder
- [ ] Parallel/distributed: D3, Redistribute DataSet, Partition Balancer, Reduction Filter, Aggregate Dataset, Client Server Move Data, MPI Move Data, Pass Through, Force Static Mesh, Voronoi Merge Connected
- [~] Annotation (text, time label and ruler done): Annotate Attribute Data, Annotate Global Data, Annotate Selection, Annotate Time Filter, Environment Annotation, Python Annotation, Programmable Annotation, Text, 3D Text, Logo, Ruler, Protractor
- [~] Programmable: Programmable Filter ★ (Python), Programmable Source, Python Calculator ★ (done), Live Programmable Source, Color By Array, OME TIFF Channel Calculator, Threshold Table, Field Arrays From File

### 2.4 Sources
- [~] Geometric: Sphere ★, Cone ★, Cylinder ★, Box ★, Plane ★, Line ★, Disk, Ellipse, Arrow, Axes, Superquadric, Point Source, PolyLine Source, PolyPoint Source, Spline Source, Outline Source
- [~] Synthetic data: Wavelet ★, Mandelbrot, Fast Uniform Grid, Hierarchical Fractal, Hyper Tree Grid (+Random), AMR Gaussian Pulse, Data Object Generator, Unstructured Cell Types (done, counts match ParaView), Partitioned DataSet Collection Source, Cell Grid Source, Spatio Temporal Harmonics Uniform Grid, Time Source, Trivial Producer
- [ ] Selection sources (ID, global ID, pedigree ID, location, frustum, threshold, value, block, query, composite/hierarchical ID)

### 2.5 Filter infrastructure
- [~] Every filter has: typed properties, defaults, validation, documentation, search keywords, Python binding, undo entry, and progress reporting.
- [ ] GPU implementations (wgpu compute) for: contour, clip, slice, threshold, gradient, resample, glyph, stream tracer, particle advection, connectivity, histogram.
- [~] Custom filter authoring: Python (`@afx.filter`, done) and Rust plugins.

---

## 3. Rendering and views

### 3.1 Render view (`aurafx-render`, wgpu)
- [x] Camera: perspective/orthographic, trackball/terrain/joystick styles, reset/fit, standard views (+X,-X,...), isometric, roll, zoom to box, zoom to data, camera undo/redo, saved camera links.
- [x] Surface rendering with flat/Gouraud/PBR shading; backface handling.
- [x] Scalar coloring with color maps (ColorBrewer, viridis family, ParaView presets, imported `.json/.xml`), range modes (auto, custom, time-wide), log scale, discrete, NaN/above/below colors, opacity transfer functions, per-component or magnitude, categorical.
- [~] Representations: Surface, Surface With Edges, Wireframe, Points, Outline, Feature Edges, Point Gaussian (splats: sphere, gaussian, custom shader), 3D Glyphs, Volume, Slice (image), Slice (unstructured), Multi-slice, Orthographic slice, Molecule (ball/stick/VDW), Hyper Tree Grid, Cell Grid, Polar axes, Data labels, Selection highlight, Texture.
- [~] Volume rendering: GPU ray casting, smart/fixed-point, 2D transfer functions, shade, gradient opacity, multi-component, blend modes (composite, MIP, MinIP, additive, isosurface), cropping, clipping planes, unstructured-grid projected tetra / ray casting.
- [~] Lighting (light kit, Eye-Dome Lighting, MSAA and order-independent transparency done): key/fill/head/back light kit, custom lights with widgets, shadows, ambient occlusion (SSAO), Eye-Dome Lighting, tone mapping, FXAA/MSAA/TAA, depth peeling for correct transparency.
- [ ] Surface LIC, ray tracing (OSPRay/Embree- or Rust path tracer, optional), environment maps/skyboxes, materials editor.
- [~] Axes grid (data axes grid with ticks and labels, orientation axes, scalar bars and text done; custom bounds and units todo): Axes grid (data, custom bounds, labels, units), orientation axes, center-of-rotation marker, scalar bars (position, titles, number format, custom labels), legends, logos, text, interactive 2D annotations.
- [~] Widgets (implicit plane and ruler done): implicit plane, box, sphere, cylinder, cone, annulus, frustum, line, polyline, spline, handle/point, distance, protractor, ruler, slider, light, coordinate frame, four-lines, contour, display-sized plane, text, logo, equalizer, progress bar.
- [~] Selection: by click/rubber band/polygon on points/cells/blocks, through-frustum, by query (`field > value`, nearest, etc.), grow/shrink, invert, hover tooltips and interactive labels, "Find Data" panel.
- [~] Picking tools: point probe, coordinate readout, interactive cell/point info.
- [ ] Stereo (anaglyph, crystal eyes, interlaced, side-by-side), VR (OpenXR) later.
- [ ] Huge data: LOD, frustum/occlusion culling, instanced glyphs, GPU-resident meshes, mesh streaming, remote render with image streaming.

### 3.2 Other views
- [~] Line chart ★ (multiple series, log axes, styles, legends, markers, selection link), Bar chart, Point chart, XY bar chart, Histogram view, Box chart / Quartile chart, Plot matrix (scatter matrix), Parallel coordinates, Image chart, Comparative views (render/line chart), Slice view, Orthographic slice view, Spreadsheet view ★ (point/cell/field/row data, sort, filter, column visibility, precision, export CSV), Python view (matplotlib embedded), Spreadsheet of selection.
- [~] Layouts (split views, linked cameras, match view direction done): split horizontally/vertically, tabs, link cameras, link views, link selections, maximize, save/restore layouts, multiple windows/monitors.

### 3.3 Animation and time
- [~] Time toolbar (play, step, loop, time value entry, snap to steps, real-time, sequence modes), time inspector.
- [~] Animation view (camera keyframes, numeric property tracks with linear ramps, time scrubbing and preview done; easing modes, Python cue, comparative cues todo): keyframes for any property (ramp, linear, exponential, sinusoid, boolean), camera paths (orbit, path, interpolate), Python cue, comparative cues.
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
- [x] Color map editor with live histogram, drag handles, presets gallery, color-blind safe set, favorites, import/export (ParaView `.json`), reverse and discrete bands.
- [~] Onboarding (sample dataset done): sample datasets gallery, guided tours, tooltips with screenshots, "Explain this filter" via AI.
- [ ] Keyboard-first: fully rebindable shortcuts, vim-style view navigation optional.
- [~] Themes: dark/light/high-contrast, scalable UI (HiDPI), font size.
- [~] Session recovery after crash, autosave of state. (Last session restored on start.)
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
- [~] Bundled scientific stack management (packages ship Python 3.12 + NumPy): detect/install numpy, scipy, pandas, polars, matplotlib, h5py, netCDF4, scikit-learn, torch (CUDA/ROCm wheels), with a UI for environments.
- [x] Programmable filter/source with NumPy and typed dataset wrappers (`afx.Data`).
- [ ] Jupyter kernel exposure (drive auraFX from a notebook) and notebook panel inside the app.
- [~] Custom Python plugins (reader, filter, source done; panel todo) with hot-reload.

---

## 6. AI assistant (`aurafx-ai`)

- [x] Provider abstraction: Anthropic API, OpenAI-compatible API, Ollama (local and cloud models), with streaming.
- [~] Subscription/OAuth sign-in for hosted providers; API keys stored in the OS keyring.
- [x] Chat panel with Markdown rendering, code blocks with "Run in Python console", conversation history per project.
- [~] Context awareness: the model sees pipeline state, selected dataset summaries (arrays, ranges, bounds, time steps), camera, recent errors and logs, without uploading raw data unless asked.
- [~] Tools the model can call (agent mode acts through generated Python): load file, add filter, set property, color by, take screenshot (and view it), run Python, read a chart's data, search docs, run a simulation job.
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

- [~] Client/server split (data server and server-side rendering done: `aurafx-server` over SSH stdio, server-side pipeline with caching, surface/summary delivery, GPU-drawn color+depth images composited by depth; MPI todo): `aurafx-server` (data + render) over TCP/SSH tunnel with the same protocol as local.
- [~] Multi-process data parallel (all-core shared-memory execution done; multi-process parallel servers done: workers per piece, piece-wise reading of partitioned files, sort-last compositing; MPI launchers and ghost cells todo) (MPI optional; Rust-native work distribution first), ghost cells, D3 redistribution, compositing (sort-last), tile displays.
- [ ] Collaboration: shared sessions with multiple clients.
- [ ] In-situ: Catalyst/Conduit endpoint, live visualization of running jobs.

---

## 10. Quality, docs, community

- [~] Golden-image render tests, filter regression tests against VTK outputs, reader conformance suite with sample files for every format.
- [ ] Benchmarks vs ParaView (load time, filter time, frame rate) published in docs.
- [ ] User guide, filter reference (auto-generated from property metadata), Python API docs, tutorials, example gallery.
- [~] Contributing guide (done), code of conduct, issue templates, plugin cookbook (done: `docs/plugins.md`).

---

## Delivery phases

| Phase | Target | Contents |
|---|---|---|
| P0 Skeleton | done | Workspace, data model, legacy VTK + VTU/VTP + LAMMPS readers, wgpu render view, docking UI, command palette, Python console, AI chat (Ollama/Anthropic/OpenAI) |
| P1 Daily driver | mostly done | Exodus, PVD/time series, core ★ filters, color maps + scalar bar, spreadsheet + line chart, screenshots, state files, settings |
| P2 Analysis | mostly done | Statistics/data-analysis filters, selection + Find Data, Python API breadth, Python trace, programmable filter, charts breadth |
| P3 Simulation | mostly done (Slurm untested) | Job runner local + SSH, templates, sweeps, watch folders, remote file browser, GPU monitoring |
| P4 Rendering depth | in progress | Volume rendering, widgets, lighting/SSAO/EDL, animation + keyframes, exporters, LIC, ray tracing |
| P5 Breadth | started | Remaining readers/writers/filters from the catalog, AMR/HTG/molecule/cell-grid types, higher-order cells |
| P6 Scale | started (data server) | Client/server, distributed, in-situ, collaboration, out-of-core |
| P7 Polish | packaging done | Plugins, packaging on all OSes, accessibility, localization, docs and benchmarks |

---

## Status (2026-11-04)

Done in iteration 2 (on top of iteration 1 below): light/dark/system themes; Windows menu with
panel toggles and layout presets; undo/redo; `.afx` state files and last-session restore;
writers (VTK XML, legacy, CSV, STL, PLY); picking, rectangle selection, Extract Selection, Find
Data; 18 more filters including Stream Tracer and Tube; Temporal Statistics; GPU volume
rendering; animation export; interactive plane widget; AI agent mode; NetCDF-4 Exodus;
OpenFOAM, XYZ, LAMMPS data, PLY and OBJ readers; live file watching; parameter sweeps; CI.

Done in iteration 3: multiple 3D views side by side or stacked, with per-view visibility,
linked cameras and one-click "look from" directions shared across linked views; packaging for
Linux (tar.gz, AppImage, `install.sh`), macOS (`.dmg`), Windows (installer, zip, `install.ps1`),
Android (APK) and iOS (unsigned IPA) with a GitHub release workflow; desktop packages bundle
Python 3.12 + NumPy; volume rendering that depth-merges with opaque geometry; visible-only cell
selection through a GPU id pass; text annotations, time label and ruler; decomposed OpenFOAM
cases.

Done in iteration 4: sending objects to 3D views from the right-click menu; data axes grid;
keyframe animation (camera and filter values on a timeline, preview, export); OpenFOAM boundary
patches; Exodus II writer with all time steps; Extract Subset; curved quadratic cells. The
OpenFOAM patches, the Exodus writer and Extract Subset are checked against ParaView 6.1.1.

Done in iteration 5: color map editor (histogram, markers, bands, reverse, user presets,
ParaView `.json` import/export) and a readable scalar bar backing.

Done in iteration 6: multi-input filters in the pipeline (Ctrl+click several objects, then
Append; an Inputs section in Properties; `afx.append_datasets(a, b)`), with Append Datasets,
Append Geometry, Append Attributes and Resample With Dataset checked against ParaView 6.1.1.
Probing image and rectilinear data is now trilinear like VTK (it was split into tetrahedra),
Point/Cell Data conversions keep structured types and drop the source arrays as ParaView does
(a "Pass" option keeps them), and a `Cells::default()` bug that lost the first appended cell
is fixed.

Done in iteration 7: time-aware pipeline execution and the first temporal filters: Particle
Tracer, Particle Path and Streak Line (incremental while playing; seeds from any dataset or a
line), Temporal Interpolator (in-between and regular time steps), Temporal Shift Scale,
Temporal Array Operator and a live Temporal Statistics. Interpolator, Shift Scale and Array
Operator match ParaView 6.1.1. The particle filters are checked against the exact paths of
`testdata/rotating.pvd` instead: ParaView advects each interval with the velocity at its end
(4.25 rad instead of 4.0 at t = 2) and its Streak Line output mixes seeds and has empty cells;
auraFX interpolates the velocity linearly in time and builds one streak per seed. Particle ids
are unique per particle (ParaView restarts them at every release).

Done in iteration 8: Python plugins (`@afx.filter`, `@afx.source`, `@afx.reader` in `.py` files
from the plugin folder, `AURAFX_PLUGIN_PATH` or the Plugins window; hot reload re-runs nodes that
use them; errors shown per plugin), the Plugins window (Edit menu), `afx.Data` with NumPy arrays
and `afx.make_*` constructors, and the Programmable Filter, Programmable Source and Python
Calculator with a code editor in Properties. Text parameters now keep what you type until you
apply it (they used to lose keystrokes). Guide: `docs/plugins.md`.

Done in iteration 9: arbitrary-order Lagrange and Bezier cells. Node numbering matches VTK for
orders 1 to 6, including anisotropic orders. Extract Surface and the 3D view draw curved cells
through their nodes, with a "Curved cells" subdivision level on the Display panel and on
Extract Surface. Slice, Clip, Contour, Probe, Integrate Variables and Tetrahedralize use VTK's
linear sub-cells. New: the Tessellate filter, the Unstructured Cell Types source, a Lagrange
sample, and three curved test meshes (`scripts/gen_higher_order.py`).

Against ParaView 6.1.1, Integrate Variables, Slice, Clip, Contour and Extract Surface at levels
0-3 agree to about 1e-8 on Lagrange hexes, a rational Bezier hex and one cell of every other
shape. There are two deliberate differences:
- auraFX draws curved faces as quads, where ParaView splits them into triangles.
- On Bezier cells, ParaView integrates the control net (3.0 for the quarter cylinder) and
  skips Lagrange curves with an odd number of points; auraFX integrates the evaluated cells.

VTK XML files are now written as version 2.2, so ParaView reads our Lagrange hexes correctly.
Older files are converted on reading the way VTK does. Curved cells also orient their faces
from their corners, because VTK's Lagrange wedges are wound opposite to linear ones. Legacy
`.vtk` files cannot carry degrees or rational weights for VTK, so use `.vtu` for curved cells.

Done in iteration 10 (first part): serendipity quadratic cells. 20-node hexes, 15-node wedges
and 8-node quads use serendipity shape functions. Quadratic-linear quads and wedges and cubic
lines are treated as Lagrange cells. Extract Surface draws their faces with ParaView's pattern
at every level: same point counts at levels 0-3 and areas within 1e-7 on
`testdata/quadratic_shapes.vtu`. Slice and Contour match ParaView exactly. Integrate Variables
gives positive volumes where ParaView's are signed or wrong (inverted 15-node wedge,
quadratic-linear wedge 0.07 instead of 0.56, cubic line 1.48 for a 2.25 arc).

Done in iteration 10: client/server. `aurafx-server` is a headless program that the app starts
on a host over SSH (`aurafx-server --stdio`, so keys, jump hosts and `~/.ssh/config` apply and
no port is opened). Its parts:
- **Server side:** the server runs its own pipeline session. Each request carries the upstream
  recipe of the node it wants; the server re-runs only what changed and keeps the rest.
- **What comes back:** the data itself when it is under 64 MB, else its surface, or only a
  summary for hidden nodes. "Bring all data" fetches everything.
- **Opening files:** File › Open on Server… browses the host, checks the server and can
  install it. From Python: `afx.load(path, host=...)` or `ssh://host/path`.
- **Where filters run:** built-in filters below a server file (including temporal filters)
  run on the server; Python and plugin filters run locally on the data brought over.
- **Tests:** the results match local runs in tests through an in-process server, a server
  process and a real SSH host (a 32-core Ubuntu box). Guide: `docs/client-server.md`.

Done in iteration 11: the heavy filters use every core, locally and on servers.
- **Parallel and exact:** Slice, Contour, Clip, Threshold and Extract Surface run cell chunks in
  parallel; Stream Tracer traces its seeds in parallel. Each chunk numbers its own points; the merge restores the single-threaded order, so
  results are bit-identical on any number of threads (and were identical to the old serial code
  before the crack fix below changed how cells are split).
- **Extract Surface:** a new face-matching method for linear meshes takes 4M hexahedra from
  14.9 s to 0.19 s. The 3D view runs it for every volume mesh.
- **Reductions:** Integrate Variables and Gradient add fixed chunks in order. Results do not
  depend on the thread count and moved by about 1e-15 (the integrated volume got more accurate).
  Gradient now keeps image data as image data.
- **Image inputs:** structured-to-cell conversion is parallel.

Timings on 4M cells (16-thread laptop, old → new): slice 415 → 73 ms, contour 565 → 99 ms,
clip 704 → 194 ms, threshold 577 → 106 ms, integrate 1559 → 103 ms, gradient 1319 → 384 ms. On
a 32-core server, slice takes 49 ms, extract surface 109 ms and integrate 64 ms.

Settings › Performance limits the threads; so do `AURAFX_THREADS` and
`aurafx-server --threads N`. Benchmark: `cargo run --release -p aurafx-filters --example bench`.

Two existing bugs found and fixed along the way:
- **Cracked isosurfaces:** hexahedra, wedges and pyramids were split into tetrahedra with a fixed
  pattern, so neighbouring cells cut their shared faces along different diagonals. A wavelet
  contour had 45,000 open edges, and Tetrahedralize left inner faces. Quad faces are now cut
  through their smallest point id, the usual rule for matching splits. The split tables were
  found by brute-force search; every case the rule can produce has a 5- or 6-tetrahedron
  split. Contours are now watertight, and Clip splits its prisms the same way.
- **Crash on huge surfaces:** the 3D view crashed when a surface needed more than the GPU's
  largest vertex buffer (about 10M triangles at 256 MB). It now splits the buffers.

Done in iteration 12: drawing on the server. When a server object's surface would be too big
to transfer (over 64 MB), or Display › Draw on is "the server", the data stays there:
- **On the server:** it draws the object on its GPU and returns a color image plus a depth
  image for each view.
- **In the app:** the images are blended into the 3D view by depth, so local and server objects
  hide each other correctly. The last picture stays up while the camera moves.

The protocol carries each view's exact camera (including near/far), so server and app depths
agree. A server-drawn picture matched this laptop's own drawing pixel for pixel, both
in-process and from the 32-core host's Radeon GFX1151 over SSH (31 ms to draw 640×480, 64 ms
round trip). `aurafx-server` finds its GPU at start (`--no-gpu` to opt out), and its version
check reports it.

Two existing bugs were fixed along the way:
- **Script objects hidden with several views open:** drawing an inactive view swapped
  visibility for every node, so objects a script created during that frame ended up hidden.
- **Server binary needs new glibc:** a binary built on a current distribution requires glibc
  2.43 (re-versioned float math functions). The release packages are built on Ubuntu 22.04,
  and the installer now explains this.

Done in iteration 13: parallel (multi-process) servers. `aurafx-server --workers N` (or
`host:N,...` over ssh) coordinates worker processes, each holding one piece of every file. From
the app: Open on Server › Processes; from Python: `afx.load(..., processes=N)`.
- **Reading:** partitioned VTK files are read piece by piece. Other data is read, then split
  (`aurafx_data::pieces`: blocks, cell ranges or slabs of structured data, reassembled exactly).
- **Filters:** those that work cell by cell run on every piece. Their automatic values (centers,
  ranges, sizes) come from the whole data.
- **Results:** data is appended, surfaces are merged without the faces between pieces,
  summaries are added up, and pictures are composited by depth. Workers draw without
  multisampling at double size, so pieces meet without seams.
- **Other filters** run on the coordinator, on the gathered data.
- **Tests:** results match one process with in-process workers and with real worker processes:
  integrals to 1e-9, the same surface faces, and pictures agreeing away from outlines.

Two side fixes came out of this:
- **Tetrahedral splits:** cells are now split by the position of their corners rather than by
  point id, so results no longer depend on how points are numbered.
- **Test race:** a test no longer changes a process-wide variable while another test runs.

Done in iteration 14 (rendering depth, back in roadmap phase order):
- **Order-independent transparency:** translucent surfaces, lines and points are drawn with
  weighted blended transparency (McGuire & Bavoil). They accumulate into two targets that are
  composited over the scene, so the picture no longer depends on drawing order (a test swaps the
  order and gets identical pixels).
- **Eye-Dome Lighting** (View menu, `afx.lighting(edl=True)`): outlines and depth cues for point
  clouds, particles and dense meshes. It runs after the opaque pass, so pictures drawn on a
  server get it too.
- **Light kit:** key, fill and head lights, on by default; it brightens the side facing away
  from the key light.
- **Opaque points and lines** now write depth, so they hide behind surfaces and take part in EDL.

The frame is now an opaque pass, EDL, volumes, transparency accumulation and composite,
overlays, then one multisample resolve. A test-only crash on exit was also fixed: in-process
GPU servers are now shut down before the test process ends.

Known gaps worth doing next: SSAO, shadows, FXAA/tone mapping and custom lights (P4); Surface
LIC and ray tracing (P4); MPI launchers (srun) and ghost cells for parallel servers;
temporal filters on parallel servers; translucent and volume objects drawn on the server; parallel Probe/Resample and readers; curved pyramids in cutting filters and distributed execution; native (Rust) plugins and plugin panels; Slurm runs not yet exercised on real
infrastructure; the macOS, Windows and iOS packaging jobs have only been written, not yet run.

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

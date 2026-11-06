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
- [~] Composite (MultiBlock done): `MultiBlock`, `PartitionedDataSet`, `PartitionedDataSetCollection`, overlapping and non-overlapping AMR, `HyperTreeGrid` (read and written as `.htg`; To Unstructured Grid, To Dual Grid, Depth Limiter, Generate Fields, Evaluate Coarse; descriptor and random sources with masks), `Molecule`, `CellGrid` (DG cells: HGRAD/HCURL/HDIV/constant fields on eight shapes; read from `.dg`, drawn at the corners; Centers, Elevation, source).
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
- [x] PNG/JPEG/BMP/TIFF images and stacks as `ImageData` (multi-page TIFF and `img_*.png` wildcards as volumes; iteration 18).
- [~] VTKHDF (`.vtkhdf`: image data, unstructured grids, poly data, partitioned collections with assemblies, time steps, gzip; iteration 18, checked against ParaView); HDF5 generic browser todo.

### 1.2 Tier 2 (simulation ecosystem)
- [x] OpenFOAM (`.foam` case, ASCII/binary, decomposed and reconstructed, time directories, boundary patches with boundary values; verified against ParaView).
- [~] EnSight Gold (`.case`, ASCII and C binary, all element types, blocks, time sets; iteration 18), Tecplot ASCII (iteration 18) and binary (iteration 20), PLOT3D (iteration 19), Xdmf2/3 (iteration 19), CGNS (HDF5, iteration 20; ADF, iteration 24), Fluent `.cas/.msh`, Nastran/BDF, ANSYS `.inp`, LS-DYNA `d3plot`, Silo, Xdmf2/3, PLOT3D, Nek5000, MFIX, ADIOS2 BP4/BP5, openPMD, Fides, Conduit/Catalyst.
- [~] NetCDF CF (iteration 19: classic and NetCDF-4, lon/lat on a sphere with bounds, time, packing, fill values); MPAS (iteration 25; vertical levels, multilayer and lon/lat map in iteration 27); CAM-SE and POP (iteration 28); GeoTIFF and Esri ASCII grid rasters (iteration 26); GIS vectors: Shapefile, GeoJSON, KML, GeoPackage (iteration 31), KMZ, FlatGeobuf, GML 2/3/3.2 (iteration 38); other GDAL rasters, MapInfo, file geodatabases, SEG-Y todo.
- [~] Molecular: PDB and Gaussian Cube (iteration 18), XYZ molecules (done); VASP POSCAR/CONTCAR/CHGCAR/LOCPOT/ELFCAR (iteration 19); CML, OUTCAR todo.
- [~] Cosmology: Gadget, GenericIO, Cosmo, Enzo/FLASH AMR (iteration 39), Chombo (iteration 40), AMReX plotfiles (iteration 32), FLASH and Enzo particles (iteration 42), Chombo particles and node-centered Chombo (iteration 43).
- [ ] Medical/imaging: DICOM, NRRD, MetaImage (`.mhd`), MRC, OME-TIFF, NIfTI.
- [ ] Point clouds: LAS/LAZ (PDAL), PTS, H5Part.
- [ ] CAD: STEP, IGES (via OpenCASCADE feature), VRML, Alembic, USD, OpenVDB.
- [ ] Every remaining reader in the catalog, tracked per entry in `docs/reference/format-matrix.md`.

### 1.3 Writers
- [~] VTK XML (all dataset types, parallel pieces, `.pvd`), VTK legacy, VTKHDF (iteration 21; read by ParaView 6.1.1 since iteration 22).
- [~] Exodus II (done: element blocks, nodal/element variables, all time steps; verified in ParaView), EnSight Gold and Xdmf 3 (iteration 22), CGNS/HDF5 (iteration 23), all verified in ParaView; OpenVDB, GenericIO, Fides.
- [~] CSV, STL, PLY (done), OBJ, glTF, Houdini `.geo`.
- [~] Images: PNG (JPEG/TIFF/BMP/EXR todo), screenshot with resolution override and transparent background (done, iteration 16).
- [~] Animation: PNG sequence, MP4 (ffmpeg), GIF (done); WebM.
- [~] Scene export: glTF (.glb), OBJ with vertex colors (iteration 16), X3D, VRML and SVG vector pictures (iteration 17); POV-Ray, PDF, WebGL/HTML interactive scene todo.
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
- [x] Texture Map to Plane/Cylinder/Sphere, Perlin Noise, Spatio Temporal Harmonics (filter and source; checked against ParaView), plus the display Texture (an image on a surface)
- [~] Resample To Image ★, Resample With Dataset ★ (done, matches ParaView point by point), Resample To Line, Resample AMR (iteration 41), Adaptive Resample To Image, Gaussian Resampling, Probe Location ★, Point Interpolator (Gaussian/Shepard/Voronoi/Linear kernels), Point Line/Plane/Volume/Dataset Interpolator, SPH Interpolator (+ Line/Plane/Volume/Dataset), Point Set To Octree Image, Octree Image to Point Set, Median, Equalizer Filter
- [~] Particle Tracer, Particle Path, Streak Line (done; exact on an analytic flow, where ParaView 6.1.1 is 0.25 rad off), Legacy Particle Path, Legacy Streak Line, In Situ Particle Path, Temporal Particles To Pathlines
- [~] Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator (done, checked against ParaView), Temporal Snap-to-TimeStep, Temporal Cache, Temporal Smoothing, Merge Time, Group Time Steps, Generate Time Steps, Force Time, Synchronize Time, Critical Time, Date To Numeric, Time Step Progress Bar, Animate Modes
- [~] Hyper Tree Grid (iteration 34: type, .htg reader, both sources, To Unstructured Grid, Depth Limiter, Generate Fields, Evaluate Coarse, Cell Centers; iteration 37: .htg writer, To Dual Grid, masked random source; iteration 44: Geometry, Feature Edges): Axis Reflection, Cell Centers, Depth Limiter, Evaluate Coarse, Feature Edges, Generate Fields, Geometry Filter, Ghost Cells Generator, Redistribute, To Dual Grid, To Unstructured Grid, Visible Leaves Size
- [~] AMR (foundations, readers, Gaussian pulse, level ids in iteration 32; crack-free Contour and Dual Grid in 33): Connectivity, Contour, Dual Clip, Fragment Integration, Fragments Filter, Material Interface Filter, Intersect Fragments, Youngs Material Interface
- [~] Cell Grid (iteration 36: Centers, Elevation, To Unstructured Grid, source; iteration 45: Transform, Warp, Convert To Cell Grid, .dg writer): Centers, Elevation, Probe, To Unstructured Grid, Transform, Warp
- [~] Molecules (iteration 35: Molecule representation, CML reader, Compute Molecule Bonds; molecules are polydata, so Molecule To Lines / Convert Into Molecule are not needed): Compute Molecule Bonds, Molecule To Lines, EAM Project To Sphere
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
- [x] Lighting (iterations 14–15): light kit, custom lights (camera-relative or fixed, colored), shadows, screen-space ambient occlusion, Eye-Dome Lighting, ACES/Reinhard tone mapping and exposure, MSAA + FXAA, order-independent transparency. Light widgets done in iteration 16. Not done: TAA. Original list: key/fill/head/back light kit, custom lights with widgets, shadows, ambient occlusion (SSAO), Eye-Dome Lighting, tone mapping, FXAA/MSAA/TAA, depth peeling for correct transparency.
- [x] Surface LIC and ray tracing (Rust path tracer, iteration 15); environment maps as light and background, materials (metallic, roughness, specular) in Properties, GPU path tracing (iteration 16).
- [~] Axes grid (data axes grid with ticks and labels, orientation axes, scalar bars and text done; custom bounds and units todo): Axes grid (data, custom bounds, labels, units), orientation axes, center-of-rotation marker, scalar bars (position, titles, number format, custom labels), legends, logos, text, interactive 2D annotations.
- [~] Widgets (implicit plane, box, sphere, line, point, light and ruler done): implicit plane, box, sphere, cylinder, cone, annulus, frustum, line, polyline, spline, handle/point, distance, protractor, ruler, slider, light, coordinate frame, four-lines, contour, display-sized plane, text, logo, equalizer, progress bar.
- [~] Selection: by click/rubber band/polygon on points/cells/blocks, through-frustum, by query (`field > value`, nearest, etc.), grow/shrink, invert, hover tooltips and interactive labels, "Find Data" panel.
- [~] Picking tools: point probe, coordinate readout, interactive cell/point info.
- [ ] Stereo (anaglyph, crystal eyes, interlaced, side-by-side), VR (OpenXR) later.
- [ ] Huge data: LOD, frustum/occlusion culling, instanced glyphs, GPU-resident meshes, mesh streaming, remote render with image streaming.

### 3.2 Other views
- [~] Line chart ★ (multiple series, log axes, styles, legends, markers, selection link), Bar chart, Point chart, XY bar chart, Histogram view, Box chart / Quartile chart, Plot matrix (scatter matrix), Parallel coordinates, Image chart, Comparative views (render/line chart), Slice view, Orthographic slice view, Spreadsheet view ★ (point/cell/field/row data, sort, filter, column visibility, precision, export CSV), Python view (matplotlib embedded), Spreadsheet of selection.
- [~] Layouts (split views, linked cameras, match view direction done): split horizontally/vertically, tabs, link cameras, link views, link selections, maximize, save/restore layouts, multiple windows/monitors.

### 3.3 Animation and time
- [~] Time toolbar (play, step, loop, time value entry, snap to steps, real-time, sequence modes), time inspector.
- [~] Animation view (camera keyframes, numeric filter and display property tracks, linear/step/ease/exponential interpolation per key, Python cue, time scrubbing and preview done; sinusoid and comparative cues todo): keyframes for any property (ramp, linear, exponential, sinusoid, boolean), camera paths (orbit, path, interpolate), Python cue, comparative cues.
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
| P5 Breadth | in progress | Remaining readers/writers/filters from the catalog, AMR/HTG/molecule/cell-grid types, higher-order cells |
| P6 Scale | started (data server) | Client/server, distributed, in-situ, collaboration, out-of-core |
| P7 Polish | packaging done | Plugins, packaging on all OSes, accessibility, localization, docs and benchmarks |

---

## Status (2026-11-05)

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

Done in iteration 15 (the rest of the lighting list):
- **Custom lights** (View › Lighting › Custom lights, `afx.lighting(lights=[...])`): any number
  of colored directional lights (up to 8 drawn), each following the camera or fixed in the
  scene, set by azimuth/elevation; ambient level.
- **Shadows** from the key light (or the first custom light with shadows ticked): a 2048² shadow
  map fitted to the visible objects, filtered over 3×3 texels.
- **Ambient occlusion** (screen-space, from depth alone): 16 samples per pixel in a 4×4 pattern,
  then a depth-aware blur, so contact areas and creases darken without grain or halos.
- **FXAA, tone mapping (Reinhard, ACES filmic) and exposure** in a final pass.
- Lights and shadows go with server render requests (protocol 3), so server-drawn pictures are
  lit like local objects; EDL, ambient occlusion, FXAA and tone mapping run here over the
  composited picture. Shadows are per machine: server-drawn objects don't cast shadows on local
  ones.

- **Surface LIC** (representation "Surface LIC", `node.set(representation="Surface LIC",
  lic_vectors="V")`): the vector field on any surface drawn as streaks, colored by any array as
  usual. Done in screen space like VTK: the surface's projected tangent directions go into a
  float target, then noise is convolved along them for a set streak length; streaks stop at
  silhouettes and occluders. Options: vector array, strength, streak length. LIC objects on a
  server are drawn here.

- **Ray tracing** (View › Lighting › Ray tracing, `afx.lighting(ray_tracing=True,
  ray_samples=256)`): a CPU path tracer (binned-SAH BVH, rayon) for the opaque surfaces of a
  view, with the view's lights as soft-shadowed sun lights, the ambient level as a sky, and
  diffuse bounces. It refines while the view is still (the rasterizer draws while the camera
  moves) and its color + depth picture is composited like a server picture, so edges, points,
  lines, translucent objects and overlays are still drawn on top. Traced depth is offset like
  the rasterizer's polygon offset so edges stay visible. Scripted screenshots wait until it
  finishes, and in general until the data, camera and lighting the script set are in the
  picture. Tests: same depth and brightness as the rasterizer on a lit floor (ratio 0.98),
  shadow under a ball, noise 5x lower after 32 passes, deterministic.

Done in iteration 16 (exporters):
- **Screenshots at any size, with a transparent background** (File › Save Screenshot, or
  `afx.screenshot(path, width=3840, transparent=True)`; give one of width/height to keep the
  view's shape). Transparent pictures are rendered premultiplied and divided by coverage when
  saved, so anti-aliased edges keep their color. With ray tracing on, the picture is traced at
  the asked size first.
- **`afx.screenshot` waits until the file is written** (and raises if it can't be), so a
  script can change the camera, lighting or data between screenshots.
- **Scene export** (File › Export Scene, `afx.export_scene("scene.glb")`): the visible
  surfaces, lines and points with their display colors as glTF 2.0 binary (vertex colors,
  normals, translucency, unlit objects, large coordinates in node translations) or OBJ with
  vertex colors. ParaView 6.1 reads both with the same points, cells and bounds.

- **3D widgets** for the active object: box (Clip by box, Box source: center and six face
  handles), sphere (Clip/Slice by sphere, Stream Tracer point-cloud seeds, Sphere source:
  center and radius), line (Plot Over Line, Stream Tracer line seeds, particle tracers, Line
  source: both ends and the middle), point (Probe Location), and the existing plane. Handles
  follow the pointer in the plane facing the camera, box faces move along their axis.
- **Light widgets**: custom lights appear as sun handles around the objects; dragging one aims
  that light (camera-relative lights stay camera-relative). View › Lighting › Show lights.

- **Keyframes:** each key says how it moves on to the next one: linear, step (hold), ease in,
  ease out, ease in-out, or exponential (constant ratio, for scales and zooms); a menu per key
  in the Animation window, `node.key(..., interp=)`, `afx.key_camera(t, interp=)`. Display
  properties (opacity, colors, point size, line width, LIC strength, volume density) can be
  keyed like filter values. Older state files load with linear keys.
- **Python cue:** `afx.save_animation("out.mp4", cue=lambda t: ..., frames=120)` calls the
  function before each frame (after the keyframes) and returns when the file is written; the
  script renders frames with the waiting screenshot and the app encodes them.

- **Materials:** metallic, roughness and highlight strength per object (Properties ›
  Material, `node.set(metallic=1, roughness=0.2)`); the defaults reproduce the earlier
  shading exactly. Metals reflect in their own color; glTF export carries the factors.
- **Environment maps** (View › Lighting › Environment, `afx.lighting(environment="sky")`):
  built-in sky, studio and white, or an equirectangular .hdr/.png/.jpg. The environment
  replaces the flat ambient light with its diffuse light (spherical harmonics), adds
  reflections blurred by roughness (mip levels), and can be shown as the background. The
  built-in lights are dimmed by half while an environment is on. The path tracer uses the same
  materials (a glossy lobe for reflections) and the environment for rays that leave the scene;
  in a white room both give a matte grey its own color, and a metal ball reflects the sky above
  and the ground below.

- **GPU path tracing:** the same path tracer as a compute shader (threaded BVH without a
  stack, triangle corners in their own buffer); used by default, with the CPU tracer when the
  scene doesn't fit the GPU's buffers or when chosen (View › Lighting › Ray tracing › CPU,
  `afx.lighting(ray_device="cpu")`). Its pictures match the CPU tracer's to 1.3 levels on 8×8
  blocks, with the same depth. On this laptop (RX 5500M against 16 CPU threads) both run an
  800×600, 378k-triangle frame in about 18 ms per sample; the GPU leaves the CPU free for
  filters.

Done in iteration 17:
- **Screenshots and animation frames include the overlays**: color legend, axes grid labels,
  time label, text annotations and orientation axes. They are painted by the same code as in
  the view, through an offscreen egui pass at the picture's scale, so a 4× screenshot has the
  same layout with sharp text. `afx.screenshot(..., annotations=False)` or the Save Screenshot
  dialog leaves them out.
- **SVG vector pictures** (`afx.screenshot("figure.svg")`, or SVG in Save Screenshot): the
  visible triangles flat-shaded with the view's lights and sorted back to front, lines and
  points on top, and the overlays converted from the same egui shapes (text stays text).
  Where surfaces cut through each other, sorting by depth leaves jagged boundaries, as with
  ParaView's GL2PS export. PDF: convert the SVG (Inkscape, rsvg-convert).
- **X3D and VRML** scene export (`.x3d`, `.wrl`), with absolute coordinates; ParaView reads the
  VRML file with the same points, cells and bounds as the glTF.
- **Wave keyframes:** a key that swings to the next key's value and back (sinusoid).

Done in iteration 18 (P5 breadth: readers), each compared with ParaView 6.1.1's reader:
- **VTKHDF** (`.vtkhdf`, pure Rust through `hdf5-reader`): image data, unstructured grids, poly
  data (several parts), partitioned collections nested by their assembly, time steps from the
  `Steps` group, contiguous, chunked and gzip-compressed storage. Counts, bounds and ranges
  match ParaView on its own exports (a disk merged to one grid, Exodus blocks, a 3-step series).
- **Images**: PNG, JPEG, BMP, TIFF (8/16-bit, float, gray/RGB/RGBA) as image data with
  ParaView's array names and row order; multi-page TIFFs and wildcard paths
  (`afx.load("slices/img_*.png")`, natural order) as volumes. ParaView 6.1.1 crashes on the
  multi-page 16-bit TIFF that VTK writes; VTK's writer also labels its rows top-down while
  storing them bottom-up, and we follow the label as image viewers do.
- **EnSight Gold** (`.case`): ASCII and C binary (either byte order), every standard element
  type (wedges turned round as VTK does), nsided/nfaced, structured blocks, per-node and
  per-element scalars/vectors/tensors, time sets with wildcard file names; one block per part.
- **Tecplot ASCII**: ordered and FE zones, POINT/BLOCK packing, cell-centered variables,
  old-style FEPOINT headers, multi-line headers, `n*value` repeats, Fortran exponents (the last
  three confuse ParaView's reader, which loses the zone).
- **PDB** (all atoms, hydrogens included, MODELs as time steps, CONECT plus distance bonds) and
  **Gaussian cube** (density volume and atoms in Å; ParaView keeps grid-index units).

Done in iteration 19 (more readers):
- **Xdmf 2/3**: inline, HDF5 and raw binary data; mixed and single-type unstructured,
  curvilinear, rectilinear and image topologies; spatial and temporal collections;
  `xi:include`/`Reference` XPath pointers (the way FEniCS and Dolfinx share one mesh across
  time steps); HyperSlab and JOIN items. Matches ParaView on its own exports; ParaView drops a
  HyperSlab field and fails on a curvilinear Xdmf 2 grid, both read here.
- **NetCDF CF** (classic and NetCDF-4): the grid most variables share, lon/lat on a unit sphere
  with cells between the `bounds` edges (as ParaView), rectilinear otherwise, time steps from the
  time coordinate, `scale_factor`/`add_offset`, fill values as NaN. Counts, ranges and values
  match ParaView on the IPCC sea surface temperature example and on generated files. Exodus
  files with a `.nc` name still open as Exodus.
- **PLOT3D**: grids and Q solutions, the binary layout found from the file size (Fortran or C
  binary, byte order, precision, multi-grid, 2-D/3-D, IBLANK), ASCII too; Density, Momentum,
  StagnationEnergy, Pressure and Properties as in ParaView.
- **VASP**: POSCAR/CONTCAR (VASP 4 and 5, direct or Cartesian, scale or volume) and
  CHGCAR/LOCPOT/ELFCAR/PARCHG volumes on the cell's axes (charge per Å³), known by file name.

Fixed on the way: NaN values were drawn in the color map's low color (the shader's `x != x`
test was optimized away by the GPU compiler; now a bit-pattern test, with a test), and
`afx.view(...)` followed by `afx.reset_camera()` lost the view (one request slot; a pending
view now stays, since it fits the data anyway).

Done in iteration 20:
- **CGNS** (HDF5 storage, pure Rust): bases and zones as blocks; structured zones; unstructured
  zones with every standard element type (quadratic node orders converted), MIXED sections
  with or without CGNS 4 offsets, NGON/NFACE polyhedra; volume elements only (boundary
  sections left out, as ParaView's default); vertex and cell-center solutions with X/Y/Z
  components joined into vectors; time steps from the iterative-data pointers. Matches
  ParaView's CGNS reader on its own export and on a hand-made file with a structured zone,
  a MIXED section, a boundary section and two time steps. (ADF storage: iteration 24.)
- **Binary Tecplot** (`.plt`, format 1xx): ordered and FE zones, node- and cell-centered
  variables of any storage type, shared variables and connectivity, auxiliary data. A file
  written from the spec reads the same as its ASCII twin; VisIt's reader in ParaView agrees on
  everything but the double-precision cell-centered variable, which it misreads. SZL
  (`.szplt`) files are not read.

Done in iteration 21: a VTKHDF writer on auraFX's own small HDF5 encoder (`node.save("x.vtkhdf")`,
`all_times=True`, the Save dialog; read back by h5py and our reader, not yet by ParaView's
reader), and the **user guide** (`docs/userguide`: Markdown sources, mdBook HTML, PDF) with a
generated screenshot per feature (`scripts/userguide/shots.sh`, `build.sh`). Rule in CLAUDE.md:
every feature ships with a guide entry and screenshot.

Done in iteration 22: **EnSight Gold writer** (C binary `.case`; parts per block, structured
blocks, every element type, polygons and polyhedra, time series with the geometry written once
when it does not change) and **Xdmf 3 writer** (`.xmf` + `.h5`; Co/Rect/SMesh, single-type or
Mixed topologies with polyhedra, collections, temporal collections sharing unchanged points and
cells); the Xdmf reader reads Mixed polyhedra. Both read by ParaView 6.1.1 with the same counts,
bounds and ranges. VTKHDF output now opens in ParaView: our HDF5 groups did not track link
creation order, which VTK iterates by, and ParaView's HDF5 then allocated until the machine
swapped. Fixed along the way: `afx.active_view()` raced the app (per-view show/hide from
scripts), blocking updates (`save(all_times=True)`, values over time) could use a stale output
while the app's worker held the node, and a color array set before the data arrived was looked
for in the wrong attribute set. `cargo run -p aurafx-io --example inspect -- file [step]` prints
what a file holds.

Done in iteration 23: **CGNS writer** (HDF5, CGNS 4 layout: structured zones, one element
section per zone (typed, or MIXED with offsets) plus NGON/NFACE for polygons and polyhedra,
bases by cell dimension, vertex and cell-center solutions with vectors as X/Y/Z, time series
with per-step solutions and moving coordinates through `ZoneIterativeData` pointers); read by
ParaView 6.1.1 with the same counts, bounds and ranges. `afx.split_view()` waits for the new
view and returns its number (two splits in a row split the same view before).

Done in iteration 24: **CGNS in ADF storage** (the pre-HDF5 format, same `.cgns` extension,
told apart by the file header): the 64-bit "B" layout the CGNS library writes today and the
old 32-bit "A" layout with hex-text pointers, little- or big-endian data, data split into
chunks, links inside the file and to other ADF files. The reader walks ADF nodes and HDF5
groups through one node type, so everything the CGNS reader does works for both. The test
files are written by the CGNS library in ParaView 6.1.1 (`scripts/gen_cgns_adf.py`); an
old-layout file made from one of them (`scripts/gen_adf_old.py`) is read back by that library
to the same values. Every file the CGNS writer produces, copied to ADF, reads exactly as the
HDF5 original, and ParaView reports the same counts, bounds and ranges on the ADF copies.

Done in iteration 25: **MPAS** NetCDF output (ocean, atmosphere, land ice), found from the
file's variables: the dual grid as ParaView's MPAS reader builds it by default (cell centers as
points, `cellsOnVertex` triangles or quads), cell variables as point data and vertex variables
as cell data at the first index of extra dimensions (the top level), numbered time steps,
`xtime` in field data. Boundary triangles and triangles touching land (`maxLevelCell`) are left
out; ParaView keeps them collapsed onto an extra point at the origin. On an MPAS-style test
file (`scripts/gen_mpas.py`) every point array and every cell ParaView keeps match exactly.
Tecplot SZL is set aside: ParaView 6.1.1 has no reader to compare with and the format is not
published. Reader options (MPAS vertical level, multilayer and lon/lat views) need a reader
parameter path through the pipeline first.

Done in iteration 26: **GIS rasters** without GDAL: GeoTIFF (found from its GeoTIFF tags; any
integer or float bands, compressed, strips or tiles, pixel- or band-interleaved, tie point and
scale or a transformation matrix, pixel-is-point moved to the corner as GDAL does) and Esri
ASCII grids, as ParaView's GDAL raster reader lays them out: image data with a cell per pixel
at map coordinates, north up, bands named "Elevation", "Band 2"..., RGB(A) as one array,
`NO_DATA_VALUE` and `MAP_PROJECTION` (EPSG code) in field data. No-data becomes NaN (ParaView
hides the cells). Test files are written by the GDAL library in ParaView through ctypes
(`scripts/gen_rasters.py`); extents, origins, spacings, first/last values and sums match
ParaView. The `tiff` crate reads only the first plane of band-interleaved images, so those are
put together chunk by chunk. ParaView's GDAL build has no ASCII grid driver; that file is
checked against the GeoTIFF it was written from.

Done in iteration 27: **reader options**. A reader can offer settings for a file
(`aurafx_io::reader_options`); they are the reader node's parameters, shown under Properties ›
Reader options, set from Python with `node.set(...)` (unknown keys are refused with the list),
saved in state files and undo history, and applied by reading again (static files) or with each
time step (series), through a thread-local like the piece setting. First user: **MPAS** vertical
level, multilayer view (wedge/hex columns, topography per level), layer thickness (0 = automatic),
atmosphere (layers outwards) and the lon/lat map with cells crossing the edge mirrored to both
sides and a chosen center longitude. Checked against ParaView's MPAS reader with the same
settings: level 2 ranges, map point/cell counts and bounds (both centers), multilayer map depth.
ParaView 6.1.1's multilayer point values are uninitialised memory and it never drops land
layers (unsigned comparison); both noted in the guide. Options are not sent to remote servers yet.

Done in iteration 28: **CAM-SE** (data file plus connectivity file, found beside it or named in
a reader option): ParaView's CAM reader layout on a lon/lat map, seam quads moved to their
shorter side on copied points (ParaView's inverted-cell test and anchor rule), surface /
midpoint / interface variables, all levels as hexahedra with a level array or one level, and a
sphere layout ParaView lacks. **POP** and other curvilinear CF files: 2-D longitude/latitude
from the `coordinates` attribute put the grid on the sphere, a vertical axis as radius with
ParaView's rule (positive="down" negated, shifted to non-negative; now also for 1-D lon/lat
grids), the auxiliary coordinates kept out of the data, and a "Variables on" reader option to
pick the grid (ParaView's Dimensions). ParaView's own POP reader reads only 3-D variables, so
POP history files are compared with its NetCDF reader. CAM checked in all four of ParaView's
modes (counts, bounds, first cell, array sums); POP on both grids (bounds, ranges).

Done in iteration 29: **reader options on servers** (protocol 8). Reader nodes already sent
their parameters with each request and the server applies them to its reader node, which now
re-reads with them; what was missing is the client knowing the options. Each Compute answer
lists, per reader in the request, the options of its file as the server sees it (cached per
path, sent as plain `aurafx_io::ReaderOption`s: `ParamValue`'s untagged serde form cannot cross
postcard), stored on the client's server node; Properties and Python use them (Python accepts
any name until the first answer). Parallel servers pass them to every process; saved states
keep server nodes' parameters (they were dropped on load before). Tested in-process, with a
3-process parallel server, and through a real aurafx-server process (the guide screenshot).

Done in iteration 30: **texture mapping and made-up data**. Texture Map to Plane (least-squares
plane or origin/point 1/point 2), Cylinder (axis from the oriented bounding box, with VTK's
Jacobi eigenvector signs, or two points) and Sphere (center = mean of the points), each with
Prevent Seam, ported from VTK so all 14 cases on two surfaces match ParaView 6.1.1 to float
precision. The display gets a **Texture** (PNG/JPEG, mipmapped, repeating): texture
coordinates travel packed as two halves in the vertex scalar, so the vertex layout is
unchanged; the image replaces array colors, lighting still applies (ray tracing, scene export
and server-drawn pictures show the solid color). **Perlin Noise** (VTK's Graphics Gems
version, bit-exact hashing) and **Spatio Temporal Harmonics** as a filter (input's time; time 0
on static data, as ParaView) and as a source with **time steps of its own**: sources can now
be temporal (their steps join the animation, they recompute per step, saved states keep
them). Values match ParaView's filters and source (default harmonics and times as ParaView's
rounded defaults).

Done in iteration 31: **GIS vectors** without GDAL: ESRI Shapefiles (.shp/.dbf/.prj; all
shape types but multipatch, dBASE field typing as GDAL), GeoJSON (field order and typing as
GDAL), KML (folders as layers, Name/Description/extended data typed by Schemas) and GeoPackage
(own read-only SQLite reader: b-tree pages, overflow chains, rowid aliases; GPKG geometry
blobs and WKB with Z/M), laid out as ParaView's GDAL vector reader (vertices, polylines,
polygon rings as closed lines, attributes as cell data). Reader options: one block per layer
(default) or per feature (ParaView's), layer choice, filled polygons (ear clipping with hole
bridges, exact areas on a 20 000-point polygon with holes), feature ids. With one block per
feature every point, cell size and attribute matches ParaView 6.1.1 on GDAL-written shapefiles,
GeoJSON and GeoPackage. Also fixed: `node.set` now handles the documented display keys
(`solid_color`, `range`, `log_scale`, `component`, `line_width`, `lighting`, ...; before they
were silently stored as parameters) and `color_by=None` sticks when set before the data loads.

Done in iteration 32: **AMR foundations**. Overlapping AMR is a multi-block of levels of image
boxes (`aurafx_data::amr`: boxes in level index space, refinement ratios from spacings,
blanking of covered coarse cells as `vtkGhostType` REFINEDCELL, a root marker). Cells VTK
treats as invisible (REFINED|HIDDEN) are skipped by the shared cell loop (slice, contour,
clip, threshold, integrate, surface) and by the probe locator (Resample To Image, probes), and
image data with hidden cells takes the general surface path; Extract Surface now works block
by block (cell data kept). Readers: VTK `.vthb`/`.vth` and AMReX plotfiles (Version_v1 FABs,
float/double, either byte order, `amrexvec_` vectors). Source: AMR Gaussian Pulse (2-D/3-D);
filter: Overlapping AMR Level Ids; Outline draws every box. Checked against ParaView 6.1.1:
source (cells, sums, covered cells), both readers (per-box origin, size, sums), Integrate
Variables, slices, Threshold, Extract Surface, Cell Data to Point Data, Resample To Image.
Array parameters given by name now find cell arrays too (shared `array_or_default`).

Done in iteration 33: **AMR Contour and AMR Dual Grid**. The dual grid joins the cell centers
of all levels; dual cells of a finer level whose corners fall outside its boxes take the
coarser cells' centers there (degenerate hexes), and dual cells touching covered cells are
left to the finer level, so contours (and clips, slices on the dual grid) are crack-free
across levels. Contour on an AMR cell array uses it automatically. Checked on a plotfile whose
isosurface is a sphere crossing the level boundary: closed, Euler characteristic 2, area within
0.2 %, centered (the box-by-box contour had 346 open edges); 2-D isolines close. ParaView's
AMR Contour / Dual Clip only take CTH non-overlapping AMR, so there is no ParaView reference.
Also fixed: Surface With Edges drew the edges from the color map (now the edge color, with a
Properties control), and the renderer could keep a stale mesh when a new mesh reused a freed
one's address (it now holds the mesh it uploaded).

Done in iteration 34: **hyper tree grids**. A new dataset type (`aurafx_data::HyperTreeGrid`:
root grid coordinates, branch factor, per tree a breadth-first refinement list with the cell
index of each vertex, mask, cell data for every vertex), read from VTK `.htg` (XML version 2,
bit arrays MSB first) and made by ParaView's Hyper Tree Grid source (string descriptor, VTK's
level-major numbering) and Random Hyper Tree Grid source (VTK's minimal-standard random
sequence per tree, creation-order numbering; no masking). Filters: To Unstructured Grid
(leaves with shared corners, `OriginalIds`), Depth Limiter, Generate Fields, Evaluate Coarse
(all operators); every other filter (and drawing, writing) works on the visible leaves.
Checked against ParaView 6.1.1: counts, leaf order (OriginalIds), depth sums, limiter,
fields, coarse operators, both sources. Also fixed: pixel cells were drawn and surfaced with
their corners out of order (bow-ties); they now become quads.

Done in iteration 35: **molecules**. The Molecule representation (ball and stick, van der
Waals, liquorice; atom size, bond radius, bond orders as parallel sticks; bond halves in their
atoms' colors) built as sphere/cylinder meshes with level of detail, the Elements color map
(Jmol colors, exact per element over [-0.5, 127.5]), element data from VTK's periodic table
(`aurafx_data::elements`), molecules opening that way by default (up to 100 000 atoms), the
CML reader (bond orders) and Compute Molecule Bonds (VTK's simple bond perceiver; existing
bonds kept, no duplicates). Checked against ParaView 6.1.1: the CML reader (atoms, bonds,
orders) and the bond sets for absolute and relative tolerances. Protocol 9 (display settings
and hyper tree grids changed the wire format). Cell grids were moved after molecules: the
.dg format needs VTK's DG cell framework (HGRAD/HCURL/HDIV bases on eight shapes).

Done in iteration 36: **cell grids**. A new dataset type (`aurafx_data::cellgrid::CellGrid`:
array groups, DG cell types, cell attributes with function space, basis, order and DOF
sharing) read from VTK `.dg` files (also the ones VTK's writer saves with empty role names).
The 52 basis and gradient functions (HGRAD C1/C2/I2/F2, HCURL I1, HDIV I1, constant on eight
shapes) are translated from VTK's generated headers by `scripts/gen_dg_basis.py`
(`aurafx_data::dg_basis`); edge fields are mapped by the inverse transposed Jacobian, face
fields by J/det J. The grid is drawn, listed and filtered as its linear cells with every field
evaluated at the corners and averaged where cells meet (cached in the grid). Filters: Cell
Grid To Unstructured Grid, Cell Grid Centers, Cell Grid Elevation; the Cell Grid source.
Checked against ParaView 6.1.1 on VTK's ten DG test files: every corner point and value
(HGRAD, HCURL, HDIV), the centers and their values, elevation, the source's fields.
Differences: quadratic cells become linear cells through the corners (ParaView keeps the extra
nodes but evaluates them at parametric (0,0,0)); flat cells' Jacobians are completed with the
unit normal (ParaView: NaN/undefined for face/edge fields); Centers on mixed types gives every
cell (ParaView one point). Protocol 10.

Done in iteration 37: **hyper tree grid writer, dual grid and masking**. The `.htg` writer (XML
version 2: per tree its breadth-first descriptor, vertices per depth, mask and cell data in
that order), Hyper Tree Grid To Dual Grid (VTK's Moore super cursor walk reproduced: neighbors
found level by level, never below a masked cell; corner ownership by level and neighborhood
index; boundary and mask point shifts, including VTK's diagonal signs), and the Random Hyper
Tree Grid source's Masked fraction (VTK's shuffled trees and children, masking cost per level).
Fixed: the Hyper Tree Grid source's mask string lost its digits, so Use mask always failed.
Checked against ParaView 6.1.1: our `.htg` files read in ParaView (cells, leaves, Depth sums)
and back in auraFX (same leaves and values); the dual grid cell by cell on eleven grids (1-D,
2-D, 3-D, files, masks, masked random grids); the masked descriptor source and the masked
random source (cells, visible leaves, depth sums, masked counts). auraFX's dual grid leaves out
VTK's unused points at the origin.

Done in iteration 38: **more GIS vector formats**. FlatGeobuf (own FlatBuffers reader: header
columns and CRS, features after the packed Hilbert R-tree index, every property type, all
simple geometry types), GML 2, 3 and 3.2 (feature types as layers, fields typed by the `.xsd`
beside the file or by their values, posList/pos/coordinates/coord, Curve and Surface patches;
coordinates swapped back for latitude-first EPSG URNs as GDAL does) and KMZ (own zip directory
reader, deflate via flate2, then the KML reader). Checked against ParaView 6.1.1 on FlatGeobuf
files written by GDAL (including 300 features with a three-level index); GML (3.2 and 2) and
KMZ, which ParaView's GDAL cannot read (no XML parser), against the same data as GeoJSON.

Done in iteration 39: **FLASH and Enzo AMR readers**. FLASH HDF5 plot/checkpoint files (FLASH 2
and FLASH 3/4 layouts: file format version from the dataset or "sim info", nxb/nyb/nzb from
"simulation parameters" or "integer scalars", blocks from gid, bounding box and refine level,
every variable in "unknown names" as doubles; recognized by content since the names have no
extension) and Enzo dumps (parameter file, `.hierarchy` parsed token by token as VTK does,
`GridNNNNNNNN` groups in the HDF5 grid files, the field names of the grid with the fewest cells,
stored types kept, reader option Convert to CGS from `#DataCGSConversionFactor`). Both become
overlapping AMR with covered cells hidden. Checked against ParaView 6.1.1 on files written by
scripts/gen_amr_files.py (FLASH 3 and FLASH 2 layouts of a 3-D grid; Enzo with three levels,
with and without CGS): every block's origin, spacing, size, array types and sums, covered
cells. ParaView 6.1.1 cannot read 2-D FLASH files ("Inconsistent types of vtkCartesianGrid",
segfault with several levels); the 2-D file is checked against its analytic values.

Done in iteration 40: **Chombo AMR reader**. Chombo HDF5 files (cell-centered, 1-D to 3-D):
`Chombo_global/SpaceDim`, component names, `level_N` groups with compound `boxes`, dx or
`vec_dx`, `prob_lo` and `aspect_ratio` (compound or array attributes, decoded from their raw
bytes), every box's components from `data:datatype=0` with the `outputGhost` ghost cells
dropped. Overlapping AMR with covered cells hidden. Checked against ParaView 6.1.1's VisIt
Chombo reader on two files from scripts/gen_amr_files.py (3-D; 2-D with ghosts, prob_lo,
vec_dx and three levels): every block's origin, spacing, dimensions, array sums and covered
cells, and the 2-D values against the functions they were written from. ParaView keeps level
0's ghost cells as ordinary cells (and drops the finer levels'); auraFX drops them everywhere.

Done in iteration 41: **Resample AMR**. AMR data sampled on a uniform grid (number of samples,
whole domain or a Min/Max box clipped to the data with VTK's sample adjustment), each point
taking the finest holding cell's values found the way vtkAMRResampleFilter's probe does (closed
box bounds, the previous point's box tried first, then finer levels), or cell centers as cell
data; unheld points hidden. Checked against ParaView 6.1.1 (demand-driven mode off): grid and
every value on the 3-D and 2-D Gaussian pulse, a region past the data, and a FLASH file.
ParaView's cell-center mode copies wrong cells' values; ours is checked geometrically.

Done in iteration 42: **FLASH and Enzo particles**. Reader option Data (Grid / Particles) on
files with particles. FLASH 3 particle tables (columns named by "particle names") and FLASH 2
compound particle datasets, every property as doubles; Enzo particle_position_x/y/z and the
other particle_* datasets of every grid (names from the smallest grid with particles), stored
types kept, all grids in one point set. Checked against VTK's particle readers as ParaView
6.1.1 runs them (driven directly; its proxies list no arrays): positions and properties.
ParaView repeats FLASH particles in every block and reads every FLASH 3 property from column 0.

Done in iteration 43: **Chombo node-centered data and particles**. Node-centered Chombo files
(data_centering 7: a value per node, ghosts dropped) give point data on the boxes; the
/particles group (position_x/y/z and one dataset per property, name order, doubles) is read
with the reader option Data = Particles. Checked against ParaView 6.1.1's VisIt Chombo reader:
the node-centered boxes and point sums, and every particle and property (its positions are
single precision).

Done in iteration 44: **Hyper Tree Grid Geometry and Feature Edges**. The neighbor lookup of the
dual grid became a shared cursor (Moore and von Neumann neighborhoods as VTK's super cursors
find them). Geometry: a line/quad per visible leaf in 1-D/2-D; in 3-D the faces of visible
leaves next to the grid's boundary or masked cells and the faces of masked cells next to a
coarser visible leaf, corners in VTK's edge-walk order, points shared within a cell. Feature
Edges: VTK's rules (2-D outline; 3-D corner and fold tests on the three neighbors of each
edge, masked-cell cases). Checked against ParaView 6.1.1 on eight grids: every cell in order,
its points and values, point counts. Material interfaces and ghost cells not done.

Done in iteration 45: **cell grid writer, Transform, Warp, Convert To Cell Grid**. The .dg
writer (VTK's JSON layout; ParaView opens the files); Cell Grid Transform (HGRAD/constant 3-D
values, shared or per cell, by ParaView's transform), Cell Grid Warp (shape + scale x a shared
HGRAD attribute of the same basis and order), Convert To Cell Grid (eight linear shapes, a
shared coordinates group, point data as shared HGRAD C1, cell data as constant attributes).
Checked against ParaView 6.1.1 through To Unstructured Grid (Transform on dgHexahedra with its
HCURL/HDIV fields, Warp on the source, Convert on a tetrahedralized wavelet); written files read
back exactly and open in ParaView with the same values. Also fixed: Transform rotated in the
wrong order (now ParaView's Rz Rx Ry; checked on a point), and serde_json now parses floats
exactly (float_roundtrip). Cell Grid Probe not done.

Known gaps worth doing next (P5): Tecplot SZL (no reference reader), more GDAL formats
(other rasters, MapInfo, file geodatabases), remaining filter catalog (CTH material AMR filters, which need non-overlapping AMR data; 4-D Chombo; HTG material interfaces, Cell Grid Probe
grid, molecules); rendering: wide lines (line_width is ignored by the renderer); P4 odds: TAA, comparative views, more widgets; P6: srun/MPI workers, ghost cells,
temporal filters on parallel servers; P7: Rust plugins, plugin panels, localization.

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

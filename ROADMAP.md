# auraFX Roadmap

auraFX is a Rust-native scientific visualization and analysis workstation. The goal is
a complete catalog of readers, filters and views (293 filters, 266 sources/readers, 46 writers, 101
view/representation types, 206 file-type groups; full inventory in
`docs/reference/paraview-6.1.1-catalog.json`) delivered through a UI that a first-time
user can drive without a manual, plus four things that set it apart:

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
- [~] CI: build + test on Linux/macOS/Windows, clippy, rustfmt, licence check (green on `main` since iteration 71; not yet run on the CAD branch, whose pull request starts it).
- [~] Packaging: AppImage, tar.gz + `install.sh`, `.dmg`, Windows installer + zip, Android APK, unsigned iOS IPA (done); `.deb`, MSIX, Flatpak (todo).
- [~] Crash reporting and structured logs (`tracing`), log panel in the UI. (Logs and log panel done; crash reporting todo.)
- [~] Settings system (TOML in platform config dir), settings UI with search.
- [x] Plugin ABI (dynamic Rust crates + Python plugins) and plugin manager UI. (Python plugins and the Plugins window; Rust plugins: iteration 86, a C table checked by ABI and data versions.)

### 0.2 Data model (`aurafx-data`)
Mirror of VTK's data object hierarchy, column-oriented, zero-copy where possible.
- [x] Typed arrays: i8..i64, u8..u64, f32, f64, with N components; string arrays.
- [x] Attribute sets: point data, cell data, field data; active scalars/vectors/normals/tcoords/tensors; global IDs; ghost arrays.
- [x] Datasets: `ImageData`, `RectilinearGrid`, `StructuredGrid`, `PolyData`, `UnstructuredGrid`, `Table`, `PointCloud`/particles.
- [~] Composite (MultiBlock done): `MultiBlock`, `PartitionedDataSet`, `PartitionedDataSetCollection`, overlapping and non-overlapping AMR, `HyperTreeGrid` (read and written as `.htg`; To Unstructured Grid, To Dual Grid, Depth Limiter, Generate Fields, Evaluate Coarse; descriptor and random sources with masks), `Molecule`, `CellGrid` (DG cells: HGRAD/HCURL/HDIV/constant fields on eight shapes; read from `.dg`, drawn at the corners; Centers, Elevation, source).
- [x] Cell types: all VTK linear types (vertex, line, polyline, tri, quad, polygon, tet, hex, wedge, pyramid, pentagonal/hexagonal prism, polyhedron).
- [x] Higher-order: Lagrange and Bezier cells of any order (curve, triangle, quad, tet, hex, wedge; per-cell `HigherOrderDegrees`, rational Bezier weights) and quadratic cells (complete, serendipity 8/20/15-node, quadratic-linear, cubic line), drawn curved and cut/integrated through their nodes like VTK (verified against the reference implementation). Pyramids (quadratic, Lagrange, Bezier) are still cut through their corners only.
- [~] Time: `TimeSeries` wrapper with lazy per-step loading, time-step metadata, time arrays.
- [~] Selections (IDs, rectangle/frustum, visible-only, threshold query done): IDs, global IDs, pedigree IDs, frustum, location, block, thresholds, query.
- [~] Spatial structures (cell locator for probe and stream tracer done): point locator, cell locator, BVH/octree, kd-tree (used by probe, interpolate, picking).
- [~] Out-of-core/mmap-backed arrays for files larger than RAM. (Iteration 88: files streamed in pieces through one server process, `afx.load(path, stream=N)`; VTK XML grids and `.vtu` read only each piece's bytes through a memory map; other readers todo.)
- [x] Unit and physical-quantity metadata on arrays. (Iteration 87: units on every array, read from NetCDF, set with Set Units or `data.point_data.set_units`, shown in the legend, Information panel and spreadsheet; a label, no conversion.)

### 0.3 Pipeline engine (`aurafx-pipeline`)
- [x] Demand-driven DAG: sources -> filters -> representations, with dirty tracking and memoized outputs.
- [x] Multi-input filters: extra inputs per node, scheduled and invalidated like the main input, kept in undo and state files, cycles rejected.
- [~] Per-node progress and cancellation; background execution off the UI thread.
- [x] Time-aware execution: temporal filters read their input at any time (the upstream pipeline is re-evaluated on the worker thread), add or remap time steps, keep state between runs (particles advance incrementally) and skip re-running when their result does not depend on time.
- [x] Streaming/pieces for huge datasets; ghost-cell generation. (Pieces on the parallel server, the Ghost Cells and Redistribute DataSet filters: iteration 83; ghost layers between server processes and pieces streamed through one process: iteration 88.)
- [x] Undo/redo of every pipeline and property change.
- [~] State files (`.afx` JSON/TOML) and `.pvsm` state import (best effort).
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
- [~] VTKHDF (`.vtkhdf`: image data, unstructured grids, poly data, partitioned collections with assemblies, time steps, gzip; iteration 18, verified against the reference reader); HDF5 generic browser todo.

### 1.2 Tier 2 (simulation ecosystem)
- [x] OpenFOAM (`.foam` case, ASCII/binary, decomposed and reconstructed, time directories, boundary patches with boundary values; verified against the reference reader).
- [~] EnSight Gold (`.case`, ASCII and C binary, all element types, blocks, time sets; iteration 18), Tecplot ASCII (iteration 18) and binary (iteration 20), PLOT3D (iteration 19), Xdmf2/3 (iteration 19), CGNS (HDF5, iteration 20; ADF, iteration 24), Fluent `.cas/.msh`, Nastran/BDF, ANSYS `.inp`, LS-DYNA `d3plot`, Silo, Xdmf2/3, PLOT3D, Nek5000, MFIX, ADIOS2 BP4/BP5, openPMD, Fides, Conduit/Catalyst.
- [~] NetCDF CF (iteration 19: classic and NetCDF-4, lon/lat on a sphere with bounds, time, packing, fill values); MPAS (iteration 25; vertical levels, multilayer and lon/lat map in iteration 27); CAM-SE and POP (iteration 28); GeoTIFF and Esri ASCII grid rasters (iteration 26), ERDAS Imagine .img (iteration 51); GIS vectors: Shapefile, GeoJSON, KML, GeoPackage (iteration 31), KMZ, FlatGeobuf, GML 2/3/3.2 (iteration 38), MapInfo .tab/.mif and Esri file geodatabases (iteration 50); other GDAL rasters, SEG-Y todo.
- [~] Molecular: PDB and Gaussian Cube (iteration 18), XYZ molecules (done); VASP POSCAR/CONTCAR/CHGCAR/LOCPOT/ELFCAR (iteration 19); CML (iteration 35); OUTCAR todo.
- [~] Cosmology: Gadget, GenericIO, Cosmo, Enzo/FLASH AMR (iteration 39), Chombo (iteration 40), AMReX plotfiles (iteration 32), FLASH and Enzo particles (iteration 42), Chombo particles and node-centered Chombo (iteration 43), 4-D Chombo (iteration 52).
- [x] Medical/imaging: DICOM, NRRD, MetaImage (`.mhd`), MRC, OME-TIFF (iteration 83); NIfTI (iteration 85).
- [x] Point clouds: LAS/LAZ, H5Part (iteration 83); PTS (iteration 85).
- [~] CAD: STEP, IGES, 3MF and `.sc` (read and written by the CAD environment, section 13, iteration 78); VRML 2.0, glTF, OFF (iteration 83); Alembic, USD, OpenVDB todo.
- [ ] Every remaining reader in the catalog, tracked per entry in a format matrix (`docs/reference/format-matrix.md`, not written yet).

### 1.3 Writers
- [~] VTK XML (all dataset types; parallel pieces `.pvtu/.pvtp/.pvti/.pvts/.pvtr` and `.pvd` collections, iteration 83), VTK legacy, VTKHDF (iteration 21; read by the reference reader since iteration 22).
- [~] Exodus II (done: element blocks, nodal/element variables, all time steps; verified with the reference reader), EnSight Gold and Xdmf 3 (iteration 22), CGNS/HDF5 (iteration 23), all verified with the reference reader; OpenVDB, GenericIO, Fides.
- [x] CSV, STL, PLY (done), OBJ, glTF, Houdini `.geo` (iteration 85).
- [~] Images: PNG, image stacks of volumes as PNG slices or one multi-page TIFF, MetaImage (iteration 83), screenshot with resolution override and transparent background (done, iteration 16), pictures of every view as laid out (iteration 83); JPEG/BMP/EXR todo.
- [x] Animation: PNG sequence, MP4 (ffmpeg), GIF (done); WebM (iteration 85).
- [~] Scene export: glTF (.glb), OBJ with vertex colors (iteration 16), X3D, VRML and SVG vector pictures (iteration 17); POV-Ray, PDF, WebGL/HTML interactive scene todo.
- [ ] Extracts and Catalyst-style in-situ scripts.

---

## 2. Filters (catalog)

Each group below is a menu category. Every filter of the catalog is listed; those marked with a star are the ones that ship in the first usable build.

### 2.1 Common (toolbar) ★
- [~] Calculator ★ (expression language with vector/scalar/tensor functions, per-point and per-cell)
- [~] Contour ★ (marching cubes/tetrahedra, multiple isovalues, compute normals/gradients, generate triangles)
- [~] Clip ★ (plane, box, sphere, cylinder, cone, annulus, scalar, crinkle, inside-out)
- [~] Slice ★ (plane, box, sphere, cylinder, multiple offsets, crinkle, merge points; Axis-Aligned Slice; Slice With Plane; Slice Along PolyLine; Slice Generic Dataset)
- [~] Threshold ★ (between/below/above, all-points/any-point, component modes)
- [~] Extract Subset ★ (VOI, sample rate, include boundary done; Extract Subset With Seed todo)
- [~] Glyph ★ (arrow, cone, sphere, cylinder, line, 2D glyph, custom source; scale/orient by array; masking strategies), Glyph With Custom Source, Tensor Glyph, Normal Glyphs
- [~] Stream Tracer ★ (RK2/RK4/RK45, point cloud and line seeds, surface streamlines, vorticity), Stream Tracer With Custom Source, Evenly Spaced Streamlines 2D
- [~] Warp By Vector ★, Warp By Scalar ★
- [~] Group Datasets, Extract Block (done), Extract Level
- [~] Plot Over Line ★, Plot Data, Plot Data Over Time, Plot Selection Over Time, Plot Global Variables Over Time, Plot On Intersection Curves, Plot On Sorted Lines, Plot Over Lines From Custom Source

### 2.2 Data analysis
- [~] Histogram ★, Histogram 2D, Descriptive Statistics, Contingency Statistics, Multicorrelative Statistics, Principal Component Analysis, K-Means, Compute Quartiles, Extract Statistical Model Tables, Generate Statistics, Integrate Variables ★, Min Max, Scatter Plot, Table FFT, FFT Of Selection Over Time, Temporal Statistics ★ (live filter), Compute Derivatives, Gradient ★, Gradient Magnitude, Curvature, Elevation, Mesh Quality, Cell Quality, Boundary Mesh Quality, Cell Size, Polyline Length, Count Cell Faces, Count Cell Vertices, Validate Cells, Overlapping Cells Detector, Vortex Cores, Yield Criteria, Tensor Principal Invariants, Surface Flow, Minkowski Filter, OpenTURNS: OT Density Map, OT Kernel Smoothing

### 2.3 Alphabetical / geometry and topology
- [~] Append Attributes, Append Datasets ★, Append Geometry (done), Append Reduce, Append Molecule, Append Selections
- [~] Cell Centers ★, Cell Data to Point Data ★, Point Data to Cell Data ★, Field Data to Attribute, Pass Arrays ★, Rename Arrays, Extract Component ★, Merge Vector Components, Extract Attributes, Coordinates, Global Point And Cell Ids, Point And Cell Ids, Process Ids, Block Ids, Overlapping AMR Level Ids, Random Attributes, Random Vectors
- [~] Clean ★, Clean to Grid, Clean Cells to Grid, Merge Blocks ★, Convert To Multi Block, Convert To Partitioned DataSet Collection, Convert AMR dataset to Multiblock, Convert To Point Cloud, Convert To Cell Grid, Convert Polyhedral Cells, Convert Into Molecule, Convert Selection, Image Data To AMR, Image Data To Uniform Grid, Image Data to Point Set, Rectilinear Data to Point Set, Table To Points ★, Table To Structured Grid, Transpose Table, Points Matching Transform
- [~] Connectivity ★, Rectilinear Grid Connectivity, Extract Surface ★, Extract Edges, Feature Edges ★, Feature Edges Region Ids, Extract Region Surface, Extract Enclosed Points, Extract Cells By Region, Extract Cells By Type, Extract Cells Along Line, Extract Cells Along Lines (custom source), Extract Location, Extract Ghost Cells, Ghost Cells, Remove Ghost Information, Extract Selection ★, Extract Time Steps, Extract Particles Over Time, Extract AMR Blocks, Extract CTH Parts, Geometry Filter, Outline ★, Outline Corners, Outline Curvilinear DataSet, Outline Generic DataSet, Bounding Ruler
- [~] Transform ★, Axis-Aligned Transform, Reflect, Axis-Aligned Reflect, Shrink, Explode DataSet, Crop, Image Shrink, Align Image Origins, Stitch Image Data With Ghosts
- [~] Triangulate ★, Tetrahedralize ★, Tessellate (done: uniform levels instead of chord-error refinement), Tessellate Generic Dataset, Delaunay 2D ★, Delaunay 3D ★, Decimate ★, Quadric Decimation, Quadric Clustering, Decimate Polyline, Subdivide, Loop Subdivision, Smooth ★, Triangle Strips, Reverse Sense, Surface Normals ★, Surface Tangents, Surface Vectors, Deflect Normals, PCA Normal Estimation, Ribbon, Tube ★, Linear Extrusion, Linear Cell Extrusion, Rotational Extrusion, Volume Of Revolution, Clip Closed Surface, Iso Volume ★, Mask Points, Distribute Points, Generate Quadrature Points, Generate Quadrature Scheme Dictionary, Interpolate to Quadrature Points, Finite Element Field Distributor, Compute Sides
- [x] Texture Map to Plane/Cylinder/Sphere, Perlin Noise, Spatio Temporal Harmonics (filter and source), plus the display Texture (an image on a surface)
- [~] Resample To Image ★, Resample With Dataset ★ (done, verified point by point against the reference implementation), Resample To Line, Resample AMR (iteration 41), Adaptive Resample To Image, Gaussian Resampling, Probe Location ★, Point Interpolator (Gaussian/Shepard/Voronoi/Linear kernels), Point Line/Plane/Volume/Dataset Interpolator, SPH Interpolator (+ Line/Plane/Volume/Dataset), Point Set To Octree Image, Octree Image to Point Set, Median, Equalizer Filter
- [~] Particle Tracer, Particle Path, Streak Line (done; exact on an analytic flow), Legacy Particle Path, Legacy Streak Line, In Situ Particle Path, Temporal Particles To Pathlines
- [~] Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator (done), Temporal Snap-to-TimeStep, Temporal Cache, Temporal Smoothing, Merge Time, Group Time Steps, Generate Time Steps, Force Time, Synchronize Time, Critical Time, Date To Numeric, Time Step Progress Bar, Animate Modes
- [~] Hyper Tree Grid (iteration 34: type, .htg reader, both sources, To Unstructured Grid, Depth Limiter, Generate Fields, Evaluate Coarse, Cell Centers; iteration 37: .htg writer, To Dual Grid, masked random source; iteration 44: Geometry, Feature Edges; iteration 49: material interfaces in Geometry): Axis Reflection, Cell Centers, Depth Limiter, Evaluate Coarse, Feature Edges, Generate Fields, Geometry Filter, Ghost Cells Generator, Redistribute, To Dual Grid, To Unstructured Grid, Visible Leaves Size
- [~] AMR (foundations, readers, Gaussian pulse, level ids in iteration 32; crack-free Contour and Dual Grid in 33): Connectivity, Contour, Dual Clip, Fragment Integration, Fragments Filter, Material Interface Filter, Intersect Fragments, Youngs Material Interface
- [~] Cell Grid (iteration 36: Centers, Elevation, To Unstructured Grid, source; iteration 45: Transform, Warp, Convert To Cell Grid, .dg writer; iteration 46: Probe; iteration 47: Compute Sides): Centers, Compute Sides, Elevation, Probe, To Unstructured Grid, Transform, Warp
- [~] Molecules (iteration 35: Molecule representation, CML reader, Compute Molecule Bonds; molecules are polydata, so Molecule To Lines / Convert Into Molecule are not needed): Compute Molecule Bonds, Molecule To Lines, EAM Project To Sphere
- [~] Cosmology: ANL Halo Finder, ANL Subhalo Finder, LANL FOF/SOD Halo Finder (Halo Finder, friends of friends with SOD radius and mass: iteration 83; subhalos todo)
- [~] Parallel/distributed (Redistribute DataSet and Ghost Cells: iteration 83; ghost exchange between server processes: iteration 88; the rest todo): D3, Redistribute DataSet, Partition Balancer, Reduction Filter, Aggregate Dataset, Client Server Move Data, MPI Move Data, Pass Through, Force Static Mesh, Voronoi Merge Connected
- [~] Annotation (text, time label and ruler done): Annotate Attribute Data, Annotate Global Data, Annotate Selection, Annotate Time Filter, Environment Annotation, Python Annotation, Programmable Annotation, Text, 3D Text, Logo, Ruler, Protractor
- [~] Programmable: Programmable Filter ★ (Python), Programmable Source, Python Calculator ★ (done), Live Programmable Source, Color By Array, OME TIFF Channel Calculator, Threshold Table, Field Arrays From File

### 2.4 Sources
- [~] Geometric: Sphere ★, Cone ★, Cylinder ★, Box ★, Plane ★, Line ★, Disk, Ellipse, Arrow, Axes, Superquadric, Point Source, PolyLine Source, PolyPoint Source, Spline Source, Outline Source
- [~] Synthetic data: Wavelet ★, Mandelbrot, Fast Uniform Grid, Hierarchical Fractal, Hyper Tree Grid (+Random), AMR Gaussian Pulse, Data Object Generator, Unstructured Cell Types (done), Partitioned DataSet Collection Source, Cell Grid Source, Spatio Temporal Harmonics Uniform Grid, Time Source, Trivial Producer
- [ ] Selection sources (ID, global ID, pedigree ID, location, frustum, threshold, value, block, query, composite/hierarchical ID)

### 2.5 Filter infrastructure
- [~] Every filter has: typed properties, defaults, validation, documentation, search keywords, Python binding, undo entry, and progress reporting.
- [~] GPU implementations (wgpu compute) for: contour, clip, slice, threshold, gradient, resample, glyph, stream tracer, particle advection, connectivity, histogram. (Iteration 88: contour and slice of grids, the GPU finding the cells to cut and the CPU cutting them, so results are unchanged; a GPU histogram was slower than the CPU here and was left out.)
- [x] Custom filter authoring: Python (`@afx.filter`, done) and Rust plugins (iteration 86: the `aurafx-plugin` crate, shared libraries in the plugin folder).

---

## 3. Rendering and views

### 3.1 Render view (`aurafx-render`, wgpu)
- [x] Camera: perspective/orthographic, trackball/terrain/joystick styles, reset/fit, standard views (+X,-X,...), isometric, roll, zoom to box, zoom to data, camera undo/redo, saved camera links.
- [x] Surface rendering with flat/Gouraud/PBR shading; backface handling.
- [x] Scalar coloring with color maps (ColorBrewer, viridis family, the classic preset gallery, imported `.json/.xml`), range modes (auto, custom, time-wide), log scale, discrete, NaN/above/below colors, opacity transfer functions, per-component or magnitude, categorical.
- [~] Representations: Surface, Surface With Edges, Wireframe, Points, Outline, Feature Edges, Point Gaussian (splats: sphere, gaussian, custom shader), 3D Glyphs, Volume, Slice (image), Slice (unstructured), Multi-slice, Orthographic slice, Molecule (ball/stick/VDW), Hyper Tree Grid, Cell Grid, Polar axes, Data labels, Selection highlight, Texture. (3D Glyphs: iteration 88, instanced.)
- [~] Volume rendering: GPU ray casting, smart/fixed-point, 2D transfer functions, shade, gradient opacity, multi-component, blend modes (composite, MIP, MinIP, additive, isosurface), cropping, clipping planes, unstructured-grid projected tetra / ray casting.
- [x] Lighting (iterations 14–15): light kit, custom lights (camera-relative or fixed, colored), shadows, screen-space ambient occlusion, Eye-Dome Lighting, ACES/Reinhard tone mapping and exposure, MSAA + FXAA, order-independent transparency. Light widgets done in iteration 16. Not done: TAA. Original list: key/fill/head/back light kit, custom lights with widgets, shadows, ambient occlusion (SSAO), Eye-Dome Lighting, tone mapping, FXAA/MSAA/TAA, depth peeling for correct transparency.
- [x] Surface LIC and ray tracing (Rust path tracer, iteration 15); environment maps as light and background, materials (metallic, roughness, specular) in Properties, GPU path tracing (iteration 16).
- [~] Axes grid (data axes grid with ticks and labels, orientation axes, scalar bars and text done; array units in the scalar bar: iteration 87; custom bounds and axis units todo): Axes grid (data, custom bounds, labels, units), orientation axes, center-of-rotation marker, scalar bars (position, titles, number format, custom labels), legends, logos, text, interactive 2D annotations.
- [~] Widgets (implicit plane, box, sphere, line, point, light and ruler done): implicit plane, box, sphere, cylinder, cone, annulus, frustum, line, polyline, spline, handle/point, distance, protractor, ruler, slider, light, coordinate frame, four-lines, contour, display-sized plane, text, logo, equalizer, progress bar.
- [~] Selection: by click/rubber band/polygon on points/cells/blocks, through-frustum, by query (`field > value`, nearest, etc.), grow/shrink, invert, hover tooltips and interactive labels, "Find Data" panel.
- [~] Picking tools: point probe, coordinate readout, interactive cell/point info.
- [ ] Stereo (anaglyph, crystal eyes, interlaced, side-by-side), VR (OpenXR) later.
- [~] Huge data: LOD, frustum/occlusion culling, instanced glyphs, GPU-resident meshes, mesh streaming, remote render with image streaming. (GPU-resident meshes and server pictures done; iteration 88: frustum culling by runs of triangles, a coarse copy drawn while the camera moves, instanced 3D Glyphs; occlusion culling and mesh streaming todo.)

### 3.2 Other views
- [~] Line chart ★ (multiple series, log axes, styles, legends, markers, selection link), Bar chart, Point chart, XY bar chart, Histogram view, Box chart / Quartile chart, Plot matrix (scatter matrix), Parallel coordinates, Image chart, Comparative views (render/line chart), Slice view, Orthographic slice view, Spreadsheet view ★ (point/cell/field/row data, sort, filter, column visibility, precision, export CSV), Python view (matplotlib embedded), Spreadsheet of selection.
- [x] Several chart views at once (iteration 83), each plotting its own pipeline object (or following the selection), saved with the state and opened from menus, the Pipeline and Python; pictures and animations of every open view (3D views and charts) as laid out.
- [~] Layouts (split views, linked cameras, match view direction done): split horizontally/vertically, tabs, link cameras, link views, link selections, maximize, save/restore layouts, multiple windows/monitors.

### 3.3 Animation and time
- [~] Time toolbar (play, step, loop, time value entry, snap to steps, real-time, sequence modes), time inspector.
- [~] Animation view (camera keyframes, numeric filter and display property tracks, linear/step/ease/exponential interpolation per key, Python cue, time scrubbing and preview done; sinusoid and comparative cues todo): keyframes for any property (ramp, linear, exponential, sinusoid, boolean), camera paths (orbit, path, interpolate), Python cue, comparative cues.
- [~] Save animation at any resolution/format with frame-rate control; stride; preview.

---

## 4. User experience

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
- [x] Keyboard-first: fully rebindable shortcuts, vim-style view navigation optional. (Iteration 85: Settings › Keyboard, `afx.set_shortcut`.)
- [x] Themes: dark/light/high-contrast, scalable UI (HiDPI), font size. (High contrast: iteration 85.)
- [~] Session recovery after crash, autosave of state. (Last session restored on start.)
- [x] Accessibility: screen-reader labels, keyboard navigation, color-blind color maps by default. (Iteration 85: AccessKit, a high-contrast theme; Viridis was already the default.)
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
- [x] Jupyter kernel exposure (drive auraFX from a notebook) and notebook panel inside the app. (Iteration 86: `aurafx --install-jupyter-kernel`, the kernel drives the window; the Notebook panel opens and saves `.ipynb`; `afx.snapshot`, `afx.display`.)
- [x] Custom Python plugins (reader, filter, source done; panels: iteration 86, `@afx.panel`) with hot-reload.

---

## 6. AI assistant (`aurafx-ai`)

- [x] Provider abstraction: Anthropic API, OpenAI-compatible API, Ollama (local and cloud models), with streaming.
- [~] Subscription/OAuth sign-in for hosted providers; API keys stored in the OS keyring.
- [x] Chat panel with Markdown rendering, code blocks with "Run in Python console", conversation history per project.
- [~] Context awareness: the model sees pipeline state, selected dataset summaries (arrays, ranges, bounds, time steps), camera, recent errors and logs, without uploading raw data unless asked.
- [~] Tools the model can call (agent mode acts through generated Python): load file, add filter, set property, color by, take screenshot (and view it), run Python, read a chart's data, search docs, run a simulation job.
- [~] Modes: "Explain", "Do it for me" (agentic with confirmation per step), "Write a script", "Debug this error".
- [x] Natural-language filter creation (iteration 82: Filters › Describe a filter…) ("show me regions where von Mises stress > 300 MPa").
- [x] Anomaly and feature detection helpers (outliers in fields, mesh quality problems, time-series change points). (Iteration 85: Find Outliers, Find Change Points; mesh problems by Mesh Quality and Validate Cells.)
- [x] Prompt templates and user-defined skills; privacy controls (iteration 82: local-only mode and path redaction; iteration 85: saved prompts and skills).
- [x] Vision: send rendered frames to multimodal models for interpretation (iteration 82: the camera box next to Send, `afx.ask(..., view=True)`; Ollama, Anthropic and OpenAI formats).

---

## 7. Simulation runner (`aurafx-jobs`)

- [~] Job model: command, working dir, environment, inputs, outputs, resources (CPU, GPU count/vendor, memory), dependencies between jobs, templates per solver.
- [~] Local executor with live stdout/stderr streaming, log search, kill/pause, resource monitoring (CPU, GPU via NVML/ROCm-SMI).
- [~] Templates: LAMMPS, MOOSE, OpenFOAM, Abaqus/CalculiX, GROMACS, custom shell/Python.
- [~] Python pipeline runs: define a pipeline in Python (pre -> run -> post), run as a job, auto-load outputs when done.
- [x] Parameter sweeps and ensembles with a results table and comparative views.
- [~] Remote executor over SSH: connect (key, agent, password, jump hosts, 2FA prompts), remote file browser, sync inputs/outputs (rsync-style), run under nohup/tmux, Slurm/PBS/LSF submission and queue monitoring, reconnect to running jobs. (Slurm done, untested on a real cluster; PBS and LSF todo.)
- [x] Remote rendering/data server mode (done in iterations 10 to 13; iteration 82 made it one guided window, Remote › Open data on another machine…): run `aurafx-server` on the GPU box, stream geometry or images to the local client.
- [x] Watch folders: auto-refresh when the solver writes a new step.

---

## 8. GPU compute (`aurafx-gpu`)

- [x] wgpu device management: adapter choice (AMD, NVIDIA, Intel, software), features, limits, memory stats in UI.
- [~] Compute kernels in WGSL for filters listed in 2.5, array math (calculator JIT to WGSL), reductions, histograms, sorting. (Iteration 88: the `aurafx-gpu` crate, cell-candidate kernels for values and planes.)
- [ ] Optional vendor backends behind features: `cuda` (cudarc) and `rocm` (HIP) for kernels where WGSL is too limited (fp64, large shared memory), and for interop with PyTorch tensors.
- [x] GPU memory budgeting, fallbacks to CPU (rayon), benchmark panel. (Iteration 88: slabs sized to the device's buffer limits, CPU fallback on any GPU error or without a GPU, Settings › Benchmark and `afx.gpu_benchmark`.)

---

## 9. Distributed and remote

- [~] Client/server split (data server and server-side rendering done: `aurafx-server` over SSH stdio, server-side pipeline with caching, surface/summary delivery, GPU-drawn color+depth images composited by depth; MPI and Slurm launchers: iteration 86): `aurafx-server` (data + render) over TCP/SSH tunnel with the same protocol as local.
- [~] Multi-process data parallel (all-core shared-memory execution done; multi-process parallel servers done: workers per piece, piece-wise reading of partitioned files, sort-last compositing; MPI and Slurm launchers: iteration 86; ghost layers between the server's processes: iteration 88, so Gradient, Cell Data to Point Data, Surface Normals, Curvature and Compute Derivatives run on every piece) (MPI optional; Rust-native work distribution first), ghost cells, D3 redistribution, compositing (sort-last), tile displays.
- [ ] Collaboration: shared sessions with multiple clients.
- [ ] In-situ: Catalyst/Conduit endpoint, live visualization of running jobs.

---

## 10. Quality, docs, community

- [~] Golden-image render tests, filter regression tests against VTK outputs, reader conformance suite with sample files for every format.
- [x] Benchmarks (load time, filter time, frame rate) published in docs. (Iteration 85: the guide's Benchmarks page, written by an ignored release-mode test.)
- [~] User guide (done: Markdown, HTML book and PDF, a picture per feature), tutorials and examples (done, iteration 71); filter reference auto-generated from property metadata (iteration 85, kept current by a test), Python API docs todo.
- [~] Contributing guide (done), code of conduct, issue templates, plugin cookbook (done: `docs/plugins.md`).

---

## 11. Grain workbench (microstructure analysis and building)

EBSD and synthetic microstructures, grain segmentation and statistics, texture, boundaries
and mechanics as filters, sources, readers, writers and charts, for FCC, BCC and HCP crystals. Design and stage details: `docs/design/grains-and-meshing.md`.

- [x] G1 `aurafx-crystal` (orientation conversions, the 11 Laue groups, disorientation, fundamental zones, IPF/Rodrigues/Euler colours); readers for `.ang`, `.ctf`, `.h5ebsd`, EDAX/Bruker `.h5`, `.dream3d`; Orientation Colors, IPF Color Key, Convert Orientations, Rotate Orientations, Crop, Change Resolution, Align Sections.
  - [x] G1a: `aurafx-crystal`, `.ang` (square grids) and `.ctf` readers, Orientation Colors, IPF Color Key, Convert Orientations, Rotate Orientations; 8-bit RGB arrays drawn as their own colors (iteration 53).
  - [x] G1b: `.h5ebsd`, EDAX/Bruker `.h5`, `.dream3d` readers; Crop, Change Resolution, Align Sections (iteration 54).
- [x] G2 Features: Segment Features (misorientation, scalar, C-axis), Find Sizes/Neighbors/Shapes/Centroids/Boundary Cells/Surface Features/Average Orientations/Misorientations/Kernel Average Misorientation/Reference Misorientations/Euclidean Distance Map, Min Size, Fill Bad Data, Merge Twins, Find Twin Boundaries; Features and Ensembles tables.
- [x] G3 Texture: pole figures, inverse pole figures, ODF sections and texture index, MDF, texture fractions, chord lengths (charts, pictures, CSV).
  - [x] G3a: pole figures (stereographic, equal area), inverse pole figures, ODF sections, texture index, CSV of multi-panel output (iteration 59).
  - [x] G3b: misorientation distribution, texture component fractions, chord lengths (iteration 60).
- [x] G4 Synthetic builder: ensemble statistics (StatsGenerator's size/shape/neighbour/ODF/MDF/axis-ODF), Initialize Volume, Pack Primary Phases, Insert Precipitates, Matrix Phase, Match Crystallography, orientation noise/jumble; Voronoi/Laguerre tessellation and columnar AM grains as sources; Generate Ensemble Statistics from data.
  - [x] G4a: Voronoi/Laguerre grains, columnar AM grains (the toolkit's generator, voxel for voxel), Add Orientation Noise, Jumble Orientations, Generate Ensemble Statistics (iteration 63).
  - [x] G4b: Pack Grains (goal statistics, four shapes, matrix phase, calibration), Pack Like, Insert Precipitates, Match Crystallography (iteration 64).
- [x] G5 Surface meshing and GBCD: Quick Surface Mesh, Laplacian/Taubin smoothing with node types, triangle-geometry filters, STL per feature, Find GBCD, face IPF and misorientation colouring.
- [x] G6 Mechanics and AM: Schmid factors for FCC/BCC/HCP slip systems, Taylor factor, directional modulus, yield estimate, SROD by build height, Nye and KAM GND, layer plots; materials table.
- [x] G7 Files and runs: `.dream3d` writer with pipeline JSON, `.tex`, SPPARKS sites, `.dream3d` pipeline JSON import; MOOSE polycrystal, SPPARKS and PINNACLE job templates with the pipeline's files filled in; results reopened for analysis.

## 12. Geometry and mesh generator

Implicit geometry, structured hex meshing (hexaform's methods) and an overlay-grid mesher for
arbitrary and voxel geometry, with a command language, quality tables and sets. Design: `docs/design/grains-and-meshing.md`.

- [x] M1 Geometry kernel: implicit primitives, transforms, CSG, 2-D outlines extruded/revolved/swept, STL and existing surfaces as signed distance fields, named faces; the Geometry source and the `afx.mesh` command language (journals through the trace).
- [x] M2 Structured hex: box, mapped blocks, cube with packed inclusions (cubed-sphere O-grid macro-blocks, transfinite cores, graded shells, weld), plate with hole, dog-bone, 2-D sweep, Elevate to HEX20/27, blocks/side sets/node sets, periodic pairs; Mesh Quality with the full metric set and acceptable ranges; Exodus, VTK, Abaqus `.inp`, Gmsh `.msh` writers.
  - [x] M2a: hex mesh model (blocks, side sets, node sets, element variables), box and mapped blocks with grading, the inclusion mesher (grid packing, cubed-sphere O-grid blocks, transfinite cores, graded shells, exact weld; spheres, ellipsoids, superquadrics, harmonic grains), HEX20/HEX27 with surface projection, periodic pairs by coordinates, Extract Set, Mesh Quality with the full hexahedron metric set and a summary table, Exodus sets and HEX20/27, Abaqus `.inp`, Gmsh `.msh` (iteration 57).
  - [x] M2b: plate with hole, dog-bone, 2-D quad sweep, collective packing, mesh commands (`mesh body`, `block`, `sideset`, `nodeset`) in the geometry language for bricks and cylinders, the benchmark's meshes as references (iteration 58).
- [x] M3 Overlay-grid mesher: volume fractions from geometry, STL or voxel images, interface reconstruction, projection, pillowing, tiered smoothing, defeaturing, void meshing, generated side sets, periodic RVE sets, stair-step fallback.
  - [x] M3a: overlay grid from geometry bodies, closed surfaces and labelled images, defeaturing and pinch removal, pillowing, projection along stair-step normals, guarded tiered smoothing and untangling, void meshing, domain/surface/interface side sets, periodic pairs, stair-step mode, `scheme overlay` in the geometry commands, Overlay Mesh filter (iteration 61).
  - [x] M3b: sharp edges and corners kept by the overlay mesher (patches between creases for surfaces), saddle-free clean-up for fine microstructures, parallel smoothing (iteration 62).
- [x] M4 Refinement and tets: conformal and local refinement, smoothing schemes, Delaunay tri/tet meshing with sizing, THex/HTet, boundary layers.
  - [x] M4a: uniform refinement of hexahedra and tetrahedra, HTet/THex conversions, mesh smoothing (Laplacian, smart Laplacian, condition number), boundary layers, tetrahedron quality metrics (iteration 66).
  - [x] M4b: tetrahedral meshing of implicit and STL geometry (isosurface stuffing), local refinement of tetrahedra, Tet mesh output and filters (iteration 67).
  - [ ] Later: local refinement of hexahedra with transition templates, Winslow smoothing, graded sizing by curvature, sharp edges in tetrahedral meshes, conforming tetrahedra between touching bodies.
- [x] M5 Grains on meshes: Assign Grains (Voronoi, Laguerre, nearest voxel, from a volume), per-grain blocks and boundary side sets, Mesh Grains (M3 on FeatureIds); the worked geometry → mesh → grains → solver → statistics example.

---

## 13. CAD modeling environment (P10)

A parametric CAD modeler inside the one window, beside the command-language modeler of
section 12: constrained sketches, a feature timeline that rebuilds on a parameter change,
fillets, shells, holes, threads, patterns, assemblies, STEP/IGES/3MF in and out, and a commit
step that hands the geometry back to the pipeline after checking everything downstream.
Design, with the data model, the two kernels, the pull-in of SolidCraft, the downstream check
and a file-by-file port plan from EZ-YX-craft: `docs/design/cad-environment.md`. Branch
`feat/CAD-modeling-environment`. The crate is `aurafx-cad`; the environment is `aurafx-app/src/cad/`.

How to work a stage (for the agent picking one up): read the design's section 6 row, port the
named EZ-YX-craft file where one is named (`/mnt/extended/projects/ez-yx-craft`), keep its
tests, write the guide entry in `docs/userguide/src/cad.md` with a `scripts/userguide/shots/cad_*.py`
screenshot, add the Python calls to the AI help, tick the box here, add a Status paragraph.

- [x] C0 Skeleton: crate `aurafx-cad` (kernel trait, `Evaluated` contract, errors, `Id`), the expression evaluator ported with its tests, the design document, this section (iteration 72).
- [x] C1 Document (iteration 73): `document.rs` (Feature, Shape, Modifier, Parameter, Binding, Configuration, Document, resolution of parameters and configurations), `sketch.rs` + `path.rs` (entities, constraints, dimensions, the Levenberg-Marquardt solver with its DOF report), `templates.rs`, `analysis.rs` (mass properties, interference, sections), `assembly.rs`; `.afxcad` JSON files with a format version. Tests: EZ-YX-craft's, plus every template through a JSON round trip.
- [x] C2 Implicit kernel and the node (iteration 74): `kernel/implicit.rs` on `aurafx-mesh` (every Shape and Modifier as a signed distance; surface with BodyId/FaceId/FaceNames as the Geometry source writes them); the `cad_model` source whose parameters are the document's parameters; `NodeKind::Cad`; the document kept in `.afx` state; `afx.cad_model()`, `node.cad.*`, `node.set(param=...)`. Tests: volumes of the primitives and of extrude/revolve within 1 % of analytic values; an overlay mesh of a template passes Mesh Quality.
- [x] C3 Environment shell (iteration 75): `Session.cad_editing` pauses the scheduler and marks other views stale; the CAD menu; the ribbon (Sketch, Features, Modify, Assemble, Inspect, Files) in place of the toolbar; FeatureManager in place of the Pipeline panel; PropertyManager for every feature, modifier and parameter; the timeline bar (rollback, suppress, reorder); Apply, Apply-and-keep-editing, Discard, Save as; `node.edit_cad()`; one undo group per commit. Ported from `ezyx-app/src/solid/{mod,props}.rs`. Test: an app-driven script enters, adds a box and a hole, applies, and the node's output has two faces more.
- [x] C5 Downstream check (iteration 76; before C4 on purpose): `aurafx-pipeline/src/cad_check.rs` and `aurafx-cad/src/diff.rs`; references collected from downstream parameters (face and body names and ids, set names, positions inside the old bounds, geometry command text, Python code as "may reference"); the diff by stable ids, names, then centroid/normal/area; conflict kinds Removed, Renamed, Moved, Ambiguous, Count; default fixers as `set_param_json`; the dialog (default fix, assistant's fix, leave; Apply fixes and re-run, Apply without fixing, Back to CAD, Discard); the "Re-run N downstream filters?" confirmation; `afx.cad_check(node)`; the assistant prompt and its JSON answer parser. Tests: plate with a hole → Extract Set of the hole; rename the hole feature (Renamed, fixed), delete it (Removed, offered), move a Refine Region box (Moved); a canned assistant answer applied.
- [x] C4 Sketching (iteration 77): the sketch canvas in the 3D view (plane-aligned camera, grid, snapping), entities, constraints, dimensions typed as expressions, the solver's status colours and DOF count, sketches on faces. Ported from `ezyx-app/src/solid/sketch_ui.rs`. Tests: the solver's (fully constrained rectangle, over-constraint rejected, tangent arc), a canvas script.
- [x] C6 B-rep kernel and CAD files, shipped in the repository (iterations 78 and 81; the CAD readers are registered in aurafx-io's reader table, not compiled into the crate, because aurafx-cad depends on aurafx-mesh, which depends on aurafx-io; four of the six templates build exactly, the other two fall back by design; see Status): `vendor/solidcraft` and `vendor/solidcore` carried as plain files at a recorded revision (first added by `git subtree`; after iteration 82 the history was rewritten so that one commit adds only the carried directories, and `scripts/cad/upstream-pull.sh` takes upstream work as a three-way patch; see the Status note on vendored code) and pruned to the crates we build (engine, doc, kernel, smlib, io, step, geom, profile, sketch, derive, testkit; not its UI, apps, browser build or docs), their licences and NOTICE kept, `vendor/README.md` (tag, date, pruning, auraFX patches marked `// auraFX:`), `docs/reference/third-party.md`, `scripts/cad/upstream-pull.sh` and `upstream-diff.sh` (subtree pull at a new tag; upstream commits and new command ids since the recorded tag); workspace `rust-version` 1.95; the B-rep kernel always built (no cargo feature, no external bridge); `kernel/brep.rs` mapping every Shape and Modifier to SolidCraft commands run in process; STEP AP242, IGES, 3MF and `.sc` read and written; STEP/IGES import as a base feature; the faceted STEP writer for implicit-only bodies; readers registered in `aurafx-io`'s reader table (`aurafx_io::register_reader`); `testdata/cad/` with small STEP/IGES/`.sc` files and their provenance. Tests: the templates give the same volumes on both kernels within 1 %; a STEP and an IGES file written here measure the same through the vendored engine and an independent reader (done: `scripts/cad/step_check.py`); the committed tree builds offline from a clean clone (`cargo build --offline --locked -p aurafx-cad --release`, done; the installers themselves were not run).
- [x] C7 Edge operations and assemblies (iterations 78 and 81; see Status): picking faces and edges on the B-rep tessellation (a click in the 3D view, or the list); fillet and chamfer on picked edges, draft, shell with picked open faces, patterns on faces, mirror about faces, combine; components with placements, interference, mass properties, a measure tool. Tests: fillet volume against SolidCraft's measure; interference of two boxes analytic.
- [x] C8 Meshing from CAD (iterations 79 and 81; Overlay Mesh and Tet Mesh both keep the faces' names): `mesh body` on CAD bodies, face names to side sets, `import 'part.step'` and `import 'part.afxcad'` in journals, STEP → mesh → grains → solver case end to end as a test and in the guide's worked example.
- [x] C9 Polish (iterations 79 and 81; see Status): templates gallery, shortcuts, PropertyManager help, the assistant modelling in the environment through `afx.cad`, the project page's CAD video, the README's credits for SolidCraft and solidcore.

## 14. Gaps against the reference programs (P11)

The user asked for this comparison (2026-11-07): section 2 lists every filter of the
reference visualization program's catalog (`docs/reference/paraview-6.1.1-catalog.json`), and
sections 11 and 12 were designed against the other two programs' feature sets; this section
picks the items that matter for auraFX's users and says, for each, where it goes and how it
is checked, so an agent can take one without further research. Describe each feature in
auraFX's own words when it lands; the program names below only say where the gap was
noticed. Priority: **P1** next, **P2** after, **P3** when asked.

### 14.1 Visualization and analysis (179 catalog filters without a counterpart by name; the ones that count)

| Pri | Item | Where | Check |
|---|---|---|---|
| P1 done (iterations 80, 82) | **Delaunay 2D and 3D** (points to triangles/tetrahedra, with alpha), **Decimate** (quadric), **Loop/Linear Subdivide**, **Curvature** (Gaussian/mean), **Linear and Rotational Extrusion**, **Ribbon**, **Reverse Sense** | `aurafx-filters/src/surface.rs`, `simplex.rs` | areas/volumes of a sphere sample against analytic; Decimate keeps volume within 2 % at 50 % |
| P1 done (iteration 82) | **Iso Volume** (threshold on point data with interpolated cuts), **Extract Cells By Region** (implicit box/sphere/plane), **Extract Cells By Type**, **Clip Closed Surface** (capped clip) | `clip.rs`, `threshold.rs` | capped clip of a cube is watertight (Integrate Variables volume equals the half cube) |
| P1 done (iteration 83) | **Point interpolation** (Gaussian/Shepard/linear kernels, SPH) from points to a grid or dataset, **Resample To Line**, **Plot On Sorted Lines**, **Plot On Intersection Curves** | `interp.rs`, `probe.rs` | interpolated field of a known polynomial within tolerance |
| P1 done (iteration 82) | **Table To Points / To Structured Grid**, **Transpose Table**, **Table FFT**, **Merge Vector Components**, **Point and Cell Ids**, **Generate Global Ids**, **Coordinates as array** | `attributes.rs` | round trips |
| P1 done (iteration 82; Surface Flow and Surface Vectors too; Vortex Cores as the Q and lambda2 criteria of Compute Derivatives) | **Tensor Glyph**, **Tensor Principal Invariants**, **Yield Criteria** (von Mises, Tresca, principal), **Compute Derivatives** (strain, vorticity from vectors), **Surface Vectors/Tangents**, **Surface Flow**, **Vortex Cores** | `analysis.rs`, `glyph.rs` | invariants against NumPy on a random tensor field; vorticity of a rigid rotation |
| P1 done (iteration 83; the tracer's seeds-over-time option was already there: Release again every N steps) | **Evenly Spaced Streamlines 2D**, **Particle Path / Streak Line with injection over time** (expose the tracer's seeds-over-time option), **Temporal Cache / Force Time / Extract Time Steps / Generate Time Steps / Temporal Snap** | `streamlines.rs`, `temporal.rs` | known analytic flows |
| P2 done (iteration 83; also the Box plot chart style) | **Statistics**: K-means, PCA, Multicorrelative, Contingency, Quartiles (box plots), Random Attributes | `analysis.rs` | against NumPy/scipy on a fixed sample |
| P2 done (iteration 83) | **Annotate** filters (time, global data, attribute data, selection), **Environment Annotation**, **Python Annotation** (the overlay text exists; add the filters that write it) | `overlays.rs` in the app + an `annotate_*` family | screenshot tests |
| P2 done (iteration 83: Redistribute DataSet, Ghost Cells, Remove Ghost Information, `.pvtu` pieces as partitions) | **Ghost cells**, **Partitioned datasets** (read `.pvtu` pieces as partitions, Redistribute, D3 for the server's processes), **Overlapping Cells Detector**, **Validate Cells** | `aurafx-data/pieces.rs`, `aurafx-io/xml_vtk.rs` | the parallel server tests exist for pieces (`aurafx-server/tests`) |
| P2 done (iteration 83) | **Youngs / Material Interface** reconstruction for volume-fraction fields, **AMR Dual Clip**, **AMR Fragment Integration** | `amr.rs` | fragment volumes of a known sphere |
| P3 done (iteration 83: Halo Finder (friends of friends with SOD radius and mass), SPH cubic and Wendland kernels, Minkowski Functionals, Kernel Smoothing and Density Map) | Cosmology halo finders, SPH interpolators beyond the basic kernels, Minkowski, OT density map | `cosmology.rs`, `pointinterp.rs` | three clumps found with their counts; a ball's volume, area and mean curvature within 1, 2, 3 %, Euler characteristic 1 (torus 0); densities integrate to one |

Readers and writers (iteration 83: all done but Silo and ADIOS2, see Status): **P1** DICOM/NRRD/MetaImage/MRC/OME-TIFF
(medical and microscopy volumes; `aurafx-io/src/images.rs` already stacks images), LAS/LAZ
point clouds, STEP/IGES (done with C6), glTF/VRML/OFF import, LS-DYNA d3plot, Fluent
`.cas/.msh`, Nastran BDF; **P2** openPMD/ADIOS2 (HDF5 side first), Silo, H5Part, Nek5000, PLY
and PNG series; writers: PNG/TIFF image stacks of volumes, MetaImage, the parallel VTK
`.pv*` pieces, a `.pvd` collection writer (the app writes series as one file per format today).

### 14.2 Microstructure (after sections 11 and 13)

| Pri | Item | Where | Check |
|---|---|---|---|
| P1 done (iteration 83; the AM Bench pipeline maps 40 of its 71 steps, the rest are file reading and writing, array bookkeeping and the two-data-set alignment) | **Rotate Sample Reference Frame** and **Apply Transformation to Geometry** on volumes (the pipeline import skips both today), **Identify Sample / Isolate Largest Feature**, **Erode and Dilate Bad Data**, **Erode/Dilate by Coordination Number**, **Replace Element Attributes with Neighbor**, **Neighbor Orientation Correlation** and **Neighbor CI Correlation** (bad-data cleanup before segmentation) | `grain_grid.rs`, `features.rs`; map them in `pipeline_json.rs` | the AM Bench pipeline imports with fewer skipped steps; a seeded noisy map cleans to the known grains |
| P1 done (iteration 82: hexagonal .ang grids; iteration 83: Resample Rectilinear To Image) | **Convert hexagonal EBSD grids to square**, **Resample rectilinear to image** | `ebsd.rs` (reader option) | a `.ang` on a hex grid against its square export |
| P1 done (iteration 83) | **Find Neighborhoods**, **Find Largest Cross Sections**, **Find Feature Clustering (RDF)**, **Find Boundary Strength (m', F1)** and **Slip Transmission** metrics, **Find Twin Boundary Schmid Factors**, **Find C-axis locations / Feature Reference C-axis Misorientations** (HCP), **Identify Micro-Texture Regions** | `features.rs`, `mechanics.rs` | the AM orientation toolkit where it has the metric; otherwise analytic cases (two grains with a known misorientation) |
| P1 done (iteration 82; DAMASK checked with its own Python package) | **Solver inputs for crystal-plasticity FFT codes**: DAMASK (`.vti` + material config), Los Alamos FFT texture file, **LAMMPS atoms from a polycrystal** (a lattice filled per grain with the grain's orientation) | `aurafx-pipeline/src/cases.rs`, `aurafx-io` | read back by the codes' own Python readers where installed, else format checks |
| P2 done (iteration 83) | **Insert Transformation Phases**, **Adjust Volume**, **Generate Precipitate statistics**, **Find Surface-Area-to-Volume**, **Triangle dihedral angles / Feature Face Curvature**, **Write triangle/node files** | `synthetic.rs`, `grain_surface.rs` | |
| P2 done (iteration 83: size and shape with live plots over the packed volume's, orientations as a choice; MDF goals are matched by Match Crystallography) | **Goal statistics as a panel**: edit the size, shape, ODF, MDF and axis-ODF distributions with live plots and feed Pack Grains | `aurafx-app` panel + `synthetic.rs` | the packed volume's statistics match the panel's |
| P2 done (iteration 83; also Point Sample Triangle Geometry, Crop Points, Iterative Closest Point, Set Origin and Spacing, Copy Feature Array to Cells) | **Image filters on volumes**: median, Gaussian, Otsu threshold, binary morphology (the pipeline import skips them) | `aurafx-filters/src/images.rs` (new) | against scipy.ndimage on a small volume |
| P3 done but indexing (iteration 83: `save_pole_figures`, the Nye tensor on 2-D maps; indexing patterns needs pattern files and a reference indexer) | Pole figure images per phase as files, Nye on 2-D maps, EBSD indexing itself | later | |

### 14.3 Geometry and meshing (after sections 12 and 13)

| Pri | Item | Where | Check |
|---|---|---|---|
| P1 done (iteration 82: plane, cylinder and body webcuts, imprint, merge; iteration 83: sheet and swept-curve webcuts) | **Webcut** (plane, cylinder, sheet, sweep of a curve) to decompose bodies into sweepable blocks, **imprint and merge** (shared faces between touching bodies so their meshes conform) | `aurafx-mesh/src/model.rs` (commands `webcut`, `imprint`, `merge`), `overlay.rs` (shared interfaces exist for voxels) | two touching bricks meshed with one shared face: node count equals the union; Exodus side sets on the interface |
| P1 done (iteration 83: Face Mesh, Remesh Surface; quads by pairing and splitting triangles, then Sweep Quads) | **Triangle meshing of arbitrary surfaces** (advancing front / Delaunay with sizing) and **quad paving** of planar faces, then **sweep** of paved faces into hexahedra (the sweep of structured quads exists) | `aurafx-mesh/src/pave.rs` (new), `structured.rs` | a plate with a hole paved and swept: Mesh Quality acceptable, hex count as expected |
| P1 done but transition templates (iteration 83: intervals and bias per axis, sizes near curves, from a point array and from curvature, Winslow smoothing; hexahedra are refined locally by turning a region into tetrahedra) | **Sizing functions**: per-curve intervals and bias, per-face/body size, curvature-based size, size from a scalar field; **transition templates** for local hex refinement; **Winslow smoothing** (the "Later" line of M4) | `model.rs`, `improve.rs`, `hex.rs` | graded box: element counts per curve; refined region conforms |
| P1 done (iteration 83; named blocks per body come through `import` in the geometry commands, each body its own block) | **Nastran BDF** and **Fluent `.msh`** mesh writers; Exodus named blocks per body from CAD (C8) | `mesh_writers.rs` | read back by the readers of 14.1 |
| P2 partly (iteration 84: `thicken`, `loft`, `sew` with a tolerance; fillets on picked curves and healing of implicit bodies open, the B-rep kernel has fillets, chamfers and lofts on picked edges) | **Fillet and chamfer on picked curves/vertices** of implicit bodies (today whole-body), **loft between faces**, **thicken sheet bodies**, **sewing surfaces into volumes**, **healing/tolerant imprint** of imported STEP (C6/C7 give the B-rep versions) | `aurafx-mesh/src/implicit.rs`, C7 | |
| P2 done (iteration 83: Tetrahedra in a Region, Define Mesh Set, Merge Nodes; Transform keeps blocks and sets; iteration 84: groups in the geometry commands) | **Pyramid transitions** hex-to-tet, **node and side sets by location or by expression**, **groups** of entities, **merge nodes / equivalence** with tolerance, **mesh scale/translate/rotate** commands | `model.rs`, `tet.rs` | |
| P3 partly (iteration 84: `scheme mapped` for bodies a box bends into; multi-source sweeps and submapping open) | Sweep with multiple sources/targets, mapped hex of general 6-sided volumes, submapping | later | |

## Delivery phases

| Phase | Target | Contents |
|---|---|---|
| P0 Skeleton | done | Workspace, data model, legacy VTK + VTU/VTP + LAMMPS readers, wgpu render view, docking UI, command palette, Python console, AI chat (Ollama/Anthropic/OpenAI) |
| P1 Daily driver | mostly done | Exodus, PVD/time series, core ★ filters, color maps + scalar bar, spreadsheet + line chart, screenshots, state files, settings |
| P2 Analysis | mostly done | Statistics/data-analysis filters, selection + Find Data, Python API breadth, Python trace, programmable filter, charts breadth |
| P3 Simulation | mostly done (Slurm untested) | Job runner local + SSH, templates, sweeps, watch folders, remote file browser, GPU monitoring |
| P4 Rendering depth | in progress | Volume rendering, widgets, lighting/SSAO/EDL, animation + keyframes, exporters, LIC, ray tracing |
| P5 Breadth | in progress | Remaining readers/writers/filters from the catalog, AMR/HTG/molecule/cell-grid types, higher-order cells |
| P6 Scale | in progress (data server, parallel server processes with MPI/Slurm launchers; out-of-core, collaboration, in-situ todo) | Client/server, distributed, in-situ, collaboration, out-of-core |
| P7 Polish | mostly done (packaging, Python and Rust plugins, accessibility; localization and three package formats todo) | Plugins, packaging on all OSes, accessibility, localization, docs and benchmarks |
| P8 Grains | done (section 11) | Crystal core, EBSD readers, features, texture, synthetic builder, surface meshing, mechanics, `.dream3d`, solver templates |
| P9 Mesh | done (section 12) | Geometry kernel and command language, structured hex (hexaform), overlay-grid mesher, refinement and tets, grains on meshes |
| P10 CAD | built, C0 to C9 (section 13; the iOS target and the installers were not built here), branch `feat/CAD-modeling-environment` | auraFX's own parametric CAD modeler: the document and two kernels (implicit; B-rep from SolidCraft's code carried in `vendor/` as plain files, free to diverge), the environment with its ribbon, commit with the downstream check and fixers, STEP/IGES/3MF, meshing from CAD; upstream work comes in as a three-way patch of the carried directories (`vendor/UPSTREAM.toml`, `scripts/cad/upstream-pull.sh`) |
| P11 Gaps | mostly done (iterations 80 to 84; the open rows wait for the user's research) | The prioritized items missing against the reference programs' catalogs: filters and readers, microstructure cleanup and metrics, solver inputs, webcut/imprint/merge, paving and sizing |

P8 and P9 are developed interleaved (G1, M1, G2, M2, G3, M3, G4, G5, M4, G6, M5, G7) so that
each side is usable early; see the design document for the test reference of every stage.

P10 is developed on its own branch in the order C0, C1, C2, C3, C5, C4, C6, C7, C8, C9 (the
commit-and-check loop before sketching, the B-rep kernel after the environment works on the
implicit one); P11 items are independent of each other and of P10 except where a row says so,
so they can be picked up one at a time in any order within a priority.

---

## Status (2026-11-10)

Review of iterations 2 to 52 against the catalog: the catalog marks, the "Done in iteration"
notes and the user guide (every feature has its entry and screenshot; all 150 tests pass)
agree; the README's feature table had not followed iterations 18 to 52 and was brought up to
date, as were `docs/ARCHITECTURE.md` (new dataset types, the server) and the CML catalog
entry. The wordmark logo lost its black background (`assets/branding/logo-wordmark.png`, with
`logo-wordmark-light.png` for light backgrounds). Sections 11 and 12 (phases P8 and P9)
were added from `docs/design/grains-and-meshing.md`, the design for the grain workbench and
the geometry/mesh generator.

Done in iteration 53 (G1a, the first stage of P8): the `aurafx-crystal` crate (rotations
between Euler angles, quaternions, matrices, axis-angle, Rodrigues and homochoric vectors;
the 11 Laue classes built by closure from their generators; disorientation angle and axis;
fundamental-zone reduction; inverse pole figure and Euler colors), EBSD readers for TSL/EDAX
`.ang` (square grids) and Oxford `.ctf` (hexagonal phases moved to a1 along x by default),
the Grains filters Orientation Colors, Convert Orientations and Rotate Orientations, and the
IPF Color Key source. 8-bit arrays with 3 or 4 components are drawn as their own colors
(rasterizer, points and the path tracer), with a Display switch to use the color map.
Checked against `testdata/crystal/reference.json` (SciPy rotations, brute-force
disorientations over SciPy's groups, IPF colors reduced independently; the toolkit's cubic
disorientation agrees with the brute force) and the synthetic maps of
`scripts/gen_ebsd.py`. Guide chapter: Grains and crystals.

Done in iteration 54 (G1b, closing G1): HDF5 EBSD readers (`.h5ebsd` TSL/HKL stacks read as one
volume, EDAX OIM and Bruker Esprit `.h5` scans recognised by their root Manufacturer dataset,
points placed by position or beam index) and the `.dream3d` reader (image, rectilinear and
shared-list geometries; cell/vertex arrays as cell/point data, feature and ensemble arrays as
field data; reader options for the data container and for reading only some arrays of big
volumes). Grains filters Crop (box in sample coordinates, features renumbered), Change
Resolution (nearest cell) and Align Sections (misorientation, feature ids or given shifts;
hill-climbing search per slice pair, in parallel). Orientation Colors agrees with DREAM.3D's
stored IPF colors within 1/255 on a window of NIST's AM Bench IN718 volume
(`testdata/dream3d/am_bench_subset.dream3d`); the full 3.5 GB volume reads in 3 s and aligns in
7 s. Next: M1.

Done in iteration 55 (M1, the first stage of P9): the `aurafx-mesh` crate with implicit solids
(brick, sphere, cylinder, cone, torus, prism, superquadric, spherical-harmonic shape), moves,
rotations and scaling, unite/subtract/intersect, 2-D outlines (polylines, rectangles, circles,
ellipses, regular polygons, splines, appended lines and arcs, fillets) extruded, revolved
(full or partial) or swept along a path, closed surfaces (STL, PLY, OBJ, VTK) as signed
distance fields (bounding-box tree, pseudonormal sign), named faces that survive booleans,
and the geometry command language with its journal. The Geometry source (Properties: command
box and a Bodies tree) draws each body by contouring its distance field and snapping the
points onto the surface; Signed Distance Field turns any closed surface into a distance grid.
`afx.mesh.geometry()` builds and checks commands from Python; command-box edits go into the
trace. Volumes and areas match closed forms (0.5 % / 1 %); STL round trip. Guide chapter:
Geometry and meshing. Next: G2.

Done in iteration 56 (G2): Segment Features (misorientation, c-axis alignment, scalar; mask;
shuffled ids), Fill Bad Data, Minimum Size, Feature Sizes, Centroids, Shapes (exact
moments; equal-moment ellipsoid or ellipse, Omega3), Average Orientations, Reference
Misorientations, Kernel Average Misorientation, Feature Neighbors (neighbour counts, surface
features, boundary cells, mean neighbour misorientation), Distance to Boundaries (exact
Euclidean distance transform), Merge Twins (cubic Σ3), Feature Boundaries (faces with
misorientation and twin flags), Feature Colors and Feature Table. Per-feature results are
field arrays with a row per feature id; Feature Table turns them into a table. Checked
against DREAM.3D's stored results on a window of the AM Bench IN718 1 µm volume
(`testdata/dream3d/am_bench_features.dream3d`) and synthetic shapes; the full 2 µm volume
segments in 9 s. Guide chapter: Grains and crystals. Next: M2.

Done in iteration 57 (M2a): the hexahedral mesh model in `aurafx-mesh` (element blocks, side
sets, node sets, element variables; exact weld; orientation; HEX20/HEX27 elevation with shared
nodes and surface projection; periodic pairing by coordinates), structured box and mapped
blocks with grading, and the inclusion mesher after hexaform (background grid, packing of
whole-cell boxes, cubed-sphere O-grid blocks with transfinite cores and graded shells; spheres,
ellipsoids, superquadrics, harmonic grains). Sources Box Mesh and Inclusion Mesh; Mesh filters
Elevate and Extract Set; Mesh Quality with the 19 hexahedron metrics and a summary table with
acceptable ranges. Exodus gains HEX20/HEX27, blocks from `ElementBlock`, side sets and node
sets (read and written); new Abaqus `.inp` and Gmsh `.msh` writers. Checked node for node
against hexaform's own meshes (HEX8 and HEX27) and against ParaView's Mesh Quality on 24
hexahedra. Guide chapter: Geometry and meshing. Next: M2b.

Done in iteration 58 (M2b, closing M2): Plate Mesh (circular or elliptical through-hole with a
conforming O-grid ring) and Dog-bone Mesh (fillets, spline thickness profile), both node for
node with the benchmark study's hexaform meshes (`testdata/benchmark`); Sweep Quads; collective
packing for Inclusion Mesh; `mesh body`, `block`, `sideset` and `nodeset` commands meshing
bricks and cylinders (O-grid disks swept along the axis, curved HEX20/27 nodes on the surface)
with a side set per geometry face; the Geometry source's Hex mesh output. Guide chapter:
Geometry and meshing. Next: G3.

Done in iteration 59 (G3a): Pole Figure (kernel density in MRD, stereographic or equal-area
disks, any crystal directions), Inverse Pole Figure (density on the standard triangle for X, Y,
Z), ODF Sections (φ2 sections, texture index and maximum on a midpoint grid over a whole
number of copies of orientation space), the exact uniform kernel value from the random
disorientation distribution of every class; pole figures and ODF values match the AM
orientation toolkit within 1e-9 on the AM Bench window; CSV of multi-panel output with a Block
column. Guide chapter: Grains and crystals. Next: G3b.

Done in iteration 60 (G3b, closing G3): Misorientation Distribution (neighbour pairs, random
pairs and the exact uniform distribution of the class, as densities per degree for the Charts
panel), Texture Components (fractions within a cone of each sample axis) and Chord Lengths
(grain-boundary chords along x, y, z with gaps and specks handled, list or histogram); the
fractions and the chords equal the AM orientation toolkit's on the AM Bench window. Guide
chapter: Grains and crystals. Next: M3.

Done in iteration 61 (M3a): the overlay-grid hex mesher (aurafx-mesh overlay.rs) for geometry
bodies, closed surfaces and labelled images: sampled classification, clean-up of small pieces,
protrusions, one-cell layers and pinches, pillowing, projection along stair-step normals onto
exact distances (Taubin smoothing with the pillow layer for voxels), guarded Laplacian and
tiered optimisation with an untangling energy, void meshing, side sets for domain faces,
model surfaces and material interfaces, periodic pairs, stair-step mode; `scheme overlay` for
any body in the geometry commands, Overlay Mesh filter. Quality floors pass on CSG, STL and
synthetic voxel parts; a measured microstructure keeps a few nearly flat elements at saddles.
Guide chapter: Geometry and meshing. Next: M3b.

Done in iteration 62 (M3b, closing M3): the clean-up removes saddles and pinches by single-cell
changes that only lower the problem count (so it ends), which makes every element of the
measured AM Bench microstructure valid (smallest scaled Jacobian 0.15); smoothing runs in
parallel over colour classes of nodes sharing no element (100 000 voxels in about 3 s instead
of 10); convex sharp edges and corners are captured (bisection of the face change along
surface edges, three-plane corners), with imported surfaces split into patches between
creases. Guide chapter: Geometry and meshing. Next: G4.

Done in iteration 63 (G4a): Voronoi Grains (Voronoi or Laguerre power tessellation of random
seeds, periodic, random or fibre orientations), Columnar Grains (the AM toolkit's columnar
generator, identical voxel for voxel when its random numbers are replayed), Add Orientation
Noise, Jumble Orientations and Generate Ensemble Statistics (lognormal size and neighbour
fits, beta shape fits). Guide chapter: Grains and crystals. Next: G4b.

Done in iteration 64 (G4b, closing G4): Pack Grains (lognormal sizes, beta aspect ratios,
ellipsoid/superellipsoid/cube-octahedron/cylinder grains moved until overlaps and gaps are
least, gaps filled by growth, matrix phase below a full fraction, calibration that repacks
until the measured mean size and aspect ratios match the goals), Pack Like (a statistical
copy of a measured volume with its orientations), Insert Precipitates, Match Crystallography
(texture from a reference, swaps that match its boundary misorientation distribution).
Guide chapter: Grains and crystals. Next: G5.

Done in iteration 65 (G5): Quick Surface Mesh (FaceLabels, NodeType, feature orientations),
Smooth Boundaries (Laplacian/Taubin by node type, outer faces kept), Triangle Geometry (areas,
normals, minimum angles, enclosed feature volumes), Feature Surfaces (closed per-feature
surfaces; STL of a multiblock writes one file per block), Face Orientation Colors, GBCD
Section (normalised exactly to random; checked with random boundaries and a coherent-twin
stack). Guide chapter: Grains and crystals. Next: M4.

Done in iteration 66 (M4a): tetrahedral meshes with blocks and sets (aurafx-mesh tet.rs),
uniform 8-way refinement of HEX8 and tetrahedral meshes, conforming hexahedron-to-tetrahedron
(12 per hexahedron) and tetrahedron-to-hexahedron (4 per tetrahedron) conversions, Smooth Mesh
(Laplacian, smart Laplacian, condition-number optimisation), Boundary Layers on side sets,
and 20 tetrahedron quality metrics matching ParaView. Guide chapter: Geometry and meshing.
Next: M4b.

Done in iteration 67 (M4b, closing M4 but for the items listed as later): isosurface stuffing
(aurafx-mesh stuffing.rs: BCC lattice, warping by the published alpha values, cut tetrahedra
split by coning from their lowest-numbered vertex) for geometry bodies (Geometry output Tet
mesh) and closed surfaces (Tet Mesh), local refinement of tetrahedra with centre-point
closure (Refine Region). Smallest dihedral angles 21-30 degrees on the test shapes. Guide
chapter: Geometry and meshing. Next: G6.

Done in iteration 68 (G6): aurafx-crystal mechanics.rs (FCC, BCC and HCP slip families
expanded by symmetry, Schmid factors, exact full-constraint Taylor factors with family
weights, directional Young's modulus from the 6 x 6 compliance, a materials table with the
toolkit's IN718 and CoNi), filters Crystal Mechanics (yield estimate), SROD (grain classes,
bins, by-height table) and GND Density (Nye tensor, from KAM); Taylor, Schmid, modulus, SROD
and Nye GND match the AM orientation toolkit. Guide chapter: Grains and crystals. Next: M5.

Done in iteration 69 (M5): Assign Grains (Voronoi/Laguerre seeds, nearest point of a point set
or table, the feature of a microstructure volume; grain_id and orientations per element,
blocks per grain, grain-boundary side sets), Overlay Mesh carrying grain orientations (Mesh
Grains), the .tex orientation writer, the worked geometry-to-polycrystal example; the nearest
point rule reproduces the benchmark's element-to-grain table exactly. The solver run of the
worked example comes with G7's simulation templates. Guide chapter: Geometry and meshing.
Next: G7.

Done in iteration 70 (G7): the `.dream3d` writer (volumes, point sets and one-type meshes as
data containers, feature and ensemble matrices, the auraFX pipeline as JSON in
`/Pipeline/Pipeline`, read back by h5py and by auraFX), File › Import Pipeline (auraFX
pipelines rebuilt exactly; numbered-step pipeline files mapped to auraFX filters, checked on the
AM Bench serial-section pipeline), SPPARKS site files and lattice dumps (read as volumes) and
the site writer, the `.tex` layout of `EulerAngleFileReader`, and three case templates that
write their files from the selected data and run them (MOOSE polycrystal, SPPARKS grain
growth, PINNACLE crystal plasticity; files uploaded to remote hosts; results reopened named
after the run). The PINNACLE case was generated, validated and run with the installed
PINNACLE, its result read back with the grains recovered exactly; MOOSE is not built here,
so its deck is checked against the solid mechanics module's source and tests, not run.
P8 and P9 are complete. Guide chapters: Running simulations, Grains and crystals, File
formats, Geometry and meshing. Next: the user's choice among the remaining phases.

Done in iteration 71 (release 0.1.0): the workspace formatted with rustfmt (CI checks it), the
mdBook HTML committed and the book's site URL set for GitHub Pages, the crushed-can sample
committed so every guide picture regenerates from the repository, examples and tutorials
(`examples/`: heat diffusion, Cahn-Hilliard spinodal decomposition and Potts grain growth as
Python pipelines, geometry-to-polycrystal and EBSD-analysis scripts) with the guide chapter
Tutorials and examples, the install table for every platform and the server in Getting
started, and the project page `index.html` at the repository root (application videos
rendered by `scripts/site/media.sh`, install links, documentation links, licenses and the
notice on AI-built software). Tag v0.1.0 builds the Linux, macOS, Windows, Android and iOS
packages through the Package workflow. The Linux package script was run here to check it.
Two fixes found while making the page: numbered files (`run_0001.vtk`, ...) now open as a
time series as the guide said (the detection existed but was never called; result files of
a run that form one series open once), and Assign Grains keeps the mesh's point arrays when it
rebuilds the mesh with blocks or boundary sets.

Done in iteration 72 (planning, branch `feat/CAD-modeling-environment`): the CAD modeling
environment designed (`docs/design/cad-environment.md`: one window with a CAD mode, ribbon,
FeatureManager and PropertyManager, stale views, Apply/Discard, the downstream check with
default fixers and an assistant suggestion, a kernel-free document with an implicit kernel and
a B-rep kernel through SolidCraft, the port plan from EZ-YX-craft file by file), section 13
with stages C0 to C9, section 14 with the prioritized gaps against the reference programs,
the crate `aurafx-cad` started (kernel contract, the expression evaluator ported with its
tests). No feature code was written in this iteration. Next: C1.

Done in iteration 73 (C1, the document): `aurafx-cad` now holds the whole part description,
ported from the user's own EZ-YX-craft with its tests: `document.rs` (features with stable
ids that are never reused, shapes, modifiers, parameters, bindings, configurations),
`sketch.rs` and `path.rs` (constrained sketches with the damped least-squares solver and its
degrees-of-freedom report), `params.rs` (unit-aware parameters in dependency order, cycles
refused, bindings into any numeric field, configurations), `templates.rs` (six starter
parts), `analysis.rs` (mass properties, sections, interference), `assembly.rs`, the
signed-distance evaluator `kernel/sdf.rs` (the base of the implicit kernel of C2) and
`mesh.rs` (triangle meshes, marching tetrahedra), and `file.rs`: `.afxcad` files, pretty JSON
with a format version (older files load; a newer one is refused with a message), bare
documents accepted, optional assembly. Every template survives the file round trip.
50 tests. The guide chapter CAD modeling starts with C2, when the document becomes reachable
from a source node and Python. Next: C2.

Done in iteration 74 (C2, the implicit kernel and the node): `kernel/implicit.rs` turns a
document into bodies with named faces, a surface and a distance function. The surface is
drawn by marching tetrahedra over a grid with the points moved onto the exact surface (volumes
of boxes, cylinders, spheres, tori, extruded and revolved sketches are within 1 % of the
closed forms at 96 cells); every triangle belongs to the feature whose own field is nearest
and to the face of that feature's shape its normal points along (a brick's six sides, a
cylinder's or hole's side, bottom and top, an extruded sketch's bottom, top and one side per
sketch line, arc or circle, a revolve's one face per profile entity), so faces keep stable
`(feature id, index)` identities and names like `Base z+` or `Bore side` across rebuilds.
`edit.rs` is the operation API shared by Python, the assistant and the later environment
(parameter, add, sketch_extrude, sketch_revolve, modifier, suppress, remove, rename, move,
bind, set, configuration, template; numbers may be expressions of the parameters, which
become bindings; mistakes read as sentences). The CAD Model source (`cad_model`, Sources ›
CAD Model) keeps the document as `.afxcad` JSON in a parameter (so state files, undo and the
trace carry it without a new node kind; this replaces the planned `NodeKind::Cad`), takes
parameter overrides and a configuration from the node, and outputs the Surface (layout of the
Geometry source) or the Distance field. Python: `afx.cad_model()`, `node.cad.*` (solids,
sketches, extrude, revolve, modifiers, timeline, configurations, faces, volume, save/load),
`node.set(width=50)`. Properties show the dimensions (editable expressions), configuration,
features (checkbox to suppress) and faces. A flange template meshed by Overlay Mesh has no
inverted element. Guide chapter: CAD modeling (`cad_part`, `cad_panel` pictures); assistant
help added. 73 new tests across the crates. Next: C3.

Done in iteration 75 (C3, the environment shell): the CAD environment is one mode of the one
window. The session holds a working copy of the document (`aurafx-pipeline/src/cad_edit.rs`:
`cad_enter`, `cad_env_edit`, its own undo and redo, the rollback marker, `cad_env_evaluate`,
`cad_apply`, `cad_discard`); the scheduler pauses while it is open; the environment builds the
working copy on a worker thread and shows the surface as the node's output (so every view
works unchanged); Apply gives the node the document and resumes the pipeline as one undo step
(a part that does not build is refused with the reason), Discard restores the node's output
and document. The app (`aurafx-app/src/cad/`) swaps the toolbar for the ribbon (Sketch,
Features, Modify, Inspect, Files; Apply, Apply and keep editing, Discard), the Pipeline panel
for the FeatureManager (dimensions, features with suppress, rename, reorder, roll back,
delete, finishing modifiers, configurations), Properties for the PropertyManager (every
field of the selected feature, numbers or expressions, choices as menus, sketch dimensions,
placement, modifiers, bindings), adds the timeline bar with the rollback marker, a CAD menu,
the Esc question and grey banners on the stale views, charts and spreadsheets. Entry:
Sources › CAD Model…, Edit in CAD… (Pipeline context menu, Properties button), Python
`node.edit_cad()` with `cad.apply(keep_editing)`, `discard`, `undo`, `redo`, `rollback`,
`select`, `editing`. New edit operations for the managers: `set_op`, `set_dim`, `set_json`,
`remove_modifier`, `set_finish`, `remove_finish`. Tests: the session layer (enter, edit,
evaluate, undo, rollback, refused apply, apply as one undo step, discard, keep editing), the
Python flow, and `scripts/cad/env_test.sh`, an app-driven script that enters, adds a box and
a blind hole, applies and finds the node's surface with 7 faces more. The Assemble tab comes
with C7. Guide: CAD modeling › The CAD environment (`cad_environment`).

Done in iteration 76 (C5, the downstream check): `aurafx-cad/src/diff.rs` compares two builds
by stable face ids, then names, then place (kind, direction within 25 degrees, centroid within
a third of the part, area within a factor of three): each old face is the same, renamed,
replaced by one successor, ambiguous between several, or removed; bodies and dimensions are
tracked too. `aurafx-pipeline/src/cad_check.rs` collects what the nodes below refer to from
their parameters: face and body names as whole values or whole words inside command and
expression text (Python filters' code is reported as "may reference"), face and body numbers,
positions inside the old bounds that are outside the new ones, and the part's own overrides of
dimensions that were removed; it classifies each (Removed, Renamed, Moved, Ambiguous, Count)
and gives default fixes that are plain parameter changes (renamed: certain; one successor:
marked check; moved: onto the new bounds); `ai_prompt` writes the assistant's question and
`parse_ai_fixes` reads its JSON answer (code fence or bare; node by id or name; bad answers are
refused with the reason). Apply runs the check, applies the part and the chosen fixes as the
entry's single undo step, and can postpone the re-run (`RunMode::Later`: the nodes below wait,
a banner offers Run now). The dialog (`aurafx-app/src/cad/conflicts.rs`): the diff, a table of
object, parameter, what changed, default fix, assistant's fix and a choice per row, Ask the
assistant (its answer from the chat fills the column), Apply fixes and re-run, Apply without
fixing, Back to CAD, Discard; with no conflicts the question Re-run N downstream filters?
Python: `cad.check()`, `afx.cad_check()`, `cad.apply(fix='default'|'none'|list|answer text,
run='now'|'later')`, `cad.run_held()`. Centered rectangles made by the operation API stay
centered when their dimensions change. Tests: a renamed hole is fixed by default and undone
together in one step, a removed hole is offered without a fix, a replaced hole names its
successor as a guess, a refinement box outside the narrowed plate is moved onto it, the
assistant's prompt and a canned answer (parsed, applied, bad answers refused), whole-name
matching in command text and postponed re-runs, and the Python flow. Guide: CAD modeling ›
What the nodes below depend on (`cad_check`).

Done in iteration 77 (C4, sketching): the sketch canvas (`aurafx-app/src/cad/sketch.rs`, ported
from EZ-YX-craft's sketch mode) takes the place of the active 3D view while a sketch is open:
grid and axes, pan and zoom, line, rectangle, circle, arc, spline and point tools with
snapping to points and to a grid and horizontal/vertical inference, selection and dragging of
points, constraints and dimensions from the ribbon's Sketch tab (13 geometric constraints,
7 dimensions), driving dimensions edited as values or expressions of the part's dimensions in
the PropertyManager's constraint list, the solver after every change with the status colored
blue (under-defined, with the degrees of freedom left), white (fully defined) or red
(over-defined), construction shapes, delete with renumbering of everything that referred to
the deleted shapes. A sketch starts on a principal plane, on the top of the part, or on a flat
face with an axis-aligned normal (PropertyManager Faces: Sketch here; the plane, offset and
direction come from the face), or edits an existing feature's sketch (FeatureManager and
PropertyManager: Edit sketch…); it finishes as an Extrude (depth a number or an expression,
reverse), a Cut, a Revolve, or Apply sketch (`set_sketch`). New edit operations: `set_sketch`
and `sketch_json` on extrude and revolve. Python: `cad.sketch_new`, `cad.sketch_edit` open the
canvas from a script. Tests: the solver's (ported with it: fully constrained rectangle,
over-constraint reported, tangent arcs), deleting and renumbering, the finished sketch as
operations the document accepts (extrusion with an expression depth, edited sketch, revolve
volumes), faces to planes, curve flattening, and the canvas driven by a script
(`cad_sketch` picture). Guide: CAD modeling › Sketching (`cad_sketch`).

Done in iteration 78 (C6, the B-rep kernel and CAD files; first part of C7): SolidCraft v0.40.0-rc.3
and solidcore (the revision it pins) are carried in `vendor/` by `git subtree add --squash`
and pruned by deleting directories only (`vendor/prune.txt`; `vendor/README.md` says what is
carried, how it was pruned and where auraFX patches go: none yet); the root workspace
excludes `vendor/`, `aurafx-cad` depends on `solidcraft-engine` by path, a `[patch]` points
SolidCraft's git reference to solidcore at the vendored copy, the workspace `rust-version` is
1.95, and `scripts/cad/upstream-pull.sh <tag>`, `upstream-diff.sh` and `third-party.sh` pull,
diff and document (`docs/reference/third-party.md`, SPDX of every crate; README credits). The
B-rep kernel (`kernel/brep.rs`) maps the document onto SolidCraft commands run in process on a
headless session (`solid.box|sphere|cylinder|cone|torus|extrude|revolve|fillet|chamfer|shell|
combine`, `sketch.*`, `file.export`, `doc.open`): primitives, extruded and revolved sketches
(planes and rotations through custom sketch frames), holes of five kinds by cutter solids,
patterns by copies, rounding and hollowing of a feature (built as its own body, then
combined) or of the whole part, cap fillets and chamfers, picked-edge fillets and chamfers and
shells with picked open faces; what it cannot map (threads, lofts, sweeps, junction blends,
rotated tori, nested bodies) is refused with the feature and the reason, and `Auto` (the node's
default `kernel` parameter) falls back to the implicit kernel noting why. Every exact face is
labelled with the implicit kernel's names (`Base z+`, `Bore side`; `part face N` for imported
solids), so names, ids, the diff and the downstream check agree across kernels; volumes are
the engine's exact measure. Files (`io.rs`): STEP AP242/AP214/AP203, IGES, 3MF, SolidCraft
`.sc`, STL and OBJ are read as the base feature of a document (the bytes kept inside it, so it
is self-contained; `File › Open` and `afx.load` open them as CAD Model nodes that later
features cut into); STEP, 3MF, STL, OBJ and `.sc` are written by the engine's exact writers,
and a faceted B-rep STEP (one planar `ADVANCED_FACE` per triangle) for parts only the
implicit kernel builds, labelled faceted in its header; IGES is read, not written. The
implicit kernel reads imported solids through their tessellation. Edge work (C7): the part's
exact edges with their two faces, a point and a length (`cad.edges()`); the features
`EdgeBlend` and `ShellFaces`, picked by points, by face names or from the picker (the Modify
tab lists the edges or faces to tick); `Evaluated::face_pick`, `mass_properties`,
`measure_faces` (distance between parallel faces, angle) and `interference_built` (overlap of
two built parts at placements) with Python `cad.mass`, `measure_faces`, `interference`.
Tests: primitives exact to 1e-3, the plate with a hole equal on both kernels in volume and face
names, patterns, revolves, rounding of the whole part, refusals and the `Auto` fallback, the
L-bracket template on both kernels within 2 %, STEP/3MF/`.sc`/STL round trips with the same
volume and cutting into an imported STEP, the faceted STEP read back by the exact kernel,
picked fillets and chamfers against hand formulas, a picked-face shell, the analytic overlap
of two cubes and face measures, and the Python flow; `testdata/cad/` holds the committed
files (the README says how they were written). Not done then (settled in iteration 81, below): `aurafx-io` readers for
the CAD extensions (the Session opens them instead), a check against an independent STEP
reader (none is installed here), picking in the 3D view (lists are used), draft, patterns on
faces, mirror about faces, components with placements in the environment (the assembly module
and `interference` exist; there is no Assemble tab), combine as a feature, and a CI and
packaging pass over the new dependency on the Android and iOS targets. Guide: CAD modeling ›
Kernels, STEP and other files; Edges, faces and measures (`cad_import`, `cad_pick`).

Done in iteration 79 (C8, meshing from CAD, and a first pass of C9): the faces of a CAD
surface (`FaceId`, `FaceNames`) now drive the Overlay Mesh: the surface mesh's patches are the
CAD faces (`TriMesh::new_labeled`, `surface_mesh_with_faces`), so the hexahedral mesh has one
side set per face, named `Bore side`, `Base z+` or `plate_with_hole face 3` for a STEP file;
Extract Set shows one; the CAD readers (`step stp igs iges 3mf sc afxcad`) are registered in
aurafx-io's reader table, so `import 'part.step'` in geometry commands brings a CAD part in as a
body whose faces keep their names and `mesh body 1 size 0.5 scheme overlay` meshes it (the
Hex mesh output carries the same sets); solver case set names match with or without their
spaces. The worked example as a test: a committed STEP file → CAD Model → Overlay Mesh →
Assign Grains → a MOOSE case that fixes and pulls two faces by their names; guide: CAD modeling
› From a CAD part to a mesh and a solver case (`cad_mesh`). Not done then (done in iteration 81): tetrahedral meshes
of a CAD surface (Tet Mesh) keep numbered patches instead of face names. C9: keyboard (Ctrl+Enter
applies, Delete removes the selected feature, F2 renames it, Alt+1 to Alt+5 the ribbon tabs; in
the shortcuts window), hover help on the PropertyManager's fields, the Files tab's template
menu as the gallery, the assistant's modelling workflow and keyboard help, README credits.
Not done then (done in iteration 81): a gallery with pictures, the CAD video for the project page
(`scripts/site/media/cad.py`), a picture per ribbon tab (the Modify, Features, Sketch and
Inspect tabs appear in the chapter's pictures; Files has none).

Done in iteration 80 (the first rows of section 14.1): Reverse Sense, Linear Extrusion,
Rotational Extrusion, Ribbon, Subdivide (Linear, Loop), Decimate (quadric edge collapse),
Curvature (Gaussian, mean) and Delaunay 2D (`aurafx-filters/src/surface_ops.rs`), checked against closed forms (a swept line's area, the
cone of revolution's pi r l, the prism's surface and volume, linear subdivision keeping area and
volume, Loop rounding an octahedron, a sphere's 1/r² and 1/r to 4 %, a grid's triangle count and
hull, a sphere at half its triangles within 2 % of its volume); guide: Filters › Surface operations
(`surface_ops`).

Done in iteration 81 (the rest of C6 to C9, and the pictures in the light theme): every CAD picture
of the guide is regenerated with the light theme (`scripts/userguide/shots.sh` takes it by default,
`AFX_SHOT_THEME=dark` the dark one; the user's settings file is put back afterwards). The B-rep
kernel now builds the rounded junction of an added feature with the part (found from the labelled
faces; left sharp, with a note in `KernelNotes`, where the engine's own check refuses it, as for
the enclosure's standoffs), legacy revolved outlines, lofts, combined parts, mirrors about a plane
(`Shape::MirrorPlane`), drafts (`Shape::Draft`, `solid.draft`) and rounds every edge of a feature
or part while leaving out the smooth ones; the implicit kernel builds the mirror and the
patterns and says it leaves a draft undone. Four of the six templates (enclosure, flange, knob,
bracket) build on both kernels and agree within 1 % (3 % for the enclosure's open rim, which the
implicit kernel flags as approximate); the threaded rod (a modelled thread) and the vase (a shell
of a loft that the engine's check refuses) fall back by design. IGES is written (exact kernel
only) and, with STEP and the faceted STEP, read back by an independent reader
(`scripts/cad/step_check.py`, run by a test when `AURAFX_OCP_PYTHON` names a Python with OCP; one
solid, 7 faces, the volume within 0.1 %). Face operations, all on the one edit-op API (Python,
assistant and window alike): `face_pattern` (a row, grid or circle of holes or bosses on a flat
face, following the face's own axes; the new `Modifier::Offset`), `mirror_plane`, `draft`,
`combine` (another part added, cut or intersected); pickers for them in the Modify tab, and
picking in the 3D view: while a picker is open a click chooses the face under it, or the edge of
that face nearest to the click (chosen edges drawn in orange). Assemblies:
`Document.components` (parts with their own documents and placements), `kernel/assemble.rs`
(every part built with the chosen kernel, merged into one surface with a body per part and faces
named `Part/Face`, a union distance field, `component_interference`), the edit ops
`component_add/move/remove/visible/rename/op`, the Assemble tab (Insert part, Add part, Cut with
part, Keep common part, Check fit), a Components section in the feature tree with position and
turn, Python `cad.component_*`. Tet Mesh keeps the CAD faces' names as side sets. C9: the project
page has a CAD card with a video (`scripts/site/media/cad_model.py`), the guide has the
templates gallery and the Inspect tab picture, Alt+1 to Alt+6. The assistant: `afx.assistant_status()`
publishes its state to scripts, and `scripts/cad/assistant_check.sh` asks it, in the running app, to
start a CAD model, to build a part, to edit it and to change it in the CAD environment, checking the
part after each step against numbers worked out by hand. Result with `kimi-k2.7-code:cloud`: all four
steps pass (a 60 x 40 x 8 plate named by its dimensions, volume 19200; the L-bracket with Wall, Hole 1
and Hole 2, volume 33148 as worked out; the edit to 80 mm wide and 10 mm thick on the same part; a 3 mm
fillet of the whole part applied from the CAD environment, volume 44435 to 43042). With the local
`qwen3-coder-next` the runs differ from one to the next: one passed initiate, generate, edit and the
environment change (its fillet was checked from the transcript), another placed the wall wrongly and
did not apply, so the check reports the model's precision, not whether the assistant can reach CAD.
What the checks found and fixed: a script's command to the window (`afx.ask`, `show_panel`, ...) was
not carried out while the window was idle, because an idle window draws no frames (the session now
wakes it); the agent loop kept iterating on a reply whose only code block held comments (such a block
is no longer code to run: 4 messages instead of 18 or more); a second `afx.cad_model()` for an edit
started another part (the help text now says to use the existing node, how to change a dimension and a
feature's own number, and that a feature name can be used once, which the edit now enforces);
the environment's live preview and summary showed only the first part of an assembly. Checks of the build: `cargo check --target
aarch64-linux-android -p aurafx-cad` passes; the committed tree builds offline from a clean clone
(`cargo build --offline --locked -p aurafx-cad --release`, 7 minutes); the whole workspace's tests pass (345). Not done: the iOS target
(not installed here) and the installers (`packaging/`). Guide: CAD modeling › Assemblies; Edges,
faces and measures; Kernels, STEP and other files; Inspect and templates (`cad_assembly`,
`cad_face_ops`, `cad_inspect`, `cad_templates`).

Done in iteration 82 (clean-up, then the five recommended items): the Filters menu listed a
category once per run of its filters in the registry (an adjacent-only dedup), so filters appeared
several times; every menu that lists filters now comes from one builder (`aurafx-app/src/menus.rs`)
that lists each category once, the Pipeline's right-click Add filter has the same categories, and a
test checks that categories and labels are unique (the source "Spatio Temporal Harmonics" became
"… Grid"). Every menu scrolls when it is taller than the window (`MenuExt::scroll_menu`). Every
ribbon button, tab and sketch tool, and the main toolbar, has an icon beside its name (one
label-to-icon table, Phosphor 2.1.1 codepoints checked against the bundled font; a test checks the
ribbon labels). Adding a source opens a window with its standard settings and the choice to insert
or change them first (for a CAD Model: a template, or the plate's width, depth, thickness); "Don't
ask again" and Settings › Ask before adding a source. A new child now hides every node above it
(tables and charts excepted). The remote setup is one guided window (Remote menu and File › Open on
Server…): add a machine (SSH target, port, key, jump host; ~/.ssh/config aliases), check or install
aurafx-server, choose where the picture is drawn (here, on the server, auto), open a file. All 101
guide pictures were regenerated (the toolbar icons show in every window). Then: (1) solver inputs from
a microstructure volume, DAMASK (grid.vti, material.yaml with one material per grain as a DAMASK
quaternion, load.yaml; loaded by DAMASK's own Python package: complete, valid, same quaternions), an
FFT texture file, and LAMMPS atoms of a polycrystal (each grain filled with the FCC/BCC lattice at its
orientation, overlaps removed across boundaries; atom count, minimum distance and <110> neighbour
directions checked); hexagonal-grid .ang files read on a square grid by nearest point (checked
against brute force; `testdata/ebsd/hex_grains.ang`). (2) Iso Volume, Extract Cells By Region and By
Type, Clip Closed Surface (watertight caps with holes, oriented outward; cube and torus checked
exactly), Delaunay 3D (Bowyer–Watson with a symbolic vertex at infinity and an in-sphere determinant;
empty spheres, hull volume, a degenerate grid's volume and surface checked). (3) webcut (plane,
cylinder, body; a brick cut along its own axis stays two structured bricks), imprint and merge, and
interface side and node sets between any two bodies' elements. (4) Compute Derivatives (gradient,
strain rate, vorticity, divergence, Q, lambda2), Tensor Principal Invariants, Yield Criteria, Tensor
Glyph, Surface Vectors, Surface Flow, Table To Points / Structured Grid, Transpose Table, Table FFT
(Bluestein for any length), Merge Vector Components, Point And Cell Ids, Generate Global Ids,
Coordinates. (5) the assistant: a picture of the 3D view with a message (all three providers'
formats), local-only mode and path redaction, Filters › Describe a filter…; and the CI's first run
through a pull request. Bugs found on the way: a script's command to an idle window waited for a
frame (fixed earlier this iteration), a search for "iso" now still finds Contour first. Guide:
Getting started (Adding a source, hidden parents, scrolling menus); Data on another machine;
Simulations (DAMASK, FFT, LAMMPS); Grains (hexagonal grids); Geometry (webcut, imprint, merge);
Filters (Extracting parts and making tetrahedra; Tensors, derivatives and tables); AI assistant.

Vendored code and licences (after iteration 82, 2026-11-08): the branch history was rewritten
before it reached `main` so that one commit adds only the carried SolidCraft and solidcore
directories (the two whole-repository subtree snapshots and the prune commit are gone; the CAD
branch adds 176 MB to the history instead of 312 MB, and solidcore's test corpus and
SolidCraft's translations are no longer carried). `vendor/UPSTREAM.toml` records the upstream
revisions; `scripts/cad/upstream-pull.sh` takes upstream work as a three-way patch of the
carried directories (checked against upstream `main`: auraFX edits stay, same-line edits are
conflicts; `main` now keeps solidcore inside SolidCraft's tree, a one-time move described in
`vendor/README.md`). `scripts/licences.sh` writes `THIRD-PARTY-NOTICES.txt` (568 crates, all
under licences auraFX can ship; the NOTICE and licence files of SolidCraft and solidcore; the
font licences) and `docs/reference/third-party.md`; CI runs `--check`; the Linux, macOS and
Windows packages ship the notices and Help › About auraFX › Third-party licences shows them.
A fresh clone of the branch passes all 378 tests with `--locked`. Guide: Getting started ›
Open-source software in auraFX (`about_licences`).

Asked next (2026-11-08): several chart views at once, each plotting its own pipeline object, and
pictures and animations of all the open views (3D views and charts) as they are laid out, for
presentations; then the rest of section 14, before the pull request that runs CI on this branch.

Done in iteration 83 (2026-11-09), the request above and section 14: several chart views at
once, each bound to its own pipeline object (View › New Chart View, the Pipeline's Plot in a new
chart view, `node.plot(...)`, `afx.chart_view`), saved in state files; pictures of all the views
as laid out (3D views and charts, `afx.screenshot(views='all')`, File › Save Screenshot) and
animations of them (`afx.save_animation(views='all')`, File › Animation), without the controls.
14.1: point interpolation (Gaussian, Shepard, linear, Voronoi, SPH kernels onto volumes, planes,
lines and data sets), Resample To Line, plots on sorted lines and intersection curves, evenly
spaced 2-D streamlines, the temporal utilities, K-means, PCA, multicorrelative and contingency
statistics, quartiles, random attributes, the annotation filters, Validate Cells, overlapping
cells, redistribution into partitions, ghost cells, material interfaces, fragments, the AMR dual
clip, halos, Minkowski functionals, kernel smoothing and density maps; the readers DICOM, NRRD,
MetaImage, MRC, OME-TIFF and TIFF stacks, LAS/LAZ, H5Part, openPMD, Nek5000, LS-DYNA d3plot,
Fluent and Nastran, glTF, VRML, OFF, and the writers for image stacks, MetaImage, `.pv*` pieces,
`.pvd`, Fluent and Nastran, checked against ParaView 6.1.1 where it reads the format. 14.2: the
bad-data clean-up filters, sample transforms, the feature metrics (neighbourhoods, cross
sections, clustering, boundary strengths, twin Schmid factors, c-axis filters, micro-texture
regions, surface area to volume, precipitate statistics), volume image filters checked against
scipy, point sampling, cropping and Iterative Closest Point, transformation phases, Adjust
Volume, triangle angles and boundary curvature, `.nodes`/`.triangles` files, the Goal
Statistics panel, pole figure pictures per phase and the Nye tensor on 2-D maps; the AM Bench
pipeline file maps 40 of its 71 steps. 14.3: sheet and swept-curve webcuts, Face Mesh
(triangles or all quads on planar faces with holes, refined near them) and Sweep Quads,
Remesh Surface (fixed size, a size array, curvature), intervals and bias per axis, Winslow
smoothing, Define Mesh Set, Merge Nodes and Tetrahedra in a Region (pyramid transitions).

Done in iteration 84 (2026-11-09), asked after iteration 83: in the CAD environment, snapping on
the sketch canvas (points, centers, midpoints, quadrants, intersections, curves, the grid, each
switched on or off; Ctrl places freely) with dashed guide lines through points also far away;
sketches on any flat face (slanted ones too, picked in the 3D view) and on reference planes
(from a plane or a face, moved and turned, or through three points; drawn in the 3D view; kept
with the part); dimensions drawn with extension lines and arrows, labels dragged and
double-clicked to type a value, distances from a point to a line and between parallel lines;
the assistant ships with no model chosen and agent mode on. Section 14: groups in the geometry
commands, the mapped scheme, `thicken`, `loft` and `sew`.

Still open in section 14: the Silo and ADIOS2 readers (no Silo library or reference reader here),
indexing EBSD patterns, all-hexahedral transition templates for local refinement (hexahedra are
refined everywhere, or a region becomes tetrahedra), fillets on picked curves and healing of
implicit bodies (the B-rep kernel has fillets, chamfers and lofts on picked edges), and
multi-source sweeps and submapping. The older open items
of sections 0 to 10 are unchanged.

Done in iterations 85 to 88 (2026-11-10), sections 1 to 10 with section 14 left until the
user has looked into it: the GPU server-render tests no longer crash with several devices at
once; NIfTI and PTS readers, Houdini `.geo`, WebM, Find Outliers and Find Change Points, a
generated filter reference and a benchmarks page, keyboard shortcuts and accessibility,
surface writers, saved assistant prompts (85); the Jupyter kernel and the Notebook panel, MPI
and Slurm launchers for parallel servers, Rust plugins (the `aurafx-plugin` crate) and Python
plugin panels (86); units on arrays (87); ghost layers between server processes and files
streamed in pieces, GPU help for Slice and Contour, 3D Glyphs, culling and coarse surfaces
while moving (88). Still open in sections 0 to 10, by
priority: piece readers for more formats, more GPU filters, localization, collaboration, in-situ
coupling, VR and stereo, POV-Ray and HTML scene export, and the remaining catalog filters.

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
OpenFOAM patches, the Exodus writer and Extract Subset are checked against the reference implementation.

Done in iteration 5: color map editor (histogram, markers, bands, reverse, user presets,
ParaView `.json` import/export) and a readable scalar bar backing.

Done in iteration 6: multi-input filters in the pipeline (Ctrl+click several objects, then
Append; an Inputs section in Properties; `afx.append_datasets(a, b)`), with Append Datasets,
Append Geometry, Append Attributes and Resample With Dataset checked against the reference implementation.
Probing image and rectilinear data is now trilinear like VTK (it was split into tetrahedra),
Point/Cell Data conversions keep structured types and drop the source arrays by default
(a "Pass" option keeps them), and a `Cells::default()` bug that lost the first appended cell
is fixed.

Done in iteration 7: time-aware pipeline execution and the first temporal filters: Particle
Tracer, Particle Path and Streak Line (incremental while playing; seeds from any dataset or a
line), Temporal Interpolator (in-between and regular time steps), Temporal Shift Scale,
Temporal Array Operator and a live Temporal Statistics. Interpolator, Shift Scale and Array
Operator match the reference implementation. The particle filters are checked against the exact paths of
`testdata/rotating.pvd` instead: the reference implementation advects each interval with the velocity at its end
(4.25 rad instead of 4.0 at t = 2) and its Streak Line output mixes seeds and has empty cells;
auraFX interpolates the velocity linearly in time and builds one streak per seed. Particle ids
are unique per particle (the reference implementation restarts them at every release).

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

Against the reference implementation, Integrate Variables, Slice, Clip, Contour and Extract Surface at levels
0-3 agree to about 1e-8 on Lagrange hexes, a rational Bezier hex and one cell of every other
shape. There are two deliberate differences:
- auraFX draws curved faces as quads, where the reference implementation splits them into triangles.
- On Bezier cells, the reference implementation integrates the control net (3.0 for the quarter cylinder) and
  skips Lagrange curves with an odd number of points; auraFX integrates the evaluated cells.

VTK XML files are now written as version 2.2, so the reference reader reads our Lagrange hexes correctly.
Older files are converted on reading the way VTK does. Curved cells also orient their faces
from their corners, because VTK's Lagrange wedges are wound opposite to linear ones. Legacy
`.vtk` files cannot carry degrees or rational weights for VTK, so use `.vtu` for curved cells.

Done in iteration 10 (first part): serendipity quadratic cells. 20-node hexes, 15-node wedges
and 8-node quads use serendipity shape functions. Quadratic-linear quads and wedges and cubic
lines are treated as Lagrange cells. Extract Surface draws their faces with the reference implementation's pattern
at every level: same point counts at levels 0-3 and areas within 1e-7 on
`testdata/quadratic_shapes.vtu`. Slice and Contour match the reference implementation exactly. Integrate Variables
gives positive volumes where the reference implementation's are signed or wrong (inverted 15-node wedge,
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
  vertex colors. The reference viewer reads both with the same points, cells and bounds.

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
  GL2PS exports. PDF: convert the SVG (Inkscape, rsvg-convert).
- **X3D and VRML** scene export (`.x3d`, `.wrl`), with absolute coordinates; the reference viewer reads the
  VRML file with the same points, cells and bounds as the glTF.
- **Wave keyframes:** a key that swings to the next key's value and back (sinusoid).

Done in iteration 18 (P5 breadth: readers), each compared with the reference reader:
- **VTKHDF** (`.vtkhdf`, pure Rust through `hdf5-reader`): image data, unstructured grids, poly
  data (several parts), partitioned collections nested by their assembly, time steps from the
  `Steps` group, contiguous, chunked and gzip-compressed storage. Counts, bounds and ranges
  match the reference reader on its own exports (a disk merged to one grid, Exodus blocks, a 3-step series).
- **Images**: PNG, JPEG, BMP, TIFF (8/16-bit, float, gray/RGB/RGBA) as image data with
  the reference reader's array names and row order; multi-page TIFFs and wildcard paths
  (`afx.load("slices/img_*.png")`, natural order) as volumes. The reference reader crashes on the
  multi-page 16-bit TIFF that VTK writes; VTK's writer also labels its rows top-down while
  storing them bottom-up, and we follow the label as image viewers do.
- **EnSight Gold** (`.case`): ASCII and C binary (either byte order), every standard element
  type (wedges turned round as VTK does), nsided/nfaced, structured blocks, per-node and
  per-element scalars/vectors/tensors, time sets with wildcard file names; one block per part.
- **Tecplot ASCII**: ordered and FE zones, POINT/BLOCK packing, cell-centered variables,
  old-style FEPOINT headers, multi-line headers, `n*value` repeats, Fortran exponents (the last
  three confuse the reference reader, which loses the zone).
- **PDB** (all atoms, hydrogens included, MODELs as time steps, CONECT plus distance bonds) and
  **Gaussian cube** (density volume and atoms in Å; the reference reader keeps grid-index units).

Done in iteration 19 (more readers):
- **Xdmf 2/3**: inline, HDF5 and raw binary data; mixed and single-type unstructured,
  curvilinear, rectilinear and image topologies; spatial and temporal collections;
  `xi:include`/`Reference` XPath pointers (the way FEniCS and Dolfinx share one mesh across
  time steps); HyperSlab and JOIN items. Matches the reference reader on its own exports; it drops a
  HyperSlab field and fails on a curvilinear Xdmf 2 grid, both read here.
- **NetCDF CF** (classic and NetCDF-4): the grid most variables share, lon/lat on a unit sphere
  with cells between the `bounds` edges (as the reference reader), rectilinear otherwise, time steps from the
  time coordinate, `scale_factor`/`add_offset`, fill values as NaN. Counts, ranges and values
  match the reference reader on the IPCC sea surface temperature example and on generated files. Exodus
  files with a `.nc` name still open as Exodus.
- **PLOT3D**: grids and Q solutions, the binary layout found from the file size (Fortran or C
  binary, byte order, precision, multi-grid, 2-D/3-D, IBLANK), ASCII too; Density, Momentum,
  StagnationEnergy, Pressure and Properties as in the reference reader.
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
  sections left out, as the reference reader's default); vertex and cell-center solutions with X/Y/Z
  components joined into vectors; time steps from the iterative-data pointers. Matches
  the reference CGNS reader on its own export and on a hand-made file with a structured zone,
  a MIXED section, a boundary section and two time steps. (ADF storage: iteration 24.)
- **Binary Tecplot** (`.plt`, format 1xx): ordered and FE zones, node- and cell-centered
  variables of any storage type, shared variables and connectivity, auxiliary data. A file
  written from the spec reads the same as its ASCII twin; the VisIt reader agrees on
  everything but the double-precision cell-centered variable, which it misreads. SZL
  (`.szplt`) files are not read.

Done in iteration 21: a VTKHDF writer on auraFX's own small HDF5 encoder (`node.save("x.vtkhdf")`,
`all_times=True`, the Save dialog; read back by h5py and our reader, not yet by the reference
reader), and the **user guide** (`docs/userguide`: Markdown sources, mdBook HTML, PDF) with a
generated screenshot per feature (`scripts/userguide/shots.sh`, `build.sh`). Rule in CLAUDE.md:
every feature ships with a guide entry and screenshot.

Done in iteration 22: **EnSight Gold writer** (C binary `.case`; parts per block, structured
blocks, every element type, polygons and polyhedra, time series with the geometry written once
when it does not change) and **Xdmf 3 writer** (`.xmf` + `.h5`; Co/Rect/SMesh, single-type or
Mixed topologies with polyhedra, collections, temporal collections sharing unchanged points and
cells); the Xdmf reader reads Mixed polyhedra. Both read by the reference reader with the same counts,
bounds and ranges. VTKHDF output now opens in the reference viewer: our HDF5 groups did not track link
creation order, which VTK iterates by, and its HDF5 then allocated until the machine
swapped. Fixed along the way: `afx.active_view()` raced the app (per-view show/hide from
scripts), blocking updates (`save(all_times=True)`, values over time) could use a stale output
while the app's worker held the node, and a color array set before the data arrived was looked
for in the wrong attribute set. `cargo run -p aurafx-io --example inspect -- file [step]` prints
what a file holds.

Done in iteration 23: **CGNS writer** (HDF5, CGNS 4 layout: structured zones, one element
section per zone (typed, or MIXED with offsets) plus NGON/NFACE for polygons and polyhedra,
bases by cell dimension, vertex and cell-center solutions with vectors as X/Y/Z, time series
with per-step solutions and moving coordinates through `ZoneIterativeData` pointers); read by
the reference reader with the same counts, bounds and ranges. `afx.split_view()` waits for the new
view and returns its number (two splits in a row split the same view before).

Done in iteration 24: **CGNS in ADF storage** (the pre-HDF5 format, same `.cgns` extension,
told apart by the file header): the 64-bit "B" layout the CGNS library writes today and the
old 32-bit "A" layout with hex-text pointers, little- or big-endian data, data split into
chunks, links inside the file and to other ADF files. The reader walks ADF nodes and HDF5
groups through one node type, so everything the CGNS reader does works for both. The test
files are written by the reference implementation's CGNS library (`scripts/gen_cgns_adf.py`); an
old-layout file made from one of them (`scripts/gen_adf_old.py`) is read back by that library
to the same values. Every file the CGNS writer produces, copied to ADF, reads exactly as the
HDF5 original, and the reference reader reports the same counts, bounds and ranges on the ADF copies.

Done in iteration 25: **MPAS** NetCDF output (ocean, atmosphere, land ice), found from the
file's variables: the dual grid as the reference MPAS reader builds it by default (cell centers as
points, `cellsOnVertex` triangles or quads), cell variables as point data and vertex variables
as cell data at the first index of extra dimensions (the top level), numbered time steps,
`xtime` in field data. Boundary triangles and triangles touching land (`maxLevelCell`) are left
out; the reference reader keeps them collapsed onto an extra point at the origin. On an MPAS-style test
file (`scripts/gen_mpas.py`) every point array and every cell the reference reader keeps match exactly.
Tecplot SZL is set aside: there is no reference reader to compare with and the format is not
published. Reader options (MPAS vertical level, multilayer and lon/lat views) need a reader
parameter path through the pipeline first.

Done in iteration 26: **GIS rasters** without GDAL: GeoTIFF (found from its GeoTIFF tags; any
integer or float bands, compressed, strips or tiles, pixel- or band-interleaved, tie point and
scale or a transformation matrix, pixel-is-point moved to the corner as GDAL does) and Esri
ASCII grids, as GDAL's raster reader lays them out: image data with a cell per pixel
at map coordinates, north up, bands named "Elevation", "Band 2"..., RGB(A) as one array,
`NO_DATA_VALUE` and `MAP_PROJECTION` (EPSG code) in field data. No-data becomes NaN (the reference viewer
hides the cells). Test files are written by the GDAL library through ctypes
(`scripts/gen_rasters.py`); extents, origins, spacings, first/last values and sums match
the reference reader. The `tiff` crate reads only the first plane of band-interleaved images, so those are
put together chunk by chunk. The reference GDAL build has no ASCII grid driver; that file is
checked against the GeoTIFF it was written from.

Done in iteration 27: **reader options**. A reader can offer settings for a file
(`aurafx_io::reader_options`); they are the reader node's parameters, shown under Properties ›
Reader options, set from Python with `node.set(...)` (unknown keys are refused with the list),
saved in state files and undo history, and applied by reading again (static files) or with each
time step (series), through a thread-local like the piece setting. First user: **MPAS** vertical
level, multilayer view (wedge/hex columns, topography per level), layer thickness (0 = automatic),
atmosphere (layers outwards) and the lon/lat map with cells crossing the edge mirrored to both
sides and a chosen center longitude. Checked against the reference MPAS reader with the same
settings: level 2 ranges, map point/cell counts and bounds (both centers), multilayer map depth.
The reference reader's multilayer point values are uninitialised memory and it never drops land
layers (unsigned comparison); both noted in the guide. Options are not sent to remote servers yet.

Done in iteration 28: **CAM-SE** (data file plus connectivity file, found beside it or named in
a reader option): the reference CAM reader's layout on a lon/lat map, seam quads moved to their
shorter side on copied points (the reference reader's inverted-cell test and anchor rule), surface /
midpoint / interface variables, all levels as hexahedra with a level array or one level, and a
sphere layout of its own. **POP** and other curvilinear CF files: 2-D longitude/latitude
from the `coordinates` attribute put the grid on the sphere, a vertical axis as radius with
the reference reader's rule (positive="down" negated, shifted to non-negative; now also for 1-D lon/lat
grids), the auxiliary coordinates kept out of the data, and a "Variables on" reader option to
pick the grid. The reference POP reader reads only 3-D variables, so
POP history files are compared with the reference NetCDF reader. CAM checked in all four of the
reference reader's modes (counts, bounds, first cell, array sums); POP on both grids (bounds, ranges).

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
Prevent Seam, ported from VTK so all 14 cases on two surfaces match the reference implementation to float
precision. The display gets a **Texture** (PNG/JPEG, mipmapped, repeating): texture
coordinates travel packed as two halves in the vertex scalar, so the vertex layout is
unchanged; the image replaces array colors, lighting still applies (ray tracing, scene export
and server-drawn pictures show the solid color). **Perlin Noise** (VTK's Graphics Gems
version, bit-exact hashing) and **Spatio Temporal Harmonics** as a filter (input's time; time 0
on static data, as the reference implementation) and as a source with **time steps of its own**: sources can now
be temporal (their steps join the animation, they recompute per step, saved states keep
them). Values match the reference filters and source (default harmonics and times as its
rounded defaults).

Done in iteration 31: **GIS vectors** without GDAL: ESRI Shapefiles (.shp/.dbf/.prj; all
shape types but multipatch, dBASE field typing as GDAL), GeoJSON (field order and typing as
GDAL), KML (folders as layers, Name/Description/extended data typed by Schemas) and GeoPackage
(own read-only SQLite reader: b-tree pages, overflow chains, rowid aliases; GPKG geometry
blobs and WKB with Z/M), laid out as GDAL's vector reader (vertices, polylines,
polygon rings as closed lines, attributes as cell data). Reader options: one block per layer
(default) or per feature, layer choice, filled polygons (ear clipping with hole
bridges, exact areas on a 20 000-point polygon with holes), feature ids. With one block per
feature every point, cell size and attribute matches the reference reader on GDAL-written shapefiles,
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
filter: Overlapping AMR Level Ids; Outline draws every box. Checked against the reference implementation:
source (cells, sums, covered cells), both readers (per-box origin, size, sums), Integrate
Variables, slices, Threshold, Extract Surface, Cell Data to Point Data, Resample To Image.
Array parameters given by name now find cell arrays too (shared `array_or_default`).

Done in iteration 33: **AMR Contour and AMR Dual Grid**. The dual grid joins the cell centers
of all levels; dual cells of a finer level whose corners fall outside its boxes take the
coarser cells' centers there (degenerate hexes), and dual cells touching covered cells are
left to the finer level, so contours (and clips, slices on the dual grid) are crack-free
across levels. Contour on an AMR cell array uses it automatically. Checked on a plotfile whose
isosurface is a sphere crossing the level boundary: closed, Euler characteristic 2, area within
0.2 %, centered (the box-by-box contour had 346 open edges); 2-D isolines close. The reference
AMR Contour / Dual Clip only take CTH non-overlapping AMR, so there is no reference to compare with.
Also fixed: Surface With Edges drew the edges from the color map (now the edge color, with a
Properties control), and the renderer could keep a stale mesh when a new mesh reused a freed
one's address (it now holds the mesh it uploaded).

Done in iteration 34: **hyper tree grids**. A new dataset type (`aurafx_data::HyperTreeGrid`:
root grid coordinates, branch factor, per tree a breadth-first refinement list with the cell
index of each vertex, mask, cell data for every vertex), read from VTK `.htg` (XML version 2,
bit arrays MSB first) and made by the Hyper Tree Grid source (string descriptor, VTK's
level-major numbering) and Random Hyper Tree Grid source (VTK's minimal-standard random
sequence per tree, creation-order numbering; no masking). Filters: To Unstructured Grid
(leaves with shared corners, `OriginalIds`), Depth Limiter, Generate Fields, Evaluate Coarse
(all operators); every other filter (and drawing, writing) works on the visible leaves.
Checked against the reference implementation: counts, leaf order (OriginalIds), depth sums, limiter,
fields, coarse operators, both sources. Also fixed: pixel cells were drawn and surfaced with
their corners out of order (bow-ties); they now become quads.

Done in iteration 35: **molecules**. The Molecule representation (ball and stick, van der
Waals, liquorice; atom size, bond radius, bond orders as parallel sticks; bond halves in their
atoms' colors) built as sphere/cylinder meshes with level of detail, the Elements color map
(Jmol colors, exact per element over [-0.5, 127.5]), element data from VTK's periodic table
(`aurafx_data::elements`), molecules opening that way by default (up to 100 000 atoms), the
CML reader (bond orders) and Compute Molecule Bonds (VTK's simple bond perceiver; existing
bonds kept, no duplicates). Checked against the reference implementation: the CML reader (atoms, bonds,
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
Checked against the reference implementation on VTK's ten DG test files: every corner point and value
(HGRAD, HCURL, HDIV), the centers and their values, elevation, the source's fields.
Differences: quadratic cells become linear cells through the corners (the reference implementation keeps the extra
nodes but evaluates them at parametric (0,0,0)); flat cells' Jacobians are completed with the
unit normal (the reference implementation: NaN/undefined for face/edge fields); Centers on mixed types gives every
cell (the reference implementation one point). Protocol 10.

Done in iteration 37: **hyper tree grid writer, dual grid and masking**. The `.htg` writer (XML
version 2: per tree its breadth-first descriptor, vertices per depth, mask and cell data in
that order), Hyper Tree Grid To Dual Grid (VTK's Moore super cursor walk reproduced: neighbors
found level by level, never below a masked cell; corner ownership by level and neighborhood
index; boundary and mask point shifts, including VTK's diagonal signs), and the Random Hyper
Tree Grid source's Masked fraction (VTK's shuffled trees and children, masking cost per level).
Fixed: the Hyper Tree Grid source's mask string lost its digits, so Use mask always failed.
Checked against the reference implementation: our `.htg` files read in the reference reader (cells, leaves, Depth sums)
and back in auraFX (same leaves and values); the dual grid cell by cell on eleven grids (1-D,
2-D, 3-D, files, masks, masked random grids); the masked descriptor source and the masked
random source (cells, visible leaves, depth sums, masked counts). auraFX's dual grid leaves out
VTK's unused points at the origin.

Done in iteration 38: **more GIS vector formats**. FlatGeobuf (own FlatBuffers reader: header
columns and CRS, features after the packed Hilbert R-tree index, every property type, all
simple geometry types), GML 2, 3 and 3.2 (feature types as layers, fields typed by the `.xsd`
beside the file or by their values, posList/pos/coordinates/coord, Curve and Surface patches;
coordinates swapped back for latitude-first EPSG URNs as GDAL does) and KMZ (own zip directory
reader, deflate via flate2, then the KML reader). Checked against the reference reader on FlatGeobuf
files written by GDAL (including 300 features with a three-level index); GML (3.2 and 2) and
KMZ, which the reference GDAL build cannot read (no XML parser), against the same data as GeoJSON.

Done in iteration 39: **FLASH and Enzo AMR readers**. FLASH HDF5 plot/checkpoint files (FLASH 2
and FLASH 3/4 layouts: file format version from the dataset or "sim info", nxb/nyb/nzb from
"simulation parameters" or "integer scalars", blocks from gid, bounding box and refine level,
every variable in "unknown names" as doubles; recognized by content since the names have no
extension) and Enzo dumps (parameter file, `.hierarchy` parsed token by token as VTK does,
`GridNNNNNNNN` groups in the HDF5 grid files, the field names of the grid with the fewest cells,
stored types kept, reader option Convert to CGS from `#DataCGSConversionFactor`). Both become
overlapping AMR with covered cells hidden. Checked against the reference reader on files written by
scripts/gen_amr_files.py (FLASH 3 and FLASH 2 layouts of a 3-D grid; Enzo with three levels,
with and without CGS): every block's origin, spacing, size, array types and sums, covered
cells. The reference reader cannot read 2-D FLASH files ("Inconsistent types of vtkCartesianGrid",
segfault with several levels); the 2-D file is checked against its analytic values.

Done in iteration 40: **Chombo AMR reader**. Chombo HDF5 files (cell-centered, 1-D to 3-D):
`Chombo_global/SpaceDim`, component names, `level_N` groups with compound `boxes`, dx or
`vec_dx`, `prob_lo` and `aspect_ratio` (compound or array attributes, decoded from their raw
bytes), every box's components from `data:datatype=0` with the `outputGhost` ghost cells
dropped. Overlapping AMR with covered cells hidden. Checked against the VisIt
Chombo reader on two files from scripts/gen_amr_files.py (3-D; 2-D with ghosts, prob_lo,
vec_dx and three levels): every block's origin, spacing, dimensions, array sums and covered
cells, and the 2-D values against the functions they were written from. The reference reader keeps level
0's ghost cells as ordinary cells (and drops the finer levels'); auraFX drops them everywhere.

Done in iteration 41: **Resample AMR**. AMR data sampled on a uniform grid (number of samples,
whole domain or a Min/Max box clipped to the data with VTK's sample adjustment), each point
taking the finest holding cell's values found the way vtkAMRResampleFilter's probe does (closed
box bounds, the previous point's box tried first, then finer levels), or cell centers as cell
data; unheld points hidden. Checked against the reference implementation (demand-driven mode off): grid and
every value on the 3-D and 2-D Gaussian pulse, a region past the data, and a FLASH file.
The reference implementation's cell-center mode copies wrong cells' values; ours is checked geometrically.

Done in iteration 42: **FLASH and Enzo particles**. Reader option Data (Grid / Particles) on
files with particles. FLASH 3 particle tables (columns named by "particle names") and FLASH 2
compound particle datasets, every property as doubles; Enzo particle_position_x/y/z and the
other particle_* datasets of every grid (names from the smallest grid with particles), stored
types kept, all grids in one point set. Checked against VTK's particle readers
(driven directly; the reference proxies list no arrays): positions and properties.
The reference reader repeats FLASH particles in every block and reads every FLASH 3 property from column 0.

Done in iteration 43: **Chombo node-centered data and particles**. Node-centered Chombo files
(data_centering 7: a value per node, ghosts dropped) give point data on the boxes; the
/particles group (position_x/y/z and one dataset per property, name order, doubles) is read
with the reader option Data = Particles. Checked against the VisIt Chombo reader:
the node-centered boxes and point sums, and every particle and property (its positions are
single precision).

Done in iteration 44: **Hyper Tree Grid Geometry and Feature Edges**. The neighbor lookup of the
dual grid became a shared cursor (Moore and von Neumann neighborhoods as VTK's super cursors
find them). Geometry: a line/quad per visible leaf in 1-D/2-D; in 3-D the faces of visible
leaves next to the grid's boundary or masked cells and the faces of masked cells next to a
coarser visible leaf, corners in VTK's edge-walk order, points shared within a cell. Feature
Edges: VTK's rules (2-D outline; 3-D corner and fold tests on the three neighbors of each
edge, masked-cell cases). Checked against the reference implementation on eight grids: every cell in order,
its points and values, point counts. Material interfaces and ghost cells not done.

Done in iteration 45: **cell grid writer, Transform, Warp, Convert To Cell Grid**. The .dg
writer (VTK's JSON layout; the files open in the reference viewer); Cell Grid Transform (HGRAD/constant 3-D
values, shared or per cell, by the reference transform), Cell Grid Warp (shape + scale x a shared
HGRAD attribute of the same basis and order), Convert To Cell Grid (eight linear shapes, a
shared coordinates group, point data as shared HGRAD C1, cell data as constant attributes).
Checked against the reference implementation through To Unstructured Grid (Transform on dgHexahedra with its
HCURL/HDIV fields, Warp on the source, Convert on a tetrahedralized wavelet); written files read
back exactly and open in the reference viewer with the same values. Also fixed: Transform rotated in the
wrong order (now Rz Rx Ry, as the reference implementation; checked on a point), and serde_json now parses floats
exactly (float_roundtrip). Cell Grid Probe not done.

Done in iteration 46: **Cell Grid Probe**: for each cell, the
probe points within the sphere around its corners and behind every bounding side's plane
(VTK's half-spaces, quirks kept: a quad face's test corner taken from the cell, a 2-D cell's
edges as directions), then Newton steps from the parametric origin (20, to 1e-7) and VTK's
IsInside; a vertex per (point, cell) with CellType, ContainingCellID, ParametricCoordinates
(NaN when outside) and the attribute's value. Checked against the reference implementation with its SMP
backend sequential (its parallel interpolation races on a shared cache) on eight of VTK's DG
files (a lattice plus cell centers): the same hits, and coordinates and values (HCURL/HDIV
too) wherever the reference implementation has them. It gives NaN on 2-D and 1-D cells (singular Jacobian),
reads undefined memory for edges and crashes on quadratic shapes; auraFX solves them (checked
by the cell centers of the quadratic files). Edge Jacobians are now completed like 2-D ones.
Compute Sides not done.

Done in iteration 47: **Cell Grid Compute Sides**. VTK's side tables for the eight shapes (sides,
sides of sides, side types); cell types now carry VTK's side specs (blanked, offset, (cell,
side) lists by side shape), read and written in .dg files and drawn by the corner mesh (fields
from the side's own cell). The filter is vtkCellGridComputeSides' three passes: hash every
requested side (the SideFlags setting, omitting sides of renderable inputs; sides of input side
specs walked down through sides of sides) by shape and canonical point order; keep the first
(type name, cell, side) per hash under the Boundary, Winding or AnyOccurrence rule; blank the
cells and append side specs by descending shape with VTK's offsets. Checked against the reference
implementation on the ten DG files for eight settings and a second pass: the cell-type entries it
writes, every (cell, side) pair (the reference lists are in hash-table order; compared as sets) and
the cells of To Unstructured Grid. The reference To Unstructured Grid evaluates side fields on
the wrong cell (the side's list index) and crashes on vertex sides of edges; auraFX's continuous
fields on sides match the input's. Ghost-node marks are not read.

Done in iteration 48: **wide lines**. Line width (Properties › Display for wireframes, edges,
outlines and line data; `line_width=`) is now drawn: above 1 pixel each segment is an instance
of a screen-space quad (`vs_wide_line`, both vertices from the line buffer with a 2-vertex
stride, clipped to the front of the camera), extended by half the width at both ends so bent
lines join; opaque, translucent (OIT) and overlay pipelines, cell edges included, servers too.
1-pixel lines keep the hairline pipeline. Checked by a headless render: 1, 3 and 7 pixels
across and no gaps at a polyline bend. Picking still uses 1-pixel lines.

Done in iteration 49: **hyper tree grid material interfaces**. Grids keep VTK's interface
array names (InterfaceNormalsName/InterfaceInterceptsName, read and written in .htg).
Hyper Tree Grid Geometry ports vtkHyperTreeGridGeometry's interface code: in 1-D and 2-D the
material part of each mixed leaf (one plane, or the sandwich between two), Fill Material off
giving 2-D interface lines; in 3-D the faces of mixed cells and of their neighbors clipped to
the material and the interface polygons linked edge by edge, with VTK's by-value point copies
(which decide the point numbering) and the swap of inverted sandwich intercepts. The default
drawing of a grid with interfaces goes through it (picking mapped to the leaves). Checked
against the reference implementation on six grids made by scripts/htg_interface_reference.py (1-D, 2-D,
masked, branch factor 3, 3-D, masked 3-D; one- and two-plane interfaces, swapped intercepts,
zero normals), Fill Material on and off: every cell's point ids, points (the reference implementation keeps single
precision), type and source cell. Client/server protocol 11 (also covers iteration 47's cell
grid side lists).

Done in iteration 50: **MapInfo**. Native .tab tables (the .tab definition's field types, the
.dat records with binary values, the .id index into the .map file: its header's integer
scale, displacement and quadrant with GDAL 3.7's rounding to the scale's power of ten,
object blocks with compressed and plain coordinates, chained coordinate blocks; points, font
and custom symbols, lines, polylines, multi-polylines and regions with their holes in all
versions, multipoints) and .mif/.mid interchange files (Columns, Delimiter, Transform; Point,
Line, Pline [Multiple], Region with rings grouped by area and containment as GDAL's
organizePolygons, Multipoint; the .mid rows). Per-feature blocks now include features without
geometry as empty blocks, as GDAL does. Checked against the reference GDAL reader on
.tab and .mif versions of the three test layers and a 63-feature detail layer (compressed
regions with holes, 300-vertex roads across coordinate blocks, a two-point line, compressed
and plain multi-lines, points, no geometry): every point, cell and attribute. Text, arcs,
rectangles, ellipses and collections keep their attributes but no geometry.
Also **Esri file geodatabases** (OpenFileGDB): the system catalog's tables in order, each
.gdbtable's field descriptions (UTF-16 names, types, nullability, defaults, the geometry
field's origin, scales and coordinate system), rows through the .gdbtablx index (with its
map of present 1024-row blocks), null bits, every value type (int16/32, float32/64, text in
UTF-8 or UTF-16, dates as days since 1899-12-30, GUIDs, XML), and the compressed shape
buffers (varint counts, zig-zag deltas on the field grid, Z after XY; polygon rings grouped as
GDAL's organizePolygons; curves as their vertices). Checked against the reference reader on a GDAL-
written geodatabase of six layers (multi types, heights): each layer alone and all together
(the reference reader cannot select a file's last layer alone: its index reads every layer).

Done in iteration 51: **ERDAS Imagine rasters** (.img, GDAL's HFA driver). The file's data
dictionary is parsed (types of fields: counts, pointers, objects, inline objects, enums,
BASEDATA) and drives the decoding of the entry tree; every Eimg_Layer of the first one's size
is a band (u1/u2/u4 to f64, 64 x 64 or other tiles, plain or run-length compressed blocks,
invalid blocks filled with no-data), placed by Eprj_MapInfo as HFAGetGeoTransform does
(including arc-second units), with Eimg_NonInitializedValue as no-data and EPSG codes for
UTM/longitude-latitude on WGS 84. Checked against the GeoTIFFs GDAL copied them from (every
cell, placement, projection) and the reference reader's band sums, plus compressed byte, 1-bit and
4-bit layers against their formulas. The reference reader places .img files at the origin with zero
spacing (its GCP branch reads GDAL's empty GCP list) and leaves Elevation empty.

Done in iteration 52: **4-D Chombo**, as VisIt's Chombo reader shows it (single level):
boxes with the same (i, j) extent grouped into one 2-D box (VisIt's representative boxes),
each component an array with a value per velocity cell (k major, l minor, NaN where no box
covers it), `_vpar`/`_mu` velocity coordinates and `<component>_sum` (VisIt's array_sum
expression, skipping uncovered cells). The VisIt reader builds the same two 2-D
boxes (checked: origin, spacing, size) but no arrays; the values were checked against the
formulas of scripts/gen_amr_files.py's chombo4d.hdf5.

Known gaps worth doing next (P5): Tecplot SZL (no reference reader), more GDAL rasters
(NetCDF through GDAL, JPEG 2000...), remaining filter catalog (CTH material AMR filters, which need non-overlapping AMR data); P4 odds: TAA, comparative views, more widgets; P6: srun/MPI workers, ghost cells,
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

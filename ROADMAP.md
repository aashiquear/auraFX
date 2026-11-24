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

Legend: `[ ]` not started, `[~]` partly done (the done part is named on the line or in the
"Status" section at the end), `[x]` done. Sections 0 to 14 are the catalog; "Status" says what
works in the current release, what is open and what is known to be limited.

---

## 0. Foundations

### 0.1 Workspace and tooling
- [x] Cargo workspace with one crate per subsystem (see `docs/ARCHITECTURE.md`).
- [x] App icon and branding assets (`assets/icons`, `assets/branding`).
- [x] CI: build + test on Linux/macOS/Windows, clippy, rustfmt, licence check (on pull requests and on `main`).
- [~] Packaging: AppImage, tar.gz + `install.sh`, `.dmg`, Windows installer + zip, Android APK, unsigned iOS IPA (done); `.deb`, MSIX, Flatpak (todo).
- [x] Crash reporting and structured logs (`tracing`), log panel in the UI. (A local crash report file and a notice at the next start; nothing is sent.)
- [~] Settings system (TOML in platform config dir), settings UI with search.
- [x] Plugin ABI (dynamic Rust crates + Python plugins) and plugin manager UI. (Python plugins and the Plugins window; Rust plugins through a C table checked by ABI and data versions.)

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
- [~] Out-of-core/mmap-backed arrays for files larger than RAM. (Files streamed in pieces through one server process, `afx.load(path, stream=N)`; VTK XML grids and `.vtu` read only each piece's bytes through a memory map; other readers todo.)
- [x] Unit and physical-quantity metadata on arrays. (Units on every array, read from NetCDF, set with Set Units or `data.point_data.set_units`, shown in the legend, Information panel and spreadsheet; a label, no conversion.)

### 0.3 Pipeline engine (`aurafx-pipeline`)
- [x] Demand-driven DAG: sources -> filters -> representations, with dirty tracking and memoized outputs.
- [x] Multi-input filters: extra inputs per node, scheduled and invalidated like the main input, kept in undo and state files, cycles rejected.
- [~] Per-node progress and cancellation; background execution off the UI thread.
- [x] Time-aware execution: temporal filters read their input at any time (the upstream pipeline is re-evaluated on the worker thread), add or remap time steps, keep state between runs (particles advance incrementally) and skip re-running when their result does not depend on time.
- [x] Streaming/pieces for huge datasets; ghost-cell generation. (Pieces on the parallel server, the Ghost Cells and Redistribute DataSet filters; ghost layers between server processes and pieces streamed through one process.)
- [x] Undo/redo of every pipeline and property change.
- [~] State files (`.afx` JSON/TOML) and `.pvsm` state import (best effort).
- [~] Python trace: every UI action recorded as an equivalent `aurafx` Python call.

---

## 1. Readers (file formats)

Priority is driven by the stated use-cases (Exodus, VTU/VTK, LAMMPS) and then by breadth.
The readers are pure Rust, HDF5 and NetCDF-4 included (no C library is linked).

### 1.1 Tier 1 (ship first)
- [x] VTK legacy `.vtk` (ASCII and binary; all dataset types).
- [x] VTK XML: `.vtu`, `.vtp`, `.vti`, `.vtr`, `.vts`, `.vtt` (inline, appended, base64; raw; zlib and LZ4 block compression). `.vtm`, `.pvtu/.pvtp/...` parallel pieces and `.pvd` time collections.
- [x] LAMMPS dump (text; `ITEM: ATOMS` with arbitrary columns, triclinic boxes, multi-frame), plus `.dump.gz`.
- [~] LAMMPS data files, XYZ/extended XYZ, `.lammpstrj` (done); LAMMPS binary dump todo.
- [~] Exodus II (NetCDF-3 classic/64-bit offset and NetCDF-4/HDF5): nodes, blocks, side sets, node sets, nodal/element/global variables, time steps, displacement application, name maps.
- [~] CSV/TSV with delimiter detection, header sniffing, and "table to points/structured grid".
- [~] STL, OBJ, PLY, OFF, glTF/GLB.
- [x] PNG/JPEG/BMP/TIFF images and stacks as `ImageData` (multi-page TIFF and `img_*.png` wildcards as volumes).
- [~] VTKHDF (`.vtkhdf`: image data, unstructured grids, poly data, partitioned collections with assemblies, time steps, gzip, verified against the reference reader); HDF5 generic browser todo.

### 1.2 Tier 2 (simulation ecosystem)
- [x] OpenFOAM (`.foam` case, ASCII/binary, decomposed and reconstructed, time directories, boundary patches with boundary values; verified against the reference reader).
- [~] EnSight Gold (`.case`, ASCII and C binary, all element types, blocks, time sets), Tecplot ASCII and binary, PLOT3D, Xdmf2/3, CGNS (HDF5 and ADF), Fluent `.cas/.msh`, Nastran/BDF, LS-DYNA `d3plot`, Nek5000, openPMD (HDF5) (done); ANSYS `.inp`, Silo, MFIX, ADIOS2 BP4/BP5, Fides, Conduit/Catalyst todo.
- [~] NetCDF CF (classic and NetCDF-4, lon/lat on a sphere with bounds, time, packing, fill values); MPAS (vertical levels, multilayer and lon/lat map); CAM-SE and POP; GeoTIFF and Esri ASCII grid rasters, ERDAS Imagine .img; GIS vectors: Shapefile, GeoJSON, KML, GeoPackage, KMZ, FlatGeobuf, GML 2/3/3.2, MapInfo .tab/.mif and Esri file geodatabases; other GDAL rasters, SEG-Y todo.
- [~] Molecular: PDB and Gaussian Cube, XYZ molecules (done); VASP POSCAR/CONTCAR/CHGCAR/LOCPOT/ELFCAR; CML; OUTCAR todo.
- [~] Cosmology and AMR: Enzo/FLASH AMR, Chombo (cell- and node-centered, 1-D to 4-D), AMReX plotfiles, FLASH, Enzo and Chombo particles (done); Gadget, GenericIO, Cosmo todo.
- [x] Medical/imaging: DICOM, NRRD, MetaImage (`.mhd`), MRC, OME-TIFF; NIfTI.
- [x] Point clouds: LAS/LAZ, H5Part; PTS.
- [~] CAD: STEP, IGES, 3MF and `.sc` (read and written by the CAD environment, section 13); VRML 2.0, glTF, OFF; Alembic, USD, OpenVDB todo.
- [ ] Every remaining reader in the catalog, tracked per entry in a format matrix (`docs/reference/format-matrix.md`, not written yet).

### 1.3 Writers
- [~] VTK XML (all dataset types; parallel pieces `.pvtu/.pvtp/.pvti/.pvts/.pvtr` and `.pvd` collections), VTK legacy, VTKHDF (read by the reference reader).
- [~] Exodus II (done: element blocks, nodal/element variables, all time steps; verified with the reference reader), EnSight Gold and Xdmf 3, CGNS/HDF5, all verified with the reference reader; OpenVDB, GenericIO, Fides.
- [x] CSV, STL, PLY (done), OBJ, glTF, Houdini `.geo`.
- [~] Images: PNG, image stacks of volumes as PNG slices or one multi-page TIFF, MetaImage, screenshot with resolution override and transparent background, pictures of every view as laid out; JPEG, BMP and TIFF screenshots; EXR todo.
- [x] Animation: PNG sequence, MP4 (ffmpeg), GIF (done); WebM.
- [~] Scene export: glTF (.glb), OBJ with vertex colors, X3D, VRML and SVG vector pictures; an interactive web page; POV-Ray and PDF todo.
- [ ] Extracts and Catalyst-style in-situ scripts.

---

## 2. Filters (catalog)

The groups below follow the catalog; every filter of the catalog is listed, and a star marks the core ones. In the app the filters are in the menu categories Common, Data, Geometry, Grains, Mesh, Temporal, Annotation and Python (sources under Sources), and all of them by name under Filters › All filters. A `[~]` group has some of its filters; the guide's Filter reference lists every filter that exists.

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
- [~] Resample To Image ★, Resample With Dataset ★ (done, verified point by point against the reference implementation), Resample To Line, Resample AMR, Adaptive Resample To Image, Gaussian Resampling, Probe Location ★, Point Interpolator (Gaussian/Shepard/Voronoi/Linear kernels), Point Line/Plane/Volume/Dataset Interpolator, SPH Interpolator (+ Line/Plane/Volume/Dataset), Point Set To Octree Image, Octree Image to Point Set, Median, Equalizer Filter
- [~] Particle Tracer, Particle Path, Streak Line (done; exact on an analytic flow), Legacy Particle Path, Legacy Streak Line, In Situ Particle Path, Temporal Particles To Pathlines
- [~] Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator, Temporal Snap-to-TimeStep, Temporal Cache, Generate Time Steps, Force Time, Extract Time Steps (done); Temporal Smoothing, Merge Time, Group Time Steps, Synchronize Time, Critical Time, Date To Numeric, Time Step Progress Bar, Animate Modes
- [~] Hyper Tree Grid (type, .htg reader, both sources, To Unstructured Grid, Depth Limiter, Generate Fields, Evaluate Coarse, Cell Centers; .htg writer, To Dual Grid, masked random source; Geometry, Feature Edges; material interfaces in Geometry): Axis Reflection, Cell Centers, Depth Limiter, Evaluate Coarse, Feature Edges, Generate Fields, Geometry Filter, Ghost Cells Generator, Redistribute, To Dual Grid, To Unstructured Grid, Visible Leaves Size
- [~] AMR (foundations, readers, Gaussian pulse, level ids, crack-free Contour and Dual Grid, Resample AMR, Dual Clip, Fragment Integration, Material Interface done): Connectivity, Contour, Dual Clip, Fragment Integration, Fragments Filter, Material Interface Filter, Intersect Fragments, Youngs Material Interface
- [~] Cell Grid (Centers, Elevation, To Unstructured Grid, source; Transform, Warp, Convert To Cell Grid, .dg writer; Probe; Compute Sides): Centers, Compute Sides, Elevation, Probe, To Unstructured Grid, Transform, Warp
- [~] Molecules (Molecule representation, CML reader, Compute Molecule Bonds; molecules are polydata, so Molecule To Lines / Convert Into Molecule are not needed): Compute Molecule Bonds, Molecule To Lines, EAM Project To Sphere
- [~] Cosmology: ANL Halo Finder, ANL Subhalo Finder, LANL FOF/SOD Halo Finder (Halo Finder, friends of friends with SOD radius and mass; subhalos todo)
- [~] Parallel/distributed (Redistribute DataSet and Ghost Cells; ghost exchange between server processes; the rest todo): D3, Redistribute DataSet, Partition Balancer, Reduction Filter, Aggregate Dataset, Client Server Move Data, MPI Move Data, Pass Through, Force Static Mesh, Voronoi Merge Connected
- [~] Annotation (text, time label and ruler done): Annotate Attribute Data, Annotate Global Data, Annotate Selection, Annotate Time Filter, Environment Annotation, Python Annotation, Programmable Annotation, Text, 3D Text, Logo, Ruler, Protractor
- [~] Programmable: Programmable Filter ★ (Python), Programmable Source, Python Calculator ★ (done), Live Programmable Source, Color By Array, OME TIFF Channel Calculator, Threshold Table, Field Arrays From File

### 2.4 Sources
- [~] Geometric: Sphere ★, Cone ★, Cylinder ★, Box ★, Plane ★, Line ★, Disk, Ellipse, Arrow, Axes, Superquadric, Point Source, PolyLine Source, PolyPoint Source, Spline Source, Outline Source
- [~] Synthetic data: Wavelet ★, Mandelbrot, Fast Uniform Grid, Hierarchical Fractal, Hyper Tree Grid (+Random), AMR Gaussian Pulse, Data Object Generator, Unstructured Cell Types (done), Partitioned DataSet Collection Source, Cell Grid Source, Spatio Temporal Harmonics Uniform Grid, Time Source, Trivial Producer
- [ ] Selection sources (ID, global ID, pedigree ID, location, frustum, threshold, value, block, query, composite/hierarchical ID)

### 2.5 Filter infrastructure
- [~] Every filter has: typed properties, defaults, validation, documentation, search keywords, Python binding, undo entry, and progress reporting.
- [~] GPU implementations (wgpu compute) for: contour, clip, slice, threshold, gradient, resample, glyph, stream tracer, particle advection, connectivity, histogram. (Contour and slice of grids, the GPU finding the cells to cut and the CPU cutting them, so results are unchanged; a GPU histogram was slower than the CPU here and was left out.)
- [x] Custom filter authoring: Python (`@afx.filter`, done) and Rust plugins (the `aurafx-plugin` crate, shared libraries in the plugin folder).

---

## 3. Rendering and views

### 3.1 Render view (`aurafx-render`, wgpu)
- [x] Camera: perspective/orthographic, trackball/terrain/joystick styles, reset/fit, standard views (+X,-X,...), isometric, roll, zoom to box, zoom to data, camera undo/redo, saved camera links.
- [x] Surface rendering with flat/Gouraud/PBR shading; backface handling.
- [x] Scalar coloring with color maps (ColorBrewer, viridis family, the classic preset gallery, imported `.json/.xml`), range modes (auto, custom, time-wide), log scale, discrete, NaN/above/below colors, opacity transfer functions, per-component or magnitude, categorical.
- [~] Representations: Surface, Surface With Edges, Wireframe, Points, Outline, Feature Edges, Point Gaussian (splats: sphere, gaussian, custom shader), 3D Glyphs, Volume, Slice (image), Slice (unstructured), Multi-slice, Orthographic slice, Molecule (ball/stick/VDW), Hyper Tree Grid, Cell Grid, Polar axes, Data labels, Selection highlight, Texture. (3D Glyphs, instanced.)
- [~] Volume rendering: GPU ray casting, smart/fixed-point, 2D transfer functions, shade, gradient opacity, multi-component, blend modes (composite, MIP, MinIP, additive, isosurface), cropping, clipping planes, unstructured-grid projected tetra / ray casting.
- [x] Lighting: light kit, custom lights (camera-relative or fixed, colored), shadows, screen-space ambient occlusion, Eye-Dome Lighting, ACES/Reinhard tone mapping and exposure, MSAA + FXAA, order-independent transparency. Light widgets. Not done: TAA.
- [x] Surface LIC and ray tracing (Rust path tracer); environment maps as light and background, materials (metallic, roughness, specular) in Properties, GPU path tracing.
- [~] Axes grid (data axes grid with ticks and labels, orientation axes, scalar bars and text done; array units in the scalar bar; custom bounds and axis units todo): Axes grid (data, custom bounds, labels, units), orientation axes, center-of-rotation marker, scalar bars (position, titles, number format, custom labels), legends, logos, text, interactive 2D annotations.
- [~] Widgets (implicit plane, box, sphere, line, point, light and ruler done): implicit plane, box, sphere, cylinder, cone, annulus, frustum, line, polyline, spline, handle/point, distance, protractor, ruler, slider, light, coordinate frame, four-lines, contour, display-sized plane, text, logo, equalizer, progress bar.
- [~] Selection: by click/rubber band/polygon on points/cells/blocks, through-frustum, by query (`field > value`, nearest, etc.), grow/shrink, invert, hover tooltips and interactive labels, "Find Data" panel.
- [~] Picking tools: point probe, coordinate readout, interactive cell/point info.
- [ ] Stereo (anaglyph, crystal eyes, interlaced, side-by-side), VR (OpenXR) later.
- [~] Huge data: LOD, frustum/occlusion culling, instanced glyphs, GPU-resident meshes, mesh streaming, remote render with image streaming. (GPU-resident meshes and server pictures done; frustum culling by runs of triangles, a coarse copy drawn while the camera moves, instanced 3D Glyphs; occlusion culling and mesh streaming todo.)

### 3.2 Other views
- [~] Line chart ★ (multiple series, log axes, styles, legends, markers, selection link), Bar chart, Point chart, XY bar chart, Histogram view, Box chart / Quartile chart, Plot matrix (scatter matrix), Parallel coordinates, Image chart, Comparative views (render/line chart), Slice view, Orthographic slice view, Spreadsheet view ★ (point/cell/field/row data, sort, filter, column visibility, precision, export CSV), Python view (matplotlib embedded), Spreadsheet of selection.
- [x] Several chart views at once, each plotting its own pipeline object (or following the selection), saved with the state and opened from menus, the Pipeline and Python; pictures and animations of every open view (3D views and charts) as laid out.
- [~] Layouts (split views, linked cameras, match view direction done): split horizontally/vertically, tabs, link cameras, link views, link selections, maximize, save/restore layouts, multiple windows/monitors.

### 3.3 Animation and time
- [~] Time toolbar (play, step, loop, time value entry, snap to steps, real-time, sequence modes), time inspector.
- [~] Animation view (camera keyframes, numeric filter and display property tracks, linear/step/ease/exponential/wave interpolation per key, Python cue, time scrubbing and preview done; comparative cues todo): keyframes for any property (ramp, linear, exponential, sinusoid, boolean), camera paths (orbit, path, interpolate), Python cue, comparative cues.
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
- [x] Keyboard-first: fully rebindable shortcuts, vim-style view navigation optional. (Settings › Keyboard, `afx.set_shortcut`.)
- [x] Themes: dark/light/high-contrast, scalable UI (HiDPI), font size. 
- [x] Session recovery after crash, autosave of state. (Last session restored on start; saved every half minute, offered after a crash.)
- [x] File › New (asks about unsaved work), the system clipboard in every text box, view buttons (fit, zoom, pan) in the 3D view, an All filters menu, filters suggested for the selected data, a Welcome window; Help › User guide opens the installed guide or the published one (always the published one on a tablet or a phone), and About links to the project page and the repository.
- [x] Touch interface for tablets and phones: Desktop, Tablet and Phone interfaces chosen by the device, safe areas, touch-sized controls, two-finger pan, pinch and twist, layouts that follow the screen's turning.
- [x] Accessibility: screen-reader labels, keyboard navigation, color-blind color maps by default. (AccessKit, a high-contrast theme, Viridis as the default color map.)
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
- [x] Jupyter kernel exposure (drive auraFX from a notebook) and notebook panel inside the app. (`aurafx --install-jupyter-kernel`, the kernel drives the window; the Notebook panel opens and saves `.ipynb`; `afx.snapshot`, `afx.display`.)
- [x] Custom Python plugins (reader, filter, source done; panels, `@afx.panel`) with hot-reload.

---

## 6. AI assistant (`aurafx-ai`)

- [x] Provider abstraction: Anthropic API, OpenAI-compatible API, Ollama (local and cloud models), with streaming.
- [~] Subscription/OAuth sign-in for hosted providers; API keys stored in the OS keyring.
- [x] Chat panel with Markdown rendering, code blocks with Run code, Copy code and Copy to console, a copy button per message, saved conversations (new, history, delete); models that reason before they answer are handled.
- [~] Context awareness: the model sees pipeline state, selected dataset summaries (arrays, ranges, bounds, time steps), camera, recent errors and logs, without uploading raw data unless asked.
- [~] Tools the model can call (agent mode acts through generated Python): load file, add filter, set property, color by, take screenshot (and view it), run Python, read a chart's data, search docs, run a simulation job.
- [~] Modes: "Explain", "Do it for me" (agentic with confirmation per step), "Write a script", "Debug this error".
- [x] Natural-language filter creation (Filters › Describe a filter…) ("show me regions where von Mises stress > 300 MPa").
- [x] Anomaly and feature detection helpers (outliers in fields, mesh quality problems, time-series change points). (Find Outliers, Find Change Points; mesh problems by Mesh Quality and Validate Cells.)
- [x] Prompt templates and user-defined skills; privacy controls (local-only mode and path redaction; saved prompts and skills).
- [x] Vision: send rendered frames to multimodal models for interpretation (the camera box next to Send, `afx.ask(..., view=True)`; Ollama, Anthropic and OpenAI formats).

---

## 7. Simulation runner (`aurafx-jobs`)

- [~] Job model: command, working dir, environment, inputs, outputs, resources (CPU, GPU count/vendor, memory), dependencies between jobs, templates per solver.
- [~] Local executor with live stdout/stderr streaming, log search, kill/pause, resource monitoring (CPU, GPU via NVML/ROCm-SMI).
- [~] Templates: LAMMPS, MOOSE, OpenFOAM, custom shell/Python, and cases written from the pipeline's data (MOOSE polycrystal, SPPARKS, PINNACLE, JAX-FEM polycrystal, DAMASK, FFT texture file, LAMMPS polycrystal) (done); Abaqus/CalculiX, GROMACS todo.
- [~] Python pipeline runs: define a pipeline in Python (pre -> run -> post), run as a job, auto-load outputs when done.
- [x] Parameter sweeps and ensembles with a results table and comparative views.
- [~] Remote executor over SSH: connect (key, agent, password, jump hosts, 2FA prompts), remote file browser, sync inputs/outputs (rsync-style), run under nohup/tmux, Slurm/PBS/LSF submission and queue monitoring, reconnect to running jobs. (Slurm, PBS and LSF are tested with stand-in commands only, not on a real cluster.)
- [x] Remote rendering/data server mode (set up in one guided window, Remote › Open data on another machine…): run `aurafx-server` on the GPU box, stream geometry or images to the local client.
- [x] Watch folders: auto-refresh when the solver writes a new step.

---

## 8. GPU compute (`aurafx-gpu`)

- [x] wgpu device management: adapter choice (AMD, NVIDIA, Intel, software), features, limits, memory stats in UI.
- [~] Compute kernels in WGSL for filters listed in 2.5, array math (calculator JIT to WGSL), reductions, histograms, sorting. (The `aurafx-gpu` crate, cell-candidate kernels for values and planes.)
- [ ] Optional vendor backends behind features: `cuda` (cudarc) and `rocm` (HIP) for kernels where WGSL is too limited (fp64, large shared memory), and for interop with PyTorch tensors.
- [x] GPU memory budgeting, fallbacks to CPU (rayon), benchmark panel. (Slabs sized to the device's buffer limits, CPU fallback on any GPU error or without a GPU, Settings › Benchmark and `afx.gpu_benchmark`.)

---

## 9. Distributed and remote

- [~] Client/server split (data server and server-side rendering done: `aurafx-server` over SSH stdio, server-side pipeline with caching, surface/summary delivery, GPU-drawn color+depth images composited by depth; MPI and Slurm launchers): `aurafx-server` (data + render) over TCP/SSH tunnel with the same protocol as local.
- [~] Multi-process data parallel (all-core shared-memory execution done; multi-process parallel servers done: workers per piece, piece-wise reading of partitioned files, sort-last compositing; MPI and Slurm launchers; ghost layers between the server's processes, so Gradient, Cell Data to Point Data, Surface Normals, Curvature and Compute Derivatives run on every piece) (MPI optional; Rust-native work distribution first), ghost cells, D3 redistribution, compositing (sort-last), tile displays.
- [ ] Collaboration: shared sessions with multiple clients.
- [ ] In-situ: Catalyst/Conduit endpoint, live visualization of running jobs.

---

## 10. Quality, docs, community

- [~] Golden-image render tests, filter regression tests against VTK outputs, reader conformance suite with sample files for every format.
- [x] Benchmarks (load time, filter time, frame rate) published in docs. (The guide's Benchmarks page, written by an ignored release-mode test.)
- [~] User guide (done: Markdown, HTML book and PDF, a picture per feature, a Sample pipelines chapter that goes through every clip of the project page step by step in the window and in Python), tutorials and examples; filter reference auto-generated from property metadata (kept current by a test), Python API reference generated from the module's docstrings (kept current by a test).
- [~] Contributing guide (done), code of conduct, issue templates, plugin cookbook (done: `docs/plugins.md`).

---

## 11. Grain workbench (microstructure analysis and building)

EBSD and synthetic microstructures, grain segmentation and statistics, texture, boundaries
and mechanics as filters, sources, readers, writers and charts, for FCC, BCC and HCP crystals. Design and stage details: `docs/design/grains-and-meshing.md`.

- [x] G1 `aurafx-crystal` (orientation conversions, the 11 Laue groups, disorientation, fundamental zones, IPF/Rodrigues/Euler colours); readers for `.ang`, `.ctf`, `.h5ebsd`, EDAX/Bruker `.h5`, `.dream3d`; Orientation Colors, IPF Color Key, Convert Orientations, Rotate Orientations, Crop, Change Resolution, Align Sections.
  - [x] G1a: `aurafx-crystal`, `.ang` (square grids) and `.ctf` readers, Orientation Colors, IPF Color Key, Convert Orientations, Rotate Orientations; 8-bit RGB arrays drawn as their own colors.
  - [x] G1b: `.h5ebsd`, EDAX/Bruker `.h5`, `.dream3d` readers; Crop, Change Resolution, Align Sections.
- [x] G2 Features: Segment Features (misorientation, scalar, C-axis), Find Sizes/Neighbors/Shapes/Centroids/Boundary Cells/Surface Features/Average Orientations/Misorientations/Kernel Average Misorientation/Reference Misorientations/Euclidean Distance Map, Min Size, Fill Bad Data, Merge Twins, Find Twin Boundaries; Features and Ensembles tables.
- [x] G3 Texture: pole figures, inverse pole figures, ODF sections and texture index, MDF, texture fractions, chord lengths (charts, pictures, CSV).
  - [x] G3a: pole figures (stereographic, equal area), inverse pole figures, ODF sections, texture index, CSV of multi-panel output.
  - [x] G3b: misorientation distribution, texture component fractions, chord lengths.
- [x] G4 Synthetic builder: ensemble statistics (size, shape, neighbour, ODF, MDF, axis ODF), Initialize Volume, Pack Primary Phases, Insert Precipitates, Matrix Phase, Match Crystallography, orientation noise/jumble; Voronoi/Laguerre tessellation and columnar AM grains as sources; Generate Ensemble Statistics from data.
  - [x] G4a: Voronoi/Laguerre grains, columnar AM grains, Add Orientation Noise, Jumble Orientations, Generate Ensemble Statistics.
  - [x] G4b: Pack Grains (goal statistics, four shapes, matrix phase, calibration), Pack Like, Insert Precipitates, Match Crystallography.
  - [x] G4c: Orientation Gradient (a turn along the build direction, a smooth spread and sub-grain cells inside each grain) and Grain Realism (boundary relaxation, coarsening, roughness; competitive growth, layers and fine grains for columnar grains), as filters and as settings of the synthetic sources; the gradient also in plane (every grain turned across itself).
  - [x] G4d: Synthetic Microstructure, one entry of the Sources menu whose window lists every synthetic type with its standard settings: the three grain sources and 35 morphological models (grains by nucleation and growth, dead leaves; overlapping, penetrable, hard and clustered spheres; overlapping, endless, curved, hard, bending and cross-linked fibres, Delaunay networks, laminates; Gaussian fields, transparent leaves, reconstruction from a two-point statistic; triply periodic surfaces, strut lattices, disordered and hierarchical lattices, fractal sponges, open-cell and level-set foams, scale-rich networks; cellular automata, activator-inhibitor and reaction-diffusion patterns, aggregation, pipe networks, branching trees; nacre, twisted plywood, honeycombs and wood, Haversian bone), with the filters Coat Phase and Combine Microstructures.
  - [x] G4e: Measuring and simulating microstructures (Filters › Microstructure, on any image of phases): Phase Measures, Two-Point Statistics, Phase Chord Lengths, Local Thickness, Tortuosity, Fabric Tensor, Percolation, Representative Volume, Unfold Section Sizes, Split Touching Particles, Stretch Microstructure, Reconstruct Microstructure; Microstructure Objects, Smooth Surface, Framework Rigidity; Effective Conductivity, Effective Stiffness, Wave Band Structure; the sources Optimized Unit Cell, Polarized Blocks, Microstructure Template (20 starting points, also the Start from list of the window), Microstructure Formation (time steps) and Microstructure Study (percolation thresholds, integral range); model variants: packing by collisions, stealthy seeds, planar Delaunay networks, automata on hexagons, triangles, two tilings that never repeat and two lattice cells, grain growth, polarized and graded scale-rich networks, pyrochlore lattice, rays of wood as cells, foam struts measured one by one.
  - [x] G4f: The Sources menu in groups, each one entry whose window lists its types with their standard settings: Geometric Shape (sphere, box, cylinder, cone, plane, line, point cloud), Solid Model (Geometry Commands, CAD Model), Mesh (box, plate, dog-bone, inclusion and face meshes), Sample Data (wavelet, cell types, harmonics in time, AMR and hyper tree grids) and Synthetic Microstructure; IPF Color Key and Programmable Source stand alone. In Synthetic Microstructure, Start from belongs to the chosen type: its standard settings, then the starting points of that type only (50 starting points over 35 types).
- [x] G5 Surface meshing and GBCD: Quick Surface Mesh, Laplacian/Taubin smoothing with node types, triangle-geometry filters, STL per feature, Find GBCD, face IPF and misorientation colouring.
- [x] G6 Mechanics and AM: Schmid factors for FCC/BCC/HCP slip systems, Taylor factor, directional modulus, yield estimate, SROD by build height, Nye and KAM GND, layer plots; materials table.
- [x] G7 Files and runs: `.dream3d` writer with pipeline JSON, `.tex`, SPPARKS sites, `.dream3d` pipeline JSON import; MOOSE polycrystal, SPPARKS and PINNACLE job templates with the pipeline's files filled in; results reopened for analysis.

## 12. Geometry and mesh generator

Implicit geometry, structured hex meshing and an overlay-grid mesher for
arbitrary and voxel geometry, with a command language, quality tables and sets. Design: `docs/design/grains-and-meshing.md`.

- [x] M1 Geometry kernel: implicit primitives, transforms, CSG, 2-D outlines extruded/revolved/swept, STL and existing surfaces as signed distance fields, named faces; the Geometry source and the `afx.mesh` command language (journals through the trace).
- [x] M2 Structured hex: box, mapped blocks, cube with packed inclusions (cubed-sphere O-grid macro-blocks, transfinite cores, graded shells, weld), plate with hole, dog-bone, 2-D sweep, Elevate to HEX20/27, blocks/side sets/node sets, periodic pairs; Mesh Quality with the full metric set and acceptable ranges; Exodus, VTK, Abaqus `.inp`, Gmsh `.msh` writers.
  - [x] M2a: hex mesh model (blocks, side sets, node sets, element variables), box and mapped blocks with grading, the inclusion mesher (grid packing, cubed-sphere O-grid blocks, transfinite cores, graded shells, exact weld; spheres, ellipsoids, superquadrics, harmonic grains), HEX20/HEX27 with surface projection, periodic pairs by coordinates, Extract Set, Mesh Quality with the full hexahedron metric set and a summary table, Exodus sets and HEX20/27, Abaqus `.inp`, Gmsh `.msh`.
  - [x] M2b: plate with hole, dog-bone, 2-D quad sweep, collective packing, mesh commands (`mesh body`, `block`, `sideset`, `nodeset`) in the geometry language for bricks and cylinders, the benchmark's meshes as references.
- [x] M3 Overlay-grid mesher: volume fractions from geometry, STL or voxel images, interface reconstruction, projection, pillowing, tiered smoothing, defeaturing, void meshing, generated side sets, periodic RVE sets, stair-step fallback.
  - [x] M3a: overlay grid from geometry bodies, closed surfaces and labelled images, defeaturing and pinch removal, pillowing, projection along stair-step normals, guarded tiered smoothing and untangling, void meshing, domain/surface/interface side sets, periodic pairs, stair-step mode, `scheme overlay` in the geometry commands, Overlay Mesh filter.
  - [x] M3b: sharp edges and corners kept by the overlay mesher (patches between creases for surfaces), saddle-free clean-up for fine microstructures, parallel smoothing.
- [x] M4 Refinement and tets: conformal and local refinement, smoothing schemes, Delaunay tri/tet meshing with sizing, THex/HTet, boundary layers.
  - [x] M4a: uniform refinement of hexahedra and tetrahedra, HTet/THex conversions, mesh smoothing (Laplacian, smart Laplacian, condition number), boundary layers, tetrahedron quality metrics.
  - [x] M4b: tetrahedral meshing of implicit and STL geometry (isosurface stuffing), local refinement of tetrahedra, Tet mesh output and filters.
  - [ ] Later: local refinement of hexahedra with transition templates, sharp edges in tetrahedral meshes, conforming tetrahedra between touching bodies. (Winslow smoothing and sizing by curvature are done, section 14.3.)
- [x] M5 Grains on meshes: Assign Grains (Voronoi, Laguerre, nearest voxel, from a volume), per-grain blocks and boundary side sets, Mesh Grains (M3 on FeatureIds); the worked geometry → mesh → grains → solver → statistics example.

---


## 13. CAD modeling environment (P10)

A parametric CAD modeler inside the one window, beside the command-language modeler of
section 12: constrained sketches, a feature timeline that rebuilds on a parameter change,
fillets, shells, holes, threads, patterns, assemblies, STEP/IGES/3MF in and out, and an apply
step that hands the geometry back to the pipeline after checking everything downstream.
Design: `docs/design/cad-environment.md`. The crate is `aurafx-cad`; the environment is
`aurafx-app/src/cad/`. The B-rep kernel and the CAD file formats are built on the SolidCraft and
solidcore code carried in `vendor/` (`vendor/README.md`, `vendor/UPSTREAM.toml`).

- [x] Document (`aurafx-cad`): features with stable ids, shapes, modifiers, parameters with unit-aware expressions, bindings, configurations, kept sketches, reference planes, components and mates; constrained sketches with a damped least-squares solver and a degrees-of-freedom report; six starter templates; mass properties, sections, interference; `.afxcad` JSON files with a format version.
- [x] Implicit kernel: every shape and modifier as a signed distance, a surface with stable face ids and names (`Base z+`, `Bore side`), never fails on a combination.
- [x] B-rep kernel (always built, in process): primitives, extruded and revolved sketches, holes, patterns, fillets and chamfers (whole feature, whole part or picked edges), shells with picked open faces, drafts, mirrors, lofts, sweeps along a flat path, combined parts; faces named as the implicit kernel names them; what it cannot build falls back to the implicit kernel with a note (`kernel = Auto`).
- [x] The CAD Model source: the document is a parameter of the node (state files, undo and the trace carry it), the document's dimensions are node parameters (`node.set(width=50)`), output as a surface or a distance field.
- [x] The environment: a mode of the one window (ribbon with Sketch, Features, Modify, Assemble, Inspect and Files tabs, model tree, feature panel, timeline bar with rollback, suppress and reorder); the pipeline pauses and other views are marked stale; Apply, Apply and keep editing, Discard, Save as; one undo step per apply.
- [x] Sketching: the canvas in the 3D view (grid, snapping with guide lines, line, rectangle, circle, arc, spline and point tools, 13 constraints, 7 dimensions typed as expressions, status colours and degrees of freedom), sketches on principal planes, on any flat face and on reference planes; sketches that follow their face; the part shown under the sketch; edges of the part taken into a sketch; planes and faces previewed as filled rectangles while they are chosen.
- [x] Downstream check: what the nodes below refer to (face and body names and numbers, set names, positions, command text) compared with the rebuilt part; conflicts (removed, renamed, moved, ambiguous, count) with default fixes and an assistant's suggestion; applied with the part as one undo step; re-run now or later.
- [x] Files: STEP (AP242, AP214, AP203), IGES, 3MF and `.sc` read as the base feature of a part and written; STL and OBJ in and out; a faceted STEP for parts only the implicit kernel builds; a Geometry source or any closed surface taken in as imported bodies (Edit in CAD).
- [x] Edge and face work: picking faces and edges in the 3D view, patterns on a face, mirror about a face, draft, combine, edges with their faces and lengths, mass properties, distance and angle measures.
- [x] Assemblies: components with placements, a component edited on its own, mates (coincident, concentric, distance, angle, fixed) solved into placements and moved by a value, a fit check (interference).
- [x] Meshing from CAD: face names as side sets in Overlay Mesh and Tet Mesh, `import 'part.step'` and `import 'part.afxcad'` in the geometry commands, the STEP → mesh → grains → solver case example.
- [x] Finding one's way: a Next strip that follows the work, a command search at the pointer, a right-click menu on faces, reasons on greyed buttons, the failing feature marked in the tree, a templates gallery, keyboard shortcuts, the assistant modelling through the same operations (`afx.cad`).
- [x] Python: `afx.cad_model()`, `afx.cad_from()`, `node.edit_cad()`, `node.cad.*` (solids, sketches with constraints and dimensions, features, modifiers, components, mates, timeline, configurations, measures, save and load), `afx.cad_check()`.
- [ ] Later: closed sweep paths, rails and changing sections; lofts between sections that are not parallel; mate limits, gears and collision while dragging; a component edited in place with the rest of the assembly faded; axis and edge picks for mates; faces recognised on a part started from triangles.

## 14. Additions by priority (P11)

Items chosen from the catalog of section 2 and from sections 11 to 13 because they matter most
to auraFX's users. Each row says how it is checked. Priority: **P1** first, **P2** after,
**P3** when asked.

### 14.1 Visualization and analysis

| Pri | State | Item | Check |
|---|---|---|---|
| P1 | done | **Delaunay 2D and 3D**, **Decimate** (quadric), **Subdivide** (Loop, linear), **Curvature** (Gaussian, mean), **Linear and Rotational Extrusion**, **Ribbon**, **Reverse Sense** | areas and volumes of a sphere sample against closed forms; Decimate keeps volume within 2 % at 50 % |
| P1 | done | **Iso Volume**, **Extract Cells By Region**, **Extract Cells By Type**, **Clip Closed Surface** (capped clip) | a capped clip of a cube is watertight and encloses the half cube |
| P1 | done | **Point interpolation** (Gaussian, Shepard, linear, Voronoi and SPH kernels) onto volumes, planes, lines and data sets, **Resample To Line**, **Plot On Sorted Lines**, **Plot On Intersection Curves** | the interpolated field of a known polynomial within tolerance |
| P1 | done | **Table To Points / To Structured Grid**, **Transpose Table**, **Table FFT**, **Merge Vector Components**, **Point And Cell Ids**, **Generate Global Ids**, **Coordinates** | round trips |
| P1 | done | **Tensor Glyph**, **Tensor Principal Invariants**, **Yield Criteria**, **Compute Derivatives** (gradient, strain rate, vorticity, divergence, Q and lambda2 for vortex cores), **Surface Vectors**, **Surface Flow** | invariants against NumPy on a random tensor field; the vorticity of a rigid rotation |
| P1 | done | **Evenly Spaced Streamlines 2D**, particle paths and streak lines with seeds released over time, **Temporal Cache**, **Force Time**, **Extract Time Steps**, **Generate Time Steps**, **Temporal Snap to Time Step** | known analytic flows |
| P2 | done | **Statistics**: K-Means, Principal Component Analysis, Multicorrelative, Contingency, Compute Quartiles (with a box plot chart style), Random Attributes | against NumPy and SciPy on a fixed sample |
| P2 | done | **Annotation** filters (time, global data, attribute data, selection), **Environment Annotation**, **Python Annotation** | screenshot tests |
| P2 | done | **Ghost Cells**, **Remove Ghost Information**, **Redistribute DataSet**, `.pvtu` pieces as partitions, **Overlapping Cells Detector**, **Validate Cells** | the parallel server tests |
| P2 | done | **Material Interface** reconstruction for volume-fraction fields, **AMR Dual Clip**, **Fragment Integration** | fragment volumes of a known sphere |
| P3 | done | **Halo Finder** (friends of friends with SOD radius and mass), SPH cubic and Wendland kernels, **Minkowski Functionals**, **Kernel Smoothing**, **Density Map** | three clumps found with their counts; a ball's volume, area and mean curvature within 1, 2 and 3 %; densities integrate to one |
| P1, P2 | done but two | Readers: DICOM, NRRD, MetaImage, MRC, OME-TIFF, LAS/LAZ, STEP/IGES, glTF, VRML, OFF, LS-DYNA d3plot, Fluent, Nastran, openPMD (HDF5), H5Part, Nek5000; writers: image stacks of volumes, MetaImage, parallel VTK pieces, `.pvd` collections. **Open: Silo and ADIOS2 readers.** | against the reference reader where it reads the format (`docs/reference/verification.md`) |

### 14.2 Microstructure

| Pri | State | Item | Check |
|---|---|---|---|
| P1 | done | **Rotate Sample Reference Frame**, **Apply Transformation to Geometry**, **Identify Sample**, **Isolate Largest Feature**, **Erode Dilate Bad Data**, **Erode Dilate Coordination Number**, **Replace Element Attributes with Neighbor**, **Neighbor Orientation Correlation**, **Bad Data Neighbor Orientation Check**; mapped by File › Import Pipeline | a seeded noisy map cleans to the known grains; the AM Bench pipeline file maps 40 of its 71 steps (the rest read and write files, keep arrays in order or align two data sets) |
| P1 | done | hexagonal-grid `.ang` files read on a square grid, **Resample Rectilinear To Image** | a `.ang` on a hexagonal grid against a brute-force nearest point |
| P1 | done | **Find Neighborhoods**, **Find Largest Cross Sections**, **Find Feature Clustering**, **Find Boundary Strengths**, **Find Twin Boundary Schmid Factors**, **Find C-Axis Locations**, **Find Feature Reference C-Axis Misorientations**, **Identify Micro-Texture Regions** | analytic cases (two grains with a known misorientation) and a reference implementation where one has the metric |
| P1 | done | Solver inputs from a microstructure volume: DAMASK (grid, material and load files), an FFT texture file, LAMMPS atoms of a polycrystal | DAMASK's own Python package loads the case; atom count, minimum distance and neighbour directions for LAMMPS |
| P2 | done | **Insert Transformation Phases**, **Adjust Volume**, **Generate Precipitate Statistics**, **Find Surface Area to Volume**, **Find Triangle Angles**, **Find Feature Face Curvature**, `.nodes` and `.triangles` files | closed forms (`docs/reference/verification.md`) |
| P2 | done | The **Goal Statistics** panel: size and shape distributions edited with live plots over the packed volume's, orientations as a choice; boundary misorientations are matched by Match Crystallography | the packed volume's statistics match the panel's |
| P2 | done | Image filters on volumes: **Median Image**, **Gaussian Smooth Image**, **Otsu Threshold**, **Binary Morphology**; **Point Sample Triangle Geometry**, **Crop Points**, **Iterative Closest Point**, **Set Origin and Spacing**, **Copy Feature Array to Cells** | against scipy.ndimage on a small volume |
| P3 | done but one | Pole figure pictures per phase (`save_pole_figures`), the Nye tensor on 2-D maps. **Open: indexing EBSD patterns** (needs pattern files and a reference indexer) | |

### 14.3 Geometry and meshing

| Pri | State | Item | Check |
|---|---|---|---|
| P1 | done | **Webcut** (plane, cylinder, body, sheet, swept curve), **imprint** and **merge**, interface side and node sets between bodies | two touching bricks meshed with one shared face; the volumes of the pieces |
| P1 | done | **Face Mesh** (triangles or all quads on planar faces with holes), **Remesh Surface** (a size, a size array, curvature), **Sweep Quads** into hexahedra | a plate with a hole meshed and swept: quality acceptable, element counts as expected |
| P1 | done but one | Sizing: intervals and bias per axis, sizes near curves, from a point array and from curvature; **Winslow smoothing**. **Open: transition templates for local refinement of hexahedra** (hexahedra are refined everywhere, or a region becomes tetrahedra through pyramids) | a graded box's element counts; a disturbed grid smoothed back |
| P1 | done | **Nastran** and **Fluent** mesh writers; a block per body for parts brought in with `import` | read back by auraFX's readers |
| P2 | partly | `thicken`, `loft` and `sew` with a tolerance in the geometry commands. **Open: fillets and chamfers on picked curves and healing of implicit bodies** (the B-rep kernel of section 13 has fillets, chamfers and lofts on picked edges) | |
| P2 | done | **Tetrahedra in a Region** (pyramid transitions), **Define Mesh Set** (by location or expression), groups in the geometry commands, **Merge Nodes**, Transform keeping blocks and sets | |
| P3 | partly | `scheme mapped` for bodies a box bends into. **Open: sweeps with several sources or targets, submapping** | |

## Delivery phases

| Phase | State | Contents |
|---|---|---|
| P0 Skeleton | done | Workspace, data model, legacy VTK + VTU/VTP + LAMMPS readers, wgpu render view, docking UI, command palette, Python console, AI chat (Ollama/Anthropic/OpenAI) |
| P1 Daily driver | mostly done | Exodus, PVD/time series, core ★ filters, color maps + scalar bar, spreadsheet + line chart, screenshots, state files, settings |
| P2 Analysis | mostly done | Statistics/data-analysis filters, selection + Find Data, Python API breadth, Python trace, programmable filter, charts breadth |
| P3 Simulation | mostly done (Slurm, PBS and LSF not tried on a real cluster) | Job runner local + SSH, templates, sweeps, watch folders, remote file browser, GPU monitoring |
| P4 Rendering depth | mostly done | Volume rendering, widgets, lighting/SSAO/EDL, animation + keyframes, exporters, LIC, ray tracing |
| P5 Breadth | in progress | Remaining readers/writers/filters from the catalog, AMR/HTG/molecule/cell-grid types, higher-order cells |
| P6 Scale | in progress (data server, parallel server processes with MPI and scheduler launchers, ghost layers, files streamed in pieces; collaboration and in-situ open) | Client/server, distributed, in-situ, collaboration, out-of-core |
| P7 Polish | mostly done (packaging, Python and Rust plugins, accessibility, touch interface; localization and three package formats open) | Plugins, packaging on all OSes, accessibility, localization, docs and benchmarks |
| P8 Grains | done (section 11) | Crystal core, EBSD readers, features, texture, synthetic builder, surface meshing, mechanics, `.dream3d`, solver templates |
| P9 Mesh | done (section 12) | Geometry kernel and command language, structured hex, overlay-grid mesher, refinement and tets, grains on meshes |
| P10 CAD | done (section 13) | The parametric CAD modeler: the document and two kernels, the environment with its ribbon, apply with the downstream check and fixes, STEP/IGES/3MF, meshing from CAD |
| P11 Additions | mostly done (the open rows of section 14) | Filters and readers, microstructure clean-up and metrics, solver inputs, webcut/imprint/merge, face meshing and sizing |

---

## Status

The state of the repository at release 0.9.2. The catalog above says what is planned; this
section says what works, what is open and what is known to be limited. How each reader, writer,
filter and mesher was checked is in `docs/reference/verification.md`.

### What works

**Foundations (section 0).** A Cargo workspace of 18 crates. Typed arrays with units, attribute
sets, image, rectilinear, structured, polygonal and unstructured data, tables, multi-block data,
overlapping AMR, hyper tree grids, cell grids, molecules, time series; every linear cell type,
quadratic and serendipity cells, Lagrange and Bezier cells of any order drawn curved. A
demand-driven pipeline with multi-input and time-aware filters, background execution, undo and
redo, `.afx` state files, autosave with recovery after a crash, a Python trace of what was done
by hand, and reader options per file. Settings, structured logs with a Log panel, a local crash
report. CI on Linux, macOS and Windows (format, licences, build, tests, clippy). Packages:
Linux tar.gz and AppImage (x86_64, aarch64), macOS `.dmg` (Apple silicon, Intel), Windows
installer and zip, Android APK and Android Studio project, unsigned iOS `.ipa` and Xcode
project; desktop packages bundle Python 3.12 with NumPy.

**Readers and writers (section 1).** Readers: VTK legacy and XML (serial, parallel, multi-block,
`.pvd`), VTKHDF, Exodus II (NetCDF-3 and NetCDF-4), OpenFOAM, EnSight Gold, CGNS (HDF5 and ADF),
Xdmf 2/3, Tecplot ASCII and binary, PLOT3D, LS-DYNA d3plot, Fluent, Nastran, Nek5000, openPMD
(HDF5), LAMMPS dumps and data files, XYZ, PDB, Gaussian cube, VASP, CML, NetCDF CF, MPAS, CAM-SE,
POP, GeoTIFF, Esri ASCII grids, ERDAS Imagine, Shapefile, GeoJSON, KML/KMZ, GeoPackage,
FlatGeobuf, GML, MapInfo, Esri file geodatabases, AMReX, FLASH, Enzo, Chombo (1-D to 4-D) and
VTK AMR with particles, hyper tree grids (`.htg`), cell grids (`.dg`), images and image stacks,
DICOM, NRRD, MetaImage, MRC, OME-TIFF, NIfTI, LAS/LAZ, H5Part, PTS, STL, PLY, OBJ, OFF, glTF,
VRML, CSV, EBSD (`.ang`, `.ctf`, `.h5ebsd`, EDAX and Bruker `.h5`), `.dream3d`, SPPARKS, and the
CAD formats of section 13. Writers: VTK XML (with parallel pieces and `.pvd`), VTK legacy,
VTKHDF, Exodus II, EnSight Gold, Xdmf 3, CGNS, `.htg`, `.dg`, Abaqus `.inp`, Gmsh `.msh`,
Nastran, Fluent, `.dream3d`, `.tex`, SPPARKS sites, CSV, STL, PLY, OBJ, glTF, Houdini `.geo`,
image stacks and MetaImage; screenshots as PNG, JPEG, BMP, TIFF and SVG; scenes as glTF, OBJ,
X3D, VRML and an interactive web page; animations as PNG frames, GIF, MP4 and WebM.

**Filters (section 2).** 268 filters and sources in nine menu categories (Common, Data,
Geometry, Grains, Mesh, Temporal, Annotation, Python, Sources), each with typed settings, help,
search keywords, a Python call and an entry in the generated filter reference. Custom filters,
sources, readers and panels in Python; filters and sources in Rust as shared libraries.

**Rendering and views (section 3).** A wgpu renderer with surface, edge, wireframe, point,
outline, molecule, 3D glyph, Surface LIC, texture and volume representations; a light kit, custom
lights, shadows, ambient occlusion, Eye-Dome Lighting, tone mapping, MSAA and FXAA,
order-independent transparency, materials and environment maps, CPU and GPU path tracing.
Several 3D views with linked cameras, several chart views, a spreadsheet, a data axes grid,
legends with units, text, time and ruler annotations, plane, box, sphere, line, point and light
widgets, picking and selection, a color map editor. Keyframe animation of the camera, filter
values and display properties with a Python cue. Big surfaces are culled and drawn coarse while
the camera moves.

**User experience (section 4).** A docking window with layout presets and layouts that follow
the window's shape, a command palette, a Welcome window, File › New, filters suggested for the
selected data with the reason, an All filters menu, right-click menus in the Pipeline and the
3D view, view buttons (fit, zoom, pan) in the 3D view, dark, light, high-contrast and system
themes, shortcuts that can be changed, screen-reader labels, the system clipboard, and a touch
interface for tablets and phones (Desktop, Tablet and Phone interfaces, safe areas, two-finger
gestures).

**Python (section 5).** An embedded interpreter with the `aurafx` module (NumPy in and out), a
console with completion, a script editor, `--script`, the trace, a Notebook panel for `.ipynb`
files, a Jupyter kernel that drives the window, plugins with reload on save, and a generated
API reference.

**AI assistant (section 6).** Ollama, Anthropic and OpenAI-compatible providers with streaming;
the pipeline's description as context; agent mode that runs code and iterates; a picture of the
3D view with a message; Describe a filter; saved prompts and skills; local-only mode and path
redaction; models that reason before they answer (reasoning shown while waiting, off by default
for Ollama); a copy button per message and code block; saved conversations.

**Simulation runner (section 7).** Local and SSH jobs, Slurm, PBS and LSF submission, live
logs, GPU selection and monitoring, parameter sweeps, watch folders, a remote file browser.
Templates: LAMMPS, MOOSE, OpenFOAM, Python pipeline, shell command, and cases written from the
pipeline's data: MOOSE polycrystal, SPPARKS grain growth, PINNACLE crystal plasticity, JAX-FEM
polycrystal, DAMASK, an FFT texture file and LAMMPS atoms of a polycrystal.

**GPU compute (section 8).** Adapter choice and memory statistics; GPU help for Slice and
Contour of grids (the GPU finds the cells to cut, the CPU cuts them, so results are unchanged);
fallback to the CPU on any GPU error; a benchmark in Settings.

**Remote and distributed (section 9).** `aurafx-server` over SSH: the pipeline runs where the
data is and sends back the data, its surface, or pictures drawn by the server's GPU and
composited by depth. Parallel servers with one piece per process, started by the server,
`mpirun`, `srun` or a command of the user's, with ghost layers between pieces. Files larger
than memory streamed in pieces through one process. A guided window for setting up a machine.

**Documentation (section 10).** The user guide as Markdown, an HTML book and a PDF with a
generated picture per feature; a filter reference and a Python API reference generated from the
code and kept current by tests; a benchmarks page; examples, tutorials and a Sample pipelines chapter
that goes through every clip of the project page step by step; the project page.

**Grain workbench (section 11), geometry and meshing (section 12), CAD (section 13).** Complete
as listed in those sections, including orientation gradients along the build direction and in
plane, and realistic synthetic grains. Sources › Synthetic Microstructure gathers every
synthetic type in one window: the grain sources and 37 morphological models of particles,
fibres, random fields, lattices, foams, growths and bio-inspired structures, each starting from
standard settings or from a named starting point, with a model followed while it forms and
studies over sizes and seeds. Filters › Microstructure measures any image of phases
(fractions, surface, two-point statistics, thickness, tortuosity, fabric, percolation,
representative volume, section sizes), draws exact geometry and smooth surfaces from the
models, and computes effective conductivity, stiffness and wave bands.

### Open

- **Readers and writers:** Silo, ADIOS2, Tecplot SZL, SEG-Y, VASP OUTCAR, Alembic, USD, OpenVDB,
  further GDAL rasters, a generic HDF5 browser; EXR pictures; POV-Ray and PDF scenes; the
  per-format matrix `docs/reference/format-matrix.md` is not written.
- **Filters:** the catalog entries of section 2 without a counterpart, among them the filters
  for non-overlapping (CTH) AMR, subhalo finding, selection sources, hyper tree grid ghost
  cells; indexing EBSD patterns.
- **Rendering and views:** temporal anti-aliasing, stereo and VR, occlusion culling, mesh
  streaming, comparative views, custom bounds and units on the axes grid, the remaining widgets.
- **Scale:** per-piece reading for formats other than VTK XML, more GPU filters, vendor GPU
  backends (CUDA, ROCm), collaboration between several clients, in-situ coupling with running
  solvers, tile displays.
- **Polish:** localization; `.deb`, MSIX and Flatpak packages; subscription or OAuth sign-in for
  hosted assistant providers; job templates for Abaqus/CalculiX and GROMACS.
- **Meshing:** transition templates for local refinement of hexahedra, sharp edges in
  tetrahedral meshes, conforming tetrahedra between touching bodies, fillets on picked curves
  and healing of implicit bodies, sweeps with several sources or targets, submapping.
- **CAD:** the *Later* line of section 13.
- **Synthetic grains:** boundaries are all alike (no dependence on misorientation), a grain cut
  in two keeps one id, the layer model has no melt-pool shape, and Pack Grains has no realism
  setting of its own (the Grain Realism filter works on its output).
- **Synthetic microstructures:** the tiling by golden rhombohedra and the periodic
  approximants of the tilings that never repeat are not there for the automata, which do not
  wrap on their other cells; reaction and diffusion run on the cubic grid only; mixtures of
  laws of directions for fibres are not there. The property filters are linear (conduction,
  elasticity): plasticity, damage, fracture, contact and fatigue are left to the solvers the
  cases are written for. The wave bands use plane waves (weak with voids); the optimized cell
  has one material and three goals. Hollow fibres, flat ends and coatings are not in the
  description that Smooth Surface draws from (the cells are used for those).
- **Touch:** the real keyboard height on iOS (a share of the screen is kept free instead) and
  composed text input on Android.

### Not verified

- Slurm, PBS and LSF submission is tested with stand-in commands, not on a real cluster.
- The MOOSE polycrystal deck is checked against the solid mechanics module's sources and tests;
  MOOSE itself was not run. The JAX-FEM script was not run against an installed JAX-FEM. The
  PINNACLE case was run and read back; the DAMASK case is loaded by DAMASK's own Python package.
- On devices: dragging tabs with a finger, the direction of the twist gesture, the iPad picture
  filling the screen (touches landing where they are made) and opening the published guide and
  the project's pages from the Android and iOS apps were built and checked in code, not yet
  confirmed on a tablet or a phone by the author of the change.
- Pasting from another program into a text box, and the CAD environment's plane preview under
  a real pointer, its dark theme and the Show part toggle switched off, were checked through
  scripted states and pictures only.
- The independent STEP reader check runs only when `AURAFX_OCP_PYTHON` names a Python with
  OpenCASCADE's bindings.
- The assistant's CAD modelling check (`scripts/cad/assistant_check.sh`) depends on the model:
  results differ between models and between runs of a small local model.

### Known limits

- **Curved cells:** higher-order pyramids are cut through their corners only; legacy `.vtk`
  files cannot carry per-cell degrees or rational weights, so curved cells are saved as `.vtu`.
- **Cell grids:** quadratic cells are shown as linear cells through their corners; ghost-node
  marks are not read.
- **MapInfo:** text, arcs, rectangles, ellipses and collections keep their attributes but have
  no geometry.
- **Pictures:** an SVG picture sorts triangles by depth, so surfaces that cut through each
  other get jagged boundaries; a texture image shows in the rasterized view only (ray tracing,
  scene export and server-drawn pictures show the solid color); server-drawn objects do not cast
  shadows on local ones; wide lines are picked as one-pixel lines.
- **Streaming:** only VTK XML grids and `.vtu` files read just the bytes of a piece; other
  formats are read whole, then split.
- **Pipeline import:** numbered-step pipeline files are mapped step by step where auraFX has
  the filter; the rest are listed as skipped.
- **CAD kernels:** the B-rep kernel does not build a modelled thread, a sweep with twist, or a
  shell its own check refuses, and does not cut a body made of triangles; such parts are built by
  the implicit kernel, which approximates fillets and leaves a
  draft undone (each said in the part's notes). A part only the implicit kernel builds is
  written to STEP as facets; IGES is written by the B-rep kernel only.
- **CAD sketches and assemblies:** a sweep's path is one open chain in one sketch; a loft is
  straight between parallel sections; mate angles run from 0 to 180 degrees; a part started
  from triangles has one face per imported body.
- **Mobile builds** are viewers: no embedded Python and no simulation runner; the user guide is
  not inside the app (Help › User guide opens the published one, which needs a network).
- **Sketches from the part:** an edge taken into a sketch is a copy that does not follow later
  changes of the part; a round edge seen at a slant becomes a polyline; the part's outlines under
  a sketch are jagged while the part is drawn by the implicit kernel.

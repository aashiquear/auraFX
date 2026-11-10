# Filter reference

<!-- Generated from the filter registry by crates/aurafx-filters/tests/filter_reference.rs; do not edit. -->

Every filter and source, by menu, with what it does, its Python name (`node.<name>(...)` for a filter, `afx.<name>(...)` for a source) and its settings with their defaults. Settings marked *advanced* show under **Show advanced** in Properties.

## Annotation

### Annotate Attribute Data

Show the value of an array at one point or cell (by its number) in a corner of the 3D views: a probe that follows the time steps.

Python: `annotate_attribute_data` · also found by: value at point, probe label, annotation, point value, cell value

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| Array | `array` | text | (empty) |  |
| Point or cell number | `id` | whole number 0 to any | 0 |  |
| Format (empty: name = value) | `format` | text | (empty) |  |
| Place | `corner` | upper left / upper right / lower left / lower right / upper center / lower center | `upper left` |  |
| Text size | `size` | number 6 to 96 | 16 |  |

### Annotate Global Data

Show a global (field) value of the data in a corner of the 3D views, such as the kinetic energy of a simulation; a global with one value per time step shows the current step's.

Python: `annotate_global_data` · also found by: global variable, field data, energy, annotation, value label

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Field array | `array` | text | (empty) |  |
| Component | `component` | whole number 0 to 1000 | 0 |  |
| Format (empty: name = value) | `format` | text | (empty) |  |
| Place | `corner` | upper left / upper right / lower left / lower right / upper center / lower center | `upper left` |  |
| Text size | `size` | number 6 to 96 | 16 |  |

### Annotate Selection

List selected points in a corner of the 3D views: how many, and the first ones with their numbers and values. Apply it to the output of Extract Selection.

Python: `annotate_selection` · also found by: selection label, selected points, annotation, list selection

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Arrays (empty: the first three) | `arrays` | text | (empty) |  |
| Points listed | `rows` | whole number 0 to 1000 | 5 |  |
| Place | `corner` | upper left / upper right / lower left / lower right / upper center / lower center | `lower left` |  |
| Text size | `size` | number 6 to 96 | 16 |  |

### Annotate Time Filter

Show the time in a corner of the 3D views, formatted as you like (Time: {:.3} s), optionally scaled and shifted (time x scale + shift).

Python: `annotate_time_filter` · also found by: time label, time text, clock, annotation, simulation time

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Format | `format` | text | `Time: {:.3}` | {} is the value; {:.3} three decimals, {:.2e} an exponent, {:.4g} four significant digits. |
| Scale | `scale` | number -∞ to ∞ | 1 |  |
| Shift | `shift` | number -∞ to ∞ | 0 |  |
| Place | `corner` | upper left / upper right / lower left / lower right / upper center / lower center | `upper left` |  |
| Text size | `size` | number 6 to 96 | 16 |  |

### Environment Annotation

Show the date and time, the user, the machine, the operating system and the auraFX version in a corner of the 3D views (for pictures that record how they were made).

Python: `environment_annotation` · also found by: date, user name, host, machine, version, annotation, stamp

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Date and time | `date` | on/off | on |  |
| User | `user` | on/off | on |  |
| Machine | `machine` | on/off | on |  |
| Operating system | `system` | on/off | off |  |
| auraFX version | `version` | on/off | off |  |
| Place | `corner` | upper left / upper right / lower left / lower right / upper center / lower center | `lower right` |  |
| Text size | `size` | number 6 to 96 | 16 |  |

### Python Annotation

Show the result of a Python expression in a corner of the 3D views: the point and cell arrays are NumPy arrays by name (the blocks of a multi-block joined), with points, inputs, t (the time) and step (the time step), e.g. f'max T = {T.max():.1f} K'.

Python: `python_annotation` · also found by: python, expression, annotation, numpy, custom text, programmable annotation

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Expression | `expression` | text | `f'{len(points)} points'` |  |
| Place | `corner` | upper left / upper right / lower left / lower right / upper center / lower center | `upper left` |  |
| Text size | `size` | number 6 to 96 | 16 |  |

## Common

### AMR Contour

Isosurfaces of a cell array of AMR data, without cracks where levels meet (on the dual grid).

Python: `amr_contour` · also found by: amr, amr contour, cell data contour, adaptive mesh refinement, crack-free, dual

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cell array | `array` | array | (the active array) |  |
| Component | `component` | text | (empty) |  |
| Values | `values` | list of numbers | [] | Leave empty for the middle of the data range. |

### Append Datasets

Combine several datasets into one unstructured grid. Arrays present in every input are kept.

Python: `append_datasets` · also found by: append, combine, merge, join, concatenate, multiple inputs

### Calculator

Compute a new array from an expression of existing arrays and coordinates.

Python: `calculator` · also found by: expression, formula, math, compute, derived

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Attribute | `attribute` | Points / Cells | `Points` |  |
| Expression | `expression` | list of numbers | `coordsX` | e.g. mag(velocity), sqrt(x^2+y^2), if(T > 300, 1, 0) |
| Result name | `result` | text | `Result` |  |

### Clip

Cut away part of the data with a plane, box, sphere or scalar value.

Python: `clip` · also found by: cut, remove, half, crop, cutaway

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Clip type | `type` | Plane / Box / Sphere / Scalar | `Plane` |  |
| Origin / center | `origin` | x, y, z | (NaN, NaN, NaN) |  |
| Normal | `normal` | x, y, z | (1, 0, 0) |  |
| Radius (sphere) | `radius` | number 0 to ∞ | 1 |  |
| Box min | `box_min` | x, y, z | (NaN, NaN, NaN) |  |
| Box max | `box_max` | x, y, z | (NaN, NaN, NaN) |  |
| Scalar (for Scalar clip) | `array` | array | (the active array) |  |
| Component | `component` | text | (empty) |  |
| Value (for Scalar clip) | `value` | number -∞ to ∞ | 0 |  |
| Invert | `invert` | on/off | off |  |

### Contour

Isosurfaces (3D) or isolines (2D) where a scalar equals given values.

Python: `contour` · also found by: isosurface, isoline, level set, marching cubes, iso

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Contour by | `array` | array | (the active array) |  |
| Component | `component` | text | (empty) |  |
| Values | `values` | list of numbers | [] | Leave empty for the midpoint of the data range. |
| Auto count (when values empty) | `count` | whole number 1 to 100 | 1 | *advanced* |

### Extract Subset

Crop an image or structured grid to a box of grid indices, optionally keeping every n-th point.

Python: `extract_subset` · also found by: voi, crop, subsample, downsample, stride, region, ijk, index

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| From index (i, j, k) | `voi_min` | x, y, z | (NaN, NaN, NaN) | First grid index kept along each axis; leave unset for the start of the grid. |
| To index (i, j, k) | `voi_max` | x, y, z | (NaN, NaN, NaN) | Last grid index kept along each axis; leave unset for the end of the grid. |
| Every n-th point | `rate` | x, y, z | (1, 1, 1) | Sample rate along i, j and k (2 keeps every other point). |
| Include last index | `include_boundary` | on/off | off | Always keep the last index even when the sample rate skips it. |

### Glyph

Draw arrows, spheres or cubes at points, oriented and scaled by arrays.

Python: `glyph` · also found by: arrows, vectors, quiver, hedgehog, markers

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Shape | `shape` | Arrow / Line / Sphere / Cube | `Arrow` |  |
| Orientation | `orient` | array | (the active array) |  |
| Scale by | `scale_array` | array | (the active array) |  |
| Scale factor | `scale` | number 0 to ∞ | NaN | Leave unset for 5% of the data size. |
| Maximum glyphs | `max_glyphs` | whole number 1 to 10000000 | 5000 |  |
| Resolution | `resolution` | whole number 3 to 64 | 12 | *advanced* |

### Slice

Cut the data with a plane or sphere and show the values on the cut surface.

Python: `slice` · also found by: cut, plane, section, cross section

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cut type | `type` | Plane / Sphere | `Plane` |  |
| Origin | `origin` | x, y, z | (NaN, NaN, NaN) | Leave unset to use the center of the data. |
| Normal | `normal` | x, y, z | (1, 0, 0) |  |
| Radius (sphere) | `radius` | number 0 to ∞ | 1 |  |
| Offsets | `offsets` | list of numbers | [0.0] | *advanced* Additional parallel slices at these distances along the normal. |

### Stream Tracer

Streamlines of a vector field from seeds on a line or in a sphere.

Python: `stream_tracer` · also found by: streamlines, flow, trajectories, integrate, velocity, field lines

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `vectors` | array | (the active array) |  |
| Seeds | `seed_type` | Line / Point Cloud | `Point Cloud` |  |
| Line start | `point1` | x, y, z | (NaN, NaN, NaN) |  |
| Line end | `point2` | x, y, z | (NaN, NaN, NaN) |  |
| Cloud center | `center` | x, y, z | (NaN, NaN, NaN) |  |
| Cloud radius | `radius` | number 0 to ∞ | NaN |  |
| Number of seeds | `seeds` | whole number 1 to 100000 | 100 |  |
| Direction | `direction` | Both / Forward / Backward | `Both` |  |
| Max steps | `max_steps` | whole number 1 to 1000000 | 2000 | *advanced* |
| Step (fraction of size) | `step` | number 0.000001 to 1 | 0.005 | *advanced* |
| Max length (fraction of size) | `max_length` | number 0 to 1000 | 2 | *advanced* |

### Threshold

Keep only cells whose values fall within a range.

Python: `threshold` · also found by: filter, range, between, select, cutoff

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Scalars | `array` | array | (the active array) |  |
| Component | `component` | text | (empty) |  |
| Lower | `lower` | number -∞ to ∞ | NaN |  |
| Upper | `upper` | number -∞ to ∞ | NaN |  |
| Point rule | `method` | All points / Any point | `All points` | *advanced* |
| Invert | `invert` | on/off | off |  |

### Warp By Scalar

Displace points along a direction (or normals) by a scalar value.

Python: `warp_by_scalar` · also found by: height, elevation, carpet plot, deform

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Scalars | `array` | array | (the active array) |  |
| Scale factor | `scale` | number -∞ to ∞ | 1 |  |
| Direction | `normal` | x, y, z | (0, 0, 1) |  |
| Use point normals | `use_normals` | on/off | off |  |

### Warp By Vector

Displace points by a vector field (e.g. displacement) times a scale factor.

Python: `warp_by_vector` · also found by: deform, displacement, displace, deformed shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `vectors` | array | (the active array) |  |
| Scale factor | `scale` | number -∞ to ∞ | 1 |  |

## Data

### AMR Dual Clip

Clip AMR data by a cell array without cracks between levels: the levels' cell centres are joined into one stitched mesh (the dual grid) and the part where the array is above a value is kept.

Python: `amr_dual_clip` · also found by: amr clip, dual clip, crack free, volume fraction clip, adaptive mesh

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cell array | `array` | array | (the active array) |  |
| Keep above | `value` | number -∞ to ∞ | 0.5 |  |
| Keep below instead | `invert` | on/off | off |  |

### AMR Dual Grid

AMR data as one mesh joining the cell centers (levels stitched together), cell arrays on its points: for crack-free clips, slices and streamlines.

Python: `amr_dual_grid` · also found by: amr, dual grid, cell centers, stitch levels, adaptive mesh refinement, dual clip

### Append Attributes

Copy the arrays of other inputs with the same number of points or cells onto the first input.

Python: `append_attributes` · also found by: merge arrays, combine fields, add arrays, join attributes

### Binary Morphology

Erode, dilate, open (erode then dilate) or close (dilate then erode) the non-zero cells of an array of a volume with a ball of a radius in cells (radius 1: the six face neighbours); the edges of the grid are not eaten.

Python: `binary_morphology` · also found by: erode, dilate, opening, closing, morphology, mask, binary

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Operation | `operation` | Erode / Dilate / Open / Close | `Open` |  |
| Radius (cells) | `radius` | whole number 1 to 50 | 1 |  |
| Result name | `result` | text | `Morphology` |  |

### Boundary Patches (OpenFOAM)

Show the boundary patches (inlet, outlet, walls, ...) of an OpenFOAM case with their boundary values.

Python: `openfoam_patches` · also found by: openfoam, foam, patch, boundary, wall, inlet, outlet, mesh regions

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Patches | `patches` | text | (empty) | Comma-separated patch names; leave empty for all patches |

### Cell Data to Point Data

Average cell values onto the points for smooth coloring.

Python: `cell_data_to_point_data` · also found by: interpolate, smooth, nodal

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Pass cell data | `pass_cell_data` | on/off | off |  |

### Cell Grid Centers

A point at the center of every cell of a cell grid, with its fields evaluated there.

Python: `cell_grid_centers` · also found by: cell grid, dg, centers, centroids

### Cell Grid Compute Sides

Find the sides of a cell grid's cells (by default the outer faces of its 3-D cells) and draw them instead of the cells.

Python: `cell_grid_compute_sides` · also found by: cell grid, dg, sides, faces, edges, boundary, surface, compute sides

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Sides to include | `include` | Next lowest dimension / Surfaces of inputs / Edges of inputs / Vertices of inputs / All sides | `Next lowest dimension` | Next lowest dimension: faces of 3-D cells, edges of 2-D cells and end points of edges. The others: faces, edges or corners of every cell of higher dimension, or all of them. |
| Omit sides of surfaces and lines | `omit_renderable` | on/off | on | Cells (and side lists) of 2 or fewer dimensions give no sides. |
| Keep surfaces and lines | `preserve_renderable` | on/off | on | Keep drawing the 2-D and 1-D cells and the side lists already there; 3-D cells are always hidden. |
| Keep | `strategy` | Boundary / Odd count / Every side | `Boundary` | *advanced* Boundary: faces found once (not shared by two cells) and every lower side; Odd count: sides found an odd number of times; Every side: each side once. |

### Cell Grid Elevation

A field on a cell grid: the distance along an axis from an origin (or from the origin).

Python: `cell_grid_elevation` · also found by: cell grid, dg, elevation, height

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Name | `name` | text | `Elevation` |  |
| Origin | `origin` | x, y, z | (0, 0, 0) |  |
| Axis | `axis` | x, y, z | (0, 0, 1) |  |
| Distance | `distance` | Planar distance / Spherical distance | `Planar distance` |  |

### Cell Grid Probe

Find the cells of a cell grid holding the points of another dataset: each point's cell, parametric coordinates and an attribute's value there.

Python: `cell_grid_probe` · also found by: cell grid, dg, probe, sample, locate, parametric coordinates

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Attribute | `attribute` | text | (empty) | The attribute to sample at the points; empty to only find the cells. |

### Cell Grid To Unstructured Grid

A cell grid as ordinary cells, its fields evaluated at the corners (averaged where cells meet).

Python: `cell_grid_to_unstructured_grid` · also found by: cell grid, dg, convert, unstructured

### Cell Grid Transform

Move, rotate and scale a cell grid (its shape, or another 3-D attribute): scale, then rotate about Y, X and Z, then translate.

Python: `cell_grid_transform` · also found by: cell grid, dg, transform, translate, rotate, scale

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Attribute | `attribute` | text | (empty) | The attribute to transform; empty for the cells' shape. |
| Translate | `translate` | x, y, z | (0, 0, 0) |  |
| Rotate (deg) | `rotate` | x, y, z | (0, 0, 0) |  |
| Scale | `scale` | x, y, z | (1, 1, 1) |  |

### Cell Grid Warp

Move a cell grid's points by a vector attribute shared at its points, times a scale.

Python: `cell_grid_warp` · also found by: cell grid, dg, warp, deform, displacement

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Deformation attribute | `attribute` | text | (empty) |  |
| Scale | `scale` | number -∞ to ∞ | 1 |  |

### Cell Size

Volume, area and length of every cell.

Python: `cell_size` · also found by: volume, area, length, measure

### Compute Derivatives

Derivatives of a vector field at the points: its gradient (dv_i/dx_j, 9 components), strain rate, vorticity (curl), divergence, and the vortex criteria Q (positive where rotation beats strain) and lambda2 (negative in a vortex core).

Python: `compute_derivatives` · also found by: vorticity, curl, strain rate, divergence, jacobian, velocity gradient, q criterion, lambda2, vortex cores

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `array` | array | (the active array) |  |

### Compute Molecule Bonds

Bonds between atoms closer than the sum of their covalent radii plus a tolerance.

Python: `compute_molecule_bonds` · also found by: molecule, bonds, atoms, chemistry, covalent, connect atoms

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Atomic numbers | `array` | array | (the active array) | Leave empty for atomic_number (or VTK's Atomic Numbers). |
| Tolerance | `tolerance` | number 0 to ∞ | 0.45 | Absolute: Å added to the radii sum. Relative: factor on each radius. |
| Absolute tolerance | `absolute` | on/off | on |  |

### Compute Quartiles

Minimum, first quartile, median, third quartile and maximum of chosen arrays, with the interquartile range and the whisker ends of a box plot (a row per variable).

Python: `compute_quartiles` · also found by: quartiles, box plot, median, percentiles, iqr, whiskers, five number summary

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| Variables (empty: every array with one value) | `variables` | text | (empty) | Array names separated by commas; V is the magnitude of a vector, V[1] its second component. |

### Contingency Statistics

How often each pair of values of two arrays occurs (real values are put in bins first): a table of counts and joint and conditional probabilities, with chi-square, its degrees of freedom and the mutual information as field data.

Python: `contingency_statistics` · also found by: contingency table, cross tabulation, chi square, mutual information, categories, joint distribution

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| First array | `x` | text | (empty) |  |
| Second array | `y` | text | (empty) |  |
| Bins for real values | `bins` | whole number 1 to 100000 | 10 |  |

### Convert To Cell Grid

A mesh of linear cells as a cell grid: point data as attributes at the points, cell data as constant attributes.

Python: `convert_to_cell_grid` · also found by: cell grid, dg, convert, discontinuous galerkin

### Coordinates

Adds the point positions as a 3-component point array Coordinates (to color or compute by position).

Python: `coordinates` · also found by: positions, xyz, points as array, coordinates

### Density Map

Where pairs of values of two arrays gather: a Gaussian kernel density on an image (x across, y up), with the density levels holding 50, 90 and 99 % of the samples as field data (contour the image at them).

Python: `density_map` · also found by: 2d density, joint distribution, kde, scatter density, heat map, density estimate

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| X array | `x` | text | (empty) |  |
| Y array | `y` | text | (empty) |  |
| Resolution | `resolution` | whole number 2 to 4096 | 128 |  |

### Descriptive Statistics

Count, min, max, mean, standard deviation, median and sum of every array.

Python: `descriptive_statistics` · also found by: stats, mean, average, std, summary

### Elevation

Scalar that increases along a direction (height coloring).

Python: `elevation` · also found by: height, gradient color, z value

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Low point | `low_point` | x, y, z | (NaN, NaN, NaN) |  |
| High point | `high_point` | x, y, z | (NaN, NaN, NaN) |  |

### Evenly Spaced Streamlines 2D

Streamlines of a vector field in the XY plane, spread evenly: each new line starts at the separation from the lines already drawn and stops where it comes too close to another, until the plane is filled (a picture of a 2-D flow without seeds to place).

Python: `evenly_spaced_streamlines_2d` · also found by: streamlines, even spacing, flow lines, 2d flow, planar flow, uniform density streamlines

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `vectors` | array | (the active array) |  |
| First seed | `start` | x, y, z | (NaN, NaN, NaN) | Unset: the center of the data. |
| Separation (0 = 1/30 of the data's size) | `separation` | number 0 to ∞ | 0 |  |
| Stop at this fraction of the separation | `test_ratio` | number 0.01 to 1 | 0.5 |  |
| Step as a fraction of the separation | `step_ratio` | number 0.01 to 1 | 0.1 | *advanced* |
| Steps per line, each way | `max_steps` | whole number 1 to 10000000 | 2000 | *advanced* |

### Extract Cells By Region

Whole cells inside a half-space (the side a plane's normal points to), a box or a sphere: those with all their points inside, or those the region's boundary crosses too. Cells are kept whole, not cut.

Python: `extract_cells_by_region` · also found by: region, box, sphere, half space, select cells, crop, subset, inside

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Region | `region` | Box / Sphere / Plane | `Box` |  |
| Cells kept | `mode` | All points inside / Crossing the boundary too | `All points inside` |  |
| Center / plane point | `origin` | x, y, z | (NaN, NaN, NaN) |  |
| Plane normal | `normal` | x, y, z | (1, 0, 0) |  |
| Sphere radius | `radius` | number 0 to ∞ | NaN | Empty: a quarter of the data's size |
| Box min | `box_min` | x, y, z | (NaN, NaN, NaN) |  |
| Box max | `box_max` | x, y, z | (NaN, NaN, NaN) |  |
| Outside instead | `invert` | on/off | off |  |

### Extract Cells By Type

Whole cells of the types named (Tetra, Hexahedron, Wedge, Pyramid, Triangle, Quad, Line, Vertex, quadratic types…) or of a dimension (3D, 2D, 1D, 0D).

Python: `extract_cells_by_type` · also found by: cell type, element type, hexahedra, tetrahedra, surface cells, volume cells, filter cells

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Types | `types` | text | `3D` | Names or dimensions, separated by commas: Hexahedron, Tetra or 3D, 2D |
| All but these | `invert` | on/off | off |  |

### Extract Component

Copy one component of a vector/tensor into a scalar array.

Python: `extract_component` · also found by: component, x, y, z

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Component | `component` | whole number 0 to 8 | 0 |  |

### Extract Selection

Extract the selected cells or points as a new dataset.

Python: `extract_selection` · also found by: selection, extract, subset, pick, cells, points

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Selection of | `attribute` | Cells / Points | `Cells` |  |
| Ids | `ids` | list of numbers | [] | Flattened cell or point indices (filled in by the 3D view's selection tools). |

### Find Change Points

Where a table column (a value over time, along a line) changes its level: splits that lower the squared error by more than Penalty times the noise variance. Adds ChangePoint (1 on the first row of each new level) and SegmentMean columns, and the change rows (and x values) as field data.

Python: `find_change_points` · also found by: change point, change points, step, jump, regime change, time series, segmentation, anomaly

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Column | `column` | text | (empty) |  |
| x column (for the field data) | `x_column` | text | `Time` |  |
| Penalty | `penalty` | number 0 to 1000000000 | 25 | Higher finds fewer changes; about 2 ln(n) to 10 ln(n) for n rows. |
| Shortest segment (rows) | `min_size` | whole number 1 to 1000000 | 3 |  |

### Find Data

Extract cells or points matching a condition, e.g. Temp > 600 & Pres < 0.1.

Python: `find_data` · also found by: query, search, where, condition, select

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Find | `attribute` | Cells / Points | `Cells` |  |
| Condition | `expression` | list of numbers | (empty) | Calculator syntax; non-zero means selected. Examples: T > 300, mag(V) >= 2 & coordsZ < 0 |

### Find Outliers

Points or cells whose value stands out from the rest of the array (a vector by its length): a robust z-score from the median and the median absolute deviation, a z-score, or the interquartile fences. Gives OutlierScore and Outlier (1 or 0), and the count as field data; Threshold on Outlier keeps them.

Python: `find_outliers` · also found by: outliers, anomaly, anomalies, spikes, bad values, z-score, mad, iqr, detect

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Method | `method` | Robust z-score (median, MAD) / z-score (mean, standard deviation) / Interquartile fences | `Robust z-score (median, MAD)` |  |
| Threshold | `threshold` | number 0 to 1000 | 3.5 | Robust z-score and z-score: how many deviations away (3.5 is usual); fences: how many IQRs past the quartiles (1.5 mild, 3 extreme). |

### Fragment Integration

Find the fragments of a material: cells whose volume fraction (or any cell array) is at least a threshold, joined into connected regions through shared faces (across AMR levels through touching boxes). Gives a table with each fragment's cell count, volume (fraction x cell volume), centroid and the volume integrals of the other cell arrays, or the fragment cells with FragmentId.

Python: `fragment_integration` · also found by: fragments, connected regions, droplets, particles, volume fraction, amr fragments, fragment volume, debris

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Volume fraction (cell array) | `array` | array | (the active array) |  |
| Threshold | `threshold` | number -∞ to ∞ | 0.5 |  |
| Output | `output` | Table / Cells | `Table` |  |

### Gaussian Smooth Image

Smooth a volume (or picture) with a Gaussian of a standard deviation in cells along each axis (cut at 4 standard deviations; edges repeat the outer values).

Python: `gaussian_image` · also found by: gaussian, blur, smooth image, denoise, low pass

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Standard deviation (cells) | `sigma` | x, y, z | (1, 1, 1) |  |
| Result name | `result` | text | `Gaussian` |  |

### Generate Global Ids

Adds GlobalPointIds and GlobalCellIds, unique over every block of a multiblock; points that coincide share one id.

Python: `generate_global_ids` · also found by: global ids, unique ids, numbering, blocks

### Ghost Cells

Give each partition of partitioned data (the blocks of a multi-block, e.g. a .pvtu read as partitions) layers of the neighbouring partitions' cells that touch it, marked as ghosts in vtkGhostType, so that filters looking at neighbours give the same answer at partition borders.

Python: `ghost_cells` · also found by: ghost cells, halo, overlap, partition border, ghost cells generator, ghost layers

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Layers | `layers` | whole number 1 to 16 | 1 |  |

### Gradient

Spatial gradient of a point scalar (adds <name>_gradient).

Python: `gradient` · also found by: derivative, slope, grad, flux

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Scalar | `array` | array | (the active array) |  |

### Halo Finder

Find halos in particles: particles closer than a linking length (a fraction of the mean spacing) are friends, friends of friends are one halo. Adds HaloId to the particles (-1 outside halos), or gives a table of halos with their count, mass, centre of mass, mean velocity, and the radius and mass within which the mean density is an overdensity (200) times the box's mean density.

Python: `halo_finder` · also found by: friends of friends, fof, halo, sod, spherical overdensity, cosmology, dark matter, clusters of particles, subhalo

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Linking length (fraction of the mean spacing) | `linking` | number 0.000001 to 10 | 0.2 |  |
| Smallest halo (particles) | `min_members` | whole number 1 to any | 10 |  |
| Mass array (empty: 1 each) | `mass` | text | (empty) |  |
| Velocity array (empty: none) | `velocity` | text | (empty) |  |
| Overdensity | `overdensity` | number 1 to 1000000000 | 200 |  |
| Output | `output` | Particles / Halos | `Particles` |  |

### Histogram

Distribution of an array as a table of bins (shown in the Charts panel).

Python: `histogram` · also found by: distribution, bins, frequency, pdf

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Component | `component` | text | (empty) |  |
| Bins | `bins` | whole number 1 to 100000 | 64 |  |
| Range min | `lower` | number -∞ to ∞ | NaN | *advanced* |
| Range max | `upper` | number -∞ to ∞ | NaN | *advanced* |

### Hyper Tree Grid Depth Limiter

Cut the trees of a hyper tree grid at a depth: cells at that depth become leaves.

Python: `hyper_tree_grid_depth_limiter` · also found by: htg, hyper tree grid, depth, level, coarsen, limit

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Depth | `depth` | whole number 0 to 64 | 0 |  |

### Hyper Tree Grid Evaluate Coarse

Values of refined (coarse) cells computed from their children: min, max, sum, average...

Python: `hyper_tree_grid_evaluate_coarse` · also found by: htg, hyper tree grid, coarse, average, aggregate, parent values

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Operator | `operator` | Don't change fast / Don't change / Min / Max / Sum / Average / Unmasked Average / Elder Child / Splatting Average | `Average` |  |
| Default | `default` | number -∞ to ∞ | 0 | Value counted for missing children (Average, Splatting Average). |

### Hyper Tree Grid Feature Edges

The edges of a hyper tree grid's visible region: its outline in 2-D, its corners and folds in 3-D, as lines with the leaves' values.

Python: `hyper_tree_grid_feature_edges` · also found by: hyper tree grid, htg, edges, outline, feature edges

### Hyper Tree Grid Generate Fields

Cell arrays CellSize, ValidCell and CellCenter, and TotalVisibleVolume in field data.

Python: `hyper_tree_grid_generate_fields` · also found by: htg, hyper tree grid, cell size, volume, visible leaves size, cell center

### Hyper Tree Grid Geometry

The outer surface of a hyper tree grid's visible leaves (and the walls next to masked cells), as polygons with the leaves' values.

Python: `hyper_tree_grid_geometry` · also found by: hyper tree grid, htg, surface, geometry, boundary

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Fill material | `fill_material` | on/off | on | With material interfaces: draw the material's part of each cell (off: only the interface lines in 2-D; pure cells left out in 1-D and 2-D). |

### Hyper Tree Grid To Dual Grid

A point at the center of every visible leaf of a hyper tree grid, joined into cells across the leaves (gives smooth contours and point data).

Python: `hyper_tree_grid_to_dual_grid` · also found by: hyper tree grid, htg, dual, cell centers, unstructured

### Hyper Tree Grid To Unstructured Grid

The visible leaves of a hyper tree grid as an unstructured grid (voxels, pixels or lines), with OriginalIds.

Python: `hyper_tree_grid_to_unstructured_grid` · also found by: htg, hyper tree grid, leaves, convert, unstructured

### Integrate Variables

Volume (or area/length) and the integral of every array over the domain.

Python: `integrate_variables` · also found by: integral, total, volume, area, mass, sum

### Iso Volume

The part of the data where a point scalar lies between two values, cut through the cells at those values (so the result's outer faces follow the value surfaces, not the cells).

Python: `iso_volume` · also found by: isovolume, band, range, between, value range, clip by scalar, interval

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Scalars | `array` | array | (the active array) |  |
| Component | `component` | text | (empty) |  |
| Lower | `lower` | number -∞ to ∞ | NaN | Empty: a quarter of the way up the range |
| Upper | `upper` | number -∞ to ∞ | NaN | Empty: three quarters of the way up the range |

### K-Means

Group the points (or cells, or table rows) into K clusters by the values of chosen arrays (k-means, started with k-means++): adds ClusterId and ClusterDistance, with the centers, their sizes and the within-cluster sum of squares as field data.

Python: `k_means` · also found by: clustering, kmeans, k means, classify, group, segmentation, unsupervised

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| Variables (empty: every array with one value) | `variables` | text | (empty) | Array names separated by commas; V is the magnitude of a vector, V[1] its second component. |
| Clusters (K) | `k` | whole number 1 to 10000 | 3 |  |
| Iterations | `max_iterations` | whole number 1 to 100000 | 100 |  |
| Standardize the variables | `standardize` | on/off | on | Divide each variable by its standard deviation first, so variables in large units do not dominate. |
| Random seed | `seed` | whole number 0 to any | 1 |  |

### Kernel Smoothing

A smooth estimate of an array's distribution (a Gaussian kernel density, bandwidth by Scott's rule unless given): a table of value and density for the Charts panel, a smooth histogram that integrates to one.

Python: `kernel_smoothing` · also found by: kernel density, kde, distribution, pdf, smooth histogram, density estimate

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Bandwidth (0 = automatic) | `bandwidth` | number 0 to ∞ | 0 |  |
| Samples | `samples` | whole number 2 to 100000 | 256 |  |

### Material Interface

Rebuild the interfaces between materials from cell arrays of volume fractions (0 to 1): in every mixed cell a plane facing down the fraction's gradient, placed so that the cell's material side holds exactly its fraction (Youngs' method). One block of interface polygons per material (lines on surface meshes), with the material's volume as field data.

Python: `material_interface` · also found by: volume fraction, youngs, interface reconstruction, vof, material boundary, multimaterial, plic

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Volume-fraction cell arrays (empty: all with one value) | `arrays` | text | (empty) | Names separated by commas, one per material. |
| Fractions this close to 0 or 1 count as pure | `tolerance` | number 0 to 0.5 | 0.000001 |  |

### Median Image

Replace each value of a volume (or picture) by the median of the values in a box around it (a radius of cells along each axis): removes speckle noise and keeps edges.

Python: `median_image` · also found by: median, denoise, speckle, smooth image, salt and pepper

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Radius (cells) | `radius` | x, y, z | (1, 1, 1) |  |
| Result name | `result` | text | `Median` |  |

### Merge Vector Components

Three scalar arrays (points, cells or table columns) combined into one vector array.

Python: `merge_vector_components` · also found by: combine, vector, components, xyz arrays, join arrays

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Attribute | `attribute` | Points / Cells | `Points` |  |
| X array | `x` | text | (empty) |  |
| Y array | `y` | text | (empty) |  |
| Z array | `z` | text | (empty) |  |
| Result | `result` | text | `vector` |  |

### Mesh Quality

Edge ratio and minimum angle of every cell, and for hexahedra and tetrahedra the full sets of quality metrics (scaled Jacobian, skew, aspect ratio, dihedral angles...), or a summary table with acceptable ranges.

Python: `mesh_quality` · also found by: quality, aspect ratio, skew, distortion, jacobian, scaled jacobian, taper, oddy, condition, verdict, mesh check

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Output | `output` | Cell values / Summary table | `Cell values` | Summary table: each hexahedron and tetrahedron metric's minimum, maximum and mean, its acceptable range and how many cells fall outside it. |

### Minkowski Functionals

Shape measures of the region where an array is above a threshold, from its boundary surface: volume, surface area, integrated mean curvature, Euler characteristic (1 for a ball, 0 for a torus, minus the number of tunnels plus the number of pieces and cavities), and the normalised W0 to W3.

Python: `minkowski_functionals` · also found by: minkowski, topology, euler characteristic, genus, mean curvature, porosity, morphology, surface area, minkowski filter

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Threshold | `threshold` | number -∞ to ∞ | 0 |  |

### Multicorrelative Statistics

Means, the covariance matrix and the correlation matrix (Pearson) of chosen arrays, as a table with a row per variable.

Python: `multicorrelative_statistics` · also found by: covariance, correlation, pearson, correlation matrix, joint statistics

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| Variables (empty: every array with one value) | `variables` | text | (empty) | Array names separated by commas; V is the magnitude of a vector, V[1] its second component. |

### Otsu Threshold

Split the values of a volume into classes by the thresholds that best separate its histogram (largest between-class variance): a label array (0 below the first threshold, 1 above it...) and the thresholds as field data.

Python: `otsu_threshold` · also found by: otsu, threshold, segmentation, binarize, automatic threshold, multiple thresholds

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Thresholds | `thresholds` | whole number 1 to 4 | 1 |  |
| Histogram bins | `bins` | whole number 2 to 4096 | 128 |  |
| Result name | `result` | text | `Otsu` |  |

### Overlapping AMR Level Ids

Cell array LevelIdScalars with the refinement level of every cell of AMR data.

Python: `overlapping_amr_level_ids` · also found by: amr, level, refinement level, adaptive mesh

### Overlapping Cells Detector

Count, for every cell, how many other cells share volume with it (or area, for surface cells in one plane): NumberOfOverlapsPerCell. Cells that only touch do not count.

Python: `overlapping_cells_detector` · also found by: overlap, intersecting cells, duplicate cells, mesh check, interpenetration

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Smallest overlap seen (fraction of a cell) | `tolerance` | number 0 to 0.5 | 0.05 |  |

### Pass Arrays

Keep only the listed arrays (smaller data, faster export).

Python: `pass_arrays` · also found by: remove arrays, filter arrays, keep

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Point arrays | `point_arrays` | text | (empty) | Comma-separated names; empty keeps none |
| Cell arrays | `cell_arrays` | text | (empty) |  |

### Perlin Noise

Smooth random-looking values at every point (Perlin noise), for textures and test data.

Python: `perlin_noise` · also found by: noise, random, procedural, turbulence, texture

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Frequency | `frequency` | x, y, z | (1, 1, 1) | Bumps per unit length along x, y and z. |
| Phase | `phase` | x, y, z | (0, 0, 0) | Shifts the pattern (half a unit per 1). |
| Amplitude | `amplitude` | number -∞ to ∞ | 1 |  |
| Result name | `result` | text | `PerlinNoise` |  |

### Plot On Intersection Curves

The curves where a plane cuts a surface, each ordered along its length with arc_length and the surface's arrays, one block per curve (for the Charts panel): a profile around a section of a body.

Python: `plot_on_intersection_curves` · also found by: intersection, section curve, profile, cut surface, wall plot, pressure around

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Origin | `origin` | x, y, z | (NaN, NaN, NaN) | Unset: the center of the data. |
| Normal | `normal` | x, y, z | (1, 0, 0) |  |

### Plot On Sorted Lines

Join line segments (from Slice, Contour on a surface, Feature Edges...) into curves ordered from one end to the other, each with arc_length, one block per curve: the Charts panel draws one line per curve.

Python: `plot_on_sorted_lines` · also found by: sort lines, order points, polyline, arc length, curve plot, contour line plot

### Plot Over Line

Sample values along a straight line; shows a chart of every array versus distance.

Python: `plot_over_line` · also found by: line plot, profile, probe line, graph, sample

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Point 1 | `point1` | x, y, z | (NaN, NaN, NaN) | Unset: from the minimum corner of the data bounds. |
| Point 2 | `point2` | x, y, z | (NaN, NaN, NaN) | Unset: to the maximum corner of the data bounds. |
| Resolution | `resolution` | whole number 1 to 1000000 | 1000 |  |

### Point And Cell Ids

Adds PointIds and CellIds: the index of every point and cell.

Python: `point_and_cell_ids` · also found by: ids, index, numbering, element number, node number

### Point Data to Cell Data

Average point values onto the cells.

Python: `point_data_to_cell_data` · also found by: elemental, average

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Pass point data | `pass_point_data` | on/off | off |  |

### Point Interpolator

Carry the point arrays of scattered points (particles, measurements, a point cloud) onto a volume, a plane, a line or the points of a second input, through a kernel over the nearest points: Shepard (inverse distance), Gaussian, linear (an average), nearest point, or SPH (cubic spline or Wendland, with mass and density arrays when given). Adds vtkValidPointMask (0 where no point was near).

Python: `point_interpolator` · also found by: interpolate points, scattered data, kriging, shepard, inverse distance, gaussian kernel, sph, smoothed particle, point cloud to grid, voronoi, point volume interpolator, point line interpolator, point plane interpolator, point dataset interpolator, sph interpolator

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Kernel | `kernel` | Shepard / Gaussian / Linear / Nearest point / SPH cubic / SPH quintic | `Shepard` |  |
| Neighbours | `neighbours` | N closest / Radius | `N closest` | SPH kernels use every point within twice the smoothing length. |
| N closest | `n_closest` | whole number 1 to 10000 | 8 |  |
| Radius (0 = from the spacing) | `radius` | number 0 to ∞ | 0 |  |
| Shepard power | `power` | number 0 to 20 | 2 |  |
| Gaussian sharpness | `sharpness` | number 0 to 100 | 2 |  |
| SPH smoothing length (0 = from the spacing) | `smoothing_length` | number 0 to ∞ | 0 |  |
| SPH density array | `density` | text | (empty) | *advanced* |
| SPH mass array | `mass` | text | (empty) | *advanced* |
| Where no point is near | `null` | Value / Nearest point | `Value` |  |
| Value | `null_value` | number -∞ to ∞ | 0 |  |
| Output (without a second input) | `output` | Volume / Plane / Line | `Volume` |  |
| X samples | `nx` | whole number 1 to 4096 | 50 |  |
| Y samples | `ny` | whole number 1 to 4096 | 50 |  |
| Z samples | `nz` | whole number 1 to 4096 | 50 |  |
| Plane normal | `axis` | X / Y / Z | `Z` |  |
| Plane position (0 to 1 across the bounds) | `at` | number 0 to 1 | 0.5 |  |
| Line start | `point1` | x, y, z | (NaN, NaN, NaN) | Unset: from the minimum corner of the bounds. |
| Line end | `point2` | x, y, z | (NaN, NaN, NaN) | Unset: to the maximum corner of the bounds. |
| Line resolution | `resolution` | whole number 1 to 1000000 | 200 |  |

### Principal Component Analysis

The principal components of chosen arrays: the projections PCA 0, PCA 1, ... on the data (largest variance first) and, as field data, the eigenvalues, the fraction of the variance each explains and the components.

Python: `principal_component_analysis` · also found by: pca, principal components, eigen, dimension reduction, variance, correlation structure

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| Variables (empty: every array with one value) | `variables` | text | (empty) | Array names separated by commas; V is the magnitude of a vector, V[1] its second component. |
| Components kept (0 = all) | `keep` | whole number 0 to 10000 | 0 |  |
| Standardize the variables | `standardize` | on/off | off |  |

### Probe Location

Interpolated values at a single point.

Python: `probe_location` · also found by: probe, point value, query, sample point

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Location | `location` | x, y, z | (NaN, NaN, NaN) |  |

### Random Attributes

A random array on the points or cells (uniform between two values; scalars or vectors), for testing filters and color maps.

Python: `random_attributes` · also found by: random, noise, test data, random vectors, random scalars

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Data | `data` | Points / Cells | `Points` |  |
| Name | `name` | text | `RandomValues` |  |
| Components (1 scalars, 3 vectors) | `components` | whole number 1 to 9 | 1 |  |
| Minimum | `minimum` | number -∞ to ∞ | 0 |  |
| Maximum | `maximum` | number -∞ to ∞ | 1 |  |
| Random seed | `seed` | whole number 0 to any | 1 |  |

### Redistribute DataSet

Split a mesh into partitions of nearly equal cell counts by cutting it in halves along its longest side, again and again (one block per partition), with layers of ghost cells if asked: work in pieces, or check how a mesh would be shared between processes.

Python: `redistribute_dataset` · also found by: partition, decompose, split mesh, domain decomposition, d3, balance, pieces, parallel

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Partitions | `partitions` | whole number 1 to 100000 | 4 |  |
| Ghost layers | `ghost_layers` | whole number 0 to 16 | 0 |  |

### Remove Ghost Information

Drop the ghost cells (vtkGhostType 1) and the ghost arrays.

Python: `remove_ghost_information` · also found by: ghost cells, remove ghosts, strip halo

### Rename Array

Give an array a new name.

Python: `rename_array` · also found by: rename, label

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| New name | `new_name` | text | `renamed` |  |

### Resample AMR

AMR data sampled on a uniform grid: each point takes the values of the finest cell holding it.

Python: `resample_amr` · also found by: amr, resample, uniform grid, regrid, adaptive mesh refinement, flatten levels

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Number of samples | `samples` | x, y, z | (10, 10, 10) |  |
| Whole domain | `whole_domain` | on/off | on | Sample the data's bounds; off to use Min and Max. |
| Min | `min` | x, y, z | (0, 0, 0) |  |
| Max | `max` | x, y, z | (1, 1, 1) |  |
| Transfer to nodes | `to_nodes` | on/off | on | Values at the grid points (point data); off for values at the cell centers (cell data). |

### Resample To Image

Interpolate any mesh onto a uniform grid (for volume-style analysis or export).

Python: `resample_to_image` · also found by: regrid, voxelize, interpolate, uniform grid

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| X samples | `nx` | whole number 1 to 4096 | 50 |  |
| Y samples | `ny` | whole number 1 to 4096 | 50 |  |
| Z samples | `nz` | whole number 1 to 4096 | 50 |  |

### Resample To Line

Sample the data along a straight line: at evenly spaced points, at the points where the line enters and leaves each cell, or once in the middle of each cell it crosses. Gives arc_length and the arrays there (for the Charts panel).

Python: `resample_to_line` · also found by: line probe, sample line, cell boundaries, cell centers, profile, plot over line

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Point 1 | `point1` | x, y, z | (NaN, NaN, NaN) | Unset: from the minimum corner of the data bounds. |
| Point 2 | `point2` | x, y, z | (NaN, NaN, NaN) | Unset: to the maximum corner of the data bounds. |
| Samples | `sampling` | Uniform / Cell boundaries / Cell centers | `Uniform` |  |
| Resolution (uniform) | `resolution` | whole number 1 to 1000000 | 1000 |  |

### Resample With Dataset

Sample this data's arrays at the points of another mesh (the destination).

Python: `resample_with_dataset` · also found by: probe, interpolate onto, map fields, transfer, sample on mesh, project

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Keep destination point arrays | `pass_point_arrays` | on/off | off |  |
| Keep destination cell arrays | `pass_cell_arrays` | on/off | off |  |

### Set Units

Say what units an array's values are in (K, m/s, MPa). The color legend, the Information panel and the spreadsheet show them; empty clears them.

Python: `set_units` · also found by: units, unit, dimension, physical, label, legend

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Units | `units` | text | (empty) |  |

### Spatio Temporal Harmonics

A sum of travelling sine waves at every point, changing with the input's time.

Python: `spatio_temporal_harmonics` · also found by: waves, sine, harmonics, synthetic, test data, animation

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Harmonics | `harmonics` | list of numbers | [1.0, 1.0, 0.6283, 0.6283, 0.6283, 0.0, 3.0, 1.0, 0.6283, 0.0, 0.0, 1.5708, 2.0, 2.0, 0.0, 0.6283, 0.0, 3.1416, 1.0, 3.0, 0.0, 0.0, 0.6283, 4.7124] | 6 numbers per wave: amplitude, temporal frequency, wave vector x y z, phase. Value = sum of amplitude * sin(frequency * time + wave vector . point + phase). |

### Surface Flow

The flux of a vector field through a surface (the integral of v·n over its area), as a one-row table with the flow and the area.

Python: `surface_flow` · also found by: flux, flow rate, integral, surface integral, mass flow

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `array` | array | (the active array) |  |

### Surface Vectors

A vector field on a surface made tangent to it (its normal part removed), or only its normal part kept.

Python: `surface_vectors` · also found by: tangent, project vectors, normal component, surface flow lines

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `array` | array | (the active array) |  |
| Keep | `keep` | Tangent part / Normal part | `Tangent part` |  |

### Table FFT

The discrete Fourier transform of every numeric column of a table (complex, and its magnitude), with the frequency of each row when a time step is given.

Python: `table_fft` · also found by: fft, fourier, spectrum, frequency, signal

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Time step (0: no frequency column) | `dt` | number 0 to 1000000000000 | 0 |  |

### Table To Points

The rows of a table as points: three columns give x, y and z (by default the columns named x, y, z, else the first three numeric ones); every column becomes point data.

Python: `table_to_points` · also found by: csv to points, point cloud, table, spreadsheet, xyz

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| X column | `x` | text | (empty) |  |
| Y column | `y` | text | (empty) |  |
| Z column | `z` | text | (empty) |  |
| 2D (z = 0) | `two_d` | on/off | off |  |

### Table To Structured Grid

The rows of a table as the points of a structured grid of the given dimensions (x fastest), positions from three columns.

Python: `table_to_structured_grid` · also found by: csv to grid, table, structured, curvilinear

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Dimensions | `dims` | list of numbers | [1.0, 1.0, 1.0] |  |
| X column | `x` | text | (empty) |  |
| Y column | `y` | text | (empty) |  |
| Z column | `z` | text | (empty) |  |

### Tensor Principal Invariants

Principal values (largest first), principal directions and the invariants I1, I2, I3, J2, J3 of a symmetric tensor array (6 components XX YY ZZ XY YZ XZ, or 9).

Python: `tensor_principal_invariants` · also found by: eigenvalues, principal stress, principal strain, eigenvectors, invariants, stress tensor

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Tensor | `array` | array | (the active array) |  |

### Texture Map to Cylinder

Texture coordinates around a cylinder: t along its axis, s around it.

Python: `texture_map_to_cylinder` · also found by: texture coordinates, uv, tcoords, cylindrical mapping, image on surface, wrap

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Automatic | `automatic` | on/off | on | Put the axis along the longest side of the data's oriented bounding box. |
| Point 1 | `point1` | x, y, z | (0, 0, 0) | Axis start (t = 0) when Automatic is off. |
| Point 2 | `point2` | x, y, z | (0, 0, 0) | Axis end (t = 1) when Automatic is off. |
| Prevent seam | `prevent_seam` | on/off | on | s goes 0 to 1 and back around the axis, so the texture shows twice without a seam. |

### Texture Map to Plane

Texture coordinates from a plane: fitted to the points, or through Origin, Point 1 and Point 2.

Python: `texture_map_to_plane` · also found by: texture coordinates, uv, tcoords, planar mapping, image on surface

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Automatic | `automatic` | on/off | on | Fit the plane to the points (Origin and Point 1 must stay at 0 0 0). |
| Origin | `origin` | x, y, z | (0, 0, 0) | Where s and t are 0. |
| Point 1 | `point1` | x, y, z | (0, 0, 0) | Where s is 1 (t = 0). |
| Point 2 | `point2` | x, y, z | (0, 0, 0) | Where t is 1 (s = 0). |

### Texture Map to Sphere

Texture coordinates on a sphere around the data: t from pole to pole (along z), s around z.

Python: `texture_map_to_sphere` · also found by: texture coordinates, uv, tcoords, spherical mapping, globe, earth texture, image on surface

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Center | `center` | x, y, z | (NaN, NaN, NaN) | *advanced* Leave empty to use the mean of the points. |
| Prevent seam | `prevent_seam` | on/off | on | s goes 0 to 1 and back around z, so the texture shows twice without a seam. Turn off for a world map. |

### Transpose Table

Rows become columns: each numeric column becomes a row, named in the column Column.

Python: `transpose_table` · also found by: transpose, table, swap rows columns, pivot

### Validate Cells

Check every cell: ValidityState is 0 for a good cell, otherwise the sum of 1 wrong number of points, 2 degenerate (no volume, area or length), 4 inverted (turned inside out), 8 non-planar face, 16 non-convex polygon, 32 repeated points; field data counts each problem.

Python: `validate_cells` · also found by: check mesh, bad cells, inverted, degenerate, negative jacobian, mesh errors, cell validator

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Tolerance (fraction of the cell's size) | `tolerance` | number 0 to 1 | 0.000000001 |  |

### Yield Criteria

From a stress tensor array (6 or 9 components): von Mises and Tresca equivalent stresses, the largest principal stress, the pressure and the stress triaxiality.

Python: `yield_criteria` · also found by: von mises, tresca, equivalent stress, principal stress, triaxiality, pressure, failure

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Stress tensor | `array` | array | (the active array) |  |

## Data Analysis

### Curvature

Gaussian or mean curvature of a triangle surface at its points (1/length², 1/length). Mean is positive where the surface is convex.

Python: `curvature` · also found by: gaussian curvature, mean curvature, bending, principal curvature, surface analysis

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Curvature | `kind` | Gaussian / Mean | `Gaussian` |  |

## Geometry

### Append Geometry

Combine several surface meshes into one polygonal mesh.

Python: `append_geometry` · also found by: append, combine, merge polydata, join surfaces

### Cell Centers

A point at the center of every cell, carrying the cell data.

Python: `cell_centers` · also found by: centroids, centers

### Clean

Merge duplicate points and remove unused ones.

Python: `clean` · also found by: merge points, duplicate, weld, tolerance

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Tolerance | `tolerance` | number 0 to ∞ | 0 |  |

### Clip Closed Surface

Cut a closed surface with a plane and close the cut with a flat cap, so the result is still a closed surface (watertight; holes in the cut, such as the inside of a ring, stay open in the cap).

Python: `clip_closed_surface` · also found by: capped clip, cap, close, watertight, cut solid, section

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Plane point | `origin` | x, y, z | (NaN, NaN, NaN) |  |
| Normal (the side kept) | `normal` | x, y, z | (1, 0, 0) |  |
| Keep the other side | `invert` | on/off | off |  |

### Connectivity

Label connected regions (RegionId) and optionally keep only one.

Python: `connectivity` · also found by: regions, islands, components, fragments, largest

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Keep | `extract` | text | `all` | 'all', 'largest' or a region number |

### Crop Points

Keep the points inside a box (with their point arrays), as vertices.

Python: `crop_points` · also found by: crop, box, points, point cloud, vertex geometry

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Box minimum | `min` | x, y, z | (-inf, -inf, -inf) |  |
| Box maximum | `max` | x, y, z | (inf, inf, inf) |  |

### Decimate

Reduce the triangles of a surface by collapsing the edges that change its shape least (quadric error), keeping its outline and volume.

Python: `decimate` · also found by: simplify, reduce triangles, quadric decimation, lod, mesh simplification, fewer triangles

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Reduction | `reduction` | number 0 to 0.99 | 0.5 | The fraction of the triangles to remove. |
| Keep the open edges | `preserve_boundary` | on/off | on |  |

### Delaunay 2D

Triangulate points seen from above (their x and y) so that no point lies inside another triangle's circle. An alpha value drops long thin triangles, leaving holes and concavities.

Python: `delaunay_2d` · also found by: triangulate points, tin, mesh from points, point cloud to surface, alpha shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Alpha (0: none) | `alpha` | number 0 to 1000000000 | 0 | Drops triangles whose circumcircle is larger than this radius. |

### Delaunay 3D

Join points into tetrahedra so that no point lies inside the sphere through another tetrahedron's corners, filling their convex hull. An alpha value drops the large tetrahedra, leaving concavities and gaps.

Python: `delaunay_3d` · also found by: tetrahedralize points, tet mesh from points, point cloud to volume, alpha shape, convex hull

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Alpha (0: none) | `alpha` | number 0 to 1000000000000 | 0 | Drops tetrahedra whose sphere has a larger radius than this. |

### Extract Block

Keep some blocks of a multi-block dataset (by name or index).

Python: `extract_block` · also found by: block, part, subset, element block, material

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Blocks | `blocks` | text | `0` | Comma-separated names or indices, e.g. 0,2 or Block 1 |

### Extract Edges

All unique cell edges as lines.

Python: `extract_edges` · also found by: wireframe, edges, mesh lines

### Extract Surface

Outer surface of a volume mesh as polygons.

Python: `extract_surface` · also found by: skin, boundary, faces, exterior

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Nonlinear subdivision level | `nonlinear_subdivision_level` | whole number 0 to 6 | 1 | *advanced* Curved (higher-order) cells: 0 uses only their corners, 1 goes through their nodes, each further level halves the pieces. |

### Feature Edges

Boundary and sharp edges of a surface.

Python: `feature_edges` · also found by: silhouette, crease, sharp, boundary edges

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature angle | `angle` | number 0 to 180 | 30 |  |
| Boundary edges | `boundary` | on/off | on |  |
| Sharp edges | `sharp` | on/off | on |  |

### Iterative Closest Point

Move a point set (this input) rigidly onto another (the second input): each round pairs every point with its nearest target point and finds the best rotation and translation, until it stops improving. Gives the moved points with the 4 x 4 transform and the RMS distance as field data.

Python: `iterative_closest_point` · also found by: icp, registration, align, rigid, point cloud, match

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Iterations | `iterations` | whole number 1 to 100000 | 100 |  |

### Linear Extrusion

Sweep lines into strips of quads, or polygons into closed prisms, along a vector.

Python: `linear_extrusion` · also found by: extrude, sweep, prism, thicken, 2d to 3d

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vector | `vector` | x, y, z | (0, 0, 1) | The direction and length of the sweep. |
| Scale factor | `scale` | number -1000000000 to 1000000000 | 1 | Multiplies the vector. |
| Cap the ends (polygons) | `capping` | on/off | on |  |

### Mask Points

Keep a subset of points (every Nth, at most a maximum count).

Python: `mask_points` · also found by: subsample, decimate points, thin

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Every Nth | `stride` | whole number 1 to 1000000 | 2 |  |
| Maximum points | `max_points` | whole number 1 to 100000000 | 5000 |  |

### Merge Blocks

Combine all blocks of a multi-block dataset into one mesh.

Python: `merge_blocks` · also found by: append, combine, flatten

### Outline

Bounding box of the data as lines.

Python: `outline` · also found by: bounding box, bbox, frame

### Point Sample Triangle Geometry

Points spread over a surface at random, each triangle getting points in proportion to its area (seeded): a point cloud of a surface, for Iterative Closest Point or comparisons.

Python: `point_sample_surface` · also found by: sample surface, point cloud, random points, surface points, sampling

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Points | `samples` | whole number 1 to 100000000 | 10000 |  |
| Random seed | `seed` | whole number 0 to any | 1 |  |

### Reflect

Mirror the data across a plane (symmetric models).

Python: `reflect` · also found by: mirror, symmetry

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Plane | `plane` | X / Y / Z | `X` |  |
| Position | `position` | number -∞ to ∞ | 0 |  |
| Keep original | `copy_input` | on/off | on |  |

### Reverse Sense

Turn a surface inside out: every polygon's vertex order is reversed (so its faces look the other way) and, optionally, the sign of its normals.

Python: `reverse_sense` · also found by: flip, invert normals, orientation, inside out, winding, reverse normals

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Flip the normals arrays too | `normals` | on/off | on |  |

### Ribbon

Make a strip of the given width along every polyline, facing a chosen direction.

Python: `ribbon` · also found by: strip, band, streamline ribbon, flat tube

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Width | `width` | number 0 to 1000000000 | 0.1 |  |
| Faces toward | `normal` | x, y, z | (0, 0, 1) |  |

### Rotational Extrusion

Sweep lines (or the boundary of polygons) round an axis: a surface of revolution, a screw or a spiral.

Python: `rotational_extrusion` · also found by: revolve, lathe, surface of revolution, spin, sweep, helix

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Angle (degrees) | `angle` | number -3600 to 3600 | 360 |  |
| Steps | `resolution` | whole number 1 to 10000 | 36 |  |
| Axis | `axis` | x, y, z | (0, 0, 1) |  |
| A point on the axis | `origin` | x, y, z | (0, 0, 0) |  |
| Translation along the axis | `translation` | number -1000000000 to 1000000000 | 0 | How far the sweep moves along the axis over the whole angle (a screw). |
| Change of radius per turn | `delta_radius` | number -1000 to 1000 | 0 | The radius grows by this fraction per turn (a spiral). |

### Shrink

Shrink cells toward their centers to see the mesh structure.

Python: `shrink` · also found by: explode, cells, mesh

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Factor | `factor` | number 0 to 1 | 0.8 |  |

### Smooth

Laplacian smoothing of a surface.

Python: `smooth` · also found by: laplacian, fair, denoise

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Iterations | `iterations` | whole number 1 to 10000 | 20 |  |
| Relaxation | `relaxation` | number 0 to 1 | 0.1 |  |

### Subdivide

Split every triangle of a surface into four, again and again. Linear keeps the shape; Loop rounds it to a smooth surface.

Python: `subdivide` · also found by: loop subdivision, refine surface, smooth surface, linear subdivision, finer surface

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Scheme | `scheme` | Linear / Loop | `Linear` |  |
| Levels | `levels` | whole number 0 to 6 | 1 |  |

### Surface Normals

Compute normals of a surface (adds a Normals array).

Python: `surface_normals` · also found by: normals, orientation, shading

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Flip normals | `flip` | on/off | off |  |
| Also cell normals | `cell_normals` | on/off | off |  |

### Tensor Glyph

An ellipsoid at points of a tensor field, its axes along the principal directions and as long as the principal values (absolute) times a scale; colored by the largest principal value.

Python: `tensor_glyph` · also found by: tensor, ellipsoid, stress glyph, principal directions, glyph

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Tensor | `array` | array | (the active array) |  |
| Scale | `scale` | number 0 to 1000000000000 | 1 |  |
| Every n-th point | `stride` | whole number 1 to 1000000 | 1 |  |
| Ellipsoid resolution | `resolution` | whole number 6 to 128 | 16 | *advanced* |

### Tessellate

Replace curved (higher-order Lagrange, Bezier and quadratic) cells by linear cells that follow their shape.

Python: `tessellate` · also found by: higher order, lagrange, bezier, quadratic, curved, linearize, subdivide, nonlinear

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Subdivision level | `level` | whole number 1 to 6 | 2 | 1 places one linear cell between neighbouring nodes; each further level halves them (8x the cells in 3D). |

### Tetrahedralize

Split every cell into tetrahedra.

Python: `tetrahedralize` · also found by: tets, simplices

### Transform

Translate, rotate and scale the data.

Python: `transform` · also found by: move, rotate, scale, translate, affine

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Translate | `translate` | x, y, z | (0, 0, 0) |  |
| Rotate (deg) | `rotate` | x, y, z | (0, 0, 0) |  |
| Scale | `scale` | x, y, z | (1, 1, 1) |  |

### Triangulate

Split surface polygons into triangles.

Python: `triangulate` · also found by: triangles, polygons

### Tube

Turn lines (e.g. streamlines) into 3D tubes.

Python: `tube` · also found by: pipe, thick lines, streamtubes, cylinder

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Radius | `radius` | number 0 to ∞ | NaN | Unset: 0.5% of the data size. |
| Sides | `sides` | whole number 3 to 64 | 12 |  |
| Vary radius by | `scale_by` | array | (the active array) | *advanced* |

## Grains

### Add Orientation Noise

Turn every point's orientation (or every feature's) by a small random rotation with a normally distributed angle.

Python: `orientation_noise` · also found by: noise, perturb orientations, scatter, synthetic, orientation spread

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Angle (degrees, standard deviation) | `sigma` | number 0 to 180 | 2 |  |
| One rotation per feature | `per_feature` | on/off | off |  |
| Orientations | `orientations` | array | (the active array) | *advanced* Empty: EulerAngles, Quats or OrientationMatrix. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Adjust Volume

Move cells on feature boundaries between neighbouring features so that each feature's size comes closer to a target: a per-feature field array of target cell counts or equivalent diameters (scaled to the volume).

Python: `adjust_volume` · also found by: adjust volume, target size, grain size distribution, synthetic, packing

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Target array (per feature) | `target` | text | `GoalEquivalentDiameters` |  |
| Sweeps | `sweeps` | whole number 1 to 10000 | 20 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Align Sections

Move the slices of a serial-section volume in x and y so each matches the one below, by orientation or by feature ids, or by given shifts.

Python: `align_sections` · also found by: serial sectioning, registration, align, slices, shift, 3d ebsd, stack

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Match by | `method` | Misorientation / Feature ids / Given shifts | `Misorientation` | Misorientation: cells match when their orientations differ by less than the tolerance. Feature ids: cells match when they belong to the same feature. Given shifts: move each slice by the numbers below. |
| Tolerance (degrees) | `tolerance` | number 0 to 180 | 5 |  |
| Largest shift (cells) | `max_shift` | whole number 0 to 10000 | 20 | The largest move of one slice against the one below, in cells along x and y. |
| Mask | `mask` | array | (the active array) | Only cells where this array is not 0 are compared (for example a good-data or sample mask). |
| Orientations | `orientations` | array | (the active array) | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |
| Phases | `phases` | array | (the active array) | *advanced* |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Given shifts | `shifts` | list of numbers | [] | *advanced* x and y cells for each slice after the first, against the slice below (Given shifts only). |

### Apply Transformation to Geometry

Scale, turn and move the grid of a volume: without a turn only the origin and spacing change; with one, the cells are resampled onto a grid around the turned sample (nearest cell).

Python: `apply_transformation_to_geometry` · also found by: transform, translate, scale, rotate, geometry, move grid

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Translation | `translation` | x, y, z | (0, 0, 0) |  |
| Rotation axis | `axis` | x, y, z | (0, 0, 1) |  |
| Rotation angle (degrees) | `angle` | number -360 to 360 | 0 |  |
| Scale | `scale` | x, y, z | (1, 1, 1) |  |
| 4 x 4 matrix (row by row; empty: the values above) | `matrix` | list of numbers | [] | *advanced* |

### Average Orientations

The average crystal orientation of every feature (AvgQuats, AvgEulerAngles).

Python: `average_orientations` · also found by: mean orientation, grain orientation, avg quats, texture

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Orientations | `orientations` | array | (the active array) | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |
| Phases | `phases` | array | (the active array) | *advanced* |

### Bad Data Neighbor Orientation Check

A bad cell (mask off) with enough good neighbours of nearly its own orientation becomes good; from 6 neighbours down to the number given.

Python: `bad_data_neighbor_orientation_check` · also found by: bad data, mask, orientation check, clean up, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Mask (good cells) | `mask` | text | `Mask` |  |
| Misorientation tolerance (degrees) | `tolerance` | number 0 to 180 | 5 |  |
| Required neighbours | `neighbors` | whole number 1 to 6 | 4 |  |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### Change Resolution

Resample a voxel volume or EBSD map onto larger or smaller cells, each taking the value of the old cell at its centre.

Python: `change_resolution` · also found by: resample, resolution, voxel size, coarsen, refine, spacing, downsample, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| New cell size | `spacing` | x, y, z | (1, 1, 1) |  |
| Renumber features | `renumber_features` | on/off | on | Number the remaining feature ids from 1 and keep only their rows of the per-feature field arrays. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Chord Lengths

Grain-boundary chord lengths along lines in x, y and z of a map or volume, as a list or a histogram, with their means.

Python: `chord_lengths` · also found by: chord length, linear intercept, grain size, intercept length, line intercept, grain boundary spacing

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Boundary angle (degrees) | `threshold` | number 0.1 to 180 | 10 |  |
| Line spacing | `spacing` | number 0 to ∞ | 0 | Distance between the lines in length units; 0: every row of points. |
| Output | `output` | Chords / Histogram | `Chords` |  |
| Histogram bin (length) | `bin` | number 0 to ∞ | 0 | 0: a thirtieth of the longest chord. |
| Largest gap (points) | `max_gap` | whole number 0 to 1000 | 3 | *advanced* Runs of more unused points end a line segment; chords touching a segment end are left out. |
| Ignore single-point specks | `despeckle` | on/off | off | *advanced* |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |

### Convert Orientations

Convert orientations between Euler angles, quaternions, orientation matrices, axis-angle, Rodrigues and homochoric vectors, optionally reduced to the fundamental zone.

Python: `convert_orientations` · also found by: euler, quaternion, rodrigues, homochoric, axis angle, orientation matrix, fundamental zone, crystal

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Orientations | `orientations` | array | (the active array) |  |
| Input | `input_type` | Euler angles / Quaternions / Orientation matrices / Axis-angle / Rodrigues vectors / Homochoric vectors | `Euler angles` |  |
| Output | `output_type` | Euler angles / Quaternions / Orientation matrices / Axis-angle / Rodrigues vectors / Homochoric vectors | `Quaternions` |  |
| Reduce to the fundamental zone | `reduce` | on/off | off | Replace each orientation by its symmetry equivalent with the smallest rotation angle. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phases | `phases` | array | (the active array) | *advanced* |
| Output name | `name` | text | (empty) | *advanced* Empty: EulerAngles, Quats, OrientationMatrix, AxisAngles, Rodrigues or Homochoric. |

### Copy Feature Array to Cells

Give every cell the value its feature has in a per-feature (field) array, such as each grain's equivalent diameter, to color the cells by it.

Python: `copy_feature_array_to_cells` · also found by: feature to cell, per grain, field to cell, copy, grain size map

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature array | `array` | text | `EquivalentDiameters` |  |
| Result name (empty: the same) | `result` | text | (empty) |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Crop

Keep the cells of a voxel volume or EBSD map inside a box (sample coordinates), renumbering the features that remain.

Python: `crop_volume` · also found by: crop, box, region, subvolume, trim, voxels, ebsd, features

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Box minimum | `min` | x, y, z | (NaN, NaN, NaN) | Cells whose centres lie at or above these coordinates are kept; leave unset for an open side. |
| Box maximum | `max` | x, y, z | (NaN, NaN, NaN) | Cells whose centres lie at or below these coordinates are kept; leave unset for an open side. |
| Renumber features | `renumber_features` | on/off | on | Number the remaining feature ids from 1 and keep only their rows of the per-feature field arrays. |
| Box in | `units` | Coordinates / Cell indices | `Coordinates` | *advanced* Cell indices: the first and last cell kept along x, y and z, counted from 0. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Crystal Mechanics

Schmid factor, Taylor factor and directional Young's modulus of every orientation for a loading direction, from a material's slip systems and elastic constants, with volume means and a yield-strength estimate.

Python: `crystal_mechanics` · also found by: schmid factor, taylor factor, youngs modulus, elastic anisotropy, slip systems, yield strength, crystal plasticity, fcc bcc hcp

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Material | `material` | IN718 / CoNi GammaPrint-700 / Nickel / Copper / Aluminium / Iron (BCC) / Titanium (HCP) / Magnesium (HCP) / Custom | `IN718` |  |
| Loading direction | `direction` | x, y, z | (0, 0, 1) |  |
| Taylor factor | `taylor` | on/off | on | The full-constraint Taylor factor (slower: every set of five independent slip systems is tried). |
| Dislocation density (1/m2) | `gnd` | array | (the active array) | *advanced* Optional: its mean adds the Taylor hardening alpha mu b M sqrt(rho) to the yield estimate. |
| Custom: lattice | `lattice` | FCC / BCC / HCP | `FCC` | *advanced* |
| Custom: elastic constants (GPa) | `elastic` | list of numbers | [250.0, 160.0, 120.0] | *advanced* Cubic: C11 C12 C44. Hexagonal: C11 C12 C13 C33 C44. |
| Custom: shear modulus (GPa) | `shear_modulus` | number 0 to ∞ | 80 | *advanced* |
| Custom: Burgers vector (m) | `burgers` | number 0 to ∞ | 0.00000000025 | *advanced* |
| Custom: c/a | `c_over_a` | number 0.1 to 10 | 1.633 | *advanced* |
| Custom: slip families | `families` | text | (empty) | *advanced* Like 'Basal <a>=1, Prismatic <a>=0.7, Pyramidal <c+a>=3' (relative resolved shear stresses). Empty: the lattice's first family. |
| Custom: base yield strength (MPa) | `sigma_base` | number 0 to ∞ | 0 | *advanced* |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |

### Distance to Boundaries

Each cell's distance to the nearest grain boundary, triple line and quadruple point.

Python: `distance_to_boundaries` · also found by: euclidean distance map, distance, boundary, triple junction, quadruple point

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Erode Dilate Bad Data

Grow the bad cells (feature 0) into the features (dilate) or the features into the bad cells (erode: each bad cell next to a feature takes, with every cell array, the values of a neighbour of the most common feature), a number of times, along the axes switched on.

Python: `erode_dilate_bad_data` · also found by: erode, dilate, bad data, clean up, morphology, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Operation | `operation` | Erode / Dilate | `Erode` |  |
| Iterations | `iterations` | whole number 1 to 1000 | 2 |  |
| Along x | `x` | on/off | on |  |
| Along y | `y` | on/off | on |  |
| Along z | `z` | on/off | on |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Erode Dilate Coordination Number

Smooth feature boundaries: a cell with at least the coordination number of face neighbours in one other feature joins it (taking its values), repeated until nothing changes if asked.

Python: `erode_dilate_coordination_number` · also found by: coordination number, smooth boundaries, clean up, jagged, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Coordination number | `coordination` | whole number 1 to 6 | 6 |  |
| Repeat until nothing changes | `repeat` | on/off | off |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Face Orientation Colors

Colour boundary triangles by the inverse pole figure key of their plane in the crystal, and by the misorientation across them.

Python: `face_orientation_colors` · also found by: face ipf, boundary plane, misorientation colors, grain boundary character, colour boundaries

### Feature Boundaries

The faces between features as a surface, with the misorientation across them and twin boundaries marked.

Python: `feature_boundaries` · also found by: grain boundaries, boundary map, twin boundaries, interfaces, misorientation map

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Include the outer surface | `outer` | on/off | off | Also the faces against cells without a feature and the edge of the volume. |
| Twin axis tolerance (degrees) | `axis_tolerance` | number 0 to 90 | 3 | *advanced* |
| Twin angle tolerance (degrees) | `angle_tolerance` | number 0 to 60 | 2 | *advanced* |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Orientations | `orientations` | array | (the active array) | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |
| Phases | `phases` | array | (the active array) | *advanced* |

### Feature Centroids

The centre (mean of the cell centres) of every feature.

Python: `feature_centroids` · also found by: centroid, grain centre, position, statistics

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Feature Colors

A distinct color for every feature (FeatureColors, drawn as its own colors).

Python: `feature_colors` · also found by: grain map, random colors, feature map, label colors

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Feature Neighbors

Neighbour counts, features on the surface, boundary cells and the mean misorientation to neighbouring features.

Python: `feature_neighbors` · also found by: neighbors, boundary cells, surface features, grain boundary, contiguity

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Feature Shapes

Axis lengths, aspect ratios, axis directions and Omega3 of every feature, from its moments of inertia.

Python: `feature_shapes` · also found by: shape, aspect ratio, ellipsoid, axis lengths, omega3, elongation, statistics

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Feature Sizes

Cells, volume and equivalent sphere (or circle) diameter of every feature.

Python: `feature_sizes` · also found by: grain size, volume, equivalent diameter, esd, statistics

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Feature Surfaces

The closed surface of each feature of a boundary surface, facing outwards, one block per feature (save as STL for one file each).

Python: `feature_surfaces` · also found by: stl per grain, grain surfaces, export grains, closed surface, watertight, per feature

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Features (empty: all) | `features` | list of numbers | [] |  |

### Feature Table

The per-feature results as a table, one row per feature, for the spreadsheet, charts and CSV files.

Python: `feature_table` · also found by: table, statistics, csv, spreadsheet, grain list, histogram

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Fill Bad Data

Fill small clusters of cells without a feature from the surrounding features (every cell array copied from a neighbour).

Python: `fill_bad_data` · also found by: clean up, fill, defects, holes, unindexed, noise, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Smallest kept defect (cells) | `min_defect` | whole number 1 to 1000000000 | 7 | Clusters of cells without a feature with fewer cells than this are filled. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Find Boundary Strengths

Slip transmission across every boundary between two grains for a load direction: m' (|cos psi cos kappa|, plane normals and slip directions) of the two systems with the highest Schmid factors, the largest m' of any pair (F1), m' weighted by both Schmid factors (F1spt) and the best slip direction alignment (F7); a table with a row per grain pair.

Python: `find_boundary_strengths` · also found by: slip transmission, m prime, luster morris, boundary strength, f1, f7, schmid

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Load direction | `direction` | x, y, z | (0, 0, 1) |  |
| Slip systems | `lattice` | Auto / BCC | `Auto` |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### Find C-Axis Locations

The c axis of every hexagonal cell in the sample frame (cell array CAxisLocation, unit vectors in the upper hemisphere).

Python: `find_c_axis_locations` · also found by: c axis, hexagonal, titanium, hcp, basal pole

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### Find Feature Clustering

The radial distribution function of feature centres (of one phase, or all): how many centres lie at each distance from each other, over what a random arrangement of the same density gives. A table of r and RDF for the Charts panel.

Python: `find_feature_clustering` · also found by: rdf, radial distribution, clustering, precipitates, pair correlation

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Bins | `bins` | whole number 1 to 10000 | 40 |  |
| Largest distance (0 = half the smallest side) | `max_distance` | number 0 to ∞ | 0 |  |
| Phase (0 = all) | `phase` | whole number 0 to 1000 | 0 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Find Feature Face Curvature

The mean and Gaussian curvature of every grain boundary of a surface mesh (made by Quick Surface Mesh), each boundary measured on its own, per triangle.

Python: `feature_face_curvature` · also found by: curvature, grain boundary, mean curvature, gaussian curvature, surface mesh

### Find Feature Reference C-Axis Misorientations

The angle between each hexagonal cell's c axis and its feature's mean c axis (cell array, degrees), and its mean per feature.

Python: `find_feature_reference_c_axis_misorientations` · also found by: c axis, misorientation, hexagonal, hcp, intragranular

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### Find Largest Cross Sections

The largest area each feature covers in one slice normal to an axis (field array LargestCrossSections).

Python: `find_largest_cross_sections` · also found by: cross section, area, slice area, grain size

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Slices normal to | `axis` | X / Y / Z | `Z` |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Find Neighborhoods

For every feature, how many other features have their centre within some multiple of the mean equivalent diameter of its centre (field array Neighborhoods).

Python: `find_neighborhoods` · also found by: neighborhood, neighbours, clustering, feature density

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Multiples of the mean diameter | `multiples` | number 0 to 100 | 1 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Find Surface Area to Volume

Each feature's surface area (its cell faces against other features and the outside), its ratio to the volume and its sphericity, as per-feature field arrays.

Python: `find_surface_area_to_volume` · also found by: surface area, volume, sphericity, shape, ratio

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Find Triangle Angles

The smallest and largest interior angle of each triangle of a surface mesh and the largest dihedral angle with its neighbours on the same boundary (cell arrays, degrees): mesh quality of grain surfaces.

Python: `triangle_angles` · also found by: triangle quality, minimum angle, dihedral angle, surface mesh

### Find Twin Boundary Schmid Factors

For every twin boundary between cubic grains (60 degrees about a shared <111>), the Schmid factors of the three <110> slip directions in the twin plane, in each grain, for a load direction.

Python: `find_twin_boundary_schmid_factors` · also found by: twin, schmid factor, annealing twin, sigma 3

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Load direction | `direction` | x, y, z | (0, 0, 1) |  |
| Axis tolerance (degrees) | `axis_tolerance` | number 0 to 45 | 3 |  |
| Angle tolerance (degrees) | `angle_tolerance` | number 0 to 45 | 3 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### GBCD Section

The grain boundary character distribution for one misorientation: how often each boundary plane occurs, in multiples of random, as a pole figure of crystal directions.

Python: `gbcd_section` · also found by: gbcd, grain boundary character distribution, boundary planes, sigma 3, twin boundaries, five parameter, mrd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Misorientation angle (degrees) | `angle` | number 0 to 180 | 60 |  |
| Misorientation axis (crystal) | `axis` | x, y, z | (1, 1, 1) |  |
| Tolerance (degrees) | `tolerance` | number 0.5 to 30 | 5 |  |
| Smoothing (degrees) | `sigma` | number 0.5 to 45 | 5 |  |
| Projection | `projection` | Stereographic / Equal area | `Stereographic` |  |
| Resolution | `resolution` | whole number 4 to 400 | 40 | *advanced* |

### GND Density

Density of geometrically necessary dislocations in every cell (1/m2): from Nye's tensor of the lattice curvature inside each grain, or from a kernel average misorientation.

Python: `gnd_density` · also found by: gnd, geometrically necessary dislocations, nye tensor, dislocation density, lattice curvature, kam, stored energy

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Method | `method` | Nye tensor / From KAM | `Nye tensor` |  |
| Burgers vector (m) | `burgers` | number 0.000000000001 to 0.00000001 | 0.000000000254 |  |
| Kernel (cells) | `kernel` | whole number 3 to 15 | 3 | Nye tensor: the curvature is fitted over this many cells across (odd). |
| KAM array (degrees) | `kam` | array | (the active array) | *advanced* |
| Step for KAM (micrometres) | `step` | number 0 to ∞ | 0 | *advanced* 0: the smallest cell size. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Generate Ensemble Statistics

Fit the grain size (lognormal), shape (beta fits of the aspect ratios b/a and c/a) and neighbour count distributions of a segmented microstructure (a table).

Python: `ensemble_statistics` · also found by: statistics, grain size distribution, lognormal, beta, aspect ratio, neighbors, stats generator, fit

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Include features on the surface | `include_surface` | on/off | off |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Generate Precipitate Statistics

Statistics of the features of one phase (precipitates): count, volume fraction, number density, the lognormal fit of their equivalent diameters and the mean distance to the nearest other precipitate, as a table.

Python: `precipitate_statistics` · also found by: precipitates, second phase, statistics, volume fraction, number density, particles

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Phase | `phase` | whole number 0 to 1000 | 2 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Identify Micro-Texture Regions

Join neighbouring hexagonal features whose mean c axes line up (within a tolerance) into micro-texture regions: MTRIds per cell, ParentIds per feature, IsMTR for regions of at least a number of features.

Python: `identify_micro_texture_regions` · also found by: micro texture, mtr, macrozone, c axis, titanium, hcp

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| C-axis tolerance (degrees) | `tolerance` | number 0 to 90 | 15 |  |
| Fewest features in a region | `min_features` | whole number 2 to 100000 | 2 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### Identify Sample

Find the sample among the scanned cells: the largest connected region of good cells (the mask) is kept and every other good cell marked bad; with fill holes, bad regions enclosed by the sample become good.

Python: `identify_sample` · also found by: sample, mask, largest region, background, fill holes, ebsd clean up

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Mask (good cells) | `mask` | text | `Mask` |  |
| Fill holes | `fill_holes` | on/off | off |  |

### Insert Precipitates

Insert particles of a new phase (lognormal sizes, apart from one another) into a microstructure until they fill a volume fraction, at random or on grain boundaries.

Python: `insert_precipitates` · also found by: precipitates, second phase, particles, inclusions, carbides, synthetic

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Volume fraction | `fraction` | number 0 to 0.9 | 0.05 |  |
| Size mu (log of diameter) | `esd_mu` | number -∞ to ∞ | 1 | The diameters are lognormal: mean of their logarithms. |
| Size sigma | `esd_sigma` | number 0 to 3 | 0.2 |  |
| On grain boundaries | `at_boundaries` | on/off | off |  |
| Crystal structure | `structure` | Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `Cubic m-3m` |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Insert Transformation Phases

Put plates (such as annealing twins) into grains: in a fraction of the grains of a parent phase, a plate of a given thickness through the grain, on one of its {111} planes, becomes a new feature of a new phase, turned from its parent by an angle about the plane's normal (60 degrees: a twin).

Python: `insert_transformation_phases` · also found by: twins, plates, transformation, lamellae, synthetic, annealing twins

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Parent phase (0 = all) | `parent_phase` | whole number 0 to 1000 | 1 |  |
| New phase | `new_phase` | whole number 1 to 1000 | 2 |  |
| Fraction of grains | `fraction` | number 0 to 1 | 0.5 |  |
| Thickness (cells) | `thickness` | number 0.5 to 1000 | 2 |  |
| Turn about the plane normal (degrees) | `angle` | number -180 to 180 | 60 |  |
| Random seed | `seed` | whole number 0 to any | 1 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Inverse Pole Figure

Inverse pole figures: the density (multiples of random) of the crystal directions along sample X, Y or Z, on the standard triangle of the crystal class.

Python: `inverse_pole_figure` · also found by: texture, ipf density, inverse pole figure, mrd, fiber texture, standard triangle, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Sample directions | `directions` | X, Y and Z / X / Y / Z | `X, Y and Z` |  |
| Projection | `projection` | Stereographic / Equal area | `Stereographic` |  |
| Smoothing (degrees) | `sigma` | number 0.5 to 45 | 5 | Width of the Gaussian kernel. |
| Resolution | `resolution` | whole number 4 to 400 | 40 | *advanced* |
| Bin size (degrees) | `bin` | number 0.25 to 10 | 1 | *advanced* |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |
| Orientations used at most | `max_orientations` | whole number 100 to 10000000 | 60000 | *advanced* A random subset (fixed seed) when there are more. |

### Isolate Largest Feature

Keep only the feature with the most cells (feature 1); every other cell gets feature 0.

Python: `isolate_largest_feature` · also found by: largest grain, keep one feature, isolate

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Jumble Orientations

Swap the orientations of the features at random: the same set of grain orientations, placed on other grains.

Python: `jumble_orientations` · also found by: shuffle orientations, randomize, uncorrelated, synthetic, permute

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Orientations | `orientations` | array | (the active array) | *advanced* Empty: EulerAngles, Quats or OrientationMatrix. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Kernel Average Misorientation

Each cell's mean misorientation to the cells of its feature around it (KAM), a measure of local lattice curvature.

Python: `kernel_average_misorientation` · also found by: kam, local misorientation, deformation, dislocation, strain, gnd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Kernel (cells each way) | `kernel` | x, y, z | (1, 1, 1) | How many cells either side along x, y and z (1 1 1: the 26 neighbours). |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Orientations | `orientations` | array | (the active array) | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |
| Phases | `phases` | array | (the active array) | *advanced* |

### Match Crystallography

Give the grains orientations drawn from a reference microstructure, then swap them until the misorientations across grain boundaries are distributed as in the reference.

Python: `match_crystallography` · also found by: match texture, match mdf, orientation assignment, synthetic, misorientation distribution, odf

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Moves per grain | `moves` | whole number 1 to 100000 | 50 | *advanced* |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Merge Twins

Give twin-related neighbouring features (cubic, 60 degrees about <111>) a common parent id.

Python: `merge_twins` · also found by: twins, sigma 3, annealing twins, parent grains, fcc

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Axis tolerance (degrees) | `axis_tolerance` | number 0 to 90 | 3 |  |
| Angle tolerance (degrees) | `angle_tolerance` | number 0 to 60 | 2 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Minimum Size

Remove features with fewer cells than a minimum, filling them from their neighbours, and renumber the rest.

Python: `minimum_size` · also found by: clean up, small grains, minimum size, remove, noise

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Fewest cells | `min_cells` | whole number 1 to 1000000000 | 7 |  |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Misorientation Distribution

The distribution of disorientation angles between neighbouring points, between random pairs of points and between random orientations (a table for the Charts panel).

Python: `misorientation_distribution` · also found by: mdf, misorientation distribution, disorientation angle, grain boundary angles, mackenzie, random distribution, boundary statistics

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Smallest angle (degrees) | `min_angle` | number 0 to 179 | 5 | Angles below it (inside grains) are left out of every curve. |
| Bin size (degrees) | `bin` | number 0.1 to 30 | 1 |  |
| Random pairs | `pairs` | whole number 1000 to 100000000 | 100000 | *advanced* |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |

### Neighbor Orientation Correlation

A cell whose confidence is below a minimum takes the values of the neighbour that most other neighbours agree with in orientation (misorientation below a tolerance), when enough of them agree; from 6 agreeing neighbours down to 6 minus the level.

Python: `neighbor_orientation_correlation` · also found by: orientation correlation, clean up, noise, low confidence, ebsd, misorientation

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Confidence array | `confidence` | text | `Confidence Index` |  |
| Minimum confidence | `min_confidence` | number -∞ to ∞ | 0.2 |  |
| Misorientation tolerance (degrees) | `tolerance` | number 0 to 180 | 5 |  |
| Cleanup level | `level` | whole number 0 to 6 | 2 |  |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |

### ODF Sections

The orientation distribution function on sections of constant phi2 (phi1 across, Phi down, degrees), with the texture index and the ODF maximum.

Python: `odf_sections` · also found by: texture, odf, orientation distribution function, euler space, texture index, texture strength, phi2 sections, mrd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Sections (phi2, degrees) | `sections` | text | `0, 45, 65` |  |
| Step (degrees) | `step` | number 0.5 to 30 | 5 |  |
| Smoothing (degrees) | `sigma` | number 1 to 45 | 7.5 | Width of the Gaussian kernel on the disorientation angle. |
| Texture index step (degrees) | `index_step` | number 1 to 30 | 5 | *advanced* |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |
| Orientations used at most | `max_orientations` | whole number 100 to 10000000 | 10000 | *advanced* A random subset (fixed seed) when there are more. |

### Orientation Colors

Color crystal orientations by the inverse pole figure key or by their Euler angles (an 8-bit RGB array drawn as its own colors).

Python: `orientation_colors` · also found by: ipf, inverse pole figure, ebsd, orientation map, grain, crystal, euler colors, texture

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Orientations | `orientations` | array | (the active array) | Euler angles (3 components, radians), quaternions (4, stored x y z w) or orientation matrices (9). Empty: EulerAngles, Quats or OrientationMatrix. |
| Color key | `key` | Inverse pole figure / Euler angles | `Inverse pole figure` |  |
| Reference direction | `direction` | x, y, z | (0, 0, 1) | The sample direction whose crystal direction is colored (z: the map normal). |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | From the phases: each point's class comes from its phase and the CrystalStructures field array, as EBSD files give them. |
| Phases | `phases` | array | (the active array) | *advanced* The phase of each point (0: not indexed). Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are black. |
| Output name | `name` | text | `IPFColor` | *advanced* |

### Pack Like

A new microstructure packed with the grain size, shape and orientation statistics measured on the input (a synthetic copy of a measured volume).

Python: `pack_like` · also found by: synthetic copy, statistically equivalent, rve, pack grains, reconstruct, ensemble statistics

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cells | `cells` | list of numbers | [64.0, 64.0, 64.0] |  |
| Cell size | `spacing` | number 0 to ∞ | 0 | 0: the input's. |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Pole Figure

Pole figures of crystal directions (densities in multiples of random) as disks, one per family, sample X right and Y up.

Python: `pole_figure` · also found by: texture, pole figure, pf, mrd, stereographic, equal area, ebsd, crystallographic texture, 100 110 111

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Directions | `families` | text | `100, 110, 111` | Crystal directions, like 100, 110, -111 or 1 1 2, separated by commas; every symmetry equivalent is included. |
| Projection | `projection` | Stereographic / Equal area | `Stereographic` |  |
| Smoothing (degrees) | `sigma` | number 0.5 to 45 | 7.5 | Width of the Gaussian kernel. |
| Resolution | `resolution` | whole number 4 to 400 | 40 | *advanced* |
| Bin size (degrees) | `bin` | number 0.25 to 10 | 2 | *advanced* |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |
| Orientations used at most | `max_orientations` | whole number 100 to 10000000 | 60000 | *advanced* A random subset (fixed seed) when there are more. |

### Quick Surface Mesh

Triangles on every voxel face between different features, with the features on each side (FaceLabels) and the number of features at each node (NodeType).

Python: `quick_surface_mesh` · also found by: grain boundary mesh, surface mesh, boundaries, triangles, stl, interfaces, voxel surface, face labels

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Reference Misorientations

Each cell's misorientation from its feature's average orientation, and the mean per feature.

Python: `reference_misorientations` · also found by: grain reference orientation deviation, gros, misorientation, deformation, intragranular

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |
| Orientations | `orientations` | array | (the active array) | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |
| Phases | `phases` | array | (the active array) | *advanced* |

### Replace Element Attributes with Neighbor

Neighbour confidence correlation: every cell whose confidence array is below a minimum (or above, if asked) takes every cell array of its neighbour with the best confidence; repeated until nothing changes if asked.

Python: `replace_element_attributes_with_neighbor` · also found by: confidence index, neighbor ci correlation, clean up, low confidence, replace, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Confidence array | `confidence` | text | `Confidence Index` |  |
| Minimum | `min` | number -∞ to ∞ | 0.1 |  |
| Replace values above instead | `greater` | on/off | off |  |
| Repeat until nothing changes | `repeat` | on/off | on |  |

### Resample Rectilinear To Image

Put a rectilinear grid (unevenly spaced lines) on an evenly spaced image: cells of the smallest spacing along each axis (or a given one), each taking the values of the cell it falls in.

Python: `resample_rectilinear_to_image` · also found by: rectilinear, uniform grid, resample, image data, regular grid

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Spacing (0 = the smallest step) | `spacing` | x, y, z | (0, 0, 0) |  |

### Rotate Orientations

Re-express orientations after turning the sample or crystal reference frame by an angle about an axis (the data does not move).

Python: `rotate_orientations` · also found by: reference frame, sample frame, crystal frame, euler, rotate, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Orientations | `orientations` | array | (the active array) |  |
| Frame | `frame` | Sample / Crystal | `Sample` |  |
| Axis | `axis` | x, y, z | (0, 0, 1) |  |
| Angle (degrees) | `angle` | number -360 to 360 | 90 |  |

### Rotate Sample Reference Frame

Turn the sample (its grid and every cell array, not the crystal orientations) by an angle about an axis: a new grid around the turned sample, each cell taking the values under its centre; turns by multiples of 90 degrees are exact.

Python: `rotate_sample_reference_frame` · also found by: rotate sample, sample frame, turn volume, reorient, reference frame

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Axis | `axis` | x, y, z | (0, 0, 1) |  |
| Angle (degrees) | `angle` | number -360 to 360 | 90 |  |

### SROD

How far each grain's orientation has turned from the one it started with, by height above its base in an additive build (z the build direction), with the grain's class (plate, epitaxial, interface, originating).

Python: `srod` · also found by: srod, orientation shift, misorientation from start, additive manufacturing, build height, columnar grains, lattice rotation, layer

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Layer thickness | `layer` | number 0.000000001 to ∞ | 40 | The height of a bin, in the volume's length unit. |
| Plate top (z) | `plate_top` | number -∞ to ∞ | NaN | Height of the base plate's top; empty: the bottom of the volume. |
| Interface height | `interface` | number 0 to ∞ | NaN | Grains starting below this height above the plate are 'interface' grains; empty: one layer. |
| Output | `output` | Volume / Bins / By height | `Volume` |  |
| Smallest grain (volume) | `min_volume` | number 0 to ∞ | 212 | *advanced* |
| Reference volume | `reference_volume` | number 0 to ∞ | 106 | *advanced* The grain's lowest sections holding this volume give its starting orientation. |
| At most this many sections | `reference_sections` | whole number 1 to 1000 | 5 | *advanced* |
| Smallest bin (volume) | `min_bin_volume` | number 0 to ∞ | 424 | *advanced* |
| Feature ids | `feature_ids` | text | `FeatureIds` | *advanced* |

### Segment Features

Group neighbouring cells into features (grains): by misorientation, by c-axis alignment or by a scalar, giving FeatureIds.

Python: `segment_features` · also found by: grains, segmentation, reconstruct, feature ids, misorientation, grain map, ebsd, flood fill

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Group by | `method` | Misorientation / C-axis alignment / Scalar | `Misorientation` |  |
| Tolerance | `tolerance` | number 0 to ∞ | 5 | Degrees for misorientation and c-axis alignment; a value difference for Scalar. |
| Mask | `mask` | array | (the active array) | Only cells where this array is not 0 are grouped (a good-data or sample mask). |
| Scalar | `scalar` | array | (the active array) | The cell array compared by the Scalar method. |
| Shuffle ids | `shuffle` | on/off | off | Number the features in a shuffled order, so neighbouring features get unrelated ids (better colors). |
| Orientations | `orientations` | array | (the active array) | *advanced* |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` | *advanced* |
| Phases | `phases` | array | (the active array) | *advanced* |

### Set Origin and Spacing

Give image data a new origin (the corner of its first cell) and spacing; the cells keep their values.

Python: `set_origin_spacing` · also found by: origin, spacing, resolution, voxel size, calibrate

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Change the origin | `change_origin` | on/off | on |  |
| Origin | `origin` | x, y, z | (0, 0, 0) |  |
| Change the spacing | `change_spacing` | on/off | on |  |
| Spacing | `spacing` | x, y, z | (1, 1, 1) |  |

### Smooth Boundaries

Laplacian or Taubin smoothing of a grain boundary surface: triple lines follow triple lines, quadruple points and the outer faces stay put.

Python: `smooth_boundaries` · also found by: laplacian smoothing, taubin, smooth surface mesh, staircase, grain boundaries, node type

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Iterations | `iterations` | whole number 0 to 10000 | 20 |  |
| Step (lambda) | `lambda` | number 0 to 1 | 0.5 |  |
| Inflating step (mu, Taubin) | `mu` | number -1 to 0 | -0.53 | 0: plain Laplacian smoothing, which shrinks the grains; about -lambda - 0.03 keeps their volume. |
| Step on triple lines | `triple_lambda` | number 0 to 1 | 0.25 | *advanced* |
| Step at quadruple points | `quad_lambda` | number 0 to 1 | 0 | *advanced* |
| Smooth on the outer faces | `smooth_outer` | on/off | on | *advanced* |

### Texture Components

The fraction of orientations with a crystal direction of each family within a cone around each sample axis (a table).

Python: `texture_components` · also found by: texture fractions, fiber fraction, component fraction, cube texture, volume fraction, 100 fiber, 111 fiber

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Directions | `families` | text | `100, 110, 111` |  |
| Sample axes | `axes` | text | `X, Y, Z` |  |
| Cone (degrees) | `cone` | number 0.1 to 90 | 15 |  |
| Orientations | `orientations` | array | (the active array) | Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix. |
| Crystal structure | `structure` | From the phases / Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `From the phases` |  |
| Phase | `phase` | whole number 0 to 255 | 1 | The phase analysed (0: every indexed point, which must all be of one crystal class). |
| Phases | `phases` | array | (the active array) | *advanced* Empty: Phases. |
| Mask | `mask` | array | (the active array) | *advanced* Points where this array is 0 are left out. |
| Orientations used at most | `max_orientations` | whole number 100 to 10000000 | 10000000 | *advanced* A random subset (fixed seed) when there are more. |

### Triangle Geometry

Areas, normals, centroids and smallest angles of the triangles of a boundary surface, and the volume and boundary area of every feature it encloses.

Python: `triangle_geometry` · also found by: triangle area, face normals, minimum angle, dihedral, surface area, enclosed volume, mesh quality

## Mesh

### Assign Grains

Give every element of a mesh a grain and an orientation: Voronoi or Laguerre grains, the nearest point of a point set with grain ids, or the feature of a microstructure volume; optionally one block per grain and a side set on the grain boundaries.

Python: `assign_grains` · also found by: polycrystal, grain_id, voronoi, grains on mesh, microstructure mapping, crystal plasticity, orientations, grain blocks

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Grains from | `method` | Voronoi / Nearest point / From a volume | `Voronoi` |  |
| Grains (Voronoi) | `grains` | whole number 1 to 10000000 | 30 |  |
| Size spread (Laguerre) | `size_sigma` | number 0 to 3 | 0 |  |
| Orientations (Voronoi) | `orientations` | Random / <100> along Z / <111> along Z | `Random` |  |
| Fibre spread (degrees) | `spread` | number 0 to 90 | 10 |  |
| Grain id array of the source | `grain_array` | text | `grain_id` | Nearest point: the points' grain id column (grain_id); from a volume: its feature ids (FeatureIds). |
| One block per grain | `blocks` | on/off | off |  |
| Side set on grain boundaries | `boundaries` | on/off | off |  |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Boundary Layers

Insert thin layers of hexahedra, growing in thickness, under the faces of a side set (for walls in flow or contact problems).

Python: `boundary_layers` · also found by: boundary layer, inflation layers, prism layers, wall layers, y plus, near wall, grading

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Side set | `side_set` | text | (empty) |  |
| Layers | `layers` | whole number 1 to 50 | 3 |  |
| First layer thickness | `first` | number 0.000000000001 to ∞ | 0.01 |  |
| Growth ratio | `growth` | number 1 to 10 | 1.3 |  |

### Convert Elements

Hexahedra into tetrahedra (12 each, conforming) or tetrahedra into hexahedra (4 each), keeping blocks and sets.

Python: `convert_elements` · also found by: htet, thex, hex to tet, tet to hex, tetrahedralize, split hexahedra, convert mesh

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Convert | `to` | Hexahedra to tetrahedra / Tetrahedra to hexahedra | `Hexahedra to tetrahedra` |  |

### Define Mesh Set

Add a node set (the nodes where an expression of the coordinates holds) or a side set (the boundary faces whose nodes all satisfy it) to a mesh, for boundary conditions; written with the mesh to Exodus, Abaqus, Gmsh and the other mesh files.

Python: `define_mesh_set` · also found by: node set, side set, boundary condition, nodeset, sideset, by location, expression, group

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Set | `kind` | Side set / Node set | `Side set` |  |
| Name | `name` | text | `selected` |  |
| Where (expression) | `expression` | text | `coordsX < 1e-6` | Of coordsX, coordsY, coordsZ and the point arrays, like the Calculator: abs(coordsZ - 1) < 1e-6, coordsX*coordsX + coordsY*coordsY < 0.25. |

### Elevate

Turn 8-node hexahedra into 20- or 27-node ones (shared mid-edge, mid-face and centre nodes).

Python: `elevate` · also found by: quadratic, higher order, hex20, hex27, serendipity, promote

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Element | `element` | HEX20 / HEX27 | `HEX27` |  |

### Extract Set

A side set's faces as a surface, or a node set's nodes as points, by name.

Python: `extract_set` · also found by: side set, node set, boundary, surface, sideset, nodeset, boundary condition

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Set name | `set` | text | `interface` | The name of a side set or node set (for example x-, interface). |

### Merge Nodes

Merge mesh nodes closer than a tolerance (meshes made separately that touch), renumbering the cells and node sets; field data MergedNodes gives how many went.

Python: `merge_nodes` · also found by: equivalence, merge nodes, duplicate nodes, tolerance, join meshes, clean

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Tolerance | `tolerance` | number 0 to ∞ | 0.000001 |  |

### Overlay Mesh

Hexahedra on a Cartesian grid laid over a labelled image (each label a block, 0 empty) or a closed surface, with smooth pillowed interfaces or whole cells.

Python: `overlay_mesh` · also found by: voxel mesh, microstructure mesh, grain mesh, stl to hex, conforming hex, pillow, stair step, rve, image to mesh, hex mesh

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Labels | `array` | array | (the active array) | Image data: the cell array of material or grain numbers (0: empty). Empty: FeatureIds. |
| Cell size | `cell` | number 0 to ∞ | 0 | 0: the image's own voxels, or a thirtieth of a surface's largest size. |
| Interfaces | `mode` | Smooth / Stair-step | `Smooth` |  |
| Mesh the empty space | `void` | on/off | off |  |
| Periodic (pair opposite faces) | `periodic` | on/off | off |  |
| Keep sharp edges | `sharp` | on/off | on | *advanced* Surfaces: nodes go onto the creases between flat patches and onto corners. |
| Smallest piece (cells) | `min_cells` | whole number 0 to 1000000 | 8 | *advanced* Pieces of a material with fewer cells join their surroundings. |
| Smoothing passes | `smoothing` | whole number 0 to 200 | 8 | *advanced* |
| Element | `element` | HEX8 / HEX20 / HEX27 | `HEX8` | *advanced* |

### Refine Mesh

Split every element of a hexahedral or tetrahedral mesh into eight, keeping blocks, side sets and node sets.

Python: `refine_mesh` · also found by: refine, subdivide, uniform refinement, 8x, h-refinement, finer mesh, split elements

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Levels | `levels` | whole number 1 to 4 | 1 |  |

### Refine Region

Split the tetrahedra inside a box (or where a cell array reaches a value) into eight, keeping the mesh conforming around them.

Python: `refine_region` · also found by: local refinement, adaptive, refine box, refine where, h-refinement, tetrahedra

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Box min | `box_min` | x, y, z | (-1000000000000000000000000000000, -1000000000000000000000000000000, -1000000000000000000000000000000) |  |
| Box max | `box_max` | x, y, z | (1000000000000000000000000000000, 1000000000000000000000000000000, 1000000000000000000000000000000) |  |
| Or where this cell array | `array` | array | (the active array) | *advanced* |
| is at least | `value` | number -∞ to ∞ | 1 | *advanced* |

### Remesh Surface

Mesh any surface again at a size: triangles or all quadrilaterals, the size fixed, from a point array (the size at each point), or following the curvature (a chord tolerance); sharp edges and open boundaries are kept.

Python: `remesh_surface` · also found by: remesh, surface mesh, isotropic, triangle mesh, quad mesh, sizing, curvature, resample surface, pave

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Size (0 = the input's mean edge) | `size` | number 0 to ∞ | 0 |  |
| Elements | `element` | Triangles / Quadrilaterals | `Triangles` |  |
| Size from a point array | `size_array` | text | (empty) | A point array whose value is the size wanted there (empty: none). |
| Curvature: chord tolerance (0 = off) | `tolerance` | number 0 to ∞ | 0 | Where the surface curves, the size keeps the gap between the mesh and a circle of the local curvature below this. |
| Sharp edge angle (degrees) | `feature_angle` | number 0 to 180 | 30 |  |
| Smallest size | `min` | number 0 to ∞ | 0 | *advanced* |
| Largest size (0 = no limit) | `max` | number 0 to ∞ | 0 | *advanced* |
| Iterations | `iterations` | whole number 1 to 50 | 4 | *advanced* |

### Signed Distance Field

The distance to a closed surface (negative inside) on an image grid around it.

Python: `signed_distance` · also found by: sdf, distance field, implicit, level set, inside outside, voxelize, stl

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Resolution | `resolution` | whole number 4 to 1024 | 64 | Cells along the longest side of the surface. |
| Margin | `margin` | number 0 to 10 | 0.1 | Space added around the surface on every side, as a fraction of its size. |

### Smooth Mesh

Move the inner nodes of a hexahedral or tetrahedral mesh to better its elements: Laplacian, guarded Laplacian or condition-number optimisation; the boundary stays put.

Python: `smooth_mesh` · also found by: mesh smoothing, laplacian, smart laplacian, condition number, optimize mesh, untangle, improve quality

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Scheme | `scheme` | Smart Laplacian / Laplacian / Condition number / Winslow | `Smart Laplacian` |  |
| Iterations | `iterations` | whole number 1 to 1000 | 10 |  |
| Also hold the node set | `fixed` | text | (empty) | *advanced* |

### Sweep Quads

Sweep a surface of quadrilaterals into layers of hexahedra along a direction.

Python: `sweep_quads` · also found by: extrude, sweep, 2d to 3d, quad mesh, hex mesh, layers

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Direction | `direction` | x, y, z | (0, 0, 1) |  |
| Distance | `distance` | number -∞ to ∞ | 1 |  |
| Layers | `layers` | whole number 1 to 100000 | 4 |  |
| Grading (largest/smallest) | `grading` | number 0.01 to 100 | 1 |  |
| Small layers at | `cluster` | Start / End / Both | `Start` |  |

### Tet Mesh

Fill a closed surface (STL, OBJ, PLY ...) with well-shaped tetrahedra of one size, a side set per patch of the surface.

Python: `tet_mesh` · also found by: tetrahedral mesh, tetrahedralize, stl to tet, volume mesh, isosurface stuffing, tet4, fem mesh

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Element size | `cell` | number 0 to ∞ | 0 | 0: a thirtieth of the surface's largest size. |

### Tetrahedra in a Region

Turn the hexahedra where an expression holds into tetrahedra, with pyramids where they meet the hexahedra that stay, so the mixed mesh is conforming; cell arrays and side sets follow.

Python: `hex_to_tets` · also found by: pyramid, transition, hex to tet, mixed mesh, tetrahedra, region, conforming

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Where (expression) | `expression` | text | `coordsX < 0.5` | Hexahedra all of whose nodes satisfy it are converted. |

## Python

### Programmable Filter

Run your own Python (NumPy) on the data: read inputs[0], change output (a copy of it).

Python: `programmable_filter` · also found by: python, script, numpy, custom, code

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Script | `script` | list of numbers | `# inputs[0] is the input, output starts as a copy of it.; # Arrays are NumPy: output.point_data['T2'] = inputs[0].point_data['T'] * 2; # Points: output.points (N x 3). Also cell_data, field_data, n_points, n_cells.; ` |  |

### Python Calculator

A new array from a NumPy expression over the arrays, e.g. T * 2 or np.linalg.norm(velocity, axis=1).

Python: `python_calculator` · also found by: numpy, expression, formula, compute array, python

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Expression | `expression` | list of numbers | (empty) |  |
| Result name | `result` | text | `result` |  |
| Attribute | `attribute` | Points / Cells | `Points` |  |

## Sources

### AMR Gaussian Pulse

A small AMR test dataset: a coarse box and two finer boxes holding a Gaussian pulse.

Python: `amr_gaussian_pulse` · also found by: amr, adaptive mesh refinement, levels, test data, overlapping amr

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Dimension | `dimension` | 2D / 3D | `3D` |  |
| Root spacing | `root_spacing` | number 0.000000001 to ∞ | 0.5 |  |
| Refinement ratio | `refinement_ratio` | 2 / 3 / 4 | `2` |  |
| Pulse origin | `pulse_origin` | x, y, z | (0, 0, 0) |  |
| Pulse width | `pulse_width` | x, y, z | (0.5, 0.5, 0.5) |  |
| Pulse amplitude | `pulse_amplitude` | number -∞ to ∞ | 0.5 |  |

### Box

An axis-aligned box.

Python: `box` · also found by: cube, shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Center | `center` | x, y, z | (0, 0, 0) |  |
| Size | `size` | x, y, z | (1, 1, 1) |  |

### Box Mesh

A box of hexahedra, optionally graded, with side sets and node sets on its six faces.

Python: `box_mesh` · also found by: hex mesh, structured mesh, block, brick mesh, grid, graded, finite element

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Size | `size` | x, y, z | (1, 1, 1) |  |
| Origin | `origin` | x, y, z | (0, 0, 0) |  |
| Cells | `cells` | x, y, z | (10, 10, 10) |  |
| Grading (largest/smallest) | `grading` | x, y, z | (1, 1, 1) | Ratio of the largest to the smallest interval along x, y and z (1: uniform). |
| Small intervals at | `cluster` | Start / End / Both | `Start` |  |
| Element | `element` | HEX8 / HEX20 / HEX27 | `HEX8` |  |

### CAD Model

A parametric part: sketches, features and parameters that rebuild the solid when a dimension changes, with named faces. Change a dimension from Properties or Python without opening the CAD environment.

Python: `cad_model` · also found by: cad, parametric, part, sketch, feature, extrude, revolve, fillet, hole, solid model, step, dimension, design

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Document | `document` | list of numbers | `{;   "format": 1,;   "kind": "auraFX CAD document",;   "document": {;     "name": "Part",;     "features": [;       {;         "id": 1,;         "name": "Base",;         "op": "Add",;         "shape": {;           "kind": "SketchExtrude",;           "sketch": {;             "points": [;               {;                 "x": -20.0,;                 "y": -15.0,;                 "fixed": false;               },;               {;                 "x": 20.0,;                 "y": -15.0,;                 "fixed": false;               },;               {;                 "x": 20.0,;                 "y": 15.0,;                 "fixed": false;               },;               {;                 "x": -20.0,;                 "y": 15.0,;                 "fixed": false;               },;               {;                 "x": 0.0,;                 "y": 0.0,;                 "fixed": true;               };             ],;             "entities": [;               {;                 "type": "Line",;                 "a": 0,;                 "b": 2,;                 "construction": true;               },;               {;                 "type": "Line",;                 "a": 0,;                 "b": 1,;                 "construction": false;               },;               {;                 "type": "Line",;                 "a": 1,;                 "b": 2,;                 "construction": false;               },;               {;                 "type": "Line",;                 "a": 2,;                 "b": 3,;                 "construction": false;               },;               {;                 "type": "Line",;                 "a": 3,;                 "b": 0,;                 "construction": false;               };             ],;             "constraints": [;               {;                 "Midpoint": [;                   4,;                   0;                 ];               },;               {;                 "Horizontal": 1;               },;               {;                 "Vertical": 2;               },;               {;                 "Horizontal": 3;               },;               {;                 "Vertical": 4;               },;               {;                 "Length": [;                   1,;                   {;                     "value": 40.0,;                     "expr": "width";                   };                 ];               },;               {;                 "Length": [;                   2,;                   {;                     "value": 30.0,;                     "expr": "depth";                   };                 ];               };             ],;             "plane": "XY",;             "offset": 0.0;           },;           "depth": 8.0,;           "end": "Blind",;           "draft_deg": 0.0,;           "top_edge": {;             "type": "None";           },;           "bottom_edge": {;             "type": "None";           },;           "reverse": false;         },;         "placement": {;           "translate": [;             0.0,;             0.0,;             0.0;           ],;           "rotate_deg": [;             0.0,;             0.0,;             0.0;           ];         },;         "blend": 0.0,;         "modifiers": [],;         "suppressed": false,;         "blend_kind": "Round",;         "bindings": [;           {;             "path": "/shape/depth",;             "expr": "thickness";           };         ];       };     ],;     "finish": [],;     "parameters": [;       {;         "name": "width",;         "expr": "40 mm",;         "comment": "";       },;       {;         "name": "depth",;         "expr": "30 mm",;         "comment": "";       },;       {;         "name": "thickness",;         "expr": "8 mm",;         "comment": "";       };     ],;     "configurations": [],;     "active_configuration": null,;     "next_id": 1;   }; }` | The part as an .afxcad document (JSON). Edit it with the CAD environment or with node.cad in Python. |
| Parameter overrides | `parameters` | text | (empty) | A JSON object of parameter name to expression that replaces the document's own values, for example {"width": "50 mm"}. |
| Configuration | `configuration` | text | (empty) | The name of a configuration of the document (a named set of parameter values); empty: the document's own. |
| Resolution | `resolution` | whole number 8 to 1024 | 64 | Cells along the longest side of the part when drawing its surface. |
| Output | `output` | Surface / Distance field | `Surface` | Surface: the part's surface with BodyId and FaceId and the face names. Distance field: the signed distance to the part on a grid. |
| Kernel | `kernel` | Auto / Implicit / B-rep | `Auto` | B-rep: exact faces and edges (what STEP export writes); Implicit: signed distance fields, builds every feature. Auto: B-rep when it can build the part, else Implicit. |
| Snap to the surface | `project` | on/off | on | *advanced* Move the drawn points onto the exact surface (Implicit kernel). |

### Cell Grid

One cell of a discontinuous-Galerkin cell grid with test fields.

Python: `cell_grid` · also found by: cell grid, dg, discontinuous galerkin, hcurl, hdiv, hgrad, test data

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cell type | `cell_type` | Hexahedron / Tetrahedron / Pyramid / Wedge / Quadrilateral / Triangle / Edge / Vertex | `Hexahedron` |  |

### Columnar Grains

Columnar grains of an additive build: tilted columns growing from the base plate along Z, stopping at lognormal heights or when overgrown, replaced by new grains.

Python: `columnar_grains` · also found by: additive manufacturing, am, columnar, build, epitaxial, synthetic microstructure, laser powder bed, grains

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Box size | `size` | x, y, z | (300, 300, 450) |  |
| Voxel size | `voxel` | number 0.000000001 to ∞ | 5 |  |
| Grain width | `width` | number 0.000000001 to ∞ | 50 |  |
| Median grain height | `height` | number 0.000000001 to ∞ | 150 |  |
| Height spread (log sigma) | `height_sigma` | number 0 to 3 | 0.5 |  |
| Column tilt (degrees, standard deviation) | `tilt` | number 0 to 60 | 6 |  |
| Width spread (log sigma) | `width_sigma` | number 0 to 3 | 0 | *advanced* |
| Width grows with height (exponent) | `width_height_exponent` | number -2 to 2 | 0 | *advanced* |
| Orientations | `orientations` | Random / <100> along Z / <111> along Z | `<100> along Z` |  |
| Fibre spread (degrees) | `spread` | number 0 to 90 | 10 |  |
| Crystal structure | `structure` | Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `Cubic m-3m` |  |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Cone

A polygonal cone.

Python: `cone` · also found by: shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Center | `center` | x, y, z | (0, 0, 0) |  |
| Direction | `direction` | x, y, z | (1, 0, 0) |  |
| Height | `height` | number 0 to ∞ | 1 |  |
| Radius | `radius` | number 0 to ∞ | 0.5 |  |
| Resolution | `resolution` | whole number 3 to 1024 | 32 |  |

### Cylinder

A capped cylinder along Y.

Python: `cylinder` · also found by: tube, shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Center | `center` | x, y, z | (0, 0, 0) |  |
| Height | `height` | number 0 to ∞ | 1 |  |
| Radius | `radius` | number 0 to ∞ | 0.5 |  |
| Resolution | `resolution` | whole number 3 to 1024 | 32 |  |

### Dog-bone Mesh

A flat dog-bone tensile specimen of hexahedra with shoulder fillets and an optional thickness profile along the gage.

Python: `dogbone_mesh` · also found by: tensile specimen, dogbone, dog bone, coupon, gage, hex mesh, test specimen

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Total length | `length` | number 0.000000001 to ∞ | 25.4 |  |
| Gage length | `gage_length` | number 0.000000001 to ∞ | 8 |  |
| Gage width | `gage_width` | number 0.000000001 to ∞ | 1.85 |  |
| Grip width | `grip_width` | number 0.000000001 to ∞ | 4.8 |  |
| Fillet radius | `fillet_radius` | number 0.000000001 to ∞ | 3.175 |  |
| Grip thickness | `thickness` | number 0.000000001 to ∞ | 0.35 |  |
| Thickness stations | `stations` | list of numbers | [] | Pairs: position from the gage start, thickness there (a spline through them along the gage). Empty: constant thickness. |
| Cell size | `cell` | number 0.000000001 to ∞ | 0.2 |  |
| Grip cell size | `grip_cell` | number 0.000000001 to ∞ | 0.4 |  |
| Layers through the thickness | `layers` | whole number 1 to 100 | 1 |  |
| Element | `element` | HEX8 / HEX20 / HEX27 | `HEX8` |  |

### Face Mesh

A planar face drawn with outline commands (an outer outline with holes) meshed at a size with triangles or all quadrilaterals, finer near the holes if asked: sweep the quadrilaterals into hexahedra with Sweep Quads.

Python: `face_mesh` · also found by: pave, paving, quad mesh, 2d mesh, triangulate, planar face, hole, plate, unstructured quads, sizing

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Outline commands | `commands` | text | `outline 1 rectangle x 4 y 2 center 2 1; outline 2 circle radius 0.5 center 2 1` | The geometry language's outline commands: polyline, rectangle, circle, ellipse, spline, regular, add line/arc, fillet. |
| Outer outline | `outer` | whole number 1 to 100000 | 1 |  |
| Holes (outline numbers) | `holes` | text | `2` |  |
| Size | `size` | number 0.000000001 to ∞ | 0.2 |  |
| Elements | `element` | Quadrilaterals / Triangles | `Quadrilaterals` |  |
| Size at the holes (0 = the size) | `near` | number 0 to ∞ | 0 |  |
| Growth away from the holes | `growth` | number 0 to 10 | 0.2 | How fast the size grows with the distance from the holes (per unit length). |
| Pieces per outline (0 = from the size) | `intervals` | text | (empty) | *advanced* One number per outline, outer first: the outline is divided into that many pieces. |
| Smallest size | `min` | number 0 to ∞ | 0 | *advanced* |
| Largest size (0 = no limit) | `max` | number 0 to ∞ | 0 | *advanced* |

### Geometry

Solids built from geometry commands (primitives, booleans, extruded, revolved or swept outlines, imported surfaces), with named faces.

Python: `geometry` · also found by: cad, csg, solid, brick, cylinder, boolean, extrude, revolve, sweep, implicit, mesh geometry, model

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Commands | `commands` | list of numbers | `# A block with a hole and a rounded boss.; brick x 4 y 2 z 1; cylinder radius 0.5 height 2; subtract body 2 from body 1; outline 1 rectangle x 1 y 1; outline 1 fillet radius 0.3; extrude outline 1 height 1; move body 3 x 1.4 z 0.5; unite body 1 3; ` | One command per line (or separated by ;). See the Geometry and meshing chapter of the user guide, or ask the assistant. |
| Resolution | `resolution` | whole number 8 to 1024 | 64 | Cells along the longest side of each body when drawing its surface. |
| Output | `output` | Surface / Distance field / Hex mesh / Tet mesh | `Surface` | Surface: the bodies' surfaces with BodyId and FaceId. Distance field: the signed distance to the nearest body on a grid. Hex mesh: the hexahedral mesh of the bodies named by 'mesh body' commands. Tet mesh: their tetrahedral mesh. |
| Snap to the surface | `project` | on/off | on | *advanced* Move the drawn points onto the exact surface. |

### Hyper Tree Grid

A hyper tree grid (tree-based refinement) from a text descriptor.

Python: `hyper_tree_grid` · also found by: htg, hyper tree grid, octree, quadtree, adaptive, refinement, test data

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Dimensions | `dimensions` | x, y, z | (2, 2, 2) | Points of the root grid along x, y, z (1 makes an axis flat). |
| Branch factor | `branch_factor` | 2 / 3 | `2` |  |
| Max depth | `max_depth` | whole number 1 to 64 | 1 |  |
| Descriptor | `descriptor` | text | `.` | Levels separated by \|; per level, R = refined and . = leaf for every vertex of all trees in order (spaces are ignored). |
| Grid scale | `grid_scale` | x, y, z | (1, 1, 1) |  |
| Use mask | `use_mask` | on/off | off |  |
| Mask | `mask` | text | `1` | Same layout as the descriptor: 0 hides a leaf, 1 keeps it. |

### IPF Color Key

The inverse pole figure color key of a crystal class: its standard triangle in stereographic projection, colored.

Python: `ipf_color_key` · also found by: ipf, legend, color key, standard triangle, stereographic, crystal, ebsd

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Crystal structure | `structure` | Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `Cubic m-3m` |  |
| Resolution | `resolution` | whole number 4 to 400 | 60 |  |

### Inclusion Mesh

A box of matrix with randomly placed inclusions (spheres, ellipsoids, superquadrics, harmonic grains), meshed with conforming hexahedra that follow the inclusion surfaces exactly.

Python: `inclusion_mesh` · also found by: particles, composite, rve, inclusions, spheres in cube, hex mesh, o-grid, periodic, microstructure mesh, voids

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Box size | `size` | x, y, z | (1, 1, 1) |  |
| Cells along the shortest side | `cells` | whole number 4 to 1000 | 24 | The background cell size is the shortest side divided by this. |
| Inclusions | `count` | whole number 0 to 100000 | 8 |  |
| Radius | `radius` | number 0.000000001 to ∞ | 0.075 |  |
| Radius spread | `radius_std` | number 0 to ∞ | 0 | Standard deviation of the radius (normal, cut at 3 spreads). |
| Shape | `shape` | Sphere / Ellipsoid / Superquadric / Harmonic grain | `Sphere` |  |
| Semi-axes / radius | `aspect` | x, y, z | (1, 0.7, 0.5) | Ellipsoids and superquadrics: their semi-axes as multiples of the radius. |
| Exponents | `exponents` | list of numbers | [4.0, 4.0] | Superquadrics: the in-plane and vertical exponents (2 2 is an ellipsoid; larger is boxier). |
| Roughness | `roughness` | number 0 to 1 | 0.12 | Harmonic grains: size of the surface perturbation. |
| Random orientation | `random_orientation` | on/off | on |  |
| Seed | `seed` | whole number 0 to any | 7 |  |
| Periodic | `periodic` | on/off | off | Pair the nodes of opposite faces (node sets x-_node_set ... and PeriodicPairs) for periodic boundary conditions. |
| Element | `element` | HEX8 / HEX20 / HEX27 | `HEX8` | HEX20 and HEX27 put their new nodes on the inclusion surfaces too. |
| Packing | `packing` | Random / Collective | `Random` | Collective moves the placed inclusions to open room for more (denser packings, slower). |
| One block per inclusion | `separate_blocks` | on/off | off | *advanced* |
| Box / inclusion size | `box_ratio` | number 1.05 to 4 | 1.35 | *advanced* |
| Cells between boxes | `gap_cells` | whole number 0 to 100 | 2 | *advanced* |
| Cells from the walls | `wall_cells` | whole number 0 to 100 | 1 | *advanced* |
| Layers inside the surface | `n_inner` | whole number 1 to 50 | 2 | *advanced* |
| Layers outside the surface | `n_outer` | whole number 1 to 50 | 2 | *advanced* |
| Inner grading | `inner_grading` | number 0.01 to 100 | 1 | *advanced* |
| Outer grading | `outer_grading` | number 0.01 to 100 | 1 | *advanced* |
| Core size | `core_fraction` | number 0.1 to 0.95 | 0.55 | *advanced* |
| Core roundness | `core_roundness` | number 0 to 1 | 0.35 | *advanced* |
| Surface spacing blend | `patch_blend` | number 0 to 1 | 0.6 | *advanced* |

### Line

A straight polyline.

Python: `line` · also found by: segment

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Point 1 | `point1` | x, y, z | (-0.5, 0, 0) |  |
| Point 2 | `point2` | x, y, z | (0.5, 0, 0) |  |
| Resolution | `resolution` | whole number 1 to 1000000 | 10 |  |

### Pack Grains

A synthetic microstructure packed from goal statistics: lognormal grain sizes, beta-distributed aspect ratios, ellipsoids or other shapes, with an optional matrix phase.

Python: `pack_grains` · also found by: pack primary phases, synthetic microstructure, statistics, grain size distribution, ellipsoids, rve, matrix phase, stats generator

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cells | `cells` | list of numbers | [64.0, 64.0, 64.0] |  |
| Cell size | `spacing` | number 0.000000001 to ∞ | 1 |  |
| Size mu (log of diameter) | `esd_mu` | number -∞ to ∞ | 2.3 | The equivalent diameters are lognormal: mean of their logarithms (2.3: about 10 cells). |
| Size sigma | `esd_sigma` | number 0 to 3 | 0.2 |  |
| Smallest size (sigmas below) | `min_cut` | number 0 to 10 | 3 | *advanced* |
| Largest size (sigmas above) | `max_cut` | number 0 to 10 | 3 | *advanced* |
| b/a beta (alpha, beta) | `ba` | list of numbers | [15.0, 2.0] |  |
| c/a beta (alpha, beta) | `ca` | list of numbers | [12.0, 4.0] | At most b/a. |
| Shape | `shape` | Ellipsoid / Superellipsoid / Cube-octahedron / Cylinder | `Ellipsoid` |  |
| Superellipsoid exponent | `exponent` | number 1 to 50 | 4 | *advanced* |
| Axes along x, y, z | `aligned` | on/off | off |  |
| Grain volume fraction | `fraction` | number 0.01 to 1 | 1 | Below 1, the space between the grains is a matrix phase. |
| Periodic | `periodic` | on/off | off |  |
| Orientations | `orientations` | Random / <100> along Z / <111> along Z | `Random` |  |
| Fibre spread (degrees) | `spread` | number 0 to 90 | 10 |  |
| Crystal structure | `structure` | Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `Cubic m-3m` |  |
| Match the goal sizes and shapes | `calibrate` | on/off | on | Space-filling grains measure more irregular than the shapes drawn: repack with corrected goals until the measured statistics match (slower). |
| Placement moves per grain | `moves` | whole number 0 to 100000 | 20 | *advanced* |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Plane

A subdivided rectangle.

Python: `plane` · also found by: quad, grid, shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Origin | `origin` | x, y, z | (-0.5, -0.5, 0) |  |
| Point 1 | `point1` | x, y, z | (0.5, -0.5, 0) |  |
| Point 2 | `point2` | x, y, z | (-0.5, 0.5, 0) |  |
| X resolution | `xres` | whole number 1 to 4096 | 10 |  |
| Y resolution | `yres` | whole number 1 to 4096 | 10 |  |

### Plate Mesh

A plate of hexahedra (one through the thickness) with an optional circular or elliptical through-hole meshed by a conforming ring.

Python: `plate_mesh` · also found by: plate with hole, notch, specimen, stress concentration, hex mesh, o-grid, fracture, phase field

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Width, height, thickness | `size` | x, y, z | (100, 50, 1) |  |
| Cell size | `cell` | number 0.000000001 to ∞ | 2.5 | The width and height must be whole multiples of it. |
| Hole | `hole` | None / Circle / Ellipse | `Circle` |  |
| Semi-axis a (radius) | `a` | number 0.000000001 to ∞ | 2.5 |  |
| Semi-axis b | `b` | number 0.000000001 to ∞ | 2.5 |  |
| Ellipse angle (degrees) | `angle` | number -360 to 360 | 0 |  |
| Hole centre | `centre` | list of numbers | [] | x y of the hole centre; empty: the plate centre. |
| Element | `element` | HEX8 / HEX20 / HEX27 | `HEX8` |  |

### Point Cloud

Random points inside a sphere.

Python: `point_source` · also found by: random, points, seeds

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Center | `center` | x, y, z | (0, 0, 0) |  |
| Radius | `radius` | number 0 to ∞ | 0.5 |  |
| Number of points | `count` | whole number 1 to 100000000 | 1000 |  |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Programmable Source

Make data with your own Python (NumPy): assign output.

Python: `programmable_source` · also found by: python, script, numpy, custom, generate

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Script | `script` | list of numbers | `# Assign output, e.g. a random point cloud:; xyz = np.random.default_rng(0).random((1000, 3)); output = afx.make_points(xyz); output.point_data['height'] = xyz[:, 2]; ` |  |

### Random Hyper Tree Grid

A hyper tree grid refined at random.

Python: `random_hyper_tree_grid` · also found by: htg, hyper tree grid, random, octree, adaptive, test data

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Dimensions | `dimensions` | x, y, z | (6, 6, 3) | Points of the root grid along x, y, z (1 makes an axis flat). |
| Output bounds | `bounds` | list of numbers | [-10.0, 10.0, -10.0, 10.0, -10.0, 10.0] |  |
| Seed | `seed` | whole number -2147483648 to 2147483647 | 0 |  |
| Max depth | `max_depth` | whole number 0 to 30 | 5 |  |
| Split fraction | `split_fraction` | number 0 to 1 | 0.5 | Chance that a cell is refined. |
| Masked fraction | `masked_fraction` | number 0 to 1 | 0 | About this share of the volume is hidden (whole subtrees at a time). |

### Spatio Temporal Harmonics Grid

Image volume of travelling sine waves with its own time steps (animated test data).

Python: `spatio_temporal_harmonics_grid` · also found by: waves, sine, harmonics, synthetic, test data, animation, time steps, uniform grid

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Whole extent | `extent` | list of numbers | [-10.0, 10.0, -10.0, 10.0, -10.0, 10.0] | x min, x max, y min, y max, z min, z max (point indices, spacing 1). |
| Harmonics | `harmonics` | list of numbers | [1.0, 1.0, 0.6283, 0.6283, 0.6283, 0.0, 3.0, 1.0, 0.6283, 0.0, 0.0, 1.5708, 2.0, 2.0, 0.0, 0.6283, 0.0, 3.1416, 1.0, 3.0, 0.0, 0.0, 0.6283, 4.7124] | 6 numbers per wave: amplitude, temporal frequency, wave vector x y z, phase. Value = sum of amplitude * sin(frequency * time + wave vector . point + phase). |
| Time steps | `time_steps` | list of numbers | [0.0, 0.3141, 0.6283, 0.9425, 1.2566, 1.5708, 1.885, 2.1991, 2.5133, 2.8274, 3.1416, 3.4558, 3.7699, 4.0841, 4.3982, 4.7124, 5.0266, 5.3407, 5.6549, 5.969] | The source's time steps (default: one period of the first wave in 20 steps). |

### Sphere

A polygonal sphere.

Python: `sphere` · also found by: ball, shape

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Center | `center` | x, y, z | (0, 0, 0) |  |
| Radius | `radius` | number 0 to ∞ | 0.5 |  |
| Theta resolution | `theta` | whole number 3 to 1024 | 32 |  |
| Phi resolution | `phi` | whole number 3 to 1024 | 32 |  |

### Unstructured Cell Types

A block of cells of one type, including quadratic, Lagrange and Bezier cells of any order, for trying filters on them.

Python: `unstructured_cell_types` · also found by: cell type, higher order, lagrange, bezier, quadratic, tetrahedra, wedge, pyramid, test data

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cell type | `cell_type` | Line / Quadratic Line / Triangle / Quadratic Triangle / Quad / Bi-Quadratic Quad / Tet / Quadratic Tet / Hex / Tri-Quadratic Hexahedron / Wedge / Pyramid / Lagrange Curve / Lagrange Triangle / Lagrange Quadrilateral / Lagrange Tetrahedron / Lagrange Hexahedron / Lagrange Wedge / Bezier Curve / Bezier Triangle / Bezier Quadrilateral / Bezier Tetrahedron / Bezier Hexahedron / Bezier Wedge | `Hex` |  |
| Blocks | `blocks` | x, y, z | (5, 5, 5) | Number of unit blocks along x, y and z (2D cells use x and y, curves x). |
| Cell order | `order` | whole number 1 to 10 | 3 | Order of Lagrange and Bezier cells. |

### Voronoi Grains

A synthetic microstructure of grains: a Voronoi tessellation of random seeds, or a Laguerre (power) tessellation with spread grain sizes, with random or fibre-textured orientations.

Python: `voronoi_grains` · also found by: voronoi, laguerre, tessellation, synthetic microstructure, polycrystal, grains, rve, power diagram

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Cells | `cells` | list of numbers | [64.0, 64.0, 64.0] |  |
| Cell size | `spacing` | number 0.000000001 to ∞ | 1 |  |
| Grains | `grains` | whole number 1 to 10000000 | 100 |  |
| Size spread (log sigma) | `size_sigma` | number 0 to 3 | 0 | 0: Voronoi grains. Above 0: Laguerre grains, seed radii spread lognormally. |
| Periodic | `periodic` | on/off | off |  |
| Orientations | `orientations` | Random / <100> along Z / <111> along Z | `Random` |  |
| Fibre spread (degrees) | `spread` | number 0 to 90 | 10 |  |
| Crystal structure | `structure` | Cubic m-3m / Cubic m-3 / Hexagonal 6/mmm / Hexagonal 6/m / Trigonal -3m / Trigonal -3 / Tetragonal 4/mmm / Tetragonal 4/m / Orthorhombic mmm / Monoclinic 2/m / Triclinic -1 | `Cubic m-3m` |  |
| Random seed | `seed` | whole number 0 to any | 1 | *advanced* |

### Wavelet

Synthetic 3D scalar volume, handy for trying filters.

Python: `wavelet` · also found by: test data, volume, example, rtdata

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Half size | `half_size` | whole number 1 to 512 | 10 |  |
| Maximum | `maximum` | number -∞ to ∞ | 255 |  |
| Std. deviation | `std_dev` | number 0 to ∞ | 0.5 |  |

## Temporal

### Extract Time Steps

Keep some time steps of the data: a list such as 0, 3, 5-8, 10:20:2 (step numbers from 0), or every N-th step from a first one.

Python: `extract_time_steps` · also found by: choose steps, subset of time, every nth step, skip steps, time stride

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Steps (empty: every N-th) | `steps` | text | (empty) | Step numbers from 0: 0, 3, 5-8 (inclusive), 10:20:2 (start:stop:stride, stop left out). |
| First step | `first` | whole number 0 to 10000000 | 0 |  |
| Every N-th step | `stride` | whole number 1 to 10000000 | 1 |  |

### Force Time

Always show the data at one time, whatever time the window is at: compare a step with the animation beside it.

Python: `force_time` · also found by: freeze time, fixed time step, hold time, pin time, ignore time

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Choose by | `pick` | Step number / Time value | `Step number` |  |
| Step number | `step` | whole number 0 to 10000000 | 0 |  |
| Time value | `time` | number -∞ to ∞ | 0 |  |

### Generate Time Steps

Give the data a new set of time values (evenly spaced from a start to an end): each is shown with the data at that time, or at the step before it for data with steps; static data gets a time axis to animate filters against.

Python: `generate_time_steps` · also found by: time values, add time steps, resample time, time axis

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Start time (unset: the first step) | `start` | number -∞ to ∞ | NaN |  |
| End time (unset: the last step) | `end` | number -∞ to ∞ | NaN |  |
| Number of time steps | `count` | whole number 0 to 1000000 | 10 |  |

### Particle Path

The path each particle released at the seeds has followed through a time-varying flow (pathlines).

Python: `particle_path` · also found by: particles, pathlines, streaklines, unsteady flow, advection, trajectories, lagrangian

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `vectors` | array | (the active array) |  |
| Release again every N steps (0 = once) | `reinject_every` | whole number 0 to 1000000 | 0 |  |
| Seed line start (no seed input) | `point1` | x, y, z | (NaN, NaN, NaN) |  |
| Seed line end (no seed input) | `point2` | x, y, z | (NaN, NaN, NaN) |  |
| Seeds on the line | `seeds` | whole number 1 to 100000 | 10 |  |
| Start time | `start_time` | number -∞ to ∞ | NaN | *advanced* |
| Terminal speed | `terminal_speed` | number 0 to ∞ | 0.000000000001 | *advanced* |

### Particle Tracer

Particles released at seed points, carried by a time-varying flow; shows where they are now.

Python: `particle_tracer` · also found by: particles, pathlines, streaklines, unsteady flow, advection, trajectories, lagrangian

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `vectors` | array | (the active array) |  |
| Release again every N steps (0 = once) | `reinject_every` | whole number 0 to 1000000 | 0 |  |
| Seed line start (no seed input) | `point1` | x, y, z | (NaN, NaN, NaN) |  |
| Seed line end (no seed input) | `point2` | x, y, z | (NaN, NaN, NaN) |  |
| Seeds on the line | `seeds` | whole number 1 to 100000 | 10 |  |
| Start time | `start_time` | number -∞ to ∞ | NaN | *advanced* |
| Terminal speed | `terminal_speed` | number 0 to ∞ | 0.000000000001 | *advanced* |

### Streak Line

For each seed, a line through all particles it released so far, like dye in a flow.

Python: `streak_line` · also found by: particles, pathlines, streaklines, unsteady flow, advection, trajectories, lagrangian

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Vectors | `vectors` | array | (the active array) |  |
| Release again every N steps (0 = once) | `reinject_every` | whole number 0 to 1000000 | 1 |  |
| Seed line start (no seed input) | `point1` | x, y, z | (NaN, NaN, NaN) |  |
| Seed line end (no seed input) | `point2` | x, y, z | (NaN, NaN, NaN) |  |
| Seeds on the line | `seeds` | whole number 1 to 100000 | 10 |  |
| Start time | `start_time` | number -∞ to ∞ | NaN | *advanced* |
| Terminal speed | `terminal_speed` | number 0 to ∞ | 0.000000000001 | *advanced* |

### Temporal Array Operator

Combine an array at two time steps (+ - * /), e.g. the change since the previous step.

Python: `temporal_array_operator` · also found by: difference over time, change, delta, time derivative, compare steps

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Array | `array` | array | (the active array) |  |
| Operator | `operator` | + / - / * / / | `-` |  |
| Relative to the current step | `relative` | on/off | on |  |
| Step shift (relative) | `shift` | whole number -100000 to 100000 | -1 |  |
| First step index | `first_step` | whole number 0 to 1000000 | 0 |  |
| Second step index | `second_step` | whole number 0 to 1000000 | 0 |  |
| Output name suffix (default _add/_sub/_mul/_div) | `suffix` | text | (empty) | *advanced* |

### Temporal Cache

Keep the last time steps that were shown in memory, so stepping back and forth or replaying a loop does not read and filter them again.

Python: `temporal_cache` · also found by: cache time steps, memory, faster playback, keep steps

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Steps kept | `size` | whole number 1 to 100000 | 8 |  |

### Temporal Interpolator

Blend the two nearest time steps so any time in between can be shown; optionally add regular time steps.

Python: `temporal_interpolator` · also found by: interpolate time, smooth animation, in-between steps, resample time

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Time step spacing (0 = input steps) | `interval` | number 0 to ∞ | 0 |  |

### Temporal Shift Scale

Shift and stretch the time axis: output time = (input time + pre shift) x scale + post shift.

Python: `temporal_shift_scale` · also found by: time offset, time scale, align time, shift time

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Pre shift | `pre_shift` | number -∞ to ∞ | 0 |  |
| Scale | `scale` | number -∞ to ∞ | 1 |  |
| Post shift | `post_shift` | number -∞ to ∞ | 0 |  |

### Temporal Snap to Time Step

Show the data at one of its own time steps when the window is at a time in between (set by other data or by Temporal Interpolator): the nearest step, the one before or the one after.

Python: `temporal_snap_to_time_step` · also found by: snap time, nearest step, no interpolation, time step rounding

| Setting | Python | Takes | Default | |
|---|---|---|---|---|
| Snap to | `mode` | Nearest / Previous / Next | `Nearest` |  |

### Temporal Statistics

Average, minimum, maximum and standard deviation of every array over all time steps.

Python: `temporal_statistics` · also found by: time average, mean over time, max over time, std over time


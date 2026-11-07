# Grains and crystals

auraFX reads electron backscatter diffraction (EBSD) maps, serial-section stacks and voxel
microstructures, and works with crystal orientations. It colors them, converts them between
representations and re-expresses them in a turned reference frame. It also crops, resamples
and aligns voxel volumes. It finds the grains (features) and measures them: sizes, shapes,
average orientations, misorientations, neighbours and boundaries. Texture analysis and
building synthetic microstructures come in later releases.

## EBSD maps

Open a map with **File › Open** or `afx.load("scan.ang")`.

| Format | Extension | Notes |
|---|---|---|
| TSL/EDAX | `.ang` | square grids; hexagonal grids are not read yet |
| Oxford | `.ctf` | angles in degrees in the file, converted to radians |
| EBSD stacks | `.h5ebsd` | serial sections from TSL or Oxford (HKL) maps, read as one volume |
| EDAX OIM HDF5 | `.h5` | square-grid scans; choose the scan with the **Scan** reader option |
| Bruker Esprit HDF5 | `.h5` | angles in degrees in the file; phases from their space group |
| Microstructures | `.dream3d` | voxel volumes and meshes with their cell, feature and phase arrays; see below |

`.h5` files are recognised by their contents, so other HDF5 files with that extension still
open with their own readers.

A map becomes image data with one cell per measured point, the cells centered on the points
(so the map's corner lies half a step before the first point). Its arrays are:

- **Cell data:** `EulerAngles` (three Bunge angles φ1, Φ, φ2 in radians, as stored in the
  file) and `Phases` (the phase of each point, 0 where the pattern was not indexed). A `.ang`
  file adds `Image Quality`, `Confidence Index`, `SEM Signal` and `Fit`. A `.ctf` file adds
  `Bands`, `Error`, `MAD`, `BC` and `BS`.
- **Field data:** one entry per phase, entry 0 standing for "not indexed".
  `CrystalStructures` holds the crystal class of each phase (see the numbers below),
  `MaterialName` its name and `LatticeConstants` a, b, c, α, β, γ.

A single-phase `.ang` scan writes phase 0 for every point. auraFX counts those points as
phase 1, unless their confidence index is negative.

An `.h5ebsd` stack becomes one volume with a cell layer per slice, the slices `Z Resolution`
apart. The stack's sample and Euler transformations are not applied. They are kept as the
field arrays `SampleTransformationAngle`, `SampleTransformationAxis`,
`EulerTransformationAngle` and `EulerTransformationAxis`. Apply them with Rotate Orientations
(and Transform for the sample) if you need them.

**Hexagonal axes.** Oxford files describe hexagonal crystals with the [10-10] direction along
x. auraFX, `.ang` files and the color key put [2-1-10] along x. By default the `.ctf` reader
adds 30° to φ2 of hexagonal and trigonal phases to match. To keep the angles as stored, set
the reader option **Hexagonal axes** to *As stored* in the Properties panel, or in Python:

```python
m = afx.load("titanium.ctf", hexagonal_axes="As stored")
```

### Crystal classes

| Number | Class | | Number | Class |
|---|---|---|---|---|
| 0 | Hexagonal 6/mmm | | 6 | Orthorhombic mmm |
| 1 | Cubic m-3m | | 7 | Tetragonal 4/m |
| 2 | Hexagonal 6/m | | 8 | Tetragonal 4/mmm |
| 3 | Cubic m-3 | | 9 | Trigonal -3 |
| 4 | Triclinic -1 | | 10 | Trigonal -3m |
| 5 | Monoclinic 2/m | | 999 | Unknown |

Face-centered and body-centered cubic metals (nickel, aluminium, iron) are Cubic m-3m.
Hexagonal close-packed metals (titanium, magnesium, zirconium) are Hexagonal 6/mmm.

### Microstructure files (.dream3d)

A `.dream3d` file holds one or more data containers, each a geometry with arrays. auraFX reads
image (voxel), rectilinear, vertex, edge, triangle, quadrilateral, tetrahedral and
hexahedral geometries.

- Cell arrays (voxels, triangles, ...) become cell data, and vertex arrays become point data.
- Feature arrays (one row per feature, such as `AvgQuats`, `Volumes` or `Centroids`) and phase
  arrays (`CrystalStructures`, `LatticeConstants`, `MaterialName`) become field data.
- Neighbor lists are not read.
- A file with several data containers opens as blocks. The **Data container** reader option
  picks one.
- Large volumes load faster with only the arrays you need. Give their names in the **Arrays**
  reader option, separated by commas.

```python
v = afx.load("volume.dream3d", arrays="EulerAngles, Phases, FeatureIds")
```

![A window of a real additively manufactured nickel alloy volume read from a .dream3d file, drawn with the inverse pole figure colors stored in it](images/dream3d_volume.png)

## Orientation conventions

An orientation turns the sample axes into the crystal axes. A direction with sample
coordinates *s* has crystal coordinates *g s*, where *g* is the orientation matrix. Euler
angles are Bunge angles in radians: *g* = Rz(φ2) Rx(Φ) Rz(φ1). Quaternion arrays store
(x, y, z, w), with w ≥ 0. Orientation matrices are stored row by row (9 components).

## Orientation Colors

**Filters › Grains › Orientation Colors** adds an 8-bit RGB array named `IPFColor`. Each
point is colored by one of two keys:

- **Inverse pole figure** (the default). The reference direction, z by default (the map
  normal), is turned into crystal coordinates. The crystal's symmetry brings it into the
  standard triangle of its class, and its place there gives the color. For cubic crystals,
  [001] is red, [101] green and [111] blue. For hexagonal crystals, [0001] is red,
  [2-1-10] green and [10-10] blue. Choose x or y as the reference direction to see the
  orientation along the map's edges.
- **Euler angles**: φ1, Φ and φ2 scaled to red, green and blue.

The class of each point comes from its phase and `CrystalStructures`, as EBSD files give
them. For data without phases, choose the class under **Crystal structure**. Points not
indexed, of an unknown class, or where the optional **Mask** array is 0, are black. The
orientation array can hold Euler angles (3 components), quaternions (4) or orientation
matrices (9). If none is chosen, the filter uses `EulerAngles`, `Quats` or
`OrientationMatrix`, whichever it finds first.

```python
m = afx.load("scan.ang")
ipf = m.orientation_colors(direction=(0, 0, 1))      # key="Euler angles" for the other key
ipf.set(color_by="IPFColor")
```

### Arrays drawn as their own colors

An array of 8-bit values with 3 or 4 components, like `IPFColor`, is drawn with its own
colors when you color by it, not through a color map. No legend is shown for it. To map
such an array through a color map instead, clear **Colors: the array's own colors** under
Color by in the Properties panel, or use `node.set(direct_colors=False)`. Choosing one component of the
array also uses the color map.

## IPF Color Key

**Sources › IPF Color Key** draws the color key of a crystal class: its standard triangle in
stereographic projection, colored as Orientation Colors colors it. It lies around the origin
in the xy plane, about half a unit across, so move and enlarge it with **Transform** to place
it next to a map.

```python
key = afx.ipf_color_key(structure="Cubic m-3m", resolution=60)
key.transform(scale=(20, 20, 1), translate=(42, 12, 0)).set(color_by="IPFColor")
```

![A nickel EBSD map colored by the inverse pole figure key along the map normal, with the cubic color key beside it](images/ebsd_ipf_map.png)

## Convert Orientations

**Filters › Grains › Convert Orientations** writes the orientations in another
representation. It reads and writes Euler angles, quaternions, orientation matrices,
axis-angle pairs (the unit axis and the angle in radians), Rodrigues vectors (the unit axis
and tan(ω/2)) and homochoric vectors. With **Reduce to the fundamental zone**, each
orientation is replaced by its symmetry equivalent with the smallest rotation angle (for cubic
crystals, at most 62.8°). The output is named after the representation (`Quats`,
`OrientationMatrix`, …) unless you give a name.

```python
q = m.convert_orientations(input_type="Euler angles", output_type="Quaternions", reduce=True)
```

## Rotate Orientations

**Filters › Grains › Rotate Orientations** re-expresses the orientations after the reference
frame is turned by an angle (degrees) about an axis. The points do not move; only the
description of their orientations changes.

- **Sample** frame: use this when the map's axes differ from those you want, such as a scan
  taken with the sample turned 90° about its normal.
- **Crystal** frame: turns the crystal axes the orientations refer to.

```python
r = m.rotate_orientations(frame="Sample", axis=(0, 0, 1), angle=90)
```

## Crop

**Filters › Grains › Crop** keeps the cells of a voxel volume or EBSD map whose centres lie
inside a box, given in sample coordinates (leave a side unset to keep everything on that
side). The cells keep their place in space. With **Renumber features** (on by default), the
remaining feature ids are numbered from 1 and the per-feature field arrays keep only the
rows of those features. To crop by grid index instead, use **Extract Subset**.

```python
c = v.crop_volume(min=(0, 0, 0), max=(100, 80, 20))
```

## Change Resolution

**Filters › Grains › Change Resolution** resamples a voxel volume or EBSD map onto cells of
another size. Each new cell takes the values of the old cell that holds its centre, so
orientations, phases and feature ids stay valid. The volume keeps its corner and, rounded to
whole cells, its size. Features that disappear are dropped and the rest renumbered, as in
Crop. Point data is not carried over.

```python
coarse = v.change_resolution(spacing=(2, 2, 2))
```

## Align Sections

**Filters › Grains › Align Sections** moves each z slice of a serial-section volume in x and
y, by whole cells, so it matches the slice below. Choose how slices are compared:

- **Misorientation** (the default): two cells match when their orientations differ by less
  than the **Tolerance** (5° by default), with the crystal class of their phase.
- **Feature ids**: two cells match when they belong to the same feature.
- **Given shifts**: no search. Each slice moves by the x and y cells you give, against the
  slice below.

The search starts from no move and steps to the neighbouring shift with fewer differing cells
until none is better, at most **Largest shift** cells each way. A **Mask** array (a sample or
good-data mask) limits the comparison to cells where it is not 0. Cells uncovered by a move
are set to 0 (phase 0, not indexed). The total shift of each slice is kept as the field array
`SectionShifts` (x, y cells). Point data is not carried over.

```python
a = stack.align_sections(method="Misorientation", tolerance=5, mask="SampleVoxels")
a.align_sections(method="Given shifts", shifts=[2, -1, 3, 1, -1, 2])   # 3 slices after the first
```

![A stack of four shifted slices colored by orientation, as read (left) and after Align Sections (right); both are stretched along z, and the black edges are cells uncovered by the moves](images/align_sections.png)

## Features (grains)

A feature is a connected set of cells, cells sharing a face, that belong together: usually a
grain. **Segment Features** finds them and writes the cell array `FeatureIds` (0 for cells
in no feature). The other feature filters then add per-feature results as field arrays, one
row per feature id (row 0 stands for id 0). All of them are under **Filters › Grains** and work
on voxel volumes and on flat EBSD maps.

A typical sequence for an EBSD volume:

```python
v = afx.load("scan.dream3d")
g = (v.segment_features(tolerance=2, mask="SampleVoxels")
      .fill_bad_data(min_defect=7)
      .minimum_size(min_cells=7)
      .feature_sizes().feature_centroids().feature_shapes()
      .average_orientations().reference_misorientations()
      .kernel_average_misorientation(kernel=(2, 2, 1))
      .feature_neighbors())
table = g.feature_table()           # one row per grain, for the Spreadsheet, charts and CSV
g.feature_colors().set(color_by="FeatureColors")
```

![An EBSD map segmented at 5 degrees, each grain in its own color, with the boundaries between grains](images/grain_map.png)

![A window of a real nickel alloy volume segmented into grains (2 degrees, within the sample mask, small defects and grains under 7 cells cleaned up), colored by grain](images/grain_volume.png)

### Segment Features

Neighbouring cells join the same feature when they pass the test of **Group by**:

- **Misorientation**: same phase, and misorientation below the **Tolerance** in degrees.
- **C-axis alignment**: same phase, and crystal c axes within the tolerance either way round.
  This is for hexagonal crystals.
- **Scalar**: values of the chosen cell array differ by at most the tolerance.

Cells outside the **Mask** (cells where it is 0), and cells of no known crystal class, stay
at 0. Features are numbered in the order their first cell appears (x fastest, then y, then
z). **Shuffle ids** numbers them in a shuffled order instead, which gives better colors
when coloring by id. The field arrays `Active` and `FeaturePhases` (the phase of each
feature) are added, and per-feature arrays from an earlier segmentation are removed.

### Cleaning up

- **Fill Bad Data**: clusters of cells without a feature that are smaller than **Smallest
  kept defect** are filled from the surrounding features. Each cell takes every cell array
  from the neighbour whose feature most of its neighbours belong to, sweep after sweep.
  Larger clusters (pores, the outside of the sample) stay 0.
- **Minimum Size**: features with fewer cells than **Fewest cells** are removed and filled
  from their neighbours the same way. Feature 0 can win, so a small feature against the
  outside may become outside. The remaining features are renumbered from 1 in their old
  order, and per-feature field arrays keep their rows.

### Per-feature results

| Filter | Field arrays (per feature) | Cell arrays |
|---|---|---|
| Feature Sizes | `NumElements`, `Volumes`, `EquivalentDiameters` (sphere of equal volume; circle of equal area on flat maps) | |
| Feature Centroids | `Centroids` (mean of the cell centres) | |
| Feature Shapes | `AxisLengths` (semi-axes a ≥ b ≥ c of the ellipsoid with the same moments of inertia), `AspectRatios` (b/a, c/a), `AxisEulerAngles` (Bunge angles of the axes: the rows of the rotation are the a, b and c directions), `Omega3s` (1 for an ellipsoid, smaller for other shapes) | |
| Average Orientations | `AvgQuats` (x, y, z, w), `AvgEulerAngles` | |
| Reference Misorientations | `FeatureAvgMisorientations` | `FeatureReferenceMisorientations` (degrees from the feature's average orientation) |
| Kernel Average Misorientation | | `KernelAverageMisorientations` (degrees) |
| Feature Neighbors | `NumNeighbors`, `SurfaceFeatures` (1 when touching the edge of the volume or cells without a feature), `AvgMisorientations` (mean misorientation to the neighbouring features of the same crystal class, when average orientations exist) | `BoundaryCells` (face neighbours in another feature) |
| Distance to Boundaries | | `GBEuclideanDistances`, `TJEuclideanDistances`, `QPEuclideanDistances` (distance between cell centres to the nearest boundary, triple-line and quadruple-point cell; -1 where there is none) |
| Merge Twins | `ParentIds` | `ParentIds` |

- **Feature Shapes** uses the exact second moments of the cells: each cell counted at its
  centre with its own inertia. On flat maps it gives the ellipse of the same moments: a and b,
  b/a, and the angle of the long axis in the plane as the first Euler angle.
- **Average Orientations** adds, cell by cell, the symmetry equivalent nearest the running
  average (the identity for the first cell), then normalises.
- **Kernel Average Misorientation** averages a cell's misorientation to the cells of the same
  feature within **Kernel** cells along x, y and z (1 1 1 means the 26 neighbours). The cell
  itself counts, with 0. Cells without a feature get 0.
- **Merge Twins** gives neighbouring cubic m-3m features a common parent when their average
  orientations differ by 60° about a ⟨111⟩ axis, within the **Axis** and **Angle**
  tolerances. Chains of twins share one parent. Parents are numbered in the order of their
  first feature.

### Boundaries, colors and the table

- **Feature Boundaries** gives the cell faces between different features as a surface. Each
  face has `FeatureIdA` and `FeatureIdB`, the `Misorientation` (degrees) between the two
  cells, and `Twin` (1 on twin boundaries of cubic m-3m crystals). **Include the outer
  surface** adds the faces against cells without a feature and the edge of the volume.
  Draw it as **Wireframe** over a flat map to see the boundaries as lines.
- **Feature Colors** gives each feature a distinct, stable color (`FeatureColors`, drawn as
  its own colors).
- **Feature Table** turns the per-feature field arrays into a table, one row per feature with
  a `FeatureId` column, for the Spreadsheet, charts and **Export CSV**.

![The per-feature results of an EBSD map in the Spreadsheet as a Feature Table](images/feature_table.png)

## Texture

The texture filters (**Filters › Grains**) describe how the orientations of a map or volume
are distributed. Every density is in multiples of a random distribution (MRD): 1 is what
randomly oriented crystals give, 3 means three times as often as random. Each density is a
smoothed count: every orientation spreads over a Gaussian of the **Smoothing** width.

The texture filters work on one phase:
- **Phase** chooses it. With 0, every indexed point is used, and they must all have one
  crystal class.
- **Mask** can leave out more points.
- **Orientations used at most** caps how many are counted (60 000 for the figures, 10 000 for
  the ODF). When there are more, a random subset with a fixed seed is taken, so the result is
  the same each time.

The figures come out as flat panels side by side in the view, colored by `MRD` or `ODF`. To
keep the numbers, save the output as CSV: every panel goes into one file, with a `Block`
column naming the panel and the direction or angles of each point.

### Pole Figure

**Pole Figure** shows where the crystal directions of a family point in the sample.
- **Directions** lists the families, written like `100, 110, 111`, `-112` or `1 2 3`.
  Every symmetry-equivalent direction and its opposite are counted.
- Each family is a disk: sample X to the right, Y up, sample Z at the centre and the
  horizontal plane at the rim.
- **Projection** is *Stereographic*, which keeps angles, or *Equal area*, which keeps areas.
- The directions are first gathered in **Bin size** cells (2° by default) before smoothing.
  This keeps large maps fast.

```python
pf = ebsd.pole_figure(families="100, 110, 111", projection="Stereographic", sigma=7.5)
```

![The {100}, {110} and {111} pole figures of a window of a measured nickel-superalloy volume](images/pole_figures.png)

### Inverse Pole Figure

**Inverse Pole Figure** shows which crystal directions lie along a sample direction (X, Y
or Z, or all three side by side). The density is drawn on the standard triangle of the
crystal class, the same region the orientation color key uses. A fibre texture shows here
as one bright corner or spot.

```python
ipf = ebsd.inverse_pole_figure(directions="X, Y and Z")
```

![Inverse pole figures for sample X, Y and Z on the cubic standard triangle](images/inverse_pole_figures.png)

### ODF Sections

**ODF Sections** computes the orientation distribution function (ODF): how often each
orientation occurs, smoothed over the disorientation angle.
- Each section has a constant φ2 (**Sections**, 0, 45 and 65° by default). φ1 runs across
  from 0 to 360°, and Φ runs down from 0 to 90° for classes with a two-fold axis across the
  c axis (otherwise to 180°).
- Each point's Euler angles are kept with its value.
- The output also carries the **texture index** `TextureIndex`: the mean of the squared ODF
  over all orientations. It is 1 for a random texture and grows as the texture sharpens.
- `MaxODF` is the ODF's largest value. Both come from a grid of **Texture index step**
  degrees over the whole orientation space.
- With a finite number of orientations, even a random set gives an index a little above 1:
  about 1.001 for 20 000 random orientations with 7.5° smoothing.

```python
odf = ebsd.odf_sections(sections="0, 45, 65", step=5)
print(odf.array("TextureIndex", "field"))
```

![ODF sections at phi2 = 0, 45 and 65 degrees](images/odf_sections.png)

### Misorientation Distribution

**Misorientation Distribution** makes a table of disorientation angles (the smallest
rotation between two orientations, allowing for crystal symmetry) with three columns:
- **Neighbours:** between neighbouring points of a map or volume.
- **Random pairs:** between randomly chosen pairs of points (**Random pairs** of them). This
  shows what the texture alone gives, without any spatial correlation.
- **Uniform:** between randomly oriented crystals, computed exactly for the crystal class.

Each column is a density per degree, so all three can be plotted on one axis. Angles below
**Smallest angle** (5° by default, meant to leave out the small angles inside grains) are
left out of all three. Peaks in Neighbours that are missing from the other two point to
special boundaries, such as twins near 60° in cubic metals.

The table plots in the **Charts** panel with `bin_center` on the x axis. Its field data
counts the neighbour pairs and gives their mean angle.

```python
m = ebsd.misorientation_distribution(min_angle=5, bin=1)
x = m.array("bin_center")
afx.plot(x, m.array("Neighbours"), label="Neighbours", clear=True)
afx.plot(x, m.array("Uniform"), label="Uniform")
```

![The misorientation distribution of a measured nickel-superalloy volume in the Charts panel](images/misorientation_distribution.png)

### Texture Components

**Texture Components** makes a table of the fraction of orientations that have a crystal
direction of a family within **Cone** degrees of a sample axis. It has one row per direction
and axis, such as `<100> Z` for orientations with a cube axis along Z. The defaults are the
families `100, 110, 111` against the axes `X, Y, Z`, with a 15° cone.

```python
tc = ebsd.texture_components(families="100, 111", axes="Z", cone=15)
```

### Chord Lengths

**Chord Lengths** measures grain size along straight lines through a map or volume.
- **Lines.** They run in x, y and, for a volume, z. **Line spacing** is the distance between
  them, in length units; 0 uses every row of points.
- **Boundaries.** A boundary lies between two neighbouring points whose disorientation is at
  least **Boundary angle** (10° by default). A chord is the distance between two boundaries
  that follow each other along a line.
- **Unused points.** Points of other phases, unindexed points and masked points are skipped.
  A run of more than **Largest gap** of them ends the line segment there.
- **Partial chords.** A chord that touches a segment's end, or the edge of the data, is left
  out, because its true length is unknown.
- **Specks.** **Ignore single-point specks** stops a single stray point, whose two
  neighbours agree with each other, from counting as two boundaries.

**Output** gives either every chord (its direction and length) or a histogram with one column
per direction. Both carry the mean chord and the number of chords per direction as field
data (`MeanChordX`, `ChordsX`, ...).

```python
ch = ebsd.chord_lengths(threshold=10, output="Histogram", bin=2)
print(ch.array("MeanChordX", "field"))
```

## Synthetic microstructures

These sources build microstructures without measured data. They have the same arrays an
EBSD reader gives: `FeatureIds`, `Phases` and `EulerAngles` per cell, and
`CrystalStructures`. Every filter in this chapter therefore works on them, and Overlay Mesh
turns them into a mesh. A fixed **Random seed** gives the same microstructure every time.

### Voronoi Grains

**Sources › Voronoi Grains** places **Grains** seeds at random in a grid of **Cells** and
gives every cell to its nearest seed.
- **Size spread** above 0 makes Laguerre grains: each seed gets a radius drawn from a
  lognormal distribution with that log-sigma, and a cell goes to the seed with the
  smallest squared distance minus the squared radius. Grain sizes then vary much more than
  in Voronoi grains.
- **Periodic** wraps the grains across opposite faces, for representative volume elements.
- **Orientations** are random, or a fibre with `<100>` or `<111>` along Z, scattered by
  **Fibre spread** and random about Z.
- The seed positions are kept as the field array `Seeds`.

```python
v = afx.voronoi_grains(cells=[64, 64, 64], grains=300, size_sigma=0.4, periodic=True)
```

![Laguerre grains colored by the inverse pole figure key](images/voronoi_grains.png)

### Columnar Grains

**Sources › Columnar Grains** grows the columnar grains of an additive build along Z:
- **Columns.** As many columns as fit one **Grain width** apart are alive at every height.
  Each is tilted at random (**Column tilt**) and grows to a lognormal height (**Median
  grain height**, **Height spread**).
- **Ending.** A column also ends when its neighbours grow over it. Each slice of the box is
  shared among the columns alive there, so a column can lose its whole cross-section.
- **Renewal.** A column that ends is replaced at that height by a new grain at a random
  position.
- **Widths.** **Width spread** and **Width grows with height** make the columns differ in
  width, with tall grains wider.
- **Orientations.** By default every grain has a `<100>` direction near Z, as in many
  additively built cubic metals.
- **Per-grain field arrays:** `GrainBase`, `GrainTop`, `GrainTilt` and `Originating`
  (1 for grains that start inside the build rather than on the base plate).

```python
g = afx.columnar_grains(size=(300, 300, 450), voxel=5, width=50, height=150)
```

![Columnar grains of an additive build colored by the inverse pole figure key for X](images/columnar_grains.png)

### Add Orientation Noise and Jumble Orientations

**Add Orientation Noise** turns every point's orientation by a small random rotation. The
rotation is about a random axis, with an angle drawn from a normal distribution with
standard deviation **Angle**, so the mean turn is about 0.8 times **Angle**. With **One
rotation per feature**, every point of a grain turns alike.

**Jumble Orientations** gives the grains each other's orientations at random. The texture
stays the same, but any link between a grain's orientation and its neighbours' or its
shape is lost. Each grain takes the orientation of the first point of the grain it
swapped with.

```python
noisy = v.orientation_noise(sigma=2)
mixed = v.jumble_orientations()
```

### Generate Ensemble Statistics

**Generate Ensemble Statistics** describes a segmented microstructure (one with
`FeatureIds`) as a table of fits:
- **Size:** a lognormal fit of the equivalent diameters (`ESD mu` and `ESD sigma` of their
  logarithms), with the mean, smallest and largest diameter.
- **Shape:** beta-distribution fits of the aspect ratios b/a and c/b.
- **Neighbours:** a lognormal fit of the neighbour counts.

Features touching the outside are left out unless **Include features on the surface** is
on, because their size and neighbours are cut short. The values are also field data of the
table.

```python
st = v.ensemble_statistics(include_surface=True)
print(st.array("ESD sigma", "field"))
```

## Limits

- Hexagonal-grid `.ang` and EDAX files, Oxford `.h5oina` files, and `.dream3d` neighbor lists
  are not read yet.
- Cubochoric vectors are not among the representations.
- Neighbor lists (each feature's neighbours and shared areas) are not kept as arrays; Feature
  Boundaries holds the same information face by face.
- Merge Twins handles cubic m-3m crystals only.
- Texture directions are Cartesian crystal directions. For cubic crystals they are the Miller
  indices. For hexagonal and trigonal crystals, `001` is the c axis and `100` the a1 axis.
  Four-index Miller-Bravais notation is not read.
- The texture filters assume no sample symmetry and count every point equally.
- Voronoi and Laguerre grains are tessellations of random seeds, and columnar grains are
  power diagrams slice by slice. Neither packs grains to match given size and shape
  statistics.
- The feature filters copy the volume they work on: a volume of 80 million cells needs several
  gigabytes of memory. Segmentation of such a volume takes about 10 seconds.
- The orientation mathematics is checked against independent references: rotations from
  SciPy, disorientation angles by brute force over every crystal class, and the color key by
  a separate implementation. On a window of a real volume, the inverse pole figure colors
  agree with those stored in the file within one level of 255. The verification notes (`docs/reference/verification.md` in the
  source tree) list the comparisons.

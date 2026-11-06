# Grains and crystals

auraFX reads electron backscatter diffraction (EBSD) maps and works with crystal orientations:
it colors them, converts them between representations and re-expresses them in a turned
reference frame. This is the first part of the grain workbench; segmenting grains, grain
statistics and building synthetic microstructures come in later releases.

## EBSD maps

Open a map with **File › Open** or `afx.load("scan.ang")`.

| Format | Extension | Notes |
|---|---|---|
| TSL/EDAX | `.ang` | square grids; hexagonal grids are not read yet |
| Oxford | `.ctf` | angles in degrees in the file, converted to radians |

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

## Limits

- Hexagonal-grid `.ang` files, HDF5 EBSD files (`.h5ebsd`, EDAX and Bruker `.h5`) and
  `.dream3d` files are not read yet.
- Cubochoric vectors are not among the representations.
- The orientation mathematics is checked against independent references: rotations from
  SciPy, disorientation angles by brute force over every crystal class, and the color key by
  a separate implementation. The verification notes (`docs/reference/verification.md` in the
  source tree) list the comparisons.

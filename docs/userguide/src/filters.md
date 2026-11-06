# Filters

A filter takes the output of the object above it and makes a new object. Select an input,
choose the filter, adjust it in Properties. Every filter is also a method in Python
(`node.slice(...)`), and the assistant knows all of them.

![A slice, a contour and a clip of the same file](images/filters_pipeline.png)

## Common filters (toolbar)

| Filter | What it does | Main parameters |
|---|---|---|
| **Slice** | the data on a plane or a sphere, with the values interpolated onto it | type, origin, normal (or radius); the plane can be dragged in the view |
| **Clip** | keeps one side of a plane, the inside/outside of a box or sphere, or the cells above/below a scalar value; **crinkle** keeps whole cells | type, origin, normal, box corners, radius, scalar and value, invert |
| **Contour** | isosurfaces (or isolines on 2-D data) at one or more values | array, values; computes normals |
| **Threshold** | the cells whose values lie in a range | array, min, max, all/any point |
| **Calculator** | a new array from an expression over other arrays (`sqrt(u*u+v*v)`, `mag(V)`, vectors with `iHat`...) | expression, result name, point or cell |
| **Glyph** | arrows, cones, spheres, cubes or lines at the points, scaled and oriented by arrays | shape, orientation and scale arrays, factor, maximum glyphs |
| **Warp By Vector / Scalar** | moves points by a vector array (displacements) or along the normal by a scalar | array, factor |
| **Stream Tracer** | streamlines of a vector field from a line or a point cloud of seeds; **Tube** makes them solid | vectors, seed type, seed line or cloud, integration direction and length |
| **Plot Over Line** | samples the data along a line and plots it in Charts | the two end points (draggable) |
| **Probe Location** | the values at one point, shown in Information and the Spreadsheet | location (draggable) |

## Analysis

Resample To Image, Resample With Dataset, Histogram, Descriptive Statistics, Integrate
Variables (areas, volumes and integrals of every array), Gradient (with divergence, vorticity
and Q-criterion), Cell Size, Mesh Quality, Elevation, Cell Centers, Connectivity, Find Data,
Extract Selection, Temporal Statistics (mean, min, max and standard deviation over all time
steps), Temporal Interpolator, Temporal Shift Scale, Temporal Array Operator, and the particle
filters (Particle Tracer, Particle Path, Streak Line) that follow particles through a
time-varying flow. Texture Map to Plane / Cylinder / Sphere, Perlin Noise and Spatio Temporal
Harmonics are described [below](#textures-and-made-up-data).

## Geometry

Extract Surface, Extract Edges, Feature Edges, Outline, Transform, Reflect, Shrink, Clean,
Surface Normals, Smooth, Triangulate, Tetrahedralize, Tessellate (curved cells into straight
pieces), Mask Points, Merge Blocks, Extract Block, Extract Subset (a sub-volume of an image),
Append Datasets / Geometry / Attributes, Cell Data to Point Data and back, Extract Component,
Rename Array, Pass Arrays, and OpenFOAM Boundary Patches.

## Sources

**Sources** adds objects without a file: Sphere, Cone, Cylinder, Box, Plane, Line, Point
Cloud, Wavelet (a test volume), Spatio Temporal Harmonics (a test volume of waves with its own
time steps), AMR Gaussian Pulse (a small [AMR](file-formats.md#amr-data-adaptive-mesh-refinement)
dataset) and Unstructured Cell Types (one of every cell type, for tests).

## Textures and made-up data

### Showing an image on a surface

An image (PNG or JPEG) can be wrapped onto a surface, the way a world map goes onto a globe.
The surface needs **texture coordinates**, which say where each point falls in the image. Some
files and the Plane source come with them. For anything else, add one of these filters
(**Filters › Texture Map to Plane / Cylinder / Sphere**):

| Filter | How the image is laid on | Parameters |
|---|---|---|
| **Texture Map to Plane** | projected straight onto the surface from a plane, like a slide projector | **Automatic** fits the plane to the points and stretches the image over the bounds. Turn it off and give **Origin** (image corner), **Point 1** (the image's right edge) and **Point 2** (its top edge). |
| **Texture Map to Cylinder** | rolled around an axis: up the axis is up the image, around it is across | **Automatic** puts the axis along the object's longest side. Turn it off and give **Point 1** and **Point 2**, the axis ends. |
| **Texture Map to Sphere** | wrapped around a center: the image's top at the +z pole, its bottom at the −z pole | **Center** (under Show advanced; empty = the middle of the points) |

**Prevent seam** (cylinder and sphere, on by default) runs the image across half the way
round and back mirrored. That avoids a visible join, but shows the image twice. Turn it off
to wrap the image once, as for a world map. A thin seam then shows where the edges meet
(ParaView shows the same seam).

Then pick the image in **Properties › Display › Texture › Choose…** (× removes it). The row
appears when the object has texture coordinates. The image replaces the array colors and the
solid color; lighting and the material still apply. Coordinates outside 0 to 1 repeat the
image.

![A cylinder and a globe with the same map, from Texture Map to Cylinder and Texture Map to Sphere](images/texture_maps.png)

```python
ball = afx.sphere(radius=1, theta=128, phi=96)
globe = ball.texture_map_to_sphere(prevent_seam=False)
globe.set(texture="/path/to/world.jpg")       # texture=None removes it
p = d.texture_map_to_plane(automatic=False, origin=(0, 0, 0), point1=(10, 0, 0), point2=(0, 5, 0))
c = d.texture_map_to_cylinder(point1=(0, 0, -1), point2=(0, 0, 1), automatic=False)
```

The new array is called **Texture Coordinates**. The three filters give the same numbers as
ParaView's filters of the same names, and the coordinates can also be colored by like any
other array.

Limits:
- The image is shown on surfaces (Surface and Surface With Edges) and on lines. Points and
  volumes don't show it.
- Ray tracing, scene export and pictures drawn on a server show the solid color instead of
  the image.
- The coordinates are stored at half precision for drawing (about 1/2000 of the image
  width), which only matters for images repeated many times.

### Made-up data for tests and demos

| Filter or source | What it makes | Parameters |
|---|---|---|
| **Perlin Noise** (filter) | a smooth random-looking value at every point (array **PerlinNoise**), for test data or rough-looking surfaces | **Frequency** (bumps per unit along x, y, z), **Phase** (shifts the pattern), **Amplitude**, result name |
| **Spatio Temporal Harmonics** (filter) | a sum of travelling sine waves at every point (array **SpatioTemporalHarmonics**) | **Harmonics**: 6 numbers per wave (amplitude, temporal frequency, wave vector x y z, phase). The value is the sum of amplitude × sin(frequency × time + wave vector · point + phase). |
| **Spatio Temporal Harmonics** (source, **Sources** menu) | an image volume with those waves and **20 time steps of its own** (one period), so it animates without a file | **Whole extent** (x min, x max, y min, y max, z min, z max; spacing 1), **Harmonics**, **Time steps** |

The source's time steps join the time toolbar like a file's. The filter follows the time of
its input. On data without time steps it uses time 0, as ParaView does. Both match ParaView's
filter and source with the same defaults (ParaView rounds π/5 to 0.6283 in its defaults, and
so does auraFX).

![The harmonics source at its sixth time step, and a plane warped by Perlin noise](images/harmonics_noise.png)

```python
h = afx.spatio_temporal_harmonics_grid()     # waves in a 21×21×21 volume, 20 time steps
h.set(harmonics=[2, 1, 0.3, 0, 0, 0])         # one wave along x
afx.time(5)
n = afx.plane(xres=200, yres=200).perlin_noise(frequency=(4, 4, 4), amplitude=0.1)
n.warp_by_scalar(array="PerlinNoise")
```

## Programmable filters

**Programmable Filter**, **Programmable Source** and **Python Calculator** run Python that is
stored in the pipeline and in state files. The filter's code gets `data` (points and arrays
as NumPy) and returns it changed, or a new dataset built with `afx.make_*`. For code you want
to keep across sessions, write a [plugin](plugins.md).

## Filters with several inputs

Append Datasets, Append Geometry, Append Attributes and Resample With Dataset take several
inputs. Ctrl+click the inputs in the Pipeline, then choose the filter; the Properties panel
gets an **Inputs** section where the order can be changed.

```python
afx.apply_multi("append_datasets", [a, b, c])
r = probe_mesh.resample_with_dataset(source)
```

## Parameters that say "auto"

Many parameters start as **auto (data center)** or similar: the value is taken from the data
each time it runs (the center of the bounds, the middle of a range, 5 % of the size). Press
**edit** to type a value instead. Auto values follow the data through time steps and across
pieces of a parallel server.

## Speed

Slice, Contour, Clip, Threshold, Extract Surface, Stream Tracer, Gradient and Integrate
Variables run on every core and give exactly the same result on any number of threads. A
slice of 4 million cells takes about 80 ms. The thread count can be limited in **Settings ›
Filter threads** (0 = all cores) or with `AURAFX_THREADS`.

## From Python

```python
afx.filters()                 # every filter with its parameters
afx.help_filter("clip")       # one filter explained
s = d.slice(normal=(0, 1, 0))
c = d.contour(array="Temp", values=[400, 600])
k = d.clip(type="Box", box_min=(-3, -3, -2), box_max=(3, 3, 4))
t = d.threshold(array="Pres", min=0.01)
s.set(origin=(0, 1, 0))       # change a parameter; the filter re-runs
s.update()                    # wait for it
```

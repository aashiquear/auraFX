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
time-varying flow.

## Geometry

Extract Surface, Extract Edges, Feature Edges, Outline, Transform, Reflect, Shrink, Clean,
Surface Normals, Smooth, Triangulate, Tetrahedralize, Tessellate (curved cells into straight
pieces), Mask Points, Merge Blocks, Extract Block, Extract Subset (a sub-volume of an image),
Append Datasets / Geometry / Attributes, Cell Data to Point Data and back, Extract Component,
Rename Array, Pass Arrays, and OpenFOAM Boundary Patches.

## Sources

**Sources** adds objects without a file: Sphere, Cone, Cylinder, Box, Plane, Line, Point
Cloud, Wavelet (a test volume) and Unstructured Cell Types (one of every cell type, for tests).

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

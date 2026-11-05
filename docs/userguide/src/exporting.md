# Exporting pictures, scenes and animations

## Screenshots

**File › Save Screenshot…** (Ctrl+Shift+S) opens a small dialog:

- **Size**: the view's size, a multiple of it (2×, 4×), 1920×1080, 3840×2160, or any size;
  **keep the view's shape** ties width and height together. Point sizes and line widths are
  in pixels, so they look thinner in a larger picture.
- **Background › transparent** saves a PNG with an alpha channel, with clean edges.
- **Annotations** includes the legend, axes-grid labels, time label, text annotations and
  the orientation axes, laid out as on screen and scaled with the picture.
- **PNG** or **SVG**: an SVG holds the surfaces as shaded polygons sorted back to front, the
  lines and points, and the annotations as real text, for figures that scale without pixels
  (where surfaces cut through each other the boundary is jagged; convert to PDF with Inkscape
  if needed).

With ray tracing on, the picture is traced at the requested size first.

```python
afx.screenshot("fig.png")                               # the view as shown
afx.screenshot("fig.png", width=3840, transparent=True) # keeps the view's shape
afx.screenshot("fig.png", width=1600, height=900, annotations=False)
afx.screenshot("fig.svg")
```

`afx.screenshot` returns only when the file is written, so a script can change the camera or
the data between pictures.

## Scenes for other programs

**File › Export Scene…** (`afx.export_scene("scene.glb")`) writes the visible objects, with
their colors, as:

| Format | Notes |
|---|---|
| **glTF binary** (`.glb`) | for Blender, three.js, PowerPoint, Windows 3D Viewer; vertex colors, normals, translucency, materials (metallic, roughness) |
| **OBJ** (`.obj`) | with vertex colors after the coordinates (Blender and MeshLab read them) |
| **X3D** (`.x3d`), **VRML** (`.wrl`) | absolute coordinates, vertex colors; ParaView reads the VRML |

Volumes are not exported (they are not geometry).

## Animations

**File › Animation…** records the time steps, a turntable or the keyframes as **GIF**, **MP4**
(`ffmpeg` on the PATH) or a folder of PNG frames; see
[Time, charts and animation](time-animation.md).

## Saving data

Right-click an object › **Save data…**, or `node.save("out.vtu")`, writes its output; the
format follows the extension:

| Extension | Format |
|---|---|
| `.vtu .vtp .vti .vtr .vts .vtm` | VTK XML (appended binary); `.vtm` writes a folder of pieces |
| `.vtk` | legacy VTK |
| `.vtkhdf` | VTKHDF (HDF5); multiblocks as a collection with the block tree; `all_times=True` writes every time step into one file |
| `.exo` | Exodus II with blocks and variables; `all_times=True` writes every time step |
| `.csv` | the point data or the table |
| `.stl .ply` | surfaces |

```python
d.save("slice.vtp")
d.save("run_all.exo", all_times=True)
d.save("run_all.vtkhdf", all_times=True)
```

When the data has several time steps and the chosen format holds them all (Exodus, VTKHDF),
the Save dialog writes every step.

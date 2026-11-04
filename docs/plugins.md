# Writing auraFX plugins

A plugin is a Python file that adds filters, sources and file readers. They appear in the
Filters and Sources menus, in Ctrl+K search, in Python (`node.my_filter()`, `afx.my_source()`)
and to the AI assistant, exactly like the built-in ones.

## Where plugins live

- The plugin folder: **Edit › Plugins… › Open plugin folder** (`~/.config/aurafx/plugins` on
  Linux, the auraFX config folder on macOS and Windows). Every `.py` file in it is loaded.
- Folders listed in the `AURAFX_PLUGIN_PATH` environment variable.
- Files or folders added with **Add file or folder…** in the Plugins window.
- From a script: `afx.load_plugin("path/to/tools.py")`.

Plugins reload when you save them; nodes that use their filters re-run. Files whose name starts
with `_` are skipped (use them for shared helpers). A plugin that fails to load shows its error
in the Plugins window and the Log; switch a plugin off with its checkbox.

**New plugin…** in the Plugins window writes a working example and opens it in your editor.

## Data

Filters receive an `afx.Data`, a copy of the input:

| | |
|---|---|
| `data.points` | N x 3 NumPy array (assign to move points) |
| `data.point_data["T"]`, `data.cell_data[...]`, `data.field_data[...]` | NumPy arrays; assign to add or replace, `del` to remove |
| `data.n_points`, `data.n_cells`, `data.bounds`, `data.type` | |
| `data.cells` | (offsets, connectivity, VTK cell types) |
| `data.blocks()` | pieces of a multi-block dataset |
| `data.copy()` | |

ParaView spellings work too: `data.PointData.append(array, "name")`.

Build new data with `afx.make_points(xyz)`, `afx.make_polydata(points, polys=[[0, 1, 2]],
lines=..., verts=...)`, `afx.make_image(dims, spacing, origin)`,
`afx.make_grid(points, cells, vtk_types)` and `afx.make_multiblock([a, b])`.

## Filters

```python
import aurafx as afx
import numpy as np

@afx.filter(category="Flow")
def speed(data, vectors=afx.ArrayParam(vector=True), name="speed"):
    """Magnitude of a vector array."""
    v = data.point_data[vectors]
    data.point_data[name] = np.linalg.norm(v, axis=1)
    return data
```

Keyword arguments become parameters in the Properties panel. Their type comes from the default:
`float`, `int`, `bool`, `str`, a 3-element tuple (vector), `afx.Choice([...])` (a list),
`afx.ArrayParam(vector=False, cells=False)` (an array of the input, `None` until chosen) and
`afx.Code("...")` (a multi-line text box). Or pass `params={"factor": 2.0}` to the decorator.

The function returns an `afx.Data` (or `None` after changing `data` in place). The docstring's
first line is the description. Other decorator options: `label`, `description`, `keywords`,
`id`, and `inputs` for several inputs: `inputs=2` or `inputs=(2, None)` (two or more) makes the
function receive a list, and the Properties panel gets an Inputs section.

## Sources and readers

```python
@afx.source(category="Shapes")
def helix(turns=4.0, points=400):
    t = np.linspace(0, turns * 2 * np.pi, int(points))
    return afx.make_polydata(np.c_[np.cos(t), np.sin(t), t / 10], lines=[list(range(len(t)))])

@afx.reader(extensions=["xyzt"], label="XYZ + temperature")
def read_xyzt(path):
    a = np.loadtxt(path, ndmin=2)
    d = afx.make_points(a[:, :3])
    d.point_data["T"] = a[:, 3]
    return d
```

Readers take precedence over built-in readers for their extensions and appear in the Open
dialog.

## Rules

- Filter, source and reader functions run on a worker thread while the pipeline runs. Do not call
  pipeline functions (`afx.load`, `node.slice()`, `show`, ...) inside them; that is reported as
  an error of the filter.
- Ids must be unique and cannot replace built-in filters.
- One-off code does not need a plugin: the **Programmable Filter**, **Programmable Source** and
  **Python Calculator** run Python stored in the pipeline (and in `.afx` state files).

Native (Rust) plugins are not supported yet.

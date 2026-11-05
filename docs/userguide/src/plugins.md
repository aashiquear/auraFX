# Plugins

![The Plugins window](images/plugins.png)

A plugin is a Python file that adds filters, sources and file readers. They appear in the
Filters and Sources menus, in the command palette, in Python (`node.my_filter()`,
`afx.my_source()`), in the Open dialog (readers) and to the AI assistant, like the built-in
ones.

## Where plugins live

- The plugin folder: **Edit › Plugins… › Open plugin folder** (`~/.config/aurafx/plugins` on
  Linux, the auraFX config folder on macOS and Windows). Every `.py` file in it is loaded.
- Folders listed in the `AURAFX_PLUGIN_PATH` environment variable.
- Files or folders added with **Add file or folder…** in the Plugins window.
- From a script: `afx.load_plugin("path/to/tools.py")`.

Plugins reload when you save them, and the objects that use their filters re-run. Files whose
name starts with `_` are skipped (use them for shared helpers). A plugin that fails to load
shows its error in the Plugins window and the Log; switch a plugin off with its checkbox.
**New plugin…** writes a working example and opens it in your editor.

## A filter

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

Keyword arguments become parameters in Properties; their type comes from the default:
`float`, `int`, `bool`, `str`, a 3-tuple (a vector), `afx.Choice([...])`,
`afx.ArrayParam(vector=False, cells=False)` (an array of the input) and `afx.Code("...")`
(a text box). The docstring's first line is the description. Other decorator options:
`label`, `description`, `keywords`, `id`, and `inputs=2` or `inputs=(2, None)` for filters
with several inputs (the function receives a list).

`data` is an `afx.Data`, a copy of the input: `data.points` (N×3), `data.point_data["T"]`,
`data.cell_data[...]`, `data.field_data[...]` (assign to add, `del` to remove), `data.n_points`,
`data.n_cells`, `data.bounds`, `data.type`, `data.cells` (offsets, connectivity, VTK types),
`data.blocks()`, `data.copy()`. ParaView spellings such as `data.PointData.append(a, "name")`
work too. Build new data with `afx.make_points`, `afx.make_polydata`, `afx.make_image`,
`afx.make_grid` and `afx.make_multiblock`.

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

Readers take precedence over the built-in readers for their extensions.

## Rules

- Plugin functions run on a worker thread while the pipeline runs: do not call pipeline
  functions (`afx.load`, `node.slice()`, `show`…) inside them.
- Ids must be unique and cannot replace built-in filters.
- For one-off code, the **Programmable Filter**, **Programmable Source** and **Python
  Calculator** run Python stored in the pipeline (and in state files).
- Native (Rust) plugins are not supported yet.

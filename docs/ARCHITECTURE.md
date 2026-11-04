# auraFX architecture

```
                 ┌───────────────────────────── aurafx-app (egui) ─────────────────────────────┐
                 │ menus · toolbar · command palette · pipeline · properties · 3D view ·       │
                 │ charts · spreadsheet · Python console · AI assistant · simulations · log    │
                 └──────┬───────────────┬────────────────┬──────────────────┬─────────────────┘
                        │               │                │                  │
              ┌─────────▼───────┐ ┌─────▼──────┐ ┌───────▼───────┐ ┌────────▼────────┐
              │ aurafx-pipeline │ │ aurafx-    │ │ aurafx-python │ │ aurafx-ai       │
              │ Session (graph, │ │ render     │ │ PyO3 + `afx`  │ │ Ollama/Claude/  │
              │ time, display,  │ │ wgpu       │ │ module        │ │ OpenAI streams  │
              │ charts, trace)  │ │            │ │               │ └─────────────────┘
              └───┬─────────┬───┘ └────────────┘ └───────┬───────┘
                  │         │                            │
       ┌──────────▼───┐ ┌───▼────────────┐       ┌───────▼────────┐
       │ aurafx-io    │ │ aurafx-filters │       │ aurafx-jobs    │
       │ readers      │ │ registry +     │       │ local/SSH/Slurm│
       │              │ │ algorithms     │       │ GPUs, templates│
       └──────┬───────┘ └───┬────────────┘       └────────────────┘
              └──────┬──────┘
               ┌─────▼──────┐
               │ aurafx-data│
               └────────────┘
```

## Data model (`aurafx-data`)

* `ArrayData` is a typed vector (`i8 … f64`, `f16`, `String`), `Array` adds a name and a
  component count. Layout is tuple-major, matching VTK, so readers can decode straight into it.
* `Attributes` is an ordered map of arrays plus "active" designations (scalars, vectors,
  normals, …).
* Datasets mirror VTK: `ImageData`, `RectilinearGrid`, `StructuredGrid`, `PolyData`,
  `UnstructuredGrid`, `Table`, `MultiBlock`. Cells use VTK 9's offsets + connectivity layout
  and VTK cell type ids, so higher-order and polyhedral cells round-trip.
* `TimeSeries` holds per-step references (`File`, `FileFrame`, or an already loaded dataset);
  steps load lazily.

## Pipeline (`aurafx-pipeline`)

`Session` owns the graph. Each `Node` is a reader, source or filter with typed `Params`,
`DisplayProps`, its memoized output and an error slot. Editing a parameter marks the node and
its descendants dirty. A worker thread repeatedly calls `take_work` (the next dirty node
whose input is clean), executes it without holding the lock, and stores the result with
`complete`. The UI never blocks; Python uses `update_blocking`.

The session also carries what other subsystems need to see: global time, a Python trace of
UI actions, camera/screenshot/panel requests from scripts, chart series, and
`describe()`, the text summary sent to the AI assistant.

## Filters (`aurafx-filters`)

Algorithms are plain functions over `DataSet`. Each one is registered once as a `FilterDef`
with id, label, category, keywords and `ParamSpec`s. The UI builds the properties panel
from the specs, the command palette searches the keywords, Python exposes `node.<id>(**kw)`,
and the AI assistant gets the catalog in its system prompt.

Cutting filters (slice, clip, contour) share one core: cells are decomposed into simplices
and cut against a per-point signed function, with edge-point de-duplication and attribute
interpolation (`interp::PointBuilder`). Probing uses a binned simplex locator.

## Rendering (`aurafx-render`)

`tessellate` turns any dataset into triangle, line and point vertex streams, with
positions relative to a per-mesh origin for float precision and crease-angle normals. The
`Renderer` draws into an MSAA sRGB target that the UI shows as a texture. It also reads
pixels back for screenshots. `headless()` creates the same renderer without a window, for
batch rendering and tests.

## Python (`aurafx-python`)

The interpreter runs on its own thread (`worker.rs` in the app). The `aurafx` module talks
to the same `Session` behind a mutex, so scripts and the UI see one pipeline. Arrays
cross the boundary as NumPy arrays.

## Jobs (`aurafx-jobs`)

A `JobSpec` describes what to run, where (`Host`: local or SSH target), with which GPUs
(`CUDA_VISIBLE_DEVICES` / `HIP_VISIBLE_DEVICES`) and scheduler (direct or Slurm). SSH goes
through the system `ssh`/`scp` so user configuration just works. Logs stream into a ring
buffer; finished jobs can fetch result files into `~/.cache/aurafx` and ask the app to open
them.

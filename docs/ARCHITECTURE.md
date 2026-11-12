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
  `UnstructuredGrid`, `Table`, `MultiBlock`, `HyperTreeGrid` and `CellGrid`. Cells use VTK 9's
  offsets + connectivity layout and VTK cell type ids, so higher-order and polyhedral cells
  round-trip. Overlapping AMR is a `MultiBlock` of levels of image boxes with a `vtkGhostType`
  array hiding covered cells (`amr.rs`); molecules are poly data with an atomic-number array
  and bonds as lines. Hyper tree grids and cell grids carry a cached mesh of their visible
  leaves / linear cells, so every filter and the renderer see them as ordinary cells.
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

## Server (`aurafx-server`)

The app can run a `Session` on another machine: `aurafx-server --stdio` over SSH speaks a
versioned postcard protocol (`aurafx_pipeline::remote`, protocol 11). Reader and filter nodes
marked remote execute there; the client receives surfaces, pictures drawn by the server's
GPU (`aurafx_render::server`), or the data itself when it is small. `--workers N` splits the
data into pieces over processes or hosts and merges the results (`cluster.rs`, `pieces.rs`).

## Jobs (`aurafx-jobs`)

A `JobSpec` describes what to run, where (`Host`: local or SSH target), with which GPUs
(`CUDA_VISIBLE_DEVICES` / `HIP_VISIBLE_DEVICES`) and scheduler (direct or Slurm). SSH goes
through the system `ssh`/`scp` so user configuration just works. Logs stream into a ring
buffer; finished jobs can fetch result files into `~/.cache/aurafx` and ask the app to open
them.

## Grains and meshes

`aurafx-crystal` (orientation mathematics, crystal symmetries, slip systems, elastic
constants; no dependency on the rest of auraFX) and `aurafx-mesh` (implicit geometry and its
command language, structured and overlay-grid hex meshing, tetrahedra, quality metrics) are
the two libraries behind the grain workbench and the mesh generator; their filters and
sources register in `aurafx-filters` like every other one (`crystal.rs`, `features.rs`,
`crystal_texture.rs`, `synthetic.rs`, `grain_surface.rs`, `mechanics.rs`, `am.rs`,
`mesh_gen.rs`, `geometry_model.rs`). Two pieces live in `aurafx-pipeline` because they need
the session: `pipeline_json.rs` writes the chain of steps that made a node as JSON (recorded in
`.dream3d` files by `aurafx-io/dream3d_writer.rs`) and rebuilds pipelines from such files,
mapping other programs' numbered steps to auraFX filters; `cases.rs` writes solver cases
(MOOSE polycrystal, SPPARKS grain growth, PINNACLE crystal plasticity) from a node's data,
which the job templates in `aurafx-jobs/templates.rs` stage into the working directory,
uploading them to remote hosts. See `docs/design/grains-and-meshing.md`.

## CAD modeling environment

`aurafx-cad` holds a parametric CAD document (feature timeline, constrained sketches,
parameters with unit-aware expressions, configurations) that is plain serde data, and the
kernels that evaluate it: an implicit kernel on `aurafx-mesh` and a B-rep kernel with the
STEP/IGES/3MF/`.sc` formats through the SolidCraft crates carried in `vendor/` (path
dependencies, always built; its UI is not pulled in). The app's `cad/` module is the
environment: a mode of the one window with its own ribbon, Model tree and Feature panel,
while other views are stale; applying runs the downstream check in
`aurafx-pipeline/src/cad_check.rs` (reference collection, geometry diff, default fixers, an
assistant prompt) before the pipeline re-runs. Design and stages: `docs/design/cad-environment.md`.

## Scale: servers, pieces and the GPU

`aurafx-server` is the pipeline without the window: the app sends it the recipe of a node
(`aurafx-pipeline/src/remote.rs`, a length-prefixed binary protocol over SSH's standard streams)
and gets back a summary, the data, its outer surface or pictures. A parallel server is a
coordinator with workers that each hold one piece (`cluster.rs`): cell-by-cell filters run on
every piece, filters that look at neighbours get a layer of ghost cells first
(`aurafx-filters/src/ghost_exchange.rs`), and anything else runs on the gathered data. Workers
are the server's own processes, ranks started by `mpirun`/`srun` that connect back, or (for
files larger than memory) pieces read one after another in one process.

`aurafx-gpu` holds compute kernels (wgpu) that filters may use through
`aurafx-filters/src/gpu.rs`. The GPU works in 32-bit and only narrows down which cells a cut
can touch; the CPU finishes them in 64-bit, so results do not depend on the GPU.

## Plugins and notebooks

Python plugins register filters, sources, readers and panels from the embedded interpreter
(`aurafx-python/src/plugins.rs`). Rust plugins are shared libraries written against
`aurafx-plugin`: one exported C table, data passed as bytes, versions checked at load
(`aurafx-filters/src/rust_plugins.rs`). The app is also a Jupyter kernel
(`aurafx-app/src/jupyter.rs`, the messaging protocol over ZeroMQ) and has a Notebook panel that
reads and writes `.ipynb` files.

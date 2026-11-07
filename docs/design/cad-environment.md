# CAD modeling environment

Design for section 13 of the roadmap: a parametric CAD modeler inside auraFX that creates,
imports, edits and exports geometry, and hands the result back to the regular pipeline with a
check of everything downstream. It sits beside the existing command-language modeler
(the Geometry source of section 12) and pulls in SolidCraft for B-rep geometry and the CAD
file formats. Written for follow-on agents: every stage names its files, its tests and its
guide entry. Branch: `feat/CAD-modeling-environment`.

Vocabulary used below: *document* = a CAD part (feature timeline, sketches, parameters);
*kernel* = what turns a document into geometry; *environment* = the app mode in which the
document is edited; *commit* = handing the edited geometry back to the pipeline.

## 1. What the user gets

- **Sources › CAD Model…** starts an empty document and enters the environment. **File › Open**
  of a `.step`, `.stp`, `.igs`, `.iges`, `.3mf`, `.sc` (SolidCraft design), `.afxcad` (our
  document) or a mesh (`.stl`, `.obj`, `.ply`, `.glb`) that the user wants to edit opens it as
  a **CAD Model** node in the pipeline (not in the environment: viewing a STEP file must not
  switch modes).
- **Edit in CAD** (right-click a CAD Model node, a Geometry node, or any surface node; the
  Properties panel of those nodes has the same button; Python `node.edit_cad()`) enters the
  environment on that node. A Geometry node (command language) is converted to a document whose
  first feature is the command journal (section 5.4); any other surface becomes a mesh body.
- **In the environment** the window changes: the menu bar gains a **CAD** menu, the toolbar is
  replaced by the CAD ribbon (tabs Sketch, Features, Modify, Assemble, Inspect, Files), the
  Pipeline panel is replaced by the **FeatureManager** (timeline tree: origin planes, sketches,
  features, bodies, parameters, configurations), Properties becomes the **PropertyManager** of
  the selected feature or sketch entity, the active 3D view shows the document (with the sketch
  canvas when a sketch is open), and the bottom hosts the **timeline bar** (rollback marker,
  suppress, reorder). Every other 3D view, chart or spreadsheet that was open stays as it was
  but is marked **stale** (a grey banner "pipeline paused while editing CAD"); their pipelines
  do not run until the environment closes.
- **Leaving.** The ribbon's right end has ✔ **Apply** (commit and leave), ✔⟳ **Apply, keep
  editing** (commit, stay), ✖ **Discard** (leave, document as it was), and **Save as…**
  (`.afxcad`, `.step`, `.sc`, meshes). Esc asks the same question. Apply runs the downstream
  check of section 4 before the environment closes; on conflicts the dialog of 4.3 offers the
  fixes, "Back to CAD" and "Discard".
- **Afterwards** the CAD Model node carries the new geometry; downstream nodes are dirty and
  re-run when the user confirms ("Re-run 5 downstream filters?" with Run now / Later). The
  node's Properties show the document's parameters as ordinary parameters, so a dimension can
  be changed and the whole pipeline re-run **without** entering the environment.

The environment is one mode of the one window, not a second window: the same session, the same
undo stack (environment edits are one undo group per commit), the same Python and assistant.

## 2. Why two modelers, and how they share

The Geometry source (section 12) is a command-language modeler on implicit geometry: fast,
scriptable, good for simulation specimens, blocks, bodies with named faces, and meshing. A
parametric modeler adds what simulation users bring from CAD: sketches with constraints and
dimensions, a feature timeline that rebuilds when a parameter changes, fillets and shells that
follow edges, holes and threads, patterns, assemblies, STEP/IGES in and out.

The two share the data path: both produce *bodies with named faces* (BodyId, FaceId, face
names, side sets) and a *signed distance* when asked, so every consumer that exists today
(overlay and tet meshers, Assign Grains, the `mesh body` commands, side sets, Mesh Quality,
the solver cases) works unchanged on a CAD Model. A Geometry journal can be a feature of a
CAD document, and a CAD body can be imported into a Geometry journal (`import 'part.step'`,
which today reads STL only).

## 3. Architecture

### 3.1 Crates

| Crate | Role | Depends on |
|---|---|---|
| `aurafx-cad` (new) | the CAD **document** (feature timeline, sketches and constraints, parameters, expressions, configurations, templates), the **kernel trait** and its two kernels, the conflict checker's geometry diff, document files | `aurafx-data`, `aurafx-mesh`, `glam`, `serde`; optionally the SolidCraft crates |
| `aurafx-filters` | the **CAD Model** source (`cad_model`): a FilterDef whose parameters are the document's parameters, plus `cad_import` (STEP/IGES/3MF/`.sc` as a static read when no editing is wanted) | `aurafx-cad` |
| `aurafx-io` | readers for `.step .stp .igs .iges .3mf .sc .afxcad` (through `aurafx-cad`), writers for `.step .igs .3mf .afxcad .sc` | `aurafx-cad` |
| `aurafx-pipeline` | `NodeKind::Cad { document }` next to Reader/Source/Filter; `Session::enter_cad / commit_cad / discard_cad`; the **downstream check** (`cad_check.rs`): reference collection, diff, default fixers, AI prompt | `aurafx-cad` |
| `aurafx-app/src/cad/` | the environment: mode switch, ribbon, FeatureManager, PropertyManager, timeline bar, sketch canvas, pick in the 3D view, the Apply/Discard flow and the conflict dialog | |
| `aurafx-python` | `afx.cad_model()`, `node.edit_cad()`, `node.cad` (document API: parameters, features, sketches, `apply()`), `afx.cad_check(node)` | |
| `aurafx-ai` | help text for the above; the conflict-fix prompt template lives in `aurafx-pipeline` and is called through the assistant | |

Workspace members are `crates/*`, so the new crate joins automatically. The optional SolidCraft
dependency is a cargo feature of `aurafx-cad` (`brep`), re-exported by `aurafx-app` as
`cad-brep` and **on by default in the release packages**; the implicit kernel is always built,
so a build without the feature still has the whole environment on implicit geometry.

### 3.2 Kernels

```rust
pub trait Kernel {
    fn name(&self) -> &'static str;                      // "implicit" | "brep"
    fn evaluate(&self, doc: &Document, cfg: &Resolved) -> Result<Evaluated, CadError>;
}
pub struct Evaluated {
    pub bodies: Vec<EvaluatedBody>,   // one per solid: name, stable id, faces (id, name, kind, bounds, area, normal/axis), volume, mass properties
    pub surface: DataSet,             // PolyData multiblock, one block per body; cell arrays BodyId, FaceId; field FaceNames, BodyNames, Volume, SurfaceArea
    pub distance: Option<Box<dyn Fn([f64; 3]) -> f64 + Send + Sync>>, // signed distance when the kernel has it
    pub brep: Option<BrepHandle>,     // the B-rep for export and for the next evaluation (B-rep kernel only)
}
```

- **Implicit kernel** (`aurafx-cad/src/kernel/implicit.rs`): the port of `ezyx-cad/src/sdf.rs`
  (847 lines) onto `aurafx-mesh`'s implicit geometry: every `Shape` and `Modifier` of the
  document becomes a signed-distance function (sketch extrude/revolve from the sketch's
  profiles, sweep, loft, holes, threads, fillets as smooth minimum, chamfers, shells, patterns,
  mirror, split, scale). Surfaces come from the existing iso-surface extraction with face ids
  from the feature that owns the nearest distance term (as the Geometry source does today).
  Never fails topologically; no exact edges; STEP export of this kernel's result is a
  tessellated STEP (faceted B-rep), clearly labelled.
- **B-rep kernel** (`aurafx-cad/src/kernel/brep.rs`, feature `brep`): the document's features
  mapped one-to-one onto SolidCraft's commands (`sketch.create`, `solid.extrude`,
  `solid.revolve`, `solid.sweep`, `solid.loft`, `solid.fillet`, `solid.chamfer`,
  `solid.shell`, `solid.hole`, `solid.thread`, `pattern.*`, `solid.mirror`, `solid.split`,
  `solid.combine`, `param.*`), run through `solidcraft_engine::Session::execute` in process.
  Exact edges and faces, STEP AP242 / IGES / 3MF through `solidcraft-io`, STEP/IGES import as
  a base feature, assemblies. The signed distance of a B-rep body is computed from its
  tessellation (the Geometry source's `import` path does this today for STL).
- **Bridge** (`aurafx-cad/src/kernel/bridge.rs`, always built): when the `brep` feature is off
  or SolidCraft fails to initialise, the same command list is written as a SolidCraft JSON
  script and run with `solidcraft-cli run script.json --out part.step` found on the PATH, the
  result read back as a mesh with face ids. This is the ez-yx-craft bridge made round-trip.
  It also lets the user open the document in the SolidCraft app ("Open in SolidCraft") and
  re-import the saved `.sc`.

The document never depends on a kernel: it is serde data (section 5). The kernel is a per-node
choice (Properties › Kernel: B-rep when available, else Implicit) and the environment shows
which one is live.

### 3.3 Pulling SolidCraft in

Facts checked on 2026-11-07: SolidCraft 0.40 is MIT OR Apache-2.0, Rust edition 2024,
`rust-version = 1.95`, `egui 0.37`, `egui-wgpu 0.37`, `wgpu 30` (the same as auraFX, which
makes an in-process pull-in possible), with its B-rep kernel in a separate pinned git
dependency (`solidcore`, an Apache-2.0 port of SMLib). It vendors patched copies of `egui-wgpu`
and `winit` for its own desktop app, so **its UI crate (`solidcraft-ui-egui`) is not pulled in**;
auraFX writes its own egui panels against the engine. The crates to depend on (git, pinned to a
tag, moved on their own commit, like SolidCraft pins solidcore):

| SolidCraft crate | Used for |
|---|---|
| `solidcraft-engine` | `Session`, the command registry (`command_specs()` for the ribbon and the assistant), document evaluation |
| `solidcraft-doc` | parameters, expressions, the timeline (the B-rep kernel keeps a SolidCraft `Document` as the mirror of ours) |
| `solidcraft-kernel`, `solidcore`, `solidcore-record` | the B-rep bodies, measures, tessellation plans |
| `solidcraft-io` | STEP/IGES/3MF/glTF/OBJ/STL import and export, `.sc` files |
| `solidcraft-geom`, `solidcraft-sketch` | profiles, the constraint solver (used by the B-rep kernel; our own sketch solver, ported from ez-yx-craft, stays the one the environment edits so the document is kernel-free) |

Consequences to plan for: auraFX's `rust-version` goes from 1.85 to 1.95 when the feature is
on (CI uses stable: fine); first build time grows by the git dependencies (cache them in CI with
`Swatinem/rust-cache`, already used); the browser build is out of scope; the pinned revision is
recorded in `docs/reference/third-party.md` with its licence, and the README's credits name
SolidCraft and solidcore as libraries (the naming rule allows crediting a library; do not
present the environment as a copy of any CAD product).

### 3.4 What is ported from ez-yx-craft

`/mnt/extended/projects/ez-yx-craft` (the user's own project, MIT OR Apache-2.0) has a complete
headless CAD core and a desktop UI for it. Port, do not rewrite:

| From | To | Size | Notes |
|---|---|---|---|
| `crates/ezyx-cad/src/expr.rs` | `aurafx-cad/src/expr.rs` | 360 lines | self-contained; ported in this branch's first commit, tests included |
| `crates/ezyx-cad/src/lib.rs` (Shape, Feature, Modifier, Parameter, Binding, Configuration, Part, resolution) | `aurafx-cad/src/document.rs` | ~900 lines | `Part` becomes `Document`; `Polyline`/`Heightmap` from `ezyx-core` become `aurafx-mesh` types (the geometry kernel's polyline outline already exists: `aurafx-mesh/src/outline.rs`); drop `Relief` (carving) |
| `crates/ezyx-cad/src/sketch.rs` | `aurafx-cad/src/sketch.rs` | 1113 lines | the constraint solver (Levenberg-Marquardt, DOF report); needs `arc_segments`, `shapes`, `DEFAULT_TOLERANCE` from `ezyx-core/src/path.rs` (341 lines): port that file as `aurafx-cad/src/path.rs` |
| `crates/ezyx-cad/src/params.rs` | `aurafx-cad/src/document.rs` (resolution) | 186 lines | |
| `crates/ezyx-cad/src/sdf.rs` | `aurafx-cad/src/kernel/implicit.rs` | 847 lines | re-targeted onto `aurafx-mesh::implicit` (same mathematics; the `Sdf` enum maps onto the existing `Shape`/CSG tree) |
| `crates/ezyx-cad/src/analysis.rs`, `assembly.rs` | `aurafx-cad/src/analysis.rs`, `assembly.rs` | 300 lines | mass properties, interference, sections, components |
| `crates/ezyx-cad/src/templates.rs` | `aurafx-cad/src/templates.rs` | 258 lines | starter parts |
| `crates/ezyx-app/src/solid/{mod,props,sketch_ui}.rs` | `aurafx-app/src/cad/{mod,property_manager,sketch_canvas}.rs` | 2336 lines | the ribbon, FeatureManager, PropertyManager and sketch canvas; ez-yx-craft's `ribbon`, `viewport::Projector` and `theme` helpers become auraFX's `widgets`, the view's camera and `theme` |

Keep the file headers' origin note ("ported from EZ-YX-craft, MIT OR Apache-2.0"). ez-yx-craft's
"send to pipeline / update with merge report" is the ancestor of section 4.

## 4. Commit and the downstream check

### 4.1 What a commit changes

A CAD Model node's output is the `Evaluated.surface` (plus the distance and B-rep handles kept
on the node). Downstream nodes reference it by **node id** (unchanged) and, inside their
parameters, by **names and ids of the geometry**: face names (`brick 1 z+`, `inlet`), face ids,
body ids, set names made from them (side sets `NodeSet x-`, `SideSet cylinder 2 side`),
parameter names (bindings), and numbers that only make sense for the old shape (box corners of
`refine_region`, seeds of `assign_grains`, `clip` planes, probe locations).

### 4.2 Detecting conflicts

`aurafx-pipeline/src/cad_check.rs`:

1. **Collect references.** Walk the downstream nodes of the CAD Model. For each node, from its
   FilterDef's `ParamSpec`s and current values, collect: `Array` params naming BodyId/FaceId,
   `Str`/`Choice` params whose value matches a face or body name or a set name of the old
   output (`extract_set`, `boundary_layers`, `cases` fixed/loaded sets, geometry `mesh body`,
   `sideset … add face`, `block … add body` commands in a Geometry node's text), `Vec3` params
   inside the old bounds (positions that may fall outside the new shape), and the Python
   filters' code (a text search for the same names, reported as "may reference").
2. **Diff old and new.** `aurafx-cad::diff(old: &Evaluated, new: &Evaluated) -> GeometryDiff`:
   bodies added/removed/renamed (by stable feature ids, then by name, then by centroid and
   volume within 5 %), faces the same way (by feature id and index, then name, then
   centroid/normal/area), bounds change, volume change, parameters added/removed/renamed.
3. **Classify.** Each reference against the diff gives a `Conflict { node, param, kind }` with
   kinds: `Removed` (the face/body/parameter is gone), `Renamed { to }` (the diff found the
   successor), `Moved { outside: bool }` (a position now outside the body), `Ambiguous
   { candidates }` (several plausible successors), `Count { was, now }` (a filter that assumed
   N bodies). No conflicts: the commit proceeds to "Re-run downstream?".
4. **Default fixers**, one per kind: `Renamed` → rewrite the parameter; `Removed` with a unique
   nearest successor (same kind of face, centroid within the face's size) → rewrite and flag
   "check"; `Moved` → clamp to the new bounds / nearest point on the new surface; `Ambiguous` →
   list the candidates for the user; `Count` → re-run with the new count and warn. Every fix is
   a plain `set_param_json` on a node, so it is undoable and scriptable.
5. **AI suggestion.** When an assistant is connected, the dialog offers "Ask the assistant": the
   prompt holds the diff (as text), the conflicts, each affected node's filter id and
   parameters, and asks for a JSON list of `{node, param, value, reason}`; the answer is shown
   next to the default fixes and applied the same way. The prompt lives in
   `cad_check::ai_prompt()`, the parsing in `cad_check::parse_ai_fixes()` (tests with canned
   answers; no network in tests).

### 4.3 The dialog

"Applying the CAD changes affects N downstream objects" with a table: object, parameter, what
changed, **default fix**, **assistant's fix** (column present only when connected), and a
choice per row (default / assistant / leave as is). Buttons: **Apply fixes and re-run**,
**Apply without fixing** (conflicting nodes will show their own errors), **Back to CAD**,
**Discard CAD changes**. Python: `afx.cad_check(node)` returns the conflicts and fixes;
`node.cad.apply(fix="default" | "ai" | "none")`.

### 4.4 Stale views

While the environment is open, `Session.cad_editing = Some(node)` and the scheduler does not
run any node (the environment evaluates its document itself, on its own thread, with a
progress bar in the timeline bar). Other views draw their last output with a banner. Charts
and spreadsheets keep their content. On commit the CAD node is marked dirty and the downstream
nodes run on confirmation; on discard nothing changes.

## 5. Document and files

### 5.1 Data model (from ez-yx-craft, renamed)

```
Document { name, features: Vec<Feature>, finish: Vec<Modifier>, parameters: Vec<Parameter>,
           configurations: Vec<Configuration>, active_configuration, sketches: BTreeMap<Id, Sketch>,
           components: Vec<Component> (assemblies), units: Units, kernel: KernelChoice, journal: Vec<Command> }
Feature  { id, name, op: Add|Cut|Intersect, shape: Shape, placement, blend, blend_kind, modifiers, suppressed, bindings }
Shape    = Box | Sphere | Cylinder | Cone | Torus | Extrude{profile} | Revolve{profile}
         | SketchExtrude{sketch, end: Blind|MidPlane|ThroughAll, draft, cap: EdgeTreatment}
         | SketchRevolve{sketch, angle} | Sweep{profile, path} | Loft{sections}
         | Hole{kind: Simple|Drilled|Counterbore|Countersink|Tapped, ...} | Thread{...}
         | Body{document} | Mesh{surface} (imported STL/OBJ/glTF) | Brep{bytes} (imported STEP/IGES base feature)
         | Journal{text} (a Geometry command journal, section 12)
Modifier = Fillet | Chamfer | Shell | ShellOpen | LinearPattern | GridPattern | CircularPattern | Mirror | Split | Scale | Draft
Sketch   { plane: XY|XZ|YZ|Offset|Face{body, face}, entities (point, line, circle, arc, spline, polyline, text),
           constraints (coincident, horizontal, vertical, parallel, perpendicular, tangent, equal, concentric, midpoint,
           symmetric, fix), dims (distance, radius, diameter, angle; expressions), solve report (status, DOF) }
Parameter { name, expr, comment }   Binding { path, expr }   Configuration { name, overrides }
```

Every feature and sketch entity has a stable `Id` (u64, never reused) so the diff of 4.2 and
the B-rep mirror can follow them across edits.

### 5.2 Files

- `.afxcad`: the document as JSON (serde), with a `format` version; pretty-printed; the file the
  `.afx` session state embeds for CAD Model nodes.
- `.sc`: SolidCraft designs, read and written through `solidcraft-io` (B-rep feature) or the
  bridge; features SolidCraft has and we do not (sheet metal, surface workspace) stay as a
  `Brep` base feature.
- `.step/.stp/.igs/.iges/.3mf`: through `solidcraft-io`; without the `brep` feature, STEP
  export writes a faceted B-rep from the surface (`aurafx-cad/src/io/step_faceted.rs`, our own
  Part 21 writer: `MANIFOLD_SOLID_BREP` of `ADVANCED_FACE`s on `PLANE`s, one per triangle, or
  per planar face when the face ids are planar) and import is refused with a message naming
  the feature.
- Meshes: `.stl .obj .ply .glb` as mesh bodies (readers exist; glTF is new).

### 5.3 Python

```python
m = afx.cad_model()                              # new document; afx.cad_model("part.step") imports
m.cad.parameter("width", "40 mm"); m.cad.sketch("base", plane="XY").rectangle(0, 0, "width", 20)
m.cad.extrude("base", 10, name="plate"); m.cad.hole(center=(20, 10), diameter=5, through=True)
m.cad.fillet(edges="plate top", radius=2)
m.cad.apply()                                    # evaluate; the node's output updates
m.set(width=50)                                  # the document's parameters are node parameters
afx.cad_check(m)                                 # conflicts downstream, with fixes
m.save("part.step"); m.save("part.afxcad")
node.edit_cad()                                  # enter the environment in the app
```

### 5.4 The Geometry source and the CAD Model

- `Edit in CAD` on a Geometry node makes a document with one `Journal` feature holding the
  commands; the environment shows it as a feature whose PropertyManager is the command text
  (with the existing command help), and the user can add parametric features after it.
- A CAD Model can be used in a journal: `import 'part.afxcad'` and `import 'part.step'` in the
  command language (today `import` reads STL) evaluate the document and take its surface.
- Both keep face names; the `mesh body … scheme overlay` commands mesh CAD bodies through the
  surface and distance path already used for imported STL.

## 6. Stages

Each stage ends with: tests passing, the user guide chapter **CAD modeling** (`docs/userguide/src/cad.md`,
new; add to SUMMARY.md under Data after Geometry and meshing) updated with a screenshot script,
the AI help updated, the roadmap checkbox ticked and a Status paragraph. Sizes are rough line
counts to help a small agent budget the work.

| Stage | Deliverable | Files | Tests | Size |
|---|---|---|---|---|
| **C0** (this branch's first commit) | crate skeleton, `expr.rs` ported, this design, roadmap section 13 | `crates/aurafx-cad/{Cargo.toml,src/lib.rs,src/expr.rs,src/kernel/mod.rs}` | `expr` unit tests (ported) | done |
| **C1 Document** | `document.rs`, `sketch.rs`, `path.rs`, `params`, `templates.rs`, `analysis.rs`, `assembly.rs` ported; `.afxcad` read/write; `Document::resolved()` | `aurafx-cad/src/*` | ez-yx-craft's unit tests ported; a round trip of every template through JSON | 3 000 |
| **C2 Implicit kernel** | `kernel/implicit.rs` on `aurafx-mesh`; `Evaluated` with face ids and names; `cad_model` FilterDef (parameters = document parameters); `NodeKind::Cad`; `.afx` state keeps the document | `aurafx-cad/src/kernel/implicit.rs`, `aurafx-filters/src/cad.rs`, `aurafx-pipeline/src/session.rs`, `state.rs` | every template evaluates; volumes of box/cylinder/sphere/extrude/revolve within 1 % of analytic values (ez-yx-craft's `analysis` tests give the numbers); Mesh Quality acceptable on an overlay mesh of a template | 1 500 |
| **C3 Environment shell** | mode switch (`Session.cad_editing`, stale banners, scheduler pause), CAD menu, ribbon with tabs, FeatureManager, PropertyManager for every Feature/Modifier/Parameter, timeline bar, Apply/Apply-keep/Discard, `node.edit_cad()`; undo groups | `aurafx-app/src/cad/{mod,ribbon,feature_manager,property_manager,timeline}.rs`, `app.rs` (menu, layout) | an app-driven script test (like `scripts/userguide/shots`) that enters, adds a feature, applies; `cargo test` of the pipeline part | 2 500 (ported) |
| **C4 Sketching** | sketch canvas in the 3D view (plane-aligned camera, grid, snapping), entities, constraints and dimensions with the solver's DOF status, sketch on a face | `aurafx-app/src/cad/sketch_canvas.rs` (ported `sketch_ui.rs`), `aurafx-cad/src/sketch.rs` | solver tests ported (fully constrained rectangle, over-constrained rejection, tangent arcs); canvas script test | 1 200 |
| **C5 Downstream check** | `cad_check.rs`: reference collection, `aurafx-cad::diff`, kinds, default fixers, the dialog, re-run confirmation, Python `afx.cad_check`, assistant prompt and parser | `aurafx-pipeline/src/cad_check.rs`, `aurafx-cad/src/diff.rs`, `aurafx-app/src/cad/conflicts.rs` | a plate with a hole → `extract_set("hole")` → rename the hole feature: `Renamed` fixed by default; delete it: `Removed` offered; move a `refine_region` box: `Moved`; canned assistant answer parsed and applied | 1 200 |
| **C6 B-rep kernel** | cargo feature `brep`; SolidCraft crates pinned; `kernel/brep.rs` mapping every Shape/Modifier to commands; exact faces and edges with ids; STEP AP242, IGES, 3MF, `.sc` read and write; STEP/IGES import as `Brep` base feature; the bridge through `solidcraft-cli` when the feature is off | `aurafx-cad/src/kernel/{brep,bridge}.rs`, `aurafx-cad/src/io/*`, `aurafx-io` registrations, `Cargo.toml`s, CI cache | the same templates evaluate on both kernels with volumes within 1 %; a STEP written here reopens in SolidCraft's CLI (`solidcraft-cli measure`) with the same volume; `testdata/cad/` gets small STEP/IGES files written by SolidCraft (documented in `testdata/README.md`) | 2 000 |
| **C7 Edge operations and assemblies** | fillet/chamfer on picked edges and faces (B-rep), draft, shell with picked open faces, patterns on faces, mirror about faces, combine; components with placements, interference, mass properties panel; measure tool | `aurafx-app/src/cad/pick.rs` (edges/faces from the B-rep tessellation), `aurafx-cad/src/assembly.rs` | B-rep fillet volume against SolidCraft's measure; interference volume of two overlapping boxes analytic | 1 500 |
| **C8 Meshing from CAD** | `mesh body` commands accept CAD bodies; face names to side sets; `import 'x.step'` in journals; CAD Model → tet mesh / overlay mesh / Assign Grains / solver case end to end; the guide's worked example extended (STEP → mesh → grains → case) | `aurafx-mesh/src/model.rs`, `bodymesh.rs`, `aurafx-filters/src/mesh_gen.rs` | the worked example as a test (as `worked_example` in `grains_on_mesh.rs`) from a STEP file in testdata | 600 |
| **C9 Polish** | templates gallery, keyboard shortcuts, PropertyManager help texts, the assistant modelling in the environment through the same commands (`afx.cad` from agent mode), the project page's CAD video | docs, `scripts/site/media/cad.py` | guide complete; screenshot per ribbon tab | 500 |

Order: C1 → C2 → C3 → C5 → C4 → C6 → C7 → C8 → C9. C5 comes before C4 on purpose: the
commit/check loop is the part that is new to auraFX and must be solid before the environment
grows; sketches can be built from Python until C4.

## 7. Rules for the agents

- Nothing in the environment owns geometry data: every edit is a change to the `Document`
  followed by an evaluation; panels read the document and the last `Evaluated`.
- One kernel-free document. A feature that only the B-rep kernel can evaluate (exact fillet on
  curved edges) must still *exist* in the implicit kernel, approximated and flagged in the
  timeline ("approximate on this kernel"), never silently dropped.
- Stable ids on everything the diff follows. Never renumber.
- The naming rule of `CLAUDE.md` holds: describe the environment in its own words; credit
  SolidCraft and solidcore as libraries in the README and `third-party.md`; ez-yx-craft may be
  named as the origin of ported files.
- Tests use `testdata/cad/` files that `testdata/README.md` explains (how each STEP/IGES/`.sc`
  was written, by which program and version); keep them under 300 KB.
- The user guide chapter is written as the stages land, with screenshots from scripts under
  `scripts/userguide/shots/cad_*.py` (the environment can be driven by `node.edit_cad()` and
  `afx.cad.*`).

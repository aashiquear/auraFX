# CAD modeling environment: design

The design of ROADMAP.md section 13, as built: a parametric CAD modeler inside auraFX that
creates, imports, edits and exports geometry, and hands the result back to the regular
pipeline with a check of everything downstream. It sits beside the command-language modeler
(the Geometry source of section 12). The user-facing description is the guide chapter *CAD
modeling* (`docs/userguide/src/cad.md`).

Vocabulary: *document* = a CAD part (feature timeline, sketches, parameters); *kernel* = what
turns a document into geometry; *environment* = the app mode in which the document is edited;
*apply* = handing the edited geometry back to the pipeline.

## 1. What the user gets

- **Sources › CAD Model…** starts a document. **File › Open** of a `.step`, `.stp`, `.igs`,
  `.iges`, `.3mf`, `.sc` or `.afxcad` file opens it as a **CAD Model** node in the pipeline
  (not in the environment: viewing a STEP file does not switch modes).
- **Edit in CAD** (right-click a node in the Pipeline, the button in Properties, or Python
  `node.edit_cad()`) enters the environment on a CAD Model. On a Geometry source or any closed
  surface it makes a new CAD Model whose first features are the triangles as imported bodies
  (`afx.cad_from`).
- **In the environment** the window changes: the menu bar gains a **CAD** menu, the toolbar is
  replaced by the ribbon (tabs Sketch, Features, Modify, Assemble, Inspect, Files), the
  Pipeline panel by the **model tree** (dimensions, features, components, mates,
  configurations, with a *Next* strip of suggested steps), Properties by the **feature panel**
  of the selected feature, and the bottom holds the **timeline bar** (rollback marker,
  suppress, reorder). The active 3D view shows the part, or the sketch canvas while a sketch is
  open. Every other view, chart or spreadsheet keeps what it showed and is marked stale; the
  pipeline does not run until the environment closes.
- **Leaving.** **Apply** (hand over and leave), **Apply, keep editing**, **Discard**, and
  **Save as…**. Esc asks the same question. Apply runs the downstream check of section 4.
- **Afterwards** the CAD Model node carries the new geometry and the nodes below re-run (now,
  or later on request). The node's Properties show the document's dimensions as ordinary
  parameters, so a dimension can be changed and the whole pipeline re-run without entering
  the environment.

The environment is one mode of the one window: the same session, the same Python and
assistant, and one undo step in the pipeline's history per apply (the environment has its own
undo and redo while it is open).

## 2. Two modelers that share a data path

The Geometry source is a command-language modeler on implicit geometry: fast, scriptable, good
for simulation specimens, blocks, bodies with named faces, and meshing. The parametric modeler
adds sketches with constraints and dimensions, a feature timeline that rebuilds when a
parameter changes, fillets and shells that follow edges, holes and threads, patterns,
assemblies, and STEP/IGES in and out.

Both produce *bodies with named faces* (`BodyId`, `FaceId`, face names) and a *signed distance*
when asked, so every consumer (the overlay and tetrahedral meshers, Assign Grains, the
`mesh body` commands, side sets, Mesh Quality, the solver cases) works unchanged on a CAD
Model. A CAD part can be brought into a Geometry command list with `import 'part.step'` or
`import 'part.afxcad'`, keeping its face names.

## 3. Architecture

### 3.1 Crates

| Crate | Role |
|---|---|
| `aurafx-cad` | the **document** (`document.rs`, `params.rs`, `sketch.rs`, `path.rs`, `templates.rs`), the edit operations shared by the window, Python and the assistant (`edit.rs`), the **kernels** (`kernel/implicit.rs` with `sdf.rs`, `kernel/brep.rs`, `kernel/assemble.rs`), kept sketches turned into sweeps and lofts (`swept.rs`), edges of the part projected into a sketch (`project.rs`), mates (`mates.rs`), the geometry diff (`diff.rs`), analysis (`analysis.rs`), and files (`file.rs`, `io.rs`) |
| `aurafx-filters` | the **CAD Model** source (`cad_model`): a `FilterDef` whose parameters include the document and the overrides of its dimensions |
| `aurafx-io` | the reader table, in which the CAD readers are registered at start (`aurafx_io::register_reader`; `aurafx-cad` depends on `aurafx-mesh`, which depends on `aurafx-io`, so they are not compiled into it) |
| `aurafx-pipeline` | the editing session (`cad_edit.rs`: enter, edit, evaluate, undo, rollback, focus on a component, apply, discard) and the **downstream check** (`cad_check.rs`) |
| `aurafx-app/src/cad/` | the environment: mode switch and ribbon (`mod.rs`, `ribbon.rs`), model tree (`feature_manager.rs`), feature panel (`property_manager.rs`), timeline (`timeline.rs`), sketch canvas (`sketch.rs`, `sketch_dims.rs`, `sketch_snap.rs`, `sketch_under.rs`), reference planes and their previews (`planes.rs`, `preview.rs`), picking (`pick.rs`), the Next strip and command search (`guide.rs`), the conflict dialog (`conflicts.rs`) |
| `aurafx-python` | `afx.cad_model()`, `afx.cad_from()`, `node.edit_cad()`, `node.cad` (the document API), `afx.cad_check(node)` |
| `aurafx-ai` | the help text for the above; the conflict-fix prompt lives in `aurafx-pipeline` |

The document is a parameter of the CAD Model source (its `.afxcad` JSON text), so state files,
undo and the Python trace carry it without a node kind of its own.

### 3.2 Kernels

```rust
pub trait Kernel: Send + Sync {
    fn name(&self) -> &'static str;                 // "implicit" or "brep"
    fn evaluate(&self, document: &Document) -> Result<Evaluated, CadError>;
}
```

`Evaluated` holds the bodies (name, stable id, faces with id, name, kind, area, centroid and
direction, volume, area, bounds), the surface as a dataset with `BodyId` and `FaceId` cell
arrays and the names in field data, a signed-distance function when the kernel has one, the
name of the kernel that built it, and notes on anything built approximately.

- **Implicit kernel.** Every shape and modifier of the document is a signed-distance function.
  The surface is extracted over a grid with the points moved onto the exact surface; every
  triangle belongs to the feature whose own field is nearest and to the face of that feature's
  shape its normal points along, so faces keep stable `(feature id, index)` identities and
  names like `Base z+` or `Bore side` across rebuilds. It does not fail on a combination of
  features; it has no exact edges.
- **B-rep kernel.** The document's features are mapped onto the commands of the SolidCraft
  engine carried in `vendor/`, run in process on a headless session. Exact faces and edges,
  exact volumes. Every exact face is labelled with the names the implicit kernel gives, so
  names, ids, the diff and the downstream check agree across kernels. What it cannot map is
  refused with the feature and the reason.
- **Choice.** The node's `kernel` parameter is `Auto` by default: the B-rep kernel, and the
  implicit kernel with a note when the B-rep kernel refuses. The environment shows which one
  built the part.
- **Assemblies.** `kernel/assemble.rs` builds the document's own features and every visible
  component with the chosen kernel, places them (after the mates are solved), and merges them
  into one `Evaluated`: a body per part, faces named `Part/Face`, the union of the distance
  fields.

The document never depends on a kernel: it is plain serde data. A feature that only one
kernel builds exactly still exists on the other, approximated and noted, never silently
dropped.

### 3.3 The vendored B-rep code

The B-rep kernel and the CAD file formats come from the open-source SolidCraft and solidcore
projects (MIT OR Apache-2.0, and Apache-2.0), as code carried in this repository, so that a
release builds from the repository alone and auraFX is free to diverge.

- `vendor/solidcraft/` and `vendor/solidcore/` are plain files copied at a recorded revision
  (`vendor/UPSTREAM.toml`) and pruned to the directories the build uses, with their licence,
  notice and attribution files kept. `vendor/README.md` records what is carried and every
  auraFX change inside it (marked `// auraFX:` in the code).
- The workspace excludes `vendor/` from its members and depends on the crates by path; the
  B-rep kernel is always built (no cargo feature, no external program).
- `scripts/cad/upstream-pull.sh <tag>` takes upstream work as a three-way patch of the carried
  directories (auraFX's edits stay; a line both sides changed is a conflict);
  `scripts/cad/upstream-diff.sh` lists upstream commits and command ids since the recorded
  revision. After either, `scripts/licences.sh` regenerates `THIRD-PARTY-NOTICES.txt` and
  `docs/reference/third-party.md`.
- The kernel maps the document onto the engine's **commands** (stable dotted ids), not onto
  its internal types, which keeps taking upstream work cheap.

The document model, the sketch solver and the expression evaluator were ported from the
author's EZ-YX-craft project (MIT OR Apache-2.0); the ported files say so in their headers.

## 4. Apply and the downstream check

### 4.1 What an apply changes

Downstream nodes refer to a CAD Model by node id (unchanged) and, inside their parameters, by
**names and numbers of the geometry**: face names (`Base z+`, `Bore side`), face and body
numbers, set names made from them, dimension names, and positions that only make sense for the
old shape (box corners of Refine Region, clip planes, probe locations).

### 4.2 Detecting conflicts (`aurafx-pipeline/src/cad_check.rs`)

1. **Collect references.** For each node below the part, from its parameters: face and body
   names as whole values or as whole words inside command and expression text, face and body
   numbers, positions inside the old bounds, the part's own overrides of dimensions, and the
   code of Python filters (reported as "may reference").
2. **Diff old and new** (`aurafx-cad/src/diff.rs`): each old face is the same, renamed,
   replaced by one successor, ambiguous between several, or removed; matched by stable id,
   then by name, then by place (kind, direction, centroid, area). Bodies and dimensions are
   tracked the same way.
3. **Classify.** Each reference against the diff gives a conflict of kind *Removed*,
   *Renamed*, *Moved*, *Ambiguous* or *Count*.
4. **Default fixes**, one per kind, each a plain parameter change on a node (so undoable and
   scriptable): a rename is rewritten; a face with one successor is rewritten and marked
   *check*; a position is moved onto the new bounds; ambiguous ones list the candidates.
5. **The assistant's suggestion.** When an assistant is connected, the dialog offers *Ask the
   assistant*: `cad_check::ai_prompt()` writes the diff, the conflicts and each affected
   node's filter and parameters; `cad_check::parse_ai_fixes()` reads the JSON answer (node by
   id or name; a bad answer is refused with the reason).

### 4.3 The dialog

"Applying the CAD changes affects N downstream objects" with a table: object, parameter, what
changed, default fix, assistant's fix, and a choice per row. Buttons: **Apply fixes and
re-run**, **Apply without fixing**, **Back to CAD**, **Discard**. With no conflicts the
question is only whether to re-run the nodes below now or later (a banner then offers *Run
now*). The part and the chosen fixes go into the history as one undo step.

Python: `cad.check()`, `afx.cad_check(node)`,
`cad.apply(fix='default'|'none'|list|answer text, run='now'|'later')`, `cad.run_held()`.

### 4.4 Stale views

While the environment is open the scheduler runs no node; the environment evaluates its
working copy on a worker thread and shows the surface as the node's output, so every view
works unchanged. Other views draw their last output under a banner. A part that does not build
is not applied (the reason is shown); Discard restores the node's output and document.

## 5. Document and files

### 5.1 Data model

```
Document  { name, features, finish (modifiers of the whole part), parameters, configurations,
            active_configuration, next_id, components, mates, planes (reference planes),
            sketches (kept sketches) }
Feature   { id, name, operation (add, cut, intersect), shape, placement, modifiers,
            suppressed, bindings }
Shape     = Box | Sphere | Cylinder | Cone | Torus | Extrude | Revolve (outlines)
          | SketchExtrude | SketchRevolve | Sweep | Loft | SketchSweep | SketchLoft (kept sketches)
          | Hole (simple, drilled, counterbore, countersink, tapped) | Thread
          | Brep (an imported solid) | Body (another part combined)
          | EdgeBlend (picked edges) | ShellFaces (picked open faces) | MirrorPlane | Draft
Modifier  = Fillet | Shell | ShellOpen | LinearPattern | GridPattern | CircularPattern
          | CircularPatternAngle | Offset | Mirror | Split | Scale
Sketch    { plane (principal, offset, a face by name, a reference plane), entities with
            stable numbers (point, line, circle, arc, spline), geometric constraints,
            dimensions as expressions, the solver's report (status, degrees of freedom) }
Parameter { name, expression, comment }   Binding { path, expression }
Configuration { name, overrides }         Component { id, name, document, placement, visible }
```

Every feature and sketch entity has a stable id that is never reused, so the diff and the
B-rep labels follow them across edits. Side faces of an extrusion are named and numbered by
the sketch entity they come from, so a face keeps its name when the outline gains or loses a
line. A sketch on a face remembers the face's name and its plane is put back on that face at
every rebuild; a face that is gone stops the part with a message naming the feature.

**Edit operations.** `edit.rs` is the one API for changing a document (add a feature, set a
dimension, add a modifier, suppress, reorder, components, mates, ...). The window, Python and
the assistant all go through it; numbers may be expressions of the parameters, which become
bindings; a refused edit leaves the document as it was and reads as a sentence.

**Mates** (coincident, concentric, distance, angle, fixed, between named faces) are solved by
damped least squares over each free part's position and turn on every build; the degrees of
freedom left and the mates that cannot be met are reported. A mate's value can follow a
dimension, so keying the dimension animates the assembly.

### 5.2 Files

- `.afxcad`: the document as pretty-printed JSON with a format version (older files load; a
  newer one is refused with a message). This is what `.afx` state files embed.
- `.step/.stp` (AP242, AP214, AP203), `.igs/.iges`, `.3mf`, `.sc`: read as the base feature of
  a document, with the bytes kept inside it so the document is self-contained; written by the
  B-rep kernel's exact writers.
- A part that only the implicit kernel builds is written to STEP as a faceted B-rep by auraFX's
  own writer and labelled as faceted in the file's header.
- `.stl`, `.obj`: read as triangle bodies and written.

### 5.3 Python

```python
m = afx.cad_model()                              # a plate; afx.cad_model("part.step") opens a file
m.cad.parameter("width", "60 mm")                # a named dimension
m.cad.hole(diameter="width / 10", depth=20, name="Bore", at=(0, 0, 12))
m.cad.fillet(0.8)                                # round the whole part
m.set(width=80)                                  # the part's dimensions are node parameters
e = m.edit_cad()                                 # the environment, from a script
e.rename("Bore", "Pin hole")
e.apply()                                        # check downstream, apply the default fixes
m.cad.save("part.step"); m.cad.save("part.afxcad")
```

The full list of calls is in the guide's Python API reference and in the assistant's help
text (`crates/aurafx-ai/src/lib.rs`).

## 6. Rules

- The vendored trees are upstream code: change them only with an `// auraFX:` mark and an
  entry in `vendor/README.md`; put auraFX's own work in `aurafx-cad` and the app.
- Nothing in the environment owns geometry data: every edit is a change to the document
  followed by an evaluation; panels read the document and the last `Evaluated`.
- One kernel-free document, and stable ids on everything the diff follows.
- Tests use the files in `testdata/cad/`, whose origin `testdata/README.md` explains.
- User guide pictures of the environment come from `scripts/userguide/shots/cad_*.py`, which
  drive it with `node.edit_cad()`.

## 7. Open

Listed in ROADMAP.md (section 13, *Later*, and Status): closed sweep paths, rails and
changing sections; lofts between sections that are not parallel; mate limits, gears and
collision while dragging; a component edited in place with the rest of the assembly faded;
axis and edge picks for mates; faces recognised on a part started from triangles.

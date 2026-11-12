# Getting started

## Installing

Prebuilt packages are attached to each release on GitHub
(`github.com/aashiquear/auraFX/releases`). A one-line installer downloads the right one and
puts the app in your path:

```bash
curl -fsSL https://raw.githubusercontent.com/aashiquear/auraFX/main/scripts/install.sh | sh   # Linux, macOS
irm https://raw.githubusercontent.com/aashiquear/auraFX/main/scripts/install.ps1 | iex          # Windows PowerShell
```

| System | Package | Notes |
|---|---|---|
| Linux (x86_64, aarch64) | `auraFX-<version>-<arch>.AppImage`, `auraFX-<version>-linux-<arch>.tar.gz` | the AppImage runs from anywhere (`chmod +x` first); the installer unpacks the tarball into `~/.local` and adds a menu entry (`--prefix DIR` for another place, `--uninstall` to remove); needs a Vulkan driver (Mesa or the vendor's) |
| macOS 11 or newer (Apple silicon, Intel) | `auraFX-<version>-macos-<arch>.dmg` | drag the app to Applications, or use the installer line; the app is signed ad hoc, so the first start is **right-click › Open** |
| Windows 10/11 (x64) | `auraFX-<version>-windows-x64-setup.exe`, `auraFX-<version>-windows-x64.zip` | the installer adds a Start-menu entry and file associations; the zip is portable (unpack anywhere and run `aurafx.exe`) |
| Android 8 or newer | `auraFX-<version>-android.apk` | a viewer build (files, filters, rendering; no Python console, no simulations); allow the install from the browser or file manager |
| iOS / iPadOS 15 or newer | `auraFX-<version>-ios-unsigned.ipa` | a viewer build; sign and install it with Xcode, AltStore or Sideloadly using your own Apple ID |

Desktop packages bundle Python 3.12 with NumPy, so the Python console works without setup;
the packages carry `aurafx-server` as well, for reading data on another machine (see [Data on
another machine](client-server.md): **File › Open on Server… › Install server** copies it to a
host over SSH). On Linux the installer also sets up the `.desktop` entry
(`scripts/install-desktop.sh` does only that, for a build from source).

To build from source you need Rust 1.95 or newer, Python 3.10+ with development headers, and a
Vulkan (Linux, Windows) or Metal (macOS) GPU driver:

```bash
cargo build --release
./target/release/aurafx                   # start
./target/release/aurafx run.e             # open a file
./target/release/aurafx --script setup.py # run a Python script at startup
```

## The window

![The main window with a file open](images/main_window.png)

The window has five areas; every panel can be closed, reopened from the **Windows** menu, and
dragged to another place.

| Area | Purpose |
|---|---|
| **Pipeline** (top left) | the files you opened and the filters below them; click to select, click the eye to show or hide |
| **Properties** / **Information** (bottom left) | the selected object's parameters and display settings; the Information tab shows arrays, ranges, bounds, cell types and time steps |
| **3D View** (middle) | the picture; drag to rotate, right-drag or wheel to zoom, middle-drag (or Shift-drag) to pan, double-click to fit |
| **Python**, **Charts**, **Spreadsheet**, **Log** (bottom) | the console, plots, the data as a table, messages |
| **Assistant**, **Simulations** (right) | the AI chat and the job runner |

The toolbar has the most used filters (Slice, Clip, Contour, Threshold, Calculator, Glyph,
Warp), the camera buttons (Reset, +X, +Y, +Z, Iso) and, for the selected object, its
representation, the array it is colored by and the color map. The status bar at the bottom
names the GPU in use and counts the visible points and cells.

## Opening a file

- **File › Open…** (Ctrl+O), or drop a file onto the window, or start the app with the file
  name.
- **File › Open Recent** keeps the last files; the three sample datasets at the bottom of that
  menu (a heated disk, a wavelet volume, a sphere) are good for trying things out.
- Numbered files (`run_0001.vtu`, `run_0002.vtu`, …), `.pvd` collections, Exodus and
  OpenFOAM cases open as time series: the time toolbar appears under the main toolbar.

When a file opens, the object is colored by its first array and the camera fits the data. The
file is watched: if a solver rewrites it, auraFX reloads it (**Watch file** in Properties).

```python
d = afx.load("run.e")              # Python does the same
d.set(color_by="Temp")
afx.view("iso"); afx.reset_camera()
```

## Applying a filter

Select an object, then pick a filter from the toolbar, the **Filters** menu, or the command
palette (**Ctrl+K**, type part of its name). The filter runs at once with sensible defaults (a
slice through the center, a contour at the middle value) and appears below its input in the
Pipeline. Change its parameters in Properties; they apply as you type. **Re-run** forces a
recomputation and **Reset to defaults** restores the defaults.

When a filter is added, every object above it in the Pipeline is **hidden**, so the view shows
the new result alone; click an object's eye to show it again. Filters whose result is a table or a
chart (Histogram, Plot Over Line, the statistics) leave their input shown.

### What fits this data

auraFX looks at the selected object (what kind of data it is, its cells and its arrays) and
puts the filters that make sense for it first:

- **Right-click** an object in the Pipeline, or right-click it in the 3D view, for a short list
  *Suggested for this data*: Slice, Contour and Threshold for a volume with values, Stream
  Tracer and Glyph when there are vectors, Surface Normals and Decimate for a surface, Table To
  Points and Histogram for a table, Orientation Colors for an EBSD map. Hover over one to read
  why it is offered. *All filters* below has the rest.
- In the 3D view the menu is about the point you clicked: **Slice here** and **Clip here** put
  the cut through that point, **Turn the view about this point** moves the center of rotation,
  and *Draw as* and *Color by* change how the object looks. (Dragging with the right button
  still zooms; the menu opens on a click.)
- The **Filters** menu and the command palette show the same suggestions first. A filter that
  is unlikely to work on this data is faded and says what it needs ("needs a vector array",
  "needs a table"); you can still choose it. The toolbar's buttons fade the same way.

![The command palette on a volume with values, before anything is typed: the suggested filters come first, each with its reason](images/suggestions_palette.png)

Filters can be stacked: a Contour of a Clip of a file. Delete an object with the trash button
or the Delete key; the objects below it go too.

## Adding a source

The **Sources** menu makes data without a file: shapes, synthetic fields, meshes, CAD parts.
Picking one opens a small window that lists its standard settings (a sphere's radius and
resolution, a mesh's sizes, a CAD Model's 40 × 30 × 8 mm plate) with two choices:

- **Insert with these settings** adds it as it is.
- **Change settings first…** shows its basic settings to edit before it is added (for a CAD
  Model, the starter part, a template or the plate with its width, depth and thickness). The other
  settings are in Properties afterwards.

Tick **Don't ask again** to insert sources straight away; **Settings › Ask before adding a
source** turns the window back on. Sources added from Python (`afx.sphere(radius=2)`) never ask.

![Adding a source: the standard settings of a Columnar Mesh, and the button to change them first](images/source_dialog.png)

## Saving your work

- **File › Save State** (Ctrl+S) writes an `.afx` file holding the pipeline, every parameter,
  the display settings, the camera and any keyframes. **File › Open** reopens it, including
  objects that live on a remote server.
- On the next start, **Restore last session** in the empty view (or in the File menu) brings
  back the last state. The state is also saved every half minute while you work, so little is
  lost if the computer or the program stops.
- If auraFX ever closes on an internal error, the next start says so and shows where the
  **crash report** was written (a text file under `~/.cache/aurafx/crashes`, with what failed
  and where). Nothing is sent anywhere; attach the file to a bug report. The same window offers
  to restore the last session.
- Right-click an object › **Save data…** writes its output to a file (see
  [File formats](file-formats.md)).
- **Edit › Undo / Redo** cover every change to the pipeline and display settings.

## Getting help

- The **Welcome** window opens at the first start (and from **Help › Welcome**): one button into
  each kind of work (look at results, model a part, build a mesh, run and automate) and a few
  things worth knowing. Untick *Show at start* when you no longer need it.
- **Help › User guide** opens this guide in your browser.
- The **Model** menu starts a CAD part or a Geometry source and lists the meshing filters.
- **Help › Keyboard shortcuts** and **Help › Python API cheat sheet** open inside the app.
- Hover over anything for what it does; right-click an object for what can be done with it.
- The command palette (Ctrl+K) finds filters, sources and actions by name.
- Menus with many entries (the Sources menu, a category of filters) scroll with the mouse wheel
  when they are taller than the window. Each filter appears once, under its category; the
  Pipeline's right-click **All filters** has the same categories.
- The AI assistant answers questions about the loaded data and the app (see
  [AI assistant](ai-assistant.md)).

![The Welcome window: where to start, and a few things worth knowing](images/welcome.png)

## Open-source software in auraFX

**Help › About auraFX** shows the version, the graphics card in use and the Python version.
auraFX includes open-source software: the CAD environment's B-rep kernel and its CAD file formats
come from the SolidCraft and solidcore projects (see the README's Credits), and the program is
built from several hundred open-source Rust libraries. **Third-party licences…** in the About
window lists each of them with its licence, followed by the copyright notices and licence texts
they ask to be passed on. Type in **Find** to show only the lines with a library's name or a
licence (for example `Apache-2.0`). The same text is the file `THIRD-PARTY-NOTICES.txt` next to
the program in every package.

![The About window and its third-party licences, found by a library name](images/about_licences.png)

# Time, charts and animation

## Time steps

Files with several time steps (Exodus, `.pvd`, OpenFOAM cases, LAMMPS dumps, numbered file
series, VTKHDF, EnSight, Xdmf, NetCDF, CGNS) show the **time toolbar**: play/pause, first,
previous, next, last, a slider, the step number, the time value and the playback speed.
Every filter follows the time step; filters that look across time (Temporal Statistics,
Particle Tracer, Temporal Interpolator…) read the steps they need.

```python
afx.times()          # the time values
afx.time(20)         # go to step 20
```

![A time step of an impact simulation, with the maximum plastic strain over time](images/time_charts.png)

## Charts

The **Charts** panel plots lines with zoom, legends and log axes:

- **Plot Over Line** (a filter) plots the arrays along a line;
- **Edit › Plot Over Time…** or `afx.over_time(node, "EQPS", stat="max")` plots a statistic
  (max, min, mean, sum, or the value of a global variable) over every time step;
- `afx.plot(x, y, label="T")` plots your own NumPy arrays; `afx.clear_plots()` empties the panel.

The **Spreadsheet** panel shows the selected object's points, cells or field data as a table
with sorting, column choice and export to CSV.

## Animation

**File › Animation…** records one of three things:

| Mode | What it plays |
|---|---|
| **Time steps** | every time step of the data |
| **Orbit 360°** | the camera turning once around the data (choose the number of frames) |
| **Keyframes** | the keyframe timeline described below |

Pick the frame rate and the format: **GIF**, **MP4** (needs `ffmpeg` on the PATH) or a folder
of **PNG frames**. Frames are captured at the 3D view's size, with the legend, labels and
annotations.

```python
afx.save_animation("run.gif", fps=10)          # time steps (or keyframes if any)
afx.save_animation("turn.mp4", orbit=72)       # a turntable
```

## Keyframes

![The Animation window with a camera track and an opacity track](images/animation.png)

The **Keyframes** tab of the Animation window is a timeline from 0 % to 100 %:

1. Move the slider, set up the view, press **Key view at …** to record the camera.
2. Select a filter or display value in the list (any numeric filter parameter, or opacity,
   point size, line width, colors…), set it, press **Key value at …**.
3. Each key has an interpolation menu for the way it moves on to the next key: **linear**,
   **step** (hold), **ease in**, **ease out**, **ease in-out**, **exponential** (constant
   ratio, for zooms and scales) or **wave** (there and back, a sinusoid).
4. Press play to preview; tick **Also play the time steps** to run the data's time along
   the animation; set the number of **Frames**; **Save…** records it.

The camera moves on a sphere around the focal point, so turns look even and zooms do not
rush. Orange dots on the timeline are camera keys, blue dots value keys; click a key to jump
to it, right-click to delete it.

```python
afx.key_camera(0.0); afx.view("+y"); afx.key_camera(1.0, interp="ease in-out")
c.key("opacity", 0.0, 1.0); c.key("opacity", 1.0, 0.3)
s.key("origin", 0.5, (0, 2, 0), interp="wave")
afx.animation(frames=120, time=True)
afx.animation_time(0.4)                        # preview a moment
afx.save_animation("keys.mp4", fps=24)
afx.clear_keyframes()
```

## Animations driven by a function

`afx.save_animation("a.mp4", cue=f, frames=60)` calls your function `f(t)` with the
animation time `t` from 0 to 1 before each frame (after the keyframes are applied), so
anything can change between frames: filter values, colors, the data loaded. The call returns
when the file is written.

```python
def cue(t):
    c.set(values=[350 + 500 * t])
afx.save_animation("contour_sweep.gif", fps=8, cue=cue, frames=40)
```

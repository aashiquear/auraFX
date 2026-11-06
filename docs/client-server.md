# Opening data on another machine

When the data lives on a cluster or a GPU box, auraFX can read and filter it **there** and
bring back only what you look at, with nothing to set up beyond SSH.

## How it works

`aurafx-server` is a small program without a window. The app starts it on the host over SSH
(`ssh host aurafx-server --stdio`) and talks to it through that connection, so keys, jump hosts
and `~/.ssh/config` work as usual and no port is opened. Files you open on the server are
read there; filters you add below them (Slice, Clip, Contour, Threshold, Stream Tracer,
temporal filters and so on) run there too. What comes back:

| Situation | What is transferred |
|---|---|
| Visible, output smaller than 64 MB | the whole output |
| Visible, larger | its outer surface (all arrays), which is what the 3D view draws |
| Hidden (for example a reader with a slice below it) | only its summary: arrays, ranges, bounds, time steps |
| "Bring all data" ticked in Properties, or a filter below it that runs here | the whole output |

The Information panel always describes the full data on the server. Python filters (Programmable
Filter, Python Calculator) and plugin filters run on this computer, on the data brought over.
The server keeps results, so changing one filter only re-runs that filter there.

## Drawing on the server

Some outputs are too big even as a surface (a 50-million-cell mesh has a surface of a few
gigabytes). Then the server draws the object with its own GPU and sends pictures instead: a
color image and a depth image per view (LZ4-compressed). auraFX blends
them into the 3D view by depth, so server-drawn objects hide and are hidden by local objects
correctly. While you move the camera the last picture stays up until the next one arrives
(a 640×480 picture took 64 ms round trip over a LAN in the tests: 31 ms on the server's GPU).

**Display › Draw on** picks per object:

| Setting | What happens |
|---|---|
| auto | drawn here; drawn on the server only when its surface is over 64 MB |
| this computer | always drawn here (the surface comes over, however big) |
| the server | always drawn there; only pictures come over (translucent and volume-rendered objects are still drawn here) |

Server pictures use the view's lights and shadows; Eye-Dome Lighting, ambient occlusion, FXAA
and tone mapping are applied here to the whole picture. Shadows are worked out on each machine,
so server-drawn objects don't cast shadows on local ones (or the reverse).

From Python: `node.set(draw_on="server")`. Picking, rulers and selections need the geometry
here, so they don't work on server-drawn objects; switch to "this computer" for those. A server
without a GPU (or started with `--no-gpu`) sends surfaces instead. `aurafx-server --version
--gpu` shows which GPU it would use; **Check server** shows the same.

## Parallel servers (several processes)

Data too big for one process (or one machine's memory) can be spread over several server
processes, each holding one piece:

- **Open on Server › Processes**, or `afx.load(path, host="cluster", processes=8)`, starts
  `aurafx-server --workers 8`, which starts eight worker processes
  (`aurafx-server --piece k/8`) on the same machine. On a cluster,
  `aurafx-server --workers node1:16,node2:16` starts them on other machines over ssh (the data
  must be at the same path there, as on a shared file system).
- Partitioned files (`.pvtu`, `.pvtp`, `.pvts`, `.pvti`, also as steps of a `.pvd` series) are
  read piece by piece: each process opens only its own piece files. Other files are read by
  every process and then split (by blocks, cell ranges or slabs of structured data), which
  spreads the work but not the memory.
- Readers and filters that look at one cell at a time (Slice, Clip, Contour, Threshold,
  Calculator, Warp, Transform, Glyph, Tetrahedralize, Elevation, Cell Size, ...) run on every
  piece. Automatic values (slice through the center, contour at mid-range, glyph size) are taken
  from the whole data, so every piece uses the same plane and values.
- What comes back is combined: data appended, surfaces merged with the faces between pieces
  removed, summaries added up, and pictures composited by depth ("sort-last"; workers draw
  without multisampling at twice the size and the composite is reduced, so there are no seams).
- Other filters (Integrate Variables, Stream Tracer, Gradient, Extract Surface as a filter, ...)
  run on the coordinator on the gathered data. Temporal filters (particle tracers, temporal
  statistics) are not available on a parallel server yet.

Results match a single process: the tests compare integrals, surfaces and pictures to 1e-9 /
pixel level. Points on borders between pieces appear once per piece in gathered data, so point
counts are a little higher than in the file.

## Setting up a host

1. Make sure `ssh yourhost` works without a password prompt (an SSH key or agent).
2. Put `aurafx-server` on the host, any of:
   - **File › Open on Server… › Install server** copies the one shipped with the app to
     `~/.local/bin` (Linux hosts with the same CPU architecture; the release packages are
     built on Ubuntu 22.04 and run on glibc 2.35 or newer; a server you build yourself needs
     the glibc of the machine it was built on);
   - build it there: `cargo build --release -p aurafx-server`;
   - or point `AURAFX_SERVER` at it in the host's setup lines (Simulations panel).
3. **Check server** in the same window shows its version.

Hosts come from the Simulations panel and from `~/.ssh/config`. "This computer" runs the server
locally, which is handy for trying it out.

## Opening files

**File › Open on Server…**: pick the host, browse to the file, double-click it (or select it
and press **Open**). A cloud icon marks nodes that run on a server; hover it, or look at the top
of Properties, to see what was transferred and how long the server took.

From Python:

```python
d = afx.load("/scratch/run42/out.e", host="cluster")    # a Simulations host, ssh alias or user@machine
d = afx.load("ssh://cluster/scratch/run42/out.e")        # the same
s = d.slice(normal=(0, 0, 1))                             # runs on the server
s.set(all_data=True)                                      # bring the whole output over
afx.servers()                                             # [{'name': 'cluster', 'connected': True, 'cpus': 64, ...}]
```

`afx.load_remote(host, path)` still exists: it copies the file here first.

## Notes

- The connection restarts by itself if it drops; the next update sends the pipeline again.
- State files remember which host a node was on and reconnect when loaded.
- Time steps of server files drive the time bar like local ones.
- Not yet: MPI launchers (srun/mpirun) for the workers, ghost cells, sharing one server between
  people, translucent or volume-rendered objects drawn on the server (those are drawn here).

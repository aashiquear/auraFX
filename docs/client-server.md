# Opening data on another machine

When the data lives on a cluster or a GPU box, auraFX can read and filter it **there** and
bring back only what you look at, like ParaView's `pvserver`, but with nothing to set up
beyond SSH.

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

## Setting up a host

1. Make sure `ssh yourhost` works without a password prompt (an SSH key or agent).
2. Put `aurafx-server` on the host, any of:
   - **File › Open on Server… › Install server** copies the one shipped with the app to
     `~/.local/bin` (Linux hosts with the same CPU architecture, glibc 2.39 or newer for
     builds made on current distributions);
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
- Not yet: rendering on the server (images streamed back), several processes per server (MPI),
  sharing one server between people.

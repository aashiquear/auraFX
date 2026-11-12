# Data on another machine

This page moved into the user guide, which is kept current with the app:
[Data on another machine](userguide/src/client-server.md) (in the online book:
`docs/userguide/book/client-server.html`).

In short: `aurafx-server` reads and filters data where it lives and sends back only what the
3D view needs (the data, its surface, or pictures drawn by the server's GPU). It runs over SSH,
as several processes for big data (started by the server itself, `mpirun` or `srun`, with ghost
layers between the pieces), or as one process that streams a file larger than memory piece by
piece.

```python
d = afx.load("/scratch/run/out.e", host="cluster")
big = afx.load("/scratch/run/out.pvtu", host="cluster", processes=64, launcher="srun")
huge = afx.load("/data/volume.vti", stream=16)
```

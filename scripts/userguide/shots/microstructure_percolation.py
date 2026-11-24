# Percolation and Tortuosity (no file needed). Left and middle: overlapping spheres at volume
# fractions of 0.18 and 0.34, colored by connected part (ClusterIds: 1 is the largest); below
# the fraction near 0.29 they are many parts, above it one part joins the faces. Right: the
# pores of the second, colored by the length of the shortest path from the bottom face.
import aurafx as afx
shown = []
def place(src, dx=0.0, dz=0.0, color=(0.82, 0.52, 0.28), by=None, lower=2, upper=None, cmap=None, array="Phases"):
    """The cells of a microstructure whose `array` lies from `lower` (to `upper`), moved aside."""
    src.hide()
    t = src.threshold(array=array, lower=lower) if upper is None else src.threshold(array=array, lower=lower, upper=upper)
    t.hide()
    m = t.transform(translate=(1.3 * dx, 0, 1.2 * dz))
    if by:
        if cmap:
            m.set(color_by=by, cmap=cmap)
        else:
            m.set(color_by=by)
    else:
        m.set(color_by=None, solid_color=color)
    shown.append(m)
    return m
def finish(direction=(0.6, -1.0, 0.7), up=(0, 0, 1)):
    for n in afx.nodes():
        n.hide()
    for m in shown:
        m.show()
    shown[-1].activate()
    afx.view(list(direction), up=list(up)); afx.reset_camera()
    afx.screenshot(OUT)
low = afx.overlapping_spheres(radius=5, fraction=0.18).percolation(phase=2)
place(low, by="ClusterIds", cmap="Turbo")
high = afx.overlapping_spheres(radius=5, fraction=0.34)
joined = high.percolation(phase=2)
place(joined, dx=74, by="ClusterIds", cmap="Turbo")
paths = high.tortuosity(phase=1, axis="Z")
paths.hide()
cut = paths.clip(origin=(32, 32, 32), normal=(0, 1, 0)); cut.hide()
place(cut, dx=148, lower=1, upper=1, by="PathLength", cmap="Viridis")
finish()

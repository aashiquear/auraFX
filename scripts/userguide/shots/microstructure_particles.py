# Synthetic Microstructure, particles (no file needed), left to right: overlapping spheres at
# a fraction of 0.3, penetrable spheres with cores of half their radius, a dense packing of hard
# spheres at 0.55 (each sphere in its own color), and particles clustered in aggregates
# (the particles and, around them, the rest of the aggregates as a third phase).
import aurafx as afx
shown = []
def place(src, dx=0.0, dz=0.0, color=(0.82, 0.52, 0.28), by=None, lower=2, cmap=None):
    """The solid of a microstructure (its cells of phase `lower` and above), moved aside."""
    src.hide()
    t = src.threshold(array="Phases", lower=lower)
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
def finish(direction=(0.6, -1.0, 0.7)):
    for n in afx.nodes():
        n.hide()
    for m in shown:
        m.show()
    shown[-1].activate()
    afx.view(list(direction), up=[0, 0, 1]); afx.reset_camera()
    afx.screenshot(OUT)
place(afx.overlapping_spheres(radius=6, fraction=0.3), color=(0.80, 0.50, 0.26))
place(afx.penetrable_spheres(radius=6, impenetrability=0.5, density=0.6), dx=80, color=(0.36, 0.56, 0.78))
place(afx.hard_spheres(radius=5, fraction=0.55, gap=0, method="Dense packing"), dx=160, by="FeatureIds", cmap="Turbo")
place(afx.clustered_spheres(), dx=240, by="Phases", cmap="Cool to Warm")
finish()

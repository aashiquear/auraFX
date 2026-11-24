# Synthetic Microstructure, fibres and layers (no file needed). Top row: overlapping fibres in
# the XY plane, curved fibres (worm-like chains), bending fibres packed without overlap (each
# fibre in its own color). Bottom row: cross-linked fibres (the cross-links are the second
# color), a Delaunay fibre network, a random laminate with holes through its thinner layers.
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
place(afx.overlapping_fibres(radius=2, length=30, directions="In the XY plane", fraction=0.15), dz=80, color=(0.80, 0.50, 0.26))
place(afx.curved_fibres(length=60, persistence=30, fraction=0.1), dx=80, dz=80, color=(0.36, 0.56, 0.78))
place(afx.bending_fibres(radius=2, length=40, fraction=0.25), dx=160, dz=80, by="FeatureIds", cmap="Turbo")
place(afx.cross_linked_fibres(), by="Phases", cmap="Cool to Warm")
place(afx.delaunay_fibre_network(points=120, radius=1.3), dx=80, color=(0.45, 0.66, 0.42))
place(afx.random_laminate(), dx=160, color=(0.62, 0.48, 0.72))
finish()

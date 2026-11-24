# Synthetic Microstructure, random fields (no file needed), left to right: a Gaussian field
# medium at a fraction of 0.4, a transparent leaves medium at 0.35, and a medium rebuilt by
# simulated annealing from a two-point probability (40 cells along each side, fraction 0.3).
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
place(afx.gaussian_field_medium(length=8, fraction=0.4), color=(0.80, 0.50, 0.26))
place(afx.transparent_leaves_medium(radius=5, transparency=0.4, fraction=0.35), dx=80, color=(0.36, 0.56, 0.78))
place(afx.reconstructed_medium(cells=[40, 40, 40], spacing=1.6, fraction=0.3, length=4.8), dx=160, color=(0.45, 0.66, 0.42))
finish()

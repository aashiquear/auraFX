# Synthetic Microstructure, grains (no file needed), left to right: grains by nucleation at a
# constant rate stopped at time 14 (the transformed grains only), the same grown until the box
# is full, both colored by the inverse pole figure key; a dead leaves mosaic of two phases
# (fractions 0.7 and 0.3) colored by phase.
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
part = afx.nucleation_growth_grains(time=14, seed=2)
part.hide()
pc = part.orientation_colors(); pc.hide()
pt = pc.threshold(array="Phases", lower=1, upper=1); pt.hide()
a = pt.transform(); a.set(color_by="IPFColor"); shown.append(a)
full = afx.nucleation_growth_grains(seed=2)
full.hide()
fc = full.orientation_colors(); fc.hide()
b = fc.transform(translate=(104, 0, 0)); b.set(color_by="IPFColor"); shown.append(b)
leaves = afx.dead_leaves_mosaic(radius=8, phase_fractions=[0.7, 0.3], seed=3)
place(leaves, dx=160, by="Phases", lower=1, cmap="Cool to Warm")
finish()

# Coat Phase and Combine Microstructures (no file needed). Left: hard spheres with a coating two
# units thick, cut by a plane so that the cores and the coatings show (colored by phase).
# Right: a gyroid network kept only where an octet lattice of struts is solid ("Second inside
# the first"): a gyroid made of a lattice.
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
s = afx.hard_spheres(radius=7, fraction=0.15, gap=4)
s.hide()
coated = s.coat_phase(phase=2, thickness=2, periodic=True)
coated.hide()
ct = coated.threshold(array="Phases", lower=2); ct.hide()
cut = ct.clip(origin=(32, 30, 32), normal=(0, 1, 0)); cut.hide()
a = cut.transform(); a.set(color_by="Phases", cmap="Cool to Warm"); shown.append(a)
g = afx.triply_periodic_surface(surface="Gyroid", variant="Network", cell_size=64, fraction=0.5)
l = afx.strut_lattice(cell="Octet", cell_size=16, fraction=0.3)
g.hide(); l.hide()
both = afx.combine_microstructures(g, l, operation="Second inside the first")
place(both, dx=80, color=(0.36, 0.56, 0.78))
finish()

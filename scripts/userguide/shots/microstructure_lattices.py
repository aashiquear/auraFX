# Synthetic Microstructure, lattices and foams (no file needed). Top row: a gyroid sheet, an
# octet lattice of struts, a disordered lattice (struts missing, nodes moved), a hierarchical
# lattice (frames of octahedra). Bottom row: a Menger sponge, an open-cell foam, a level-set
# foam, a scale-rich network of plates. Every box is 64 units wide.
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
place(afx.triply_periodic_surface(surface="Gyroid", cell_size=32, fraction=0.2), dz=80, color=(0.80, 0.50, 0.26))
place(afx.strut_lattice(cell="Octet", cell_size=32, fraction=0.15), dx=80, dz=80, color=(0.36, 0.56, 0.78))
place(afx.disordered_strut_lattice(cell_size=16, coordination=6, perturbation=0.12, radius=1.3), dx=160, dz=80, color=(0.45, 0.66, 0.42))
place(afx.hierarchical_lattice(cells=[128, 128, 128], spacing=0.5, cell_size=64, radius=1.0), dx=240, dz=80, color=(0.62, 0.48, 0.72))
place(afx.fractal_sponge(cells=[81, 81, 81], spacing=64 / 81, cell_size=64, level=3), color=(0.78, 0.68, 0.30))
place(afx.open_cell_foam(foam_cells=64, fraction=0.12), dx=80, color=(0.75, 0.42, 0.42))
place(afx.level_set_foam(fraction=0.2), dx=160, color=(0.35, 0.65, 0.66))
place(afx.scale_rich_network(cells=[64, 64, 64], ligaments=200, decay=0.5, fraction=0.3), dx=240, color=(0.55, 0.55, 0.60))
finish()

# Synthetic Microstructure, bio-inspired structures (no file needed), left to right: nacre
# (each tablet in its own color), a twisted plywood colored by layer, a hexagonal honeycomb,
# and wood (earlywood, latewood and rays, colored by phase).
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
place(afx.nacre(cells=[96, 96, 48], spacing=64 / 96), by="FeatureIds", cmap="Turbo")
place(afx.twisted_plywood(), dx=80, by="Layer", cmap="Viridis")
place(afx.honeycomb(cells=[64, 64, 32]), dx=160, color=(0.78, 0.68, 0.30))
place(afx.honeycomb(arrangement="Wood", cells=[128, 64, 32], spacing=0.5, cell=8, wall=1.0, ring=64, ray_height=5, rays=0.06), dx=240, by="Phases", cmap="Cool to Warm")
finish(direction=(0.45, -1.0, 0.9))

# Synthetic Microstructure, variants of the types (no file needed). Top row, in the plane: an
# annealed automaton on hexagons, one on Penrose rhombi, Voronoi grains from stealthy seeds.
# Bottom row: a scale-rich network whose ligaments turn around a pole, spheres packed by
# collisions to a volume fraction of 0.6, and polarized blocks colored by the axis of each.
import aurafx as afx
shown = []
def place(src, dx=0.0, dz=0.0, color=(0.82, 0.52, 0.28), by=None, lower=2, upper=None, cmap=None, array="Phases", dy=0.0):
    """The cells of a microstructure whose `array` lies from `lower` (to `upper`), moved aside."""
    src.hide()
    t = src.threshold(array=array, lower=lower) if upper is None else src.threshold(array=array, lower=lower, upper=upper)
    t.hide()
    m = t.transform(translate=(1.3 * dx, dy, 1.2 * dz))
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
hexa = afx.cellular_automaton_structure(cells=[160, 160, 1], lattice="Hexagons", tile=6, steps=12, seed=2)
place(hexa, color=(0.20, 0.33, 0.52))
pen = afx.cellular_automaton_structure(cells=[160, 160, 1], lattice="Penrose rhombi", tile=10, steps=8, seed=3)
place(pen, dx=140, color=(0.42, 0.30, 0.56))
grains = afx.voronoi_grains(cells=[160, 160, 1], grains=120, periodic=True, stealth=0.4, seed=4)
gc = grains.orientation_colors(); gc.hide()
a = gc.transform(translate=(1.3 * 280, 0, 0)); a.set(color_by="IPFColor"); shown.append(a)
net = afx.scale_rich_network(cells=[160, 160, 1], ligaments=160, polarized=1.0, pole_angle=90, fraction=0.3, decay=0.4)
place(net, dy=-190, color=(0.60, 0.30, 0.22))
packed = afx.hard_spheres(cells=[96, 96, 96], radius=10, fraction=0.6, gap=0, method="Dense packing by collisions", growth_rate=0.02, periodic=False)
place(packed, dx=165, dy=-160, color=(0.82, 0.52, 0.28))
blocks = afx.polarized_blocks(cells=[96, 96, 96], tile_cells=24)
place(blocks, dx=305, dy=-160, by="BlockAxis", cmap="Cool to Warm")
finish(direction=(0.2, -0.45, 1), up=(0, 1, 0))

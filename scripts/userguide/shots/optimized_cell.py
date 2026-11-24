# Optimized Unit Cell (no file needed): a periodic cell found for its bulk modulus at a volume
# fraction of 0.3 on 32 x 32 x 32 cells, shown as two periods along each axis and cut open at
# one corner.
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
cell = afx.optimized_cell(goal="Bulk modulus", fraction=0.3, cells=[32, 32, 32], iterations=40)
cell.hide()
solid = cell.threshold(array="Phases", lower=2); solid.hide()
for k in range(8):
    if k == 7:
        continue
    m = solid.transform(translate=(32 * (k & 1), 32 * ((k >> 1) & 1), 32 * ((k >> 2) & 1)))
    m.set(color_by=None, solid_color=(0.36, 0.56, 0.78))
    shown.append(m)
finish(direction=(1.0, 0.8, 0.7))

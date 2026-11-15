import aurafx as afx, os, shutil, tempfile
# Pole figure pictures of both phases of a two-phase map, written as files and opened again
# (each copied into a folder of its own, so the files are not read as one numbered series).
m = afx.load(ROOT + "/testdata/ebsd/two_phase.ang")
d = tempfile.mkdtemp()
files = m.save_pole_figures(d, families="100, 111", size=192)
m.hide()
for k, (f, mrd) in enumerate(files):
    sub = os.path.join(d, "show%d" % k)
    os.makedirs(sub)
    g = os.path.join(sub, "figure.png")
    shutil.copy(f, g)
    im = afx.load(g).transform(translate=[(k % 2) * 210, -(k // 2) * 210, 0])
    im.set(color_by="PNGImage")
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT)

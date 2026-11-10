import aurafx as afx
# A NIfTI file whose sform reverses x and swaps y and z, drawn where the file places it.
n = afx.load(ROOT + "/testdata/volumes/head.nii.gz")
n.set(representation="Surface With Edges", color_by="NIFTI", cmap="Viridis")
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

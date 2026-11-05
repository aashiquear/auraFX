# A CGNS file in ADF storage (the format older CGNS files use) opens like any other: a wavelet
# on tetrahedra, clipped and colored by RTData.
import aurafx as afx
d = afx.load(ROOT + "/testdata/cgns/tets_adf.cgns")
k = d.clip(normal=(1, 0, 0)); k.set(color_by="RTData", representation="Surface With Edges")
d.hide(); k.info(); afx.view("iso"); afx.reset_camera()

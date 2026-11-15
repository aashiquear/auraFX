import aurafx as afx
# A CT-like phantom read from a folder of DICOM slices: a volume rendering and, beside it, the
# middle slice.
ct = afx.load(ROOT + "/testdata/volumes/dicom_phantom")
ct.set(representation="Volume", color_by="DICOMImage", cmap="Grayscale", range=(-100, 1100))
sl = ct.slice(normal=[0, 0, 1]).transform(translate=[80, 0, 0])
sl.set(color_by="DICOMImage", cmap="Grayscale", range=(-100, 120))
ct.show()
afx.view([0.4, -1.0, 1.0]); afx.reset_camera()
afx.screenshot(OUT)

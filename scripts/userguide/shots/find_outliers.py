import aurafx as afx
# A wavelet with a few values spoiled by a Python calculator; the cells around the outliers Find
# Outliers marks (Threshold on Outlier, any point), over a translucent slice.
w = afx.wavelet(half_size=16)
bad = w.python_calculator(expression="np.where(np.arange(len(RTData)) % 997 == 3, RTData * 3, RTData)", result="T")
o = bad.find_outliers(array="T", threshold=3.5)
o.set(representation="Outline")
hits = o.threshold(array="Outlier", lower=0.5, upper=1.5, method="Any point")
hits.set(color_by="OutlierScore", cmap="Inferno")
s = o.slice(normal=[0, 0, 1])
s.set(color_by="T", cmap="Cool to Warm", opacity=0.5)
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)

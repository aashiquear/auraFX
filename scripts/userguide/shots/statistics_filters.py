import aurafx as afx
# K-Means clusters of the Wavelet's values and positions, and a box plot of its arrays.
w = afx.wavelet()
c = w.coordinates()
k = c.k_means(k=4, variables="RTData, Coordinates[0]")
k.set(color_by="ClusterId", cmap="Turbo")
afx.hide_panel("Charts")
k.plot("RTData", "ClusterDistance", style="box", title="Box plots")
afx.view("iso"); afx.reset_camera()

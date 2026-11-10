# GPU filters: the Performance settings with the GPU switch, and a benchmark of Slice and Contour
# on a 201^3 wavelet with and without the GPU, shown in the 3D view's corner.
import aurafx as afx
dev = afx.gpu_filters()["device"]
lines = [f"{r['filter']}: CPU {r['cpu_s'] * 1000:.0f} ms, GPU {r['gpu_s'] * 1000:.0f} ms" for r in afx.gpu_benchmark(100)]
w = afx.wavelet()
w.slice(normal=[0.2, 0.5, 1.0]).set(color_by="RTData")
afx.annotate(f"{dev}\n" + "\n".join(lines), corner="lower right", size=16)
afx.view("iso"); afx.reset_camera()
afx.show_panel("settings")

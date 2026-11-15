import aurafx as afx
# The disk sample in four partitions (left) and partition 0 with a layer of ghost cells (right).
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
parts = d.redistribute_dataset(partitions=4)
parts.set(color_by="PartitionId", cmap="Turbo", representation="Surface With Edges")
g = parts.ghost_cells(layers=1).extract_block(blocks="0")
g.set(color_by="vtkGhostType", cmap="Cool to Warm", representation="Surface With Edges")
g.send_to_view("right")
afx.active_view(0)
parts.show()
afx.view("iso"); afx.reset_camera()
afx.active_view(1)
afx.view("iso"); afx.reset_camera()

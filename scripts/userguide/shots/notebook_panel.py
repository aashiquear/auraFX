import aurafx as afx, json, os, tempfile
# The Notebook panel running a small notebook (written here): a Markdown cell and a cell whose
# output is a picture of the 3D view.
nb = {"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": [
    {"cell_type": "markdown", "metadata": {}, "source": ["## A slice of the wavelet"]},
    {"cell_type": "code", "metadata": {}, "execution_count": None, "outputs": [], "source": [
        "w = afx.wavelet()\n", "s = w.slice(normal=[0, 0, 1])\n", "s.set(color_by='RTData')\n", "afx.view('+z'); afx.reset_camera()\n",
        "print('the wavelet has', w.info()['n_points'], 'points')\n", "afx.snapshot(width=520, height=150)"]},
]}
fd, path = tempfile.mkstemp(suffix=".ipynb")
os.close(fd)
with open(path, "w") as f:
    json.dump(nb, f)
for p in ("Charts", "Spreadsheet", "Log"):
    afx.hide_panel(p)
afx.show_panel("notebook run " + path)

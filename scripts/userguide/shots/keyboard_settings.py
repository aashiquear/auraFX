import aurafx as afx
# Settings in the high-contrast theme, open at the Keyboard section, with one key changed.
afx.set_theme("high contrast")
afx.set_shortcut("view_iso", "I")
w = afx.wavelet()
w.show()
afx.show_panel("settings")

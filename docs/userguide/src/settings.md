# Settings and keyboard shortcuts

## Settings

**Edit › Settings…** (Ctrl+,):

| Setting | Meaning |
|---|---|
| **Theme** | dark, light or system; the 3D background per theme |
| **Ask before adding a source** | adding a source opens a window to insert it with its standard settings or change them first (on by default) |
| **UI scale** | enlarges the whole interface |
| **Orientation axes**, **Time label** | shown in the 3D views |
| **Filter threads** | cores used by the parallel filters (0 = all); takes effect at the next start; `AURAFX_THREADS` overrides it |
| **Default color map** | for new objects |
| **AI** | provider, model, server URL, API key, auto-run and agent mode |
| **Plugins** | the Plugins window (also **Edit › Plugins…**) |
| **Lighting** | in **View › Lighting**; the choices are saved with the settings |

Settings live in `~/.config/aurafx/settings.toml` (Linux) or the app's config folder on
other systems; the window layout in `~/.local/share/aurafx/app.ron`.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| Ctrl+O | open a file |
| Ctrl+S | save state |
| Ctrl+Shift+S | save a screenshot |
| Ctrl+K, Ctrl+Space | command palette |
| Ctrl+Z, Ctrl+Y | undo, redo |
| Ctrl+Enter | run the code in the Python console |
| Ctrl+, | settings |
| R | reset the camera |
| Esc, P, M, S, D | view modes: navigate, pick, ruler, select cells, select points |
| Delete | delete the selected object |
| Space | play / pause time |
| Double-click in the view | fit the data |

**Help › Keyboard shortcuts** shows this list in the app.

## Environment variables

| Variable | Use |
|---|---|
| `AURAFX_THREADS` | thread limit for filters |
| `AURAFX_PLUGIN_PATH` | extra plugin folders |
| `AURAFX_SERVER` | path of `aurafx-server` on a host (in its setup lines) |
| `AURAFX_DATA_DIR` | where settings and state are kept |
| `OLLAMA_HOST`, `ANTHROPIC_API_KEY` | AI providers |
| `PYO3_PYTHON` (at build time) | which Python to embed |

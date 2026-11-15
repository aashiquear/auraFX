# Plugins

This page moved into the user guide, which is kept current with the app:
[Plugins](userguide/src/plugins.md) (in the online book: `docs/userguide/book/plugins.html`).

In short: a Python file in the plugin folder adds filters, sources, file readers and panels of
its own controls with `@afx.filter`, `@afx.source`, `@afx.reader` and `@afx.panel`, and reloads
when saved; filters and sources can also be written in Rust against the `aurafx-plugin` crate
and loaded as a shared library (`crates/aurafx-plugin-example` is a complete one).

```python
import numpy as np
import aurafx as afx

@afx.filter(category="Flow")
def speed(data, vectors=afx.ArrayParam(vector=True), name="speed"):
    """Magnitude of a vector array."""
    v = data.point_data[vectors]
    data.point_data[name] = np.linalg.norm(v, axis=1)
    return data
```

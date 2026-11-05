# AI assistant

![The Assistant panel](images/assistant.png)

The **Assistant** panel (right side; **Windows › Assistant**, or the **Ask AI** button) is a
chat with a language model that knows the app and your data. Ask in plain words: "slice this
through the middle and color by temperature", "what arrays does this file have?", "plot the
maximum stress over time", "why does my contour look cracked?". The assistant answers and,
when something should happen in the app, writes Python using the `afx` API.

## What the assistant sees

A text description of your pipeline: the objects, their filters and parameters, array names
with ranges, bounds, cell types and time steps, plus the last error. It never receives the
data itself. The description is rebuilt for every message.

## Running the code it writes

Code appears in a block with a **Run code** button; nothing runs until you press it, unless
**auto-run code** is ticked. With **agent mode** on, the assistant runs its code itself, looks
at the result (values, errors, the pipeline) and keeps going until the task is done, so
"find where the plate yields and plot it" becomes several steps without you in between.

```python
afx.ask("make an isosurface at the mean temperature")
afx.ask("find where the plate yields and plot the max stress over time", agent=True)
```

## Providers

Choose the provider and model in the panel's drop-downs or in **Settings**:

| Provider | Setup |
|---|---|
| **Ollama** (local or cloud models) | the server URL from `OLLAMA_HOST` or Settings; the model list comes from the server |
| **Anthropic** | set `ANTHROPIC_API_KEY` or paste a key in Settings |
| **OpenAI-compatible** (OpenAI, OpenRouter, Groq, vLLM, LM Studio…) | the base URL and key in Settings |

Answers stream as they are produced. The suggestion buttons under the welcome text are good
first questions for a new file.

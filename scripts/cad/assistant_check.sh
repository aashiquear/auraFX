#!/usr/bin/env bash
# Ask the AI assistant, in the running release app, to initiate, generate and edit a CAD model
# (scripts/cad/assistant_check.py) and print its report. Needs a running model server (Ollama:
# OLLAMA_HOST or the URL in Settings); the model is $AFX_ASSISTANT_MODEL (default qwen3-coder-next:latest).
# Backs up and restores the user's settings and app state; needs an X session like shots.sh.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/target/release/aurafx"
[ -x "$app" ] || { echo "build the release app first" >&2; exit 1; }
model="${AFX_ASSISTANT_MODEL:-qwen3-coder-next:latest}"
tmp="${TMPDIR:-/tmp}/aurafx-aicheck-$$"; mkdir -p "$tmp/plugins_empty" "$tmp/bak"
cfg="$HOME/.config/aurafx/settings.toml"; ron="$HOME/.local/share/aurafx/app.ron"
[ -f "$cfg" ] && cp "$cfg" "$tmp/bak/settings.toml"; [ -f "$ron" ] && cp "$ron" "$tmp/bak/app.ron"
restore() { [ -f "$tmp/bak/settings.toml" ] && cp "$tmp/bak/settings.toml" "$cfg"; [ -f "$tmp/bak/app.ron" ] && cp "$tmp/bak/app.ron" "$ron"; return 0; }
trap 'pkill -x aurafx 2>/dev/null; restore; rm -rf "$tmp"' EXIT
sed -i "s|^model = .*|model = \"$model\"|" "$cfg"
sed -i 's/^auto_run_ai_code = .*/auto_run_ai_code = true/' "$cfg"
result="$tmp/result.txt"
{ echo "RESULT = r'$result'"; echo "ROOT = r'$root'"; cat "$root/scripts/cad/assistant_check.py"; } > "$tmp/run.py"
pkill -x aurafx 2>/dev/null; sleep 0.5; rm -f "$ron"
( cd "$root" && AURAFX_PLUGIN_PATH="$tmp/plugins_empty" env -u WAYLAND_DISPLAY "$app" --script "$tmp/run.py" > "$tmp/app.log" 2>&1 & )
echo "model: $model"
for i in $(seq 1 "${AFX_ASSISTANT_WAIT:-1800}"); do
  [ -f "$result" ] && ! head -1 "$result" | grep -q RUNNING && break
  sleep 1
done
[ -f "$result" ] || { echo "FAIL: no result (see $tmp/app.log)"; tail -5 "$tmp/app.log"; exit 1; }
cat "$result"; echo
head -1 "$result" | grep -q '^PASS'

#!/usr/bin/env bash
# Render the project page's videos: scripts/site/media/<name>.py -> site/media/<name>.mp4.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/site/media"; mkdir -p "$out"
app="$root/target/release/aurafx"
[ -x "$app" ] || { echo "build the release app first" >&2; exit 1; }
command -v ffmpeg >/dev/null || { echo "ffmpeg is needed on the PATH" >&2; exit 1; }
tmp="${TMPDIR:-/tmp}/aurafx-site-$$"; mkdir -p "$tmp/plugins_empty" "$tmp/bak"
cfg="$HOME/.config/aurafx/settings.toml"; ron="$HOME/.local/share/aurafx/app.ron"
[ -f "$cfg" ] && cp "$cfg" "$tmp/bak/settings.toml"; [ -f "$ron" ] && cp "$ron" "$tmp/bak/app.ron"
restore() { [ -f "$tmp/bak/settings.toml" ] && cp "$tmp/bak/settings.toml" "$cfg"; [ -f "$tmp/bak/app.ron" ] && cp "$tmp/bak/app.ron" "$ron"; return 0; }
trap 'pkill -x aurafx 2>/dev/null; restore; rm -rf "$tmp"' EXIT
# The page's clips all use the light theme, whatever the user's own setting is (restored on exit).
if [ -f "$cfg" ]; then
  sed -i 's/^theme = .*/theme = "Light"/; s/^dark_theme = .*/dark_theme = false/' "$cfg"
fi
names=("$@")
[ ${#names[@]} -eq 0 ] && names=($(ls "$root/scripts/site/media/"*.py | xargs -n1 basename | sed 's/\.py$//'))
for name in "${names[@]}"; do
  mp4="$out/$name.mp4"; rm -f "$mp4"
  { echo "OUT = r'$mp4'"; echo "ROOT = r'$root'"; cat "$root/scripts/site/media/$name.py"; } > "$tmp/run.py"
  pkill -x aurafx 2>/dev/null; sleep 0.5; rm -f "$ron"
  ( cd "$root" && AURAFX_PLUGIN_PATH="$tmp/plugins_empty" env -u WAYLAND_DISPLAY "$app" --script "$tmp/run.py" > "$tmp/$name.log" 2>&1 & )
  for i in $(seq 1 "${MEDIA_WAIT:-600}"); do [ -f "$mp4" ] && break; sleep 1; done
  sleep 2; pkill -x aurafx 2>/dev/null; sleep 0.5
  if [ -f "$mp4" ]; then
    # The poster shown before the clip loads: its first frame.
    ffmpeg -loglevel error -y -i "$mp4" -frames:v 1 -q:v 3 "$out/$name.jpg"
    echo "$name: $(du -h "$mp4" | cut -f1)"
  else
    echo "$name: FAILED"; tail -5 "$tmp/$name.log"
  fi
done

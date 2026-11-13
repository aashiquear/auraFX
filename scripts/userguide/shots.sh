#!/usr/bin/env bash
# Generate user-guide screenshots. Each scripts/userguide/shots/<name>.py is run in the release app
# (aurafx --script). Scripts that call afx.screenshot(...) write their own picture; the others get a
# capture of the whole window after they ran. Pictures land in docs/userguide/src/images/<name>.png.
#
#   scripts/userguide/shots.sh            # every shot
#   scripts/userguide/shots.sh main_window lighting   # some
#
# Needs an X session (Wayland: the capture uses XWayland), xwininfo and ImageMagick's import.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/docs/userguide/src/images"
mkdir -p "$out"
app="$root/target/release/aurafx"
[ -x "$app" ] || { echo "build the release app first (cargo build --release -p aurafx-app)" >&2; exit 1; }
tmp="${TMPDIR:-/tmp}/aurafx-userguide-$$"
mkdir -p "$tmp/plugins_empty" "$tmp/bak"
cfg="$HOME/.config/aurafx/settings.toml"; ron="$HOME/.local/share/aurafx/app.ron"
[ -f "$cfg" ] && cp "$cfg" "$tmp/bak/settings.toml"; [ -f "$ron" ] && cp "$ron" "$tmp/bak/app.ron"
restore() { [ -f "$tmp/bak/settings.toml" ] && cp "$tmp/bak/settings.toml" "$cfg"; [ -f "$tmp/bak/app.ron" ] && cp "$tmp/bak/app.ron" "$ron"; return 0; }
trap 'pkill -x aurafx 2>/dev/null; restore; rm -rf "$tmp"' EXIT
# The guide's pictures use the light theme whatever the user's own setting is (restored on exit).
# AFX_SHOT_THEME=dark takes the dark ones.
# Set before every picture: a script that shows another theme must not change the next ones.
light_theme() {
  if [ -f "$cfg" ] && [ "${AFX_SHOT_THEME:-light}" = light ]; then
    sed -i 's/^theme = .*/theme = "Light"/; s/^dark_theme = .*/dark_theme = false/' "$cfg"
  fi
}
names=("$@")
[ ${#names[@]} -eq 0 ] && names=($(ls "$root/scripts/userguide/shots/"*.py | xargs -n1 basename | sed 's/\.py$//'))
for name in "${names[@]}"; do
  script="$root/scripts/userguide/shots/$name.py"
  png="$out/$name.png"
  rm -f "$png"
  # The script gets OUT (the picture path) and ROOT (the repo) as globals.
  { echo "OUT = r'$png'"; echo "ROOT = r'$root'"; cat "$script"; } > "$tmp/run.py"
  pkill -x aurafx 2>/dev/null; sleep 0.5
  rm -f "$ron"   # a fresh default layout for every picture
  light_theme
  # A script may ask for another interface, window size or safe area with a comment line such as
  #   # shot: interface=phone window=390x844 safe_area=47,34,0,0
  # (the tablet and phone pictures); the others get the desktop interface in the standard window.
  shot_env=()
  for kv in $(sed -n 's/^# *shot: *//p' "$script" | head -1); do
    case "$kv" in
      interface=*) shot_env+=("AURAFX_INTERFACE=${kv#interface=}") ;;
      window=*) shot_env+=("AURAFX_WINDOW=${kv#window=}") ;;
      safe_area=*) shot_env+=("AURAFX_SAFE_AREA=${kv#safe_area=}") ;;
      keyboard=*) shot_env+=("AURAFX_KEYBOARD=${kv#keyboard=}") ;;
    esac
  done
  ( cd "$root" && AURAFX_PLUGIN_PATH="$tmp/plugins_empty" env -u WAYLAND_DISPLAY -u AURAFX_INTERFACE -u AURAFX_WINDOW -u AURAFX_SAFE_AREA -u AURAFX_KEYBOARD "${shot_env[@]}" "$app" --script "$tmp/run.py" > "$tmp/$name.log" 2>&1 & )
  if grep -q "afx.screenshot" "$script"; then
    for i in $(seq 1 120); do [ -f "$png" ] && break; sleep 0.5; done
    sleep 1
  else
    sleep "${SHOT_DELAY:-9}"
    wid=$(xwininfo -root -tree | grep '"auraFX"' | awk '{print $1}' | head -1)
    [ -n "$wid" ] && import -window "$wid" "$png"
  fi
  pkill -x aurafx 2>/dev/null; sleep 0.5
  if [ -f "$png" ]; then
    convert "$png" -resize '1400x1400>' -strip "$png"
    if command -v pngquant >/dev/null; then
      pngquant --force --skip-if-larger --quality 70-95 --output "$png" "$png"
    elif [ "$(stat -c %s "$png")" -gt 400000 ]; then
      # No pngquant: a 256-color palette keeps text sharp and brings the file under 400 KB.
      convert "$png" -dither FloydSteinberg -colors 256 "PNG8:$png"
    fi
    echo "$name: $(identify -format '%wx%h' "$png") $(du -h "$png" | cut -f1)"
  else
    echo "$name: FAILED (see $tmp/$name.log)"; tail -5 "$tmp/$name.log"
  fi
done

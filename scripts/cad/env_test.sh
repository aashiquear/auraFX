#!/usr/bin/env bash
# Run scripts/cad/env_test.py inside the release app and report its result (needs an X session,
# like scripts/userguide/shots.sh). Backs up and restores the user's settings and app state.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
app="$root/target/release/aurafx"
[ -x "$app" ] || { echo "build the release app first" >&2; exit 1; }
tmp="${TMPDIR:-/tmp}/aurafx-cadtest-$$"; mkdir -p "$tmp/plugins_empty" "$tmp/bak"
cfg="$HOME/.config/aurafx/settings.toml"; ron="$HOME/.local/share/aurafx/app.ron"
[ -f "$cfg" ] && cp "$cfg" "$tmp/bak/settings.toml"; [ -f "$ron" ] && cp "$ron" "$tmp/bak/app.ron"
restore() { [ -f "$tmp/bak/settings.toml" ] && cp "$tmp/bak/settings.toml" "$cfg"; [ -f "$tmp/bak/app.ron" ] && cp "$tmp/bak/app.ron" "$ron"; return 0; }
trap 'pkill -x aurafx 2>/dev/null; restore; rm -rf "$tmp"' EXIT
result="$tmp/result.txt"
{ echo "RESULT = r'$result'"; echo "ROOT = r'$root'"; cat "$root/scripts/cad/env_test.py"; } > "$tmp/run.py"
pkill -x aurafx 2>/dev/null; sleep 0.5; rm -f "$ron"
( cd "$root" && AURAFX_PLUGIN_PATH="$tmp/plugins_empty" env -u WAYLAND_DISPLAY "$app" --script "$tmp/run.py" > "$tmp/app.log" 2>&1 & )
for i in $(seq 1 120); do [ -f "$result" ] && break; sleep 0.5; done
[ -f "$result" ] || { echo "FAIL: no result (see $tmp/app.log)"; cat "$tmp/app.log" | tail -5; exit 1; }
cat "$result"; echo
grep -q '^PASS' "$result"

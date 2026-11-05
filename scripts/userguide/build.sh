#!/usr/bin/env bash
# Build the user guide: mdBook HTML (docs/userguide/book) and PDF (docs/userguide/auraFX-user-guide.pdf).
# The Markdown sources in docs/userguide/src are the third format.
set -euo pipefail
cd "$(dirname "$0")/../../docs/userguide"
mdbook build
chrome=$(command -v google-chrome || command -v chromium || command -v chromium-browser || true)
if [ -z "$chrome" ]; then
  echo "no Chrome/Chromium found: HTML built, PDF skipped" >&2
  exit 0
fi
"$chrome" --headless=new --disable-gpu --no-sandbox --no-pdf-header-footer \
  --print-to-pdf="$PWD/auraFX-user-guide.pdf" "file://$PWD/book/print.html" 2>/dev/null
echo "built book/ and auraFX-user-guide.pdf"

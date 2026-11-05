Each `.py` file here makes one user-guide screenshot, `docs/userguide/src/images/<name>.png`.
It runs inside the release app (`aurafx --script`) with two globals: `OUT`, the path to write,
and `ROOT`, the repository. A script that calls `afx.screenshot(OUT)` produces a picture of the
3D view (with its legend and labels); a script that does not is captured as a whole window
after it has run (set `SHOT_DELAY` seconds if it needs longer). Run `scripts/userguide/shots.sh`
(all) or `scripts/userguide/shots.sh name`.

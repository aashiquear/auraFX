# Website media

`index.html` at the repository root is the project page (published with GitHub Pages from the
root). Its pictures are the user guide's own screenshots (`docs/userguide/src/images/`), and
its short videos are made by the scripts here, each a real auraFX rendering with no window
around it: `scripts/site/media.sh` runs every `scripts/site/media/<name>.py` in the release app
(`aurafx --script`, with `OUT` and `ROOT` as globals, like the user guide screenshots) and
writes `site/media/<name>.mp4` and its poster `site/media/<name>.jpg` (the first frame), always
in the light theme. It needs `ffmpeg` on the PATH.

The scripts only use files committed under `testdata/`, sources that need no file, and the
series written by the examples (`examples/phase_field/cahn_hilliard.py`,
`examples/grain_growth/potts.py`; run them first, they take a few seconds each).

    scripts/site/media.sh              # every video
    scripts/site/media.sh can_crash    # one

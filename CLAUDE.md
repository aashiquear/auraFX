# auraFX — rules for contributors and agents

## Every feature ships with a user guide entry

A feature is not done until the user guide describes it. For every feature added or changed:

1. Write (or update) the entry in `docs/userguide/src/<chapter>.md`: what it does, how to reach it
   in the UI (menu › item), the Python call, and any limits. Plain language, no internal names.
2. Add at least one screenshot that shows the feature in use. Screenshots are generated, not
   hand-made: add a script to `scripts/userguide/shots/` (see the README there), run
   `scripts/userguide/shots.sh <name>` and commit the PNG under `docs/userguide/src/images/`.
   Keep screenshots under 400 KB (the driver resizes them).
3. If the chapter is new, add it to `docs/userguide/src/SUMMARY.md`.
4. Rebuild the three formats with `scripts/userguide/build.sh` (Markdown sources, mdBook HTML
   in `docs/userguide/book/`, PDF at `docs/userguide/auraFX-user-guide.pdf`) and check the PDF
   opens. The HTML book is published from `docs/userguide/book/`.
5. Mention the chapter in the commit message.

Tooling: `mdbook` (cargo install mdbook) and Google Chrome (headless, for the PDF).

## Other conventions

- Rebuild `target/release/aurafx` (and `aurafx-server`) after each batch; the user runs the
  release binary directly.
- Drive the app for checks with `aurafx --script file.py`; use `AURAFX_PLUGIN_PATH` pointing at
  an empty folder so the user's plugins are untouched; stop it with `pkill -x aurafx`.
- Back up and restore `~/.config/aurafx/settings.toml` and `~/.local/share/aurafx/app.ron`
  around app runs.
- Compare readers, writers and filters against ParaView 6.1.1 (`pvpython` in
  `~/Downloads/ParaView-6.1.1-MPI-Linux-Python3.12-x86_64/bin`). Do not point ParaView at
  files our writers produced unless the file is small, and always run pvpython with a memory
  cap and a timeout: `(ulimit -v 8000000; timeout 180 pvpython check.py)`. A malformed file
  can make the HDF5 library allocate until the machine swaps (this crashed sessions twice);
  under the cap it fails with `bad_alloc` instead. ParaView's Python also has h5py.
- Count tests with `grep -oE 'ok\. [0-9]+ passed; [0-9]+ failed'` over `cargo test` output.
- Commit, do not push. Work in ROADMAP.md phase order and keep its Status section current.

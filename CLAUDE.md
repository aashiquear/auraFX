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
6. Screenshot scripts may only use files committed under `testdata/` (or sources that need no
   file), and `testdata/README.md` must say how every file there was made, so that anyone
   can regenerate every picture in the guide from the repository alone.

Tooling: `mdbook` (cargo install mdbook) and Google Chrome (headless, for the PDF).

## Every feature is reachable by the AI assistant

The assistant builds its catalog from the filter registry, so a filter or source registered
with a `FilterDef` (label, description, keywords, parameter help) is covered automatically.
Everything else a feature adds (readers and their options, writers, Python calls, display
settings, mesh commands, panels) must be added to the help text in `crates/aurafx-ai/src/lib.rs`
(a `format!` string: no double quotes inside it) in the same commit. Check it by asking the
assistant about the feature.

## Naming other software

auraFX is inspired by ParaView, DREAM.3D and Cubit, and that is said once, in the README. Everywhere
else (user guide, roadmap catalog, help texts, code comments, commit messages) describe what
auraFX does in its own terms: do not present a feature as a copy, clone or port of another
program's feature, and do not compare with those programs. The exceptions are file formats
named after a program, comments in readers and tests that explain how a reference file was written (a ParaView `.pvd` collection, DREAM.3D `.dream3d` files, a ParaView
color-map `.json`), and the test suite, `testdata/README.md` and `docs/reference/verification.md`, which may say
which reference program produced a reference file and how the results compare.

## Other conventions

- Rebuild `target/release/aurafx` (and `aurafx-server`) after each batch; the user runs the
  release binary directly.
- Drive the app for checks with `aurafx --script file.py`; use `AURAFX_PLUGIN_PATH` pointing at
  an empty folder so the user's plugins are untouched; stop it with `pkill -x aurafx`.
- Back up and restore `~/.config/aurafx/settings.toml` and `~/.local/share/aurafx/app.ron`
  around app runs.
- Check readers, writers and filters against a reference implementation where one exists; today that is ParaView 6.1.1 (`pvpython` in
  `~/Downloads/ParaView-6.1.1-MPI-Linux-Python3.12-x86_64/bin`). Do not point ParaView at
  files our writers produced unless the file is small, and always run pvpython with a memory
  cap and a timeout: `(ulimit -v 8000000; timeout 180 pvpython check.py)`. A malformed file
  can make the HDF5 library allocate until the machine swaps (this crashed sessions twice);
  under the cap it fails with `bad_alloc` instead. ParaView's Python also has h5py.
- Count tests with `grep -oE 'ok\. [0-9]+ passed; [0-9]+ failed'` over `cargo test` output.
- Commit, do not push. Work in ROADMAP.md phase order and keep its Status section current.

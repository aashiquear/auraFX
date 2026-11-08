# vendor/: the B-rep kernel, carried in the repository

auraFX's CAD environment has two kernels (`docs/design/cad-environment.md`, section 3). The
implicit kernel is auraFX's own. The B-rep kernel and the CAD file formats (STEP AP242, IGES,
3MF, `.sc`) come from two open-source libraries whose code is **carried here** instead of being
fetched at build time, so a release builds from this repository alone and auraFX is free to
change what it carries.

| Directory | Upstream | Carried version | Licence |
|---|---|---|---|
| `vendor/solidcraft/` | https://github.com/tangency-labs/solidcraft | tag `v0.40.0-rc.3` (commit `7001949c4`), carried 2026-11-07 | MIT OR Apache-2.0 |
| `vendor/solidcore/` | https://github.com/tangency-labs/solidcore | commit `a445bdaac` (the revision SolidCraft `v0.40.0-rc.3` pins), carried 2026-11-07 | Apache-2.0 |

Both keep their `LICENSE*`, `NOTICE`, `ATTRIBUTION.md`, `AGENTS.md` and `CONTRIBUTING.md`. The
licence of every crate that `aurafx-cad` pulls in, with these two, is in
`docs/reference/third-party.md` (regenerate it with `scripts/cad/third-party.sh`).

## How the code comes in

Each tree was added with `git subtree add --squash` at the version above: one commit holds the
whole upstream tree, and a second commit **pruned** it to the crates auraFX builds. Pruning
deletes whole directories and never edits an upstream file, so that later pulls merge (the
list is `vendor/prune.txt`):

- SolidCraft keeps `crates/{derive,doc,engine,geom,io,kernel,profile,render,sketch,smlib,step}`,
  its `vendor/` (patched crates it needs), `assets/` (the engine embeds some) and `xtask/`
  (a member of its workspace). Removed: `apps`, `docs`, `packaging`, `scripts`, `skills`,
  `tests`, `tools`, `examples` and the crates for its own window, GPU drawing, MCP server,
  dynamics and 3-D mouse.
- solidcore keeps `crates/` except `goldens`. Removed: `fixtures`, `reports`, `harness`,
  `site`, `docs`, `plan`, `scripts`, `assets`.

`vendor/solidcraft/apps/` is kept as an empty directory (`vendor/keep.txt`): SolidCraft's workspace
lists `apps/*` as members, and cargo reads a glob over a missing directory literally, which breaks
`cargo metadata` and `cargo fmt`.

The root `Cargo.toml` excludes `vendor/` from auraFX's workspace and depends on the crates by
path (`crates/aurafx-cad/Cargo.toml`: `solidcraft-engine`); SolidCraft names solidcore by git
revision, so a `[patch]` in the root `Cargo.toml` points both solidcore crates at
`vendor/solidcore`. The B-rep kernel is always built: there is no cargo feature and no external
program. The workspace `rust-version` is 1.95 (SolidCraft's).

## Diverging, and taking upstream work later

- auraFX's features go into `crates/aurafx-cad` and the app first. A change **inside** a
  vendored crate carries a `// auraFX:` comment and an entry in the list below, so that a
  merge shows it. Prefer new modules over edits of upstream files.
- Upstream features arrive with `scripts/cad/upstream-pull.sh <tag>`: it pulls the tag and the
  solidcore revision it pins, settles modify/delete conflicts in pruned directories by keeping
  them deleted, and prunes again. Then update the table above, rebuild, run `cargo test -p aurafx-cad`.
- `scripts/cad/upstream-diff.sh` lists the upstream commits since the carried version and the
  command ids that are new. The B-rep kernel maps auraFX's document onto SolidCraft *commands*
  (stable dotted ids such as `solid.extrude`), not onto its internal types, which keeps merges
  cheap: a new upstream command is a new arm in `kernel/brep.rs` and a new ribbon button.

## auraFX patches inside the vendored trees

None yet.

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

Both keep their `LICENSE*`, `NOTICE`, `ATTRIBUTION.md`, `AGENTS.md` and `CONTRIBUTING.md`.
`vendor/UPSTREAM.toml` records the exact upstream revision of each tree. Users never fetch
anything: the trees are plain files in this repository, and a clone builds the CAD environment
as it is.

## How the code comes in

Each tree was copied verbatim from upstream at the revision in `vendor/UPSTREAM.toml` and
pruned to the directories auraFX builds, in one commit. Only the carried files are in the
repository's history: none of upstream's own history, and none of the directories left out
(`vendor/prune.txt`). Pruning removes whole directories and never edits an upstream file.

- SolidCraft keeps `crates/{derive,doc,engine,geom,io,kernel,profile,render,sketch,smlib,step}`,
  its `vendor/` (patched crates it needs), `assets/` without `locales` (the engine embeds fonts,
  icons and render textures) and `xtask/` (a member of its workspace). Left out: `apps`, `docs`,
  `packaging`, `scripts`, `skills`, `tests`, `tools`, `examples`, `assets/locales` and the crates
  for its own window, GPU drawing, MCP server, dynamics and 3-D mouse.
- solidcore keeps `crates/` except `goldens` and the kernel test corpus
  (`crates/solidcore/tests/kernel_cases`). Left out: `fixtures`, `reports`, `harness`, `site`,
  `docs`, `plan`, `scripts`, `assets`.

`vendor/solidcraft/apps/` is kept as an empty directory (`vendor/keep.txt`): SolidCraft's workspace
lists `apps/*` as members, and cargo reads a glob over a missing directory literally, which breaks
`cargo metadata` and `cargo fmt`.

The root `Cargo.toml` excludes `vendor/` from auraFX's workspace and depends on the crates by
path (`crates/aurafx-cad/Cargo.toml`: `solidcraft-engine`); SolidCraft names solidcore by git
revision, so a `[patch]` in the root `Cargo.toml` points both solidcore crates at
`vendor/solidcore`. The B-rep kernel is always built: there is no cargo feature and no external
program. The workspace `rust-version` is 1.95 (SolidCraft's).

## Diverging, and taking upstream work later

auraFX's CAD module may diverge from upstream: auraFX adds its own features, and takes the
upstream ones it wants, when a developer decides to.

- auraFX's features go into `crates/aurafx-cad` and the app first. A change **inside** a
  vendored crate carries a `// auraFX:` comment and an entry in the list below (Apache-2.0 asks
  that changed files say so). Prefer new modules over edits of upstream files.
- `scripts/cad/upstream-diff.sh [tag]` lists the upstream commits since the carried revision
  and the command ids that are new or gone. It changes nothing.
- `scripts/cad/upstream-pull.sh <tag|branch|commit> [--dry-run]` takes the upstream change from
  the carried revision to the new one, **limited to the carried directories**, as a patch, and
  applies it with a three-way merge: auraFX's own edits stay, and a line both sides changed is
  left as a conflict to resolve. The upstream repositories are cloned (without file contents)
  into a temporary folder, so their history never enters this repository; only the changed
  carried files do. It lists the directories upstream added (carried unless added to
  `vendor/prune.txt`), takes the solidcore revision the new SolidCraft pins, records both in
  `vendor/UPSTREAM.toml`, and commits nothing. Then: review the staged change, run
  `scripts/licences.sh` (it stops on a licence auraFX cannot ship), build, run
  `cargo test -p aurafx-cad`, update the table above, commit.
- The B-rep kernel maps auraFX's document onto SolidCraft *commands* (stable dotted ids such as
  `solid.extrude`), not onto its internal types, which keeps updates cheap: a new upstream
  command is a new arm in `kernel/brep.rs` and a new ribbon button.

### When upstream moves solidcore

SolidCraft's `main` branch (November 2026, after `v0.40.0-rc.3`) carries solidcore inside its
own tree (`solidcore/`) instead of naming a revision. The first pull of such a version brings it
to `vendor/solidcraft/solidcore`, pruned like `vendor/solidcore` (the script says so). Once, by
hand: remove `vendor/solidcore` and the `[patch."https://github.com/tangency-labs/solidcore"]`
section of the root `Cargo.toml`, move the `vendor/solidcore/` lines of `vendor/prune.txt` to
`vendor/solidcraft/solidcore/`, drop the `[solidcore]` section of `vendor/UPSTREAM.toml`, and
update this page.

## Licences

SolidCraft is MIT OR Apache-2.0 and solidcore Apache-2.0 (solidcore is itself a port of NVIDIA's
USD BRep, Apache-2.0: its `NOTICE` says so). Both fit auraFX's MIT OR Apache-2.0. What Apache-2.0
asks of auraFX: keep their licence and `NOTICE` files (kept here), mark files auraFX changes (the
`// auraFX:` comments and the list below), and pass the notices on with the program:
`scripts/licences.sh` writes `THIRD-PARTY-NOTICES.txt` (both `NOTICE` files, the font licences
and the licence texts of every crate), the packages ship it, and Help › About auraFX ›
Third-party licences shows it. CI runs `scripts/licences.sh --check`. The fonts and textures
SolidCraft builds in are attributed file by file in `vendor/solidcraft/ATTRIBUTION.md` (MIT OR
Apache-2.0, SIL OFL 1.1, the Ubuntu Font Licence, public domain).

## auraFX patches inside the vendored trees

None yet.

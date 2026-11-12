# Contributing to auraFX

Thanks for helping. A few conventions keep the codebase consistent:

1. **Pick a roadmap item.** Every planned reader, filter and view is listed in
   [ROADMAP.md](ROADMAP.md). Open an issue saying which one you are taking.
2. **Filters** live in `crates/aurafx-filters`. Write the algorithm as a plain function on
   `DataSet`, then register it in `registry.rs` with a `FilterDef` (id, label, category,
   description, keywords, typed parameters). That one entry makes it appear in the menus,
   the command palette, the properties panel, the Python API (`node.<id>(...)`) and the AI
   assistant's catalog. Add a test in `crates/aurafx-filters/tests/` that checks numbers,
   ideally against an analytic answer or a reference implementation's output
   (`docs/reference/verification.md` says how the existing ones were checked).
3. **Readers** live in `crates/aurafx-io`. Register the extension in `lib.rs`
   (`supported_formats` and `read_file`). Add a small sample to `testdata/` and a test.
4. **UI**: favour defaults that work without configuration, auto-apply, and searchable
   actions. If a feature needs a manual, it needs a better default.
5. **Every feature ships with its documentation**: an entry in the user guide
   (`docs/userguide/src/<chapter>.md`) with a generated screenshot
   (`scripts/userguide/shots/`), and, for anything not registered as a filter, a line in the
   assistant's help text (`crates/aurafx-ai/src/lib.rs`). The full rules, including the ones
   for third-party code and licences, are in [CLAUDE.md](CLAUDE.md).
6. Run `cargo fmt`, `cargo clippy --workspace` and `cargo test --workspace` before opening a PR.

By contributing you agree your work is dual-licensed under MIT OR Apache-2.0.

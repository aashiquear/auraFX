# Contributing to auraFX

Thanks for helping. A few conventions keep the codebase consistent:

1. **Pick a roadmap item.** Every ParaView reader, filter and view is listed in
   [ROADMAP.md](ROADMAP.md). Open an issue saying which one you are taking.
2. **Filters** live in `crates/aurafx-filters`. Write the algorithm as a plain function on
   `DataSet`, then register it in `registry.rs` with a `FilterDef` (id, label, category,
   description, keywords, typed parameters). That one entry makes it appear in the menus,
   the command palette, the properties panel, the Python API (`node.<id>(...)`) and the AI
   assistant's catalog. Add a test in `crates/aurafx-filters/tests/` that checks numbers,
   ideally against an analytic answer or ParaView's output.
3. **Readers** live in `crates/aurafx-io`. Register the extension in `lib.rs`
   (`supported_formats` and `read_file`). Add a small sample to `testdata/` and a test.
4. **UI**: favour defaults that work without configuration, auto-apply, and searchable
   actions. If a feature needs a manual, it needs a better default.
5. Run `cargo fmt`, `cargo clippy --workspace` and `cargo test --workspace` before opening a PR.

By contributing you agree your work is dual-licensed under MIT OR Apache-2.0.

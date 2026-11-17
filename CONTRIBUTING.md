# Contributing

- Branch from `develop`; open pull requests against `develop`. `main` holds releases.
- Before submitting: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- The crate forbids `unsafe`. Behavior should match the C libmseed (`../libmseed`) unless
  a Rust-idiomatic difference is documented.
- New behavior needs tests; prefer porting the equivalent test from libmseed's `test/` directory.
- Releases: bump `version` under `[workspace.package]` in `Cargo.toml` and `version`/`date-released`
  in `CITATION.cff` (both must match the tag), update `CHANGELOG.md`, tag `vX.Y.Z` and publish a
  GitHub release; the release workflow publishes to crates.io and npm and attaches
  `libmseed.wasm`, and Zenodo archives the release and mints a DOI.

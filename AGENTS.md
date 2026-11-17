# AGENTS.md

Project instructions. These override default behavior.

libmseed-rs is a safe, pure-Rust port of the C libmseed (reference source in `../libmseed`).
The workspace has two crates: `libmseed` (root, `src/`) and `libmseed-wasm`
(WebAssembly bindings, published to npm).

## Git: never write to the repository unless told to

- Never run `git commit` unless I explicitly ask you to commit.
- Never run `git push`, to any remote, for any reason. There is no exception.
- Don't `git add` as a side effect of doing other work.

Reading git state (`git status`, `git diff`, `git log`) is always fine.

## Before reporting work as done

Run what CI runs and report any failure:

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
```

For changes to `libmseed-wasm`, also `wasm-pack test --node libmseed-wasm`.

## Porting rules

- No `unsafe`; the crate is `#![forbid(unsafe_code)]`.
- Behavior should match the C libmseed unless a Rust-idiomatic difference is
  documented. Check `../libmseed` before deciding how something should behave.
- New behavior needs tests; prefer porting the equivalent test from libmseed's `test/`
  directory into `tests/`, with any data files in `tests/data/`.
- Public items need doc comments (`#![warn(missing_docs)]`).

## After applying a fix, do two things unprompted

1. Report a single-line commit message: imperative mood, no trailing period, no
   type prefix (not `fix:`), no body. Say what the change does, not how it was
   arrived at.

   - Good: `Reject a blockette offset within the fixed header as fatal`
   - Bad: `fix: fixed the bug found in item 3 of the review`

2. Add that same line to `CHANGELOG.md` as a `- ` bullet under `## [Unreleased]`,
   in the matching [Keep a Changelog](https://keepachangelog.com/) subsection
   (`### Added`, `### Changed`, `### Fixed`, `### Removed`), creating the subsection
   if it is missing:

   ```
   ## [Unreleased]

   ### Fixed
   - Reuse the record packer and its buffers across trace-list segments
   ```

   Never add entries to a released `## [X.Y.Z] - date` section. Group closely
   related lines under one `- ` bullet with indented `- ` sub-bullets.

## Code comments

Write for a future reader who sees only the final code — never the previous state,
and never the conversation that produced it.

- Match the style, tone, and density of the comments adjacent to the code you are
  editing. Terse.
- State what the code does or what invariant holds. Do not state why it changed,
  what it used to do, or that it is new, fixed, or optimized.
- Never reference a conversation, a code review, a report, a to-do or error list,
  a benchmark run, or an AI model. None of that means anything in a release.
- Don't comment the obvious and don't restate the item name in its doc comment.

Example:

- Good: `// Sample rate is zero for records with no time series`
- Bad: `// Fixes item 4 from the review, was previously computed per segment`

## Never break the public API

The public surface is everything reachable from `src/lib.rs` (the `pub mod`s and
`pub use` re-exports), plus the JavaScript API exported by `libmseed-wasm`.
Releases follow SemVer.

Do not:

- change or remove an existing public function, method, or trait signature, or
  change observable behavior callers may rely on;
- rename, remove, or change the type of public struct fields or enum variants, or
  add them to a type that is not `#[non_exhaustive]`;
- change the value of an existing public constant;
- raise the minimum supported Rust version (`rust-version` in `Cargo.toml`).

Adding new items is fine. If a change you believe is correct requires breaking
the API, stop and tell me rather than doing it.

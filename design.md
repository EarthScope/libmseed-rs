# libmseed-rs

A pure-Rust port of libmseed.
No unsafe code.
The crate should be a port but ideomatic Rust.

Goal is a compact crate with most of the same functionality of libmseed:

- Read files and streams record-by-record, autodetecting miniSEED format and details,
  into a format agnostic container.
- Build trace listings by adding records.
- Support rolling-buffer style operation where data are added to a trace listing as
  a transitory buffer and full records are created when they can.
- Write miniSEED records from trace listings or record containers.

## Reference source and tests

limbseed C-language source code is in ../libmseed/

Port all of the tests

## published crate

This create will be published on crates.io and include documentation on docs.rs

A WASM build is created for the library and published on npm.  Also build a libmseed
.wasm file for publishing on github for releases.

## github actions

GitHub actions for:

- running tests on pushing to the develop or main branches
- running tests, packaging for crates.io, docs.rs and npm on github releases

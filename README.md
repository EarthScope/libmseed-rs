# Rust libmseed

[![CI](https://github.com/EarthScope/libmseed-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/EarthScope/libmseed-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/libmseed.svg)](https://crates.io/crates/libmseed)
[![docs.rs](https://img.shields.io/docsrs/libmseed)](https://docs.rs/libmseed)
[![npm](https://img.shields.io/npm/v/libmseed.svg)](https://www.npmjs.com/package/libmseed)
[![License](https://img.shields.io/crates/l/libmseed.svg)](LICENSE)

A pure-Rust port of [libmseed](https://github.com/EarthScope/libmseed) for reading, writing
and managing [miniSEED](https://docs.fdsn.org/projects/miniSEED3) 2 and 3 seismological time
series data. **No `unsafe` code**, a compact API in Rust idiom, and a WebAssembly build.

## Features

- Read files and streams record by record, auto-detecting the miniSEED version, record length,
  byte order and encoding, into a format agnostic `Record`
- All data encodings are supported
- Build trace lists by adding records, in any order, with merging and gap healing
- Rolling-buffer operation: add data to a trace list as it arrives and create full records
  when they can be, keeping the remainder for later; flush idle data
- Write miniSEED 3 or 2 records from trace lists or individual records
- JSON extra headers with JSON Pointer access, mapped to and from miniSEED 2 blockettes
- Data selections (glob patterns, time windows, publication versions), byte ranges,
  record lists for two-pass reading, leap second handling
- WebAssembly build published to npm

## Usage

```toml
[dependencies]
libmseed = "0.1"
```

```rust,no_run
use libmseed::{MsReader, ParseFlags};

fn main() -> Result<(), libmseed::Error> {
    for record in MsReader::open("data.mseed", ParseFlags::UNPACK_DATA)? {
        let record = record?;
        println!("{record}");
    }
    Ok(())
}
```

Build a trace list and pack records from it:

```rust,no_run
use libmseed::{Encoding, PackOptions, ParseFlags, ReadOptions, TraceList};

fn main() -> Result<(), libmseed::Error> {
    let mut traces = TraceList::new();
    traces.read_file("data.mseed", &ReadOptions::new(ParseFlags::UNPACK_DATA))?;

    let options = PackOptions {
        record_length: Some(512),
        encoding: Some(Encoding::STEIM2),
        flush: true,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    traces.pack_with(&options, |record| {
        bytes.extend_from_slice(record);
        Ok(())
    })?;
    Ok(())
}
```

See the [API documentation](https://docs.rs/libmseed) for the guide, including rolling-buffer
operation, extra headers and differences from the C library.

## Examples

For trying the library on your own data, the example `mseedview` program prints each
record in one or more files (`-p` for full headers, `-pp` to also print samples) and,
with `-T` (or `-g` to include gaps), the trace list they assemble into; `-d` decodes
data payloads (reporting decode errors without printing samples), `-n` skips the
per-record output, `-s` adds a summary and `-C` validates CRCs. A byte range can be
given as `file@start-end`:

```sh
cargo run --release --example mseedview -- -p -T data.mseed
```

More examples in [`examples/`](examples) show common tasks, run with
`cargo run --example <name>`:

- `read_buffer`: read miniSEED from a memory buffer into a trace list
- `read_recordlist`: build a trace list with record lists, optionally decoding data
- `read_selection`: read a file limited by data selections
- `pack_rollingbuffer`: add data to a trace list as it arrives and pack full records
- `extraheaders`: set, get and modify extra headers
- `sids`: map between FDSN Source Identifiers and network, station, location and channel codes
- `timestr`: convert time strings to the supported output formats

## WebAssembly

From npm, with a bundler (webpack, Vite, ...), which loads and initializes the
WebAssembly module on import:

```js
import { parseRecords, TraceList, packInt32 } from "libmseed";

const records = parseRecords(new Uint8Array(buffer), true);
```

Each release also attaches a standalone build for direct use in browsers without a bundler
(`libmseed.js`, `libmseed.wasm`, `libmseed.d.ts`), which is initialized explicitly:

```js
import init, { parseRecords } from "./libmseed.js";
await init({ module_or_path: "./libmseed.wasm" });
```

See [`libmseed-wasm`](libmseed-wasm/README.md).

## Development

```sh
cargo test --workspace          # all tests, including doctests
cargo clippy --workspace --all-targets -- -D warnings
wasm-pack test --node libmseed-wasm
```

Branches: work on `develop`, releases are tagged from `main`. Pushes to either run CI;
publishing a GitHub release publishes to crates.io (and so docs.rs) and npm.
See [CONTRIBUTING.md](CONTRIBUTING.md). Minimum supported Rust version: 1.85.

## License

Apache License, Version 2.0. See [LICENSE](LICENSE).

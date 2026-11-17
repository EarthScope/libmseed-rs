# libmseed (WebAssembly)

WebAssembly bindings for [libmseed-rs](https://github.com/EarthScope/libmseed-rs), a pure-Rust
port of libmseed for reading, writing and managing miniSEED 2 and 3 seismological data.

To try it without writing any code, a browser demo and a Node.js example are included in
this directory; see [Try it locally](#try-it-locally).

## npm, with a bundler

The npm package `libmseed` targets bundlers (webpack, Vite, Rollup with a WebAssembly
plugin, ...). The WebAssembly module is loaded and initialized on import:

```js
import { parseRecords, TraceList, packInt32 } from "libmseed";

const records = parseRecords(new Uint8Array(buffer), true);
for (const r of records) {
  console.log(r.sid, r.startTime, r.sampleRate, r.samplesInt32());
}

const traces = new TraceList();
traces.addBuffer(new Uint8Array(buffer), true);
console.log(traces.summary());

const miniseed = packInt32("FDSN:XX_TEST__B_H_Z", "2024-01-01T00:00:00Z", 40.0,
                           Int32Array.from({ length: 500 }, (_, i) => i), 11, 512, false);
```

## Browsers without a bundler

Each GitHub release attaches a standalone build: `libmseed.js`, `libmseed.wasm` and
`libmseed.d.ts`, plus a `.tar.gz` of the same build with wasm-pack's original file names.
Serve the files and initialize the module before use, giving the path of the `.wasm` file:

```html
<script type="module">
  import init, { parseRecords } from "./libmseed.js";

  await init({ module_or_path: "./libmseed.wasm" });
  const records = parseRecords(new Uint8Array(buffer), true);
</script>
```

From the `.tar.gz`, where the module is `libmseed_bg.wasm` next to `libmseed.js`,
`await init()` finds it without a path.

## Prerequisites

Building the WebAssembly module needs, in addition to a Rust toolchain installed with
[rustup](https://rustup.rs):

- the `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- [wasm-pack](https://github.com/wasm-bindgen/wasm-pack): `cargo install wasm-pack`
  (Homebrew `brew install wasm-pack` and npm `npm install -g wasm-pack` also work)

Check with `wasm-pack --version`. Running the Node.js example also needs Node.js 18 or later,
and the browser demo needs any static HTTP server (the commands below use Python's).

Nothing has to be built beforehand: `wasm-pack build` compiles the `libmseed` crate along
with these bindings, generates the JavaScript glue, and writes everything to an output
directory. Run all `wasm-pack` commands below from the **repository root** (the directory
containing the top-level `Cargo.toml`), not from `libmseed-wasm/`. The `--out-dir` path is
relative to the `libmseed-wasm/` crate, so `--out-dir pkg-web` produces
`libmseed-wasm/pkg-web/`.

## Building

```sh
wasm-pack build libmseed-wasm --target bundler --release --out-name libmseed  # npm package
wasm-pack build libmseed-wasm --target web --release --out-name libmseed      # standalone
wasm-pack test --node libmseed-wasm
```

## Try it locally

First install the tools listed under [Prerequisites](#prerequisites). Each example is a
build step followed by a run step; all commands are run from the repository root. There is
no separate `cargo build` step, since `wasm-pack build` compiles everything it needs (the
first build takes a minute or so).

### Browser demo

The demo (`www/index.html`) loads miniSEED files by drag-and-drop and shows the trace list,
the records and a plot of the first segment; it also has a sample file and a pack/parse
round-trip.

```sh
# from the repository root

# 1. Build the module for browsers; writes libmseed-wasm/pkg-web/
wasm-pack build libmseed-wasm --target web --out-dir pkg-web --out-name libmseed

# 2. Serve the repository root over HTTP (Ctrl-C to stop)
python3 -m http.server 8000
```

Then open <http://localhost:8000/libmseed-wasm/www/> in a browser.

The page must be served over HTTP, since browsers do not load WebAssembly from `file://`
URLs. Start the server from the repository root, not from `www/`: the page loads the module
from `../pkg-web/` and the sample file from `tests/data/`, both outside `www/`. Any static
server works in place of Python's, e.g. `npx serve -l 8000`.

### Node.js

`examples/node.mjs` prints the records and trace list of the files given, or of a test file
when given none:

```sh
# from the repository root

# 1. Build the module for Node.js; writes libmseed-wasm/pkg-node/
wasm-pack build libmseed-wasm --target nodejs --out-dir pkg-node --out-name libmseed

# 2. Run the example on one of the test files
node libmseed-wasm/examples/node.mjs tests/data/reference-testdata-steim2.mseed2
```

After changing the Rust code, rerun the build step; the browser page only needs a reload.

# libmseed (WebAssembly)

WebAssembly bindings for [libmseed-rs](https://github.com/EarthScope/libmseed-rs), a pure-Rust
port of libmseed for reading, writing and managing miniSEED 2 and 3 seismological data.

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

## Building

```sh
wasm-pack build libmseed-wasm --target bundler --release --out-name libmseed  # npm package
wasm-pack build libmseed-wasm --target web --release --out-name libmseed      # standalone
wasm-pack test --node libmseed-wasm
```

## Try it locally

A browser demo (`www/index.html`) loads miniSEED files by drag-and-drop and shows the trace
list, the records and a plot of the first segment; it also has a sample file and a
pack/parse round-trip. From the repository root:

```sh
wasm-pack build libmseed-wasm --target web --out-dir pkg-web --out-name libmseed
python3 -m http.server 8000      # or any static server, run from the repository root
# open http://localhost:8000/libmseed-wasm/www/
```

The page must be served over HTTP, since browsers do not load WebAssembly from `file://`
URLs. The server root has to be the repository root so the sample in `tests/data/` is
reachable.

From Node.js, `examples/node.mjs` prints the records and trace list of the files given, or
of a test file:

```sh
wasm-pack build libmseed-wasm --target nodejs --out-dir pkg-node --out-name libmseed
node libmseed-wasm/examples/node.mjs tests/data/reference-testdata-steim2.mseed2
```

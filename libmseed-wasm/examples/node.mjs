// Read a miniSEED file with the WebAssembly build from Node.js.
//
//   wasm-pack build libmseed-wasm --target nodejs --out-dir pkg-node --out-name libmseed
//   node libmseed-wasm/examples/node.mjs [file.mseed ...]
//
// Without arguments, reads a test file from the repository.

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { version, parseRecords, TraceList } = require("../pkg-node/libmseed.js");

const defaultFile = fileURLToPath(
  new URL("../../tests/data/reference-testdata-int32.mseed3", import.meta.url),
);
const files = process.argv.length > 2 ? process.argv.slice(2) : [defaultFile];

console.log(`libmseed ${version()}`);

for (const file of files) {
  const bytes = new Uint8Array(readFileSync(file));
  console.log(`\n== ${file} (${bytes.length} bytes)`);

  const records = parseRecords(bytes, false);
  for (const r of records) {
    console.log(r.summary());
    r.free();
  }

  const traces = new TraceList();
  traces.addBuffer(bytes, true);
  console.log();
  console.log(traces.summary());
  traces.free();
}

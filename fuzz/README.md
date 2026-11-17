# Fuzz targets

Fuzzing with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), which needs a nightly
toolchain:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run parse -- -max_total_time=300
```

Targets:

- `parse`: arbitrary bytes as one record, then decoding, repacking and printing it.
- `reader`: arbitrary bytes as a record stream, read directly and into a trace list.
- `time`: arbitrary strings as times; parsed times must survive a display round trip.
- `pack_roundtrip`: arbitrary samples packed with each integer encoding and format must
  read back unchanged.

Seed the corpus with the test data, e.g. `mkdir -p corpus/parse && cp ../tests/data/*.mseed* corpus/parse/`.

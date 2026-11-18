# Changelog

All notable changes are documented here. Format: [Keep a Changelog](https://keepachangelog.com/),
versioning: [SemVer](https://semver.org/).

## [Unreleased]

### Added
- Add a NO_RAW parse flag to skip retaining the raw record bytes

## [0.1.3] - 2026-11-17

### Fixed
- Convert the encoded data byte order when repacking records between formats
- Treat a zero miniSEED 2 data offset as no data payload when repacking
- Omit non-finite blockette values from extra headers instead of rejecting the miniSEED 2 record
- Keep trace segments sorted by start time after packed samples are trimmed

## [0.1.2] - 2026-11-17

### Added
- Initial port of libmseed 3.5.4 to safe Rust (`#![forbid(unsafe_code)]`).
- Reading of miniSEED 2 and 3 from files, streams and buffers (`MsReader`, `Record::parse`) with
  format, length and byte order auto-detection, CRC validation, byte ranges (`RangeBounds`) and
  selections.
- Decoding and encoding of text, int16/32, float32/64, Steim-1/2; decoding of GEOSCOPE, CDSN,
  SRO and DWWSSN.
- `TraceList` with merging, gap healing, tolerances, record lists and sample conversion,
  configured by `TraceListOptions` and read into with `ReadOptions`.
- Packing to miniSEED 3 or 2 (`Record::pack`, `RecordPacker`) configured by `PackOptions`,
  including v2 blockette mapping from extra headers, repacking (`Record::repack`), and
  rolling-buffer operation (`TraceList::pack_with`, `TraceListPacker`, idle flushing). Record
  handlers return `Result<()>` and packing stops at the first error.
- `NsTime`, a nanosecond time type with ISO 8601 `Display`/`FromStr`, `Duration` arithmetic
  and `SystemTime` conversions.
- JSON extra headers with JSON Pointer access, merge patch and typed helpers.
- Source identifier, selection and leap second facilities. Warnings are emitted through the
  `log` crate facade.
- Structured `Error` (via `thiserror`): I/O errors keep their `io::ErrorKind`, and
  `Error::other` wraps errors from caller code.
- `ParseFlags` and `RecordFlags` bit flags (via `bitflags`).
- Fuzz targets (`fuzz/`, cargo-fuzz) and a stable mutation test of record parsing.
- `libmseed-wasm` WebAssembly bindings, published to npm, and a standalone `libmseed.wasm`
  attached to GitHub releases.
- GitHub Actions for CI (develop/main) and for releases (crates.io, docs.rs, npm).

### Differences from libmseed
- Unset values are `Option`s instead of sentinels (`NSTUNSET`, `NSTERROR`, -1 counts, 0
  format versions); one `Flags` set is replaced by `ParseFlags` and option structs.
- Packing INT16 rejects samples outside the 16-bit range instead of truncating them.
- A requested integer encoding does not apply to float or text samples, which always use
  their fixed encoding, for records as for trace lists.
- `MsReader::next_record` can be called again after an error for a record of known length
  (bad CRC, undecodable data) to continue with the following record.
- `TraceList::add_record` counts the samples of a record holding samples with a sample
  count of 0, and rejects a record whose sample count disagrees with its samples.
- Time adjustments from miniSEED 2 headers that leave the representable range are errors;
  `NsTime::add_nanos` and `NsTime::nanos_since` saturate.
- No library-specific logging configuration or message registry; use a `log` backend.
- No URL reading support.
- `ms_parse_raw2/3` diagnostic functions are not ported.

### Known limitations
- miniSEED 2 blockettes 400, 405 and 2000 are discarded with a warning when reading.
- The embedded leap second list covers announcements through 2023; install a current list
  with `time::read_leap_second_file` for later data.

[Unreleased]: https://github.com/EarthScope/libmseed-rs/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/EarthScope/libmseed-rs/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/EarthScope/libmseed-rs/releases/tag/v0.1.2

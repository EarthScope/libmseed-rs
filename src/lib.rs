//! # libmseed
//!
//! A pure-Rust, safe port of [libmseed](https://github.com/EarthScope/libmseed): read,
//! write and manage [miniSEED](https://docs.fdsn.org/projects/miniSEED3) 2 and 3
//! seismological time series data.
//!
//! The crate forbids `unsafe` code and is a port in the Rust idiom: types, methods,
//! iterators, option structs and [`Result`]s rather than C-style pointers, flags and
//! return codes.
//!
//! ## Reading records
//!
//! [`MsReader`] reads records from files or any [`std::io::Read`], auto-detecting the format
//! version, record length and byte order, into a format agnostic [`Record`]:
//!
//! ```
//! use libmseed::{MsReader, ParseFlags};
//!
//! let reader = MsReader::open("tests/data/testdata-3channel-signal.mseed3", ParseFlags::UNPACK_DATA)?;
//! for record in reader.take(2) {
//!     let record = record?;
//!     println!("{record}");
//!     let samples = record.samples.as_ref().expect("unpacked");
//!     assert_eq!(samples.len() as u64, record.sample_count);
//! }
//! # Ok::<(), libmseed::Error>(())
//! ```
//!
//! A single record (or the first record of a buffer) can be parsed with [`Record::parse`];
//! it reports [`Error::Incomplete`] with a hint when more data is needed.
//!
//! Times are [`NsTime`] values, nanoseconds since the Unix epoch, which parse from and
//! display as ISO 8601 strings.
//!
//! ## Building trace lists
//!
//! A [`TraceList`] merges records into continuous segments, whatever the order they arrive
//! in, as set by its [`TraceListOptions`]:
//!
//! ```
//! use libmseed::{ParseFlags, ReadOptions, TraceList};
//!
//! let mut traces = TraceList::new();
//! traces.read_file("tests/data/testdata-oneseries-mixedlengths-mixedorder.mseed2",
//!                  &ReadOptions::new(ParseFlags::UNPACK_DATA))?;
//! let id = &traces.ids()[0];
//! assert_eq!(id.sid, "FDSN:XX_TEST_00_L_H_Z");
//! assert_eq!(id.num_segments(), 1);
//! assert_eq!(id.segments[0].num_samples(), 3952);
//! # Ok::<(), libmseed::Error>(())
//! ```
//!
//! With [`TraceListOptions::record_list`] a list of the contributing records (and where
//! they are) is kept without decoding data, so that selected segments can be decoded later
//! with [`tracelist::TraceSegment::unpack_record_list`].
//!
//! ## Writing records
//!
//! A [`Record`] holding [`Samples`] is packed into miniSEED 3 (default) or 2, as set by
//! [`PackOptions`]:
//!
//! ```
//! use libmseed::{Encoding, NsTime, PackOptions, ParseFlags, Record, Samples};
//!
//! let start: NsTime = "2024-01-01T00:00:00Z".parse()?;
//! let mut record = Record::new("FDSN:XX_TEST__B_H_Z", start);
//! record.sample_rate = 40.0;
//! record.encoding = Some(Encoding::STEIM2);
//! record.samples = Some(Samples::Int32((0..1000).collect()));
//!
//! let mut bytes = Vec::new();
//! let packed = record.pack_with(&PackOptions { flush: true, ..Default::default() }, |r| {
//!     bytes.extend_from_slice(r);
//!     Ok(())
//! })?;
//! assert_eq!(packed.samples, 1000);
//! assert!(packed.records >= 1);
//!
//! // Read it back
//! let parsed = Record::parse(&bytes, ParseFlags::UNPACK_DATA)?;
//! assert_eq!(parsed.sid, "FDSN:XX_TEST__B_H_Z");
//! # Ok::<(), libmseed::Error>(())
//! ```
//!
//! ## Rolling-buffer operation
//!
//! Add data to a [`TraceList`] as it arrives and call [`TraceList::pack_with`] (or use the
//! generator-style [`tracelist::TraceListPacker`]) periodically: full records are created
//! when they can be, the packed samples are removed from the list and the remainder waits
//! for more data. Set [`PackOptions::flush`] to pack everything, and
//! [`PackOptions::flush_idle`] to flush segments that have stopped receiving data.
//!
//! ```
//! use libmseed::{Encoding, PackOptions, Record, Samples, TraceList};
//!
//! let mut traces = TraceList::new();
//! let mut record = Record::new("FDSN:XX_TEST__B_H_Z", "2024-01-01T00:00:00Z".parse()?);
//! record.sample_rate = 100.0;
//! record.samples = Some(Samples::Int32((0..1000).collect()));
//! traces.add_record(&record)?;
//!
//! let options = PackOptions {
//!     record_length: Some(512),
//!     encoding: Some(Encoding::STEIM2),
//!     ..Default::default()
//! };
//! let mut bytes = 0;
//! let packed = traces.pack_with(&options, |record| {
//!     bytes += record.len();
//!     Ok(())
//! })?;
//! // Only full records were created, the rest remains in the list
//! assert!(packed.samples > 0 && packed.samples < 1000);
//! assert_eq!(traces.ids()[0].segments[0].num_samples(), 1000 - packed.samples);
//! # Ok::<(), libmseed::Error>(())
//! ```
//!
//! ## Extra headers
//!
//! miniSEED 3 carries JSON extra headers; [`Record::extra_headers`] parses them into
//! [`extra::ExtraHeaders`] with JSON Pointer access. miniSEED 2 blockettes are mapped to
//! the same extra headers when reading, and back to blockettes when writing miniSEED 2.
//!
//! ## Differences from the C library
//!
//! * Errors are returned as [`Error`]. Warnings, such as about unsupported blockettes or
//!   failed Steim integrity checks, are emitted through the [`log`](https://docs.rs/log)
//!   facade; install any `log` backend (e.g. `env_logger`) to see them. There is no
//!   library-specific logging configuration or message registry.
//! * Flags are split by purpose: [`ParseFlags`] for parsing, and option structs
//!   ([`PackOptions`], [`ReadOptions`], [`TraceListOptions`]) for packing and trace lists.
//! * Unset values are `Option`s rather than sentinel values such as `NSTUNSET` or -1.
//! * Times that cannot be represented are errors rather than `NSTERROR`.
//! * [`MsReader`] can continue past a record that fails to parse or decode: call
//!   [`MsReader::next_record`] again after the error.
//! * No URL reading: reading is from files and any [`std::io::Read`].
//! * Memory is managed by Rust: there are no user-definable allocators, and "free"
//!   functions are `Drop`.
//! * Data in a trace list is held as typed vectors ([`Samples`]) instead of void pointers
//!   and sample type characters.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod codec;
mod crc32c;
pub mod error;
pub mod extra;
pub mod flags;
pub mod format;
pub mod lookup;
pub mod options;
pub mod pack;
pub mod reader;
pub mod record;
pub mod selection;
pub mod sid;
pub mod time;
pub mod tracelist;
mod unpack;
pub mod writer;

pub use codec::{ByteOrder, Samples};
pub use error::{Error, Result};
pub use flags::{ParseFlags, RecordFlags};
pub use lookup::{Encoding, SampleType};
pub use options::{
    FormatVersion, PackOptions, ReadOptions, RecordList, Tolerance, TraceListOptions, Versions,
};
pub use pack::{Packed, RecordPacker};
pub use reader::MsReader;
pub use record::Record;
pub use selection::Selections;
pub use time::{NsTime, NSTMODULUS};
pub use tracelist::TraceList;

// Compile the README's Rust snippets as doctests
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// Version of libmseed this crate is ported from.
pub const LIBMSEED_VERSION: &str = "3.5.4";

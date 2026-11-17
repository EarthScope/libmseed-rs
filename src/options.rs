//! Options for packing records, reading into trace lists and building trace lists.

use crate::flags::ParseFlags;
use crate::lookup::Encoding;
use crate::record::Record;
use crate::selection::Selections;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// A miniSEED format major version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatVersion {
    /// miniSEED 2 (SEED 2.4 data records).
    V2,
    /// miniSEED 3.
    V3,
}

impl FormatVersion {
    /// The major version number, 2 or 3.
    #[must_use]
    pub fn major(self) -> u8 {
        match self {
            FormatVersion::V2 => 2,
            FormatVersion::V3 => 3,
        }
    }
}

/// Options for packing records, from a [`Record`] or a [`crate::TraceList`].
///
/// `None` values fall back to the record's own value (for trace lists, the segment's) and
/// then to the default.
///
/// ```
/// use libmseed::{Encoding, FormatVersion, PackOptions};
///
/// let opts = PackOptions {
///     format: Some(FormatVersion::V2),
///     record_length: Some(512),
///     encoding: Some(Encoding::STEIM1),
///     flush: true,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackOptions {
    /// Format version. Default: miniSEED 2 for a record parsed from miniSEED 2, otherwise
    /// miniSEED 3.
    pub format: Option<FormatVersion>,
    /// Maximum record length in bytes. Default: [`Record::reclen`], otherwise 4096.
    pub record_length: Option<usize>,
    /// Data encoding for integer samples; other sample types have a fixed encoding.
    /// Default: [`Record::encoding`], otherwise Steim-2.
    pub encoding: Option<Encoding>,
    /// Extra headers (JSON) for every record. Default: [`Record::extra`]; none for
    /// trace lists.
    pub extra_headers: Option<String>,
    /// Pack all samples, creating a final record that may not be filled. Otherwise only
    /// full records are created and the remaining samples are left (in a trace list,
    /// for later).
    pub flush: bool,
    /// \[Trace lists\] Also flush segments that have not been updated for this long.
    /// Segments need an update time, see [`TraceListOptions::update_time`].
    pub flush_idle: Option<Duration>,
    /// \[Trace lists\] Leave the trace list unchanged. Otherwise packed samples are
    /// removed from their segments and emptied segments and IDs are removed, which is
    /// rolling-buffer operation.
    pub keep_data: bool,
}

/// How publication versions of a source are kept in a [`crate::TraceList`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Versions {
    /// Merge all publication versions of a source into one ID.
    #[default]
    Merge,
    /// Keep each publication version in its own ID.
    Separate,
    /// Treat every record as having this publication version.
    Assign(u8),
}

/// Whether a [`crate::TraceList`] keeps a list of the records contributing to each segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RecordList {
    /// No record lists.
    #[default]
    Off,
    /// Record lists with each record's header values and extra headers.
    WithExtras,
    /// Record lists with each record's header values, without extra headers.
    WithoutExtras,
}

/// A callback returning a tolerance for a record being added to a trace list.
pub type ToleranceFn = Arc<dyn Fn(&Record) -> f64 + Send + Sync>;

/// Callbacks returning the time and sample rate tolerances used when merging records.
///
/// `time` returns a tolerance in seconds, `samprate` in Hertz. Each receives the record
/// being added. A return of 0.0 requires an exact match; a negative value is invalid and
/// the default is used. Without a callback the defaults are half a sample period and a
/// relative sample rate difference of 0.0001.
#[derive(Clone, Default)]
pub struct Tolerance {
    /// Time tolerance in seconds.
    pub time: Option<ToleranceFn>,
    /// Sample rate tolerance in Hz.
    pub samprate: Option<ToleranceFn>,
}

impl Tolerance {
    /// Tolerances with a custom sample rate callback and default time tolerance.
    pub fn with_samprate(f: impl Fn(&Record) -> f64 + Send + Sync + 'static) -> Self {
        Tolerance { time: None, samprate: Some(Arc::new(f)) }
    }

    /// Tolerances with a custom time callback and default sample rate tolerance.
    pub fn with_time(f: impl Fn(&Record) -> f64 + Send + Sync + 'static) -> Self {
        Tolerance { time: Some(Arc::new(f)), samprate: None }
    }
}

impl fmt::Debug for Tolerance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let show = |c: &Option<ToleranceFn>| if c.is_some() { "custom" } else { "default" };
        f.debug_struct("Tolerance")
            .field("time", &show(&self.time))
            .field("samprate", &show(&self.samprate))
            .finish()
    }
}

/// How a [`crate::TraceList`] merges the records added to it.
#[derive(Debug, Clone)]
pub struct TraceListOptions {
    /// How publication versions are kept. Default: merged.
    pub versions: Versions,
    /// Merge two segments when a record fills the gap between them. Default: true.
    pub autoheal: bool,
    /// Time and sample rate tolerances for merging. Default: half a sample period, and a
    /// relative sample rate difference of 0.0001.
    pub tolerance: Tolerance,
    /// Keep a list of contributing records per segment. Default: off.
    pub record_list: RecordList,
    /// Record the time of the last update of each segment, for idle flushing when
    /// packing (see [`PackOptions::flush_idle`]). Default: false.
    pub update_time: bool,
}

impl Default for TraceListOptions {
    fn default() -> Self {
        TraceListOptions {
            versions: Versions::Merge,
            autoheal: true,
            tolerance: Tolerance::default(),
            record_list: RecordList::Off,
            update_time: false,
        }
    }
}

/// Options for reading records into a [`crate::TraceList`].
///
/// ```
/// use libmseed::{ParseFlags, ReadOptions};
///
/// let opts = ReadOptions::new(ParseFlags::UNPACK_DATA | ParseFlags::VALIDATE_CRC);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    /// Parsing flags.
    pub flags: ParseFlags,
    /// Only add records matching these selections.
    pub selections: Option<Selections>,
    /// Skip a record identical to the record before it.
    pub skip_adjacent_duplicates: bool,
    /// Read only a byte range of a file given as a path suffix, `PATH@START-END` (either
    /// value optional), see [`crate::reader::split_path_range`].
    pub path_range: bool,
}

impl ReadOptions {
    /// Options with the given parse flags and no other options.
    #[must_use]
    pub fn new(flags: ParseFlags) -> Self {
        ReadOptions { flags, ..Default::default() }
    }
}

//! Trace lists: continuous time series reconstructed from records.
//!
//! A [`TraceList`] holds [`TraceId`] entries, ordered by source identifier and publication
//! version, each holding time-ordered [`TraceSegment`]s of continuous data. Records are
//! merged into segments as they are added, regardless of arrival order, within time and
//! sample rate tolerances, as configured by [`TraceListOptions`].
//!
//! A trace list also supports rolling-buffer operation: add records or samples as they
//! arrive and pack full records when they can be created ([`TraceList::pack_with`],
//! [`TraceListPacker`]), keeping the remainder for later.

use crate::codec::{Samples, decode};
use crate::crc32c::crc32c;
use crate::error::{Error, Result};
use crate::flags::ParseFlags;
use crate::lookup::{Encoding, SampleType};
use crate::options::{
    FormatVersion, PackOptions, ReadOptions, RecordList, TraceListOptions, Versions,
};
use crate::pack::{Packed, RecordPacker, resolve_encoding, short_of_record};
use crate::reader::MsReader;
use crate::record::{PACK_DEFAULT_ENCODING, PACK_DEFAULT_RECLEN, Record, format_g};
use crate::time::{NSTMODULUS, NsTime, Subseconds, TimeFormat};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Where a record of a [`RecordPtr`] can be found again.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordLocation {
    /// Location not recorded.
    Unknown,
    /// At a byte offset in a caller-supplied buffer.
    Buffer {
        /// Offset of the record in the buffer.
        offset: usize,
    },
    /// At a byte offset in a file.
    File {
        /// File path.
        path: PathBuf,
        /// Offset of the record in the file.
        offset: u64,
    },
}

/// A record that contributed to a [`TraceSegment`].
///
/// The record holds header values and, by default, extra headers, but no data samples
/// or raw bytes.
#[derive(Debug, Clone)]
pub struct RecordPtr {
    /// Header values of the record.
    pub record: Record,
    /// Time of the last sample of the record.
    pub end_time: NsTime,
    /// Offset from the start of the record to the encoded data, if known (determined
    /// when the record is added with its raw bytes).
    pub data_offset: Option<usize>,
    /// Where to find the record.
    pub location: RecordLocation,
}

/// A continuous segment of a trace.
#[derive(Debug, Clone)]
pub struct TraceSegment {
    /// Time of the first sample.
    pub start_time: NsTime,
    /// Time of the last sample.
    pub end_time: NsTime,
    /// Nominal sample rate in Hz, always as a rate (unlike [`Record::sample_rate`], which
    /// may be a negative sample period).
    pub sample_rate: f64,
    /// Number of samples covered, whether or not they are held in [`TraceSegment::samples`].
    pub sample_count: u64,
    /// Decoded data samples, if any.
    pub samples: Option<Samples>,
    /// Records that contributed to this segment, in the order they contributed, if built.
    pub record_list: Option<Vec<RecordPtr>>,
    /// Time of the last update, recorded when [`TraceListOptions::update_time`] is set.
    pub update_time: Option<NsTime>,
    uid: u64,
}

impl TraceSegment {
    /// Number of samples held in memory.
    pub fn num_samples(&self) -> usize {
        self.samples.as_ref().map_or(0, Samples::len)
    }

    /// Type of the samples held, if any.
    pub fn sample_type(&self) -> Option<SampleType> {
        self.samples.as_ref().map(Samples::sample_type)
    }

    fn has_time_coverage(&self) -> bool {
        self.sample_count > 0 && self.sample_rate != 0.0
    }

    /// Convert the samples to another numeric type.
    ///
    /// Converting to integers rounds to nearest and fails if a value cannot be represented
    /// in 32 bits, or, unless `truncate` is true, if precision would be lost. Nothing is
    /// modified on failure. Text cannot be converted.
    pub fn convert_samples(&mut self, to: SampleType, truncate: bool) -> Result<()> {
        let Some(samples) = &self.samples else {
            return Ok(());
        };
        if samples.sample_type() == to {
            return Ok(());
        }
        if to == SampleType::Text || samples.sample_type() == SampleType::Text {
            return Err(Error::Samples("Cannot convert text samples to/from numeric type".into()));
        }

        fn check(value: f64, truncate: bool, what: &str) -> Result<()> {
            let rounded = value + if value >= 0.0 { 0.5 } else { -0.5 };
            // Positive range test so NaN, which compares false, is rejected
            if !(rounded > i32::MIN as f64 - 1.0 && rounded < i32::MAX as f64 + 1.0) {
                return Err(Error::Samples(format!(
                    "Cannot convert {what} sample value to a 32-bit integer: {value}"
                )));
            }
            if !truncate && (value - (value as i32) as f64).abs() > 0.000001 {
                return Err(Error::Samples(format!(
                    "Loss of precision when converting {what}s to integers, loss: {}",
                    (value - (value as i32) as f64).abs()
                )));
            }
            Ok(())
        }
        let round = |v: f64| (v + if v >= 0.0 { 0.5 } else { -0.5 }) as i32;

        let converted = match (samples, to) {
            (Samples::Float32(v), SampleType::Int32) => {
                for &x in v {
                    check(x as f64, truncate, "float")?;
                }
                Samples::Int32(v.iter().map(|&x| round(x as f64)).collect())
            }
            (Samples::Float64(v), SampleType::Int32) => {
                for &x in v {
                    check(x, truncate, "double")?;
                }
                Samples::Int32(v.iter().map(|&x| round(x)).collect())
            }
            (Samples::Int32(v), SampleType::Float32) => {
                Samples::Float32(v.iter().map(|&x| x as f32).collect())
            }
            (Samples::Float64(v), SampleType::Float32) => {
                Samples::Float32(v.iter().map(|&x| x as f32).collect())
            }
            (Samples::Int32(v), SampleType::Float64) => {
                Samples::Float64(v.iter().map(|&x| x as f64).collect())
            }
            (Samples::Float32(v), SampleType::Float64) => {
                Samples::Float64(v.iter().map(|&x| x as f64).collect())
            }
            _ => return Err(Error::Samples("Unsupported sample conversion".into())),
        };
        self.samples = Some(converted);
        Ok(())
    }

    /// Decode samples from the record list without modifying the segment.
    ///
    /// Records located in a buffer are read from `buffer`, which must be the same buffer
    /// they were found in; records located in files are read from those files.
    pub fn decode_record_list(&self, buffer: Option<&[u8]>) -> Result<Samples> {
        let list =
            self.record_list.as_ref().filter(|l| !l.is_empty()).ok_or_else(|| {
                Error::InvalidArgument("Required record list is not present".into())
            })?;
        let first = &list[0].record;
        let sid = first.sid.as_str();
        let sampletype = first.encoding.and_then(Encoding::sample_type).ok_or_else(|| {
            Error::InvalidArgument(format!(
                "{sid}: Cannot determine sample size and type for encoding: {:?}",
                first.encoding
            ))
        })?;

        let mut out = Samples::empty(sampletype);
        let mut files: HashMap<PathBuf, File> = HashMap::new();
        let mut filebuf: Vec<u8> = Vec::new();

        for rp in list {
            let rec = &rp.record;
            if rec.sample_count == 0 {
                continue;
            }
            let enc = rec.encoding.unwrap_or(PACK_DEFAULT_ENCODING);
            let rtype = enc.sample_type().ok_or_else(|| {
                Error::UnsupportedEncoding(format!(
                    "{sid}: Cannot determine sample type for encoding: {}",
                    enc.0
                ))
            })?;
            if rtype != sampletype {
                return Err(Error::Samples(format!(
                    "{sid}: Mixed sample types cannot be decoded together: {} versus {}",
                    rtype.code(),
                    sampletype.code()
                )));
            }
            let reclen = rec.reclen.unwrap_or(0);

            let input: &[u8] = match &rp.location {
                RecordLocation::Buffer { offset } => {
                    let buf = buffer.ok_or_else(|| {
                        Error::InvalidArgument(format!(
                            "{sid}: Record is in a buffer, but none was supplied"
                        ))
                    })?;
                    buf.get(*offset..*offset + reclen).ok_or_else(|| {
                        Error::InvalidArgument(format!("{sid}: Record is outside the buffer"))
                    })?
                }
                RecordLocation::File { path, offset } => {
                    let f = match files.entry(path.clone()) {
                        Entry::Occupied(o) => o.into_mut(),
                        Entry::Vacant(v) => v.insert(File::open(path).map_err(|e| {
                            Error::io(e, format_args!("{sid}: Cannot open file {}", path.display()))
                        })?),
                    };
                    filebuf.resize(reclen, 0);
                    f.seek(SeekFrom::Start(*offset)).map_err(|e| {
                        Error::io(e, format_args!("{sid}: Cannot seek in file {}", path.display()))
                    })?;
                    f.read_exact(&mut filebuf).map_err(|e| {
                        Error::io(
                            e,
                            format_args!("{sid}: Cannot read record from file {}", path.display()),
                        )
                    })?;
                    &filebuf
                }
                RecordLocation::Unknown => {
                    return Err(Error::InvalidArgument(format!(
                        "{sid}: No buffer or file for record"
                    )));
                }
            };
            let data_offset = rp.data_offset.ok_or_else(|| {
                Error::InvalidArgument(format!("{sid}: Data offset of record is not known"))
            })?;
            if data_offset < crate::record::MINRECLEN || data_offset >= input.len() {
                return Err(Error::InvalidRecord(format!(
                    "{sid}: Data offset value is not valid: {data_offset}"
                )));
            }
            let decoded = decode(
                &input[data_offset..],
                enc,
                rec.sample_count as usize,
                rec.payload_order,
                sid,
            )?;
            out.extend_from(&decoded)?;
        }
        Ok(out)
    }

    /// Decode the samples of this segment from its record list into
    /// [`TraceSegment::samples`]; returns the sample count.
    ///
    /// Fails if samples are already held.
    pub fn unpack_record_list(&mut self, buffer: Option<&[u8]>) -> Result<usize> {
        if self.samples.is_some() {
            return Err(Error::InvalidArgument(
                "Segment data buffer is already allocated, cannot replace".into(),
            ));
        }
        let decoded = self.decode_record_list(buffer)?;
        let n = decoded.len();
        self.samples = Some(decoded);
        Ok(n)
    }
}

/// A source identifier and its segments.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TraceId {
    /// Source identifier.
    pub sid: String,
    /// Largest contributing publication version.
    pub pub_version: u8,
    /// Time of the earliest sample.
    pub earliest: NsTime,
    /// Time of the latest sample.
    pub latest: NsTime,
    /// Segments, ordered by start time (and by later end time first on ties).
    pub segments: Vec<TraceSegment>,
}

impl TraceId {
    /// Number of segments.
    #[must_use]
    pub fn num_segments(&self) -> usize {
        self.segments.len()
    }

    /// Move the segment at `idx` into place, ordered by start time then by descending end
    /// time, and return its new index.
    fn sort_segment(&mut self, mut idx: usize) -> usize {
        let out_of_order = |a: &TraceSegment, b: &TraceSegment| {
            a.start_time > b.start_time || (a.start_time == b.start_time && a.end_time < b.end_time)
        };
        while idx + 1 < self.segments.len()
            && out_of_order(&self.segments[idx], &self.segments[idx + 1])
        {
            self.segments.swap(idx, idx + 1);
            idx += 1;
        }
        while idx > 0 && out_of_order(&self.segments[idx - 1], &self.segments[idx]) {
            self.segments.swap(idx, idx - 1);
            idx -= 1;
        }
        idx
    }

    fn update_extent(&mut self) {
        let Some(first) = self.segments.first() else { return };
        self.earliest = first.start_time;
        self.latest = first.end_time;
        for s in &self.segments[1..] {
            self.earliest = self.earliest.min(s.start_time);
            self.latest = self.latest.max(s.end_time);
        }
    }
}

/// Position of a segment within a [`TraceList`]; valid until the list is next modified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentPos {
    /// Index of the [`TraceId`].
    pub id: usize,
    /// Index of the [`TraceSegment`] within the ID.
    pub segment: usize,
}

/// A container of continuous data series.
///
/// How records are merged is set by the list's [`TraceListOptions`].
#[derive(Debug, Clone, Default)]
pub struct TraceList {
    ids: Vec<TraceId>,
    next_uid: u64,
    options: TraceListOptions,
}

fn samprate_similar(a: f64, b: f64, tol: f64) -> bool {
    a == b || if tol >= 0.0 { (a - b).abs() <= tol } else { (b - a).abs() < 0.0001 * b.abs() }
}

impl TraceList {
    /// An empty trace list with default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty trace list that merges records as set by `options`.
    #[must_use]
    pub fn with_options(options: TraceListOptions) -> Self {
        TraceList { options, ..Self::default() }
    }

    /// The options records are merged with.
    #[must_use]
    pub fn options(&self) -> &TraceListOptions {
        &self.options
    }

    /// Trace IDs in order of source identifier then publication version.
    #[must_use]
    pub fn ids(&self) -> &[TraceId] {
        &self.ids
    }

    /// Mutable access to all segments, in order of ID then time.
    pub fn segments_mut(&mut self) -> impl Iterator<Item = &mut TraceSegment> {
        self.ids.iter_mut().flat_map(|id| id.segments.iter_mut())
    }

    /// Number of trace IDs.
    #[must_use]
    pub fn num_ids(&self) -> usize {
        self.ids.len()
    }

    /// True if there are no trace IDs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The segment at `pos`.
    #[must_use]
    pub fn segment(&self, pos: SegmentPos) -> Option<&TraceSegment> {
        self.ids.get(pos.id)?.segments.get(pos.segment)
    }

    /// Mutable access to the segment at `pos`.
    pub fn segment_mut(&mut self, pos: SegmentPos) -> Option<&mut TraceSegment> {
        self.ids.get_mut(pos.id)?.segments.get_mut(pos.segment)
    }

    /// Index of the ID matching `sid` and `pub_version`; `None` matches any version, the
    /// lowest.
    #[must_use]
    pub fn find_id(&self, sid: &str, pub_version: Option<u8>) -> Option<usize> {
        use std::cmp::Ordering;
        let idx = self.ids.partition_point(|id| match id.sid.as_str().cmp(sid) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => pub_version.is_some_and(|v| id.pub_version < v),
        });
        let id = self.ids.get(idx)?;
        (id.sid == sid && pub_version.is_none_or(|v| id.pub_version == v)).then_some(idx)
    }

    fn new_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    fn make_segment(&mut self, msr: &Record, endtime: NsTime) -> TraceSegment {
        let samples = match &msr.samples {
            Some(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        TraceSegment {
            start_time: msr.start_time,
            end_time: endtime,
            sample_rate: msr.sample_rate_hz(),
            sample_count: msr.sample_count,
            samples,
            record_list: None,
            update_time: None,
            uid: self.new_uid(),
        }
    }

    /// Add a record to the list, merging it into existing coverage where possible as set by
    /// the list's [`TraceListOptions`]. Returns the position of the segment it was added to.
    ///
    /// The record's samples, if present, are copied into the segment. A record holding
    /// samples with a [`Record::sample_count`] of 0 counts as holding `samples.len()`
    /// samples; any other mismatch is an [`Error::InvalidArgument`].
    pub fn add_record(&mut self, msr: &Record) -> Result<SegmentPos> {
        self.add_impl(msr, None)
    }

    /// Like [`TraceList::add_record`] and additionally appends a [`RecordPtr`] to the
    /// segment's record list, with the given `location`.
    ///
    /// The pointer's record omits extra headers if the list's record list option is
    /// [`RecordList::WithoutExtras`]. If the record retains its raw bytes the data offset
    /// is determined from them.
    pub fn add_record_located(
        &mut self,
        msr: &Record,
        location: RecordLocation,
    ) -> Result<SegmentPos> {
        self.add_impl(msr, Some(location))
    }

    fn make_recordptr(
        msr: &Record,
        endtime: NsTime,
        location: RecordLocation,
        extras: bool,
    ) -> RecordPtr {
        let data_offset = msr.data_bounds().ok().map(|(o, _)| o);
        let mut record = msr.header_copy();
        if !extras {
            record.extra.clear();
        }
        RecordPtr { record, end_time: endtime, data_offset, location }
    }

    #[allow(clippy::needless_range_loop)]
    fn add_impl(&mut self, msr: &Record, location: Option<RecordLocation>) -> Result<SegmentPos> {
        // Samples supplied without a sample count are counted; otherwise they must agree
        let counted;
        let msr = match msr.samples.as_ref().map(Samples::len) {
            Some(n) if n > 0 && msr.sample_count == 0 => {
                counted = Record {
                    sample_count: n as u64,
                    samples: msr.samples.clone(),
                    ..msr.header_copy()
                };
                &counted
            }
            Some(n) if n > 0 && msr.sample_count != n as u64 => {
                return Err(Error::InvalidArgument(format!(
                    "{}: Record holds {n} samples but its sample count is {}",
                    msr.sid, msr.sample_count
                )));
            }
            _ => msr,
        };
        let endtime = msr.end_time().map_err(|e| e.context("Error calculating record end time"))?;
        let (pubversion, find_version) = match self.options.versions {
            Versions::Merge => (msr.pub_version, None),
            Versions::Separate => (msr.pub_version, Some(msr.pub_version)),
            Versions::Assign(v) => (v, Some(v)),
        };
        let autoheal = self.options.autoheal;
        let update_time = self.options.update_time;
        let extras = self.options.record_list != RecordList::WithoutExtras;

        let recptr = |location: &Option<RecordLocation>| {
            location.clone().map(|l| Self::make_recordptr(msr, endtime, l, extras))
        };
        let push_recptr = |seg: &mut TraceSegment, rp: Option<RecordPtr>, whence: u8| {
            if let Some(rp) = rp {
                let list = seg.record_list.get_or_insert_with(Vec::new);
                if whence == 2 {
                    list.insert(0, rp);
                } else {
                    list.push(rp);
                }
            }
        };

        let found = self.find_id(&msr.sid, find_version);

        // No matching ID: create a new ID with one segment
        let Some(idx) = found else {
            let mut seg = self.make_segment(msr, endtime);
            push_recptr(&mut seg, recptr(&location), 0);
            if update_time {
                seg.update_time = Some(NsTime::now());
            }
            let id = TraceId {
                sid: msr.sid.clone(),
                pub_version: pubversion,
                earliest: msr.start_time,
                latest: endtime,
                segments: vec![seg],
            };
            let pos = self.ids.partition_point(|e| {
                (e.sid.as_str(), e.pub_version) < (id.sid.as_str(), id.pub_version)
            });
            self.ids.insert(pos, id);
            return Ok(SegmentPos { id: pos, segment: 0 });
        };

        // Add coverage to the matching ID
        let nsperiod = msr.period_nanos();
        let mut nstimetol = (0.5 * nsperiod as f64) as i64;
        let tolerance = &self.options.tolerance;
        if let Some(f) = &tolerance.time {
            let timetol = f(msr);
            if timetol < 0.0 {
                log::warn!(
                    "{}: Ignoring negative time tolerance ({timetol}), using default",
                    msr.sid
                );
            } else {
                nstimetol = (NSTMODULUS as f64 * timetol) as i64;
            }
        }
        let nnstimetol = if nstimetol != 0 { -nstimetol } else { 0 };
        let mut sampratetol = -1.0;
        if let Some(f) = &tolerance.samprate {
            let t = f(msr);
            if t < 0.0 {
                log::warn!(
                    "{}: Ignoring negative sample rate tolerance ({t}), using default",
                    msr.sid
                );
            } else {
                sampratetol = t;
            }
        }
        let sampratehz = msr.sample_rate_hz();

        // Take the new segment from a pre-built value so it can be inserted by index
        let new_seg = |this: &mut Self| this.make_segment(msr, endtime);

        let mut id = std::mem::replace(
            &mut self.ids[idx],
            TraceId {
                sid: String::new(),
                pub_version: 0,
                earliest: NsTime::UNIX_EPOCH,
                latest: NsTime::UNIX_EPOCH,
                segments: vec![],
            },
        );
        let result = (|| -> Result<usize> {
            let nseg = id.segments.len();
            let last = nseg - 1;
            // Gaps are negative when the record overlaps the segment and positive for a gap
            let lastgap =
                msr.start_time.nanos_since(id.segments[last].end_time).saturating_sub(nsperiod);
            let firstgap = id.segments[0].start_time.nanos_since(endtime).saturating_sub(nsperiod);

            let seg_idx: usize;

            if lastgap <= nstimetol
                && lastgap >= nnstimetol
                && samprate_similar(sampratehz, id.segments[last].sample_rate, sampratetol)
                && id.segments[last].has_time_coverage()
            {
                // Record fits at the end of the last segment
                add_msr_to_seg(&mut id.segments[last], msr, endtime, 1)?;
                id.latest = id.latest.max(endtime);
                push_recptr(&mut id.segments[last], recptr(&location), 1);
                seg_idx = last;
            } else if msr.start_time.add_nanos(nsperiod.saturating_add(nstimetol).saturating_neg())
                > id.latest
            {
                // Record is after all other coverage
                let mut seg = new_seg(self);
                push_recptr(&mut seg, recptr(&location), 0);
                id.segments.push(seg);
                id.latest = id.latest.max(endtime);
                seg_idx = id.segments.len() - 1;
            } else if endtime.add_nanos(nsperiod.saturating_add(nstimetol)) < id.earliest {
                // Record is before all other coverage
                let mut seg = new_seg(self);
                push_recptr(&mut seg, recptr(&location), 0);
                id.segments.insert(0, seg);
                id.earliest = id.earliest.min(msr.start_time);
                seg_idx = 0;
            } else if firstgap <= nstimetol
                && firstgap >= nnstimetol
                && samprate_similar(sampratehz, id.segments[0].sample_rate, sampratetol)
                && id.segments[0].has_time_coverage()
            {
                // Record fits at the beginning of the first segment
                add_msr_to_seg(&mut id.segments[0], msr, endtime, 2)?;
                id.earliest = id.earliest.min(msr.start_time);
                push_recptr(&mut id.segments[0], recptr(&location), 2);
                seg_idx = 0;
            } else {
                // Search the complete segment list
                let mut segbefore: Option<usize> = None;
                let mut segafter: Option<usize> = None;
                let mut followseg: Option<usize> = None;

                if sampratehz == 0.0 && sampratetol < 0.0 {
                    // A zero rate record cannot match before/after without an explicit
                    // tolerance, only followseg is needed, found searching backward
                    for i in (0..nseg).rev() {
                        let s = &id.segments[i];
                        if !s.has_time_coverage() {
                            continue;
                        }
                        if autoheal && msr.start_time == s.start_time && endtime == s.end_time {
                            followseg = Some(i);
                            break;
                        }
                        if s.start_time < msr.start_time {
                            followseg = Some(i);
                            break;
                        }
                    }
                } else {
                    for i in 0..nseg {
                        let s = &id.segments[i];
                        // Done when a segment starts beyond the record end plus tolerance
                        if s.start_time > endtime.add_nanos(nsperiod.saturating_add(nstimetol)) {
                            break;
                        }
                        if !s.has_time_coverage() {
                            continue;
                        }
                        if autoheal && msr.start_time == s.start_time && endtime == s.end_time {
                            followseg = Some(i);
                            break;
                        }
                        if msr.start_time > s.start_time {
                            followseg = Some(i);
                        }
                        if segbefore.is_none() {
                            let postgap =
                                msr.start_time.nanos_since(s.end_time).saturating_sub(nsperiod);
                            if postgap <= nstimetol
                                && postgap >= nnstimetol
                                && samprate_similar(sampratehz, s.sample_rate, sampratetol)
                            {
                                segbefore = Some(i);
                            }
                        }
                        if segafter.is_none() {
                            let pregap = s.start_time.nanos_since(endtime).saturating_sub(nsperiod);
                            if pregap <= nstimetol
                                && pregap >= nnstimetol
                                && samprate_similar(sampratehz, s.sample_rate, sampratetol)
                            {
                                segafter = Some(i);
                            }
                        }
                        // Done when both are found, or when not healing and one is found
                        if (segbefore.is_some() && segafter.is_some())
                            || (!autoheal && (segbefore.is_some() || segafter.is_some()))
                        {
                            break;
                        }
                    }
                }

                if let Some(mut b) = segbefore {
                    // Add the record to the end of the segment before
                    add_msr_to_seg(&mut id.segments[b], msr, endtime, 1)?;
                    push_recptr(&mut id.segments[b], recptr(&location), 1);
                    // Merge two segments that now fit when autohealing
                    if autoheal && let Some(a) = segafter.filter(|&a| a != b) {
                        let after = id.segments.remove(a);
                        if a < b {
                            b -= 1;
                        }
                        add_seg_to_seg(&mut id.segments[b], after)?;
                    }
                    seg_idx = b;
                } else if let Some(a) = segafter {
                    // Add the record to the beginning of the segment after
                    add_msr_to_seg(&mut id.segments[a], msr, endtime, 2)?;
                    push_recptr(&mut id.segments[a], recptr(&location), 2);
                    seg_idx = a;
                } else {
                    // New segment after followseg, or first in the list
                    let mut seg = new_seg(self);
                    push_recptr(&mut seg, recptr(&location), 0);
                    let at = followseg.map_or(0, |f| f + 1);
                    id.segments.insert(at, seg);
                    seg_idx = at;
                }
            }
            Ok(seg_idx)
        })();

        let seg_idx = match result {
            Ok(i) => i,
            Err(e) => {
                self.ids[idx] = id;
                return Err(e);
            }
        };

        // Track the largest publication version and the earliest and latest times
        if pubversion > id.pub_version {
            id.pub_version = pubversion;
        }
        id.earliest = id.earliest.min(msr.start_time);
        id.latest = id.latest.max(endtime);

        // The logic above limits this to few shifts
        let seg_idx = id.sort_segment(seg_idx);

        if update_time {
            id.segments[seg_idx].update_time = Some(NsTime::now());
        }

        self.ids[idx] = id;
        Ok(SegmentPos { id: idx, segment: seg_idx })
    }

    /// Add the records in a buffer; returns the number of records added.
    ///
    /// With a record list (see [`TraceListOptions::record_list`]) records are located by
    /// their offset in `buffer`. A truncated final record is ignored, and with
    /// [`ParseFlags::SKIP_NOT_DATA`] so is input that cannot be parsed as miniSEED.
    /// [`ReadOptions::path_range`] does not apply.
    ///
    /// On error, the records added before it remain in the list.
    pub fn read_buffer(&mut self, buffer: &[u8], options: &ReadOptions) -> Result<usize> {
        let flags = options.flags;
        // The buffer is the whole input
        let mut pflags = (flags | ParseFlags::END_OF_INPUT) - ParseFlags::NO_RAW;
        // Defer unpacking when selecting, to skip unneeded decoding
        if options.selections.is_some() {
            pflags.remove(ParseFlags::UNPACK_DATA);
        }
        let skip_not_data = flags.contains(ParseFlags::SKIP_NOT_DATA);
        let mut offset = 0usize;
        let mut count = 0usize;
        let mut duplicates = DuplicateFilter::new(options.skip_adjacent_duplicates);
        while buffer.len() - offset >= crate::record::MINRECLEN {
            let mut rec = match Record::parse(&buffer[offset..], pflags) {
                Ok(r) => r,
                // A truncated final record
                Err(Error::Incomplete { .. }) => break,
                Err(_) if skip_not_data => {
                    offset += 1;
                    continue;
                }
                Err(e) => {
                    return Err(
                        e.context(format_args!("Error parsing record at byte offset {offset}"))
                    );
                }
            };
            let reclen = rec.reclen.expect("parsed record has a length");
            let at = offset;
            offset += reclen;
            if let Some(sel) = &options.selections {
                if sel.matches_record(&rec).is_none() {
                    continue;
                }
                if flags.contains(ParseFlags::UNPACK_DATA) && rec.sample_count > 0 {
                    rec.unpack_data()?;
                }
            }
            if duplicates.is_duplicate(&rec) {
                continue;
            }
            self.add_read_record(&rec, || RecordLocation::Buffer { offset: at })?;
            count += 1;
        }
        Ok(count)
    }

    /// Read the records of a file into the list; returns the number of records added.
    ///
    /// With a record list (see [`TraceListOptions::record_list`]) records are located by
    /// file path and offset.
    ///
    /// On error, the records added before it remain in the list.
    pub fn read_file(&mut self, path: impl AsRef<Path>, options: &ReadOptions) -> Result<usize> {
        let path = path.as_ref();
        // Duplicate detection needs the raw record
        let flags = options.flags - ParseFlags::NO_RAW;
        let (mut reader, file_path) = match path.to_str() {
            // Record locations refer to the file without the byte range suffix
            Some(s) if options.path_range => (
                MsReader::open_path_range(s, flags)?,
                PathBuf::from(crate::reader::split_path_range(s).0),
            ),
            _ => (MsReader::open(path, flags)?, path.to_path_buf()),
        };
        if let Some(s) = &options.selections {
            reader = reader.with_selections(s.clone());
        }

        let mut duplicates = DuplicateFilter::new(options.skip_adjacent_duplicates);
        let mut count = 0;
        while let Some(rec) = reader.next_record()? {
            if duplicates.is_duplicate(&rec) {
                continue;
            }
            let offset = reader.last_record_offset();
            self.add_read_record(&rec, || RecordLocation::File {
                path: file_path.clone(),
                offset,
            })?;
            count += 1;
        }
        Ok(count)
    }

    /// Add a record read from input, with its location if keeping record lists.
    fn add_read_record(
        &mut self,
        rec: &Record,
        location: impl FnOnce() -> RecordLocation,
    ) -> Result<SegmentPos> {
        if self.options.record_list == RecordList::Off {
            self.add_record(rec)
        } else {
            self.add_record_located(rec, location())
        }
    }

    /// Remove a segment, and its ID if it was the last segment; returns the removed segment.
    pub fn remove_segment(&mut self, pos: SegmentPos) -> Option<TraceSegment> {
        let id = self.ids.get_mut(pos.id)?;
        if pos.segment >= id.segments.len() {
            return None;
        }
        let seg = id.segments.remove(pos.segment);
        if id.segments.is_empty() {
            self.ids.remove(pos.id);
        } else {
            id.update_extent();
        }
        Some(seg)
    }

    /// Release excess memory held by sample buffers.
    pub fn shrink_buffers(&mut self) {
        for id in &mut self.ids {
            for seg in &mut id.segments {
                match &mut seg.samples {
                    Some(Samples::Text(v)) => v.shrink_to_fit(),
                    Some(Samples::Int32(v)) => v.shrink_to_fit(),
                    Some(Samples::Float32(v)) => v.shrink_to_fit(),
                    Some(Samples::Float64(v)) => v.shrink_to_fit(),
                    None => {}
                }
            }
        }
    }
}

/// Detects a record identical to the one before it, by CRC of the raw record.
///
/// Trace-list reads parse without [`ParseFlags::NO_RAW`], so the raw record is present.
struct DuplicateFilter {
    enabled: bool,
    previous: Option<u32>,
}

impl DuplicateFilter {
    fn new(enabled: bool) -> Self {
        DuplicateFilter { enabled, previous: None }
    }

    fn is_duplicate(&mut self, rec: &Record) -> bool {
        if !self.enabled {
            return false;
        }
        let crc = crc32c(rec.raw().unwrap_or(&[]), 0);
        self.previous.replace(crc) == Some(crc)
    }
}

/// Add a record's coverage and samples to a segment at the end (`whence` 1) or beginning (2).
fn add_msr_to_seg(seg: &mut TraceSegment, msr: &Record, endtime: NsTime, whence: u8) -> Result<()> {
    if let Some(ms) = msr.samples.as_ref().filter(|s| !s.is_empty()) {
        match &mut seg.samples {
            None => seg.samples = Some(ms.clone()),
            Some(ss) => match whence {
                1 => ss.extend_from(ms)?,
                _ => ss.prepend(ms)?,
            },
        }
    }
    if whence == 1 {
        seg.end_time = endtime;
    } else {
        seg.start_time = msr.start_time;
    }
    seg.sample_count += msr.sample_count;
    Ok(())
}

/// Add the coverage, samples and record list of `seg2` to the end of `seg1`.
fn add_seg_to_seg(seg1: &mut TraceSegment, seg2: TraceSegment) -> Result<()> {
    if let Some(s2) = seg2.samples.as_ref().filter(|s| !s.is_empty()) {
        match &mut seg1.samples {
            None => seg1.samples = Some(s2.clone()),
            Some(s1) => s1.extend_from(s2)?,
        }
    }
    seg1.end_time = seg2.end_time;
    seg1.sample_count += seg2.sample_count;
    if let Some(mut list) = seg2.record_list {
        seg1.record_list.get_or_insert_with(Vec::new).append(&mut list);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Packing
// ---------------------------------------------------------------------------------------------

/// True if a segment does not yet hold enough samples to fill a record.
fn segment_short_of_record(sid: &str, seg: &TraceSegment, options: &PackOptions) -> bool {
    let version = match options.format {
        Some(FormatVersion::V2) => 2,
        _ => 3,
    };
    short_of_record(
        version,
        options.record_length.unwrap_or(PACK_DEFAULT_RECLEN),
        sid.len(),
        options.extra_headers.as_ref().map_or(0, String::len),
        resolve_encoding(seg.sample_type(), options.encoding),
        seg.sample_type().map_or(0, SampleType::size),
        seg.num_samples(),
    )
}

/// Start time after packing `packed` samples, preferring the apparent sample period.
fn packed_start_time(seg: &TraceSegment, packed: usize) -> NsTime {
    let n = seg.num_samples();
    if n >= 2 && seg.end_time > seg.start_time && seg.sample_rate != 0.0 {
        let apparent = seg.end_time.nanos_since(seg.start_time) as f64 / (n - 1) as f64;
        let nominal = if seg.sample_rate > 0.0 {
            NSTMODULUS as f64 / seg.sample_rate
        } else {
            NSTMODULUS as f64 * -seg.sample_rate
        };
        if (nominal - apparent).abs() < 0.0001 * nominal.abs() {
            return seg.start_time.add_nanos((packed as f64 * apparent + 0.5) as i64);
        }
    }
    // Within the segment's span, so representable; the end time is a safe fallback
    seg.start_time.sample_time(packed as u64, seg.sample_rate).unwrap_or(seg.end_time)
}

/// Remove `packed` samples from the front of a segment and advance its start time.
fn trim_segment(seg: &mut TraceSegment, packed: usize) {
    let n = seg.num_samples();
    seg.start_time = if packed == n { seg.end_time } else { packed_start_time(seg, packed) };
    seg.sample_count = seg.sample_count.saturating_sub(packed as u64);
    if let Some(s) = &mut seg.samples {
        s.drain_front(packed);
    }
}

/// Template record describing a segment, with the record length, encoding and extra
/// headers of `options` resolved; samples are moved in by the caller.
fn segment_template(id: &TraceId, seg: &TraceSegment, options: &PackOptions) -> Record {
    Record {
        pub_version: id.pub_version,
        reclen: options.record_length,
        encoding: Some(resolve_encoding(seg.sample_type(), options.encoding)),
        extra: options.extra_headers.clone().unwrap_or_default(),
        sample_rate: seg.sample_rate,
        sample_count: seg.sample_count,
        ..Record::new(id.sid.clone(), seg.start_time)
    }
}

/// Options for packing a segment template, whose record length, encoding and extra
/// headers are already resolved.
fn template_options(options: &PackOptions, flush: bool) -> PackOptions {
    PackOptions { format: options.format, flush, ..PackOptions::default() }
}

fn check_extra_length(options: &PackOptions) -> Result<()> {
    match &options.extra_headers {
        Some(e) if e.len() > u16::MAX as usize => {
            Err(Error::ExtraHeaders(format!("Extra headers are too long: {}", e.len())))
        }
        _ => Ok(()),
    }
}

/// Nanoseconds of an idle flush duration, `None` when not flushing idle segments.
fn idle_nanos(options: &PackOptions) -> Option<i64> {
    options.flush_idle.map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

impl TraceList {
    /// Pack segments into records, calling `handler` for each.
    ///
    /// Without [`PackOptions::flush`] a segment's final partial record is not created and
    /// its remaining samples stay in the list; segments idle for
    /// [`PackOptions::flush_idle`] are flushed regardless. Unless
    /// [`PackOptions::keep_data`] is set, packed samples are removed from segments and
    /// emptied segments and IDs are removed: rolling-buffer operation.
    ///
    /// Packing stops at the first error from packing or from `handler`; the samples of
    /// records already passed to `handler` are still removed.
    pub fn pack_with(
        &mut self,
        options: &PackOptions,
        mut handler: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<Packed> {
        check_extra_length(options)?;
        let idle_ns = idle_nanos(options);
        let now = NsTime::now();
        let mut total = Packed::default();

        let mut i = 0;
        while i < self.ids.len() {
            let mut j = 0;
            let mut id_gone = false;
            while j < self.ids[i].segments.len() {
                let id = &self.ids[i];
                let seg = &id.segments[j];
                // A segment idle for too long is flushed
                let idle = idle_ns
                    .is_some_and(|idle| seg.update_time.is_some_and(|u| now.nanos_since(u) > idle));
                let flush = options.flush || idle;
                // Without a flush, skip a segment that cannot yet fill a record
                if !flush && segment_short_of_record(&id.sid, seg, options) {
                    j += 1;
                    continue;
                }
                let pos = SegmentPos { id: i, segment: j };
                let (packed, moved) = self.pack_segment_impl(pos, options, flush, &mut handler)?;
                total += packed;
                // A trimmed segment sorted later leaves an unvisited segment at `j`
                if moved {
                    continue;
                }

                // Remove the segment if no samples remain and the list is not kept
                if !options.keep_data && self.ids[i].segments[j].num_samples() == 0 {
                    let was_last = self.ids[i].segments.len() == 1;
                    self.remove_segment(pos);
                    if was_last {
                        id_gone = true;
                        break;
                    }
                    continue;
                }
                j += 1;
            }
            if !id_gone {
                i += 1;
            }
        }
        Ok(total)
    }

    /// Pack one segment. Unless [`PackOptions::keep_data`] is set packed samples are
    /// removed from the segment, but an emptied segment is not removed from the list.
    ///
    /// Packing stops at the first error from packing or from `handler`. The samples of
    /// records already passed to `handler` are still removed, so they are not packed again.
    pub fn pack_segment(
        &mut self,
        pos: SegmentPos,
        options: &PackOptions,
        mut handler: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<Packed> {
        check_extra_length(options)?;
        self.pack_segment_impl(pos, options, options.flush, &mut handler).map(|(p, _)| p)
    }

    fn pack_segment_impl(
        &mut self,
        pos: SegmentPos,
        options: &PackOptions,
        flush: bool,
        handler: &mut impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<(Packed, bool)> {
        let id = self
            .ids
            .get_mut(pos.id)
            .filter(|id| pos.segment < id.segments.len())
            .ok_or_else(|| Error::InvalidArgument("Segment position is out of range".into()))?;
        if id.segments[pos.segment].num_samples() == 0 {
            return Ok((Packed::default(), false));
        }

        let mut template = segment_template(id, &id.segments[pos.segment], options);
        // Move the samples into the template for packing and restore afterward
        template.samples = id.segments[pos.segment].samples.take();
        let mut packed = Packed::default();
        let result =
            deliver_records(&template, &template_options(options, flush), handler, &mut packed);
        id.segments[pos.segment].samples = template.samples.take();

        // Whether a partially trimmed segment moved later in the list
        let mut moved = false;
        if !options.keep_data && packed.samples > 0 {
            trim_segment(&mut id.segments[pos.segment], packed.samples);
            if id.segments[pos.segment].num_samples() > 0 {
                moved = id.sort_segment(pos.segment) != pos.segment;
            }
            id.update_extent();
        }
        result.map(|()| (packed, moved))
    }

    /// Pack all data of the list and write it to a file, which is created or replaced.
    /// All samples are packed, as if [`PackOptions::flush`], and the list is not modified,
    /// as if [`PackOptions::keep_data`].
    pub fn write_file(&mut self, path: impl AsRef<Path>, options: &PackOptions) -> Result<Packed> {
        use std::io::Write;
        let path = path.as_ref();
        let mut out = crate::writer::create_output(path)?;
        let options = PackOptions { flush: true, keep_data: true, ..options.clone() };
        let packed = self.pack_with(&options, |rec| {
            out.write_all(rec).map_err(|e| {
                Error::io(e, format_args!("Error writing to output file {}", path.display()))
            })
        })?;
        crate::writer::finish_output(out, path)?;
        Ok(packed)
    }
}

/// Pack `template` and pass each record to `handler`, counting the records and samples
/// delivered so far even when an error stops packing.
fn deliver_records(
    template: &Record,
    options: &PackOptions,
    handler: &mut impl FnMut(&[u8]) -> Result<()>,
    packed: &mut Packed,
) -> Result<()> {
    let mut packer = RecordPacker::new(template, options)?;
    while let Some(rec) = packer.next_record()? {
        handler(&rec)?;
        packed.records += 1;
        packed.samples = packer.packed_samples();
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Generator style packing
// ---------------------------------------------------------------------------------------------

struct Session {
    sid: String,
    pub_version: u8,
    uid: u64,
    packer: RecordPacker<'static>,
}

/// Generates records from a [`TraceList`] one at a time, supporting rolling-buffer
/// operation where data is added between calls.
///
/// The packer does not borrow the list: pass it to each call, so records can be added in
/// between. Data should only be added between records, i.e. after
/// [`TraceListPacker::next_record`] returned `None`, not while a segment is mid-packing,
/// except that appending to the segment being packed is supported.
pub struct TraceListPacker {
    options: PackOptions,
    flush_idle_ns: Option<i64>,
    session: Option<Session>,
    /// Segment last completed when the list is kept, scanning resumes after it
    last: Option<(String, u8, u64)>,
    /// Source of the last completed segment, scanning resumes at or after it
    resume: Option<(String, u8)>,
    total_samples: usize,
    total_records: usize,
}

impl TraceListPacker {
    /// Create a packer with `options`, as for [`TraceList::pack_with`].
    pub fn new(options: PackOptions) -> Result<Self> {
        check_extra_length(&options)?;
        Ok(TraceListPacker {
            flush_idle_ns: idle_nanos(&options),
            options,
            session: None,
            last: None,
            resume: None,
            total_samples: 0,
            total_records: 0,
        })
    }

    /// Pack all remaining samples from now on, including final records that may not be
    /// filled, e.g. at the end of a data stream.
    pub fn set_flush(&mut self) {
        self.options.flush = true;
    }

    /// Counts of the records generated and the samples packed so far, including the
    /// active segment's.
    #[must_use]
    pub fn packed(&self) -> Packed {
        Packed {
            records: self.total_records,
            samples: self.total_samples
                + self.session.as_ref().map_or(0, |s| s.packer.packed_samples()),
        }
    }

    fn find_segment(list: &TraceList, sid: &str, pub_version: u8, uid: u64) -> Option<SegmentPos> {
        let ii = list.find_id(sid, Some(pub_version))?;
        let si = list.ids[ii].segments.iter().position(|s| s.uid == uid)?;
        Some(SegmentPos { id: ii, segment: si })
    }

    /// Generate the next record from `list`, `Ok(None)` when no segment can currently
    /// produce one.
    pub fn next_record(&mut self, list: &mut TraceList) -> Result<Option<Vec<u8>>> {
        let keep_data = self.options.keep_data;
        // Continue an active segment session
        if let Some(mut sess) = self.session.take() {
            let Some(pos) = Self::find_segment(list, &sess.sid, sess.pub_version, sess.uid) else {
                self.total_samples += sess.packer.packed_samples();
                return Err(Error::InvalidArgument(format!(
                    "{}: Segment being packed was merged or removed during active packing; \
                     add data only between records (after a return of None)",
                    sess.sid
                )));
            };
            let seg = &list.ids[pos.id].segments[pos.segment];
            if seg.start_time < sess.packer.template().start_time {
                self.total_samples += sess.packer.packed_samples();
                return Err(Error::InvalidArgument(format!(
                    "{}: Segment start time moved earlier during active packing; \
                     add data only between records (after a return of None)",
                    sess.sid
                )));
            }
            // Data may have been appended since the last call
            if seg.num_samples() != sess.packer.template().num_samples() {
                sess.packer.resync_samples(seg.samples.clone(), seg.sample_count);
            }
            if self.options.flush {
                sess.packer.set_flush();
            }
            match sess.packer.next_record() {
                Ok(Some(rec)) => {
                    self.total_records += 1;
                    self.session = Some(sess);
                    return Ok(Some(rec));
                }
                Err(e) => {
                    self.total_samples += sess.packer.packed_samples();
                    return Err(e);
                }
                Ok(None) => {
                    // Segment packing finished, update the segment
                    let packed = sess.packer.packed_samples();
                    self.total_samples += packed;
                    if keep_data {
                        self.last = Some((sess.sid.clone(), sess.pub_version, sess.uid));
                    } else if packed > 0 {
                        self.resume = Some((sess.sid.clone(), sess.pub_version));
                        let id = &mut list.ids[pos.id];
                        trim_segment(&mut id.segments[pos.segment], packed);
                        if id.segments[pos.segment].num_samples() > 0 {
                            id.sort_segment(pos.segment);
                            id.update_extent();
                        } else {
                            list.remove_segment(pos);
                        }
                    }
                }
            }
        }

        // Scan the list for a segment that can produce a record
        let now_cell = std::cell::OnceCell::new();
        if keep_data && self.last.is_some() {
            let (sid, ver, uid) = self.last.clone().expect("checked");
            let Some(pos) = Self::find_segment(list, &sid, ver, uid) else {
                return Err(Error::InvalidArgument(format!(
                    "{sid}: Last completed segment was merged or removed; \
                     add data only between records (after a return of None)"
                )));
            };
            let (start_id, start_seg) = if pos.segment + 1 < list.ids[pos.id].segments.len() {
                (pos.id, pos.segment + 1)
            } else {
                (pos.id + 1, 0)
            };
            if let Some(rec) = self.scan(list, &now_cell, start_id, list.ids.len(), start_seg)? {
                return Ok(Some(rec));
            }
        } else {
            // Resume at or after the SID of the segment session that completed last,
            // then wrap around to examine the remainder
            let head = 0;
            let start = match &self.resume {
                Some((sid, ver)) => list
                    .ids
                    .partition_point(|id| (id.sid.as_str(), id.pub_version) < (sid.as_str(), *ver)),
                None => head,
            };
            let start = if start >= list.ids.len() { head } else { start };
            if let Some(rec) = self.scan(list, &now_cell, start, list.ids.len(), 0)? {
                return Ok(Some(rec));
            }
            if start != head
                && let Some(rec) = self.scan(list, &now_cell, head, start, 0)?
            {
                return Ok(Some(rec));
            }
        }
        Ok(None)
    }

    fn scan(
        &mut self,
        list: &TraceList,
        now: &std::cell::OnceCell<NsTime>,
        start_id: usize,
        end_id: usize,
        first_seg: usize,
    ) -> Result<Option<Vec<u8>>> {
        let mut resume_seg = first_seg;
        for id in list.ids.iter().take(end_id.min(list.ids.len())).skip(start_id) {
            let from = std::mem::take(&mut resume_seg);
            for seg in id.segments.iter().skip(from) {
                if seg.num_samples() == 0 {
                    continue;
                }
                // The list is not trimmed when kept, a partial record cannot be continued
                // later, so pack all samples
                let idle = self.flush_idle_ns.is_some_and(|idle| {
                    seg.update_time
                        .is_some_and(|u| now.get_or_init(NsTime::now).nanos_since(u) > idle)
                });
                let flush = self.options.flush || self.options.keep_data || idle;
                if !flush && segment_short_of_record(&id.sid, seg, &self.options) {
                    continue;
                }

                let mut template = segment_template(id, seg, &self.options);
                template.samples = seg.samples.clone();
                let mut packer =
                    RecordPacker::new_owned(template, &template_options(&self.options, flush))
                        .map_err(|e| {
                            e.context(format_args!(
                                "{}: Cannot initialize segment packing state",
                                id.sid
                            ))
                        })?;
                match packer.next_record()? {
                    Some(rec) => {
                        self.total_records += 1;
                        self.session = Some(Session {
                            sid: id.sid.clone(),
                            pub_version: id.pub_version,
                            uid: seg.uid,
                            packer,
                        });
                        return Ok(Some(rec));
                    }
                    None => continue,
                }
            }
        }
        Ok(None)
    }
}

// ---------------------------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------------------------

fn gap_string(gap: f64) -> String {
    if gap >= 86400.0 || gap <= -86400.0 {
        format!("{:<3.1}d", gap / 86400.0)
    } else if gap >= 3600.0 || gap <= -3600.0 {
        format!("{:<3.1}h", gap / 3600.0)
    } else if gap == 0.0 {
        "-0  ".to_string()
    } else {
        format!("{:<4}", format_g(gap, 4))
    }
}

/// What a trace listing ([`TraceList::format_trace_list`]) includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceListing {
    /// Format of segment start and end times. Default: ISO 8601 with a `Z`.
    pub time_format: TimeFormat,
    /// Include the sample rate and count of each segment, and totals.
    pub details: bool,
    /// Include the gap before each segment.
    pub gaps: bool,
    /// Include publication versions with source identifiers.
    pub versions: bool,
}

impl Default for TraceListing {
    fn default() -> Self {
        TraceListing {
            time_format: TimeFormat::IsoMonthDayZ,
            details: false,
            gaps: false,
            versions: false,
        }
    }
}

impl TraceList {
    /// A listing of traces and segments, one segment per line.
    #[must_use]
    pub fn format_trace_list(&self, listing: &TraceListing) -> String {
        let TraceListing { time_format: timeformat, details, gaps, versions } = *listing;
        let mut out = String::new();
        out += match (details, gaps) {
            (true, true) => {
                "       SourceID                      Start sample                End sample           Gap  Hz  Samples\n"
            }
            (false, true) => {
                "       SourceID                      Start sample                End sample           Gap\n"
            }
            (true, false) => {
                "       SourceID                      Start sample                End sample           Hz  Samples\n"
            }
            (false, false) => {
                "       SourceID                      Start sample                End sample\n"
            }
        };
        let (mut tracecnt, mut segcnt) = (0, 0);
        for id in &self.ids {
            let display =
                if versions { format!("{}#{}", id.sid, id.pub_version) } else { id.sid.clone() };
            for (k, seg) in id.segments.iter().enumerate() {
                let stime = seg.start_time.format(timeformat, Subseconds::NanoMicro);
                let etime = seg.end_time.format(timeformat, Subseconds::NanoMicro);
                if gaps {
                    let mut gap = 0.0;
                    let nogap = k == 0;
                    if !nogap {
                        gap = seg.start_time.nanos_since(id.segments[k - 1].end_time) as f64
                            / NSTMODULUS as f64;
                    }
                    if gap < 0.0 {
                        let delta =
                            if seg.sample_rate != 0.0 { 1.0 / seg.sample_rate } else { 0.0 };
                        let cover = seg.end_time.nanos_since(seg.start_time) as f64
                            / NSTMODULUS as f64
                            + delta;
                        if -gap > cover {
                            gap = -cover;
                        }
                    }
                    let gapstr = if nogap { " == ".to_string() } else { gap_string(gap) };
                    if !details {
                        out += &format!("{display:<27} {stime:<28} {etime:<28} {gapstr:<4}\n");
                    } else {
                        out += &format!(
                            "{display:<27} {stime:<28} {etime:<28} {gapstr} {:<3} {}\n",
                            format_g(seg.sample_rate, 3),
                            seg.sample_count
                        );
                    }
                } else if details {
                    out += &format!(
                        "{display:<27} {stime:<28} {etime:<28} {:<3} {}\n",
                        format_g(seg.sample_rate, 3),
                        seg.sample_count
                    );
                } else {
                    out += &format!("{display:<27} {stime:<28} {etime:<28}\n");
                }
                segcnt += 1;
            }
            tracecnt += 1;
        }
        if details {
            out += &format!("Total: {tracecnt} trace(s) with {segcnt} segment(s)\n");
        }
        out
    }

    /// A SYNC format listing. `date` is the `YYYY,DDD` stamp to
    /// use, by default the current date.
    #[must_use]
    pub fn format_sync_list(
        &self,
        dccid: Option<&str>,
        subseconds: Subseconds,
        date: Option<&str>,
    ) -> String {
        let yearday = match date {
            Some(d) => d.to_string(),
            None => {
                let t = NsTime::now().parts();
                format!("{:04},{:03}", t.year, t.yday)
            }
        };
        let mut out = format!("{}|{}\n", dccid.unwrap_or("DCC"), yearday);
        for id in &self.ids {
            let nslc = crate::sid::sid_to_nslc(&id.sid).unwrap_or_default();
            for seg in &id.segments {
                let st = seg.start_time.format(TimeFormat::SeedOrdinal, subseconds);
                let et = seg.end_time.format(TimeFormat::SeedOrdinal, subseconds);
                out += &format!(
                    "{}|{}|{}|{}|{}|{}||{}|{}|||||||{}\n",
                    nslc.network,
                    nslc.station,
                    nslc.location,
                    nslc.channel,
                    st,
                    et,
                    format_g(seg.sample_rate, 10),
                    seg.sample_count,
                    yearday
                );
            }
        }
        out
    }

    /// A listing of gaps and overlaps between segments, restricted to gaps within
    /// `mingap` and `maxgap` seconds if given.
    #[must_use]
    pub fn format_gap_list(
        &self,
        timeformat: TimeFormat,
        mingap: Option<f64>,
        maxgap: Option<f64>,
    ) -> String {
        let mut out = String::from(
            "       SourceID                      Last Sample                 Next Sample          Gap  Samples\n",
        );
        let mut gapcnt = 0;
        for id in &self.ids {
            for w in id.segments.windows(2) {
                let (seg, next) = (&w[0], &w[1]);
                if !seg.has_time_coverage() {
                    continue;
                }
                let mut gap = next.start_time.nanos_since(seg.end_time) as f64 / NSTMODULUS as f64;
                if gap < 0.0 {
                    let delta = if next.sample_rate != 0.0 { 1.0 / next.sample_rate } else { 0.0 };
                    let cover = next.end_time.nanos_since(next.start_time) as f64
                        / NSTMODULUS as f64
                        + delta;
                    if -gap > cover {
                        gap = -cover;
                    }
                }
                if mingap.is_some_and(|m| gap < m) || maxgap.is_some_and(|m| gap > m) {
                    continue;
                }
                let mut nsamples = gap.abs() * seg.sample_rate;
                if gap > 0.0 {
                    nsamples -= 1.0;
                } else {
                    nsamples += 1.0;
                }
                let t1 = seg.end_time.format(timeformat, Subseconds::NanoMicro);
                let t2 = next.start_time.format(timeformat, Subseconds::NanoMicro);
                out += &format!(
                    "{:<27} {t1:<28} {t2:<28} {:<4} {}\n",
                    id.sid,
                    gap_string(gap),
                    format_g(nsamples, 8)
                );
                gapcnt += 1;
            }
        }
        out += &format!("Total: {gapcnt} gap(s)\n");
        out
    }
}

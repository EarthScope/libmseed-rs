//! The miniSEED record container.

use crate::codec::{ByteOrder, Samples};
use crate::error::{Error, Result};
use crate::extra::{ExtraHeaders, format_extra};
use crate::flags::RecordFlags;
use crate::lookup::{Encoding, SampleType};
use crate::options::FormatVersion;
use crate::time::{NSTMODULUS, NsTime, Subseconds, TimeFormat};
use std::fmt;

/// Minimum miniSEED record length supported.
pub const MINRECLEN: usize = 40;
/// Maximum miniSEED record length supported (10 MiB).
pub const MAXRECLEN: usize = 10_485_760;
/// Maximum miniSEED 2 record length supported.
pub const MAXRECLEN_V2: usize = 131_172;
/// Default record length used when packing and none is specified.
pub const PACK_DEFAULT_RECLEN: usize = 4096;
/// Default encoding used when packing and none is specified.
pub const PACK_DEFAULT_ENCODING: Encoding = Encoding::STEIM2;

/// A miniSEED record in a format agnostic container.
///
/// Records are created by parsing ([`Record::parse`]) or by populating a new record
/// to be packed ([`Record::new`]).
///
/// The header fields are public for building records to pack. Changing them on a parsed
/// record does not change how its retained raw bytes are interpreted.
#[derive(Debug, Clone)]
pub struct Record {
    /// Source identifier, usually an FDSN Source ID URN.
    pub sid: String,
    /// Format version of a parsed record, `None` for a record not from parsing.
    pub format_version: Option<FormatVersion>,
    /// Record-level flags.
    pub flags: RecordFlags,
    /// Record start time (first sample).
    pub start_time: NsTime,
    /// Nominal sample rate in Hz when positive, or sample period in seconds when negative.
    pub sample_rate: f64,
    /// Data encoding, `None` if unset.
    pub encoding: Option<Encoding>,
    /// Publication version.
    pub pub_version: u8,
    /// Number of samples in the record according to the header.
    ///
    /// Packing uses the length of [`Record::samples`] instead. When adding a record holding
    /// samples to a [`crate::TraceList`], 0 means `samples.len()`.
    pub sample_count: u64,
    /// CRC of the entire record (miniSEED 3).
    pub crc: u32,
    /// Extra headers as a JSON string, empty if none.
    pub extra: String,
    /// Length of the data payload in bytes according to the header.
    pub data_length: u32,
    /// Length of the record in bytes, `None` if unset.
    pub reclen: Option<usize>,
    /// Data samples, if unpacked or supplied for packing.
    pub samples: Option<Samples>,
    pub(crate) raw: Option<Vec<u8>>,
    pub(crate) header_order: ByteOrder,
    pub(crate) payload_order: ByteOrder,
}

impl Record {
    /// A new record with the given source identifier and start time, and otherwise empty:
    /// set at least the sample rate and samples (and optionally the encoding and record
    /// length) to pack it.
    pub fn new(sid: impl Into<String>, start_time: NsTime) -> Self {
        Record {
            sid: sid.into(),
            format_version: None,
            flags: RecordFlags::empty(),
            start_time,
            sample_rate: 0.0,
            encoding: None,
            pub_version: 0,
            sample_count: 0,
            crc: 0,
            extra: String::new(),
            data_length: 0,
            reclen: None,
            samples: None,
            raw: None,
            header_order: ByteOrder::Little,
            payload_order: ByteOrder::Little,
        }
    }

    /// A copy of the header values and extra headers, without samples or raw bytes.
    #[must_use]
    pub fn header_copy(&self) -> Record {
        Record {
            sid: self.sid.clone(),
            extra: self.extra.clone(),
            samples: None,
            raw: None,
            ..*self
        }
    }

    /// The raw record bytes, available for parsed records.
    #[must_use]
    pub fn raw(&self) -> Option<&[u8]> {
        self.raw.as_deref()
    }

    /// Discard the retained raw record bytes. Unpacking data afterwards is not possible.
    pub fn clear_raw(&mut self) {
        self.raw = None;
    }

    /// Byte order of the record header as parsed.
    #[must_use]
    pub fn header_byte_order(&self) -> ByteOrder {
        self.header_order
    }

    /// Byte order of the data payload as parsed.
    #[must_use]
    pub fn payload_byte_order(&self) -> ByteOrder {
        self.payload_order
    }

    /// Number of unpacked samples held.
    pub fn num_samples(&self) -> usize {
        self.samples.as_ref().map_or(0, Samples::len)
    }

    /// Type of the held samples.
    pub fn sample_type(&self) -> Option<SampleType> {
        self.samples.as_ref().map(Samples::sample_type)
    }

    /// Sample rate in samples/second regardless of how it is stored.
    #[must_use]
    pub fn sample_rate_hz(&self) -> f64 {
        if self.sample_rate < 0.0 { -1.0 / self.sample_rate } else { self.sample_rate }
    }

    /// Sample period in nanoseconds, rounded; 0 if the sample rate is 0.
    #[must_use]
    pub fn period_nanos(&self) -> i64 {
        if self.sample_rate > 0.0 {
            (NSTMODULUS as f64 / self.sample_rate + 0.5) as i64
        } else if self.sample_rate < 0.0 {
            (NSTMODULUS as f64 * -self.sample_rate + 0.5) as i64
        } else {
            0
        }
    }

    /// Time of the last sample, adjusted for leap seconds in the
    /// global leap second list.
    pub fn end_time(&self) -> Result<NsTime> {
        self.start_time.sample_time(self.sample_count.saturating_sub(1), self.sample_rate)
    }

    /// Data latency in seconds relative to the host clock, based on the last sample.
    #[must_use]
    pub fn host_latency(&self) -> f64 {
        let rate = self.sample_rate_hz();
        let span = if rate > 0.0 && self.sample_count > 0 {
            (1.0 / rate) * (self.sample_count - 1) as f64
        } else {
            0.0
        };
        NsTime::now().as_secs_f64() - self.start_time.as_secs_f64() - span
    }

    /// Parsed extra headers.
    pub fn extra_headers(&self) -> Result<ExtraHeaders> {
        ExtraHeaders::from_json(&self.extra)
    }

    /// Replace the extra headers with serialized `headers`.
    pub fn set_extra_headers(&mut self, headers: &ExtraHeaders) -> Result<()> {
        self.extra = headers.to_json()?;
        Ok(())
    }

    /// Replace the extra headers with a JSON string, re-serialized compactly; an empty
    /// string removes them. Returns the new length.
    pub fn replace_extra(&mut self, json: &str) -> Result<usize> {
        let eh = ExtraHeaders::from_json(json)?;
        self.extra = eh.to_json()?;
        Ok(self.extra.len())
    }

    /// Whether the value at JSON Pointer `ptr` exists in the extra headers.
    #[must_use]
    pub fn extra_exists(&self, ptr: &str) -> bool {
        self.extra_headers().map(|e| e.exists(ptr)).unwrap_or(false)
    }

    /// Set an extra header value with a single parse and serialize.
    ///
    /// For several modifications prefer [`Record::extra_headers`] and
    /// [`Record::set_extra_headers`].
    pub fn set_extra(&mut self, ptr: &str, value: serde_json::Value) -> Result<()> {
        let mut eh = self.extra_headers()?;
        eh.set_value(ptr, value)?;
        self.set_extra_headers(&eh)
    }

    /// Apply a JSON Merge Patch to the extra headers at `ptr`.
    pub fn merge_extra(&mut self, ptr: &str, patch_json: &str) -> Result<()> {
        let mut eh = self.extra_headers()?;
        eh.merge_patch(ptr, patch_json)?;
        self.set_extra_headers(&eh)
    }

    /// Determine the data payload bounds as (offset in record, size in bytes).
    /// Requires the raw record.
    pub fn data_bounds(&self) -> Result<(usize, usize)> {
        let raw = self.raw.as_deref().ok_or_else(|| {
            Error::InvalidArgument(format!("{}: Raw record is not available", self.sid))
        })?;
        let reclen = self.reclen.unwrap_or(raw.len());

        let (offset, mut size) = match self.format_version {
            // Lengths from the raw header: the `extra` field may have been edited or
            // lossily decoded and no longer match the bytes in the record
            Some(FormatVersion::V3) => (
                crate::format::MS3FSDH_LENGTH
                    + raw[33] as usize
                    + ByteOrder::Little.u16(&raw[34..36]) as usize,
                ByteOrder::Little.u32(&raw[36..40]) as usize,
            ),
            Some(FormatVersion::V2) => {
                let offset = self.header_order.u16(&raw[44..46]) as usize;
                // A zero data offset means no data payload
                if offset == 0 {
                    return Ok((0, 0));
                }
                if offset >= reclen {
                    return Err(Error::InvalidRecord(format!(
                        "{}: Data offset ({offset}) is beyond record length ({reclen})",
                        self.sid
                    )));
                }
                (offset, reclen - offset)
            }
            None => {
                return Err(Error::InvalidRecord(format!("{}: Unknown format version", self.sid)));
            }
        };

        // Fixed sample length encodings: limit to the bytes the sample count occupies
        let samplebytes = match self.encoding {
            Some(Encoding::TEXT) => 1,
            Some(Encoding::INT16) => 2,
            Some(Encoding::INT32) | Some(Encoding::FLOAT32) => 4,
            Some(Encoding::FLOAT64) => 8,
            _ => 0,
        };
        if samplebytes > 0 && self.sample_count <= (size / samplebytes) as u64 {
            size = self.sample_count as usize * samplebytes;
        }

        // Trailing, zeroed Steim frames are padding
        if size % 64 == 0
            && matches!(self.encoding, Some(Encoding::STEIM1) | Some(Encoding::STEIM2))
        {
            while size > 0
                && offset + size <= raw.len()
                && raw[offset + size - 64..offset + size].iter().all(|&b| b == 0)
            {
                size -= 64;
            }
        }
        Ok((offset, size))
    }

    /// Unpack data samples from the retained raw record into [`Record::samples`].
    /// Returns the number of samples.
    pub fn unpack_data(&mut self) -> Result<usize> {
        if self.sample_count == 0 {
            return Ok(0);
        }
        let Some(reclen) = self.reclen else {
            return Err(Error::InvalidArgument(format!("{}: Record size unknown", self.sid)));
        };
        if !(MINRECLEN..=MAXRECLEN).contains(&reclen) {
            return Err(Error::OutOfRange(format!(
                "{}: Unsupported record length: {reclen}",
                self.sid
            )));
        }
        if self.sample_count > i32::MAX as u64 {
            return Err(Error::InvalidRecord(format!(
                "{}: Too many samples to unpack: {}",
                self.sid, self.sample_count
            )));
        }
        let (offset, size) = self.data_bounds()?;
        if offset < MINRECLEN || offset >= reclen {
            return Err(Error::InvalidRecord(format!(
                "{}: Data offset value is not valid: {offset}",
                self.sid
            )));
        }
        // Legacy bare SEED data records have no encoding, assume Steim-1
        let encoding = self.encoding.unwrap_or(Encoding::STEIM1);
        let raw = self.raw.as_deref().expect("data_bounds checked raw");
        let end = (offset + size).min(raw.len());
        let samples = crate::codec::decode(
            &raw[offset..end],
            encoding,
            self.sample_count as usize,
            self.payload_order,
            &self.sid,
        )?;
        let n = samples.len();
        self.samples = Some(samples);
        Ok(n)
    }
}

/// A one-line summary, `SID, version, reclen, N samples, R Hz, start time`, or with the
/// alternate flag (`{:#}`) all header details including flags and extra headers, one per
/// line.
impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time = self.start_time.format(TimeFormat::IsoMonthDayDoyZ, Subseconds::NanoMicro);
        let reclen = self.reclen.map_or(-1, |r| r as i64);
        if !f.alternate() {
            return write!(
                f,
                "{}, {}, {reclen}, {} samples, {} Hz, {time}",
                self.sid,
                self.pub_version,
                self.sample_count,
                fmt_g(self.sample_rate_hz()),
            );
        }
        writeln!(
            f,
            "{}, version {}, {reclen} bytes (format: {})",
            self.sid,
            self.pub_version,
            self.format_version.map_or(0, FormatVersion::major)
        )?;
        writeln!(f, "             start time: {time}")?;
        writeln!(f, "      number of samples: {}", self.sample_count)?;
        writeln!(f, "       sample rate (Hz): {}", fmt_g(self.sample_rate_hz()))?;
        let b = self.flags.bits();
        writeln!(f, "                  flags: [{b:08b}] 8 bits")?;
        for (bit, text) in [
            (RecordFlags::CALIBRATION, "Calibration signals present"),
            (RecordFlags::TIME_QUESTIONABLE, "Time tag is questionable"),
            (RecordFlags::CLOCK_LOCKED, "Clock locked"),
        ] {
            if self.flags.contains(bit) {
                writeln!(
                    f,
                    "                         [Bit {}] {text}",
                    bit.bits().trailing_zeros()
                )?;
            }
        }
        for i in 3..8 {
            if b & (1 << i) != 0 {
                writeln!(f, "                         [Bit {i}] Undefined bit set")?;
            }
        }
        writeln!(f, "                    CRC: 0x{:X}", self.crc)?;
        writeln!(f, "    extra header length: {} bytes", self.extra.len())?;
        writeln!(f, "    data payload length: {} bytes", self.data_length)?;
        match self.encoding {
            Some(e) => writeln!(f, "       payload encoding: {} (val: {})", e.description(), e.0)?,
            None => writeln!(f, "       payload encoding: Unknown (val: -1)")?,
        }
        if !self.extra.is_empty() {
            writeln!(f, "          extra headers:")?;
            f.write_str(&format_extra(&self.extra, 16))?;
        }
        Ok(())
    }
}

/// Format a float like C's `%g` (6 significant digits, trailing zeros removed).
pub(crate) fn fmt_g(v: f64) -> String {
    format_g(v, 6)
}

/// Format a float like C's `%.{prec}g`, for output that matches libmseed's programs.
pub(crate) fn format_g(v: f64, prec: usize) -> String {
    let prec = prec.max(1);
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".into()
        } else if v > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    // Scientific formatting gives the correctly rounded exponent for `prec` significant digits
    let sci = format!("{:.*e}", prec - 1, v);
    let (mantissa, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= prec as i32 {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim_zeros(mantissa), exp.abs())
    } else {
        let decimals = (prec as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", decimals, v))
    }
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

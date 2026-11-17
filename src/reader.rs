//! Reading miniSEED records from files and streams.
//!
//! [`MsReader`] reads record by record from any [`std::io::Read`], auto-detecting miniSEED
//! 2 and 3 and the record length. Non-record bytes can be skipped, data can be filtered
//! with [`Selections`], and a byte range of the input can be read.

use crate::error::{Error, Result};
use crate::flags::ParseFlags;
use crate::record::{MAXRECLEN, MINRECLEN, Record};
use crate::selection::Selections;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::{Bound, RangeBounds};
use std::path::Path;

/// Bytes to skip when input at the read position cannot be identified as miniSEED
/// and [`ParseFlags::SKIP_NOT_DATA`] is set.
const SKIPLEN: usize = 1;

/// Minimum number of bytes requested per read.
const READ_CHUNK: usize = 64 * 1024;

/// A reader of miniSEED records from a byte stream.
///
/// Use [`MsReader::next_record`], or iterate: `Item` is `Result<Record>`, and the
/// iterator ends after the last record or after the first error. To continue past
/// records that fail to parse or decode, call [`MsReader::next_record`] again after an
/// error, or set [`ParseFlags::SKIP_NOT_DATA`].
///
/// Flags honored: [`ParseFlags::UNPACK_DATA`], [`ParseFlags::SKIP_NOT_DATA`], [`ParseFlags::VALIDATE_CRC`].
pub struct MsReader<R> {
    input: R,
    flags: ParseFlags,
    selections: Option<Selections>,
    buf: Vec<u8>,
    read_offset: usize,
    eof: bool,
    done: bool,
    /// An error occurred that reading cannot continue after.
    fatal: bool,
    parsed_record: bool,
    /// Last byte of the input to read (inclusive), `None` for the end of input.
    end_offset: Option<u64>,
    stream_pos: u64,
    pending_skip: u64,
    record_count: u64,
    last_offset: u64,
}

impl<R: Read> MsReader<R> {
    /// Read from any byte stream.
    pub fn new(input: R, flags: ParseFlags) -> Self {
        MsReader {
            input,
            flags,
            selections: None,
            buf: Vec::new(),
            read_offset: 0,
            eof: false,
            done: false,
            fatal: false,
            parsed_record: false,
            end_offset: None,
            stream_pos: 0,
            pending_skip: 0,
            record_count: 0,
            last_offset: 0,
        }
    }

    /// Only return records matching `selections`; other records are skipped. When set,
    /// data samples are unpacked only for matching records.
    #[must_use]
    pub fn with_selections(mut self, selections: Selections) -> Self {
        self.selections = Some(selections);
        self
    }

    /// Restrict reading to a byte range of the input, e.g. `9428..9968` or `1024..`.
    ///
    /// Records must start within the range. For a generic stream the bytes before the
    /// start are read and discarded; [`MsReader::open_range`] seeks instead. A range
    /// starting beyond the end of the input reads no records and gives
    /// [`Error::NotSeed`], as for any input without miniSEED. A range shorter than the
    /// minimum record length, such as `..0`, reads no records.
    #[must_use]
    pub fn with_byte_range(mut self, range: impl RangeBounds<u64>) -> Self {
        let (start, end) = byte_range(range);
        self.end_offset = end;
        self.stream_pos = start;
        self.pending_skip = start;
        self
    }

    /// Position in the input of the next unread byte.
    pub fn position(&self) -> u64 {
        self.stream_pos
    }

    /// Position in the input of the start of the most recently returned record.
    pub fn last_record_offset(&self) -> u64 {
        self.last_offset
    }

    /// Number of records returned so far.
    pub fn record_count(&self) -> u64 {
        self.record_count
    }

    fn buflen(&self) -> usize {
        self.buf.len() - self.read_offset
    }

    /// Read the next record; `Ok(None)` at the end of the input.
    ///
    /// Returns [`Error::NotSeed`] if the input ends without any miniSEED data having
    /// been found.
    ///
    /// After an error for a record whose length is known, such as an invalid CRC or
    /// undecodable data, the record is skipped and the next call continues with the
    /// following record. After an error that reading cannot continue from (data that is
    /// not miniSEED, or an unsupported record length) the next call returns `Ok(None)`.
    /// I/O errors are returned as they occur and reading may be retried.
    pub fn next_record(&mut self) -> Result<Option<Record>> {
        if self.fatal {
            return Ok(None);
        }
        let r = self.read_record();
        if r.is_err() {
            self.done = true;
        }
        r
    }

    /// Skip the record at the read position if its length can be determined, after an
    /// error parsing it; otherwise reading cannot continue.
    fn skip_failed_record(&mut self) {
        let len = crate::format::detect(&self.buf[self.read_offset..])
            .and_then(|d| d.reclen)
            .filter(|&l| (MINRECLEN..=MAXRECLEN).contains(&l) && l <= self.buflen());
        match len {
            Some(len) => {
                self.parsed_record = true;
                self.read_offset += len;
                self.stream_pos += len as u64;
            }
            None => self.fatal = true,
        }
    }

    fn discard_pending(&mut self) -> Result<()> {
        if self.pending_skip > 0 {
            let n =
                std::io::copy(&mut (&mut self.input).take(self.pending_skip), &mut std::io::sink())
                    .map_err(|e| {
                        Error::io(
                            e,
                            format_args!("Error skipping input to offset {}", self.stream_pos),
                        )
                    })?;
            if n < self.pending_skip {
                self.eof = true;
            }
            self.pending_skip = 0;
        }
        Ok(())
    }

    fn read_record(&mut self) -> Result<Option<Record>> {
        self.discard_pending()?;
        let mut pflags = self.flags;
        // Defer data unpacking when selecting, to avoid decoding skipped records
        if self.selections.is_some() {
            pflags.remove(ParseFlags::UNPACK_DATA);
        }
        let mut need_more = false;

        loop {
            // Finished when within MINRECLEN from a known end offset
            if self.end_offset.is_some_and(|end| {
                (end as i128 - self.stream_pos as i128) < (MINRECLEN as i128 - 1)
            }) {
                return Ok(None);
            }

            // Read more data if not at EOF and the buffer holds too little for a record
            if !self.eof && (self.buflen() < MINRECLEN || need_more) {
                if self.buflen() == 0 {
                    self.buf.clear();
                    self.read_offset = 0;
                } else if self.read_offset > 0 {
                    self.buf.drain(..self.read_offset);
                    self.read_offset = 0;
                }

                let mut readsize = MAXRECLEN - self.buf.len();
                if let Some(end) = self.end_offset {
                    let inrange = end as i128 - (self.stream_pos as i128 + self.buf.len() as i128);
                    if inrange < readsize as i128 - 1 {
                        readsize = if inrange >= 0 { (inrange + 1) as usize } else { 0 };
                    }
                }
                let want = readsize.min(READ_CHUNK);
                if want > 0 {
                    let old = self.buf.len();
                    self.buf.resize(old + want, 0);
                    let n = loop {
                        match self.input.read(&mut self.buf[old..]) {
                            Ok(n) => break n,
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) => {
                                self.buf.truncate(old);
                                return Err(Error::io(
                                    e,
                                    format_args!(
                                        "Error reading input at offset {}",
                                        self.stream_pos
                                    ),
                                ));
                            }
                        }
                    };
                    self.buf.truncate(old + n);
                    if n == 0 {
                        self.eof = true;
                    }
                }
            }

            // At the end of a known byte range once buffered data reaches the end offset
            let atrangeend =
                self.end_offset.is_some_and(|end| self.stream_pos + self.buflen() as u64 > end);

            if self.buflen() >= MINRECLEN {
                if self.eof || atrangeend {
                    pflags |= ParseFlags::END_OF_INPUT;
                }
                match Record::parse(&self.buf[self.read_offset..], pflags) {
                    Ok(mut rec) => {
                        self.parsed_record = true;
                        need_more = false;
                        let reclen = rec.reclen.expect("parsed record has length");

                        if let Some(sel) = &self.selections
                            && sel.matches_record(&rec).is_none()
                        {
                            self.read_offset += reclen;
                            self.stream_pos += reclen as u64;
                            if (self.eof || atrangeend) && self.buflen() < MINRECLEN {
                                return Ok(None);
                            }
                            continue;
                        }
                        if self.selections.is_some()
                            && self.flags.contains(ParseFlags::UNPACK_DATA)
                            && rec.sample_count > 0
                            && let Err(e) = rec.unpack_data()
                        {
                            let e = e.context(format_args!(
                                "Cannot unpack data samples for record at byte offset {}",
                                self.stream_pos
                            ));
                            self.parsed_record = true;
                            self.read_offset += reclen;
                            self.stream_pos += reclen as u64;
                            return Err(e);
                        }
                        self.last_offset = self.stream_pos;
                        self.read_offset += reclen;
                        self.stream_pos += reclen as u64;
                        self.record_count += 1;
                        return Ok(Some(rec));
                    }
                    Err(Error::Incomplete { needed: hint }) => {
                        need_more = true;
                        if self.buflen() + hint > MAXRECLEN {
                            if self.flags.contains(ParseFlags::SKIP_NOT_DATA) {
                                self.read_offset += SKIPLEN;
                                self.stream_pos += SKIPLEN as u64;
                            } else {
                                self.fatal = true;
                                return Err(Error::OutOfRange(format!(
                                    "miniSEED record length out of supported range (at byte offset {})",
                                    self.stream_pos
                                )));
                            }
                        } else if self.eof || atrangeend {
                            // Truncated record at the end of the input
                            return Ok(None);
                        }
                    }
                    Err(e) => {
                        if self.flags.contains(ParseFlags::SKIP_NOT_DATA) {
                            self.read_offset += SKIPLEN;
                            self.stream_pos += SKIPLEN as u64;
                        } else {
                            let at = self.stream_pos;
                            return Err(match e {
                                Error::NotSeed(_) => {
                                    self.fatal = true;
                                    Error::NotSeed(format!(
                                        "No miniSEED data detected (starting at byte offset {at})"
                                    ))
                                }
                                Error::OutOfRange(_) => {
                                    self.fatal = true;
                                    Error::OutOfRange(format!(
                                        "miniSEED record length out of supported range (at byte offset {at}): {e}"
                                    ))
                                }
                                other => {
                                    self.skip_failed_record();
                                    other.context(format_args!("Record at byte offset {at}"))
                                }
                            });
                        }
                    }
                }
            }

            // Finished at end-of-stream or end offset with less than MINRECLEN buffered
            if (self.eof || atrangeend) && self.buflen() < MINRECLEN {
                return if self.parsed_record {
                    Ok(None)
                } else {
                    self.fatal = true;
                    Err(Error::NotSeed("No data records read, not miniSEED?".into()))
                };
            }
        }
    }
}

impl<R: Read> Iterator for MsReader<R> {
    type Item = Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_record() {
            Ok(Some(r)) => Some(Ok(r)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl MsReader<File> {
    /// Open a file for reading.
    pub fn open(path: impl AsRef<Path>, flags: ParseFlags) -> Result<Self> {
        Self::open_range(path, flags, ..)
    }

    /// Open a file named with an optional byte range suffix, `PATH@START-END` (either value
    /// optional, e.g. `@START`, `@-END`), reading only that range; see [`split_path_range`].
    pub fn open_path_range(path: &str, flags: ParseFlags) -> Result<Self> {
        let (p, start, end) = split_path_range(path);
        let end = end.filter(|&e| e != 0).map_or(Bound::Unbounded, Bound::Included);
        Self::open_range(p, flags, (Bound::Included(start.unwrap_or(0)), end))
    }

    /// Open a file and read only a byte range of it, e.g. `9428..9968` or `1024..`.
    /// Records must start within the range.
    pub fn open_range(
        path: impl AsRef<Path>,
        flags: ParseFlags,
        range: impl RangeBounds<u64>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let (start, end) = byte_range(range);
        let mut file = File::open(path)
            .map_err(|e| Error::io(e, format_args!("Cannot open {}", path.display())))?;
        if start > 0 {
            file.seek(SeekFrom::Start(start)).map_err(|e| {
                Error::io(e, format_args!("Cannot seek {} to {start}", path.display()))
            })?;
        }
        let mut r = MsReader::new(file, flags);
        r.end_offset = end;
        r.stream_pos = start;
        Ok(r)
    }
}

/// Start offset and inclusive end offset of a byte range.
fn byte_range(range: impl RangeBounds<u64>) -> (u64, Option<u64>) {
    let start = match range.start_bound() {
        Bound::Included(&s) => s,
        Bound::Excluded(&s) => s.saturating_add(1),
        Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        Bound::Included(&e) => Some(e),
        Bound::Excluded(&e) => Some(e.saturating_sub(1)),
        Bound::Unbounded => None,
    };
    (start, end)
}

/// Split a trailing byte range, `PATH@START-END`, from a path.
///
/// START and END are optional digit strings of at most 20 digits that must fit in a
/// signed 64-bit value. If the suffix is not a valid range the path is returned whole.
#[must_use]
pub fn split_path_range(path: &str) -> (&str, Option<u64>, Option<u64>) {
    let Some(at) = path.rfind('@') else { return (path, None, None) };
    let spec = &path[at + 1..];
    let mut start_digits = String::new();
    let mut end_digits = String::new();
    let mut seen_dash = false;
    for c in spec.chars() {
        match c {
            '0'..='9' if !seen_dash => start_digits.push(c),
            '0'..='9' => end_digits.push(c),
            '-' if !seen_dash => seen_dash = true,
            _ => return (path, None, None),
        }
        if start_digits.len() >= 21 || end_digits.len() >= 21 {
            return (path, None, None);
        }
    }
    if start_digits.is_empty() && end_digits.is_empty() {
        return (path, None, None);
    }
    let conv = |s: &str| -> Option<Option<u64>> {
        if s.is_empty() {
            return Some(None);
        }
        s.parse::<u64>().ok().filter(|&v| v <= i64::MAX as u64).map(Some)
    };
    match (conv(&start_digits), conv(&end_digits)) {
        (Some(s), Some(e)) => (&path[..at], s, e),
        _ => (path, None, None),
    }
}

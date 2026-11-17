//! Low-level miniSEED format definitions and record detection.
//!
//! [`detect`] identifies a record and its length at the start of a buffer without parsing
//! it, for example to split a stream into records. To parse records use
//! [`crate::Record::parse`] or [`crate::MsReader`].

use crate::codec::ByteOrder;
use crate::options::FormatVersion;
use crate::record::MINRECLEN;

pub use crate::unpack::blockette_description;

/// Length of the fixed section of a miniSEED 3 header.
pub const MS3FSDH_LENGTH: usize = 40;
/// Length of the fixed section of a miniSEED 2 header.
pub const MS2FSDH_LENGTH: usize = 48;

/// True if the buffer (at least 15 bytes) starts with a miniSEED 3 signature.
#[must_use]
pub fn is_valid_header3(b: &[u8]) -> bool {
    b.len() >= 15
        && b[0] == b'M'
        && b[1] == b'S'
        && b[2] == 3
        && b[12] <= 23
        && b[13] <= 59
        && b[14] <= 60
}

/// True if the buffer (at least 27 bytes) starts with a miniSEED 2 signature.
#[must_use]
pub fn is_valid_header2(b: &[u8]) -> bool {
    b.len() >= 27
        && b[..6].iter().all(|&c| c.is_ascii_digit() || c == b' ' || c == 0)
        && matches!(b[6], b'D' | b'R' | b'Q' | b'M')
        && (b[7] == b' ' || b[7] == 0)
        && b[24] <= 23
        && b[25] <= 59
        && b[26] <= 60
}

/// Year and day-of-year in a plausible range.
pub(crate) fn is_valid_year_day(year: u16, day: u16) -> bool {
    (1900..=2100).contains(&year) && (1..=366).contains(&day)
}

/// Byte order of a miniSEED 2 fixed header, inferred from the year and day fields.
pub(crate) fn v2_header_order(b: &[u8]) -> ByteOrder {
    let be = (ByteOrder::Big.u16(&b[20..22]), ByteOrder::Big.u16(&b[22..24]));
    if is_valid_year_day(be.0, be.1) {
        return ByteOrder::Big;
    }
    let le = (ByteOrder::Little.u16(&b[20..22]), ByteOrder::Little.u16(&b[22..24]));
    if is_valid_year_day(le.0, le.1) {
        ByteOrder::Little
    } else {
        ByteOrder::Big
    }
}

/// Result of record detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Detection {
    /// Format version detected.
    pub format_version: FormatVersion,
    /// Record length in bytes, `None` if the record was detected but its length could not
    /// be determined from the buffer. The length is as given by the record and is not
    /// checked against [`MINRECLEN`] and [`crate::record::MAXRECLEN`].
    pub reclen: Option<usize>,
}

/// Detect a miniSEED record at the start of a buffer.
///
/// For miniSEED 2, blockette 1000 supplies the length; failing that, the buffer is
/// searched at 64-byte offsets for the next header. Returns `None` if no record is
/// detected (or the buffer is shorter than [`MINRECLEN`]).
///
/// Only the signature and length fields are checked: a detected record may still fail to
/// parse.
///
/// ```
/// use libmseed::format::detect;
/// use libmseed::FormatVersion;
///
/// let bytes = std::fs::read("tests/data/testdata-3channel-signal.mseed3")?;
/// let det = detect(&bytes).expect("miniSEED");
/// assert_eq!(det.format_version, FormatVersion::V3);
/// let reclen = det.reclen.expect("length known");
/// assert_eq!(libmseed::Record::parse(&bytes, Default::default())?.reclen, Some(reclen));
///
/// assert!(detect(&[0u8; 64]).is_none());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn detect(buf: &[u8]) -> Option<Detection> {
    if buf.len() < MINRECLEN {
        return None;
    }
    if is_valid_header3(buf) {
        let sidlength = buf[33] as usize;
        let extralength = u16::from_le_bytes([buf[34], buf[35]]) as usize;
        let datalength = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]) as usize;
        return Some(Detection {
            format_version: FormatVersion::V3,
            reclen: Some(MS3FSDH_LENGTH + sidlength + extralength + datalength),
        });
    }
    if !is_valid_header2(buf) {
        return None;
    }

    let order = v2_header_order(buf);
    let mut reclen = None;
    let mut blkt_offset =
        if buf.len() >= MS2FSDH_LENGTH { order.u16(&buf[46..48]) as usize } else { 0 };

    while blkt_offset != 0 && blkt_offset >= MS2FSDH_LENGTH && blkt_offset + 4 <= buf.len() {
        let blkt_type = order.u16(&buf[blkt_offset..]);
        let next_blkt = order.u16(&buf[blkt_offset + 2..]) as usize;

        // Found a 1000 blockette, not truncated
        if blkt_type == 1000 && blkt_offset + 8 <= buf.len() {
            let exp = buf[blkt_offset + 6];
            if exp < 31 {
                reclen = Some(1usize << exp);
                break;
            }
        }
        // Safety check for invalid offset
        if next_blkt != 0 && (next_blkt < 4 || next_blkt - 4 <= blkt_offset) {
            return None;
        }
        blkt_offset = next_blkt;
    }

    // No blockette 1000, search for the next record header at 64-byte offsets
    if reclen.is_none() {
        let mut off = 64;
        while off + 48 < buf.len() {
            if is_valid_header2(&buf[off..]) {
                reclen = Some(off);
                break;
            }
            off += 64;
        }
    }
    Some(Detection { format_version: FormatVersion::V2, reclen })
}

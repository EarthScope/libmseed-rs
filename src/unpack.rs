//! Parsing of miniSEED 2 and 3 records into [`Record`].

use crate::codec::ByteOrder;
use crate::crc32c::crc32c;
use crate::error::{Error, Result};
use crate::extra::{Calibration, EventDetection, ExtraHeaders, TimingException};
use crate::flags::{ParseFlags, RecordFlags};
use crate::format::{
    MS2FSDH_LENGTH, MS3FSDH_LENGTH, detect, is_valid_header2, is_valid_header3, v2_header_order,
};
use crate::lookup::Encoding;
use crate::options::FormatVersion;
use crate::record::{MAXRECLEN, MINRECLEN, Record};
use crate::sid::nslc_to_sid;
use crate::time::{NSTMODULUS, NsTime};

impl Record {
    /// Parse a miniSEED 2 or 3 record from the start of a buffer.
    ///
    /// The format and record length are detected automatically. On success the record's
    /// [`Record::reclen`] is the number of bytes consumed from `buf`.
    ///
    /// Returns [`Error::Incomplete`] with a hint of the additional bytes needed if the
    /// buffer holds only part of a record, and [`Error::NotSeed`] if no record is detected.
    ///
    /// Flags: [`ParseFlags::UNPACK_DATA`], [`ParseFlags::VALIDATE_CRC`] and [`ParseFlags::END_OF_INPUT`].
    pub fn parse(buf: &[u8], flags: ParseFlags) -> Result<Record> {
        let Some(det) = detect(buf) else {
            return Err(Error::NotSeed("no miniSEED record detected".into()));
        };
        let mut reclen = det.reclen;

        // A version 2 record without a determinable length at the end of input
        // takes the length of the remaining buffer if it is a power of two
        if det.format_version == FormatVersion::V2
            && reclen.is_none()
            && flags.contains(ParseFlags::END_OF_INPUT)
            && buf.len().is_power_of_two()
            && buf.len() <= MAXRECLEN
        {
            reclen = Some(buf.len());
        }

        let Some(reclen) = reclen else {
            return Err(Error::Incomplete { needed: MINRECLEN });
        };
        if !(MINRECLEN..=MAXRECLEN).contains(&reclen) {
            return Err(Error::OutOfRange(format!(
                "Record length of {reclen} is out of range allowed: {MINRECLEN} to {MAXRECLEN}"
            )));
        }
        if reclen > buf.len() {
            return Err(Error::Incomplete { needed: (reclen - buf.len()).min(MAXRECLEN) });
        }

        let rec = &buf[..reclen];
        match det.format_version {
            FormatVersion::V3 => Record::unpack_v3(rec, flags),
            FormatVersion::V2 => Record::unpack_v2(rec, flags),
        }
    }

    /// Unpack a complete miniSEED 3 record.
    pub(crate) fn unpack_v3(rec: &[u8], flags: ParseFlags) -> Result<Record> {
        let reclen = rec.len();
        if !(MINRECLEN..=MAXRECLEN).contains(&reclen) {
            return Err(Error::OutOfRange(format!(
                "Record length is out of allowed range: {reclen}"
            )));
        }
        if !is_valid_header3(rec) {
            return Err(Error::NotSeed(
                "Record header unrecognized, not a valid miniSEED record".into(),
            ));
        }
        let le = ByteOrder::Little;
        let sidlength = rec[33] as usize;
        let extralength = le.u16(&rec[34..36]) as usize;
        let datalength = le.u32(&rec[36..40]);
        let header_crc = le.u32(&rec[28..32]);

        if sidlength >= crate::sid::SIDLEN {
            return Err(Error::InvalidRecord(format!(
                "Source identifier is longer ({sidlength}) than supported ({})",
                crate::sid::SIDLEN - 1
            )));
        }
        let sid_end = (MS3FSDH_LENGTH + sidlength).min(reclen);
        let sid = String::from_utf8_lossy(&rec[MS3FSDH_LENGTH.min(reclen)..sid_end]).into_owned();

        if flags.contains(ParseFlags::VALIDATE_CRC) {
            let mut crc = crc32c(&rec[..28], 0);
            crc = crc32c(&[0u8; 4], crc);
            crc = crc32c(&rec[32..], crc);
            if crc != header_crc {
                return Err(Error::InvalidCrc(format!(
                    "{sid}: CRC is invalid, miniSEED record may be corrupt, header: 0x{header_crc:X} calculated: 0x{crc:X}"
                )));
            }
        }

        let headerlength = (MS3FSDH_LENGTH + sidlength + extralength) as u64;
        if headerlength > reclen as u64 || headerlength + datalength as u64 > reclen as u64 {
            return Err(Error::OutOfRange(format!(
                "{sid}: Record length ({reclen}) shorter than header lengths (header {headerlength}, total {})",
                headerlength + datalength as u64
            )));
        }

        let start_time = NsTime::from_ordinal(
            le.u16(&rec[8..10]) as i32,
            le.u16(&rec[10..12]) as i32,
            rec[12] as i32,
            rec[13] as i32,
            rec[14] as i32,
            le.u32(&rec[4..8]),
        )
        .map_err(|_| {
            Error::InvalidRecord(format!(
                "{sid}: Cannot convert start time to internal time representation"
            ))
        })?;

        let encoding = Encoding(rec[15]);
        let sample_rate = f64::from_bits(le.u64(&rec[16..24]));
        if sample_rate != 0.0 && !sample_rate.is_normal() {
            return Err(Error::InvalidRecord(format!("{sid}: Invalid sample rate: {sample_rate}")));
        }

        let extra_start = MS3FSDH_LENGTH + sidlength;
        let extra =
            String::from_utf8_lossy(&rec[extra_start..extra_start + extralength]).into_owned();

        // Steim payloads are big endian, all others match the little endian header
        let payload_order = if encoding == Encoding::STEIM1 || encoding == Encoding::STEIM2 {
            ByteOrder::Big
        } else {
            ByteOrder::Little
        };

        let mut msr = Record {
            sid,
            format_version: Some(FormatVersion::V3),
            flags: RecordFlags::from_bits_retain(rec[3]),
            start_time,
            sample_rate,
            encoding: Some(encoding),
            pub_version: rec[32],
            sample_count: le.u32(&rec[24..28]) as u64,
            crc: header_crc,
            extra,
            data_length: datalength,
            reclen: Some(reclen),
            samples: None,
            raw: Some(rec.to_vec()),
            header_order: le,
            payload_order,
        };

        if flags.contains(ParseFlags::UNPACK_DATA) && msr.sample_count > 0 {
            msr.unpack_data()?;
        }
        Ok(msr)
    }

    /// Unpack a complete miniSEED 2 record.
    pub(crate) fn unpack_v2(rec: &[u8], flags: ParseFlags) -> Result<Record> {
        let reclen = rec.len();
        let sid_for_err = || v2_record_sid(rec).unwrap_or_default();
        if !(64..=MAXRECLEN).contains(&reclen) {
            return Err(Error::OutOfRange(format!(
                "{}: Record length is out of allowed range: {reclen}",
                sid_for_err()
            )));
        }
        if !is_valid_header2(rec) {
            return Err(Error::NotSeed(format!(
                "{}: Record header unrecognized, not a valid miniSEED record",
                sid_for_err()
            )));
        }

        let order = v2_header_order(rec);
        let sid = v2_record_sid(rec).ok_or_else(|| {
            Error::InvalidRecord("Cannot create source identifier for record".into())
        })?;
        let factor = order.u16(&rec[32..34]) as i16;
        let multiplier = order.u16(&rec[34..36]) as i16;

        let mut msr = Record {
            sid,
            format_version: Some(FormatVersion::V2),
            sample_rate: nominal_sample_rate(factor as i32, multiplier as i32),
            sample_count: order.u16(&rec[30..32]) as u64,
            reclen: Some(reclen),
            header_order: order,
            payload_order: order,
            // Set from the fixed header after the blockettes are parsed
            ..Record::new(String::new(), NsTime::UNIX_EPOCH)
        };
        msr.pub_version = match rec[6] {
            b'M' => 4,
            b'Q' => 3,
            b'D' => 2,
            b'R' => 1,
            _ => 0,
        };

        let mut eh = ExtraHeaders::new();
        let set_flag = |eh: &mut ExtraHeaders, ptr: &str| eh.set_bool(ptr, true);
        let sid = msr.sid.clone();
        let err = |e: Error| e.context(&sid);

        // Activity flags
        let act = rec[36];
        if act & 0x01 != 0 {
            msr.flags |= RecordFlags::CALIBRATION;
        }
        if act & 0x04 != 0 {
            set_flag(&mut eh, "/FDSN/Event/Begin").map_err(err)?;
        }
        if act & 0x08 != 0 {
            set_flag(&mut eh, "/FDSN/Event/End").map_err(err)?;
        }
        if act & 0x10 != 0 {
            eh.set_i64("/FDSN/Time/LeapSecond", 1).map_err(err)?;
        }
        if act & 0x20 != 0 {
            eh.set_i64("/FDSN/Time/LeapSecond", -1).map_err(err)?;
        }
        if act & 0x40 != 0 {
            set_flag(&mut eh, "/FDSN/Event/InProgress").map_err(err)?;
        }

        // I/O and clock flags
        let io = rec[37];
        for (bit, ptr) in [
            (0x01, "/FDSN/Flags/StationVolumeParityError"),
            (0x02, "/FDSN/Flags/LongRecordRead"),
            (0x04, "/FDSN/Flags/ShortRecordRead"),
            (0x08, "/FDSN/Flags/StartOfTimeSeries"),
            (0x10, "/FDSN/Flags/EndOfTimeSeries"),
        ] {
            if io & bit != 0 {
                set_flag(&mut eh, ptr).map_err(err)?;
            }
        }
        if io & 0x20 != 0 {
            msr.flags |= RecordFlags::CLOCK_LOCKED;
        }

        // Data quality flags
        let dq = rec[38];
        for (bit, ptr) in [
            (0x01, "/FDSN/Flags/AmplifierSaturation"),
            (0x02, "/FDSN/Flags/DigitizerClipping"),
            (0x04, "/FDSN/Flags/Spikes"),
            (0x08, "/FDSN/Flags/Glitches"),
            (0x10, "/FDSN/Flags/MissingData"),
            (0x20, "/FDSN/Flags/TelemetrySyncError"),
            (0x40, "/FDSN/Flags/FilterCharging"),
        ] {
            if dq & bit != 0 {
                set_flag(&mut eh, ptr).map_err(err)?;
            }
        }
        if dq & 0x80 != 0 {
            msr.flags |= RecordFlags::TIME_QUESTIONABLE;
        }

        let timecorrect = order.u32(&rec[40..44]) as i32;
        if timecorrect != 0 {
            eh.set_f64("/FDSN/Time/Correction", timecorrect as f64 / 10000.0).map_err(err)?;
        }

        // Traverse the blockettes
        let mut b1000: Option<usize> = None;
        let mut b1001: Option<usize> = None;
        let mut blkt_offset = order.u16(&rec[46..48]) as usize;
        let mut blkt_count = 0;
        let mut blkt_end = 0;

        while blkt_offset != 0 && blkt_offset + 4 <= reclen && blkt_offset < MAXRECLEN {
            if blkt_offset < MS2FSDH_LENGTH {
                return Err(Error::InvalidRecord(format!(
                    "{}: Blockette offset ({blkt_offset}) is within the fixed header, impossible",
                    msr.sid
                )));
            }
            let blkt_type = order.u16(&rec[blkt_offset..]);
            let next_blkt = order.u16(&rec[blkt_offset + 2..]) as usize;

            if blkt_type == 2000 && blkt_offset + 6 > reclen {
                return Err(Error::InvalidRecord(format!(
                    "{}: Blockette 2000 length field extends beyond record size, truncated?",
                    msr.sid
                )));
            }

            let blkt_length = v2_blockette_length(blkt_type, &rec[blkt_offset..], order);
            if blkt_length == 0 {
                log::warn!("{}: Unknown blockette length for type {blkt_type}", msr.sid);
                if next_blkt == 0 {
                    break;
                }
                if next_blkt > blkt_offset && next_blkt >= MS2FSDH_LENGTH && next_blkt <= reclen {
                    blkt_offset = next_blkt;
                    blkt_count += 1;
                    continue;
                }
                return Err(Error::InvalidRecord(format!(
                    "{}: Unknown blockette length for type {blkt_type}",
                    msr.sid
                )));
            }
            if blkt_offset + blkt_length > reclen {
                return Err(Error::InvalidRecord(format!(
                    "{}: Blockette {blkt_type} extends beyond record size, truncated?",
                    msr.sid
                )));
            }
            blkt_end = blkt_offset + blkt_length;
            let b = &rec[blkt_offset..blkt_offset + blkt_length];
            let map_err = |what: u16, e: Error| {
                e.context(format_args!(
                    "{}: Problem mapping Blockette {what} to extra headers",
                    msr.sid
                ))
            };

            match blkt_type {
                100 => {
                    let rate = f32::from_bits(order.u32(&b[4..8]));
                    if rate < 0.0 || (rate != 0.0 && !rate.is_normal()) {
                        log::warn!(
                            "{}: Ignoring invalid Blockette 100 sample rate: {rate}",
                            msr.sid
                        );
                    } else {
                        msr.sample_rate = rate as f64;
                    }
                }
                200 | 201 => {
                    let murdock = blkt_type == 201;
                    let flags_b = b[16];
                    let mut e = EventDetection {
                        kind: if murdock { "MURDOCK" } else { "GENERIC" }.into(),
                        detector: clean_tail(&b[if murdock { 36 } else { 28 }..], 24),
                        signal_amplitude: nonzero(f32::from_bits(order.u32(&b[4..8])) as f64),
                        signal_period: nonzero(f32::from_bits(order.u32(&b[8..12])) as f64),
                        background_estimate: nonzero(f32::from_bits(order.u32(&b[12..16])) as f64),
                        onset_time: btime_to_nstime(&b[18..28], order),
                        ..Default::default()
                    };
                    if murdock {
                        e.wave =
                            if flags_b & 0x01 != 0 { "DILATATION" } else { "COMPRESSION" }.into();
                        let snr: [u8; 6] = b[28..34].try_into().expect("6 bytes");
                        e.med_snr = (snr != [0; 6]).then_some(snr);
                        e.med_lookback = Some(b[34] as i32);
                        e.med_pick_algorithm = Some(b[35] as i32);
                        e.units = "COUNTS".into();
                    } else {
                        // Bit 2 set: compression wave according to bit 0
                        if flags_b & 0x04 != 0 {
                            e.wave = if flags_b & 0x01 != 0 { "DILATATION" } else { "COMPRESSION" }
                                .into();
                        }
                        e.units = if flags_b & 0x02 != 0 { "DECONVOLVED" } else { "COUNTS" }.into();
                    }
                    eh.add_event_detection(None, &e).map_err(|e| map_err(blkt_type, e))?;
                }
                300 | 310 | 320 | 390 | 395 => {
                    let c = v2_calibration(blkt_type, b, order);
                    eh.add_calibration(None, &c).map_err(|e| map_err(blkt_type, e))?;
                }
                400 => log::warn!("{}: Blockette 400 is present but discarded", msr.sid),
                405 => log::warn!("{}: Blockette 405 is present but discarded", msr.sid),
                500 => {
                    let usec = b[18] as i8 as i64 * (NSTMODULUS / 1_000_000);
                    let t = TimingException {
                        vco_correction: Some(f32::from_bits(order.u32(&b[4..8])))
                            .filter(|v| *v >= 0.0),
                        time: btime_to_nstime(&b[8..18], order)
                            .and_then(|t| t.checked_add_nanos(usec)),
                        reception_quality: Some(b[19] as i32),
                        count: Some(order.u32(&b[20..24])).filter(|&c| c > 0),
                        kind: clean_tail(&b[24..], 16),
                        clock_status: clean_tail(&b[72..], 128),
                    };
                    eh.add_timing_exception(None, &t).map_err(|e| map_err(500, e))?;
                    // Clock model maps to a single value
                    let model = clean_tail(&b[40..], 32);
                    eh.set_str("/FDSN/Clock/Model", &model).map_err(|e| map_err(500, e))?;
                }
                1000 => {
                    b1000 = Some(blkt_offset);
                    let exp = b[6];
                    if exp < 31 {
                        let b1000reclen = 1usize << exp;
                        if b1000reclen > reclen {
                            return Err(Error::InvalidRecord(format!(
                                "{}: Record length in Blockette 1000 ({b1000reclen}) exceeds the buffer length ({reclen})",
                                msr.sid
                            )));
                        }
                        msr.reclen = Some(b1000reclen);
                    }
                    msr.encoding = Some(Encoding(b[4]));
                }
                1001 => {
                    b1001 = Some(blkt_offset);
                    eh.set_i64("/FDSN/Time/Quality", b[4] as i64).map_err(|e| map_err(1001, e))?;
                }
                2000 => log::warn!("{}: Blockette 2000 is present but discarded", msr.sid),
                _ => log::warn!("{}: Unsupported blockette type {blkt_type}, skipping", msr.sid),
            }

            if next_blkt != 0 && next_blkt < blkt_offset + blkt_length {
                log::warn!(
                    "{}: Offset to next blockette ({next_blkt}) is within current blockette ending at byte {}",
                    msr.sid,
                    blkt_offset + blkt_length
                );
                blkt_offset = 0;
            } else if next_blkt != 0 && next_blkt > reclen {
                log::warn!(
                    "{}: Offset to next blockette ({next_blkt}) from type {blkt_type} is beyond record length",
                    msr.sid
                );
                blkt_offset = 0;
            } else {
                blkt_offset = next_blkt;
            }
            blkt_count += 1;
        }

        if !eh.is_empty() {
            msr.extra = eh.to_json()?;
        }

        let dataoffset = order.u16(&rec[44..46]) as usize;
        if blkt_end != 0 && order.u16(&rec[30..32]) != 0 && dataoffset < blkt_end {
            log::warn!(
                "{}: Data offset in fixed header ({dataoffset}) is within the blockette chain ending at {blkt_end}",
                msr.sid
            );
        }
        if rec[39] as usize != blkt_count {
            log::warn!(
                "{}: Number of blockettes in fixed header ({}) does not match the number parsed ({blkt_count})",
                msr.sid,
                rec[39]
            );
        }

        // Start time, rejecting an unset (year 0) or invalid time
        msr.start_time = btime_to_nstime(&rec[20..30], order).ok_or_else(|| {
            Error::InvalidRecord(format!(
                "{}: Cannot convert start time to internal time stamp",
                msr.sid
            ))
        })?;
        let adjust = |t: NsTime, nanos: i64, what: &str| {
            t.checked_add_nanos(nanos).ok_or_else(|| {
                Error::InvalidRecord(format!(
                    "{}: Start time with {what} is beyond the representable time range",
                    msr.sid
                ))
            })
        };
        // Apply the time correction unless already applied (bit 1 of activity flags)
        if timecorrect != 0 && act & 0x02 == 0 {
            msr.start_time = adjust(
                msr.start_time,
                timecorrect as i64 * (NSTMODULUS / 10000),
                "time correction",
            )?;
        }
        // Apply microsecond precision from Blockette 1001
        if let Some(off) = b1001 {
            let usec = rec[off + 5] as i8 as i64 * (NSTMODULUS / 1_000_000);
            msr.start_time = adjust(msr.start_time, usec, "Blockette 1001 microseconds")?;
        }

        let rl = msr.reclen.unwrap_or(reclen);
        msr.data_length =
            if dataoffset > 0 && dataoffset < rl { (rl - dataoffset) as u32 } else { 0 };

        // Payload byte order from Blockette 1000, otherwise the header order
        msr.payload_order = match b1000 {
            Some(off) => {
                if rec[off + 5] > 0 {
                    ByteOrder::Big
                } else {
                    ByteOrder::Little
                }
            }
            None => order,
        };
        msr.raw = Some(rec.to_vec());

        if flags.contains(ParseFlags::UNPACK_DATA) && msr.sample_count > 0 {
            msr.unpack_data()?;
        }
        Ok(msr)
    }
}

/// Source identifier from a raw miniSEED 2 record.
pub(crate) fn v2_record_sid(rec: &[u8]) -> Option<String> {
    if rec.len() < 20 {
        return None;
    }
    let net = clean_all(&rec[18..20], 2);
    let sta = clean_all(&rec[8..13], 5);
    let loc = clean_all(&rec[13..15], 2);
    // Map 3 channel codes to BAND_SOURCE_POSITION, handled as a C string
    let mut chan = Vec::with_capacity(5);
    chan.extend_from_slice(&[rec[15], b'_', rec[16], b'_', rec[17]]);
    if let Some(p) = chan.iter().position(|&c| c == 0) {
        chan.truncate(p);
    }
    let chan = String::from_utf8_lossy(&chan).into_owned();
    nslc_to_sid(&net, &sta, &loc, &chan).ok()
}

/// Valid UTF-8 prefix of at most `len` bytes ending at the first NUL.
fn utf8_prefix(b: &[u8], len: usize) -> &str {
    let b = &b[..len.min(b.len())];
    let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
    match std::str::from_utf8(b) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&b[..e.valid_up_to()]).unwrap_or(""),
    }
}

/// Copy with all spaces removed.
pub(crate) fn clean_all(b: &[u8], len: usize) -> String {
    utf8_prefix(b, len).chars().filter(|&c| c != ' ').collect()
}

/// Copy with trailing spaces removed.
pub(crate) fn clean_tail(b: &[u8], len: usize) -> String {
    utf8_prefix(b, len).trim_end_matches(' ').to_string()
}

/// Nominal sample rate from SEED factor and multiplier.
pub fn nominal_sample_rate(factor: i32, multiplier: i32) -> f64 {
    let mut samprate = 0.0;
    if factor > 0 {
        samprate = factor as f64;
    } else if factor < 0 {
        samprate = -1.0 / factor as f64;
    }
    if multiplier > 0 {
        samprate *= multiplier as f64;
    } else if multiplier < 0 {
        samprate = -(samprate / multiplier as f64);
    }
    samprate
}

/// Length in bytes of a miniSEED 2 blockette type, 0 if unknown.
pub(crate) fn v2_blockette_length(blkt_type: u16, blkt: &[u8], order: ByteOrder) -> usize {
    match blkt_type {
        100 => 12,
        200 => 52,
        201 => 60,
        300 | 310 => 60,
        320 => 64,
        390 => 28,
        395 => 16,
        400 => 16,
        405 => 6,
        500 => 200,
        1000 | 1001 => 8,
        2000 if blkt.len() >= 6 => order.u16(&blkt[4..6]) as usize,
        _ => 0,
    }
}

/// Descriptive name of a miniSEED 2 blockette type.
#[must_use]
pub fn blockette_description(blkt_type: u16) -> Option<&'static str> {
    Some(match blkt_type {
        100 => "Sample Rate",
        200 => "Generic Event Detection",
        201 => "Murdock Event Detection",
        300 => "Step Calibration",
        310 => "Sine Calibration",
        320 => "Pseudo-random Calibration",
        390 => "Generic Calibration",
        395 => "Calibration Abort",
        400 => "Beam",
        405 => "Beam Delay",
        500 => "Timing",
        1000 => "Data Only SEED",
        1001 => "Data Extension",
        2000 => "Opaque Data",
        _ => return None,
    })
}

/// Convert a SEED BTIME (10 bytes) to an [`NsTime`].
///
/// Returns `None` when the time is unset (year 0) or invalid.
pub(crate) fn btime_to_nstime(b: &[u8], order: ByteOrder) -> Option<NsTime> {
    let year = order.u16(&b[0..2]);
    let fract = order.u16(&b[8..10]);
    if year == 0 || fract > 9999 {
        return None;
    }
    NsTime::from_ordinal(
        year as i32,
        order.u16(&b[2..4]) as i32,
        b[4] as i32,
        b[5] as i32,
        b[6] as i32,
        fract as u32 * (NSTMODULUS as u32 / 10000),
    )
    .ok()
}

/// `Some(v)` unless `v` is zero, which blockettes use for "not included".
fn nonzero(v: f64) -> Option<f64> {
    (v != 0.0).then_some(v)
}

/// Map a calibration blockette (300, 310, 320, 390, 395) to calibration parameters.
fn v2_calibration(blkt_type: u16, b: &[u8], order: ByteOrder) -> Calibration {
    let f32at = |o: usize| f32::from_bits(order.u32(&b[o..o + 4])) as f64;
    let u32at = |o: usize| order.u32(&b[o..o + 4]);
    let mut c = Calibration::default();
    let begin = btime_to_nstime(&b[4..14], order);
    let flags = b[15];
    let trigger = |f: u8| if f & 0x04 != 0 { "AUTOMATIC" } else { "MANUAL" }.to_string();

    match blkt_type {
        300 => {
            c.kind = "STEP".into();
            c.begin_time = begin;
            c.steps = Some(b[14] as i32);
            c.first_pulse_positive = (flags & 0x01 != 0).then_some(true);
            c.alternate_sign = (flags & 0x02 != 0).then_some(true);
            c.trigger = trigger(flags);
            c.continued = (flags & 0x08 != 0).then_some(true);
            c.duration = nonzero(u32at(16) as f64 / 10000.0);
            c.step_between = nonzero(u32at(20) as f64 / 10000.0);
            c.amplitude = nonzero(f32at(24));
            c.input_channel = clean_tail(&b[28..], 3);
            c.ref_amplitude = nonzero(u32at(32) as f64);
            c.coupling = clean_tail(&b[36..], 12);
            c.rolloff = clean_tail(&b[48..], 12);
        }
        310 => {
            c.kind = "SINE".into();
            c.begin_time = begin;
            c.trigger = trigger(flags);
            c.continued = (flags & 0x08 != 0).then_some(true);
            c.amplitude_range = if flags & 0x10 != 0 {
                "PEAKTOPEAK"
            } else if flags & 0x20 != 0 {
                "ZEROTOPEAK"
            } else if flags & 0x40 != 0 {
                "RMS"
            } else {
                ""
            }
            .into();
            c.duration = nonzero(u32at(16) as f64 / 10000.0);
            c.sine_period = nonzero(f32at(20));
            c.amplitude = nonzero(f32at(24));
            c.input_channel = clean_tail(&b[28..], 3);
            c.ref_amplitude = nonzero(u32at(32) as f64);
            c.coupling = clean_tail(&b[36..], 12);
            c.rolloff = clean_tail(&b[48..], 12);
        }
        320 => {
            c.kind = "PSEUDORANDOM".into();
            c.begin_time = begin;
            c.trigger = trigger(flags);
            c.continued = (flags & 0x08 != 0).then_some(true);
            if flags & 0x10 != 0 {
                c.amplitude_range = "RANDOM".into();
            }
            c.duration = nonzero(u32at(16) as f64 / 10000.0);
            c.amplitude = nonzero(f32at(20));
            c.input_channel = clean_tail(&b[24..], 3);
            c.ref_amplitude = nonzero(u32at(28) as f64);
            c.coupling = clean_tail(&b[32..], 12);
            c.rolloff = clean_tail(&b[44..], 12);
            c.noise = clean_tail(&b[56..], 8);
        }
        390 => {
            c.kind = "GENERIC".into();
            c.begin_time = begin;
            c.trigger = trigger(flags);
            c.continued = (flags & 0x08 != 0).then_some(true);
            c.duration = nonzero(u32at(16) as f64 / 10000.0);
            c.amplitude = nonzero(f32at(20));
            c.input_channel = clean_tail(&b[24..], 3);
        }
        _ => {
            // 395, calibration abort: only an end time
            c.kind = "ABORT".into();
            c.begin_time = None;
            c.end_time = begin;
        }
    }
    c
}

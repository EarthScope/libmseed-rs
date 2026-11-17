//! Packing records into miniSEED 3 or 2.
//!
//! [`Record::pack`] and [`RecordPacker`] create records from a [`Record`] holding data
//! samples, configured by [`PackOptions`]; [`Record::repack`] rewrites a parsed record's
//! header around its original encoded data.
//!
//! Version 3 is the default; miniSEED 2 is selected by [`PackOptions::format`] or by
//! [`Record::format_version`] being miniSEED 2. For miniSEED 2 the extra headers
//! (`/FDSN/...`) are mapped to blockettes where possible.

use crate::codec::{ByteOrder, Samples, encode::encode};
use crate::crc32c::crc32c;
use crate::error::{Error, Result};
use crate::extra::ExtraHeaders;
use crate::flags::RecordFlags;
use crate::format::{MS2FSDH_LENGTH, MS3FSDH_LENGTH};
use crate::lookup::{Encoding, SampleType};
use crate::options::{FormatVersion, PackOptions};
use crate::record::{MAXRECLEN, MINRECLEN, PACK_DEFAULT_ENCODING, PACK_DEFAULT_RECLEN, Record};
use crate::sid::sid_to_nslc;
use crate::time::{NSTMODULUS, NsTime};
use crate::unpack::nominal_sample_rate;
use serde_json::Value;
use std::borrow::Cow;

/// Maximum samples a Steim-1 frame holds (15 words of four 8-bit differences).
pub(crate) const STEIM1_FRAME_MAX_SAMPLES: usize = 60;
/// Maximum samples a Steim-2 frame holds (15 words of seven 4-bit differences).
pub(crate) const STEIM2_FRAME_MAX_SAMPLES: usize = 105;

/// Maximum number of samples that fit in `maxdatabytes`.
pub(crate) fn max_samples(maxdatabytes: usize, encoding: Encoding, samplesize: usize) -> usize {
    match encoding {
        Encoding::STEIM1 => (maxdatabytes / 64) * STEIM1_FRAME_MAX_SAMPLES,
        Encoding::STEIM2 => (maxdatabytes / 64) * STEIM2_FRAME_MAX_SAMPLES,
        _ if samplesize > 0 => maxdatabytes / samplesize,
        _ => 0,
    }
}

/// True when `numsamples` would not fill a miniSEED 3 record of `maxreclen`.
pub(crate) fn short_of_record(
    format_version: u8,
    maxreclen: usize,
    sidlength: usize,
    extralength: usize,
    encoding: Encoding,
    samplesize: usize,
    numsamples: usize,
) -> bool {
    if format_version != 3
        || samplesize == 0
        || numsamples == 0
        || !(MINRECLEN..=MAXRECLEN).contains(&maxreclen)
    {
        return false;
    }
    let offset = MS3FSDH_LENGTH + sidlength + extralength;
    if maxreclen < offset {
        return false;
    }
    numsamples < max_samples(maxreclen - offset, encoding, samplesize)
}

/// A packed miniSEED 2 header with the offsets of blockettes 1000 and 1001.
pub(crate) struct Header2 {
    pub bytes: Vec<u8>,
    pub b1000: usize,
    pub b1001: usize,
}

/// Default encoding for a sample type: the fixed encodings of text and floats, otherwise
/// [`PACK_DEFAULT_ENCODING`].
pub(crate) fn default_encoding(sample_type: Option<SampleType>) -> Encoding {
    match sample_type {
        Some(SampleType::Text) => Encoding::TEXT,
        Some(SampleType::Float32) => Encoding::FLOAT32,
        Some(SampleType::Float64) => Encoding::FLOAT64,
        _ => PACK_DEFAULT_ENCODING,
    }
}

/// Encoding for packing samples of `sample_type`: text and float samples have a fixed
/// encoding, integer samples (or none) use `requested`, otherwise the default.
pub(crate) fn resolve_encoding(
    sample_type: Option<SampleType>,
    requested: Option<Encoding>,
) -> Encoding {
    match sample_type {
        None | Some(SampleType::Int32) => requested.unwrap_or(PACK_DEFAULT_ENCODING),
        t => default_encoding(t),
    }
}

/// Resolved record length, encoding and extra headers for header packing.
pub(crate) struct HeaderParams<'a> {
    pub reclen: usize,
    pub encoding: Encoding,
    pub extra: &'a str,
}

// ---------------------------------------------------------------------------------------------
// Time and rate helpers
// ---------------------------------------------------------------------------------------------

/// Round a time to microseconds and split into second-resolution time, 100 microsecond
/// ticks within the second and a microsecond offset in -50..=49.
pub(crate) fn nstime_to_fsec_usec(nstime: NsTime) -> Result<(NsTime, u16, i8)> {
    let wide = nstime.as_nanos() as i128;
    let usec_time = (wide + if wide >= 0 { 500 } else { -500 }) / 1000 * 1000;
    let total_usec = usec_time / 1000;
    let mut total_fsec = usec_time / 100_000;
    let mut usec_offset = (total_usec - total_fsec * 100) as i32;
    if usec_offset > 49 {
        total_fsec += 1;
        usec_offset -= 100;
    } else if usec_offset < -50 {
        total_fsec -= 1;
        usec_offset += 100;
    }
    let mut rem = total_fsec % 10000;
    let mut seconds = total_fsec / 10000;
    if rem < 0 {
        rem += 10000;
        seconds -= 1;
    }
    let second_nstime = seconds * NSTMODULUS as i128;
    if second_nstime > i64::MAX as i128 || second_nstime < i64::MIN as i128 {
        return Err(Error::Pack("Time is not representable".into()));
    }
    Ok((NsTime::from_nanos(second_nstime as i64), rem as u16, usec_offset as i8))
}

fn rat_approx(real: f64, maxval: i32, precision: f64) -> (i32, i32) {
    let pos = real >= 0.0;
    let mut realj = real.abs();
    let preal = realj;
    let mut bj = (realj + precision) as i32;
    realj = 1.0 / (realj - bj as f64);
    let mut aj = bj;
    let mut aj1 = 1i32;
    let mut bjj = 1i32;
    let mut bj1 = 0i32;
    let mut num = aj;
    let mut den = bjj;
    if !pos {
        num = -num;
    }
    let (mut pnum, mut pden) = (aj, bjj);
    while (preal - aj as f64 / bjj as f64).abs() > precision && aj < maxval && bjj < maxval {
        let aj2 = aj1;
        aj1 = aj;
        let bj2 = bj1;
        bj1 = bjj;
        bj = (realj + precision) as i32;
        realj = 1.0 / (realj - bj as f64);
        aj = bj.wrapping_mul(aj1).wrapping_add(aj2);
        bjj = bj.wrapping_mul(bj1).wrapping_add(bj2);
        num = pnum;
        den = pden;
        if !pos {
            num = -num;
        }
        pnum = aj;
        pden = bjj;
    }
    if pnum < maxval && pden < maxval {
        num = pnum;
        den = pden;
        if !pos {
            num = -num;
        }
    }
    (num, den)
}

/// Fast inverse square root approximation used to seed the factor search.
fn rsqrt64(val: f64) -> f64 {
    let x2 = val * 0.5;
    let mut y = f64::from_bits(0x5fe6eb50c7b537a9u64.wrapping_sub(val.to_bits() >> 1));
    y *= 1.5 - (x2 * y * y);
    y *= 1.5 - (x2 * y * y);
    y *= 1.5 - (x2 * y * y);
    y
}

fn reduce_rate(samprate: f64) -> Option<(i16, i16)> {
    if !(samprate > 0.0 && samprate <= (32767i64 * 32767) as f64) {
        return None;
    }
    let intsamprate = (samprate + 0.5) as i32;

    if (samprate - intsamprate as f64).abs() < 0.0000001 {
        if intsamprate <= 32767 {
            return Some((intsamprate as i16, 1));
        } else if (intsamprate as i64) <= 32767 * 32767 {
            let mut searchfactor1 = (1.0 / rsqrt64(samprate)) as i32;
            let mut closestdiff = searchfactor1;
            let mut closestfactor = searchfactor1;
            while searchfactor1 > 1 && intsamprate % searchfactor1 != 0 {
                searchfactor1 -= 1;
                let searchfactor2 = intsamprate / searchfactor1;
                let diff = intsamprate - searchfactor1 * searchfactor2;
                if diff < closestdiff {
                    closestdiff = diff;
                    closestfactor = searchfactor1;
                }
                if searchfactor1 > 1 && intsamprate / (searchfactor1 - 1) > 32767 {
                    searchfactor1 = closestfactor;
                    break;
                }
            }
            if searchfactor1 == 0 {
                return None;
            }
            let searchfactor2 = intsamprate / searchfactor1;
            if searchfactor1 <= 32767 && searchfactor2 <= 32767 {
                return Some((searchfactor1 as i16, searchfactor2 as i16));
            }
        }
    } else if samprate <= 32767.0 {
        let (num, den) = rat_approx(samprate, 32767, 1e-8);
        return Some((num as i16, (-den) as i16));
    }
    None
}

/// SEED sample rate factor and multiplier for a rate in Hz or a (negative) period.
pub(crate) fn gen_factor_multiplier(samprate: f64) -> Option<(i16, i16)> {
    let samprate = if samprate < 0.0 { -1.0 / samprate } else { samprate };
    if samprate == 0.0 {
        return Some((0, 0));
    }
    if samprate >= 1.0 {
        reduce_rate(samprate)
    } else {
        reduce_rate(1.0 / samprate).map(|(f1, f2)| (-f1, -f2))
    }
}

// ---------------------------------------------------------------------------------------------
// Byte buffer helpers
// ---------------------------------------------------------------------------------------------

/// A zero-initialized record buffer written in a fixed byte order.
struct Buf {
    v: Vec<u8>,
    order: ByteOrder,
}

impl Buf {
    fn new(len: usize, order: ByteOrder) -> Self {
        Buf { v: vec![0; len], order }
    }
    fn u16(&mut self, off: usize, val: u16) {
        self.v[off..off + 2].copy_from_slice(&match self.order {
            ByteOrder::Little => val.to_le_bytes(),
            ByteOrder::Big => val.to_be_bytes(),
        });
    }
    fn u32(&mut self, off: usize, val: u32) {
        self.v[off..off + 4].copy_from_slice(&match self.order {
            ByteOrder::Little => val.to_le_bytes(),
            ByteOrder::Big => val.to_be_bytes(),
        });
    }
    fn f32(&mut self, off: usize, val: f32) {
        self.u32(off, val.to_bits());
    }
    fn f64(&mut self, off: usize, val: f64) {
        let bits = val.to_bits();
        self.v[off..off + 8].copy_from_slice(&match self.order {
            ByteOrder::Little => bits.to_le_bytes(),
            ByteOrder::Big => bits.to_be_bytes(),
        });
    }
    /// Left-justified, space-padded text in a fixed-width field.
    fn text(&mut self, off: usize, len: usize, s: &str) {
        let mut n = len.min(s.len());
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        let b = s.as_bytes();
        let cut = b[..n].iter().position(|&c| c == 0).unwrap_or(n);
        for i in 0..len {
            self.v[off + i] = if i < cut { b[i] } else { b' ' };
        }
    }
}

fn get_str<'a>(v: &'a Value, ptr: &str) -> Option<&'a str> {
    v.pointer(ptr).and_then(Value::as_str)
}

fn prefix_ci(h: &str, lit: &str) -> bool {
    h.len() >= lit.len() && h.as_bytes()[..lit.len()].eq_ignore_ascii_case(lit.as_bytes())
}

/// Write a SEED BTIME (10 bytes at `off`) for a time string; returns the microsecond offset.
fn put_btime(b: &mut Buf, off: usize, timestr: &str) -> Result<i8> {
    let nstime = NsTime::parse(timestr)?;
    let (second, fsec, usec) = nstime_to_fsec_usec(nstime)?;
    let t = second.parts();
    b.u16(off, t.year);
    b.u16(off + 2, t.yday);
    b.v[off + 4] = t.hour;
    b.v[off + 5] = t.min;
    b.v[off + 6] = t.sec;
    b.v[off + 7] = 0;
    b.u16(off + 8, fsec);
    Ok(usec)
}

// ---------------------------------------------------------------------------------------------
// Header packing
// ---------------------------------------------------------------------------------------------

impl Record {
    /// Pack the fixed header, source identifier and extra headers of a miniSEED 3 record.
    /// The result's length is the data offset.
    pub(crate) fn pack_header3(&self, p: &HeaderParams<'_>) -> Result<Vec<u8>> {
        let (maxreclen, encoding, extra) = (p.reclen, p.encoding, p.extra);
        if !(MINRECLEN..=MAXRECLEN).contains(&maxreclen) {
            return Err(Error::Pack(format!(
                "{}: Record length is out of range: {maxreclen}",
                self.sid
            )));
        }
        let sidlength = self.sid.len();
        if sidlength > 255 {
            return Err(Error::Pack(format!(
                "{}: Source ID too long: {sidlength} bytes",
                self.sid
            )));
        }
        if extra.len() > u16::MAX as usize {
            return Err(Error::Pack(format!(
                "{}: Extra headers too long: {} bytes",
                self.sid,
                extra.len()
            )));
        }
        let t = self.start_time.parts();

        let total = MS3FSDH_LENGTH + sidlength + extra.len();
        let mut b = Buf::new(total, ByteOrder::Little);
        b.v[0] = b'M';
        b.v[1] = b'S';
        b.v[2] = 3;
        b.v[3] = self.flags.bits();
        b.u32(4, t.nsec);
        b.u16(8, t.year);
        b.u16(10, t.yday);
        b.v[12] = t.hour;
        b.v[13] = t.min;
        b.v[14] = t.sec;
        b.v[15] = encoding.0;
        // A rate between 0 and 1 is written in period notation
        if self.sample_rate > 0.0 && self.sample_rate < 1.0 {
            b.f64(16, -1.0 / self.sample_rate);
        } else {
            b.f64(16, self.sample_rate);
        }
        b.v[32] = self.pub_version;
        b.v[33] = sidlength as u8;
        b.u16(34, extra.len() as u16);
        b.v[MS3FSDH_LENGTH..MS3FSDH_LENGTH + sidlength].copy_from_slice(self.sid.as_bytes());
        b.v[MS3FSDH_LENGTH + sidlength..].copy_from_slice(extra.as_bytes());
        Ok(b.v)
    }

    /// Pack the fixed header and blockettes of a miniSEED 2 record.
    ///
    /// Extra headers are mapped to blockettes: sample rate (100), event detections
    /// (200, 201), calibrations (300, 310, 320, 390, 395), timing exceptions (500) and
    /// data extension (1001), in addition to the mandatory blockette 1000.
    pub(crate) fn pack_header2(&self, p: &HeaderParams<'_>) -> Result<Header2> {
        let sid = &self.sid;
        let (reclen, encoding) = (p.reclen, p.encoding);
        if !(128..=MAXRECLEN).contains(&reclen) {
            return Err(Error::Pack(format!("{sid}: Record length is out of range: {reclen}")));
        }
        if !reclen.is_power_of_two() {
            return Err(Error::Pack(format!(
                "{sid}: Cannot pack miniSEED 2, record length ({reclen}) is not a power of 2"
            )));
        }
        let reclenexp = reclen.trailing_zeros() as u8;

        let nslc = sid_to_nslc(sid).map_err(|_| {
            Error::Pack(format!("{sid}: Cannot parse SEED identifier codes from full identifier"))
        })?;
        if nslc.network.len() > 2
            || nslc.station.len() > 5
            || nslc.location.len() > 2
            || nslc.channel.len() != 3
        {
            return Err(Error::Pack(format!(
                "{sid}: Cannot create miniSEED 2 for N,S,L,C codes: {}, {}, {}, {}",
                nslc.network, nslc.station, nslc.location, nslc.channel
            )));
        }

        let (second_nstime, fsec, usec_offset) = nstime_to_fsec_usec(self.start_time)?;
        let t = second_nstime.parts();
        if !(1900..=2100).contains(&t.year) {
            log::warn!("{sid}: Unlikely start year for miniSEED 2 (1900-2100): {}", t.year);
        }

        let hz = self.sample_rate_hz();
        let (factor, multiplier) = gen_factor_multiplier(hz).ok_or_else(|| {
            Error::Pack(format!(
                "{sid}: Cannot convert sample rate ({}) to factor and multiplier",
                self.sample_rate
            ))
        })?;

        let eh = ExtraHeaders::from_json(p.extra).map_err(|e| e.context(sid))?;
        let root = eh.as_value();
        let get = |ptr: &str| -> Option<&Value> { root.pointer(ptr).filter(|v| !v.is_null()) };
        let get_uint = |v: &Value, ptr: &str| v.pointer(ptr).and_then(Value::as_u64);
        let get_num = |v: &Value, ptr: &str| v.pointer(ptr).and_then(Value::as_f64);
        let get_bool = |v: &Value, ptr: &str| v.pointer(ptr).and_then(Value::as_bool);

        // Generous upper bound for the header: fixed + all blockettes
        let mut b = Buf::new(reclen.min(MS2FSDH_LENGTH + 8 + 12 + 8 + 65535), ByteOrder::Big);
        let cap = b.v.len();
        let mut written;

        // Fixed header
        match get_uint(root, "/FDSN/Sequence") {
            Some(n) if n <= 999_999 => b.v[0..6].copy_from_slice(format!("{n:06}").as_bytes()),
            Some(_) => b.v[0..6].copy_from_slice(b"999999"),
            None => b.v[0..6].copy_from_slice(b"000000"),
        }
        b.v[6] = match get_str(root, "/FDSN/DataQuality").and_then(|s| s.bytes().next()) {
            Some(c) if matches!(c, b'D' | b'R' | b'Q' | b'M') => c,
            _ => match self.pub_version {
                1 => b'R',
                3 => b'Q',
                4 => b'M',
                _ => b'D',
            },
        };
        b.v[7] = b' ';
        b.text(8, 5, &nslc.station);
        b.text(13, 2, &nslc.location);
        b.text(15, 3, &nslc.channel);
        b.text(18, 2, &nslc.network);
        b.u16(20, t.year);
        b.u16(22, t.yday);
        b.v[24] = t.hour;
        b.v[25] = t.min;
        b.v[26] = t.sec;
        b.v[27] = 0;
        b.u16(28, fsec);
        b.u16(30, 0);
        b.u16(32, factor as u16);
        b.u16(34, multiplier as u16);

        // Activity flags
        let mut act = 0u8;
        if self.flags.contains(RecordFlags::CALIBRATION) {
            act |= 0x01;
        }
        if get_bool(root, "/FDSN/Event/Begin") == Some(true) {
            act |= 0x04;
        }
        if get_bool(root, "/FDSN/Event/End") == Some(true) {
            act |= 0x08;
        }
        if let Some(n) = get_num(root, "/FDSN/Time/LeapSecond") {
            if n > 0.0 {
                act |= 0x10;
            } else if n < 0.0 {
                act |= 0x20;
            }
        }
        if get_bool(root, "/FDSN/Event/InProgress") == Some(true) {
            act |= 0x40;
        }
        // I/O and clock flags
        let mut io = 0u8;
        for (bit, ptr) in [
            (0x01, "/FDSN/Flags/StationVolumeParityError"),
            (0x02, "/FDSN/Flags/LongRecordRead"),
            (0x04, "/FDSN/Flags/ShortRecordRead"),
            (0x08, "/FDSN/Flags/StartOfTimeSeries"),
            (0x10, "/FDSN/Flags/EndOfTimeSeries"),
        ] {
            if get_bool(root, ptr) == Some(true) {
                io |= bit;
            }
        }
        if self.flags.contains(RecordFlags::CLOCK_LOCKED) {
            io |= 0x20;
        }
        // Data quality flags
        let mut dq = 0u8;
        for (bit, ptr) in [
            (0x01, "/FDSN/Flags/AmplifierSaturation"),
            (0x02, "/FDSN/Flags/DigitizerClipping"),
            (0x04, "/FDSN/Flags/Spikes"),
            (0x08, "/FDSN/Flags/Glitches"),
            (0x10, "/FDSN/Flags/MissingData"),
            (0x20, "/FDSN/Flags/TelemetrySyncError"),
            (0x40, "/FDSN/Flags/FilterCharging"),
        ] {
            if get_bool(root, ptr) == Some(true) {
                dq |= bit;
            }
        }
        if self.flags.contains(RecordFlags::TIME_QUESTIONABLE) {
            dq |= 0x80;
        }
        if let Some(n) = get_num(root, "/FDSN/Time/Correction") {
            // 0.0001 second units, rounded to nearest with ties away from zero
            let mut correction = n * 10000.0;
            correction += if correction < 0.0 { -0.5 } else { 0.5 };
            if !correction.is_finite()
                || correction <= i32::MIN as f64 - 1.0
                || correction >= i32::MAX as f64 + 1.0
            {
                return Err(Error::Pack(format!(
                    "{sid}: Time correction is not representable: {n}"
                )));
            }
            b.u32(40, (correction as i32) as u32);
            // Time corrections are always applied on read, mark as applied
            act |= 0x02;
        }
        b.v[36] = act;
        b.v[37] = io;
        b.v[38] = dq;
        b.v[39] = 1; // Blockette count, incremented below
        b.u16(44, 0);
        b.u16(46, MS2FSDH_LENGTH as u16);
        written = MS2FSDH_LENGTH;

        let check = |written: usize, need: usize, what: &str| -> Result<()> {
            if written > u16::MAX as usize || written + need > cap {
                Err(Error::Pack(format!("{sid}: Record length not large enough for {what}")))
            } else {
                Ok(())
            }
        };

        // Mandatory Blockette 1000
        let mut next_off = written + 2;
        b.u16(written, 1000);
        b.u16(written + 2, 0);
        b.v[written + 4] = encoding.0;
        b.v[written + 5] = 1; // big endian
        b.v[written + 6] = reclenexp;
        b.v[written + 7] = 0;
        let b1000 = written;
        let mut b1001 = 0;
        written += 8;

        // Blockette 1001 when there is timing quality or a microsecond offset, or when
        // continuation records may need an offset because the period is off the 100 us grid
        let mut offgrid_possible = false;
        if hz > 0.0 {
            let units = 10000.0 / hz;
            if units < 9.0e15 {
                let fraction = units - (units as i64) as f64;
                offgrid_possible = fraction > 1.0e-6 && fraction < 1.0 - 1.0e-6;
            }
        }
        let timing_quality = get_uint(root, "/FDSN/Time/Quality");
        if timing_quality.is_some() || usec_offset != 0 || offgrid_possible {
            check(written, 8, "B1001")?;
            b.u16(next_off, written as u16);
            next_off = written + 2;
            b.v[39] += 1;
            b.u16(written, 1001);
            b.u16(written + 2, 0);
            b.v[written + 4] = match timing_quality {
                Some(q) if q <= 255 => q as u8,
                _ => 0,
            };
            b.v[written + 5] = usec_offset as u8;
            b1001 = written;
            written += 8;
        }

        // Blockette 100 if the rate is not well represented by factor and multiplier
        if (hz - nominal_sample_rate(factor as i32, multiplier as i32)).abs() > 0.0001 {
            check(written, 12, "B100")?;
            b.u16(next_off, written as u16);
            next_off = written + 2;
            b.v[39] += 1;
            b.u16(written, 100);
            b.u16(written + 2, 0);
            b.f32(written + 4, hz as f32);
            written += 12;
        }

        // Blockette 500, timing exceptions
        if let Some(Value::Array(arr)) = get("/FDSN/Time/Exception") {
            for item in arr.iter().filter(|v| v.is_object()) {
                check(written, 200, "B500")?;
                b.u16(next_off, written as u16);
                next_off = written + 2;
                b.v[39] += 1;
                b.u16(written, 500);
                if let Some(n) = get_num(item, "/VCOCorrection") {
                    b.f32(written + 4, n as f32);
                }
                if let Some(ts) = get_str(item, "/Time") {
                    let usec = put_btime(&mut b, written + 8, ts).map_err(|_| {
                        Error::Pack(format!("{sid}: Cannot convert B500 time: {ts}"))
                    })?;
                    b.v[written + 18] = usec as u8;
                }
                if let Some(q) = get_uint(item, "/ReceptionQuality").filter(|&q| q <= 255) {
                    b.v[written + 19] = q as u8;
                }
                if let Some(c) = get_uint(item, "/Count").filter(|&c| c <= u32::MAX as u64) {
                    b.u32(written + 20, c as u32);
                }
                if let Some(s) = get_str(item, "/Type") {
                    b.text(written + 24, 16, s);
                }
                if let Some(s) = get_str(root, "/FDSN/Clock/Model") {
                    b.text(written + 40, 32, s);
                }
                if let Some(s) = get_str(item, "/ClockStatus") {
                    b.text(written + 72, 128, s);
                }
                written += 200;
            }
        }

        // Blockettes 200 and 201, event detections
        if let Some(Value::Array(arr)) = get("/FDSN/Event/Detection") {
            for item in arr.iter().filter(|v| v.is_object()) {
                let murdock = get_str(item, "/Type").is_some_and(|s| prefix_ci(s, "MURDOCK"));
                let (btype, blen) = if murdock { (201u16, 60) } else { (200u16, 52) };
                check(written, blen, &format!("B{btype}"))?;
                b.u16(next_off, written as u16);
                next_off = written + 2;
                b.v[39] += 1;
                b.u16(written, btype);
                if let Some(n) = get_num(item, "/SignalAmplitude") {
                    b.f32(written + 4, n as f32);
                }
                if let Some(n) = get_num(item, "/SignalPeriod") {
                    b.f32(written + 8, n as f32);
                }
                if let Some(n) = get_num(item, "/BackgroundEstimate") {
                    b.f32(written + 12, n as f32);
                }
                match get_str(item, "/Wave") {
                    Some(w) => {
                        if prefix_ci(w, "DILATATION") {
                            b.v[written + 16] |= 0x01;
                        }
                    }
                    None if btype == 200 => b.v[written + 16] |= 0x04,
                    None => {}
                }
                if btype == 200 && get_str(item, "/Units").is_some_and(|u| !prefix_ci(u, "COUNT")) {
                    b.v[written + 16] |= 0x02;
                }
                if let Some(ts) = get_str(item, "/OnsetTime") {
                    put_btime(&mut b, written + 18, ts).map_err(|_| {
                        Error::Pack(format!("{sid}: Cannot convert B{btype} time: {ts}"))
                    })?;
                }
                if btype == 200 {
                    if let Some(s) = get_str(item, "/Detector") {
                        b.text(written + 28, 24, s);
                    }
                } else {
                    if let Some(Value::Array(snr)) = item.pointer("/MEDSNR") {
                        for (i, v) in snr.iter().filter(|v| v.is_number()).take(6).enumerate() {
                            b.v[written + 28 + i] = v.as_f64().unwrap_or(0.0) as u8;
                        }
                    }
                    if let Some(v) = get_uint(item, "/MEDLookback").filter(|&v| v <= 255) {
                        b.v[written + 34] = v as u8;
                    }
                    if let Some(v) = get_uint(item, "/MEDPickAlgorithm").filter(|&v| v <= 255) {
                        b.v[written + 35] = v as u8;
                    }
                    if let Some(s) = get_str(item, "/Detector") {
                        b.text(written + 36, 24, s);
                    }
                }
                written += blen;
            }
        }

        // Blockettes 300, 310, 320, 390, 395, calibrations
        if let Some(Value::Array(arr)) = get("/FDSN/Calibration/Sequence") {
            for item in arr.iter().filter(|v| v.is_object()) {
                let (btype, blen) = match get_str(item, "/Type") {
                    Some(t) if prefix_ci(t, "STEP") => (300u16, 60usize),
                    Some(t) if prefix_ci(t, "SINE") => (310, 60),
                    Some(t) if prefix_ci(t, "PSEUDORANDOM") => (320, 64),
                    Some(t) if prefix_ci(t, "GENERIC") => (390, 28),
                    Some(t) if prefix_ci(t, "ABORT") => (395, 16),
                    Some(_) => (0, 0),
                    None if item.pointer("/EndTime").is_some() => (395, 16),
                    None => (0, 0),
                };
                if btype == 0 {
                    return Err(Error::Pack(format!(
                        "{sid}: Unknown or unset /FDSN/Calibration/Sequence/Type or /EndTime"
                    )));
                }
                if btype == 395 && get_str(item, "/EndTime").is_none() {
                    return Err(Error::Pack(format!(
                        "{sid}: Missing or non-string /FDSN/Calibration/Sequence/EndTime for B395"
                    )));
                }
                check(written, blen, &format!("B{btype}"))?;

                if btype != 395 {
                    b.u16(next_off, written as u16);
                    next_off = written + 2;
                    b.v[39] += 1;
                    b.u16(written, btype);
                    if let Some(ts) = get_str(item, "/BeginTime") {
                        put_btime(&mut b, written + 4, ts).map_err(|_| {
                            Error::Pack(format!("{sid}: Cannot convert B{btype} time: {ts}"))
                        })?;
                    }
                    let w = written;
                    let auto = get_str(item, "/Trigger").is_some_and(|s| prefix_ci(s, "AUTOMATIC"));
                    let cont = get_bool(item, "/Continued") == Some(true);
                    let dur = |p: &str| get_num(item, p).map(|n| (n * 10000.0 + 0.5) as u32);
                    let refamp = get_num(item, "/ReferenceAmplitude").map(|n| (n + 0.5) as u32);
                    match btype {
                        300 => {
                            if let Some(s) = get_uint(item, "/Steps").filter(|&s| s <= 255) {
                                b.v[w + 14] = s as u8;
                            }
                            if get_bool(item, "/StepFirstPulsePositive") == Some(true) {
                                b.v[w + 15] |= 0x01;
                            }
                            if get_bool(item, "/StepAlternateSign") == Some(true) {
                                b.v[w + 15] |= 0x02;
                            }
                            if auto {
                                b.v[w + 15] |= 0x04;
                            }
                            if cont {
                                b.v[w + 15] |= 0x08;
                            }
                            if let Some(d) = dur("/Duration") {
                                b.u32(w + 16, d);
                            }
                            if let Some(d) = dur("/StepBetween") {
                                b.u32(w + 20, d);
                            }
                            if let Some(n) = get_num(item, "/Amplitude") {
                                b.f32(w + 24, n as f32);
                            }
                            if let Some(s) = get_str(item, "/InputChannel") {
                                b.text(w + 28, 3, s);
                            }
                            if let Some(r) = refamp {
                                b.u32(w + 32, r);
                            }
                            if let Some(s) = get_str(item, "/Coupling") {
                                b.text(w + 36, 12, s);
                            }
                            if let Some(s) = get_str(item, "/Rolloff") {
                                b.text(w + 48, 12, s);
                            }
                        }
                        310 => {
                            if auto {
                                b.v[w + 15] |= 0x04;
                            }
                            if cont {
                                b.v[w + 15] |= 0x08;
                            }
                            if let Some(r) = get_str(item, "/AmplitudeRange") {
                                if prefix_ci(r, "PEAKTOPEAK") {
                                    b.v[w + 15] |= 0x10;
                                }
                                if prefix_ci(r, "ZEROTOPEAK") {
                                    b.v[w + 15] |= 0x20;
                                }
                                if prefix_ci(r, "RMS") {
                                    b.v[w + 15] |= 0x40;
                                }
                            }
                            if let Some(d) = dur("/Duration") {
                                b.u32(w + 16, d);
                            }
                            if let Some(n) = get_num(item, "/SinePeriod") {
                                b.f32(w + 20, n as f32);
                            }
                            if let Some(n) = get_num(item, "/Amplitude") {
                                b.f32(w + 24, n as f32);
                            }
                            if let Some(s) = get_str(item, "/InputChannel") {
                                b.text(w + 28, 3, s);
                            }
                            if let Some(r) = refamp {
                                b.u32(w + 32, r);
                            }
                            if let Some(s) = get_str(item, "/Coupling") {
                                b.text(w + 36, 12, s);
                            }
                            if let Some(s) = get_str(item, "/Rolloff") {
                                b.text(w + 48, 12, s);
                            }
                        }
                        320 => {
                            if auto {
                                b.v[w + 15] |= 0x04;
                            }
                            if cont {
                                b.v[w + 15] |= 0x08;
                            }
                            if get_str(item, "/AmplitudeRange")
                                .is_some_and(|s| prefix_ci(s, "RANDOM"))
                            {
                                b.v[w + 15] |= 0x10;
                            }
                            if let Some(d) = dur("/Duration") {
                                b.u32(w + 16, d);
                            }
                            if let Some(n) = get_num(item, "/Amplitude") {
                                b.f32(w + 20, n as f32);
                            }
                            if let Some(s) = get_str(item, "/InputChannel") {
                                b.text(w + 24, 3, s);
                            }
                            if let Some(r) = refamp {
                                b.u32(w + 28, r);
                            }
                            if let Some(s) = get_str(item, "/Coupling") {
                                b.text(w + 32, 12, s);
                            }
                            if let Some(s) = get_str(item, "/Rolloff") {
                                b.text(w + 44, 12, s);
                            }
                            if let Some(s) = get_str(item, "/Noise") {
                                b.text(w + 56, 8, s);
                            }
                        }
                        _ => {
                            // 390
                            if auto {
                                b.v[w + 15] |= 0x04;
                            }
                            if cont {
                                b.v[w + 15] |= 0x08;
                            }
                            if let Some(d) = dur("/Duration") {
                                b.u32(w + 16, d);
                            }
                            if let Some(n) = get_num(item, "/Amplitude") {
                                b.f32(w + 20, n as f32);
                            }
                            if let Some(s) = get_str(item, "/InputChannel") {
                                b.text(w + 24, 3, s);
                            }
                        }
                    }
                    written += blen;
                }

                // Blockette 395 when an end time is included
                if let Some(ts) = get_str(item, "/EndTime") {
                    check(written, 16, "B395")?;
                    b.u16(next_off, written as u16);
                    next_off = written + 2;
                    b.v[39] += 1;
                    b.u16(written, 395);
                    put_btime(&mut b, written + 4, ts).map_err(|_| {
                        Error::Pack(format!("{sid}: Cannot convert B395 time: {ts}"))
                    })?;
                    written += 16;
                }
            }
        }

        b.v.truncate(written);
        Ok(Header2 { bytes: b.v, b1000, b1001 })
    }

    /// Rewrite a parsed record in the given format, with this record's (possibly
    /// modified) header fields and extra headers around its original encoded data.
    ///
    /// For miniSEED 2 the record length is [`Record::reclen`] and the new header and the
    /// data must fit in it.
    pub fn repack(&self, format: FormatVersion) -> Result<Vec<u8>> {
        match format {
            FormatVersion::V3 => self.repack_v3(),
            FormatVersion::V2 => self.repack_v2(),
        }
    }

    fn header_params(&self) -> HeaderParams<'_> {
        HeaderParams {
            reclen: self.reclen.unwrap_or(PACK_DEFAULT_RECLEN),
            encoding: self.encoding.unwrap_or(PACK_DEFAULT_ENCODING),
            extra: &self.extra,
        }
    }

    fn repack_v3(&self) -> Result<Vec<u8>> {
        if self.raw.is_none() {
            return Err(Error::InvalidArgument(format!(
                "{}: Raw record is not available",
                self.sid
            )));
        }
        if self.sample_count > u32::MAX as u64 {
            return Err(Error::Pack(format!(
                "{}: Too many samples in input record ({}) for a single record",
                self.sid, self.sample_count
            )));
        }
        let mut rec = self.pack_header3(&self.header_params())?;
        let (origoffset, origsize) = self.data_bounds()?;
        let raw = self.raw.as_deref().expect("checked");
        rec.extend_from_slice(&raw[origoffset..origoffset + origsize]);

        rec[24..28].copy_from_slice(&(self.sample_count as u32).to_le_bytes());
        rec[36..40].copy_from_slice(&(origsize as u32).to_le_bytes());
        rec[28..32].fill(0);
        let crc = crc32c(&rec, 0);
        rec[28..32].copy_from_slice(&crc.to_le_bytes());
        Ok(rec)
    }

    fn repack_v2(&self) -> Result<Vec<u8>> {
        let sid = &self.sid;
        let Some(reclen) = self.reclen.filter(|&r| r > 0) else {
            return Err(Error::Pack(format!("{sid}: Record length is not set to a valid value")));
        };
        if self.raw.is_none() {
            return Err(Error::InvalidArgument(format!("{sid}: Raw record is not available")));
        }
        if self.sample_count > u16::MAX as u64 {
            return Err(Error::Pack(format!(
                "{sid}: Too many samples in input record ({}) for a single v2 record",
                self.sample_count
            )));
        }
        let header = self.pack_header2(&self.header_params())?.bytes;
        let headerlen = header.len();
        let (origoffset, origsize) = self.data_bounds()?;

        let dataoffset = if matches!(self.encoding, Some(Encoding::STEIM1) | Some(Encoding::STEIM2))
        {
            headerlen.div_ceil(64) * 64
        } else {
            headerlen
        };
        if dataoffset > u16::MAX as usize || dataoffset >= reclen {
            return Err(Error::Pack(format!(
                "{sid}: Repacked data offset ({dataoffset}) does not fit within record length ({reclen})"
            )));
        }
        let totalsize = dataoffset + origsize;
        if totalsize > reclen {
            return Err(Error::Pack(format!(
                "{sid}: Repacked record length ({totalsize}) is larger than the original record length ({reclen})"
            )));
        }
        let mut rec = vec![0u8; reclen];
        rec[..headerlen].copy_from_slice(&header);
        let raw = self.raw.as_deref().expect("checked");
        rec[dataoffset..totalsize].copy_from_slice(&raw[origoffset..origoffset + origsize]);
        rec[30..32].copy_from_slice(&(self.sample_count as u16).to_be_bytes());
        rec[44..46].copy_from_slice(&(dataoffset as u16).to_be_bytes());
        Ok(rec)
    }
}

// ---------------------------------------------------------------------------------------------
// Record packer
// ---------------------------------------------------------------------------------------------

/// Generates records from a [`Record`] holding data samples, one record per call.
///
/// The packer is an [`Iterator`] over `Result<Vec<u8>>` records. Unless
/// [`PackOptions::flush`] is set, a final partial record is not created.
pub struct RecordPacker<'a> {
    msr: Cow<'a, Record>,
    flush: bool,
    format_version: u8,
    maxreclen: usize,
    encoding: Encoding,
    order: ByteOrder,
    rawrec: Vec<u8>,
    dataoffset: usize,
    b1000: usize,
    b1001: usize,
    samplesize: usize,
    maxdatabytes: usize,
    maxsamples: usize,
    packed_samples: usize,
    record_count: usize,
    next_start_time: NsTime,
    finished: bool,
    failed: bool,
}

impl<'a> RecordPacker<'a> {
    /// Prepare to pack `msr` with `options`, see [`PackOptions`] for the defaults.
    pub fn new(msr: &'a Record, options: &PackOptions) -> Result<Self> {
        Self::from_cow(Cow::Borrowed(msr), options)
    }

    /// Like [`RecordPacker::new`] but taking ownership of the record, giving a packer that
    /// does not borrow.
    pub fn new_owned(msr: Record, options: &PackOptions) -> Result<RecordPacker<'static>> {
        RecordPacker::from_cow(Cow::Owned(msr), options)
    }

    fn from_cow(msr: Cow<'a, Record>, options: &PackOptions) -> Result<Self> {
        let rec: &Record = &msr;
        let maxreclen = options.record_length.or(rec.reclen).unwrap_or(PACK_DEFAULT_RECLEN);
        if !(MINRECLEN..=MAXRECLEN).contains(&maxreclen) {
            return Err(Error::Pack(format!(
                "{}: Record length is out of range: {maxreclen}",
                rec.sid
            )));
        }
        let format = options.format.or(rec.format_version).unwrap_or(FormatVersion::V3);
        let format_version = format.major();
        let encoding = resolve_encoding(rec.sample_type(), options.encoding.or(rec.encoding));
        let extra = options.extra_headers.as_deref().unwrap_or(&rec.extra);
        let params = HeaderParams { reclen: maxreclen, encoding, extra };
        let order = if format_version == 3 { ByteOrder::Little } else { ByteOrder::Big };

        if format_version == 3 && maxreclen < MS3FSDH_LENGTH + rec.sid.len() + extra.len() {
            return Err(Error::Pack(format!(
                "{}: Record length ({maxreclen}) is not large enough for header ({MS3FSDH_LENGTH}), SID ({}), and extra ({})",
                rec.sid,
                rec.sid.len(),
                extra.len()
            )));
        }

        let numsamples = rec.num_samples();
        let samplesize = if numsamples > 0 { rec.sample_type().map_or(0, |t| t.size()) } else { 0 };

        let mut rawrec = vec![0u8; maxreclen];
        let (mut b1000, mut b1001) = (0, 0);
        let mut dataoffset;
        if format_version == 3 {
            let h = rec.pack_header3(&params)?;
            dataoffset = h.len();
            rawrec[..dataoffset].copy_from_slice(&h);
        } else {
            let h = rec.pack_header2(&params)?;
            dataoffset = h.bytes.len();
            b1000 = h.b1000;
            b1001 = h.b1001;
            if dataoffset > maxreclen {
                return Err(Error::Pack(format!(
                    "{}: Header ({dataoffset}) does not fit within record length ({maxreclen})",
                    rec.sid
                )));
            }
            rawrec[..dataoffset].copy_from_slice(&h.bytes);
            if numsamples > 0 {
                if matches!(encoding, Encoding::STEIM1 | Encoding::STEIM2) {
                    dataoffset = dataoffset.div_ceil(64) * 64;
                }
                if dataoffset >= maxreclen || dataoffset > u16::MAX as usize {
                    return Err(Error::Pack(format!(
                        "{}: Data offset ({dataoffset}) does not fit within record length ({maxreclen})",
                        rec.sid
                    )));
                }
                rawrec[44..46].copy_from_slice(&(dataoffset as u16).to_be_bytes());
            }
        }

        let mut maxdatabytes = 0;
        let mut maxsamples = 0;
        if numsamples > 0 {
            maxdatabytes = maxreclen - dataoffset;
            maxsamples = max_samples(maxdatabytes, encoding, samplesize);
            if format_version == 2 && maxsamples > u16::MAX as usize {
                maxsamples = u16::MAX as usize;
            }
        }

        let next_start_time = rec.start_time;
        Ok(RecordPacker {
            msr,
            flush: options.flush,
            format_version,
            maxreclen,
            encoding,
            order,
            rawrec,
            dataoffset,
            b1000,
            b1001,
            samplesize,
            maxdatabytes,
            maxsamples,
            packed_samples: 0,
            record_count: 0,
            next_start_time,
            finished: false,
            failed: false,
        })
    }

    /// Pack all remaining samples, creating a final record that may not be filled.
    pub(crate) fn set_flush(&mut self) {
        self.flush = true;
    }

    /// Replace the samples being packed, e.g. after data was appended; already packed
    /// samples must remain at the front.
    pub(crate) fn resync_samples(&mut self, samples: Option<Samples>, sample_count: u64) {
        let m = self.msr.to_mut();
        m.samples = samples;
        m.sample_count = sample_count;
    }

    pub(crate) fn template(&self) -> &Record {
        &self.msr
    }

    /// Total number of samples packed so far.
    #[must_use]
    pub fn packed_samples(&self) -> usize {
        self.packed_samples
    }

    /// Number of records generated so far.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.record_count
    }

    /// Generate the next record, `Ok(None)` when finished.
    pub fn next_record(&mut self) -> Result<Option<Vec<u8>>> {
        let r = self.generate();
        if r.is_err() {
            self.failed = true;
        }
        r
    }

    fn generate(&mut self) -> Result<Option<Vec<u8>>> {
        if self.finished {
            return Ok(None);
        }
        let msr: &Record = &self.msr;
        let order = self.order;

        // Header-only record
        if msr.num_samples() == 0 {
            self.finished = true;
            self.record_count += 1;
            if self.format_version == 3 {
                let mut rec = self.rawrec[..self.dataoffset].to_vec();
                rec[15] = Encoding::TEXT.0;
                rec[24..28].fill(0);
                rec[36..40].fill(0);
                rec[28..32].fill(0);
                let crc = crc32c(&rec, 0);
                rec[28..32].copy_from_slice(&crc.to_le_bytes());
                return Ok(Some(rec));
            }
            let mut rec = self.rawrec.clone();
            if self.b1000 != 0 {
                rec[self.b1000 + 4] = Encoding::TEXT.0;
            }
            if self.dataoffset > 0 && self.dataoffset < self.maxreclen {
                rec[self.dataoffset..].fill(0);
            }
            return Ok(Some(rec));
        }

        let samples = msr.samples.as_ref().expect("num_samples > 0");
        let remaining = samples.len() - self.packed_samples;
        if remaining == 0 || (remaining < self.maxsamples && !self.flush) {
            self.finished = true;
            return Ok(None);
        }

        let mut to_pack = remaining;
        if self.format_version == 2 && to_pack > u16::MAX as usize {
            to_pack = u16::MAX as usize;
        }

        // Steim data is always big endian, other encodings follow the record format
        let data_order = if matches!(self.encoding, Encoding::STEIM1 | Encoding::STEIM2) {
            ByteOrder::Big
        } else {
            order
        };
        if self.maxdatabytes < min_bytes_needed(self.encoding, self.samplesize) {
            return Err(Error::Pack(format!(
                "{}: Not enough space in record ({}) for {} encoding",
                msr.sid, self.maxdatabytes, self.encoding
            )));
        }
        let enc = encode(
            samples,
            self.packed_samples,
            to_pack,
            self.encoding,
            self.maxdatabytes,
            data_order,
            0,
            &msr.sid,
        )
        .map_err(|e| e.context(format_args!("{}: Error packing data samples", msr.sid)))?;
        if enc.samples == 0 {
            self.finished = true;
            return Ok(None);
        }
        let datalength = enc.data.len();

        let mut rec = self.rawrec.clone();
        rec[self.dataoffset..self.dataoffset + datalength].copy_from_slice(&enc.data);

        if self.format_version == 3 {
            rec.truncate(self.dataoffset + datalength);
            rec[24..28].copy_from_slice(&(enc.samples as u32).to_le_bytes());
            rec[36..40].copy_from_slice(&(datalength as u32).to_le_bytes());
            if self.record_count > 0 {
                let t = self.next_start_time.parts();
                rec[4..8].copy_from_slice(&t.nsec.to_le_bytes());
                rec[8..10].copy_from_slice(&t.year.to_le_bytes());
                rec[10..12].copy_from_slice(&t.yday.to_le_bytes());
                rec[12] = t.hour;
                rec[13] = t.min;
                rec[14] = t.sec;
            }
            rec[28..32].fill(0);
            let crc = crc32c(&rec, 0);
            rec[28..32].copy_from_slice(&crc.to_le_bytes());
        } else {
            rec[30..32].copy_from_slice(&(enc.samples as u16).to_be_bytes());
            let content = self.dataoffset + datalength;
            if content < self.maxreclen {
                rec[content..].fill(0);
            }
            if self.record_count > 0 {
                let (second, fsec, usec) =
                    nstime_to_fsec_usec(self.next_start_time).map_err(|_| {
                        Error::Pack(format!("{}: Cannot convert record starttime", msr.sid))
                    })?;
                let t = second.parts();
                rec[20..22].copy_from_slice(&t.year.to_be_bytes());
                rec[22..24].copy_from_slice(&t.yday.to_be_bytes());
                rec[24] = t.hour;
                rec[25] = t.min;
                rec[26] = t.sec;
                rec[28..30].copy_from_slice(&fsec.to_be_bytes());
                if self.b1001 != 0 {
                    rec[self.b1001 + 5] = usec as u8;
                }
            }
        }

        self.packed_samples += enc.samples;
        self.record_count += 1;
        if self.packed_samples < samples.len() {
            self.next_start_time =
                msr.start_time.sample_time(self.packed_samples as u64, msr.sample_rate)?;
        } else {
            self.finished = true;
        }
        Ok(Some(rec))
    }
}

fn min_bytes_needed(encoding: Encoding, samplesize: usize) -> usize {
    match encoding {
        Encoding::STEIM1 | Encoding::STEIM2 => 64,
        Encoding::INT16 => 2,
        Encoding::INT32 | Encoding::FLOAT32 => 4,
        Encoding::FLOAT64 => 8,
        _ => samplesize.min(1),
    }
}

impl Iterator for RecordPacker<'_> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_record() {
            Ok(Some(r)) => Some(Ok(r)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Counts of what a packing call produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Packed {
    /// Number of records created.
    pub records: usize,
    /// Number of samples packed into them.
    pub samples: usize,
}

impl std::ops::AddAssign for Packed {
    fn add_assign(&mut self, other: Packed) {
        self.records += other.records;
        self.samples += other.samples;
    }
}

impl Record {
    /// Create a [`RecordPacker`] for this record.
    pub fn packer(&self, options: &PackOptions) -> Result<RecordPacker<'_>> {
        RecordPacker::new(self, options)
    }

    /// Pack this record's samples into records, calling `handler` with each.
    ///
    /// Packing stops at the first error from packing or from `handler`, which is returned.
    /// Without [`PackOptions::flush`] a final record that cannot be filled is not created,
    /// so fewer samples than held may be packed.
    pub fn pack_with(
        &self,
        options: &PackOptions,
        mut handler: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<Packed> {
        let mut packer = RecordPacker::new(self, options)?;
        let mut records = 0;
        while let Some(rec) = packer.next_record()? {
            handler(&rec)?;
            records += 1;
        }
        Ok(Packed { records, samples: packer.packed_samples() })
    }

    /// Pack this record's samples into a vector of records.
    pub fn pack(&self, options: &PackOptions) -> Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        self.pack_with(options, |r| {
            out.push(r.to_vec());
            Ok(())
        })?;
        Ok(out)
    }
}

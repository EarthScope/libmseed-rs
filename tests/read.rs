//! Port of libmseed test/test-read.c and test/test-msrutils.c
//!
//! Not ported: `read.stdin_no_close` (exercises C file-descriptor ownership of stdin, which
//! has no Rust equivalent: `MsReader` takes any `Read`, and dropping it never closes stdin).

mod common;

use common::*;
use libmseed::reader::{MsReader, split_path_range};
use libmseed::selection::Selections;
use libmseed::{
    Encoding, Error, FormatVersion, NsTime, PackOptions, ParseFlags, Record, RecordFlags, Samples,
};

fn read_first(name: &str, flags: ParseFlags) -> Record {
    let mut r = MsReader::open(data(name), flags).unwrap();
    r.next_record().unwrap().expect("a record")
}

fn t(s: &str) -> NsTime {
    NsTime::parse(s).unwrap()
}

#[test]
fn v3_parse() {
    let nstime = t("2010-02-27T06:50:00.069539Z");
    let msr = read_first(
        "testdata-3channel-signal.mseed3",
        ParseFlags::UNPACK_DATA | ParseFlags::VALIDATE_CRC,
    );
    assert_eq!(msr.reclen, Some(414));
    assert_eq!(msr.sid, "FDSN:IU_COLA_00_L_H_1");
    assert_eq!(msr.format_version, Some(libmseed::FormatVersion::V3));
    assert_eq!(msr.flags, RecordFlags::CLOCK_LOCKED);
    assert_eq!(msr.start_time, nstime);
    assert_eq!(msr.sample_rate, 1.0);
    assert_eq!(msr.encoding, Some(Encoding::STEIM2));
    assert_eq!(msr.pub_version, 4);
    assert_eq!(msr.sample_count, 135);
    assert_eq!(msr.crc, 0xCE52C9F7);
    assert_eq!(msr.extra.len(), 33);
    assert_eq!(msr.data_length, 320);
    assert_eq!(msr.extra, "{\"FDSN\":{\"Time\":{\"Quality\":100}}}");
    assert_eq!(msr.num_samples(), 135);
    let s = msr.samples.as_ref().unwrap().as_i32().unwrap();
    assert_eq!(&s[..4], &[-502676, -504105, -507491, -506991]);
    assert_eq!(&s[131..], &[-505212, -499533, -495590, -496168]);
}

#[test]
fn v2_parse() {
    let nstime = t("2010-02-27T06:50:00.069539Z");
    let msr = read_first("testdata-3channel-signal.mseed2", ParseFlags::UNPACK_DATA);
    assert_eq!(msr.reclen, Some(512));
    assert_eq!(msr.sid, "FDSN:IU_COLA_00_L_H_1");
    assert_eq!(msr.format_version, Some(libmseed::FormatVersion::V2));
    assert_eq!(msr.flags, RecordFlags::CLOCK_LOCKED);
    assert_eq!(msr.start_time, nstime);
    assert_eq!(msr.sample_rate, 1.0);
    assert_eq!(msr.encoding, Some(Encoding::STEIM2));
    assert_eq!(msr.pub_version, 4);
    assert_eq!(msr.sample_count, 135);
    assert_eq!(msr.crc, 0);
    assert_eq!(msr.extra.len(), 33);
    assert_eq!(msr.data_length, 448);
    assert_eq!(msr.extra, "{\"FDSN\":{\"Time\":{\"Quality\":100}}}");
    assert_eq!(msr.num_samples(), 135);
    let s = msr.samples.as_ref().unwrap().as_i32().unwrap();
    assert_eq!(&s[..4], &[-502676, -504105, -507491, -506991]);
    assert_eq!(&s[131..], &[-505212, -499533, -495590, -496168]);
}

#[test]
fn headeronly_v2() {
    let msr = read_first("reference-testdata-headeronly.mseed2", ParseFlags::empty());
    assert!(msr.samples.is_none());
    assert_eq!(msr.num_samples(), 0);
    assert!(msr.sample_type().is_none());
    assert_eq!(msr.data_length, 0);
    assert_eq!(msr.extra.len(), 569);
    assert_eq!(msr.crc, 0);
    // Exercise the diagnostic printer
    assert!(format!("{msr:#}").contains("start time"));
}

#[test]
fn headeronly_v3() {
    let msr = read_first("reference-testdata-headeronly.mseed3", ParseFlags::empty());
    assert!(msr.samples.is_none());
    assert_eq!(msr.num_samples(), 0);
    assert_eq!(msr.data_length, 0);
    assert_eq!(msr.extra.len(), 730);
    assert_eq!(msr.crc, 0xC22273A9);
}

#[test]
fn no_raw() {
    for name in ["testdata-3channel-signal.mseed2", "testdata-3channel-signal.mseed3"] {
        let full = read_first(name, ParseFlags::UNPACK_DATA);
        assert!(full.raw().is_some());

        let mut hdr = read_first(name, ParseFlags::NO_RAW);
        assert!(hdr.raw().is_none());
        assert!(hdr.samples.is_none());
        assert_eq!((&hdr.sid, hdr.start_time, hdr.sample_count), (&full.sid, full.start_time, 135));
        assert_eq!((hdr.reclen, hdr.crc, &hdr.extra), (full.reclen, full.crc, &full.extra));
        assert!(hdr.unpack_data().is_err());

        let unpacked = read_first(name, ParseFlags::NO_RAW | ParseFlags::UNPACK_DATA);
        assert!(unpacked.raw().is_none());
        assert_eq!(unpacked.samples, full.samples);

        // Unpacking deferred until after selection
        let mut sel = Selections::new();
        sel.add("FDSN:IU_COLA_00_L_H_1", None, None, None).unwrap();
        let mut r = MsReader::open(data(name), ParseFlags::NO_RAW | ParseFlags::UNPACK_DATA)
            .unwrap()
            .with_selections(sel);
        let selected = r.next_record().unwrap().expect("a record");
        assert!(selected.raw().is_none());
        assert_eq!(selected.samples, full.samples);
    }
}

fn check_encodings(ext: &str, with_le: bool) {
    let flags = ParseFlags::UNPACK_DATA;
    let isine = isinedata();
    let fsine = fsinedata();
    let file = |n: &str| format!("reference-testdata-{n}.{ext}");

    let msr = read_first(&file("text"), flags);
    assert!(msr.samples.as_ref().unwrap().as_text().unwrap().starts_with(TEXTDATA.as_bytes()));

    // Files hold the leading portion of the series, compare as many samples as decoded
    let msr = read_first(&file("float32"), flags);
    let got = msr.samples.as_ref().unwrap().as_f32().unwrap();
    assert!(got.len() > 10 && got == &fsine[..got.len()], "float32");

    let msr = read_first(&file("float64"), flags);
    let got = msr.samples.as_ref().unwrap().as_f64().unwrap();
    assert!(got.len() > 10 && got == &DSINEDATA[..got.len()], "float64 len {}", got.len());

    let mut names = vec!["int16", "int32", "steim1", "steim2"];
    if with_le {
        names.extend(["steim1-LE", "steim2-LE"]);
    }
    for n in names {
        let msr = read_first(&file(n), flags);
        let got = msr.samples.as_ref().unwrap().as_i32().unwrap();
        assert!(got.len() > 10 && got == &isine[..got.len()], "{n}");
    }
}

#[test]
fn v3_encodings() {
    check_encodings("mseed3", false);
}

#[test]
fn v2_encodings() {
    check_encodings("mseed2", true);

    let flags = ParseFlags::UNPACK_DATA;
    let first4 = |name: &str| -> Vec<i32> {
        let msr = read_first(name, flags);
        msr.samples.unwrap().as_i32().unwrap()[..4].to_vec()
    };
    assert_eq!(first4("testdata-encoding-CDSN.mseed2"), [-96, -87, -100, -128]);
    assert_eq!(first4("testdata-encoding-DWWSSN.mseed2"), [6, 5, 1, -9]);
    assert_eq!(first4("testdata-encoding-SRO.mseed2"), [39, 42, 32, 1]);

    let msr = read_first("testdata-encoding-GEOSCOPE-16bit-3exp-encoded.mseed2", flags);
    let f = msr.samples.unwrap();
    assert_eq!(&f.as_f32().unwrap()[..4], &[-1.0625, -1.078125, -1.078125, -1.078125]);
}

#[test]
fn byterange() {
    let nstime = t("2010-02-27T06:51:04.069539Z");
    let flags = ParseFlags::UNPACK_DATA;
    let base = data("testdata-oneseries-mixedlengths-mixedorder.mseed3");
    let path = |suffix: &str| format!("{}{suffix}", base.display());

    let mut r = MsReader::open_path_range(&path("@9428-9967"), flags).unwrap();
    let msr = r.next_record().unwrap().unwrap();
    assert_eq!(msr.num_samples(), 112);
    assert_eq!(msr.start_time, nstime);

    let v2 = data("testdata-oneseries-mixedlengths-mixedorder.mseed2");
    let mut r = MsReader::open_path_range(&format!("{}@9344-9855", v2.display()), flags).unwrap();
    let msr = r.next_record().unwrap().unwrap();
    assert_eq!(msr.num_samples(), 112);
    assert_eq!(msr.start_time, nstime);

    // Oversized offsets are not recognized as a range, so the path is not found
    assert!(MsReader::open_path_range(&path("@-9999999999999999999"), flags).is_err());
    assert!(MsReader::open_path_range(&path("@9999999999999999999-"), flags).is_err());

    // INT64_MAX itself is still a valid start offset: the range is parsed
    let (p, s, e) = split_path_range("file@9223372036854775807-");
    assert_eq!((p, s, e), ("file", Some(i64::MAX as u64), None));
    assert_eq!(split_path_range("file@9223372036854775808-").0, "file@9223372036854775808-");
    assert_eq!(split_path_range("file@"), ("file@", None, None));
    assert_eq!(split_path_range("file@-"), ("file@-", None, None));
    assert_eq!(split_path_range("file@1-2-3").0, "file@1-2-3");
    assert_eq!(split_path_range("file@-7"), ("file", None, Some(7)));

    // A start offset past the end of the file reads no data
    let mut r = MsReader::open_path_range(&path("@1000000-"), flags).unwrap();
    assert!(matches!(r.next_record(), Err(Error::NotSeed(_))));

    // An end offset of INT64_MAX must not overflow
    let mut r = MsReader::open_path_range(&path("@0-9223372036854775807"), flags).unwrap();
    assert!(r.next_record().unwrap().unwrap().num_samples() > 0);
}

#[test]
fn byterange_init() {
    let nstime = t("2010-02-27T06:51:04.069539Z");
    let flags = ParseFlags::UNPACK_DATA;

    let mut r = MsReader::open_range(
        data("testdata-oneseries-mixedlengths-mixedorder.mseed3"),
        flags,
        9428..=9967,
    )
    .unwrap();
    let msr = r.next_record().unwrap().unwrap();
    assert_eq!(msr.num_samples(), 112);
    assert_eq!(msr.start_time, nstime);
    assert_eq!(r.last_record_offset(), 9428);

    let mut r = MsReader::open_range(
        data("testdata-oneseries-mixedlengths-mixedorder.mseed2"),
        flags,
        9344..=9855,
    )
    .unwrap();
    let msr = r.next_record().unwrap().unwrap();
    assert_eq!(msr.num_samples(), 112);
    assert_eq!(msr.start_time, nstime);

    // A generic (non-seekable) stream discards bytes up to the start
    let bytes = std::fs::read(data("testdata-oneseries-mixedlengths-mixedorder.mseed3")).unwrap();
    let mut r = MsReader::new(&bytes[..], flags).with_byte_range(9428..=9967);
    let msr = r.next_record().unwrap().unwrap();
    assert_eq!(msr.start_time, nstime);
}

#[test]
fn selection() {
    let nstime = t("2010-02-27T06:50:00.069539Z");
    let mut sel = Selections::new();
    sel.add("FDSN:IU_COLA_*_L_H_Z", None, None, None).unwrap();

    let mut r = MsReader::open(data("testdata-3channel-signal.mseed3"), ParseFlags::UNPACK_DATA)
        .unwrap()
        .with_selections(sel);
    let msr = r.next_record().unwrap().unwrap();
    assert_eq!(msr.num_samples(), 112);
    assert_eq!(msr.start_time, nstime);

    // Draining the rest ends normally, other channels are skipped along the way
    while r.next_record().unwrap().is_some() {}
}

#[test]
fn selection_nomatch() {
    let mut sel = Selections::new();
    sel.add("FDSN:XX_NOSUCH_*_B_H_Z", None, None, None).unwrap();
    let mut r = MsReader::open(data("testdata-3channel-signal.mseed3"), ParseFlags::UNPACK_DATA)
        .unwrap()
        .with_selections(sel);
    // Records are parsed and skipped, which is not the same as the input not being SEED
    assert!(r.next_record().unwrap().is_none());
}

#[test]
fn oddball() {
    let flags = ParseFlags::UNPACK_DATA;

    // Detection record: includes an event detection and no other data
    let msr = read_first("testdata-detection.record.mseed2", flags);
    let eh = msr.extra_headers().unwrap();
    assert!(eh.exists("/FDSN/Event/Detection/0"));
    assert_eq!(
        eh.get_str("/FDSN/Event/Detection/0/OnsetTime").unwrap().unwrap(),
        "2004-07-28T20:28:06.185000Z"
    );

    // Unapplied time correction
    let msr = read_first("testdata-unapplied-timecorrection.mseed2", flags);
    assert_eq!(msr.start_time, t("2003-05-29T02:13:23.043400Z"));

    // No Blockette 1000 with Steim-1 assumption needed
    let msr = read_first("testdata-no-blockette1000-steim1.mseed2", flags);
    assert_eq!(msr.sample_count, 3632);
    assert_eq!(msr.num_samples(), 3632);
    let s = msr.samples.as_ref().unwrap().as_i32().unwrap();
    assert_eq!(&s[3628..], &[309, 211, 117, 26]);

    // Invalid blockette chain: offset falls within the fixed header
    let mut r = MsReader::open(data("testdata-invalid-blockette-offsets.mseed2"), flags).unwrap();
    assert!(matches!(r.next_record(), Err(Error::InvalidRecord(_))));
}

#[test]
fn read_error() {
    // Non-existent file
    assert!(matches!(
        MsReader::open("no/such/file.data", ParseFlags::empty()),
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound
    ));
    // Not miniSEED
    let mut r =
        MsReader::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"), ParseFlags::empty())
            .unwrap();
    assert!(matches!(r.next_record(), Err(Error::NotSeed(_))));
    // Iteration ends after the error
    assert!(r.next().is_none());
}

#[test]
fn skip_not_data() {
    let mut bytes = b"this is not miniSEED data, skip it".to_vec();
    bytes.extend(std::fs::read(data("testdata-3channel-signal.mseed3")).unwrap());
    let expected = MsReader::open(data("testdata-3channel-signal.mseed3"), ParseFlags::empty())
        .unwrap()
        .count();
    let count = MsReader::new(&bytes[..], ParseFlags::SKIP_NOT_DATA).filter(|r| r.is_ok()).count();
    assert!(expected > 0);
    assert_eq!(count, expected);
    // Without the flag the leading junk is an error
    assert!(matches!(
        MsReader::new(&bytes[..], ParseFlags::empty()).next(),
        Some(Err(Error::NotSeed(_)))
    ));
}

/// Ported from the C test `msr3_utils`.
#[test]
fn record_utils() {
    let msr = read_first("testdata-3channel-signal.mseed3", ParseFlags::UNPACK_DATA);
    let dup = msr.clone();

    assert_eq!(msr.reclen, dup.reclen);
    assert_eq!(msr.sid, dup.sid);
    assert_eq!(msr.format_version, dup.format_version);
    assert_eq!(msr.flags, dup.flags);
    assert_eq!(msr.start_time, dup.start_time);
    assert_eq!(msr.sample_rate, dup.sample_rate);
    assert_eq!(msr.encoding, dup.encoding);
    assert_eq!(msr.pub_version, dup.pub_version);
    assert_eq!(msr.sample_count, dup.sample_count);
    assert_eq!(msr.crc, dup.crc);
    assert_eq!(msr.extra, dup.extra);
    assert_eq!(msr.data_length, dup.data_length);
    assert_eq!(msr.samples, dup.samples);

    let s = dup.samples.as_ref().unwrap().as_i32().unwrap();
    assert_eq!(&s[..4], &[-502676, -504105, -507491, -506991]);
    assert_eq!(&s[131..], &[-505212, -499533, -495590, -496168]);

    assert_eq!(dup.end_time().unwrap(), t("2010-02-27T06:52:14.069539Z"));
    assert_eq!(dup.sample_rate_hz(), 1.0);
    assert_eq!(dup.period_nanos(), 1_000_000_000);
}

/// Ported from the C test `msr3_duplicate_extra`: a header copy keeps extra headers but
/// not samples or raw bytes.
#[test]
fn header_copy() {
    let msr = read_first("testdata-3channel-signal.mseed3", ParseFlags::UNPACK_DATA);
    assert!(!msr.extra.is_empty());
    assert!(msr.raw().is_some());

    let d = msr.header_copy();
    assert_eq!(d.extra, msr.extra);
    assert_eq!(d.start_time, msr.start_time);
    assert_eq!(d.sample_count, msr.sample_count);
    assert!(d.samples.is_none());
    assert!(d.raw().is_none());
}

#[test]
fn v2_truncated_header_is_incomplete() {
    let bytes = std::fs::read(data("testdata-3channel-signal.mseed2")).unwrap();
    for length in 40..48 {
        let r = Record::parse(&bytes[..length], ParseFlags::empty());
        assert!(matches!(r, Err(Error::Incomplete { needed: 40 })), "length {length}: {r:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Tests that patch a packed record
// ---------------------------------------------------------------------------------------------

fn pack_one(version2: bool, reclen: usize) -> Vec<u8> {
    let mut msr = Record::new("FDSN:XX_TEST__L_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(reclen);
    msr.pub_version = 1;
    msr.start_time = t("2020-01-01T00:00:00Z");
    msr.sample_rate = 1.0;
    let format = if version2 { FormatVersion::V2 } else { FormatVersion::V3 };
    let recs =
        msr.pack(&PackOptions { format: Some(format), flush: true, ..Default::default() }).unwrap();
    assert_eq!(recs.len(), 1);
    recs.into_iter().next().unwrap()
}

/// A version 3 record with a non-finite fixed-header sample rate is rejected.
#[test]
fn v3_invalid_samplerate() {
    let mut rec = pack_one(false, 512);
    rec[16..24].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(Record::parse(&rec, ParseFlags::empty()).is_err());
}

/// A version 2 BTIME fraction above 9999 is rejected rather than wrapped into range.
#[test]
fn v2_btime_fraction_out_of_range() {
    let mut rec = pack_one(true, 128);
    assert!(Record::parse(&rec, ParseFlags::empty()).is_ok());
    rec[28..30].copy_from_slice(&10000u16.to_be_bytes());
    assert!(Record::parse(&rec, ParseFlags::empty()).is_err());
}

/// A Blockette 100 with an invalid (negative) sample rate is ignored.
#[test]
fn v2_b100_invalid_samplerate() {
    let mut rec = pack_one(true, 128);
    let b100 = 48 + 8; // Fixed header + Blockette 1000
    assert!(rec.len() >= b100 + 12);
    // Splice a Blockette 100 with a negative rate after the mandatory Blockette 1000
    rec[48 + 2..48 + 4].copy_from_slice(&(b100 as u16).to_be_bytes());
    rec[39] += 1;
    rec[b100..b100 + 2].copy_from_slice(&100u16.to_be_bytes());
    rec[b100 + 2..b100 + 4].copy_from_slice(&0u16.to_be_bytes());
    rec[b100 + 4..b100 + 8].copy_from_slice(&(-5.0f32).to_be_bytes());
    rec[b100 + 8..b100 + 12].fill(0);

    let parsed = Record::parse(&rec, ParseFlags::empty()).unwrap();
    assert_eq!(parsed.sample_rate, 1.0, "invalid Blockette 100 sample rate was not ignored");
}

/// Pack a single miniSEED 3 Steim-2 record of a ramp with the given extra headers.
fn packed_v3_with_extra(extra: &str) -> (Vec<u8>, Vec<i32>) {
    let samples: Vec<i32> = (0..200).collect();
    let mut record = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    record.start_time = NsTime::parse("2024-01-01T00:00:00Z").unwrap();
    record.sample_rate = 40.0;
    record.encoding = Some(Encoding::STEIM2);
    record.reclen = Some(1024);
    record.extra = extra.to_string();
    record.samples = Some(Samples::Int32(samples.clone()));
    let mut packed = record.pack(&PackOptions { flush: true, ..Default::default() }).unwrap();
    assert_eq!(packed.len(), 1);
    (packed.remove(0), samples)
}

/// The data offset must come from the header lengths, not the decoded extra header string,
/// which differs when the extra header bytes are not valid UTF-8.
#[test]
fn v3_data_offset_ignores_lossy_extra() {
    let (mut bytes, samples) = packed_v3_with_extra(r#"{"X":"ab"}"#);
    // Replace "ab" with an invalid UTF-8 byte pair; lossy decoding grows the string
    let extra_start = 40 + bytes[33] as usize;
    let pos = extra_start + bytes[extra_start..].windows(2).position(|w| w == b"ab").unwrap();
    bytes[pos] = 0xFF;
    bytes[pos + 1] = 0xFE;

    let rec = Record::parse(&bytes, ParseFlags::UNPACK_DATA).unwrap();
    assert_ne!(rec.extra.len(), u16::from_le_bytes([bytes[34], bytes[35]]) as usize);
    assert_eq!(rec.samples.unwrap().as_i32().unwrap(), &samples[..]);
}

/// Editing extra headers of a parsed record must not change where its data is read from.
#[test]
fn v3_unpack_after_editing_extra() {
    let (bytes, samples) = packed_v3_with_extra(r#"{"X":1}"#);
    let mut rec = Record::parse(&bytes, ParseFlags::empty()).unwrap();
    rec.set_extra("/FDSN/Longer/Value", serde_json::json!("lengthen the extra headers")).unwrap();
    rec.unpack_data().unwrap();
    assert_eq!(rec.samples.unwrap().as_i32().unwrap(), &samples[..]);
}

/// A Steim record claiming far more samples than its frames can hold is rejected without
/// allocating for the claimed count.
#[test]
fn steim_huge_sample_count_rejected() {
    let (mut bytes, _) = packed_v3_with_extra("");
    bytes[24..28].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
    let err = Record::parse(&bytes, ParseFlags::UNPACK_DATA).unwrap_err();
    assert!(err.to_string().contains("not large enough"), "{err}");
}

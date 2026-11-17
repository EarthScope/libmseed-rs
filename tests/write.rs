//! Port of libmseed test/test-write.c (single record tests) and test/test-repack.c

mod common;

use common::*;
use libmseed::codec::Samples;
use libmseed::reader::MsReader;
use libmseed::{
    Encoding, Error, FormatVersion, NsTime, PackOptions, ParseFlags, Record, TraceList,
};
use std::path::Path;

fn t(s: &str) -> NsTime {
    NsTime::parse(s).unwrap()
}

/// Compare a written file with its reference in tests/data.
fn assert_matches_reference(written: &Path, name: &str) {
    let got = std::fs::read(written).unwrap();
    let want = std::fs::read(data(&format!("reference-{name}"))).unwrap();
    assert!(
        got == want,
        "{name}: output differs from reference ({} vs {} bytes)",
        got.len(),
        want.len()
    );
}

const SOH_JSON_HEADERS: &str = r#"{"FDSN":{"Time":{"Exception":[{"Time":"2012-01-13T00:00:00Z","VCOCorrection":51.51367,"ReceptionQuality":0,"Count":6829,"Type":"Daily Timemark","ClockStatus":"SNR=48,49,48,47,51,48,51,50,47,48,46"},{"Time":"2012-01-13T00:03:16.000001Z","VCOCorrection":51.51367,"ReceptionQuality":0,"Count":196,"Type":"UnExp Timemark","ClockStatus":"Jump of -0.999999 Seconds"},{"Time":"2012-01-13T00:03:36.000004Z","VCOCorrection":51.51367,"ReceptionQuality":90,"Count":21,"Type":"Valid Timemark","ClockStatus":"SNR=47,50,47,49,50,48,52,49,48,46,46"}]},"Clock":{"Model":"P273T11N16"}},"Manufacturer123":{"Metadata":{"FilamentCurrent":16.4,"HyperCoordinates":"1.1789:965402:73324@3.14159"}},"OperatorXYZ":{"DSP":{"PeakRMS":2067,"RMSWindow":10.5}}}"#;

/// Ported from the C test `msr3_writemseed_encodings`.
#[test]
fn write_encodings() {
    let dir = tempfile::tempdir().unwrap();
    let isine = isinedata();
    let fsine = fsinedata();

    let mut msr = Record::new("FDSN:XX_TEST__L_O_G", NsTime::UNIX_EPOCH);
    msr.reclen = Some(512);
    msr.pub_version = 1;
    msr.start_time = t("2012-05-12T00:00:00");

    for (ext, options) in [("mseed3", flush()), ("mseed2", flush_v2())] {
        let check = |msr: &Record, name: &str| {
            let path = dir.path().join(format!("testdata-{name}.{ext}"));
            let rv = msr.write_file(&path, &options).unwrap();
            assert!(rv.records > 0);
            assert_matches_reference(&path, &format!("testdata-{name}.{ext}"));
        };

        // Text
        msr.sid = "FDSN:XX_TEST__L_O_G".into();
        msr.sample_rate = 0.0;
        msr.encoding = Some(Encoding::TEXT);
        msr.samples = Some(Samples::Text(TEXTDATA.as_bytes().to_vec()));
        check(&msr, "text");

        msr.sid = "FDSN:XX_TEST__B_H_Z".into();
        msr.sample_rate = 40.0;

        msr.encoding = Some(Encoding::FLOAT32);
        msr.samples = Some(Samples::Float32(fsine.clone()));
        check(&msr, "float32");

        msr.encoding = Some(Encoding::FLOAT64);
        msr.samples = Some(Samples::Float64(DSINEDATA.to_vec()));
        check(&msr, "float64");

        // Limit to the first 220 samples, which can be represented in 16 bits
        msr.encoding = Some(Encoding::INT16);
        msr.samples = Some(Samples::Int32(isine[..220].to_vec()));
        check(&msr, "int16");

        msr.encoding = Some(Encoding::INT32);
        msr.samples = Some(Samples::Int32(isine.clone()));
        check(&msr, "int32");

        msr.encoding = Some(Encoding::STEIM1);
        msr.samples = Some(Samples::Int32(isine.clone()));
        check(&msr, "steim1");

        // All but the last sample, for which the difference cannot be represented
        msr.encoding = Some(Encoding::STEIM2);
        msr.samples = Some(Samples::Int32(isine[..isine.len() - 1].to_vec()));
        check(&msr, "steim2");

        // Default encoding (Steim2) and record length (4096)
        msr.encoding = None;
        msr.reclen = None;
        check(&msr, "defaults");

        // Reset for the next format
        msr.start_time = t("2012-05-12T00:00:00");
        msr.reclen = Some(512);
    }
}

/// Ported from the C test `msr3_writemseed_headeronly`.
#[test]
fn write_headeronly() {
    let dir = tempfile::tempdir().unwrap();
    for (ext, options) in [("mseed2", pack_opts(true, false)), ("mseed3", PackOptions::default())] {
        let mut msr = Record::new("FDSN:XX_TEST__S_O_H", NsTime::UNIX_EPOCH);
        msr.reclen = Some(4096);
        msr.pub_version = 1;
        msr.start_time = t("2012-05-12T00:00:00");
        msr.sample_rate = 0.0;
        msr.extra = SOH_JSON_HEADERS.to_string();
        msr.sample_count = 0;

        let path = dir.path().join(format!("testdata-headeronly.{ext}"));
        let rv = msr.write_file(&path, &options).unwrap();
        assert!(rv.records > 0);
        assert_matches_reference(&path, &format!("testdata-headeronly.{ext}"));
    }
}

const NSEC_EXTRA: &str = concat!(
    r#"{"FDSN":{"Time":{"Exception":[{"Time":"2012-05-12T00:00:26.987654321Z","#,
    r#""VCOCorrection":50.7080078125,"ReceptionQuality":100,"Count":7654,"Type":"Valid","#,
    r#""ClockStatus":"Drift=-1973usec, Satellite SNR in dB=23, 0, 26, 25, 29, 28"}]},"#,
    r#""Clock":{"Model":"Acme Corporation GPS3"}}}"#
);

/// Ported from the C test `msr3_writemseed_nanosecond`.
#[test]
fn write_nanosecond() {
    let dir = tempfile::tempdir().unwrap();
    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.sample_rate = 40.0;
    msr.pub_version = 1;
    msr.start_time = t("2012-05-12T00:00:00.123456789Z");
    msr.encoding = Some(Encoding::INT32);
    msr.reclen = Some(512);
    msr.samples = Some(Samples::Int32(isinedata()));
    msr.extra = NSEC_EXTRA.to_string();

    for (ver, ext) in [(FormatVersion::V3, "mseed3"), (FormatVersion::V2, "mseed2")] {
        msr.format_version = Some(ver);
        let path = dir.path().join(format!("testdata-nsec.{ext}"));
        assert!(msr.write_file(&path, &flush()).unwrap().records > 0);
        assert_matches_reference(&path, &format!("testdata-nsec.{ext}"));
    }
}

/// Ported from the C test `msr3_writemseed_olden`.
#[test]
fn write_olden() {
    let dir = tempfile::tempdir().unwrap();
    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.sample_rate = 40.0;
    msr.pub_version = 1;
    msr.start_time = t("1964-03-27T21:11:24.987654321Z");
    msr.encoding = Some(Encoding::INT32);
    msr.reclen = Some(4096);
    msr.samples = Some(Samples::Int32(isinedata()));
    msr.extra = concat!(
        r#"{"FDSN":{"Time":{"Exception":[{"Time":"1964-03-27T21:11:48.123456789Z","Count":1,"#,
        r#""Type":"Unexpected","ClockStatus":"Clock tower destroyed"}]},"#,
        r#""Clock":{"Model":"Ye Olde Clock Tower Company"}}}"#
    )
    .to_string();

    for (ver, ext) in [(FormatVersion::V3, "mseed3"), (FormatVersion::V2, "mseed2")] {
        msr.format_version = Some(ver);
        let path = dir.path().join(format!("testdata-olden.{ext}"));
        assert!(msr.write_file(&path, &flush()).unwrap().records > 0);
        assert_matches_reference(&path, &format!("testdata-olden.{ext}"));
    }
}

/// Ported from the C test `msr3_writemseed_oddrate`.
#[test]
fn write_oddrate() {
    let dir = tempfile::tempdir().unwrap();
    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.pub_version = 1;
    msr.start_time = t("2025-05-12T21:11:24.987654321Z");
    msr.encoding = Some(Encoding::INT32);
    msr.reclen = Some(512);
    msr.samples = Some(Samples::Int32(isinedata()));
    // Odd rate with a repeating decimal period
    msr.sample_rate = 1080.0;

    for (ver, ext) in [(FormatVersion::V3, "mseed3"), (FormatVersion::V2, "mseed2")] {
        msr.format_version = Some(ver);
        let path = dir.path().join(format!("testdata-oddrate.{ext}"));
        assert!(msr.write_file(&path, &flush()).unwrap().records > 0);
        assert_matches_reference(&path, &format!("testdata-oddrate.{ext}"));
    }
}

/// v2 continuation records must carry a 100 microsecond rounding carry into the next second.
/// Ported from the C test `msr3_writemseed_v2_continuation_timecarry`.
#[test]
fn write_v2_continuation_timecarry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testdata-timecarry.mseed2");
    let start = t("2012-01-01T00:00:00.999980Z");

    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(128); // Small reclen forces many continuation records
    msr.pub_version = 1;
    msr.sample_rate = 1.0;
    msr.encoding = Some(Encoding::INT32);
    msr.sample_count = 500;
    msr.samples = Some(Samples::Int32(isinedata()));
    msr.start_time = start;
    assert!(msr.write_file(&path, &flush_v2()).unwrap().records > 0);

    let mut cumulative = 0;
    let mut reccount = 0;
    for rec in MsReader::open(&path, ParseFlags::empty()).unwrap() {
        let rec = rec.unwrap();
        assert_eq!(
            rec.start_time,
            start.sample_time(cumulative, 1.0).unwrap(),
            "record {reccount} start time (continuation time carry)"
        );
        cumulative += rec.sample_count;
        reccount += 1;
    }
    assert!(reccount > 1, "continuation path not exercised");
}

/// A v2 Blockette 500 time within 50 microseconds of the next second round-trips.
/// Ported from the C test `msr3_writemseed_v2_btime_timecarry`.
#[test]
fn write_v2_btime_timecarry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testdata-btimecarry.mseed2");

    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(512);
    msr.pub_version = 1;
    msr.start_time = t("2012-06-01T00:00:00Z");
    msr.sample_rate = 1.0;
    msr.encoding = Some(Encoding::INT32);
    msr.sample_count = 4;
    msr.samples = Some(Samples::Int32(vec![1, 2, 3, 4]));
    msr.extra =
        r#"{"FDSN":{"Time":{"Exception":[{"Time":"2012-06-01T12:00:00.999980Z"}]}}}"#.into();
    assert!(msr.write_file(&path, &flush_v2()).unwrap().records > 0);

    let rec = MsReader::open(&path, ParseFlags::empty()).unwrap().next_record().unwrap().unwrap();
    let eh = rec.extra_headers().unwrap();
    let got = eh.get_str("/FDSN/Time/Exception/0/Time").unwrap().expect("B500 time");
    assert_eq!(t(got), t("2012-06-01T12:00:00.999980Z"));
}

/// v2 B500 Type and ClockStatus fields that completely fill their SEED fields round-trip.
/// Ported from the C test `msr3_writemseed_v2_b500_full_fields`.
#[test]
fn write_v2_b500_full_fields() {
    const TYPE_FULL: &str = "0123456789ABCDEF";
    const MODEL_FULL: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
    let clockstatus_full = "0123456789ABCDEF".repeat(8);
    assert_eq!(TYPE_FULL.len(), 16);
    assert_eq!(clockstatus_full.len(), 128);
    assert_eq!(MODEL_FULL.len(), 32);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testdata-b500fields.mseed2");

    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(512);
    msr.pub_version = 1;
    msr.start_time = t("2012-06-01T00:00:00Z");
    msr.sample_rate = 1.0;
    msr.encoding = Some(Encoding::INT32);
    msr.sample_count = 4;
    msr.samples = Some(Samples::Int32(vec![1, 2, 3, 4]));
    msr.extra = format!(
        concat!(
            r#"{{"FDSN":{{"Time":{{"Exception":["#,
            r#"{{"Time":"2012-06-01T00:00:01Z","Type":"{}","ClockStatus":"{}"}},"#,
            r#"{{"Time":"2012-06-01T00:00:02Z","Type":"Short","ClockStatus":"Brief status"}}"#,
            r#"]}},"Clock":{{"Model":"{}"}}}}}}"#
        ),
        TYPE_FULL, clockstatus_full, MODEL_FULL
    );
    assert!(msr.write_file(&path, &flush_v2()).unwrap().records > 0);

    let rec = MsReader::open(&path, ParseFlags::empty()).unwrap().next_record().unwrap().unwrap();
    let eh = rec.extra_headers().unwrap();
    let s = |p: &str| eh.get_str(p).unwrap().map(str::to_string);
    assert_eq!(s("/FDSN/Time/Exception/0/Type").as_deref(), Some(TYPE_FULL));
    assert_eq!(s("/FDSN/Time/Exception/0/ClockStatus").as_deref(), Some(clockstatus_full.as_str()));
    assert_eq!(s("/FDSN/Time/Exception/1/Type").as_deref(), Some("Short"));
    assert_eq!(s("/FDSN/Time/Exception/1/ClockStatus").as_deref(), Some("Brief status"));
    assert_eq!(s("/FDSN/Clock/Model").as_deref(), Some(MODEL_FULL));
}

/// A v2 record with more samples than the 16-bit FSDH count allows is split.
/// Ported from the C test `msr3_writemseed_v2_samplecount_overflow`.
#[test]
fn write_v2_samplecount_overflow() {
    const N: usize = 70000;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testdata-samplecount.mseed2");

    let mut msr = Record::new("FDSN:XX_TEST__L_O_G", NsTime::UNIX_EPOCH);
    msr.reclen = Some(131072);
    msr.pub_version = 1;
    msr.start_time = t("2012-01-01T00:00:00");
    msr.sample_rate = 0.0;
    msr.encoding = Some(Encoding::TEXT);
    msr.sample_count = N as u64;
    msr.samples = Some(Samples::Text(vec![b'A'; N]));
    assert!(msr.write_file(&path, &flush_v2()).unwrap().records > 0);

    let (mut total, mut reccount) = (0, 0);
    for rec in MsReader::open(&path, ParseFlags::empty()).unwrap() {
        let rec = rec.unwrap();
        assert!(rec.sample_count <= u16::MAX as u64);
        total += rec.sample_count;
        reccount += 1;
    }
    assert_eq!(total, N as u64);
    assert!(reccount > 1, "overflow path not exercised");
}

// ---------------------------------------------------------------------------------------------
// test-repack.c
// ---------------------------------------------------------------------------------------------

fn read_v2_input() -> Record {
    MsReader::open(data("reference-testdata-defaults.mseed2"), ParseFlags::UNPACK_DATA)
        .unwrap()
        .next_record()
        .unwrap()
        .unwrap()
}

#[test]
fn repack_v3() {
    let mut msr = read_v2_input();
    msr.sid = "FDSN:XX_REPAK__H_H_Z".into();
    msr.start_time = t("2008-05-12T13:44:55.123456789Z");
    msr.sample_rate = 100.0;
    msr.pub_version = 2;

    let rec = msr.repack(FormatVersion::V3).unwrap();
    assert_eq!(rec, std::fs::read(data("reference-testdata-repack.mseed3")).unwrap());
}

#[test]
fn repack_v2() {
    let mut msr = read_v2_input();
    msr.sid = "FDSN:XX_REPAK__H_H_Z".into();
    msr.start_time = t("2008-05-12T13:44:55.123456789Z");
    msr.sample_rate = 100.0;
    msr.pub_version = 2;

    let rec = msr.repack(FormatVersion::V2).unwrap();
    assert_eq!(rec, std::fs::read(data("reference-testdata-repack.mseed2")).unwrap());
}

/// A negative time correction applied to the start time survives a repack round trip.
#[test]
fn repack_v2_negative_time_correction() {
    let mut msr = read_v2_input();
    msr.extra = r#"{"FDSN":{"Time":{"Correction":-1.5}}}"#.into();
    let rec = msr.repack(FormatVersion::V2).unwrap();
    let parsed = Record::parse(&rec, ParseFlags::empty()).unwrap();
    assert_eq!(
        parsed.extra_headers().unwrap().get_f64("/FDSN/Time/Correction").unwrap(),
        Some(-1.5)
    );
}

// ---------------------------------------------------------------------------------------------
// v2 sample rate and calibration abort packing
// ---------------------------------------------------------------------------------------------

fn pack_v2_at_rate(samprate: f64) -> libmseed::Result<usize> {
    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(512);
    msr.pub_version = 1;
    msr.format_version = Some(FormatVersion::V2);
    msr.start_time = t("2012-05-12T00:00:00Z");
    msr.sample_rate = samprate;
    msr.encoding = Some(Encoding::STEIM1);
    msr.sample_count = 200;
    msr.samples = Some(Samples::Int32(isinedata()[..200].to_vec()));
    msr.pack_with(&flush(), |_| Ok(())).map(|p| p.records)
}

/// Rates that cannot be represented as a v2 factor/multiplier pair are rejected, and rates at
/// the limits of the representable range still pack. Maximum nominal rate is 32767 * 32767.
/// Ported from the C test `msr3_pack_v2_samprate_range`.
#[test]
fn pack_v2_samprate_range() {
    for bad in [1.0e10, 1.0e30, 3.0e9, 1073676290.0, f64::NAN, f64::INFINITY] {
        assert!(pack_v2_at_rate(bad).is_err(), "sample rate {bad} was not rejected for v2");
    }
    for good in [1073676289.0, 32767.0, 100.0, 1.0 / 3.0, -10.0] {
        assert!(pack_v2_at_rate(good).unwrap() > 0, "sample rate {good} was rejected for v2");
    }
}

fn pack_v2_with_extra(extra: &str) -> libmseed::Result<Vec<Vec<u8>>> {
    let mut msr = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(512);
    msr.pub_version = 1;
    msr.format_version = Some(FormatVersion::V2);
    msr.start_time = t("2024-01-02T03:04:05Z");
    msr.sample_rate = 1.0;
    msr.encoding = Some(Encoding::INT32);
    msr.sample_count = 4;
    msr.samples = Some(Samples::Int32(vec![1, 2, 3, 4]));
    msr.extra = extra.to_string();
    msr.pack(&flush())
}

/// A calibration abort round trips through v2 as a Blockette 395.
/// Ported from the C test `msr3_pack_v2_calibration_abort`.
#[test]
fn pack_v2_calibration_abort() {
    let recs = pack_v2_with_extra(
        r#"{"FDSN":{"Calibration":{"Sequence":[{"Type":"ABORT","EndTime":"2024-01-02T03:04:09Z"}]}}}"#,
    )
    .unwrap();
    assert!(!recs.is_empty());
    let rec = Record::parse(recs.last().unwrap(), ParseFlags::empty()).unwrap();
    let eh = rec.extra_headers().unwrap();
    assert_eq!(eh.get_str("/FDSN/Calibration/Sequence/0/Type").unwrap(), Some("ABORT"));
    assert_eq!(
        eh.get_str("/FDSN/Calibration/Sequence/0/EndTime").unwrap(),
        Some("2024-01-02T03:04:09Z")
    );

    // A sequence with only an end time is a Blockette 395 alone
    assert!(
        pack_v2_with_extra(
            r#"{"FDSN":{"Calibration":{"Sequence":[{"EndTime":"2024-01-02T03:04:09Z"}]}}}"#
        )
        .is_ok()
    );
    // An abort without an end time has no content to write
    assert!(
        pack_v2_with_extra(r#"{"FDSN":{"Calibration":{"Sequence":[{"Type":"ABORT"}]}}}"#).is_err()
    );
    // An end time must be a string
    assert!(
        pack_v2_with_extra(r#"{"FDSN":{"Calibration":{"Sequence":[{"EndTime":12345}]}}}"#).is_err()
    );
    // An unrecognized type cannot be mapped to a blockette
    assert!(pack_v2_with_extra(
        r#"{"FDSN":{"Calibration":{"Sequence":[{"Type":"BOGUS","EndTime":"2024-01-02T03:04:09Z"}]}}}"#
    )
    .is_err());
}

/// Typed extra header values survive a v2 pack and parse round trip as blockettes.
#[test]
fn v2_blockette_roundtrip() {
    let extra = concat!(
        r#"{"FDSN":{"Event":{"Detection":[{"Type":"MURDOCK","SignalAmplitude":80.0,"SignalPeriod":0.4,"#,
        r#""BackgroundEstimate":18.0,"Wave":"DILATATION","Units":"COUNTS","#,
        r#""OnsetTime":"2022-06-05T20:32:39.120000Z","MEDSNR":[1,3,2,1,4,0],"MEDLookback":2,"#,
        r#""MEDPickAlgorithm":0,"Detector":"Z_SPWWSS"}]},"#,
        r#""Calibration":{"Sequence":[{"Type":"STEP","BeginTime":"2022-06-05T20:00:00Z","Steps":2,"#,
        r#""StepFirstPulsePositive":true,"Trigger":"AUTOMATIC","Duration":5.0,"Amplitude":1.5,"#,
        r#""InputChannel":"BHZ","Coupling":"Resistive"}]},"Time":{"Quality":100}}}"#
    );
    let recs = pack_v2_with_extra(extra).unwrap();
    let rec = Record::parse(&recs[0], ParseFlags::empty()).unwrap();
    let eh = rec.extra_headers().unwrap();
    assert_eq!(eh.get_str("/FDSN/Event/Detection/0/Type").unwrap(), Some("MURDOCK"));
    assert_eq!(eh.get_f64("/FDSN/Event/Detection/0/SignalAmplitude").unwrap(), Some(80.0));
    assert_eq!(eh.get_str("/FDSN/Event/Detection/0/Wave").unwrap(), Some("DILATATION"));
    assert_eq!(eh.get_str("/FDSN/Event/Detection/0/Detector").unwrap(), Some("Z_SPWWSS"));
    assert_eq!(eh.get_i64("/FDSN/Event/Detection/0/MEDLookback").unwrap(), Some(2));
    assert_eq!(eh.get_str("/FDSN/Calibration/Sequence/0/Type").unwrap(), Some("STEP"));
    assert_eq!(eh.get_i64("/FDSN/Calibration/Sequence/0/Steps").unwrap(), Some(2));
    assert_eq!(eh.get_str("/FDSN/Calibration/Sequence/0/Trigger").unwrap(), Some("AUTOMATIC"));
    assert_eq!(eh.get_str("/FDSN/Calibration/Sequence/0/InputChannel").unwrap(), Some("BHZ"));
    assert_eq!(eh.get_i64("/FDSN/Time/Quality").unwrap(), Some(100));
}

/// A failing handler stops trace list packing, and the records delivered before the failure
/// are removed from the list so they are not packed twice.
#[test]
fn trace_list_pack_handler_error_stops() {
    let mut record = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    record.start_time = NsTime::parse("2024-01-01T00:00:00Z").unwrap();
    record.sample_rate = 100.0;
    record.sample_count = 1000;
    record.samples = Some(Samples::Int32((0..1000).collect()));
    let mut traces = TraceList::new();
    traces.add_record(&record).unwrap();

    let mut calls = 0;
    let err = traces
        .pack_with(
            &PackOptions {
                record_length: Some(512),
                encoding: Some(Encoding::INT32),
                flush: true,
                ..Default::default()
            },
            |_| {
                calls += 1;
                if calls == 2 { Err(Error::other("handler failed")) } else { Ok(()) }
            },
        )
        .unwrap_err();
    assert_eq!(err.to_string(), "handler failed");
    assert_eq!(calls, 2);

    // Only the first record's samples were removed
    let seg = &traces.ids()[0].segments[0];
    let first_record_samples = (512 - 40 - record.sid.len()) / 4;
    assert_eq!(seg.num_samples(), 1000 - first_record_samples);
    assert_eq!(seg.samples.as_ref().unwrap().as_i32().unwrap()[0], first_record_samples as i32);
}

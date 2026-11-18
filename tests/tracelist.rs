//! Port of libmseed test/test-tracelist.c

mod common;

use common::data;
use libmseed::codec::Samples;
use libmseed::tracelist::RecordLocation;
use libmseed::{
    NSTMODULUS, NsTime, ParseFlags, ReadOptions, Record, RecordList, SampleType, Selections,
    Tolerance, TraceList, TraceListOptions, Versions,
};

fn t(s: &str) -> NsTime {
    NsTime::parse(s).unwrap()
}

const MIXED_V2: &str = "testdata-oneseries-mixedlengths-mixedorder.mseed2";

/// Reads a file of one series with mixed lengths and mixed time order into a trace list, which
/// must reconstruct a single, continuous series regardless of the order records were added.
#[test]
fn readtracelist_mixedlengths_mixedorder() {
    let starttime = t("2010-02-27T06:50:00.069539Z");
    let endtime = t("2010-02-27T07:55:51.069539Z");

    let mut mstl = TraceList::new();
    mstl.read_file(data(MIXED_V2), &ReadOptions::new(ParseFlags::UNPACK_DATA)).unwrap();

    assert_eq!(mstl.num_ids(), 1);
    let id = &mstl.ids()[0];
    assert_eq!(id.sid, "FDSN:XX_TEST_00_L_H_Z");
    assert_eq!(id.earliest, starttime);
    assert_eq!(id.latest, endtime);
    assert_eq!(id.pub_version, 1);
    assert_eq!(id.num_segments(), 1);
    let seg = &id.segments[0];
    assert_eq!(seg.start_time, starttime);
    assert_eq!(seg.end_time, endtime);
    assert_eq!(seg.sample_count, 3952);
    assert_eq!(seg.sample_type(), Some(SampleType::Int32));
    assert_eq!(seg.num_samples(), 3952);
}

/// A record list is built for each segment and used to decode the data later.
#[test]
fn readtracelist_recptr() {
    let endtime = t("2010-02-27T07:55:51.069539Z");
    let mut mstl = record_list_trace_list();
    mstl.read_file(data(MIXED_V2), &ReadOptions::default()).unwrap();
    assert_eq!(mstl.num_ids(), 1);

    let seg = mstl.segments_mut().next().unwrap();
    assert!(seg.record_list.is_some());
    assert_eq!(seg.sample_count, 3952);

    // No data has been decoded
    assert!(seg.sample_type().is_none());
    assert!(seg.samples.is_none());
    assert_eq!(seg.num_samples(), 0);

    let recptr = seg.record_list.as_ref().unwrap().last().unwrap();
    // The record is in a file, not a buffer
    assert!(matches!(&recptr.location, RecordLocation::File { path, offset }
        if path == &data(MIXED_V2) && *offset == 1152));
    assert!(recptr.record.raw().is_none());
    assert_eq!(recptr.end_time, endtime);
    assert_eq!(recptr.data_offset, Some(64));

    // Decode the data
    let unpacked = seg.unpack_record_list(None).unwrap();
    assert_eq!(unpacked as u64, seg.sample_count);
    assert_eq!(seg.sample_type(), Some(SampleType::Int32));
    assert_eq!(seg.num_samples(), 3952);
    let s = seg.samples.as_ref().unwrap().as_i32().unwrap();
    assert_eq!(&s[3948..], &[28067, -9565, -71961, -146622]);

    // Decoding twice is refused
    assert!(seg.unpack_record_list(None).is_err());
}

/// The same from a buffer, where records are located by offset.
#[test]
fn readbuffer_recptr() {
    let endtime = t("2010-02-27T07:55:51.069539Z");
    let buffer = std::fs::read(data(MIXED_V2)).unwrap();
    assert_eq!(buffer.len(), 16256);

    let mut mstl = record_list_trace_list();
    let n = mstl.read_buffer(&buffer, &ReadOptions::default()).unwrap();
    assert_eq!(n, 7);
    assert_eq!(mstl.num_ids(), 1);

    let seg = mstl.segments_mut().next().unwrap();
    assert_eq!(seg.sample_count, 3952);
    assert!(seg.sample_type().is_none());
    assert!(seg.samples.is_none());

    let recptr = seg.record_list.as_ref().unwrap().last().unwrap();
    assert!(matches!(recptr.location, RecordLocation::Buffer { .. }));
    assert_eq!(recptr.end_time, endtime);
    assert_eq!(recptr.data_offset, Some(64));

    // Records in a buffer need the buffer to decode
    assert!(seg.decode_record_list(None).is_err());
    let unpacked = seg.unpack_record_list(Some(&buffer)).unwrap();
    assert_eq!(unpacked as u64, seg.sample_count);
    let s = seg.samples.as_ref().unwrap().as_i32().unwrap();
    assert_eq!(&s[3948..], &[28067, -9565, -71961, -146622]);
}

fn record_list_trace_list() -> TraceList {
    TraceList::with_options(TraceListOptions {
        record_list: RecordList::WithExtras,
        ..Default::default()
    })
}

fn header_only_record(rate: f64, start: &str) -> Record {
    let mut msr = Record::new("FDSN:XX_TEST__X_H_Z", NsTime::UNIX_EPOCH);
    msr.format_version = Some(libmseed::FormatVersion::V3);
    msr.pub_version = 1;
    msr.start_time = t(start);
    msr.sample_rate = rate;
    msr.sample_count = 100;
    msr
}

/// Extra headers are copied to the record list by default and omitted with NO_EXTRAS.
#[test]
fn addmsr_recordptr_noextras() {
    let extra = r#"{"FDSN":{"Time":{"Quality":100}}}"#;
    let mut msr = header_only_record(1.0, "2024-01-01T00:00:00.0Z");
    msr.extra = extra.into();

    let mut mstl = record_list_trace_list();
    let pos = mstl.add_record_located(&msr, RecordLocation::Unknown).unwrap();
    let rp = &mstl.segment(pos).unwrap().record_list.as_ref().unwrap()[0];
    assert_eq!(rp.record.extra, extra);

    let mut mstl = TraceList::with_options(TraceListOptions {
        record_list: RecordList::WithoutExtras,
        ..Default::default()
    });
    let pos = mstl.add_record_located(&msr, RecordLocation::Unknown).unwrap();
    let rp = &mstl.segment(pos).unwrap().record_list.as_ref().unwrap()[0];
    assert!(rp.record.extra.is_empty());
}

/// PP_UPDATE_TIME records the update time with the segment.
#[test]
fn readtracelist_ppupdatetime() {
    let now = NsTime::now();
    let mut mstl =
        TraceList::with_options(TraceListOptions { update_time: true, ..Default::default() });
    mstl.read_file(data(MIXED_V2), &ReadOptions::default()).unwrap();
    assert_eq!(mstl.num_ids(), 1);
    let update = mstl.ids()[0].segments[0].update_time.expect("update time");
    assert!(update.nanos_since(now) < 10 * NSTMODULUS);
}

/// SPLIT_IS_VERSION uses the split version value as the ID version.
#[test]
fn readtracelist_splitisversion() {
    let mut mstl = TraceList::with_options(TraceListOptions {
        versions: Versions::Assign(99),
        ..Default::default()
    });
    mstl.read_file(
        data("testdata-oneseries-mixedlengths-mixedorder.mseed3"),
        &ReadOptions::default(),
    )
    .unwrap();
    assert_eq!(mstl.num_ids(), 1);
    assert_eq!(mstl.ids()[0].pub_version, 99);
}

#[test]
fn readtracelist_selection_nomatch() {
    let mut sel = Selections::new();
    sel.add("FDSN:XX_NOSUCH_*_B_H_Z", None, None, None).unwrap();
    let mut mstl = TraceList::new();
    let options =
        ReadOptions { selections: Some(sel), ..ReadOptions::new(ParseFlags::UNPACK_DATA) };
    let n = mstl.read_file(data("testdata-3channel-signal.mseed3"), &options).unwrap();
    assert_eq!(n, 0);
    assert_eq!(mstl.num_ids(), 0);
}

#[test]
fn readtracelist_no_raw_ignored() {
    let buffer = std::fs::read(data(MIXED_V2)).unwrap();
    for flags in [ParseFlags::UNPACK_DATA, ParseFlags::UNPACK_DATA | ParseFlags::NO_RAW] {
        let options = ReadOptions { skip_adjacent_duplicates: true, ..ReadOptions::new(flags) };
        let mut from_buffer = TraceList::new();
        assert_eq!(from_buffer.read_buffer(&buffer, &options).unwrap(), 7);
        let mut from_file = TraceList::new();
        assert_eq!(from_file.read_file(data(MIXED_V2), &options).unwrap(), 7);
        for mstl in [&mut from_buffer, &mut from_file] {
            let seg = mstl.segments_mut().next().unwrap();
            assert_eq!(seg.sample_count, 3952);
            assert_eq!(seg.samples.as_ref().unwrap().len(), 3952);
        }
    }
}

#[test]
fn readtracelist_timewin() {
    let mut mstl = TraceList::new();
    let mut sel = Selections::new();
    sel.add("*", Some(t("2010-02-27T07:00:00Z")), Some(t("2010-02-27T07:10:00Z")), None).unwrap();
    let options = ReadOptions { selections: Some(sel), ..Default::default() };
    mstl.read_file(data(MIXED_V2), &options).unwrap();
    // Only records intersecting the window are included
    let seg = &mstl.ids()[0].segments[0];
    assert!(seg.sample_count < 3952);
    assert!(seg.sample_count > 0);
}

/// Build a trace list from two time-contiguous, header-only records for the same source
/// whose sample rates are given, and return the resulting number of segments.
fn addmsr_two_rates(tolerance: Option<Tolerance>, rate1: f64, rate2: f64) -> usize {
    let mut mstl = TraceList::with_options(TraceListOptions {
        tolerance: tolerance.unwrap_or_default(),
        ..Default::default()
    });
    let mut msr = header_only_record(rate1, "2024-01-01T00:00:00.0Z");
    let endtime1 = msr.end_time().unwrap();
    mstl.add_record(&msr).unwrap();

    // Record 2 starts one (record 2) sample period after record 1 ends
    msr.sample_rate = rate2;
    msr.start_time = endtime1.add_nanos(msr.period_nanos());
    mstl.add_record(&msr).unwrap();

    mstl.ids()[0].num_segments()
}

#[test]
fn addmsr_sampratetol_default() {
    assert_eq!(addmsr_two_rates(None, 100.0, 99.5), 2);
}

#[test]
fn addmsr_sampratetol_custom() {
    let tol = Tolerance::with_samprate(|_| 1.0);
    assert_eq!(addmsr_two_rates(Some(tol.clone()), 100.0, 99.5), 1);
}

/// A tolerance of 0.0 requires an exact match instead of the default relative tolerance.
#[test]
fn addmsr_sampratetol_exact() {
    let tol = Tolerance::with_samprate(|_| 0.0);
    assert_eq!(addmsr_two_rates(Some(tol.clone()), 100.0, 100.005), 2);
}

/// A negative tolerance is ignored in favor of the default.
#[test]
fn addmsr_sampratetol_negative() {
    let tol = Tolerance::with_samprate(|_| -1.0);
    assert_eq!(addmsr_two_rates(Some(tol.clone()), 100.0, 100.005), 1);
    assert_eq!(addmsr_two_rates(Some(tol.clone()), 100.0, 99.5), 2);
}

/// Build a single-segment list of the given samples and convert it.
fn convert(samples: Samples, to: SampleType, truncate: bool) -> (bool, TraceList) {
    let mut msr = Record::new("FDSN:XX_TEST__X_H_Z", NsTime::UNIX_EPOCH);
    msr.reclen = Some(512);
    msr.format_version = Some(libmseed::FormatVersion::V3);
    msr.pub_version = 1;
    msr.sample_rate = 100.0;
    msr.start_time = t("2024-01-01T00:00:00.0Z");
    msr.sample_count = samples.len() as u64;
    msr.samples = Some(samples);

    let mut mstl = TraceList::new();
    let pos = mstl.add_record(&msr).unwrap();
    let ok = mstl.segment_mut(pos).unwrap().convert_samples(to, truncate).is_ok();
    (ok, mstl)
}

fn first_seg(mstl: &TraceList) -> &libmseed::tracelist::TraceSegment {
    &mstl.ids()[0].segments[0]
}

/// NaN samples are rejected rather than converted to platform-dependent garbage.
#[test]
fn convertsamples_nan() {
    for truncate in [false, true] {
        let (ok, mstl) =
            convert(Samples::Float32(vec![1.0, 2.0, f32::NAN, 4.0]), SampleType::Int32, truncate);
        assert!(!ok);
        assert_eq!(first_seg(&mstl).sample_type(), Some(SampleType::Float32));
    }
}

/// Values outside the range of a 32-bit integer are rejected even when truncating.
#[test]
fn convertsamples_outofrange() {
    for v in [1.0e30f32, -1.0e30, f32::INFINITY, f32::NEG_INFINITY] {
        for truncate in [false, true] {
            let (ok, mstl) = convert(Samples::Float32(vec![1.0, v]), SampleType::Int32, truncate);
            assert!(!ok, "{v} truncate={truncate}");
            assert_eq!(first_seg(&mstl).sample_type(), Some(SampleType::Float32));
        }
    }
}

/// The range check is not off by one: the extreme representable values convert.
#[test]
fn convertsamples_boundary() {
    let (ok, mstl) = convert(Samples::Float32(vec![2147483520.0]), SampleType::Int32, false);
    assert!(ok);
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_i32().unwrap()[0], 2147483520);

    let (ok, mstl) = convert(Samples::Float32(vec![-2147483648.0]), SampleType::Int32, false);
    assert!(ok);
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_i32().unwrap()[0], i32::MIN);
}

#[test]
fn convertsamples_valid() {
    let (ok, mstl) = convert(Samples::Float32(vec![-2.0, 0.0, 3.0]), SampleType::Int32, false);
    assert!(ok);
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_i32().unwrap()[0], -2);

    // Loss of precision is rejected unless truncating
    assert!(!convert(Samples::Float32(vec![1.4]), SampleType::Int32, false).0);
    let (ok, mstl) = convert(Samples::Float32(vec![1.4]), SampleType::Int32, true);
    assert!(ok);
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_i32().unwrap()[0], 1);
    let (ok, mstl) = convert(Samples::Float32(vec![-1.4]), SampleType::Int32, true);
    assert!(ok);
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_i32().unwrap()[0], -1);
}

#[test]
fn convertsamples_double() {
    // NaN and out-of-range are rejected and leave the samples unconverted
    for bad in [f64::NAN, 1.0e30] {
        let (ok, mstl) = convert(Samples::Float64(vec![1.0, bad, 3.0]), SampleType::Int32, true);
        assert!(!ok);
        assert_eq!(first_seg(&mstl).sample_type(), Some(SampleType::Float64));
    }
    let (ok, mstl) = convert(Samples::Float64(vec![1.0, -2.0, 3.0]), SampleType::Int32, false);
    assert!(ok);
    assert_eq!(first_seg(&mstl).sample_type(), Some(SampleType::Int32));
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_i32().unwrap()[1], -2);

    // Widening and text conversions
    let (ok, mstl) = convert(Samples::Int32(vec![1, 2]), SampleType::Float64, false);
    assert!(ok);
    assert_eq!(first_seg(&mstl).samples.as_ref().unwrap().as_f64().unwrap(), &[1.0, 2.0]);
    assert!(!convert(Samples::Text(b"ab".to_vec()), SampleType::Int32, false).0);
}

/// Records may arrive in any order and bridging data merges segments.
#[test]
fn addmsr_ordering_and_autoheal() {
    let mut mstl = TraceList::new();
    let mk = |start: &str, n: u64| {
        let mut m = header_only_record(1.0, start);
        m.sample_count = n;
        m
    };
    // Segment B, then A (before it, leaving a gap), then a bridge C
    let b = mk("2024-01-01T00:02:00Z", 60);
    let a = mk("2024-01-01T00:00:00Z", 60);
    let c = mk("2024-01-01T00:01:00Z", 60);
    mstl.add_record(&b).unwrap();
    mstl.add_record(&a).unwrap();
    let id = &mstl.ids()[0];
    assert_eq!(id.num_segments(), 2, "gap between the segments");
    assert!(id.segments[0].start_time < id.segments[1].start_time);

    mstl.add_record(&c).unwrap();
    let id = &mstl.ids()[0];
    assert_eq!(id.num_segments(), 1, "bridge merges the segments");
    assert_eq!(id.segments[0].sample_count, 180);
    assert_eq!(id.earliest, t("2024-01-01T00:00:00Z"));
    assert_eq!(id.latest, t("2024-01-01T00:02:59Z"));

    // Without autoheal the bridge extends one segment, leaving two
    let mut mstl =
        TraceList::with_options(TraceListOptions { autoheal: false, ..Default::default() });
    mstl.add_record(&b).unwrap();
    mstl.add_record(&a).unwrap();
    mstl.add_record(&c).unwrap();
    assert_eq!(mstl.ids()[0].num_segments(), 2);
}

/// Separate IDs are kept per publication version when splitting, and order is by SID.
#[test]
fn id_ordering_and_split_versions() {
    let mut mstl = TraceList::with_options(TraceListOptions {
        versions: Versions::Separate,
        ..Default::default()
    });
    for (sid, ver) in [("FDSN:XX_B__B_H_Z", 2u8), ("FDSN:XX_A__B_H_Z", 1), ("FDSN:XX_A__B_H_Z", 3)]
    {
        let mut m = header_only_record(1.0, "2024-01-01T00:00:00Z");
        m.sid = sid.into();
        m.pub_version = ver;
        mstl.add_record(&m).unwrap();
    }
    let ids: Vec<(String, u8)> =
        mstl.ids().iter().map(|i| (i.sid.clone(), i.pub_version)).collect();
    assert_eq!(
        ids,
        [
            ("FDSN:XX_A__B_H_Z".to_string(), 1),
            ("FDSN:XX_A__B_H_Z".to_string(), 3),
            ("FDSN:XX_B__B_H_Z".to_string(), 2)
        ]
    );
    assert_eq!(mstl.find_id("FDSN:XX_A__B_H_Z", Some(3)), Some(1));
    assert_eq!(mstl.find_id("FDSN:XX_A__B_H_Z", None), Some(0));
    assert_eq!(mstl.find_id("FDSN:XX_A__B_H_Z", Some(2)), None);
    assert_eq!(mstl.find_id("FDSN:XX_C__B_H_Z", None), None);
}

#[test]
fn print_listings() {
    let mut mstl = TraceList::new();
    mstl.read_file(data(MIXED_V2), &ReadOptions::default()).unwrap();
    let s = mstl.format_trace_list(&libmseed::tracelist::TraceListing {
        details: true,
        gaps: true,
        ..Default::default()
    });
    assert!(s.contains("FDSN:XX_TEST_00_L_H_Z"));
    assert!(s.contains("2010-02-27T06:50:00.069539Z"));
    assert!(s.contains("Total: 1 trace(s) with 1 segment(s)"));
    let g = mstl.format_gap_list(libmseed::time::TimeFormat::IsoMonthDayZ, None, None);
    assert!(g.ends_with("Total: 0 gap(s)\n"));
    let sync =
        mstl.format_sync_list(Some("DCC"), libmseed::time::Subseconds::Micro, Some("2024,001"));
    assert!(sync.starts_with("DCC|2024,001\nXX|TEST|00|LHZ|2010,058,06:50:00.069539|"));
}

/// A record holding samples without a sample count counts its samples; a count that
/// disagrees with the samples is rejected.
#[test]
fn add_record_counts_samples() {
    let mut rec = Record::new("FDSN:XX_TEST__B_H_Z", t("2024-01-01T00:00:00Z"));
    rec.sample_rate = 1.0;
    rec.samples = Some(Samples::Int32((0..10).collect()));

    let mut traces = TraceList::new();
    traces.add_record(&rec).unwrap();
    let seg = &traces.ids()[0].segments[0];
    assert_eq!(seg.sample_count, 10);
    assert_eq!(seg.end_time, t("2024-01-01T00:00:09Z"));

    rec.sample_count = 5;
    assert!(matches!(traces.add_record(&rec), Err(libmseed::Error::InvalidArgument(_))));
}

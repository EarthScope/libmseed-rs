//! Port of the trace list packing tests of libmseed test/test-write.c

mod common;

use common::*;
use libmseed::codec::Samples;
use libmseed::reader::MsReader;
use libmseed::tracelist::{SegmentPos, TraceListPacker};
use libmseed::{
    Encoding, FormatVersion, NsTime, PackOptions, Packed, ParseFlags, ReadOptions, Record,
    TraceList, TraceListOptions,
};
use std::path::Path;

fn t(s: &str) -> NsTime {
    NsTime::parse(s).unwrap()
}

fn assert_matches_reference(written: &[u8], name: &str) {
    let want = std::fs::read(data(&format!("reference-{name}"))).unwrap();
    assert!(
        written == &want[..],
        "{name}: output differs from reference ({} vs {} bytes)",
        written.len(),
        want.len()
    );
}

fn msr(sid: &str, rate: f64, start: NsTime, samples: Vec<i32>) -> Record {
    let mut m = Record::new(sid, start);
    m.reclen = Some(512);
    m.pub_version = 1;
    m.sample_rate = rate;
    m.sample_count = samples.len() as u64;
    m.samples = Some(Samples::Int32(samples));
    m
}

/// Pack options with a record length and encoding.
fn opts(reclen: usize, encoding: Encoding) -> PackOptions {
    PackOptions { record_length: Some(reclen), encoding: Some(encoding), ..Default::default() }
}

/// The H_H_Z and B_H_Z test traces, 500 samples each.
fn two_trace_list() -> TraceList {
    let start = t("2012-05-12T00:00:00.123456789Z");
    let mut mstl = TraceList::new();
    mstl.add_record(&msr("FDSN:XX_TEST__H_H_Z", 100.0, start, isinedata())).unwrap();
    mstl.add_record(&msr("FDSN:XX_TEST__B_H_Z", 40.0, start, isinedata())).unwrap();
    mstl
}

/// Ported from the C test `mstl3_writemseed`.
#[test]
fn trace_list_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("testdata-steim2.mseed3.trace");
    let mut mstl = TraceList::new();
    // All but the last sample, for which the difference cannot be represented
    let samples = isinedata()[..499].to_vec();
    let mut m = msr("FDSN:XX_TEST__B_H_Z", 40.0, t("2012-05-12T00:00:00"), samples);
    m.reclen = Some(512);
    mstl.add_record(&m).unwrap();

    let rv = mstl
        .write_file(
            &path,
            &PackOptions {
                record_length: Some(512),
                encoding: Some(Encoding::STEIM2),
                flush: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(rv.records, 4);
    assert_matches_reference(&std::fs::read(&path).unwrap(), "testdata-steim2.mseed3");
    // The list is not modified
    assert_eq!(mstl.num_ids(), 1);
    assert_eq!(mstl.ids()[0].segments[0].num_samples(), 499);
}

fn pack_list(v2: bool) -> (Vec<u8>, usize, usize, TraceList) {
    let mut mstl = two_trace_list();
    let options = PackOptions {
        format: pack_opts(v2, true).format,
        flush: true,
        ..opts(512, Encoding::STEIM1)
    };
    let mut out = Vec::new();
    let packed = mstl
        .pack_with(&options, |r| {
            out.extend_from_slice(r);
            Ok(())
        })
        .unwrap();
    (out, packed.records, packed.samples, mstl)
}

/// Ported from the C test `mstl3_pack_v2`.
#[test]
fn trace_list_pack_v2() {
    let (out, records, samples, mstl) = pack_list(true);
    assert_eq!(records, 8);
    assert_eq!(samples, 1000);
    assert_matches_reference(&out, "testdata-mstlpack.mseed2");
    // Packed contents are removed from the trace list
    assert_eq!(mstl.num_ids(), 0);
}

/// Ported from the C test `mstl3_pack_v3`.
#[test]
fn trace_list_pack_v3() {
    let (out, records, samples, mstl) = pack_list(false);
    assert_eq!(records, 8);
    assert_eq!(samples, 1000);
    assert_matches_reference(&out, "testdata-mstlpack.mseed3");
    assert_eq!(mstl.num_ids(), 0);
}

/// Pack a list with the generator interface, returning the records.
fn pack_next_all(mstl: &mut TraceList, options: PackOptions) -> (Vec<Vec<u8>>, usize) {
    let mut packer = TraceListPacker::new(options).unwrap();
    let mut recs = Vec::new();
    while let Some(r) = packer.next_record(mstl).unwrap() {
        recs.push(r);
    }
    (recs, packer.packed().samples)
}

/// Ported from the C test `mstl3_pack_next_v2`.
#[test]
fn trace_list_pack_next_v2() {
    let mut mstl = two_trace_list();
    let options =
        PackOptions { format: Some(FormatVersion::V2), flush: true, ..opts(512, Encoding::STEIM1) };
    let (recs, samples) = pack_next_all(&mut mstl, options);
    assert_eq!(samples, 1000);
    assert_matches_reference(&recs.concat(), "testdata-mstlpack.mseed2");
}

/// Ported from the C test `mstl3_pack_next_v3`.
#[test]
fn trace_list_pack_next_v3() {
    let mut mstl = two_trace_list();
    let (recs, samples) =
        pack_next_all(&mut mstl, PackOptions { flush: true, ..opts(512, Encoding::STEIM1) });
    assert_eq!(samples, 1000);
    assert_matches_reference(&recs.concat(), "testdata-mstlpack.mseed3");
}

/// Samples emitted by a segment packing session still active are reported.
/// Ported from the C test `mstl3_pack_free_active_session`.
#[test]
fn trace_list_pack_free_active_session() {
    let mut mstl = TraceList::new();
    mstl.add_record(&msr(
        "FDSN:XX_TEST__H_H_Z",
        100.0,
        t("2012-05-12T00:00:00.123456789Z"),
        isinedata(),
    ))
    .unwrap();

    // A small record length forces multiple records, so the session is active after the first
    let mut packer = TraceListPacker::new(PackOptions {
        record_length: Some(128),
        encoding: Some(Encoding::STEIM1),
        ..Default::default()
    })
    .unwrap();
    let rec = packer.next_record(&mut mstl).unwrap().expect("a record");
    let expected = Record::parse(&rec, ParseFlags::empty()).unwrap().sample_count as usize;
    assert_eq!(packer.packed().samples, expected);
}

/// Packing several segments with a caller-supplied extra headers string.
/// Ported from the C test `mstl3_pack_next_extra_headers`.
#[test]
fn trace_list_pack_next_extra_headers() {
    let mut mstl = two_trace_list();
    let options = PackOptions {
        format: Some(FormatVersion::V2),
        flush: true,
        extra_headers: Some(r#"{"FDSN":{"Time":{"Quality":100}}}"#.into()),
        ..opts(512, Encoding::STEIM1)
    };
    let (recs, samples) = pack_next_all(&mut mstl, options);
    assert_eq!(samples, 1000);
    assert!(recs.len() > 1);
    for r in &recs {
        let rec = Record::parse(r, ParseFlags::empty()).unwrap();
        assert_eq!(rec.extra_headers().unwrap().get_u64("/FDSN/Time/Quality").unwrap(), Some(100));
    }
}

fn check_maintained(mstl: &TraceList) {
    assert_eq!(mstl.num_ids(), 2);
    assert_eq!(mstl.ids()[0].sid, "FDSN:XX_TEST__B_H_Z");
    assert_eq!(mstl.ids()[1].sid, "FDSN:XX_TEST__H_H_Z");
    for id in mstl.ids() {
        assert_eq!(id.num_segments(), 1);
        assert_eq!(id.segments[0].num_samples(), 500);
    }
}

/// Ported from the C test `mstl3_pack_maintainmstl`.
#[test]
fn trace_list_pack_maintainmstl() {
    let mut mstl = two_trace_list();
    let Packed { records, samples } = mstl
        .pack_with(
            &PackOptions {
                record_length: Some(512),
                encoding: Some(Encoding::STEIM1),
                flush: true,
                keep_data: true,
                ..Default::default()
            },
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(records, 8);
    assert_eq!(samples, 1000);
    check_maintained(&mstl);
}

/// Ported from the C test `mstl3_pack_next_maintainmstl`.
#[test]
fn trace_list_pack_next_maintainmstl() {
    let mut mstl = two_trace_list();
    let options = PackOptions { flush: true, keep_data: true, ..opts(512, Encoding::STEIM1) };
    let (recs, samples) = pack_next_all(&mut mstl, options);
    assert_eq!(recs.len(), 8);
    assert_eq!(samples, 1000);
    check_maintained(&mstl);
}

/// Under MAINTAIN a short trailing segment must not cause earlier IDs to be repacked or
/// their remainder dropped: every visited segment is packed completely the first time.
/// Ported from the C test `mstl3_pack_next_maintainmstl_shortsegment`.
#[test]
fn trace_list_pack_next_maintainmstl_shortsegment() {
    let start = t("2012-05-12T00:00:00.000000000Z");
    let mut mstl = TraceList::new();
    mstl.add_record(&msr("FDSN:XX_TEST__A_A_A", 100.0, start, isinedata())).unwrap();
    mstl.add_record(&msr("FDSN:XX_TEST__Z_Z_Z", 100.0, start, isinedata()[..5].to_vec())).unwrap();

    let mut packer = TraceListPacker::new(PackOptions {
        record_length: Some(512),
        encoding: Some(Encoding::STEIM1),
        keep_data: true,
        ..Default::default()
    })
    .unwrap();
    while packer.next_record(&mut mstl).unwrap().is_some() {}
    // A further flush call finds nothing left to pack
    packer.set_flush();
    assert!(packer.next_record(&mut mstl).unwrap().is_none());
    assert_eq!(packer.packed().samples, 500 + 5);
}

/// Every segment is too short to fill a record: a later flush call packs them all.
/// Ported from the C test `mstl3_pack_next_maintainmstl_allshort`.
#[test]
fn trace_list_pack_next_maintainmstl_allshort() {
    let start = t("2012-05-12T00:00:00.000000000Z");
    let mut mstl = TraceList::new();
    for sid in ["FDSN:XX_TEST__A_A_A", "FDSN:XX_TEST__Z_Z_Z"] {
        mstl.add_record(&msr(sid, 100.0, start, isinedata()[..5].to_vec())).unwrap();
    }
    let mut packer = TraceListPacker::new(PackOptions {
        record_length: Some(512),
        encoding: Some(Encoding::STEIM1),
        keep_data: true,
        ..Default::default()
    })
    .unwrap();
    let mut count = 0;
    while packer.next_record(&mut mstl).unwrap().is_some() {
        count += 1;
    }
    packer.set_flush();
    while packer.next_record(&mut mstl).unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 2);
    assert_eq!(packer.packed().samples, 10);
    assert_eq!(mstl.num_ids(), 2);
}

/// Under MAINTAIN the packer resumes after the last completed segment, so an ID added later
/// is picked up without re-initializing the packer.
/// Ported from the C test `mstl3_pack_next_maintainmstl_added_after`.
#[test]
fn trace_list_pack_next_maintainmstl_added_after() {
    let start = t("2012-05-12T00:00:00.000000000Z");
    let mut mstl = TraceList::new();
    mstl.add_record(&msr("FDSN:XX_TEST__A_A_A", 100.0, start, isinedata()[..5].to_vec())).unwrap();
    let mut packer = TraceListPacker::new(PackOptions {
        record_length: Some(512),
        encoding: Some(Encoding::STEIM1),
        keep_data: true,
        ..Default::default()
    })
    .unwrap();
    let mut count = 0;
    while packer.next_record(&mut mstl).unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 1, "first pass packs the initial ID");

    mstl.add_record(&msr("FDSN:XX_TEST__Z_Z_Z", 100.0, start, isinedata()[..5].to_vec())).unwrap();
    count = 0;
    while packer.next_record(&mut mstl).unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 1, "ID added after the scan cursor is packed");
    assert_eq!(packer.packed().samples, 10);
}

fn flushidle(v2: bool) {
    let start = t("2012-05-12T00:00:00.123456789Z");
    let mut mstl =
        TraceList::with_options(TraceListOptions { update_time: true, ..Default::default() });
    let (first, second) = if v2 {
        (("FDSN:XX_TEST__H_H_Z", 100.0), ("FDSN:XX_TEST__B_H_Z", 40.0))
    } else {
        (("FDSN:XX_TEST__B_H_Z", 40.0), ("FDSN:XX_TEST__H_H_Z", 100.0))
    };
    let mut last = SegmentPos { id: 0, segment: 0 };
    for (sid, rate) in [first, second] {
        let mut m = msr(sid, rate, start, isinedata());
        // The C test leaves the sample count unset; packing uses the samples held
        m.sample_count = 0;
        m.reclen = None;
        last = mstl.add_record(&m).unwrap();
    }
    // Make the last added trace's update time 60 seconds old
    mstl.segment_mut(last).unwrap().update_time =
        Some(NsTime::now() - std::time::Duration::from_secs(60));

    let options = PackOptions {
        flush_idle: Some(std::time::Duration::from_secs(30)),
        format: pack_opts(v2, false).format,
        ..opts(4096, Encoding::STEIM1)
    };
    let mut out = Vec::new();
    let Packed { records, samples } = mstl
        .pack_with(&options, |r| {
            out.extend_from_slice(r);
            Ok(())
        })
        .unwrap();
    assert_eq!(records, 1);
    assert_eq!(samples, 500);
    assert_matches_reference(
        &out,
        if v2 { "testdata-flushidle.mseed2" } else { "testdata-flushidle.mseed3" },
    );
}

/// Ported from the C test `mstl3_pack_ppupdate_flushidle_v2`.
#[test]
fn trace_list_pack_ppupdate_flushidle_v2() {
    flushidle(true);
}

/// Ported from the C test `mstl3_pack_ppupdate_flushidle_v3`.
#[test]
fn trace_list_pack_ppupdate_flushidle_v3() {
    flushidle(false);
}

fn rolling_halves() -> (TraceList, NsTime, Vec<i32>) {
    (TraceList::new(), t("2012-05-12T00:00:00.123456789Z"), isinedata())
}

fn add_half(mstl: &mut TraceList, start: NsTime, data: &[i32], second: bool) {
    for (sid, rate) in [("FDSN:XX_TEST__H_H_Z", 100.0), ("FDSN:XX_TEST__B_H_Z", 40.0)] {
        // As in the C test, both halves use the first 250 samples of the data set
        let off = 0;
        let st = if second { start.sample_time(250, rate).unwrap() } else { start };
        mstl.add_record(&msr(sid, rate, st, data[off..off + 250].to_vec())).unwrap();
    }
}

/// Rolling buffer with the callback interface: pack without flushing, add data, pack again.
/// Ported from the C test `mstl3_pack_rollingbuffer`.
#[test]
fn trace_list_pack_rollingbuffer() {
    let (mut mstl, start, data_) = rolling_halves();
    let mut out = Vec::new();
    let mut total = 0;

    add_half(&mut mstl, start, &data_, false);
    let Packed { records, samples } = mstl
        .pack_with(
            &PackOptions {
                record_length: Some(512),
                encoding: Some(Encoding::INT32),
                ..Default::default()
            },
            |r| {
                out.extend_from_slice(r);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(records, 4);
    assert_eq!(samples, 452);
    total += samples;

    add_half(&mut mstl, start, &data_, true);
    let Packed { records, samples } = mstl
        .pack_with(
            &PackOptions {
                record_length: Some(512),
                encoding: Some(Encoding::INT32),
                flush: true,
                ..Default::default()
            },
            |r| {
                out.extend_from_slice(r);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(records, 6);
    assert_eq!(samples, 548);
    total += samples;

    assert_eq!(total, 1000);
    assert_matches_reference(&out, "testdata-mstlpack-rollingbuffer.mseed");
    assert_eq!(mstl.num_ids(), 0);
}

/// The same with the generator interface.
/// Ported from the C test `mstl3_pack_next_rollingbuffer`.
#[test]
fn trace_list_pack_next_rollingbuffer() {
    let (mut mstl, start, data_) = rolling_halves();
    let mut packer = TraceListPacker::new(PackOptions {
        record_length: Some(512),
        encoding: Some(Encoding::INT32),
        ..Default::default()
    })
    .unwrap();
    let mut out = Vec::new();

    add_half(&mut mstl, start, &data_, false);
    let mut count = 0;
    while let Some(r) = packer.next_record(&mut mstl).unwrap() {
        out.extend_from_slice(&r);
        count += 1;
    }
    assert_eq!(count, 4);

    add_half(&mut mstl, start, &data_, true);
    count = 0;
    packer.set_flush();
    while let Some(r) = packer.next_record(&mut mstl).unwrap() {
        out.extend_from_slice(&r);
        count += 1;
    }
    assert_eq!(count, 6);
    assert_eq!(packer.packed().samples, 1000);
    assert_matches_reference(&out, "testdata-mstlpack-rollingbuffer-next.mseed");
    assert_eq!(mstl.num_ids(), 0);
}

/// The segment being actively packed is merged away by an autohealing add, which is reported.
/// Ported from the C test `mstl3_pack_next_autoheal_merge`.
#[test]
fn trace_list_pack_next_autoheal_merge() {
    let start = t("2012-05-12T00:00:00.0Z");
    let isine = isinedata();
    let sid = "FDSN:XX_TEST__H_H_Z";
    let mut mstl = TraceList::new();

    // Leading segment, short enough to never produce a full record on its own
    mstl.add_record(&msr(sid, 1.0, start, isine[..2].to_vec())).unwrap();
    // Trailing segment after a gap, large enough to need multiple records
    mstl.add_record(&msr(sid, 1.0, start.sample_time(5, 1.0).unwrap(), isine.clone())).unwrap();

    let mut packer = TraceListPacker::new(PackOptions {
        record_length: Some(512),
        encoding: Some(Encoding::INT32),
        ..Default::default()
    })
    .unwrap();
    assert!(packer.next_record(&mut mstl).unwrap().is_some());

    // Bridge the gap: autohealing merges the actively packed segment into the leading one
    mstl.add_record(&msr(sid, 1.0, start.sample_time(2, 1.0).unwrap(), isine[..3].to_vec()))
        .unwrap();
    assert_eq!(mstl.ids()[0].num_segments(), 1);

    assert!(packer.next_record(&mut mstl).is_err());
    // Samples emitted before the abort are still reported
    assert!(packer.packed().samples > 0);
}

/// Read back what was packed into a file with a trace list round trip.
#[test]
fn pack_then_read_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roundtrip.mseed3");
    let mut mstl = two_trace_list();
    mstl.write_file(
        &path,
        &PackOptions {
            record_length: Some(512),
            encoding: Some(Encoding::INT32),
            ..Default::default()
        },
    )
    .unwrap();

    let mut back = TraceList::new();
    back.read_file(&path, &ReadOptions::new(ParseFlags::UNPACK_DATA)).unwrap();
    assert_eq!(back.num_ids(), 2);
    for id in back.ids() {
        assert_eq!(id.num_segments(), 1);
        let s = id.segments[0].samples.as_ref().unwrap().as_i32().unwrap();
        assert_eq!(s, &isinedata()[..s.len()]);
        assert_eq!(s.len(), 500);
    }
    let _: &Path = &path;
    let n = MsReader::open(&path, ParseFlags::empty()).unwrap().count();
    assert!(n >= 8);
}

//! API-level properties of the public types.

use libmseed::extra::ExtraHeaders;
use libmseed::tracelist::TraceListPacker;
use libmseed::{
    Error, MsReader, PackOptions, ReadOptions, Record, RecordPacker, Selections, TraceList,
    TraceListOptions,
};

fn assert_send_sync<T: Send + Sync>() {}

/// The main types can be moved to and shared with other threads.
#[test]
fn types_are_send_and_sync() {
    assert_send_sync::<Record>();
    assert_send_sync::<TraceList>();
    assert_send_sync::<TraceListOptions>();
    assert_send_sync::<TraceListPacker>();
    assert_send_sync::<RecordPacker<'static>>();
    assert_send_sync::<MsReader<std::fs::File>>();
    assert_send_sync::<Selections>();
    assert_send_sync::<PackOptions>();
    assert_send_sync::<ReadOptions>();
    assert_send_sync::<ExtraHeaders>();
    assert_send_sync::<Error>();
}

/// Errors convert to `Box<dyn Error + Send + Sync>`, as `?` in applications needs.
#[test]
fn error_boxes() {
    let e: Box<dyn std::error::Error + Send + Sync> = Error::other("x").into();
    assert_eq!(e.to_string(), "x");
}

/// Latency is measured to the last sample, for a sample rate given either in samples per
/// second or as a (negative) sample period in seconds.
/// Ported from the C test `host_latency`.
#[test]
fn host_latency_to_last_sample() {
    let start = libmseed::NsTime::now() - std::time::Duration::from_secs(1000);
    let mut msr = Record::new("FDSN:XX_TEST__L_H_Z", start);
    for rate in [0.1, -10.0] {
        msr.sample_rate = rate;
        msr.sample_count = 1;
        let first = msr.host_latency();
        msr.sample_count = 2;
        let last = msr.host_latency();
        assert!(((first - last) - 10.0).abs() < 1.5, "rate {rate}: {first} - {last}");
    }
}

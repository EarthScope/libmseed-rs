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

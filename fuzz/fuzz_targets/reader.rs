//! Read arbitrary bytes as a stream of records, directly and into a trace list.
#![no_main]

use libfuzzer_sys::fuzz_target;
use libmseed::{MsReader, ParseFlags, ReadOptions, RecordList, TraceList, TraceListOptions};

fuzz_target!(|data: &[u8]| {
    let flags = ParseFlags::UNPACK_DATA | ParseFlags::SKIP_NOT_DATA;
    for rec in MsReader::new(data, flags).take(1000) {
        if rec.is_err() {
            break;
        }
    }

    let mut traces = TraceList::with_options(TraceListOptions {
        record_list: RecordList::WithExtras,
        ..Default::default()
    });
    if traces.read_buffer(data, &ReadOptions::new(ParseFlags::UNPACK_DATA)).is_ok() {
        for seg in traces.segments_mut() {
            let _ = seg.decode_record_list(Some(data));
        }
    }
});

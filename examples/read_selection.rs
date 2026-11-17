//! Read a file into a trace list limited by data selections (port of `lm_read_selection.c`).
//!
//! Usage: `cargo run --example read_selection -- file.mseed selections.txt`

use libmseed::time::{Subseconds, TimeFormat};
use libmseed::{NsTime, ParseFlags, ReadOptions, Selections, TraceList};

fn iso(t: NsTime) -> String {
    t.format(TimeFormat::IsoMonthDayZ, Subseconds::NanoMicroNone)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("Usage: read_selection <mseedfile> <selectionfile>");
        std::process::exit(1);
    }

    let mut selections = Selections::new();
    if let Err(e) = selections.read_file(&args[1]) {
        eprintln!("Cannot read data selection file: {e}");
        std::process::exit(1);
    }

    let mut traces = TraceList::new();
    let options = ReadOptions {
        selections: Some(selections),
        ..ReadOptions::new(ParseFlags::VALIDATE_CRC | ParseFlags::UNPACK_DATA)
    };
    if let Err(e) = traces.read_file(&args[0], &options) {
        eprintln!("Cannot read miniSEED from file: {e}");
        std::process::exit(1);
    }

    for id in traces.ids() {
        println!(
            "TraceID for {} ({}), earliest: {}, latest: {}, segments: {}",
            id.sid,
            id.pub_version,
            iso(id.earliest),
            iso(id.latest),
            id.num_segments()
        );
        for seg in &id.segments {
            let ty = seg.sample_type().map_or(' ', |t| t.code());
            println!(
                "  Segment {} - {}, samples: {}, sample rate: {}, sample type: {}",
                iso(seg.start_time),
                iso(seg.end_time),
                seg.num_samples(),
                seg.sample_rate,
                ty
            );
        }
    }
}

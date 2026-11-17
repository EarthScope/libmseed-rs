//! Read miniSEED from a memory buffer into a trace list (port of `lm_read_buffer.c`).
//!
//! Usage: `cargo run --example read_buffer -- file.mseed`

use libmseed::time::TimeFormat;
use libmseed::tracelist::TraceListing;
use libmseed::{ParseFlags, ReadOptions, TraceList};

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("read_buffer requires a single file name argument");
        std::process::exit(1);
    };
    let buffer = match std::fs::read(&path) {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => {
            eprintln!("Input file is empty");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error reading {path}: {e}");
            std::process::exit(1);
        }
    };

    let mut traces = TraceList::new();
    let options = ReadOptions::new(ParseFlags::VALIDATE_CRC | ParseFlags::UNPACK_DATA);
    let records = match traces.read_buffer(&buffer, &options) {
        Ok(n) => n as i64,
        Err(e) => {
            eprintln!("Problem reading miniSEED from buffer: {e}");
            -1
        }
    };

    let listing = TraceListing {
        time_format: TimeFormat::IsoMonthDay,
        details: true,
        gaps: true,
        versions: false,
    };
    print!("{}", traces.format_trace_list(&listing));
    eprintln!("Total records: {records}");
}

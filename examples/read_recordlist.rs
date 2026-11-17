//! Read a file into a trace list with record lists, optionally decoding and printing data
//! (port of `lm_read_recordlist.c`).
//!
//! Usage: `cargo run --example read_recordlist -- file.mseed [-d] [-D]`

use libmseed::time::{Subseconds, TimeFormat};
use libmseed::tracelist::RecordLocation;
use libmseed::{NsTime, ParseFlags, ReadOptions, RecordList, Samples, TraceList, TraceListOptions};

fn iso(t: NsTime) -> String {
    t.format(TimeFormat::IsoMonthDayZ, Subseconds::Nano)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("Usage: read_recordlist <mseedfile> [-d] [-D]");
        std::process::exit(1);
    };
    let mut printdata = ' ';
    for a in &args[1..] {
        if a.starts_with("-d") {
            printdata = 'd';
        } else if a.starts_with("-D") {
            printdata = 'D';
        }
    }

    // Validate CRCs and build a record list for each segment
    let mut traces = TraceList::with_options(TraceListOptions {
        record_list: RecordList::WithExtras,
        ..Default::default()
    });
    if let Err(e) = traces.read_file(path, &ReadOptions::new(ParseFlags::VALIDATE_CRC)) {
        eprintln!("Cannot read miniSEED from file: {e}");
        std::process::exit(1);
    }

    // Decode the data of each segment from its record list. No buffer is supplied, so the
    // records are read from the file again and the samples are held by the segment.
    if printdata != ' ' {
        for seg in traces.segments_mut() {
            if seg.record_list.as_ref().is_some_and(|l| !l.is_empty()) {
                if let Err(e) = seg.unpack_record_list(None) {
                    eprintln!("Cannot unpack samples: {e}");
                }
            }
        }
    }

    for id in traces.ids() {
        println!("TraceID for {} ({}), segments: {}", id.sid, id.pub_version, id.num_segments());
        for seg in &id.segments {
            println!(
                "  Segment {} - {}, samples: {}, sample rate: {}",
                iso(seg.start_time),
                iso(seg.end_time),
                seg.sample_count,
                seg.sample_rate
            );

            if let Some(list) = &seg.record_list {
                println!("  Record list:");
                for rp in list {
                    let (file, offset) = match &rp.location {
                        RecordLocation::File { path, offset } => {
                            (path.display().to_string(), *offset)
                        }
                        _ => ("(null)".to_string(), 0),
                    };
                    println!("    RECORD: bufferptr: NULL, fileptr: NULL, filename: {file}, fileoffset: {offset}");
                    println!("    Start: {}, End: {}", iso(rp.record.start_time), iso(rp.end_time));
                }
            }

            if printdata != ' ' {
                let Some(samples) = &seg.samples else { continue };
                println!(
                    "DATA ({} samples) of type '{}':",
                    samples.len(),
                    samples.sample_type().code()
                );
                match samples {
                    Samples::Text(t) => print!("{}", String::from_utf8_lossy(t)),
                    other => {
                        let n = other.len();
                        let lines = n / 6 + 1;
                        let mut idx = 0;
                        for _ in 0..lines {
                            let mut col = 0;
                            while col < 6 && idx < n {
                                match other {
                                    Samples::Int32(v) => print!("{:>10}  ", v[idx]),
                                    Samples::Float32(v) => {
                                        print!("{:>10}  ", v[idx])
                                    }
                                    Samples::Float64(v) => print!("{:>10}  ", v[idx]),
                                    Samples::Text(_) => {}
                                }
                                idx += 1;
                                col += 1;
                            }
                            println!();
                            if printdata == 'd' {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

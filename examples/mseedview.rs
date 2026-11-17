//! Print the records in miniSEED files and, optionally, the trace list they assemble into
//! (based on libmseed's `mseedview.c`, with the trace list options added).
//!
//! Usage: `cargo run --release --example mseedview -- [options] file[@start-end] ...`
//!
//! Options:
//! - `-p`: print full record header details; `-pp` also prints decoded samples
//! - `-d`: decode data payloads (reports decode errors without printing samples)
//! - `-T`: print a trace list after the records
//! - `-g`: include gaps in the trace list (implies `-T`)
//! - `-n`: no per-record output (only the trace list and summary)
//! - `-s`: print a summary of records and samples
//! - `-C`: validate CRCs

use libmseed::time::TimeFormat;
use libmseed::tracelist::TraceListing;
use libmseed::{MsReader, ParseFlags, Record, Samples, TraceList};

const USAGE: &str = "Usage: mseedview [-p[p]] [-d] [-T] [-g] [-n] [-s] [-C] file[@start-end] ...";

/// Print decoded samples, 6 values per line.
fn print_samples(record: &Record) {
    let Some(samples) = &record.samples else { return };
    match samples {
        Samples::Text(t) => println!("{}", String::from_utf8_lossy(t)),
        Samples::Int32(v) => print_rows(v),
        Samples::Float32(v) => print_rows(v),
        Samples::Float64(v) => print_rows(v),
    }
}

fn print_rows<T: std::fmt::Display>(values: &[T]) {
    for row in values.chunks(6) {
        let line: Vec<String> = row.iter().map(|v| format!("{v:>10}")).collect();
        println!("{}", line.join("  "));
    }
}

fn main() {
    let mut print_level = 0;
    let mut decode = false;
    let mut trace_list = false;
    let mut gaps = false;
    let mut quiet = false;
    let mut summary = false;
    let mut validate_crc = false;
    let mut files = Vec::new();

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "-d" => decode = true,
            "-T" => trace_list = true,
            "-g" => {
                trace_list = true;
                gaps = true;
            }
            "-n" => quiet = true,
            "-s" => summary = true,
            "-C" => validate_crc = true,
            a if a.len() > 1 && a.starts_with('-') && a[1..].chars().all(|c| c == 'p') => {
                print_level += a.len() - 1;
            }
            a if a.starts_with('-') => {
                eprintln!("Unknown option: {a}\n{USAGE}");
                std::process::exit(1);
            }
            _ => files.push(arg),
        }
    }
    if files.is_empty() {
        eprintln!("{USAGE}");
        std::process::exit(1);
    }

    let mut flags = ParseFlags::empty();
    if decode || print_level >= 2 {
        flags |= ParseFlags::UNPACK_DATA;
    }
    if validate_crc {
        flags |= ParseFlags::VALIDATE_CRC;
    }

    let mut traces = TraceList::new();
    let mut total_records = 0u64;
    let mut total_samples = 0u64;
    let mut failed = false;

    for file in &files {
        let mut reader = match MsReader::open_path_range(file, flags) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error opening {file}: {e}");
                failed = true;
                continue;
            }
        };
        loop {
            let record = match reader.next_record() {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err(e) => {
                    eprintln!(
                        "Error reading {file} (record near offset {}): {e}",
                        reader.last_record_offset()
                    );
                    failed = true;
                    break;
                }
            };
            total_records += 1;
            total_samples += record.sample_count;

            if !quiet {
                if print_level == 0 {
                    println!("{record}");
                } else {
                    print!("{record:#}");
                    if print_level >= 2 {
                        print_samples(&record);
                    }
                }
            }
            if trace_list && let Err(e) = traces.add_record(&record) {
                eprintln!("Error adding {} to trace list: {e}", record.sid);
                failed = true;
            }
        }
    }

    if trace_list {
        let listing = TraceListing {
            time_format: TimeFormat::IsoMonthDayZ,
            details: true,
            gaps,
            versions: true,
        };
        print!("{}", traces.format_trace_list(&listing));
    }
    if summary {
        println!("Files: {}, Records: {total_records}, Samples: {total_samples}", files.len());
    }
    if failed {
        std::process::exit(1);
    }
}

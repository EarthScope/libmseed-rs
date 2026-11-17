//! Rolling-buffer operation: add data to a trace list as it arrives and pack full records
//! when they can be created (illustrates `mstl3_pack` usage as in `lm_pack_rollingbuffer.c`).
//!
//! Run with `cargo run --example pack_rollingbuffer`; writes `rolling.mseed3` in the
//! current directory.

use libmseed::{Encoding, NsTime, PackOptions, Packed, Record, Samples, TraceList};

fn main() -> libmseed::Result<()> {
    let start = NsTime::parse("2024-01-01T00:00:00Z")?;
    let rate = 100.0;
    let mut traces = TraceList::new();
    let mut out = std::fs::File::create("rolling.mseed3")?;
    let mut total_records = 0;

    // Simulate a data acquisition feed delivering 250 samples at a time
    let mut next = 0u64;
    for chunk in 0..8 {
        let mut record = Record::new("FDSN:XX_TEST__H_H_Z", start.sample_time(next, rate)?);
        record.sample_rate = rate;
        let samples: Vec<i32> =
            (next..next + 250).map(|i| (1000.0 * (i as f64 / 20.0).sin()) as i32).collect();
        record.sample_count = samples.len() as u64;
        record.samples = Some(Samples::Int32(samples));
        next += 250;
        traces.add_record(&record)?;

        // Create records from what can fill them, the remainder stays in the list. On the
        // last chunk flush everything.
        let options = PackOptions {
            record_length: Some(512),
            encoding: Some(Encoding::STEIM2),
            flush: chunk == 7,
            ..Default::default()
        };
        let Packed { records, samples } = traces.pack_with(&options, |rec| {
            use std::io::Write;
            out.write_all(rec)?;
            Ok(())
        })?;
        total_records += records;
        let held: usize =
            traces.ids().iter().flat_map(|i| &i.segments).map(|s| s.num_samples()).sum();
        println!(
            "chunk {chunk}: packed {records} records ({samples} samples), {held} samples held"
        );
    }
    println!("Total records written: {total_records}");
    Ok(())
}

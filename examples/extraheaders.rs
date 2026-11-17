//! Set, get and modify extra headers (port of `lm_extraheaders.c`).
//!
//! Run with `cargo run --example extraheaders`.

use libmseed::extra::format_extra;
use libmseed::{NsTime, Record};

fn main() -> libmseed::Result<()> {
    let mut record = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::now());

    println!("Setting FDSN and custom headers:");
    // Parse once, set many values, serialize once
    let mut headers = record.extra_headers()?;
    headers.set_u64("/FDSN/Time/Quality", 100)?;
    headers.set_i64("/FDSN/Time/LeapSecond", 1)?;
    headers.set_f64("/FDSN/Time/Correction", -0.0235)?;
    headers.set_bool("/FDSN/Event/Begin", true)?;
    headers.set_str("/Endor/Shield/Status", "Disabled")?;
    headers.set_str("/Endor/Shield/BootTime", "2024-01-02T03:04:05.123456Z")?;
    record.set_extra_headers(&headers)?;

    println!("\n==== Printing all extra headers ====");
    print!("{}", format_extra(&record.extra, 4));

    println!("\n==== Checking existence of headers ====");
    let headers = record.extra_headers()?;
    for ptr in ["/FDSN/Time/Quality", "/FDSN/Time/MaxEstimatedError"] {
        println!("  {ptr} {}", if headers.exists(ptr) { "exists" } else { "DOES NOT exist" });
    }
    println!("  Got /FDSN/Time/Quality = {}", headers.get_u64("/FDSN/Time/Quality")?.unwrap());
    println!(
        "  Got /FDSN/Time/LeapSecond = {}",
        headers.get_i64("/FDSN/Time/LeapSecond")?.unwrap()
    );
    println!(
        "  Got /FDSN/Time/Correction = {:.6}",
        headers.get_f64("/FDSN/Time/Correction")?.unwrap()
    );
    println!("  Got /FDSN/Event/Begin = {}", headers.get_bool("/FDSN/Event/Begin")?.unwrap());
    println!(
        "  Got /Endor/Shield/Status = {:?}",
        headers.get_str("/Endor/Shield/Status")?.unwrap()
    );

    println!("\n==== Checking header types ====");
    for ptr in ["/FDSN/Time", "/FDSN/Time/Quality", "/Endor/Shield/BootTime"] {
        let ty = headers.value_type(ptr)?.map_or("not found".to_string(), |t| format!("{t:?}"));
        println!("  {ptr} type: {ty}");
    }

    println!("\n==== Apply JSON Merge Patch to modify headers ====");
    record.merge_extra("", r#"{"FDSN":{"Time":{"LeapSecond":null,"Quality":50}},"Endor":null}"#)?;
    print!("{}", format_extra(&record.extra, 4));
    Ok(())
}

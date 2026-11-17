//! Convert time strings to the supported output formats (port of `lm_timestr.c`).
//!
//! Usage: `cargo run --example timestr -- 2004-05-12T07:08:09.123456789Z [...]`

use libmseed::time::{Subseconds, TimeFormat};
use libmseed::NsTime;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: timestr timestring1 [timestring2] [...]");
        std::process::exit(1);
    }
    for timestr in &args {
        println!("Input time    : {timestr}");
        let nstime = match NsTime::parse(timestr) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Cannot convert time string to epoch format: '{timestr}': {e}");
                std::process::exit(1);
            }
        };
        let fmt = |f| nstime.format(f, Subseconds::NanoMicroNone);
        println!("ISOMONTH      : {}", fmt(TimeFormat::IsoMonthDay));
        println!("ISOMONTH_Z    : {}", fmt(TimeFormat::IsoMonthDayZ));
        println!("ISOMONTH_DOY  : {}", fmt(TimeFormat::IsoMonthDayDoy));
        println!("ISOMONTH_DOY_Z: {}", fmt(TimeFormat::IsoMonthDayDoyZ));
        println!("SEEDORDINAL   : {}", fmt(TimeFormat::SeedOrdinal));
        println!("UNIXEPOCH     : {}", fmt(TimeFormat::UnixEpoch));
        println!("NSEPOCH       : {}", fmt(TimeFormat::NanosecondEpoch));
        println!("nstime_t      : {}\n", nstime.as_nanos());
    }
}

//! Parse arbitrary bytes as a record, then exercise everything that reads a parsed record.
#![no_main]

use libfuzzer_sys::fuzz_target;
use libmseed::{FormatVersion, ParseFlags, Record};

fuzz_target!(|data: &[u8]| {
    for flags in [ParseFlags::UNPACK_DATA | ParseFlags::VALIDATE_CRC, ParseFlags::END_OF_INPUT] {
        if let Ok(mut rec) = Record::parse(data, flags) {
            let _ = format!("{rec:#}");
            let _ = rec.end_time();
            let _ = rec.extra_headers();
            let _ = rec.unpack_data();
            let _ = rec.repack(FormatVersion::V3);
            let _ = rec.repack(FormatVersion::V2);
        }
    }
});

//! Pack arbitrary integer samples with each integer encoding and format, and check that
//! the records parse back to the same samples.
#![no_main]

use libfuzzer_sys::fuzz_target;
use libmseed::{
    Encoding, FormatVersion, MsReader, NsTime, PackOptions, ParseFlags, Record, Samples,
};

fuzz_target!(|data: &[u8]| {
    let samples: Vec<i32> =
        data.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    if samples.is_empty() {
        return;
    }
    let start = NsTime::from_nanos(1_700_000_000_000_000_000);
    for encoding in [Encoding::INT32, Encoding::STEIM1, Encoding::STEIM2] {
        for format in [FormatVersion::V2, FormatVersion::V3] {
            let mut rec = Record::new("FDSN:XX_FUZZ__B_H_Z", start);
            rec.sample_rate = 100.0;
            rec.samples = Some(Samples::Int32(samples.clone()));
            let options = PackOptions {
                format: Some(format),
                encoding: Some(encoding),
                record_length: Some(512),
                flush: true,
                ..Default::default()
            };
            // Steim cannot represent every difference; failing to pack is allowed
            let Ok(records) = rec.pack(&options) else { continue };
            let bytes = records.concat();
            let mut back = Vec::new();
            for r in MsReader::new(&bytes[..], ParseFlags::UNPACK_DATA) {
                let r = r.expect("packed records parse");
                back.extend_from_slice(r.samples.as_ref().unwrap().as_i32().unwrap());
            }
            assert_eq!(back, samples, "{encoding:?} {format:?}");
        }
    }
});

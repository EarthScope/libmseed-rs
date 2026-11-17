//! Run with `wasm-pack test --node libmseed-wasm`.

use libmseed_wasm::*;
use wasm_bindgen_test::*;

#[wasm_bindgen_test]
fn pack_parse_roundtrip() {
    let samples: Vec<i32> = (0..500).collect();
    let bytes =
        pack_int32("FDSN:XX_TEST__B_H_Z", "2024-01-01T00:00:00Z", 40.0, &samples, 3, 512, false)
            .map_err(|_| ())
            .unwrap();
    let records = parse_records(&bytes, true).map_err(|_| ()).unwrap();
    assert!(records.len() > 1);
    assert_eq!(records[0].sid(), "FDSN:XX_TEST__B_H_Z");
    assert_eq!(records[0].start_time(), "2024-01-01T00:00:00Z");
    assert_eq!(records[0].sample_type(), "i");

    let mut got = Vec::new();
    for r in &records {
        got.extend(r.samples_int32().unwrap());
    }
    assert_eq!(got, samples);
}

#[wasm_bindgen_test]
fn trace_list() {
    let samples: Vec<f32> = (0..300).map(|i| i as f32 * 0.5).collect();
    let bytes =
        pack_float32("FDSN:XX_TEST__B_H_Z", "2024-01-01T00:00:00Z", 20.0, &samples, 512, true)
            .map_err(|_| ())
            .unwrap();
    let mut tl = TraceList::new();
    tl.add_buffer(&bytes, true).map_err(|_| ()).unwrap();
    assert_eq!(tl.num_ids(), 1);
    assert_eq!(tl.num_segments(0), 1);
    let seg = tl.segment(0, 0).unwrap();
    assert_eq!(seg.samples_float32().unwrap(), samples);
}

#[wasm_bindgen_test]
fn errors_are_reported() {
    assert!(
        parse_records(b"this is not miniSEED data at all, really not", false).is_err()
            || parse_records(b"this is not miniSEED data at all, really not", false)
                .unwrap()
                .is_empty()
    );
    assert!(time_to_ns("not a time").is_err());
    assert_eq!(time_to_ns("1970-01-01T00:00:01Z").map_err(|_| ()).unwrap(), 1_000_000_000);
}

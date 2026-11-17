//! Malformed input must produce errors, never panics or unbounded allocations.
//!
//! A deterministic, stable-toolchain complement to the cargo-fuzz targets in `fuzz/`:
//! records from the test data are mutated (byte flips, truncation, corrupted length and
//! count fields) and parsed, read and repacked.

mod common;

use common::data;
use libmseed::{
    Encoding, FormatVersion, MsReader, NsTime, PackOptions, ParseFlags, ReadOptions, Record,
    RecordList, Samples, TraceList, TraceListOptions,
};

/// xorshift64*, enough for reproducible mutations.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The first record of each test data file.
fn seed_records() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(data("")).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        if let Ok(rec) = Record::parse(&bytes, ParseFlags::END_OF_INPUT) {
            out.push(bytes[..rec.reclen.unwrap()].to_vec());
        }
    }
    assert!(out.len() > 20, "test data records found: {}", out.len());
    out
}

fn exercise(bytes: &[u8]) {
    for flags in [ParseFlags::UNPACK_DATA | ParseFlags::VALIDATE_CRC, ParseFlags::END_OF_INPUT] {
        if let Ok(mut rec) = Record::parse(bytes, flags) {
            let _ = format!("{rec:#}");
            let _ = rec.end_time();
            let _ = rec.extra_headers();
            let _ = rec.unpack_data();
            let _ = rec.repack(FormatVersion::V3);
            let _ = rec.repack(FormatVersion::V2);
        }
    }
    for rec in MsReader::new(bytes, ParseFlags::UNPACK_DATA | ParseFlags::SKIP_NOT_DATA).take(50) {
        if rec.is_err() {
            break;
        }
    }
    let mut traces = TraceList::with_options(TraceListOptions {
        record_list: RecordList::WithExtras,
        ..Default::default()
    });
    if traces.read_buffer(bytes, &ReadOptions::new(ParseFlags::UNPACK_DATA)).is_ok() {
        for seg in traces.segments_mut() {
            let _ = seg.decode_record_list(Some(bytes));
        }
    }
}

#[test]
fn mutated_records_do_not_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for seed in seed_records() {
        exercise(&seed);
        for _ in 0..60 {
            let mut b = seed.clone();
            match rng.below(4) {
                // Flip bytes, mostly in the headers
                0 => {
                    for _ in 0..1 + rng.below(4) {
                        let i = rng.below(b.len().min(128));
                        b[i] ^= 1 << rng.below(8);
                    }
                }
                // Random bytes anywhere
                1 => {
                    for _ in 0..1 + rng.below(8) {
                        let i = rng.below(b.len());
                        b[i] = rng.next() as u8;
                    }
                }
                // Truncate
                2 => b.truncate(rng.below(b.len())),
                // Extreme length and count fields (v3 and v2 layouts)
                _ => {
                    let v = [0u32, 1, 0xFFFF, 0x7FFF_FFFF, u32::MAX][rng.below(5)];
                    let at = [24, 30, 33, 34, 36, 44, 46][rng.below(7)];
                    let n = b.len().min(at + 4);
                    b[at..n].copy_from_slice(&v.to_le_bytes()[..n - at]);
                }
            }
            exercise(&b);
        }
    }
}

#[test]
fn random_samples_roundtrip() {
    let mut rng = Rng(42);
    let start = NsTime::from_nanos(1_700_000_000_000_000_000);
    for round in 0..40 {
        let n = 1 + rng.below(2000);
        // Mix small and large differences so all Steim widths are used
        let shift = round % 32;
        let samples: Vec<i32> = (0..n).map(|_| (rng.next() as i32) >> shift).collect();
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
                let Ok(records) = rec.pack(&options) else {
                    assert_ne!(encoding, Encoding::INT32);
                    continue;
                };
                let bytes = records.concat();
                let mut back = Vec::new();
                for r in MsReader::new(&bytes[..], ParseFlags::UNPACK_DATA) {
                    let r = r.unwrap();
                    back.extend_from_slice(r.samples.as_ref().unwrap().as_i32().unwrap());
                }
                assert_eq!(back, samples, "{encoding:?} {format:?} shift {shift}");
            }
        }
    }
}

/// A miniSEED 2 time correction that moves the start time beyond the representable range
/// is an error, not an overflow (a panic in debug builds, a wrapped time in release).
#[test]
fn v2_time_correction_overflow_is_an_error() {
    let mut rec = Record::new("FDSN:XX_TEST__B_H_Z", "2262-04-11T23:47:16Z".parse().unwrap());
    rec.sample_rate = 1.0;
    rec.samples = Some(Samples::Int32(vec![1, 2, 3]));
    let options = PackOptions {
        format: Some(FormatVersion::V2),
        record_length: Some(512),
        flush: true,
        ..Default::default()
    };
    let mut bytes = rec.pack(&options).unwrap().remove(0);
    assert!(Record::parse(&bytes, ParseFlags::empty()).is_ok());

    // Maximum positive time correction, not yet applied (activity flag bit 1 clear)
    bytes[40..44].copy_from_slice(&i32::MAX.to_be_bytes());
    bytes[36] &= !0x02;
    let err = Record::parse(&bytes, ParseFlags::empty()).unwrap_err();
    assert!(matches!(err, libmseed::Error::InvalidRecord(_)), "{err}");
}

/// After an error for one record, `next_record` continues with the following record.
#[test]
fn reader_continues_after_a_bad_record() {
    let start: NsTime = "2024-01-01T00:00:00Z".parse().unwrap();
    let mut rec = Record::new("FDSN:XX_TEST__B_H_Z", start);
    rec.sample_rate = 1.0;
    rec.samples = Some(Samples::Int32((0..300).collect()));
    let options = PackOptions {
        encoding: Some(Encoding::INT32),
        record_length: Some(512),
        flush: true,
        ..Default::default()
    };
    let records = rec.pack(&options).unwrap();
    assert_eq!(records.len(), 3);
    let mut bytes = records.concat();
    // Corrupt a data byte of the second record, invalidating its CRC
    let second = records[0].len();
    bytes[second + 100] ^= 0xFF;

    let mut reader = MsReader::new(&bytes[..], ParseFlags::VALIDATE_CRC);
    assert!(reader.next_record().unwrap().is_some());
    let err = reader.next_record().unwrap_err();
    assert!(matches!(err, libmseed::Error::InvalidCrc(_)), "{err}");
    let third = reader.next_record().unwrap().expect("third record");
    assert_eq!(reader.last_record_offset() as usize, second + records[1].len());
    assert_eq!(third.raw().unwrap(), &records[2][..]);
    assert!(reader.next_record().unwrap().is_none());

    // Not miniSEED: the error is returned once and reading ends
    let mut reader = MsReader::new(&[0u8; 100][..], ParseFlags::empty());
    assert!(reader.next_record().is_err());
    assert!(reader.next_record().unwrap().is_none());
}

/// Float and text samples use their fixed encoding whatever integer encoding is requested.
#[test]
fn requested_encoding_applies_to_integer_samples_only() {
    let mut rec = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    rec.sample_rate = 1.0;
    rec.samples = Some(Samples::Float32(vec![1.5, 2.5, 3.5]));
    let options =
        PackOptions { encoding: Some(Encoding::STEIM2), flush: true, ..Default::default() };
    let bytes = rec.pack(&options).unwrap().concat();
    let back = Record::parse(&bytes, ParseFlags::UNPACK_DATA).unwrap();
    assert_eq!(back.encoding, Some(Encoding::FLOAT32));
    assert_eq!(back.samples.unwrap().as_f32().unwrap(), &[1.5, 2.5, 3.5]);
}

/// Integer samples outside the 16-bit range cannot be packed as INT16.
#[test]
fn int16_out_of_range_is_an_error() {
    let mut rec = Record::new("FDSN:XX_TEST__B_H_Z", NsTime::UNIX_EPOCH);
    rec.sample_rate = 1.0;
    rec.samples = Some(Samples::Int32(vec![1, 40_000, 3]));
    let options =
        PackOptions { encoding: Some(Encoding::INT16), flush: true, ..Default::default() };
    let err = rec.pack(&options).unwrap_err();
    assert!(matches!(err, libmseed::Error::Samples(_)), "{err}");

    rec.samples = Some(Samples::Int32(vec![i16::MIN as i32, 0, i16::MAX as i32]));
    assert!(rec.pack(&options).is_ok());
}

/// Reading a buffer gives the same records as reading the file, including a final
/// miniSEED 2 record without blockette 1000, and honors SKIP_NOT_DATA.
#[test]
fn read_buffer_matches_read_file() {
    let path = data("testdata-no-blockette1000-steim1.mseed2");
    let options = ReadOptions::new(ParseFlags::UNPACK_DATA);
    let mut from_file = TraceList::new();
    let n_file = from_file.read_file(&path, &options).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let mut from_buffer = TraceList::new();
    let n_buffer = from_buffer.read_buffer(&bytes, &options).unwrap();
    assert!(n_file > 0);
    assert_eq!(n_buffer, n_file);

    // Leading garbage is skipped with SKIP_NOT_DATA, an error without
    let mut garbage = vec![0xFFu8; 77];
    garbage.extend_from_slice(&bytes);
    assert!(TraceList::new().read_buffer(&garbage, &options).is_err());
    let skip = ReadOptions::new(ParseFlags::UNPACK_DATA | ParseFlags::SKIP_NOT_DATA);
    assert_eq!(TraceList::new().read_buffer(&garbage, &skip).unwrap(), n_file);
}

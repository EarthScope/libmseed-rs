//! Encoders for miniSEED data encodings.

use super::{ByteOrder, Samples};
use crate::error::{Error, Result};
use crate::lookup::Encoding;

/// Result of encoding: how many samples were consumed and the encoded bytes.
pub(crate) struct Encoded {
    pub samples: usize,
    pub data: Vec<u8>,
}

fn put_u16(order: ByteOrder, v: u16, out: &mut [u8]) {
    out[..2].copy_from_slice(&match order {
        ByteOrder::Little => v.to_le_bytes(),
        ByteOrder::Big => v.to_be_bytes(),
    });
}
fn put_u32(order: ByteOrder, v: u32, out: &mut [u8]) {
    out[..4].copy_from_slice(&match order {
        ByteOrder::Little => v.to_le_bytes(),
        ByteOrder::Big => v.to_be_bytes(),
    });
}
fn put_u64(order: ByteOrder, v: u64, out: &mut [u8]) {
    out[..8].copy_from_slice(&match order {
        ByteOrder::Little => v.to_le_bytes(),
        ByteOrder::Big => v.to_be_bytes(),
    });
}

/// Encode up to `count` samples starting at `start` into at most `capacity` bytes.
///
/// `diff0` is the first difference for Steim encodings (use 0 if unknown).
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode(
    samples: &Samples,
    start: usize,
    count: usize,
    encoding: Encoding,
    capacity: usize,
    order: ByteOrder,
    diff0: i32,
    sid: &str,
) -> Result<Encoded> {
    let type_err = || {
        Error::UnsupportedEncoding(format!(
            "{sid}: Sample type {:?} is not compatible with encoding {}",
            samples.sample_type(),
            encoding.0
        ))
    };
    match encoding {
        Encoding::TEXT => {
            let Samples::Text(v) = samples else { return Err(type_err()) };
            let n = count.min(v.len() - start).min(capacity);
            Ok(Encoded { samples: n, data: v[start..start + n].to_vec() })
        }
        Encoding::INT16 => {
            let Samples::Int32(v) = samples else { return Err(type_err()) };
            let n = count.min(v.len() - start).min(capacity / 2);
            let mut data = vec![0u8; n * 2];
            for (i, s) in v[start..start + n].iter().enumerate() {
                let Ok(s) = i16::try_from(*s) else {
                    return Err(Error::Samples(format!(
                        "{sid}: Sample value {s} at index {} is out of range for INT16 encoding",
                        start + i
                    )));
                };
                put_u16(order, s as u16, &mut data[i * 2..]);
            }
            Ok(Encoded { samples: n, data })
        }
        Encoding::INT32 => {
            let Samples::Int32(v) = samples else { return Err(type_err()) };
            let n = count.min(v.len() - start).min(capacity / 4);
            let mut data = vec![0u8; n * 4];
            for (i, s) in v[start..start + n].iter().enumerate() {
                put_u32(order, *s as u32, &mut data[i * 4..]);
            }
            Ok(Encoded { samples: n, data })
        }
        Encoding::FLOAT32 => {
            let Samples::Float32(v) = samples else { return Err(type_err()) };
            let n = count.min(v.len() - start).min(capacity / 4);
            let mut data = vec![0u8; n * 4];
            for (i, s) in v[start..start + n].iter().enumerate() {
                put_u32(order, s.to_bits(), &mut data[i * 4..]);
            }
            Ok(Encoded { samples: n, data })
        }
        Encoding::FLOAT64 => {
            let Samples::Float64(v) = samples else { return Err(type_err()) };
            let n = count.min(v.len() - start).min(capacity / 8);
            let mut data = vec![0u8; n * 8];
            for (i, s) in v[start..start + n].iter().enumerate() {
                put_u64(order, s.to_bits(), &mut data[i * 8..]);
            }
            Ok(Encoded { samples: n, data })
        }
        Encoding::STEIM1 => {
            let Samples::Int32(v) = samples else { return Err(type_err()) };
            Ok(steim1(&v[start..start + count.min(v.len() - start)], capacity, order, diff0))
        }
        Encoding::STEIM2 => {
            let Samples::Int32(v) = samples else { return Err(type_err()) };
            steim2(&v[start..start + count.min(v.len() - start)], capacity, order, diff0, sid)
        }
        _ => Err(Error::UnsupportedEncoding(format!(
            "{sid}: Unsupported encoding for packing: {} ({})",
            encoding.0,
            encoding.description()
        ))),
    }
}

/// Number of bits (of 4,5,6,8,10,15,16,30,32) needed to represent a value.
fn bitwidth(v: i32) -> i32 {
    match v {
        -8..=7 => 4,
        -16..=15 => 5,
        -32..=31 => 6,
        -128..=127 => 8,
        -512..=511 => 10,
        -16384..=16383 => 15,
        -32768..=32767 => 16,
        -536870912..=536870911 => 30,
        _ => 32,
    }
}

fn steim1(input: &[i32], capacity: usize, order: ByteOrder, diff0: i32) -> Encoded {
    let samplecount = input.len();
    if samplecount == 0 {
        return Encoded { samples: 0, data: Vec::new() };
    }
    let maxframes = capacity / 64;
    let mut out = vec![0u8; maxframes * 64];
    let mut diffs = [0i32; 4];
    let mut bw = [0i32; 4];
    let mut inputidx = 0usize;
    let mut outputsamples = 0usize;
    let mut diffcount = 1usize;
    let mut packed = 0usize;
    let mut frameidx = 0usize;
    let mut first_frame_written = false;

    diffs[0] = diff0;
    bw[0] = bitwidth(diff0);

    while frameidx < maxframes && outputsamples < samplecount {
        let frame = &mut out[frameidx * 64..frameidx * 64 + 64];
        let mut nibbles = 0u32;
        let startnibble;
        if frameidx == 0 {
            put_u32(order, input[0] as u32, &mut frame[4..]);
            first_frame_written = true;
            startnibble = 3;
        } else {
            startnibble = 1;
        }

        let mut widx = startnibble;
        while widx < 16 && outputsamples < samplecount {
            if diffcount < 4 {
                for idx in 0..diffcount {
                    diffs[idx] = diffs[packed + idx];
                    bw[idx] = bw[packed + idx];
                }
                let mut idx = diffcount;
                while idx < 4 && inputidx < samplecount - 1 {
                    diffs[idx] = input[inputidx + 1].wrapping_sub(input[inputidx]);
                    bw[idx] = bitwidth(diffs[idx]);
                    diffcount += 1;
                    idx += 1;
                    inputidx += 1;
                }
            }

            let w = &mut frame[widx * 4..widx * 4 + 4];
            if diffcount == 4 && bw.iter().all(|&b| b <= 8) {
                for i in 0..4 {
                    w[i] = diffs[i] as i8 as u8;
                }
                nibbles |= 0x1 << (30 - 2 * widx);
                packed = 4;
            } else if diffcount >= 2 && bw[0] <= 16 && bw[1] <= 16 {
                put_u16(order, diffs[0] as i16 as u16, &mut w[0..2]);
                put_u16(order, diffs[1] as i16 as u16, &mut w[2..4]);
                nibbles |= 0x2 << (30 - 2 * widx);
                packed = 2;
            } else {
                put_u32(order, diffs[0] as u32, w);
                nibbles |= 0x3 << (30 - 2 * widx);
                packed = 1;
            }
            diffcount -= packed;
            outputsamples += packed;
            widx += 1;
        }
        put_u32(order, nibbles, &mut frame[0..4]);
        frameidx += 1;
    }

    if first_frame_written {
        put_u32(order, input[outputsamples - 1] as u32, &mut out[8..]);
    }
    out.truncate(frameidx * 64);
    Encoded { samples: outputsamples, data: out }
}

fn steim2(
    input: &[i32],
    capacity: usize,
    order: ByteOrder,
    diff0: i32,
    sid: &str,
) -> Result<Encoded> {
    let samplecount = input.len();
    if samplecount == 0 {
        return Ok(Encoded { samples: 0, data: Vec::new() });
    }
    let maxframes = capacity / 64;
    let mut out = vec![0u8; maxframes * 64];
    let mut diffs = [0i32; 7];
    let mut bw = [0i32; 7];
    let mut inputidx = 0usize;
    let mut outputsamples = 0usize;
    let mut diffcount = 1usize;
    let mut packed = 0usize;
    let mut frameidx = 0usize;
    let mut first_frame_written = false;

    diffs[0] = diff0;
    bw[0] = bitwidth(diff0);

    while frameidx < maxframes && outputsamples < samplecount {
        let frame = &mut out[frameidx * 64..frameidx * 64 + 64];
        let mut nibbles = 0u32;
        let startnibble;
        if frameidx == 0 {
            put_u32(order, input[0] as u32, &mut frame[4..]);
            first_frame_written = true;
            startnibble = 3;
        } else {
            startnibble = 1;
        }

        let mut widx = startnibble;
        while widx < 16 && outputsamples < samplecount {
            if diffcount < 7 {
                for idx in 0..diffcount {
                    diffs[idx] = diffs[packed + idx];
                    bw[idx] = bw[packed + idx];
                }
                let mut idx = diffcount;
                while idx < 7 && inputidx < samplecount - 1 {
                    diffs[idx] = input[inputidx + 1].wrapping_sub(input[inputidx]);
                    bw[idx] = bitwidth(diffs[idx]);
                    diffcount += 1;
                    idx += 1;
                    inputidx += 1;
                }
            }

            let d = |i: usize| diffs[i] as u32;
            let nib_shift = 30 - 2 * widx;
            let mut word = 0u32;
            let mut raw8 = false;

            if diffcount == 7 && bw.iter().all(|&b| b <= 4) {
                word = (d(6) & 0xF)
                    | (d(5) & 0xF) << 4
                    | (d(4) & 0xF) << 8
                    | (d(3) & 0xF) << 12
                    | (d(2) & 0xF) << 16
                    | (d(1) & 0xF) << 20
                    | (d(0) & 0xF) << 24
                    | 0x2 << 30;
                nibbles |= 0x3 << nib_shift;
                packed = 7;
            } else if diffcount >= 6 && bw[..6].iter().all(|&b| b <= 5) {
                word = (d(5) & 0x1F)
                    | (d(4) & 0x1F) << 5
                    | (d(3) & 0x1F) << 10
                    | (d(2) & 0x1F) << 15
                    | (d(1) & 0x1F) << 20
                    | (d(0) & 0x1F) << 25
                    | 0x1 << 30;
                nibbles |= 0x3 << nib_shift;
                packed = 6;
            } else if diffcount >= 5 && bw[..5].iter().all(|&b| b <= 6) {
                word = (d(4) & 0x3F)
                    | (d(3) & 0x3F) << 6
                    | (d(2) & 0x3F) << 12
                    | (d(1) & 0x3F) << 18
                    | (d(0) & 0x3F) << 24;
                nibbles |= 0x3 << nib_shift;
                packed = 5;
            } else if diffcount >= 4 && bw[..4].iter().all(|&b| b <= 8) {
                for i in 0..4 {
                    frame[widx * 4 + i] = diffs[i] as i8 as u8;
                }
                raw8 = true;
                nibbles |= 0x1 << nib_shift;
                packed = 4;
            } else if diffcount >= 3 && bw[..3].iter().all(|&b| b <= 10) {
                word = (d(2) & 0x3FF) | (d(1) & 0x3FF) << 10 | (d(0) & 0x3FF) << 20 | 0x3 << 30;
                nibbles |= 0x2 << nib_shift;
                packed = 3;
            } else if diffcount >= 2 && bw[0] <= 15 && bw[1] <= 15 {
                word = (d(1) & 0x7FFF) | (d(0) & 0x7FFF) << 15 | 0x2 << 30;
                nibbles |= 0x2 << nib_shift;
                packed = 2;
            } else if diffcount >= 1 && bw[0] <= 30 {
                word = (d(0) & 0x3FFF_FFFF) | 0x1 << 30;
                nibbles |= 0x2 << nib_shift;
                packed = 1;
            } else {
                return Err(Error::Pack(format!(
                    "{sid}: Unable to represent difference in <= 30 bits"
                )));
            }

            if !raw8 {
                put_u32(order, word, &mut frame[widx * 4..]);
            }
            diffcount -= packed;
            outputsamples += packed;
            widx += 1;
        }
        put_u32(order, nibbles, &mut frame[0..4]);
        frameidx += 1;
    }

    if first_frame_written {
        put_u32(order, input[outputsamples - 1] as u32, &mut out[8..]);
    }
    out.truncate(frameidx * 64);
    Ok(Encoded { samples: outputsamples, data: out })
}

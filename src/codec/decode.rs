//! Decoders for miniSEED data encodings.

use super::{ByteOrder, Samples};
use crate::error::{Error, Result};
use crate::lookup::Encoding;

const MAX12: i32 = 0x7FF;
const MAX14: i32 = 0x1FFF;
const MAX16: i32 = 0x7FFF;
const MAX24: i32 = 0x7F_FFFF;

fn sext(v: u32, bits: u32) -> i32 {
    ((v << (32 - bits)) as i32) >> (32 - bits)
}

/// Decode `samplecount` samples from an encoded payload.
///
/// `order` is the byte order of the encoded data. `sid` is used in diagnostics.
/// Fails if the payload cannot hold the requested samples or contains invalid data.
pub(crate) fn decode(
    input: &[u8],
    encoding: Encoding,
    samplecount: usize,
    order: ByteOrder,
    sid: &str,
) -> Result<Samples> {
    let Some(sampletype) = encoding.sample_type() else {
        return Err(Error::UnsupportedEncoding(format!(
            "{sid}: Unsupported encoding format {} ({})",
            encoding.0,
            encoding.description()
        )));
    };

    if samplecount == 0 {
        return Ok(Samples::empty(sampletype));
    }

    let inputsamplebytes = match encoding {
        Encoding::TEXT => 1,
        Encoding::INT16
        | Encoding::GEOSCOPE163
        | Encoding::GEOSCOPE164
        | Encoding::CDSN
        | Encoding::SRO
        | Encoding::DWWSSN => 2,
        Encoding::GEOSCOPE24 => 3,
        Encoding::INT32 | Encoding::FLOAT32 => 4,
        Encoding::FLOAT64 => 8,
        _ => 0,
    };
    if inputsamplebytes != 0 && input.len() / inputsamplebytes < samplecount {
        return Err(Error::InvalidRecord(format!(
            "{sid}: Input buffer ({} bytes) is not large enough for {samplecount} samples of encoding {}",
            input.len(),
            encoding.0
        )));
    }

    // Reject sample counts the Steim frames cannot hold before allocating for them, so a
    // corrupt or hostile header cannot force a huge allocation
    let frame_max_samples = match encoding {
        Encoding::STEIM1 => crate::pack::STEIM1_FRAME_MAX_SAMPLES,
        Encoding::STEIM2 => crate::pack::STEIM2_FRAME_MAX_SAMPLES,
        _ => 0,
    };
    if frame_max_samples != 0 && (input.len() / 64).saturating_mul(frame_max_samples) < samplecount
    {
        return Err(Error::InvalidRecord(format!(
            "{sid}: Input buffer ({} bytes) is not large enough for {samplecount} samples of encoding {}",
            input.len(),
            encoding.0
        )));
    }

    let samples = match encoding {
        Encoding::TEXT => Samples::Text(input[..samplecount].to_vec()),
        Encoding::INT16 => Samples::Int32(
            input
                .as_chunks::<2>()
                .0
                .iter()
                .take(samplecount)
                .map(|c| order.u16(c) as i16 as i32)
                .collect(),
        ),
        Encoding::INT32 => Samples::Int32(
            input
                .as_chunks::<4>()
                .0
                .iter()
                .take(samplecount)
                .map(|c| order.u32(c) as i32)
                .collect(),
        ),
        Encoding::FLOAT32 => Samples::Float32(
            input
                .as_chunks::<4>()
                .0
                .iter()
                .take(samplecount)
                .map(|c| f32::from_bits(order.u32(c)))
                .collect(),
        ),
        Encoding::FLOAT64 => Samples::Float64(
            input
                .as_chunks::<8>()
                .0
                .iter()
                .take(samplecount)
                .map(|c| f64::from_bits(order.u64(c)))
                .collect(),
        ),
        Encoding::STEIM1 => Samples::Int32(steim1(input, samplecount, order, sid)?),
        Encoding::STEIM2 => Samples::Int32(steim2(input, samplecount, order, sid)?),
        Encoding::GEOSCOPE24 | Encoding::GEOSCOPE163 | Encoding::GEOSCOPE164 => {
            Samples::Float32(geoscope(input, samplecount, encoding, order))
        }
        Encoding::CDSN => Samples::Int32(cdsn(input, samplecount, order)),
        Encoding::SRO => Samples::Int32(sro(input, samplecount, order, sid)?),
        Encoding::DWWSSN => Samples::Int32(
            input
                .as_chunks::<2>()
                .0
                .iter()
                .take(samplecount)
                .map(|c| {
                    let mut s = order.u16(c) as i32;
                    if s > MAX16 {
                        s -= 2 * (MAX16 + 1);
                    }
                    s
                })
                .collect(),
        ),
        _ => unreachable!("sample_type() is Some only for handled encodings"),
    };

    if samples.len() != samplecount {
        return Err(Error::InvalidRecord(format!(
            "{sid}: only decoded {} samples of {samplecount} expected",
            samples.len()
        )));
    }
    Ok(samples)
}

/// Decode Steim-1 frames; returns however many samples could be decoded.
fn steim1(input: &[u8], samplecount: usize, order: ByteOrder, sid: &str) -> Result<Vec<i32>> {
    let maxframes = input.len() / 64;
    if maxframes == 0 || samplecount == 0 {
        return Ok(Vec::new());
    }
    let mut out = vec![0i32; samplecount];
    let mut outputidx = 0usize;
    let mut xn = 0i32;
    let mut skipfirst = true;
    let mut diff: Vec<i32> = Vec::with_capacity(60);

    for frameidx in 0..maxframes {
        if outputidx >= samplecount {
            break;
        }
        let frame = &input[frameidx * 64..frameidx * 64 + 64];
        diff.clear();
        let startnibble;
        if frameidx == 0 {
            out[0] = order.u32(&frame[4..8]) as i32;
            outputidx += 1;
            xn = order.u32(&frame[8..12]) as i32;
            startnibble = 3;
        } else {
            startnibble = 1;
        }
        let nibbles = order.u32(&frame[0..4]);

        for widx in startnibble..16 {
            let nibble = (nibbles >> (30 - 2 * widx)) & 3;
            let w = &frame[widx * 4..widx * 4 + 4];
            match nibble {
                0 => {}
                1 => diff.extend(w.iter().map(|&b| b as i8 as i32)),
                2 => {
                    diff.push(order.u16(&w[0..2]) as i16 as i32);
                    diff.push(order.u16(&w[2..4]) as i16 as i32);
                }
                _ => diff.push(order.u32(w) as i32),
            }
        }

        let mut idx = 0;
        if skipfirst && !diff.is_empty() {
            idx = 1;
            skipfirst = false;
        }
        while idx < diff.len() && outputidx < samplecount {
            out[outputidx] = out[outputidx - 1].wrapping_add(diff[idx]);
            idx += 1;
            outputidx += 1;
        }
    }

    if outputidx == samplecount && out[outputidx - 1] != xn {
        log::warn!(
            "{sid}: Data integrity check for Steim1 failed, Last sample={}, Xn={xn}",
            out[outputidx - 1]
        );
    }
    out.truncate(outputidx);
    Ok(out)
}

/// Decode Steim-2 frames; returns however many samples could be decoded.
fn steim2(input: &[u8], samplecount: usize, order: ByteOrder, sid: &str) -> Result<Vec<i32>> {
    let maxframes = input.len() / 64;
    if maxframes == 0 || samplecount == 0 {
        return Ok(Vec::new());
    }
    let mut out = vec![0i32; samplecount];
    let mut outputidx = 0usize;
    let mut xn = 0i32;
    let mut skipfirst = 1usize;
    let mut diff: Vec<i32> = Vec::with_capacity(105);

    for frameidx in 0..maxframes {
        if outputidx >= samplecount {
            break;
        }
        let frame = &input[frameidx * 64..frameidx * 64 + 64];
        diff.clear();
        let startnibble;
        if frameidx == 0 {
            out[0] = order.u32(&frame[4..8]) as i32;
            outputidx += 1;
            xn = order.u32(&frame[8..12]) as i32;
            startnibble = 3;
        } else {
            startnibble = 1;
        }
        let nibbles = order.u32(&frame[0..4]);

        for widx in startnibble..16 {
            // Words beyond the differences needed for the sample count are ignored
            if outputidx >= samplecount || diff.len() >= samplecount - outputidx + skipfirst {
                break;
            }
            let nibble = (nibbles >> (30 - 2 * widx)) & 3;
            let w = &frame[widx * 4..widx * 4 + 4];
            match nibble {
                0 => {}
                1 => diff.extend(w.iter().map(|&b| b as i8 as i32)),
                2 | 3 => {
                    let word = order.u32(w);
                    let dnib = word >> 30;
                    if nibble == 2 {
                        match dnib {
                            0 => {
                                return Err(Error::Steim(format!(
                                    "{sid}: Impossible Steim2 dnib=00 for nibble=10"
                                )));
                            }
                            1 => diff.push(sext(word & 0x3FFF_FFFF, 30)),
                            2 => {
                                for i in 0..2 {
                                    diff.push(sext((word >> (15 - i * 15)) & 0x7FFF, 15));
                                }
                            }
                            _ => {
                                for i in 0..3 {
                                    diff.push(sext((word >> (20 - i * 10)) & 0x3FF, 10));
                                }
                            }
                        }
                    } else {
                        match dnib {
                            0 => {
                                for i in 0..5 {
                                    diff.push(sext((word >> (24 - i * 6)) & 0x3F, 6));
                                }
                            }
                            1 => {
                                for i in 0..6 {
                                    diff.push(sext((word >> (25 - i * 5)) & 0x1F, 5));
                                }
                            }
                            2 => {
                                for i in 0..7 {
                                    diff.push(sext((word >> (24 - i * 4)) & 0xF, 4));
                                }
                            }
                            _ => {
                                return Err(Error::Steim(format!(
                                    "{sid}: Impossible Steim2 dnib=11 for nibble=11"
                                )));
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }

        let mut idx = 0;
        if skipfirst == 1 && !diff.is_empty() {
            idx = 1;
            skipfirst = 0;
        }
        while idx < diff.len() && outputidx < samplecount {
            out[outputidx] = out[outputidx - 1].wrapping_add(diff[idx]);
            idx += 1;
            outputidx += 1;
        }
    }

    if outputidx == samplecount && out[outputidx - 1] != xn {
        log::warn!(
            "{sid}: Data integrity check for Steim2 failed, Last sample={}, Xn={xn}",
            out[outputidx - 1]
        );
    }
    out.truncate(outputidx);
    Ok(out)
}

fn geoscope(input: &[u8], samplecount: usize, encoding: Encoding, order: ByteOrder) -> Vec<f32> {
    let mut out = Vec::with_capacity(samplecount);
    match encoding {
        Encoding::GEOSCOPE24 => {
            for c in input.as_chunks::<3>().0.iter().take(samplecount) {
                let mut mantissa = match order {
                    ByteOrder::Big => (c[0] as i32) << 16 | (c[1] as i32) << 8 | c[2] as i32,
                    ByteOrder::Little => (c[2] as i32) << 16 | (c[1] as i32) << 8 | c[0] as i32,
                };
                if mantissa > MAX24 {
                    mantissa -= 2 * (MAX24 + 1);
                }
                out.push(mantissa as f64 as f32);
            }
        }
        _ => {
            for c in input.as_chunks::<2>().0.iter().take(samplecount) {
                let s = order.u16(c) as i32;
                let mantissa = s & 0x0FFF;
                let gainrange = if encoding == Encoding::GEOSCOPE163 {
                    (s & 0x7000) >> 12
                } else {
                    (s & 0xF000) >> 12
                };
                let exp2val = 1u64 << gainrange;
                out.push(((mantissa - 2048) as f64 / exp2val as f64) as f32);
            }
        }
    }
    out
}

fn cdsn(input: &[u8], samplecount: usize, order: ByteOrder) -> Vec<i32> {
    input
        .as_chunks::<2>()
        .0
        .iter()
        .take(samplecount)
        .map(|c| {
            let s = order.u16(c) as i32;
            let mantissa = (s & 0x3FFF) - MAX14;
            let mult = match (s & 0xC000) >> 14 {
                0 => 0,
                1 => 2,
                2 => 4,
                _ => 7,
            };
            ((mantissa as u32) << mult) as i32
        })
        .collect()
}

fn sro(input: &[u8], samplecount: usize, order: ByteOrder, sid: &str) -> Result<Vec<i32>> {
    let mut out = Vec::with_capacity(samplecount);
    for c in input.as_chunks::<2>().0.iter().take(samplecount) {
        let s = order.u16(c) as i32;
        let mut mantissa = s & 0x0FFF;
        let gainrange = (s & 0xF000) >> 12;
        if mantissa > MAX12 {
            mantissa -= 2 * (MAX12 + 1);
        }
        let exponent = -gainrange + 10;
        if !(0..=10).contains(&exponent) {
            return Err(Error::InvalidRecord(format!(
                "{sid}: SRO gain ranging exponent out of range: {exponent}"
            )));
        }
        out.push((mantissa as i64 * (1i64 << exponent)) as i32);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a 2-frame Steim buffer: X0=100, Xn=104, differences 7,1,1,1,1 as 8-bit words,
    /// in frame 0 when `firstframe`, otherwise in frame 1. A nonzero `trailnibble` places
    /// `trailword` with that nibble in the word after the differences. Returned big endian.
    fn build_steim_frames(firstframe: bool, trailnibble: u32, trailword: u32) -> Vec<u8> {
        let mut frames = [0u32; 32];
        let (base, dstart) = if firstframe { (0usize, 3usize) } else { (16, 1) };
        frames[base + dstart] = 0x07010101;
        frames[base + dstart + 1] = 0x01000000;
        frames[base] |= (0x1 << (30 - 2 * dstart)) | (0x1 << (30 - 2 * (dstart + 1)));
        if trailnibble != 0 {
            frames[base + dstart + 2] = trailword;
            frames[base] |= trailnibble << (30 - 2 * (dstart + 2));
        }
        frames[1] = 100;
        frames[2] = 104;
        frames.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    #[test]
    fn steim_first_difference() {
        for enc in [Encoding::STEIM1, Encoding::STEIM2] {
            for firstframe in [true, false] {
                let frames = build_steim_frames(firstframe, 0, 0);
                let out = decode(&frames, enc, 5, ByteOrder::Big, "").unwrap();
                assert_eq!(
                    out,
                    Samples::Int32(vec![100, 101, 102, 103, 104]),
                    "{enc:?} first={firstframe}"
                );
            }
        }
    }

    #[test]
    fn steim2_trailing_undefined() {
        // Undefined codes: nibble 10 with dnib 00, nibble 11 with dnib 11
        for (nibble, word) in [(2u32, 0x0000_0000u32), (3, 0xC000_0000)] {
            let frames = build_steim_frames(true, nibble, word);
            // An undefined code after the last needed difference is ignored
            let out = decode(&frames, Encoding::STEIM2, 5, ByteOrder::Big, "").unwrap();
            assert_eq!(out, Samples::Int32(vec![100, 101, 102, 103, 104]));
            // Reaching the undefined word is an error
            assert!(decode(&frames, Encoding::STEIM2, 9, ByteOrder::Big, "").is_err());
        }
    }
}

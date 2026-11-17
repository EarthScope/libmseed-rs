//! WebAssembly bindings for [`libmseed`], the pure-Rust miniSEED library.
//!
//! Build with `wasm-pack build libmseed-wasm --target web` (or `bundler` / `nodejs`).
//!
//! ```js
//! import init, { parseRecords, TraceList, packInt32 } from "libmseed";
//! await init();
//! const records = parseRecords(new Uint8Array(buffer), true);
//! for (const r of records) console.log(r.sid, r.startTime, r.samplesInt32());
//! ```

use libmseed::time::{Subseconds, TimeFormat};
use libmseed::tracelist::{TraceList as CoreTraceList, TraceListing};
use libmseed::{
    Encoding, FormatVersion, NsTime, PackOptions, ParseFlags, ReadOptions, Record, SampleType,
    Samples,
};
use wasm_bindgen::prelude::*;

fn err(e: libmseed::Error) -> JsError {
    JsError::new(&e.to_string())
}

fn iso(t: NsTime) -> String {
    t.format(TimeFormat::IsoMonthDayZ, Subseconds::NanoMicroNone)
}

/// Version of the libmseed C library this is ported from.
#[wasm_bindgen]
pub fn version() -> String {
    libmseed::LIBMSEED_VERSION.to_string()
}

/// Convert a time string to nanoseconds since the Unix epoch.
#[wasm_bindgen(js_name = timeToNs)]
pub fn time_to_ns(timestr: &str) -> Result<i64, JsError> {
    NsTime::parse(timestr).map(NsTime::as_nanos).map_err(err)
}

/// Convert nanoseconds since the Unix epoch to an ISO 8601 time string.
#[wasm_bindgen(js_name = nsToTime)]
pub fn ns_to_time(ns: i64) -> String {
    iso(NsTime::from_nanos(ns))
}

/// A miniSEED record.
#[wasm_bindgen]
pub struct MseedRecord {
    inner: Record,
}

#[wasm_bindgen]
impl MseedRecord {
    /// Source identifier.
    #[wasm_bindgen(getter)]
    pub fn sid(&self) -> String {
        self.inner.sid.clone()
    }
    /// Format major version, 2 or 3.
    #[wasm_bindgen(getter, js_name = formatVersion)]
    pub fn format_version(&self) -> u8 {
        self.inner.format_version.map_or(0, libmseed::FormatVersion::major)
    }
    /// Publication version.
    #[wasm_bindgen(getter, js_name = pubVersion)]
    pub fn pub_version(&self) -> u8 {
        self.inner.pub_version
    }
    /// Start time as an ISO 8601 string.
    #[wasm_bindgen(getter, js_name = startTime)]
    pub fn start_time(&self) -> String {
        iso(self.inner.start_time)
    }
    /// Start time in nanoseconds since the Unix epoch.
    #[wasm_bindgen(getter, js_name = startTimeNs)]
    pub fn start_time_ns(&self) -> i64 {
        self.inner.start_time.as_nanos()
    }
    /// End time (last sample) as an ISO 8601 string.
    #[wasm_bindgen(getter, js_name = endTime)]
    pub fn end_time(&self) -> Result<String, JsError> {
        self.inner.end_time().map(iso).map_err(err)
    }
    /// Nominal sample rate in Hz, or period in seconds when negative.
    #[wasm_bindgen(getter, js_name = sampleRate)]
    pub fn sample_rate(&self) -> f64 {
        self.inner.sample_rate
    }
    /// Number of samples in the record.
    #[wasm_bindgen(getter, js_name = sampleCount)]
    pub fn sample_count(&self) -> f64 {
        self.inner.sample_count as f64
    }
    /// Data encoding value, -1 if unknown.
    #[wasm_bindgen(getter)]
    pub fn encoding(&self) -> i32 {
        self.inner.encoding.map_or(-1, |e| e.0 as i32)
    }
    /// Record-level flag bits.
    #[wasm_bindgen(getter)]
    pub fn flags(&self) -> u8 {
        self.inner.flags.bits()
    }
    /// CRC of the record (miniSEED 3).
    #[wasm_bindgen(getter)]
    pub fn crc(&self) -> u32 {
        self.inner.crc
    }
    /// Record length in bytes.
    #[wasm_bindgen(getter)]
    pub fn reclen(&self) -> usize {
        self.inner.reclen.unwrap_or(0)
    }
    /// Extra headers as a JSON string, empty if none.
    #[wasm_bindgen(getter, js_name = extraHeaders)]
    pub fn extra_headers(&self) -> String {
        self.inner.extra.clone()
    }
    /// Sample type: `"t"`, `"i"`, `"f"`, `"d"`, or empty if samples are not unpacked.
    #[wasm_bindgen(getter, js_name = sampleType)]
    pub fn sample_type(&self) -> String {
        self.inner.sample_type().map(|t| t.code().to_string()).unwrap_or_default()
    }
    /// Unpacked 32-bit integer samples, or `undefined`.
    #[wasm_bindgen(js_name = samplesInt32)]
    pub fn samples_int32(&self) -> Option<Vec<i32>> {
        self.inner.samples.as_ref()?.as_i32().map(<[i32]>::to_vec)
    }
    /// Unpacked 32-bit float samples, or `undefined`.
    #[wasm_bindgen(js_name = samplesFloat32)]
    pub fn samples_float32(&self) -> Option<Vec<f32>> {
        self.inner.samples.as_ref()?.as_f32().map(<[f32]>::to_vec)
    }
    /// Unpacked 64-bit float samples, or `undefined`.
    #[wasm_bindgen(js_name = samplesFloat64)]
    pub fn samples_float64(&self) -> Option<Vec<f64>> {
        self.inner.samples.as_ref()?.as_f64().map(<[f64]>::to_vec)
    }
    /// Unpacked text samples as a string, or `undefined`.
    #[wasm_bindgen(js_name = text)]
    pub fn text(&self) -> Option<String> {
        self.inner.samples.as_ref()?.as_text().map(|t| String::from_utf8_lossy(t).into_owned())
    }
    /// A one line summary.
    pub fn summary(&self) -> String {
        self.inner.to_string()
    }
}

/// Parse all miniSEED records (version 2 and 3) in a buffer, optionally decoding samples.
///
/// A truncated final record is ignored.
#[wasm_bindgen(js_name = parseRecords)]
pub fn parse_records(data: &[u8], unpack: bool) -> Result<Vec<MseedRecord>, JsError> {
    let flags = if unpack { ParseFlags::UNPACK_DATA } else { ParseFlags::empty() };
    let mut out = Vec::new();
    for rec in libmseed::MsReader::new(data, flags) {
        out.push(MseedRecord { inner: rec.map_err(err)? });
    }
    Ok(out)
}

/// A continuous segment of a trace.
#[wasm_bindgen]
pub struct MseedSegment {
    start_time: NsTime,
    end_time: NsTime,
    sample_rate: f64,
    sample_count: u64,
    samples: Option<Samples>,
}

#[wasm_bindgen]
impl MseedSegment {
    /// Start time as an ISO 8601 string.
    #[wasm_bindgen(getter, js_name = startTime)]
    pub fn start_time(&self) -> String {
        iso(self.start_time)
    }
    /// End time (last sample) as an ISO 8601 string.
    #[wasm_bindgen(getter, js_name = endTime)]
    pub fn end_time(&self) -> String {
        iso(self.end_time)
    }
    /// Sample rate in Hz.
    #[wasm_bindgen(getter, js_name = sampleRate)]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
    /// Number of samples covered.
    #[wasm_bindgen(getter, js_name = sampleCount)]
    pub fn sample_count(&self) -> f64 {
        self.sample_count as f64
    }
    /// Sample type: `"t"`, `"i"`, `"f"`, `"d"`, or empty if no samples are held.
    #[wasm_bindgen(getter, js_name = sampleType)]
    pub fn sample_type(&self) -> String {
        self.samples.as_ref().map(|s| s.sample_type().code().to_string()).unwrap_or_default()
    }
    /// 32-bit integer samples, or `undefined`.
    #[wasm_bindgen(js_name = samplesInt32)]
    pub fn samples_int32(&self) -> Option<Vec<i32>> {
        self.samples.as_ref()?.as_i32().map(<[i32]>::to_vec)
    }
    /// 32-bit float samples, or `undefined`.
    #[wasm_bindgen(js_name = samplesFloat32)]
    pub fn samples_float32(&self) -> Option<Vec<f32>> {
        self.samples.as_ref()?.as_f32().map(<[f32]>::to_vec)
    }
    /// 64-bit float samples, or `undefined`.
    #[wasm_bindgen(js_name = samplesFloat64)]
    pub fn samples_float64(&self) -> Option<Vec<f64>> {
        self.samples.as_ref()?.as_f64().map(<[f64]>::to_vec)
    }
}

/// A list of traces built from miniSEED records.
#[wasm_bindgen]
pub struct TraceList {
    inner: CoreTraceList,
}

impl Default for TraceList {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl TraceList {
    /// An empty trace list.
    #[wasm_bindgen(constructor)]
    pub fn new() -> TraceList {
        TraceList { inner: CoreTraceList::new() }
    }

    /// Add all records in a buffer; returns the number of records added.
    #[wasm_bindgen(js_name = addBuffer)]
    pub fn add_buffer(&mut self, data: &[u8], unpack: bool) -> Result<usize, JsError> {
        let flags = if unpack { ParseFlags::UNPACK_DATA } else { ParseFlags::empty() };
        self.inner.read_buffer(data, &ReadOptions::new(flags)).map_err(err)
    }

    /// Number of source identifiers.
    #[wasm_bindgen(getter, js_name = numIds)]
    pub fn num_ids(&self) -> usize {
        self.inner.num_ids()
    }

    /// Source identifiers, in order.
    pub fn sids(&self) -> Vec<String> {
        self.inner.ids().iter().map(|i| i.sid.clone()).collect()
    }

    /// Number of segments of the identifier at `index`.
    #[wasm_bindgen(js_name = numSegments)]
    pub fn num_segments(&self, index: usize) -> usize {
        self.inner.ids().get(index).map_or(0, |i| i.num_segments())
    }

    /// A copy of a segment of the identifier at `id`, or `undefined`.
    pub fn segment(&self, id: usize, segment: usize) -> Option<MseedSegment> {
        let s = self.inner.ids().get(id)?.segments.get(segment)?;
        Some(MseedSegment {
            start_time: s.start_time,
            end_time: s.end_time,
            sample_rate: s.sample_rate,
            sample_count: s.sample_count,
            samples: s.samples.clone(),
        })
    }

    /// A listing of the trace list.
    pub fn summary(&self) -> String {
        self.inner.format_trace_list(&TraceListing {
            details: true,
            gaps: true,
            ..Default::default()
        })
    }
}

fn pack_samples(
    sid: &str,
    start_time: &str,
    sample_rate: f64,
    samples: Samples,
    encoding: i32,
    reclen: u32,
    version2: bool,
) -> Result<Vec<u8>, JsError> {
    let mut rec = Record::new(sid, NsTime::parse(start_time).map_err(err)?);
    rec.sample_rate = sample_rate;
    rec.pub_version = 1;
    rec.reclen = (reclen > 0).then_some(reclen as usize);
    rec.encoding = (encoding >= 0).then_some(Encoding(encoding as u8));
    rec.sample_count = samples.len() as u64;
    if matches!(samples.sample_type(), SampleType::Float32 | SampleType::Float64 | SampleType::Text)
    {
        rec.encoding = None;
    }
    rec.samples = Some(samples);
    let options = PackOptions {
        format: Some(if version2 { FormatVersion::V2 } else { FormatVersion::V3 }),
        flush: true,
        ..Default::default()
    };
    let mut out = Vec::new();
    rec.pack_with(&options, |r| {
        out.extend_from_slice(r);
        Ok(())
    })
    .map_err(err)?;
    Ok(out)
}

/// Pack 32-bit integer samples into miniSEED records.
///
/// `encoding` is a miniSEED encoding value (e.g. 11 for Steim-2), or -1 for the default;
/// `reclen` a maximum record length in bytes, or 0 for the default; `version2` selects
/// miniSEED 2 instead of 3.
#[wasm_bindgen(js_name = packInt32)]
pub fn pack_int32(
    sid: &str,
    start_time: &str,
    sample_rate: f64,
    samples: &[i32],
    encoding: i32,
    reclen: u32,
    version2: bool,
) -> Result<Vec<u8>, JsError> {
    pack_samples(
        sid,
        start_time,
        sample_rate,
        Samples::Int32(samples.to_vec()),
        encoding,
        reclen,
        version2,
    )
}

/// Pack 32-bit float samples into miniSEED records.
#[wasm_bindgen(js_name = packFloat32)]
pub fn pack_float32(
    sid: &str,
    start_time: &str,
    sample_rate: f64,
    samples: &[f32],
    reclen: u32,
    version2: bool,
) -> Result<Vec<u8>, JsError> {
    pack_samples(
        sid,
        start_time,
        sample_rate,
        Samples::Float32(samples.to_vec()),
        -1,
        reclen,
        version2,
    )
}

/// Pack 64-bit float samples into miniSEED records.
#[wasm_bindgen(js_name = packFloat64)]
pub fn pack_float64(
    sid: &str,
    start_time: &str,
    sample_rate: f64,
    samples: &[f64],
    reclen: u32,
    version2: bool,
) -> Result<Vec<u8>, JsError> {
    pack_samples(
        sid,
        start_time,
        sample_rate,
        Samples::Float64(samples.to_vec()),
        -1,
        reclen,
        version2,
    )
}

/// Pack text into miniSEED records.
#[wasm_bindgen(js_name = packText)]
pub fn pack_text(
    sid: &str,
    start_time: &str,
    text: &str,
    reclen: u32,
    version2: bool,
) -> Result<Vec<u8>, JsError> {
    pack_samples(
        sid,
        start_time,
        0.0,
        Samples::Text(text.as_bytes().to_vec()),
        -1,
        reclen,
        version2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Native tests of the non-JS logic; JsError cannot be constructed off-wasm so only
    // successful paths are exercised here (wasm tests cover errors).
    #[test]
    fn pack_parse_roundtrip() {
        let samples: Vec<i32> = (0..500).collect();
        let bytes = pack_samples(
            "FDSN:XX_TEST__B_H_Z",
            "2024-01-01T00:00:00Z",
            40.0,
            Samples::Int32(samples.clone()),
            3, // 32-bit integers, so that multiple records are needed
            512,
            false,
        )
        .ok()
        .unwrap();
        let recs = parse_records(&bytes, true).ok().unwrap();
        assert!(recs.len() > 1);
        let mut got = Vec::new();
        for r in &recs {
            got.extend(r.samples_int32().unwrap());
        }
        assert_eq!(got, samples);
        assert_eq!(recs[0].sid(), "FDSN:XX_TEST__B_H_Z");

        let mut tl = TraceList::new();
        assert_eq!(tl.add_buffer(&bytes, true).ok().unwrap(), recs.len());
        assert_eq!(tl.num_ids(), 1);
        assert_eq!(tl.segment(0, 0).unwrap().samples_int32().unwrap(), samples);
    }
}

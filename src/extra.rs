//! Extra headers: JSON stored in miniSEED 3 records.
//!
//! Extra headers are a single JSON object. Values are addressed with
//! [JSON Pointers](https://www.rfc-editor.org/rfc/rfc6901), e.g. `/FDSN/Time/Quality`.
//!
//! [`ExtraHeaders`] is the parsed form. Parse once with [`ExtraHeaders::from_json`],
//! get and set many values, and serialize with [`ExtraHeaders::to_json`]. Convenience wrappers on [`crate::Record`]
//! do this for a single operation.
//!
//! Real numbers set through [`ExtraHeaders::set_f64`] and the typed helpers are written
//! with single-precision shortest representation when they fit in an `f32`, matching
//! libmseed output.

use crate::error::{Error, Result};
use crate::time::{NsTime, Subseconds, TimeFormat};
use serde_json::{Map, Number, Value};

/// Maximum serialized size of extra headers in bytes (a 16-bit length field).
pub const MAX_EXTRA_LENGTH: usize = u16::MAX as usize;

/// Type of a value found in extra headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueType {
    /// Non-negative integer (`'u'`).
    Unsigned,
    /// Negative integer (`'i'`).
    Integer,
    /// Real number (`'n'`).
    Number,
    /// String (`'s'`).
    String,
    /// Boolean (`'b'`).
    Boolean,
    /// Object (`'o'`).
    Object,
    /// Array (`'a'`).
    Array,
}

impl ValueType {
    /// The libmseed type character: `u`, `i`, `n`, `s`, `b`, `o` or `a`.
    #[must_use]
    pub fn code(self) -> char {
        match self {
            ValueType::Unsigned => 'u',
            ValueType::Integer => 'i',
            ValueType::Number => 'n',
            ValueType::String => 's',
            ValueType::Boolean => 'b',
            ValueType::Object => 'o',
            ValueType::Array => 'a',
        }
    }
}

fn classify(v: &Value) -> Option<ValueType> {
    match v {
        Value::Number(n) if n.is_u64() => Some(ValueType::Unsigned),
        Value::Number(n) if n.is_i64() => Some(ValueType::Integer),
        Value::Number(_) => Some(ValueType::Number),
        Value::String(_) => Some(ValueType::String),
        Value::Bool(_) => Some(ValueType::Boolean),
        Value::Object(_) => Some(ValueType::Object),
        Value::Array(_) => Some(ValueType::Array),
        Value::Null => None,
    }
}

/// Make a JSON real, written as single precision when it fits in an `f32`.
fn real_value(v: f64) -> Result<Value> {
    if !v.is_finite() {
        return Err(Error::ExtraHeaders(format!(
            "Cannot store non-finite number {v} in extra headers"
        )));
    }
    let v = if v.abs() <= f32::MAX as f64 {
        // Shortest decimal that round-trips as f32, then as f64 for serialization
        format!("{}", v as f32).parse::<f64>().unwrap_or(v)
    } else {
        v
    };
    Ok(Value::Number(Number::from_f64(v).expect("finite")))
}

fn check_pointer(ptr: &str) -> Result<()> {
    if !ptr.is_empty() && !ptr.starts_with('/') {
        return Err(Error::ExtraHeaders(format!("Unsupported JSON Pointer notation: {ptr}")));
    }
    Ok(())
}

fn unescape(tok: &str) -> String {
    tok.replace("~1", "/").replace("~0", "~")
}

/// Parsed extra headers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExtraHeaders {
    /// Root value; `Null` when there are no headers.
    root: Value,
}

impl ExtraHeaders {
    /// Empty headers.
    #[must_use]
    pub fn new() -> Self {
        ExtraHeaders { root: Value::Null }
    }

    /// Parse a JSON string; an empty string yields empty headers.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.is_empty() {
            return Ok(Self::new());
        }
        let root: Value = serde_json::from_str(json)
            .map_err(|e| Error::ExtraHeaders(format!("Cannot parse extra header JSON: {e}")))?;
        Ok(ExtraHeaders { root })
    }

    /// True if there are no headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.root.is_null()
    }

    /// The underlying JSON value.
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.root
    }

    /// Serialize to compact JSON. Empty headers give an empty string.
    ///
    /// Fails if the result exceeds [`MAX_EXTRA_LENGTH`] bytes.
    pub fn to_json(&self) -> Result<String> {
        if self.root.is_null() {
            return Ok(String::new());
        }
        let s = serde_json::to_string(&self.root)
            .map_err(|e| Error::ExtraHeaders(format!("Cannot write extra header JSON: {e}")))?;
        if s.len() > MAX_EXTRA_LENGTH {
            return Err(Error::ExtraHeaders(format!(
                "New serialization size exceeds limit of {MAX_EXTRA_LENGTH} bytes: {}",
                s.len()
            )));
        }
        Ok(s)
    }

    /// Value at a JSON Pointer; `Ok(None)` if not found, `Err` for an invalid pointer.
    pub fn get(&self, ptr: &str) -> Result<Option<&Value>> {
        check_pointer(ptr)?;
        if self.root.is_null() {
            return Ok(None);
        }
        Ok(self.root.pointer(ptr).filter(|v| !v.is_null()))
    }

    /// Type of the value at a JSON Pointer.
    pub fn value_type(&self, ptr: &str) -> Result<Option<ValueType>> {
        Ok(self.get(ptr)?.and_then(classify))
    }

    /// True if a value exists at the pointer.
    #[must_use]
    pub fn exists(&self, ptr: &str) -> bool {
        matches!(self.get(ptr), Ok(Some(_)))
    }

    fn typed<'a, T>(
        &'a self,
        ptr: &str,
        want: &str,
        f: impl FnOnce(&'a Value) -> Option<T>,
    ) -> Result<Option<T>> {
        match self.get(ptr)? {
            None => Ok(None),
            Some(v) => f(v)
                .map(Some)
                .ok_or_else(|| Error::ExtraHeaders(format!("Value at {ptr} is not {want}"))),
        }
    }

    /// Unsigned integer value.
    pub fn get_u64(&self, ptr: &str) -> Result<Option<u64>> {
        self.typed(ptr, "an unsigned integer", |v| v.as_u64())
    }
    /// Integer value.
    pub fn get_i64(&self, ptr: &str) -> Result<Option<i64>> {
        self.typed(ptr, "an integer", |v| v.as_i64())
    }
    /// Number value, any JSON number.
    pub fn get_f64(&self, ptr: &str) -> Result<Option<f64>> {
        self.typed(ptr, "a number", |v| v.as_f64())
    }
    /// String value.
    pub fn get_str(&self, ptr: &str) -> Result<Option<&str>> {
        self.typed(ptr, "a string", |v| v.as_str())
    }
    /// Boolean value.
    pub fn get_bool(&self, ptr: &str) -> Result<Option<bool>> {
        self.typed(ptr, "a boolean", |v| v.as_bool())
    }

    /// Set or replace the value at a JSON Pointer, creating parent objects as needed.
    pub fn set_value(&mut self, ptr: &str, value: Value) -> Result<()> {
        check_pointer(ptr)?;
        if set_at(&mut self.root, ptr, value) {
            Ok(())
        } else {
            Err(Error::ExtraHeaders(format!("Cannot set extra header value at {ptr}")))
        }
    }

    /// Set an unsigned integer.
    pub fn set_u64(&mut self, ptr: &str, v: u64) -> Result<()> {
        self.set_value(ptr, Value::from(v))
    }
    /// Set an integer.
    pub fn set_i64(&mut self, ptr: &str, v: i64) -> Result<()> {
        self.set_value(ptr, Value::from(v))
    }
    /// Set a number, see module docs on precision.
    pub fn set_f64(&mut self, ptr: &str, v: f64) -> Result<()> {
        let val = real_value(v)?;
        self.set_value(ptr, val)
    }
    /// Set a string.
    pub fn set_str(&mut self, ptr: &str, v: &str) -> Result<()> {
        self.set_value(ptr, Value::String(v.to_string()))
    }
    /// Set a boolean.
    pub fn set_bool(&mut self, ptr: &str, v: bool) -> Result<()> {
        self.set_value(ptr, Value::Bool(v))
    }

    /// Append a value to the array at `ptr`, creating the array if needed.
    pub fn append_value(&mut self, ptr: &str, value: Value) -> Result<()> {
        check_pointer(ptr)?;
        if self.root.is_null() || self.root.pointer(ptr).is_none() {
            self.set_value(ptr, Value::Array(Vec::new()))?;
        }
        match self.root.pointer_mut(ptr) {
            Some(Value::Array(a)) => {
                a.push(value);
                Ok(())
            }
            _ => Err(Error::ExtraHeaders(format!("Value at {ptr} is not an array"))),
        }
    }

    /// Apply a JSON Merge Patch (RFC 7386) at `ptr`.
    ///
    /// The target must exist unless `ptr` is the root (`""`), which is the recommended use.
    pub fn merge_patch(&mut self, ptr: &str, patch_json: &str) -> Result<()> {
        check_pointer(ptr)?;
        let patch: Value = serde_json::from_str(patch_json)
            .map_err(|e| Error::ExtraHeaders(format!("Cannot parse JSON Merge Patch: {e}")))?;
        if ptr.is_empty() {
            merge_patch(&mut self.root, &patch);
            return Ok(());
        }
        match self.root.pointer_mut(ptr) {
            Some(target) => {
                merge_patch(target, &patch);
                Ok(())
            }
            None => Err(Error::ExtraHeaders(format!("Merge patch target {ptr} does not exist"))),
        }
    }

    /// Add an event detection entry (default pointer `/FDSN/Event/Detection`).
    pub fn add_event_detection(&mut self, ptr: Option<&str>, e: &EventDetection) -> Result<()> {
        let mut o = Map::new();
        put_str(&mut o, "Type", &e.kind);
        put_real(&mut o, "SignalAmplitude", e.signal_amplitude)?;
        put_real(&mut o, "SignalPeriod", e.signal_period)?;
        put_real(&mut o, "BackgroundEstimate", e.background_estimate)?;
        put_str(&mut o, "Wave", &e.wave);
        put_str(&mut o, "Units", &e.units);
        put_time(&mut o, "OnsetTime", e.onset_time);
        if let Some(snr) = e.med_snr {
            o.insert("MEDSNR".into(), Value::Array(snr.iter().map(|&b| Value::from(b)).collect()));
        }
        if let Some(v) = e.med_lookback {
            o.insert("MEDLookback".into(), Value::from(v));
        }
        if let Some(v) = e.med_pick_algorithm {
            o.insert("MEDPickAlgorithm".into(), Value::from(v));
        }
        put_str(&mut o, "Detector", &e.detector);
        self.append_value(ptr.unwrap_or("/FDSN/Event/Detection"), Value::Object(o))
    }

    /// Add a calibration entry (default pointer `/FDSN/Calibration/Sequence`).
    pub fn add_calibration(&mut self, ptr: Option<&str>, c: &Calibration) -> Result<()> {
        let mut o = Map::new();
        put_str(&mut o, "Type", &c.kind);
        put_time(&mut o, "BeginTime", c.begin_time);
        put_time(&mut o, "EndTime", c.end_time);
        if let Some(s) = c.steps {
            o.insert("Steps".into(), Value::from(s));
        }
        if let Some(b) = c.first_pulse_positive {
            o.insert("StepFirstPulsePositive".into(), Value::Bool(b));
        }
        if let Some(b) = c.alternate_sign {
            o.insert("StepAlternateSign".into(), Value::Bool(b));
        }
        put_str(&mut o, "Trigger", &c.trigger);
        if let Some(b) = c.continued {
            o.insert("Continued".into(), Value::Bool(b));
        }
        put_real(&mut o, "Amplitude", c.amplitude)?;
        put_str(&mut o, "InputUnits", &c.input_units);
        put_str(&mut o, "AmplitudeRange", &c.amplitude_range);
        put_real(&mut o, "Duration", c.duration)?;
        put_real(&mut o, "SinePeriod", c.sine_period)?;
        put_real(&mut o, "StepBetween", c.step_between)?;
        put_str(&mut o, "InputChannel", &c.input_channel);
        put_real(&mut o, "ReferenceAmplitude", c.ref_amplitude)?;
        put_str(&mut o, "Coupling", &c.coupling);
        put_str(&mut o, "Rolloff", &c.rolloff);
        put_str(&mut o, "Noise", &c.noise);
        self.append_value(ptr.unwrap_or("/FDSN/Calibration/Sequence"), Value::Object(o))
    }

    /// Add a timing exception entry (default pointer `/FDSN/Time/Exception`).
    pub fn add_timing_exception(&mut self, ptr: Option<&str>, t: &TimingException) -> Result<()> {
        let mut o = Map::new();
        put_time(&mut o, "Time", t.time);
        if let Some(v) = t.vco_correction {
            o.insert("VCOCorrection".into(), real_value(v as f64)?);
        }
        if let Some(v) = t.reception_quality {
            o.insert("ReceptionQuality".into(), Value::from(v));
        }
        if let Some(v) = t.count {
            o.insert("Count".into(), Value::from(v));
        }
        let kind = t.kind.trim_end_matches([' ', '\0']);
        if !kind.is_empty() {
            o.insert("Type".into(), Value::String(kind.to_string()));
        }
        let cs = t.clock_status.trim_end_matches([' ', '\0']);
        if !cs.is_empty() {
            o.insert("ClockStatus".into(), Value::String(cs.to_string()));
        }
        self.append_value(ptr.unwrap_or("/FDSN/Time/Exception"), Value::Object(o))
    }

    /// Add a recenter entry (default pointer `/FDSN/Recenter/Sequence`).
    pub fn add_recenter(&mut self, ptr: Option<&str>, r: &Recenter) -> Result<()> {
        let mut o = Map::new();
        put_str(&mut o, "Type", &r.kind);
        put_time(&mut o, "BeginTime", r.begin_time);
        put_time(&mut o, "EndTime", r.end_time);
        put_str(&mut o, "Trigger", &r.trigger);
        self.append_value(ptr.unwrap_or("/FDSN/Recenter/Sequence"), Value::Object(o))
    }
}

fn put_str(o: &mut Map<String, Value>, key: &str, v: &str) {
    if !v.is_empty() {
        o.insert(key.into(), Value::String(v.to_string()));
    }
}

fn put_real(o: &mut Map<String, Value>, key: &str, v: Option<f64>) -> Result<()> {
    if let Some(v) = v {
        o.insert(key.into(), real_value(v)?);
    }
    Ok(())
}

fn put_time(o: &mut Map<String, Value>, key: &str, t: Option<NsTime>) {
    if let Some(t) = t {
        o.insert(
            key.into(),
            Value::String(t.format(TimeFormat::IsoMonthDayZ, Subseconds::NanoMicroNone)),
        );
    }
}

/// RFC 7386 JSON Merge Patch.
fn merge_patch(target: &mut Value, patch: &Value) {
    if let Value::Object(p) = patch {
        if !target.is_object() {
            *target = Value::Object(Map::new());
        }
        let t = target.as_object_mut().expect("object");
        for (k, v) in p {
            if v.is_null() {
                t.shift_remove(k);
            } else {
                merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    } else {
        *target = patch.clone();
    }
}

/// Set a value at a pointer, creating intermediate objects. Returns false on failure.
fn set_at(root: &mut Value, ptr: &str, new: Value) -> bool {
    if ptr.is_empty() {
        *root = new;
        return true;
    }
    if root.is_null() {
        *root = Value::Object(Map::new());
    }
    let tokens: Vec<String> = ptr[1..].split('/').map(unescape).collect();
    let mut cur = root;
    let mut new = Some(new);
    for (i, tok) in tokens.iter().enumerate() {
        let last = i == tokens.len() - 1;
        match cur {
            Value::Object(m) => {
                if last {
                    m.insert(tok.clone(), new.take().expect("value"));
                    return true;
                }
                cur = m.entry(tok.clone()).or_insert_with(|| Value::Object(Map::new()));
            }
            Value::Array(a) => {
                let idx = if tok == "-" { Some(a.len()) } else { tok.parse::<usize>().ok() };
                let Some(idx) = idx else { return false };
                if last {
                    if idx < a.len() {
                        a[idx] = new.take().expect("value");
                    } else if idx == a.len() {
                        a.push(new.take().expect("value"));
                    } else {
                        return false;
                    }
                    return true;
                }
                match a.get_mut(idx) {
                    Some(v) => cur = v,
                    None => return false,
                }
            }
            _ => return false,
        }
    }
    false
}

/// Event detection parameters. `None` and empty values are omitted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventDetection {
    /// Detector type, e.g. `"MURDOCK"`.
    pub kind: String,
    /// Detector name.
    pub detector: String,
    /// Signal amplitude.
    pub signal_amplitude: Option<f64>,
    /// Signal period.
    pub signal_period: Option<f64>,
    /// Background estimate.
    pub background_estimate: Option<f64>,
    /// Detection wave, e.g. `"DILATATION"`.
    pub wave: String,
    /// Units of amplitude and background estimate, e.g. `"COUNTS"`.
    pub units: String,
    /// Onset time.
    pub onset_time: Option<NsTime>,
    /// Murdock signal to noise ratio.
    pub med_snr: Option<[u8; 6]>,
    /// Murdock lookback value.
    pub med_lookback: Option<i32>,
    /// Murdock pick algorithm.
    pub med_pick_algorithm: Option<i32>,
}

/// Calibration parameters. `None` and empty values are omitted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Calibration {
    /// Calibration type, e.g. `"STEP"`, `"SINE"`, `"PSEUDORANDOM"`.
    pub kind: String,
    /// Begin time.
    pub begin_time: Option<NsTime>,
    /// End time.
    pub end_time: Option<NsTime>,
    /// Number of step calibrations.
    pub steps: Option<i32>,
    /// Step calibration first pulse positive.
    pub first_pulse_positive: Option<bool>,
    /// Step calibration alternating sign.
    pub alternate_sign: Option<bool>,
    /// Trigger, e.g. `"AUTOMATIC"` or `"MANUAL"`.
    pub trigger: String,
    /// Continued from previous record.
    pub continued: Option<bool>,
    /// Amplitude of calibration signal.
    pub amplitude: Option<f64>,
    /// Units of input.
    pub input_units: String,
    /// E.g. `PEAKTOPEAK`, `ZEROTOPEAK`, `RMS`, `RANDOM`.
    pub amplitude_range: String,
    /// Duration in seconds.
    pub duration: Option<f64>,
    /// Period of sine.
    pub sine_period: Option<f64>,
    /// Interval between steps.
    pub step_between: Option<f64>,
    /// Channel of input.
    pub input_channel: String,
    /// Reference amplitude.
    pub ref_amplitude: Option<f64>,
    /// Coupling, e.g. `Resistive`.
    pub coupling: String,
    /// Rolloff of filters.
    pub rolloff: String,
    /// Noise for pseudo-random calibrations, e.g. `White` or `Red`.
    pub noise: String,
}

/// Timing exception parameters. `None` and empty values are omitted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimingException {
    /// Time of exception.
    pub time: Option<NsTime>,
    /// VCO correction, 0 to 100%.
    pub vco_correction: Option<f32>,
    /// Reception quality, 0 to 100%.
    pub reception_quality: Option<i32>,
    /// Exception count.
    pub count: Option<u32>,
    /// E.g. `"MISSING"` or `"UNEXPECTED"`; trailing spaces ignored.
    pub kind: String,
    /// Clock-specific status description; trailing spaces ignored.
    pub clock_status: String,
}

/// Recenter parameters. `None` and empty values are omitted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Recenter {
    /// Recenter type, e.g. `"MASS"` or `"GIMBAL"`.
    pub kind: String,
    /// Begin time.
    pub begin_time: Option<NsTime>,
    /// Estimated end time.
    pub end_time: Option<NsTime>,
    /// Trigger, e.g. `"AUTOMATIC"` or `"MANUAL"`.
    pub trigger: String,
}

/// Format extra headers JSON for display, indented, without the outer braces.
#[must_use]
pub fn format_extra(extra: &str, indent: usize) -> String {
    let mut out = String::new();
    // Drop the outer braces; the text may come from a corrupt record, so the first and
    // last characters need not be ASCII
    let mut chars = extra.chars();
    if chars.next().is_none() || chars.next_back().is_none() {
        return out;
    }
    let mut indent = indent as i64;
    let pad = |n: i64| " ".repeat(n.max(0) as usize);
    out.push_str(&pad(indent));
    let (mut instring, mut escaped) = (false, false);
    for c in chars {
        if c == '"' && !escaped {
            instring = !instring;
        }
        escaped = instring && !escaped && c == '\\';
        if instring {
            out.push(c);
            continue;
        }
        match c {
            ':' => out.push_str(": "),
            ',' => {
                out.push_str(",\n");
                out.push_str(&pad(indent));
            }
            '{' | '[' => {
                indent += 2;
                out.push(c);
                out.push('\n');
                out.push_str(&pad(indent));
            }
            '}' | ']' => {
                indent = (indent - 2).max(0);
                out.push('\n');
                out.push_str(&pad(indent));
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('\n');
    out
}

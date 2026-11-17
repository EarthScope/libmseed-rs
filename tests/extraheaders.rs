//! Port of libmseed test/test-extraheaders.c
#![allow(clippy::approx_constant)] // 3.14159 is the value used by the C test

use libmseed::NsTime;
use libmseed::Record;
use libmseed::extra::{ExtraHeaders, ValueType};
use serde_json::Value;

const TESTHEADERS: &str = r#"{
  "FDSN": {
    "Time": { "Quality": 100, "Correction": 1.234, "LeapSecond": -1 },
    "Event": {
      "Begin": true, "End": true, "InProgress": true,
      "Detection": [
        {
          "Type": "MURDOCK", "SignalAmplitude": 80, "SignalPeriod": 0.4,
          "BackgroundEstimate": 18, "Wave": "DILATATION", "Units": "COUNTS",
          "OnsetTime": "2022-06-05T20:32:39.120000Z",
          "MEDSNR": [ 1, 3, 2, 1, 4, 0 ], "MEDLookback": 2, "MEDPickAlgorithm": 0,
          "Detector": "Z_SPWWSS"
        }
      ]
    }
  }
}"#;

fn record_with_testheaders() -> Record {
    let mut msr = Record::new("", NsTime::UNIX_EPOCH);
    msr.extra = TESTHEADERS.to_string();
    msr
}

#[test]
fn get_set_ptr() {
    let msr = record_with_testheaders();
    let eh = msr.extra_headers().unwrap();

    // Matching
    assert!(eh.exists("/FDSN/Time/Quality"));
    assert!(eh.exists("/FDSN/Event/Detection/0"));
    assert_eq!(eh.get_u64("/FDSN/Time/Quality").unwrap(), Some(100));
    assert_eq!(eh.get_i64("/FDSN/Time/Quality").unwrap(), Some(100));
    assert_eq!(eh.get_f64("/FDSN/Time/Correction").unwrap(), Some(1.234));
    assert_eq!(eh.get_str("/FDSN/Event/Detection/0/Type").unwrap(), Some("MURDOCK"));
    assert_eq!(eh.get_bool("/FDSN/Event/Begin").unwrap(), Some(true));

    // Non matching
    assert!(!eh.exists("/FDSN/Event/Detection/1"));
    assert_eq!(eh.get_i64("/A/Non/Existant/Header").unwrap(), None);
    // Wrong type
    assert!(eh.get_str("/FDSN/Time/Quality").is_err());
    assert!(eh.get_u64("/FDSN/Time/LeapSecond").is_err());

    // Set and get
    let mut eh = eh;
    eh.set_u64("/New/UnsignedInteger", u64::MAX).unwrap();
    assert_eq!(eh.get_u64("/New/UnsignedInteger").unwrap(), Some(u64::MAX));
    eh.set_i64("/New/Integer", -51204).unwrap();
    assert_eq!(eh.get_i64("/New/Integer").unwrap(), Some(-51204));
    eh.set_f64("/New/Number", 3.14159).unwrap();
    assert_eq!(eh.get_f64("/New/Number").unwrap(), Some(3.14159));
    eh.set_str("/New/String", "Value").unwrap();
    assert_eq!(eh.get_str("/New/String").unwrap(), Some("Value"));
    eh.set_bool("/New/Boolean", true).unwrap();
    assert_eq!(eh.get_bool("/New/Boolean").unwrap(), Some(true));

    // Invalid JSON Pointer
    assert!(eh.get("invalid").is_err());
    assert!(eh.set_u64("invalid", 1).is_err());

    // Round trip through a record
    let mut msr = msr;
    msr.set_extra_headers(&eh).unwrap();
    let again = msr.extra_headers().unwrap();
    assert_eq!(again.get_i64("/New/Integer").unwrap(), Some(-51204));
    assert_eq!(again.get_u64("/FDSN/Time/Quality").unwrap(), Some(100));
}

#[test]
fn get_ptr_type() {
    let eh = record_with_testheaders().extra_headers().unwrap();
    let ty = |p: &str| eh.value_type(p).unwrap();

    assert_eq!(ty("/FDSN/Time/Quality"), Some(ValueType::Unsigned));
    assert_eq!(ty("/FDSN/Time/LeapSecond"), Some(ValueType::Integer));
    assert_eq!(ty("/FDSN/Time/Correction"), Some(ValueType::Number));
    assert_eq!(ty("/FDSN/Event/Detection/0/Type"), Some(ValueType::String));
    assert_eq!(ty("/FDSN/Event/Begin"), Some(ValueType::Boolean));
    assert_eq!(ty("/FDSN/Event/Detection"), Some(ValueType::Array));
    assert_eq!(ty("/FDSN/Event"), Some(ValueType::Object));
    assert_eq!(ty(""), Some(ValueType::Object));
    assert_eq!(ty("/FDSN/Non/Existant/Header"), None);
    assert!(eh.value_type("invalid").is_err());
    assert_eq!(ValueType::Unsigned.code(), 'u');
    assert_eq!(ValueType::Array.code(), 'a');
}

#[test]
fn mergepatch() {
    let mut msr = Record::new("", NsTime::UNIX_EPOCH);

    // No existing headers: create with a merge patch at the root pointer
    let patch = r#"{"root":{"string":"value"}}"#;
    msr.merge_extra("", patch).unwrap();
    assert_eq!(msr.extra, patch);

    let patch = r#"{"root":{"string":"Updated value"}}"#;
    msr.merge_extra("", patch).unwrap();
    assert_eq!(msr.extra, patch);

    // Add /root/array with pointer /root
    msr.merge_extra("/root", r#"{"array": [1,2,3]}"#).unwrap();
    assert_eq!(msr.extra, r#"{"root":{"string":"Updated value","array":[1,2,3]}}"#);

    // Remove /root/string and /root/array, add /root/boolean
    msr.merge_extra("", r#"{"root": {"string": null, "array": null, "boolean": true}}"#).unwrap();
    assert_eq!(msr.extra, r#"{"root":{"boolean":true}}"#);

    // No existing target
    assert!(msr.merge_extra("/root/doesnotexist", r#"{"key":"value"}"#).is_err());
    // Invalid pointer
    assert!(msr.merge_extra("invalid", r#"{"root":{"string":"value"}}"#).is_err());
}

#[test]
fn replace() {
    let mut msr = Record::new("", NsTime::UNIX_EPOCH);
    msr.extra = r#"{"root":{"string":"value"}}"#.to_string();

    let newdoc = r#"{"new":{"string":"Updated value"}}"#;
    assert_eq!(msr.replace_extra(newdoc).unwrap(), newdoc.len());
    assert_eq!(msr.extra, newdoc);

    // Uncompact input is serialized compactly
    let uncompact = "{  \"new\":\n  {  \"string\"  :  \n  \"Updated value\"  }  }";
    assert_eq!(msr.replace_extra(uncompact).unwrap(), newdoc.len());
    assert_eq!(msr.extra, newdoc);

    // Remove
    assert_eq!(msr.replace_extra("").unwrap(), 0);
    assert!(msr.extra.is_empty());
}

#[test]
fn internal_values_and_arrays() {
    let mut msr = Record::new("", NsTime::UNIX_EPOCH);

    msr.set_extra("/root/string", Value::from("value")).unwrap();
    assert_eq!(msr.extra, r#"{"root":{"string":"value"}}"#);

    let mut eh = msr.extra_headers().unwrap();
    eh.set_value("/root/real", Value::from(123.456)).unwrap();
    eh.set_value("/root/int", Value::from(-123456)).unwrap();
    eh.set_value("/root/bool", Value::from(false)).unwrap();
    assert_eq!(eh.get_f64("/root/real").unwrap(), Some(123.456));
    assert_eq!(eh.get_i64("/root/int").unwrap(), Some(-123456));
    assert_eq!(eh.get_bool("/root/bool").unwrap(), Some(false));

    // Build an array of values
    for v in [Value::from("value"), Value::from(123.456), Value::from(-123456), Value::from(false)]
    {
        eh.append_value("/root/array", v).unwrap();
    }
    assert_eq!(eh.get_f64("/root/array/1").unwrap(), Some(123.456));
    assert_eq!(eh.value_type("/root/array").unwrap(), Some(ValueType::Array));
    // Appending to something that is not an array fails
    assert!(eh.append_value("/root/string", Value::from(1)).is_err());
}

#[test]
fn typed_entries_and_precision() {
    use libmseed::extra::{Calibration, EventDetection, Recenter, TimingException};

    let mut eh = ExtraHeaders::new();
    eh.add_event_detection(
        None,
        &EventDetection {
            kind: "MURDOCK".into(),
            detector: "Z_SPWWSS".into(),
            signal_amplitude: Some(80.0),
            signal_period: Some(0.4000000059604645), // an f32 value
            wave: "DILATATION".into(),
            units: "COUNTS".into(),
            onset_time: Some(NsTime::parse("2022-06-05T20:32:39.12Z").unwrap()),
            med_snr: Some([1, 3, 2, 1, 4, 0]),
            med_lookback: Some(2),
            med_pick_algorithm: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    eh.add_calibration(
        None,
        &Calibration {
            kind: "STEP".into(),
            steps: Some(2),
            first_pulse_positive: Some(true),
            duration: Some(5.0),
            ..Default::default()
        },
    )
    .unwrap();
    eh.add_timing_exception(
        None,
        &TimingException { count: Some(3), kind: "MISSING  ".into(), ..Default::default() },
    )
    .unwrap();
    eh.add_recenter(None, &Recenter { kind: "MASS".into(), ..Default::default() }).unwrap();

    assert_eq!(
        eh.to_json().unwrap(),
        concat!(
            r#"{"FDSN":{"Event":{"Detection":[{"Type":"MURDOCK","SignalAmplitude":80.0,"SignalPeriod":0.4,"#,
            r#""Wave":"DILATATION","Units":"COUNTS","OnsetTime":"2022-06-05T20:32:39.120000Z","#,
            r#""MEDSNR":[1,3,2,1,4,0],"MEDLookback":2,"MEDPickAlgorithm":0,"Detector":"Z_SPWWSS"}]},"#,
            r#""Calibration":{"Sequence":[{"Type":"STEP","Steps":2,"StepFirstPulsePositive":true,"Duration":5.0}]},"#,
            r#""Time":{"Exception":[{"Count":3,"Type":"MISSING"}]},"#,
            r#""Recenter":{"Sequence":[{"Type":"MASS"}]}}}"#
        )
    );
}

#[test]
fn format_for_display() {
    let s = libmseed::extra::format_extra(r#"{"A":{"B":1,"C":[1,2]}}"#, 4);
    assert_eq!(
        s,
        "    \"A\": {\n      \"B\": 1,\n      \"C\": [\n        1,\n        2\n      ]\n    }\n"
    );
}

/// Extra headers from a corrupt record may start or end with a multi-byte character.
#[test]
fn format_extra_non_ascii_ends() {
    let s = libmseed::extra::format_extra("\u{FFFD}\"A\":1\u{FFFD}", 0);
    assert!(s.contains("\"A\": 1"));
    assert_eq!(libmseed::extra::format_extra("\u{FFFD}", 0), "");
}

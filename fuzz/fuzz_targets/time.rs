//! Parse arbitrary strings as times; anything parsed must format and parse back.
#![no_main]

use libfuzzer_sys::fuzz_target;
use libmseed::NsTime;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    if let Ok(t) = NsTime::parse(s) {
        let shown = t.to_string();
        assert_eq!(shown.parse::<NsTime>().ok(), Some(t), "{s:?} -> {shown}");
    }
    let _ = NsTime::parse_month_day(s);
    let _ = NsTime::parse_ordinal(s);
});

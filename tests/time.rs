//! Port of libmseed test/test-time.c

use libmseed::time::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ns(n: i64) -> NsTime {
    NsTime::from_nanos(n)
}

#[test]
fn nstime2timestr() {
    use Subseconds::*;
    use TimeFormat::*;
    let nstime = NsTime::parse("2004-05-12T7:8:9.123456788Z").unwrap();
    assert_eq!(nstime.as_nanos(), 1084345689123456788);

    let fmt = |t: NsTime, f, s| t.format(f, s);
    assert_eq!(fmt(nstime, IsoMonthDay, NanoMicroNone), "2004-05-12T07:08:09.123456788");
    assert_eq!(fmt(nstime, IsoMonthDayZ, NanoMicroNone), "2004-05-12T07:08:09.123456788Z");
    assert_eq!(fmt(nstime, IsoMonthDayDoy, NanoMicroNone), "2004-05-12T07:08:09.123456788 (133)");
    assert_eq!(fmt(nstime, IsoMonthDayDoyZ, NanoMicroNone), "2004-05-12T07:08:09.123456788Z (133)");
    assert_eq!(fmt(nstime, SeedOrdinal, NanoMicroNone), "2004,133,07:08:09.123456788");
    assert_eq!(fmt(nstime, UnixEpoch, NanoMicroNone), "1084345689.123456788");
    assert_eq!(fmt(nstime, NanosecondEpoch, NanoMicroNone), "1084345689123456788");

    // Extremes of the time scale
    assert_eq!(
        fmt(ns(-2145916799999999998), IsoMonthDayZ, NanoMicroNone),
        "1902-01-01T00:00:00.000000002Z"
    );
    assert_eq!(fmt(ns(i64::MAX), IsoMonthDayZ, NanoMicroNone), "2262-04-11T23:47:16.854775807Z");

    // Subsecond variations
    assert_eq!(fmt(nstime, IsoMonthDayZ, Nano), "2004-05-12T07:08:09.123456788Z");
    assert_eq!(fmt(nstime, IsoMonthDayZ, Micro), "2004-05-12T07:08:09.123456Z");
    assert_eq!(fmt(nstime, IsoMonthDayZ, Subseconds::None), "2004-05-12T07:08:09Z");

    let micro = NsTime::parse("2004-05-12T7:8:9.1234Z").unwrap();
    assert_eq!(fmt(micro, IsoMonthDayZ, NanoMicroNone), "2004-05-12T07:08:09.123400Z");
    assert_eq!(fmt(micro, IsoMonthDayZ, NanoMicro), "2004-05-12T07:08:09.123400Z");
    assert_eq!(fmt(micro, IsoMonthDayZ, MicroNone), "2004-05-12T07:08:09.123400Z");

    let whole = NsTime::parse("2004-05-12T7:8:9Z").unwrap();
    assert_eq!(fmt(whole, IsoMonthDayZ, NanoMicroNone), "2004-05-12T07:08:09Z");
    assert_eq!(fmt(whole, IsoMonthDayZ, NanoMicro), "2004-05-12T07:08:09.000000Z");
    assert_eq!(fmt(whole, IsoMonthDayZ, MicroNone), "2004-05-12T07:08:09Z");

    // Negative sub-second times: UNIXEPOCH integer seconds are floored
    assert_eq!(fmt(ns(-1_500_000_000), UnixEpoch, Subseconds::None), "-2");
    assert_eq!(fmt(ns(-1_500_000_000), IsoMonthDayZ, Subseconds::None), "1969-12-31T23:59:58Z");
    assert_eq!(fmt(ns(-1_500_000_000), UnixEpoch, Nano), "-1.500000000");
    assert_eq!(fmt(ns(-500_000_000), UnixEpoch, Nano), "-0.500000000");
}

#[test]
fn display_debug_and_fromstr() {
    let t: NsTime = "2004-05-12T7:8:9.123456788Z".parse().unwrap();
    assert_eq!(t.to_string(), "2004-05-12T07:08:09.123456788Z");
    assert_eq!(format!("{t:?}"), "NsTime(2004-05-12T07:08:09.123456788Z)");
    assert_eq!(ns(500_000).to_string(), "1970-01-01T00:00:00.000500Z");
    assert_eq!(NsTime::UNIX_EPOCH.to_string(), "1970-01-01T00:00:00Z");
    assert!("not a time".parse::<NsTime>().is_err());
    // Display output parses back to the same time
    assert_eq!(t.to_string().parse::<NsTime>().unwrap(), t);
}

#[test]
fn timestr2nstime() {
    let t = |s: &str| NsTime::parse(s).map(NsTime::as_nanos);
    assert_eq!(t("2004").unwrap(), 1072915200000000000);
    assert_eq!(t("2004-2-9").unwrap(), 1076284800000000000);
    assert_eq!(t("2004-05-12T7:8:9.12345Z").unwrap(), 1084345689123450000);
    assert_eq!(t("2004-05-12T7:8:9.12345").unwrap(), 1084345689123450000);
    assert_eq!(t("2004-05-12T7:8:9.123456788").unwrap(), 1084345689123456788);
    assert_eq!(t("1084345689.123456788").unwrap(), 1084345689123456788);
    assert_eq!(t("1969,201,20,17,40.98").unwrap(), -14182939020000000);
    assert_eq!(t("1969-201T20:17:40.987654321").unwrap(), -14182939012345679);
    assert_eq!(t("-14182939.012345679").unwrap(), -14182939012345679);

    // Extremes of the time scale
    assert_eq!(t("1902-01-01T00:00:00.000000002Z").unwrap(), -2145916799999999998);
    assert_eq!(t("2262-04-11T23:47:16.854775807Z").unwrap(), i64::MAX);

    // Near-boundary value that remains in range
    assert_eq!(t("+9223372036.5").unwrap(), 9223372036500000000);

    // Leap seconds repeat the following second on the epoch scale
    assert_eq!(t("2016-12-31T23:59:60").unwrap(), 1483228800000000000);
    assert_eq!(t("2016-12-31T23:59:60.9999999999").unwrap(), 1483228801000000000);
    assert_eq!(t("2016,366,23,59,60.9999999999").unwrap(), 1483228801000000000);

    // A fraction that rounds up to a full second on a non-leap second
    assert_eq!(t("2016-12-31T23:59:59.9999999999").unwrap(), 1483228800000000000);

    // Format-specific parsers
    assert_eq!(NsTime::parse_month_day("2004-2-9").unwrap().as_nanos(), 1076284800000000000);
    assert_eq!(NsTime::parse_ordinal("2004,040").unwrap().as_nanos(), 1076284800000000000);

    // Parsing errors
    assert!(t("this is not a time string").is_err());
    assert!(t("+99999999999").is_err());
    assert!(t("+9223372036.99999999").is_err());
    assert!(t("-9223372036.99999999").is_err());
    assert!(t("0000-00-00").is_err());
    assert!(t("5000-00-00").is_err());
    assert!(t("20040512T000000").is_err());
}

/// ms_sampletime() adjusts by one second per leap second contained in the span.
#[test]
fn sampletime_multileap() {
    let t = |s: &str| NsTime::parse(s).unwrap();
    let check = |start: &str, end: &str, leaps: i64| {
        let start = t(start);
        let offsetsec = t(end).nanos_since(start) / NSTMODULUS;
        let naivespan = (offsetsec as f64 * 1.0 * NSTMODULUS as f64 + 0.5) as i64;
        let expected = start.add_nanos(naivespan - leaps * NSTMODULUS);
        assert_eq!(start.sample_time(offsetsec as u64, -1.0).unwrap(), expected);
    };
    // 2015-07-01 and 2017-01-01
    check("2015-01-01T00:00:00Z", "2017-06-01T00:00:00Z", 2);
    check("2015-01-01T00:00:00Z", "2016-06-01T00:00:00Z", 1);
    check("2018-01-01T00:00:00Z", "2018-02-01T00:00:00Z", 0);
}

/// ms_sampletime() rejects spans that are not representable.
#[test]
fn sampletime_overflow() {
    let start = NsTime::parse("2020-01-01T00:00:00Z").unwrap();
    assert!(start.sample_time(2, 5e-324).is_err());
    assert!(start.sample_time(100, 1e-9).is_err());
    assert_eq!(
        start.sample_time(100, 40.0).unwrap(),
        start.add_nanos((100.0 / 40.0 * NSTMODULUS as f64 + 0.5) as i64)
    );
}

#[test]
fn now_and_system_time() {
    let now = NsTime::now();
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    assert!(now > NsTime::UNIX_EPOCH);
    assert!(now.as_nanos() - secs * NSTMODULUS < NSTMODULUS + 1_000_000_000);

    let t = NsTime::parse("2004-05-12T7:8:9.123456788Z").unwrap();
    assert_eq!(NsTime::try_from(SystemTime::from(t)).unwrap(), t);
    let before = ns(-1_500_000_000);
    assert_eq!(NsTime::try_from(SystemTime::from(before)).unwrap(), before);
}

#[test]
fn duration_arithmetic() {
    let t = NsTime::parse("2004-05-12T00:00:00Z").unwrap();
    let later = t + Duration::from_millis(1500);
    assert_eq!(later.nanos_since(t), 1_500_000_000);
    assert_eq!(later - Duration::from_millis(1500), t);
    let mut u = t;
    u += Duration::from_secs(86400);
    assert_eq!(u.parts().yday, t.parts().yday + 1);
    u -= Duration::from_secs(86400);
    assert_eq!(u, t);
    assert_eq!(ns(i64::MAX).checked_add(Duration::from_nanos(1)), None);
    assert_eq!(ns(i64::MIN).checked_sub(Duration::from_nanos(1)), None);
    assert_eq!(ns(5).checked_add_nanos(-10), Some(ns(-5)));
}

#[test]
fn components_and_calendar() {
    let t = NsTime::from_ordinal(2004, 133, 7, 8, 9, 123456788).unwrap();
    assert_eq!(t.as_nanos(), 1084345689123456788);
    assert_eq!(NsTime::from_calendar(2004, 5, 12, 7, 8, 9, 123456788).unwrap(), t);
    let p = t.parts();
    assert_eq!((p.year, p.yday, p.hour, p.min, p.sec, p.nsec), (2004, 133, 7, 8, 9, 123456788));
    assert_eq!((p.month, p.mday), (5, 12));
    assert!(NsTime::from_ordinal(2003, 366, 0, 0, 0, 0).is_err());
    assert!(NsTime::from_calendar(2003, 2, 29, 0, 0, 0, 0).is_err());
    assert_eq!(NsTime::from_calendar(2004, 2, 29, 0, 0, 0, 0).unwrap().parts().yday, 60);
    assert!(NsTime::from_ordinal(1600, 1, 0, 0, 0, 0).is_err());
    // Pre-epoch round trip
    let p = ns(-1).parts();
    assert_eq!((p.year, p.yday, p.hour, p.min, p.sec, p.nsec), (1969, 365, 23, 59, 59, 999999999));
}

#[test]
fn leap_second_file_parse() {
    let list = parse_leap_seconds(
        "# comment\n#@ 3913697179\n\n2272060800 10 # 1 Jan 1972\n3692217600 37\n",
    )
    .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].tai_delta, 10);
    assert_eq!(list[0].leapsecond, ns(63072000 * NSTMODULUS));
}

/// Every representable time displays as a string that parses back to it, including the
/// extremes of the range (found by fuzzing: 1677 dates were rejected by the date parser).
#[test]
fn display_roundtrip_at_range_ends() {
    for nanos in [i64::MIN, i64::MIN + 1, -9_220_022_220 * NSTMODULUS, 0, i64::MAX] {
        let t = ns(nanos);
        assert_eq!(t.to_string().parse::<NsTime>().unwrap(), t, "{t}");
    }
    assert_eq!(NsTime::parse("-00000000000000009220022220").unwrap().parts().year, 1677);
    assert!(NsTime::parse("1677-09-21T00:12:43.145224191Z").is_err());
    assert!(NsTime::parse("2262-04-11T23:47:16.854775808Z").is_err());
}

/// Date-time components include the calendar month and day.
#[test]
fn parts_calendar() {
    let p = "2024-03-01T12:34:56.789Z".parse::<NsTime>().unwrap().parts();
    assert_eq!((p.year, p.month, p.mday, p.yday), (2024, 3, 1, 61));
    assert_eq!((p.hour, p.min, p.sec, p.nsec), (12, 34, 56, 789_000_000));
    let p = "1677-12-31T23:59:59Z".parse::<NsTime>().unwrap().parts();
    assert_eq!((p.year, p.month, p.mday, p.yday), (1677, 12, 31, 365));
}

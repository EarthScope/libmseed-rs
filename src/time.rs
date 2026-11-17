//! Time representation, conversion and leap second handling.
//!
//! Times are [`NsTime`] values: integer nanoseconds since the Unix/POSIX epoch
//! (1970-01-01T00:00:00Z), covering 1677-09-21T00:12:43.145224192Z through
//! 2262-04-11T23:47:16.854775807Z.
//!
//! ```
//! use libmseed::NsTime;
//! use std::time::Duration;
//!
//! let t: NsTime = "2024-01-01T00:00:00.5Z".parse()?;
//! assert_eq!(t.to_string(), "2024-01-01T00:00:00.500000Z");
//! assert_eq!((t + Duration::from_secs(86400)).parts().yday, 2);
//! assert_eq!("2024,001,00:00:00.5".parse::<NsTime>()?, t);
//! # Ok::<(), libmseed::Error>(())
//! ```

use crate::error::{Error, Result};
use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::str::FromStr;
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Nanoseconds per second, the ticks per second of [`NsTime`].
pub const NSTMODULUS: i64 = 1_000_000_000;

/// A point in time: integer nanoseconds since the Unix/POSIX epoch (UTC).
///
/// Leap seconds are not counted, as in POSIX time; see [`NsTime::sample_time`] for how
/// they are accounted for in sample times.
///
/// [`Display`](fmt::Display) gives ISO 8601 with a `Z` and as many subsecond digits as
/// needed (none, 6 or 9); [`NsTime::format`] offers other formats. [`FromStr`] accepts
/// the formats of [`NsTime::parse`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NsTime(i64);

impl NsTime {
    /// The Unix epoch, 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: NsTime = NsTime(0);

    /// The time `nanos` nanoseconds after the Unix epoch (before it, if negative).
    #[must_use]
    pub const fn from_nanos(nanos: i64) -> Self {
        NsTime(nanos)
    }

    /// Nanoseconds since the Unix epoch.
    #[must_use]
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// Seconds since the Unix epoch as a float, with sub-microsecond precision lost for
    /// times far from the epoch.
    #[must_use]
    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / NSTMODULUS as f64
    }

    /// The current system time.
    #[must_use]
    pub fn now() -> Self {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => NsTime(d.as_nanos() as i64),
            Err(e) => NsTime(-(e.duration().as_nanos() as i64)),
        }
    }

    /// The time from year, day-of-year, hour, minute, second and nanoseconds.
    ///
    /// A second of 60 (a leap second) is accepted and is the same time as second 0 of
    /// the next minute.
    pub fn from_ordinal(
        year: i32,
        yday: i32,
        hour: i32,
        min: i32,
        sec: i32,
        nsec: u32,
    ) -> Result<Self> {
        time_to_nstime(year, yday, hour, min, sec, nsec).map(NsTime)
    }

    /// The time from year, month, day-of-month, hour, minute, second and nanoseconds.
    pub fn from_calendar(
        year: i32,
        month: i32,
        mday: i32,
        hour: i32,
        min: i32,
        sec: i32,
        nsec: u32,
    ) -> Result<Self> {
        Self::from_ordinal(year, md_to_doy(year, month, mday)?, hour, min, sec, nsec)
    }

    /// Parse a time string, detecting its format: ISO 8601 month-day
    /// (`2024-01-31T12:00:00.5Z`), ISO or SEED ordinal (`2024-031T12:00:00`,
    /// `2024,031,12:00:00`), a 4-digit year, or Unix epoch seconds (`1706702400.5`, a
    /// 4-digit value needs a sign to be epoch seconds).
    pub fn parse(s: &str) -> Result<Self> {
        timestr_to_nstime(s).map(NsTime)
    }

    /// Parse a month-day time string `YYYY[-MM-DD HH:MM:SS.FFFFFFFFF]`, with delimiters
    /// from `-,/:.` and `T` or space between date and time; omitted trailing values are
    /// zero (month and day default to 1).
    pub fn parse_month_day(s: &str) -> Result<Self> {
        mdtimestr_to_nstime(s).map(NsTime)
    }

    /// Parse an ordinal time string `YYYY[,DDD,HH,MM,SS.FFFFFFFFF]`, with delimiters from
    /// `-,:.`; omitted trailing values are zero (day-of-year defaults to 1).
    pub fn parse_ordinal(s: &str) -> Result<Self> {
        seedtimestr_to_nstime(s).map(NsTime)
    }

    /// Date-time components.
    #[must_use]
    pub fn parts(self) -> TimeParts {
        nstime_to_time(self.0)
    }

    /// Format as a string. Subsecond values are truncated, not rounded.
    #[must_use]
    pub fn format(self, format: TimeFormat, subseconds: Subseconds) -> String {
        nstime_to_string(self.0, format, subseconds)
    }

    /// This time plus `nanos` nanoseconds, `None` on overflow.
    pub fn checked_add_nanos(self, nanos: i64) -> Option<Self> {
        self.0.checked_add(nanos).map(NsTime)
    }

    /// This time plus `nanos` nanoseconds (minus, if negative), saturating at the
    /// representable range; see [`NsTime::checked_add_nanos`] to detect overflow.
    #[must_use]
    pub fn add_nanos(self, nanos: i64) -> Self {
        NsTime(self.0.saturating_add(nanos))
    }

    /// Nanoseconds from `earlier` to this time, negative if `earlier` is later.
    ///
    /// Saturates at the `i64` range, which can only be exceeded by times more than
    /// about 292 years apart.
    #[must_use]
    pub fn nanos_since(self, earlier: NsTime) -> i64 {
        self.0.saturating_sub(earlier.0)
    }

    /// This time plus `d`, `None` if not representable.
    #[must_use]
    pub fn checked_add(self, d: Duration) -> Option<Self> {
        i64::try_from(d.as_nanos()).ok().and_then(|n| self.checked_add_nanos(n))
    }

    /// This time minus `d`, `None` if not representable.
    pub fn checked_sub(self, d: Duration) -> Option<Self> {
        i64::try_from(d.as_nanos()).ok().and_then(|n| self.0.checked_sub(n)).map(NsTime)
    }
}

impl Add<Duration> for NsTime {
    type Output = NsTime;
    /// # Panics
    ///
    /// Panics if the result is not representable.
    fn add(self, d: Duration) -> NsTime {
        self.checked_add(d).expect("overflow when adding duration to NsTime")
    }
}

impl AddAssign<Duration> for NsTime {
    fn add_assign(&mut self, d: Duration) {
        *self = *self + d;
    }
}

impl Sub<Duration> for NsTime {
    type Output = NsTime;
    /// # Panics
    ///
    /// Panics if the result is not representable.
    fn sub(self, d: Duration) -> NsTime {
        self.checked_sub(d).expect("overflow when subtracting duration from NsTime")
    }
}

impl SubAssign<Duration> for NsTime {
    fn sub_assign(&mut self, d: Duration) {
        *self = *self - d;
    }
}

impl fmt::Display for NsTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format(TimeFormat::IsoMonthDayZ, Subseconds::NanoMicroNone))
    }
}

impl fmt::Debug for NsTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NsTime({self})")
    }
}

impl FromStr for NsTime {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        NsTime::parse(s)
    }
}

impl From<NsTime> for SystemTime {
    fn from(t: NsTime) -> SystemTime {
        let d = Duration::from_nanos(t.0.unsigned_abs());
        if t.0 >= 0 {
            UNIX_EPOCH + d
        } else {
            UNIX_EPOCH - d
        }
    }
}

impl TryFrom<SystemTime> for NsTime {
    type Error = Error;
    fn try_from(t: SystemTime) -> Result<Self> {
        let nanos = match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_nanos()).ok(),
            Err(e) => i64::try_from(e.duration().as_nanos()).ok().map(|n| -n),
        };
        nanos
            .map(NsTime)
            .ok_or_else(|| Error::InvalidTime("System time is beyond the NsTime range".into()))
    }
}

/// Time string formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimeFormat {
    /// `YYYY-MM-DDThh:mm:ss.sssssssss`
    IsoMonthDay,
    /// `YYYY-MM-DDThh:mm:ss.sssssssssZ`
    IsoMonthDayZ,
    /// `YYYY-MM-DDThh:mm:ss.sssssssss (doy)`
    IsoMonthDayDoy,
    /// `YYYY-MM-DDThh:mm:ss.sssssssssZ (doy)`
    IsoMonthDayDoyZ,
    /// `YYYY-MM-DD hh:mm:ss.sssssssss`
    IsoMonthDaySpace,
    /// `YYYY-MM-DD hh:mm:ss.sssssssssZ`
    IsoMonthDaySpaceZ,
    /// `YYYY,DDD,hh:mm:ss.sssssssss`
    SeedOrdinal,
    /// `ssssssssss.sssssssss`
    UnixEpoch,
    /// `sssssssssssssssssss`
    NanosecondEpoch,
}

/// Subsecond formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Subseconds {
    /// No subseconds.
    None,
    /// Microsecond resolution.
    Micro,
    /// Nanosecond resolution.
    Nano,
    /// Microsecond resolution if non-zero, otherwise none.
    MicroNone,
    /// Nanosecond resolution if non-zero, otherwise none.
    NanoNone,
    /// Nanosecond resolution if sub-microseconds present, otherwise microseconds.
    NanoMicro,
    /// Nanoseconds if present, microseconds if present, otherwise none.
    NanoMicroNone,
}

/// Broken-down UTC time components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TimeParts {
    /// Year with century.
    pub year: u16,
    /// Month, 1 - 12.
    pub month: u8,
    /// Day of month, 1 - 31.
    pub mday: u8,
    /// Day of year, 1 - 366.
    pub yday: u16,
    /// Hour, 0 - 23.
    pub hour: u8,
    /// Minute, 0 - 59.
    pub min: u8,
    /// Second, 0 - 59: leap seconds are not counted, see [`NsTime`] and
    /// [`NsTime::sample_time`].
    pub sec: u8,
    /// Nanoseconds, 0 - 999999999.
    pub nsec: u32,
}

const MONTHDAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTHDAYS_LEAP: [i32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
/// Years that are at least partly representable; the exact range is checked when
/// converting to nanoseconds.
fn valid_year(y: i32) -> bool {
    (1677..=2262).contains(&y)
}
fn valid_yday(y: i32, d: i32) -> bool {
    d >= 1 && d <= 365 + is_leap(y) as i32
}
fn month_days(y: i32) -> &'static [i32; 12] {
    if is_leap(y) {
        &MONTHDAYS_LEAP
    } else {
        &MONTHDAYS
    }
}

fn err<T>(msg: String) -> Result<T> {
    Err(Error::InvalidTime(msg))
}

/// Compute day-of-year from year, month and day-of-month.
pub(crate) fn md_to_doy(year: i32, month: i32, mday: i32) -> Result<i32> {
    if !valid_year(year) {
        return err(format!("year ({year}) is out of range"));
    }
    if !(1..=12).contains(&month) {
        return err(format!("month ({month}) is out of range"));
    }
    let days = month_days(year);
    if mday < 1 || mday > days[(month - 1) as usize] {
        return err(format!(
            "day-of-month ({mday}) is out of range for year {year} and month {month}"
        ));
    }
    Ok(days[..(month - 1) as usize].iter().sum::<i32>() + mday)
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Civil date (year, month, day) from days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Split into floored epoch seconds and non-negative nanoseconds.
fn split_floor(nstime: i64) -> (i64, u32) {
    (nstime.div_euclid(NSTMODULUS), nstime.rem_euclid(NSTMODULUS) as u32)
}

fn nstime_to_time(nstime: i64) -> TimeParts {
    let (isec, nsec) = split_floor(nstime);
    let days = isec.div_euclid(86400);
    let sod = isec.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let yday = days - days_from_civil(y, 1, 1) + 1;
    TimeParts {
        year: y as u16,
        month: m as u8,
        mday: d as u8,
        yday: yday as u16,
        hour: (sod / 3600) as u8,
        min: ((sod % 3600) / 60) as u8,
        sec: (sod % 60) as u8,
        nsec,
    }
}

fn nstime_to_string(nstime: i64, format: TimeFormat, sub: Subseconds) -> String {
    use Subseconds as S;
    use TimeFormat as F;

    // Truncated-toward-zero seconds and fraction magnitude (for UNIXEPOCH sub-second output)
    let rawisec = nstime / NSTMODULUS;
    let rawnanosec = (nstime - rawisec * NSTMODULUS).unsigned_abs();
    let (isec, nanosec) = split_floor(nstime);
    let microsec = nanosec / 1000;
    let submicro = nanosec - microsec * 1000;

    #[derive(PartialEq)]
    enum Res {
        None,
        Micro,
        Nano,
    }
    let res = if sub == S::None
        || (sub == S::MicroNone && microsec == 0)
        || (sub == S::NanoNone && nanosec == 0)
        || (sub == S::NanoMicroNone && nanosec == 0)
    {
        Res::None
    } else if sub == S::Micro
        || (sub == S::MicroNone && microsec != 0)
        || (sub == S::NanoMicro && submicro == 0)
        || (sub == S::NanoMicroNone && submicro == 0)
    {
        Res::Micro
    } else {
        Res::Nano
    };

    if format == F::NanosecondEpoch {
        return nstime.to_string();
    }

    let neg_zero = if nstime < 0 && rawisec == 0 { "-" } else { "" };
    if format == F::UnixEpoch {
        return match res {
            Res::None => isec.to_string(),
            Res::Micro => format!("{neg_zero}{rawisec}.{:06}", rawnanosec / 1000),
            Res::Nano => format!("{neg_zero}{rawisec}.{rawnanosec:09}"),
        };
    }

    let t = nstime_to_time(nstime);
    let (_, month, mday) = civil_from_days(isec.div_euclid(86400));
    let frac = match res {
        Res::None => String::new(),
        Res::Micro => format!(".{microsec:06}"),
        Res::Nano => format!(".{nanosec:09}"),
    };
    let hms = format!("{:02}:{:02}:{:02}", t.hour, t.min, t.sec);
    let zed = matches!(format, F::IsoMonthDayZ | F::IsoMonthDayDoyZ | F::IsoMonthDaySpaceZ);
    let z = if zed { "Z" } else { "" };
    match format {
        F::IsoMonthDay | F::IsoMonthDayZ | F::IsoMonthDaySpace | F::IsoMonthDaySpaceZ => {
            let sep = if matches!(format, F::IsoMonthDaySpace | F::IsoMonthDaySpaceZ) {
                ' '
            } else {
                'T'
            };
            format!("{:4}-{month:02}-{mday:02}{sep}{hms}{frac}{z}", t.year)
        }
        F::IsoMonthDayDoy | F::IsoMonthDayDoyZ => {
            format!("{:4}-{month:02}-{mday:02}T{hms}{frac}{z} ({:03})", t.year, t.yday)
        }
        F::SeedOrdinal => format!("{:4},{:03},{hms}{frac}", t.year, t.yday),
        F::UnixEpoch | F::NanosecondEpoch => unreachable!(),
    }
}

fn time_to_nstime_int(
    year: i32,
    yday: i32,
    hour: i32,
    min: i32,
    sec: i32,
    nsec: u32,
) -> Result<i64> {
    let days = days_from_civil(year as i64, 1, 1) + (yday as i64 - 1);
    let seconds = days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64;
    // Wider arithmetic: the earliest representable second times 1e9 is below i64::MIN
    let nanos = seconds as i128 * NSTMODULUS as i128 + nsec as i128;
    i64::try_from(nanos).map_err(|_| {
        Error::InvalidTime(format!(
            "Time (year {year}, day {yday}) is beyond the representable nstime range"
        ))
    })
}

fn time_to_nstime(year: i32, yday: i32, hour: i32, min: i32, sec: i32, nsec: u32) -> Result<i64> {
    if !valid_year(year) {
        return err(format!("year ({year}) is out of range"));
    }
    if !valid_yday(year, yday) {
        return err(format!("day-of-year ({yday}) is out of range for year {year}"));
    }
    if !(0..=23).contains(&hour) {
        return err(format!("hour ({hour}) is out of range"));
    }
    if !(0..=59).contains(&min) {
        return err(format!("minute ({min}) is out of range"));
    }
    if !(0..=60).contains(&sec) {
        return err(format!("second ({sec}) is out of range"));
    }
    if nsec > 999_999_999 {
        return err(format!("nanosecond ({nsec}) is out of range"));
    }
    time_to_nstime_int(year, yday, hour, min, sec, nsec)
}

/// Fraction digit string (leading period optional) to nanoseconds, rounded to nearest.
/// Result may equal 1_000_000_000 when the fraction rounds up to a whole second.
fn frac_to_nsec(frac: &[u8]) -> u32 {
    let mut it = frac.iter().copied().peekable();
    if it.peek() == Some(&b'.') {
        it.next();
    }
    let mut nsec = 0u32;
    let mut n = 0;
    while n < 9 {
        match it.peek() {
            Some(c) if c.is_ascii_digit() => {
                nsec = nsec * 10 + (*c - b'0') as u32;
                it.next();
                n += 1;
            }
            _ => break,
        }
    }
    for _ in n..9 {
        nsec *= 10;
    }
    if let Some(c) = it.peek() {
        if (b'5'..=b'9').contains(c) {
            nsec += 1;
        }
    }
    nsec
}

/// Minimal emulation of the sscanf() conversions the C parsers use.
struct Scan<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Scan<'a> {
    fn new(s: &'a str) -> Self {
        Scan { s: s.as_bytes(), pos: 0 }
    }
    /// `%d`: skips whitespace, optional sign, digits.
    fn int(&mut self) -> Option<i64> {
        while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let start = self.pos;
        let mut p = self.pos;
        if p < self.s.len() && (self.s[p] == b'-' || self.s[p] == b'+') {
            p += 1;
        }
        let dstart = p;
        while p < self.s.len() && self.s[p].is_ascii_digit() {
            p += 1;
        }
        if p == dstart {
            return None;
        }
        let v = std::str::from_utf8(&self.s[start..p]).ok()?.parse::<i64>().ok()?;
        self.pos = p;
        Some(v)
    }
    /// `%*[set]`: skip one or more characters in set; fails if none match.
    fn skip(&mut self, set: &[u8]) -> bool {
        let start = self.pos;
        while self.pos < self.s.len() && set.contains(&self.s[self.pos]) {
            self.pos += 1;
        }
        self.pos > start
    }
    /// `%15[.0-9]`
    fn frac(&mut self) -> Option<&'a [u8]> {
        let start = self.pos;
        while self.pos < self.s.len()
            && self.pos - start < 15
            && (self.s[self.pos] == b'.' || self.s[self.pos].is_ascii_digit())
        {
            self.pos += 1;
        }
        (self.pos > start).then(|| &self.s[start..self.pos])
    }
}

/// Run a sequence of "int, skip-set" scans, stopping at the first failure like sscanf().
/// Returns the parsed ints (defaults retained for unparsed), count, and fraction text.
fn scan_fields<'a>(s: &'a str, seps: &[&[u8]], vals: &mut [i64]) -> (usize, Option<&'a [u8]>) {
    let mut sc = Scan::new(s);
    let mut count = 0;
    for i in 0..vals.len() {
        if i > 0 && !sc.skip(seps[i - 1]) {
            return (count, None);
        }
        match sc.int() {
            Some(v) => {
                vals[i] = v;
                count += 1;
            }
            None => return (count, None),
        }
    }
    (count, sc.frac())
}

fn to_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn mdtimestr_to_nstime(timestr: &str) -> Result<i64> {
    let seps: [&[u8]; 5] = [b"-,/:.", b"-,/:.", b"-,/:.Tt ", b"-,/:.", b"-,/:."];
    let mut v = [0i64, 1, 1, 0, 0, 0];
    let (fields, frac) = scan_fields(timestr, &seps, &mut v);
    let (mut nsec, mut carry) = (0u32, 0i32);
    if let Some(f) = frac {
        nsec = frac_to_nsec(f);
        if nsec >= 1_000_000_000 {
            nsec = 0;
            carry = 1;
        }
    }
    if fields < 1 {
        return err(format!("Cannot parse time string: {timestr}"));
    }
    let (year, mon, mday) = (to_i32(v[0]), to_i32(v[1]), to_i32(v[2]));
    let (hour, min, sec) = (to_i32(v[3]), to_i32(v[4]), to_i32(v[5]));
    if !valid_year(year) {
        return err(format!("year ({year}) is out of range"));
    }
    if !(1..=12).contains(&mon) {
        return err(format!("month ({mon}) is out of range"));
    }
    let yday = md_to_doy(year, mon, mday)?;
    if !(0..=23).contains(&hour) {
        return err(format!("hour ({hour}) is out of range"));
    }
    if !(0..=59).contains(&min) {
        return err(format!("minute ({min}) is out of range"));
    }
    if !(0..=60).contains(&sec) {
        return err(format!("second ({sec}) is out of range"));
    }
    time_to_nstime_int(year, yday, hour, min, sec + carry, nsec)
}

fn seedtimestr_to_nstime(timestr: &str) -> Result<i64> {
    let seps: [&[u8]; 4] = [b"-,:.", b"-,:.Tt ", b"-,:.", b"-,:."];
    let mut v = [0i64, 1, 0, 0, 0];
    let (fields, frac) = scan_fields(timestr, &seps, &mut v);
    let (mut nsec, mut carry) = (0u32, 0i32);
    if let Some(f) = frac {
        nsec = frac_to_nsec(f);
        if nsec >= 1_000_000_000 {
            nsec = 0;
            carry = 1;
        }
    }
    if fields < 1 {
        return err(format!("Cannot parse time string: {timestr}"));
    }
    let (year, yday) = (to_i32(v[0]), to_i32(v[1]));
    let (hour, min, sec) = (to_i32(v[2]), to_i32(v[3]), to_i32(v[4]));
    if !valid_year(year) {
        return err(format!("year ({year}) is out of range"));
    }
    if !valid_yday(year, yday) {
        return err(format!("day-of-year ({yday}) is out of range for year {year}"));
    }
    if !(0..=23).contains(&hour) {
        return err(format!("hour ({hour}) is out of range"));
    }
    if !(0..=59).contains(&min) {
        return err(format!("minute ({min}) is out of range"));
    }
    if !(0..=60).contains(&sec) {
        return err(format!("second ({sec}) is out of range"));
    }
    time_to_nstime_int(year, yday, hour, min, sec + carry, nsec)
}

fn timestr_to_nstime(timestr: &str) -> Result<i64> {
    let b = timestr.as_bytes();
    let mut first_delim: Option<usize> = None;
    let mut separator: Option<usize> = None;
    let mut delimiters = 0;
    let mut numberlike = 0;
    let mut error = false;
    let mut end = b.len();

    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if matches!(c, b'-' | b'/' | b',' | b':' | b'.') {
            first_delim.get_or_insert(i);
            if separator.is_none() {
                delimiters += 1;
            }
            if (c == b'-' && i == 0) || c == b'.' {
                numberlike += 1;
            }
        } else if c == b'+' && i == 0 {
            numberlike += 1;
        } else if separator.is_none() && (c == b'T' || c == b' ') {
            separator = Some(i);
        } else if c.is_ascii_digit() {
            numberlike += 1;
        } else if (c == b'Z' || c == b'z') && i + 1 == b.len() {
            end = b.len();
            break;
        } else {
            error = true;
            end = i + 1;
            break;
        }
        i += 1;
        end = i;
    }
    let length = end;

    if !error && length == numberlike && (length != 4 || b[0] == b'-' || b[0] == b'+') {
        // Epoch value: integer seconds with optional fraction
        let mut sc = Scan::new(timestr);
        let sec = match sc.int() {
            Some(s) => s,
            None => return err(format!("Could not convert epoch value: '{timestr}'")),
        };
        let frac = sc.frac();
        if !(i64::MIN / NSTMODULUS..=i64::MAX / NSTMODULUS).contains(&sec) {
            return err(format!(
                "Epoch value ({timestr}) is beyond the representable nstime range"
            ));
        }
        let mut nstime = sec * NSTMODULUS;
        if let Some(f) = frac {
            let nanofrac = frac_to_nsec(f) as i64;
            let range_err =
                || err(format!("Epoch value ({timestr}) is beyond the representable nstime range"));
            if sec < 0 || b[0] == b'-' {
                nstime = match nstime.checked_sub(nanofrac) {
                    Some(v) => v,
                    None => return range_err(),
                };
            } else {
                nstime = match nstime.checked_add(nanofrac) {
                    Some(v) => v,
                    None => return range_err(),
                };
            }
        }
        return Ok(nstime);
    } else if !error && (4..=32).contains(&length) {
        match first_delim {
            Some(fd) => {
                if (b[fd] == b'-' || b[fd] == b'/') && delimiters == 2 {
                    return mdtimestr_to_nstime(timestr);
                } else if (b[fd] == b'-' && delimiters == 1) || b[fd] == b',' {
                    return seedtimestr_to_nstime(timestr);
                }
            }
            None => {
                if length == 4 && separator.is_none() {
                    return seedtimestr_to_nstime(timestr);
                }
            }
        }
    }
    err(format!("Unrecognized time string: '{timestr}'"))
}

/// A leap second entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeapSecond {
    /// Time of the leap second as an [`NsTime`] (the second *following* the inserted one).
    pub leapsecond: NsTime,
    /// TAI-UTC difference in seconds.
    pub tai_delta: i32,
}

const NTP_POSIX_EPOCH_DELTA: i64 = 2_208_988_800;

/// Embedded leap second list as NTP epoch seconds and TAI delta, good through 2023.
const EMBEDDED_LEAPS: [(i64, i32); 28] = [
    (2272060800, 10),
    (2287785600, 11),
    (2303683200, 12),
    (2335219200, 13),
    (2366755200, 14),
    (2398291200, 15),
    (2429913600, 16),
    (2461449600, 17),
    (2492985600, 18),
    (2524521600, 19),
    (2571782400, 20),
    (2603318400, 21),
    (2634854400, 22),
    (2698012800, 23),
    (2776982400, 24),
    (2840140800, 25),
    (2871676800, 26),
    (2918937600, 27),
    (2950473600, 28),
    (2982009600, 29),
    (3029443200, 30),
    (3076704000, 31),
    (3124137600, 32),
    (3345062400, 33),
    (3439756800, 34),
    (3550089600, 35),
    (3644697600, 36),
    (3692217600, 37),
];

static LEAPS: RwLock<Option<Vec<LeapSecond>>> = RwLock::new(None);

/// The embedded list converted to [`LeapSecond`]s at compile time.
static EMBEDDED: [LeapSecond; EMBEDDED_LEAPS.len()] = {
    let mut out = [LeapSecond { leapsecond: NsTime(0), tai_delta: 0 }; EMBEDDED_LEAPS.len()];
    let mut i = 0;
    while i < EMBEDDED_LEAPS.len() {
        out[i] = LeapSecond {
            leapsecond: NsTime((EMBEDDED_LEAPS[i].0 - NTP_POSIX_EPOCH_DELTA) * NSTMODULUS),
            tai_delta: EMBEDDED_LEAPS[i].1,
        };
        i += 1;
    }
    out
};

/// Run `f` on the global leap second list without copying it.
fn with_leap_seconds<R>(f: impl FnOnce(&[LeapSecond]) -> R) -> R {
    // The list is only ever replaced whole, so a poisoned lock still holds a valid list
    let guard = LEAPS.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.as_deref().unwrap_or(&EMBEDDED))
}

/// Return a copy of the global leap second list (embedded list unless replaced).
///
/// The embedded list includes the leap seconds announced through 2023 (the last was at
/// the end of 2016). Install a current list with [`read_leap_second_file`] or
/// [`set_leap_seconds`] when processing data after the embedded list's expiry.
pub fn leap_seconds() -> Vec<LeapSecond> {
    with_leap_seconds(<[LeapSecond]>::to_vec)
}

/// Replace the global leap second list.
pub fn set_leap_seconds(list: Vec<LeapSecond>) {
    *LEAPS.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(list);
}

/// Parse a leap second list in NTP format.
///
/// Comment lines start with `#`, an expiration line is `#@ <ntp-seconds>`. Other lines
/// that do not start with an NTP time and a TAI offset are ignored.
/// Returns the list; does not modify the global list.
pub fn parse_leap_seconds(text: &str) -> Result<Vec<LeapSecond>> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(a), Some(b)) = (it.next(), it.next()) else { continue };
        let (Ok(ntp), Ok(tai)) = (a.parse::<i64>(), b.parse::<i32>()) else { continue };
        let unix = ntp.saturating_sub(NTP_POSIX_EPOCH_DELTA);
        if !(i64::MIN / NSTMODULUS..=i64::MAX / NSTMODULUS).contains(&unix) {
            return err("Leap second epoch is beyond the representable nstime range".into());
        }
        out.push(LeapSecond { leapsecond: NsTime(unix * NSTMODULUS), tai_delta: tai });
    }
    Ok(out)
}

/// Read a leap second file and install it as the global list; returns the entry count.
pub fn read_leap_second_file(path: impl AsRef<std::path::Path>) -> Result<usize> {
    let text = std::fs::read_to_string(path.as_ref()).map_err(|e| {
        Error::io(e, format_args!("Cannot read leap second file {}", path.as_ref().display()))
    })?;
    let list = parse_leap_seconds(&text)?;
    let n = list.len();
    set_leap_seconds(list);
    Ok(n)
}

/// Read the leap second file named by an environment variable.
///
/// Returns `Ok(None)` when the variable is not set.
pub fn read_leap_seconds_env(var: &str) -> Result<Option<usize>> {
    match std::env::var_os(var) {
        Some(p) => read_leap_second_file(p).map(Some),
        None => Ok(None),
    }
}

impl NsTime {
    /// The time of the sample at `offset` from this first sample time, at `samprate`.
    ///
    /// A negative `samprate` is a sample period in seconds. The result is reduced by one
    /// second for each leap second in the global list (see [`leap_seconds`]) that is
    /// completely contained by the span, so that sample times stay on the UTC second
    /// grid across leap seconds.
    pub fn sample_time(self, offset: u64, samprate: f64) -> Result<NsTime> {
        let time = self.0;
        let mut span: i64 = 0;
        if offset > 0 {
            let mut spandouble = 0.0f64;
            if samprate > 0.0 {
                spandouble = offset as f64 / samprate * NSTMODULUS as f64 + 0.5;
            } else if samprate < 0.0 {
                spandouble = offset as f64 * -samprate * NSTMODULUS as f64 + 0.5;
            }
            if !spandouble.is_finite() || spandouble >= i64::MAX as f64 {
                return err(format!(
                    "Time span is not representable (offset: {offset}, samprate: {samprate})"
                ));
            }
            span = spandouble as i64;
        }
        let Some(end) = time.checked_add(span) else {
            return err("Sample time is not representable".into());
        };
        // Remove a second for each leap second completely contained in the span; as in
        // libmseed, each test uses the span reduced by the leap seconds already counted
        Ok(NsTime(with_leap_seconds(|list| {
            list.iter().fold(end, |end, ls| {
                let leap = ls.leapsecond.0;
                if leap > time && leap.checked_add(NSTMODULUS).is_some_and(|l| l <= end) {
                    end - NSTMODULUS
                } else {
                    end
                }
            })
        })))
    }
}

//! Data selections: source identifier patterns with optional time windows.
//!
//! A [`Selections`] is a list of entries; data matching *any* entry is selected (OR logic).
//! Source identifier patterns are globs supporting `*`, `?`, `[abc]`, `[a-z]`, `[^abc]`
//! and `\` escapes.

use crate::error::{Error, Result};
use crate::record::Record;
use crate::sid::{nslc_to_sid_max, seedchan_to_xchan};
use crate::time::{NsTime, Subseconds, TimeFormat};
use std::fmt;

/// A time window of a selection; `None` bounds are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectTime {
    /// Earliest data selected, `None` for open.
    pub start_time: Option<NsTime>,
    /// Latest data selected, `None` for open.
    pub end_time: Option<NsTime>,
}

impl SelectTime {
    /// True if this window and the time range from `start` to `end` intersect, inclusive;
    /// `None` bounds are open.
    #[must_use]
    pub fn intersects(&self, start: Option<NsTime>, end: Option<NsTime>) -> bool {
        let before_end = |a: Option<NsTime>, b: Option<NsTime>| match (a, b) {
            (Some(a), Some(b)) => a <= b,
            _ => true,
        };
        before_end(start, self.end_time) && before_end(self.start_time, end)
    }
}

/// One selection entry: a source identifier glob, publication version and time windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Glob pattern for source identifiers.
    pub sid_pattern: String,
    /// Selected publication version, `None` for any.
    pub pub_version: Option<u8>,
    /// Time windows, most recently added first. Empty means all times.
    pub time_windows: Vec<SelectTime>,
}

/// A successful match: the entry, and the time window that matched if the entry has any.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SelectionMatch<'a> {
    /// Matching entry.
    pub selection: &'a Selection,
    /// Matching time window, `None` if the entry has no time windows.
    pub time_window: Option<&'a SelectTime>,
}

/// A collection of data selections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selections {
    entries: Vec<Selection>,
}

impl Selections {
    /// Empty selections.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The selection entries, most recently created first.
    #[must_use]
    pub fn entries(&self) -> &[Selection] {
        &self.entries
    }

    /// True if there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add a selection.
    ///
    /// `None` times are open bounds and a `None` publication version matches any.
    /// Fails if both times are set and the start is later than the end.
    pub fn add(
        &mut self,
        sid_pattern: &str,
        start_time: Option<NsTime>,
        end_time: Option<NsTime>,
        pub_version: Option<u8>,
    ) -> Result<()> {
        if start_time.zip(end_time).is_some_and(|(s, e)| s > e) {
            return Err(Error::Selection("Selection start time is later than end time".into()));
        }
        let window = SelectTime { start_time, end_time };
        let pattern: String = sid_pattern.chars().take(99).collect();
        match self
            .entries
            .iter_mut()
            .find(|e| e.sid_pattern == pattern && e.pub_version == pub_version)
        {
            Some(e) => e.time_windows.insert(0, window),
            None => self.entries.insert(
                0,
                Selection { sid_pattern: pattern, pub_version, time_windows: vec![window] },
            ),
        }
        Ok(())
    }

    /// Add a selection from network, station, location and channel codes.
    /// A `None` code matches anything; location `"--"` is blank.
    /// A 3-character SEED channel is converted to extended form.
    #[allow(clippy::too_many_arguments)]
    pub fn add_comp(
        &mut self,
        network: Option<&str>,
        station: Option<&str>,
        location: Option<&str>,
        channel: Option<&str>,
        start_time: Option<NsTime>,
        end_time: Option<NsTime>,
        pub_version: Option<u8>,
    ) -> Result<()> {
        let net = network.unwrap_or("*");
        let sta = station.unwrap_or("*");
        let loc = match location {
            Some("--") => "",
            Some(l) => l,
            None => "*",
        };
        let chan = match channel {
            Some(c) if glob_match(c, "[?*a-zA-Z0-9][?*a-zA-Z0-9][?*a-zA-Z0-9]") => {
                seedchan_to_xchan(c).unwrap_or_else(|| c.to_string())
            }
            Some(c) => c.to_string(),
            None => "*".to_string(),
        };
        let pattern = nslc_to_sid_max(net, sta, loc, &chan, 99)?;
        self.add(&pattern, start_time, end_time, pub_version)
    }

    /// Find a selection matching a source identifier, time range and publication version.
    ///
    /// `None` time bounds are open; windows match when they intersect, inclusive.
    #[must_use]
    pub fn matches(
        &self,
        sid: &str,
        start_time: Option<NsTime>,
        end_time: Option<NsTime>,
        pub_version: u8,
    ) -> Option<SelectionMatch<'_>> {
        self.entries.iter().find_map(|sel| {
            if !glob_match(sid, &sel.sid_pattern)
                || sel.pub_version.is_some_and(|v| v != pub_version)
            {
                return None;
            }
            if sel.time_windows.is_empty() {
                return Some(SelectionMatch { selection: sel, time_window: None });
            }
            sel.time_windows
                .iter()
                .find(|w| w.intersects(start_time, end_time))
                .map(|w| SelectionMatch { selection: sel, time_window: Some(w) })
        })
    }

    /// Find a selection matching a record's source identifier, time span and publication
    /// version. If the record's end time cannot be calculated its span is open-ended.
    #[must_use]
    pub fn matches_record(&self, rec: &Record) -> Option<SelectionMatch<'_>> {
        self.matches(&rec.sid, Some(rec.start_time), rec.end_time().ok(), rec.pub_version)
    }

    /// Add selections from the text of a selection file.
    ///
    /// Lines are either `SourceID [Starttime [Endtime [Pubversion]]]` or
    /// `Network Station Location Channel [Pubversion [Starttime [Endtime]]]`;
    /// `#` starts a comment. `name` is used in messages. Returns the number of
    /// selections added.
    pub fn add_from_text(&mut self, text: &str, name: &str) -> Result<usize> {
        const DATE_GLOB: &str = "[0-9][0-9][0-9][0-9][-/,][0-9]*";
        let mut count = 0;
        for (idx, raw) in text.split('\n').enumerate() {
            let linecount = idx + 1;
            // Lines are limited to 199 characters; excess non-whitespace is an error
            let mut line = raw;
            let mut truncated = false;
            if line.len() > 199 {
                let mut cut = 199;
                while !line.is_char_boundary(cut) {
                    cut -= 1;
                }
                truncated = !line[cut..].trim().is_empty();
                line = &line[..cut];
            }
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if truncated {
                return Err(Error::Selection(format!(
                    "Data selection line {linecount} exceeds maximum length of 199 characters"
                )));
            }
            let fields: Vec<&str> = line.split_whitespace().take(8).collect();
            let nf = fields.len();
            let at = |i: usize| fields.get(i).copied();
            let isdate = |i: usize| at(i).is_some_and(|f| glob_match(f, DATE_GLOB));
            let (isstart2, isend3, isdate5, isstart6, isend7) =
                (isdate(1), isdate(2), isdate(4), isdate(5), isdate(6));

            let timefield = if isstart2 {
                at(1)
            } else if isstart6 {
                at(5)
            } else {
                None
            };
            let start_time = match timefield {
                Some(f) => Some(NsTime::parse(f).map_err(|_| {
                    Error::Selection(format!(
                        "Cannot convert data selection start time (line {linecount}): {f}"
                    ))
                })?),
                None => None,
            };
            let timefield = if isend3 {
                at(2)
            } else if isend7 {
                at(6)
            } else {
                None
            };
            let end_time = match timefield {
                Some(f) => Some(NsTime::parse(f).map_err(|_| {
                    Error::Selection(format!(
                        "Cannot convert data selection end time (line {linecount}): {f}"
                    ))
                })?),
                None => None,
            };

            // A publication version of 0 selects any version
            let parse_pubversion = |f: &str| -> Result<Option<u8>> {
                match f.parse::<i64>() {
                    Ok(v) if f.bytes().all(|b| b.is_ascii_digit()) && (0..=255).contains(&v) => {
                        Ok((v != 0).then_some(v as u8))
                    }
                    _ => Err(Error::Selection(format!(
                        "Cannot convert publication version (line {linecount}): {f}"
                    ))),
                }
            };

            if nf == 1
                || (nf == 2 && isstart2)
                || (nf == 3 && isstart2 && isend3)
                || (nf == 4 && isstart2 && isend3)
            {
                let pubversion = match at(3) {
                    Some(f) => parse_pubversion(f)?,
                    None => None,
                };
                self.add(fields[0], start_time, end_time, pubversion).map_err(|e| {
                    Error::Selection(format!(
                        "{name}: Error adding selection on line {linecount}: {e}"
                    ))
                })?;
                count += 1;
            } else if nf == 4 || nf == 5 || (nf == 6 && isstart6) || (nf == 7 && isstart6 && isend7)
            {
                if isdate5 {
                    return Err(Error::Selection(format!(
                        "Time value in publication version field (line {linecount}): {}",
                        fields[4]
                    )));
                }
                let pubversion = match at(4) {
                    Some(f) if f.bytes().all(|b| b.is_ascii_digit()) => parse_pubversion(f)?,
                    _ => None,
                };
                self.add_comp(
                    Some(fields[0]),
                    Some(fields[1]),
                    Some(fields[2]),
                    Some(fields[3]),
                    start_time,
                    end_time,
                    pubversion,
                )
                .map_err(|e| {
                    Error::Selection(format!(
                        "{name}: Error adding selection on line {linecount}: {e}"
                    ))
                })?;
                count += 1;
            } else {
                log::warn!("{name}: Skipping unrecognized data selection on line {linecount}");
            }
        }
        Ok(count)
    }

    /// Add selections from a file; returns the number added.
    pub fn read_file(&mut self, path: impl AsRef<std::path::Path>) -> Result<usize> {
        let path = path.as_ref();
        let text = std::fs::read(path)
            .map_err(|e| Error::io(e, format_args!("Cannot open file {}", path.display())))?;
        self.add_from_text(&String::from_utf8_lossy(&text), &path.display().to_string())
    }
}

/// Selections formatted for display, one line per entry followed by one line per window.
impl fmt::Display for Selections {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time = |t: Option<NsTime>, none: &str| match t {
            Some(t) => t.format(TimeFormat::IsoMonthDayZ, Subseconds::NanoMicroNone),
            None => none.to_string(),
        };
        for sel in &self.entries {
            writeln!(
                f,
                "Selection: {}  pubversion: {}",
                sel.sid_pattern,
                sel.pub_version.unwrap_or(0)
            )?;
            for w in &sel.time_windows {
                writeln!(
                    f,
                    "  {:>30}  {:>30}",
                    time(w.start_time, "No start time"),
                    time(w.end_time, "No end time")
                )?;
            }
        }
        Ok(())
    }
}

/// Match `class` (the text after `[`) against `c`; returns (matched, index after `]`)
/// or `None` for a malformed class.
fn match_class(p: &[u8], start: usize, c: u8) -> Option<(bool, usize)> {
    let mut i = start;
    let mut negate = false;
    let mut matched = false;
    if i < p.len() && (p[i] == b'^' || p[i] == b'!') {
        negate = true;
        i += 1;
    }
    if i < p.len() && p[i] == b']' {
        matched = c == b']';
        i += 1;
    }
    if i < p.len() && p[i] == b'-' {
        matched |= c == b'-';
        i += 1;
    }
    while i < p.len() && p[i] != b']' {
        let pc = p[i];
        if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' && pc <= p[i + 2] {
            matched |= c >= pc && c <= p[i + 2];
            i += 3;
        } else {
            matched |= c == pc;
            i += 1;
        }
    }
    if i >= p.len() {
        return None;
    }
    Some((if negate { !matched } else { matched }, i + 1))
}

/// Glob match of a string against a pattern with `*`, `?`, `[...]` and `\` escapes.
///
/// Matching is by byte: `?` and `[...]` match a single byte, so they suit ASCII
/// identifiers (as source identifiers are) rather than arbitrary Unicode.
#[must_use]
pub fn glob_match(string: &str, pattern: &str) -> bool {
    let (s, p) = (string.as_bytes(), pattern.as_bytes());
    let (mut si, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None; // (pattern index after '*', string index)

    loop {
        let mut ok = false;
        if pi == p.len() {
            if si == s.len() {
                return true;
            }
        } else {
            match p[pi] {
                b'*' => {
                    while pi < p.len() && p[pi] == b'*' {
                        pi += 1;
                    }
                    if pi == p.len() {
                        return true;
                    }
                    star = Some((pi, si));
                    continue;
                }
                b'?' => {
                    if si < s.len() {
                        si += 1;
                        pi += 1;
                        ok = true;
                    }
                }
                b'[' => {
                    if si < s.len()
                        && let Some((m, next)) = match_class(p, pi + 1, s[si])
                        && m
                    {
                        si += 1;
                        pi = next;
                        ok = true;
                    }
                }
                b'\\' if pi + 1 < p.len() => {
                    if si < s.len() && s[si] == p[pi + 1] {
                        si += 1;
                        pi += 2;
                        ok = true;
                    }
                }
                c => {
                    if si < s.len() && s[si] == c {
                        si += 1;
                        pi += 1;
                        ok = true;
                    }
                }
            }
        }
        if ok {
            continue;
        }
        // Backtrack: let the last '*' consume one more character
        match star {
            Some((sp, ss)) if ss < s.len() => {
                star = Some((sp, ss + 1));
                si = ss + 1;
                pi = sp;
            }
            _ => return false,
        }
    }
}

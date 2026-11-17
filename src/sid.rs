//! FDSN Source Identifier (SID) helpers.
//!
//! Source identifiers are defined at <https://docs.fdsn.org/projects/source-identifiers/>.

use crate::error::{Error, Result};

/// One more than the maximum length in bytes of a source identifier: longer identifiers
/// are rejected when parsing and building them.
pub const SIDLEN: usize = 64;

/// Network, station, location and channel codes parsed from a source identifier.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Nslc {
    /// Network code.
    pub network: String,
    /// Station code.
    pub station: String,
    /// Location code, possibly empty.
    pub location: String,
    /// Channel code; SEED form (`LHZ`) when mappable, otherwise extended (`B_S_P`).
    pub channel: String,
}

/// Convert a 3-character SEED channel to extended form `B_S_P`.
///
/// Spaces are dropped from the result. Returns `None` if `seedchan` is not 3 characters.
#[must_use]
pub fn seedchan_to_xchan(seedchan: &str) -> Option<String> {
    let c: Vec<char> = seedchan.chars().collect();
    if c.len() != 3 || c.contains(&'\0') {
        return None;
    }
    let x = format!("{}_{}_{}", c[0], c[1], c[2]);
    Some(x.chars().filter(|&ch| ch != ' ').collect())
}

/// Convert an extended channel `B_S_P` to a 3-character SEED channel.
///
/// Returns `None` if the pattern does not match.
#[must_use]
pub fn xchan_to_seedchan(xchan: &str) -> Option<String> {
    let c: Vec<char> = xchan.chars().collect();
    if c.len() == 5 && c[1] == '_' && c[3] == '_' {
        Some([c[0], c[2], c[4]].iter().collect())
    } else {
        None
    }
}

/// Parse network, station, location and channel from an FDSN source identifier.
///
/// Expects `FDSN:NET_STA_LOC_B_S_P`, optionally with additional namespace identifiers
/// (`FDSN:AGENCY:NET_STA_LOC_B_S_P`). The channel is converted to SEED form if possible.
pub fn sid_to_nslc(sid: &str) -> Result<Nslc> {
    if !sid.starts_with("FDSN:") {
        return Err(Error::InvalidSourceId(format!("Unrecognized identifier: {sid}")));
    }
    let id = &sid[sid.rfind(':').unwrap() + 1..];
    let sepcnt = id.matches('_').count();
    if sepcnt != 5 {
        return Err(Error::InvalidSourceId(format!(
            "Incorrect number of identifier delimiters ({sepcnt}): {id}"
        )));
    }
    let mut parts = id.splitn(4, '_');
    let network = parts.next().unwrap_or("").to_string();
    let station = parts.next().unwrap_or("").to_string();
    let location = parts.next().unwrap_or("").to_string();
    let xchan = parts.next().unwrap_or("");
    let channel = xchan_to_seedchan(xchan).unwrap_or_else(|| xchan.to_string());
    Ok(Nslc { network, station, location, channel })
}

/// Build an FDSN source identifier from codes.
///
/// A 3-character SEED channel is converted to extended form. Spaces are removed.
/// Fails if the identifier is longer than `SIDLEN - 1` bytes (see [`SIDLEN`]).
pub fn nslc_to_sid(network: &str, station: &str, location: &str, channel: &str) -> Result<String> {
    nslc_to_sid_max(network, station, location, channel, SIDLEN - 1)
}

/// Like [`nslc_to_sid`] with a maximum length in bytes.
pub(crate) fn nslc_to_sid_max(
    network: &str,
    station: &str,
    location: &str,
    channel: &str,
    max_len: usize,
) -> Result<String> {
    let chan = seedchan_to_xchan(channel).unwrap_or_else(|| channel.to_string());
    let sid: String = format!("FDSN:{network}_{station}_{location}_{chan}")
        .chars()
        .filter(|&c| c != ' ')
        .collect();
    if sid.len() > max_len {
        return Err(Error::InvalidSourceId(format!(
            "Source identifier is longer ({} bytes) than supported ({max_len})",
            sid.len()
        )));
    }
    Ok(sid)
}

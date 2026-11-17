//! Map between FDSN Source Identifiers and network, station, location and channel codes
//! (port of `lm_sids.c`).
//!
//! Usage: `cargo run --example sids -- FDSN:NET_STA_LOC_B_S_P` or
//! `cargo run --example sids -- NET STA LOC CHAN`

use libmseed::sid::{nslc_to_sid, sid_to_nslc};

fn map_sid(original: &str) -> libmseed::Result<()> {
    let n = sid_to_nslc(original)?;
    let sid = nslc_to_sid(&n.network, &n.station, &n.location, &n.channel)?;
    println!("Original SID: '{original}'");
    println!(
        "  network: '{}', station: '{}', location: '{}', channel: '{}'",
        n.network, n.station, n.location, n.channel
    );
    println!("  SID: '{sid}'");
    Ok(())
}

fn map_nslc(net: &str, sta: &str, loc: &str, chan: &str) -> libmseed::Result<()> {
    let sid = nslc_to_sid(net, sta, loc, chan)?;
    let n = sid_to_nslc(&sid)?;
    println!("Original network: '{net}', station: '{sta}', location: '{loc}', channel: '{chan}'");
    println!("  SID: '{sid}'");
    println!(
        "  network: '{}', station: '{}', location: '{}', channel: '{}'",
        n.network, n.station, n.location, n.channel
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.len() {
        1 => map_sid(&args[0]),
        4 => map_nslc(&args[0], &args[1], &args[2], &args[3]),
        _ => {
            println!("Usage: sids <SID> | <network> <station> <location> <channel>");
            println!("  <SID> is a FDSN SourceID, e.g. 'FDSN:NET_STA_LOC_C_H_N'");
            println!("  <network> <station> <location> <channel> are SEED codes");
            return;
        }
    };
    if let Err(e) = result {
        println!("Error: {e}");
        std::process::exit(1);
    }
}

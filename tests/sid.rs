//! Port of libmseed test/test-sid.c

use libmseed::sid::*;

#[test]
fn sid_to_nslc_cases() {
    let n = sid_to_nslc("FDSN:XX_TEST__L_H_Z").unwrap();
    assert_eq!(
        (n.network.as_str(), n.station.as_str(), n.location.as_str(), n.channel.as_str()),
        ("XX", "TEST", "", "LHZ")
    );

    let n = sid_to_nslc("FDSN:XX_TEST_00_BB_SS_ZZ").unwrap();
    assert_eq!(
        (n.network.as_str(), n.station.as_str(), n.location.as_str(), n.channel.as_str()),
        ("XX", "TEST", "00", "BB_SS_ZZ")
    );

    let n = sid_to_nslc("FDSN:EXTRANS:XX_TEST__BB_SS_ZZ").unwrap();
    assert_eq!(
        (n.network.as_str(), n.station.as_str(), n.location.as_str(), n.channel.as_str()),
        ("XX", "TEST", "", "BB_SS_ZZ")
    );

    assert!(sid_to_nslc("XX_TEST__BB_SS_ZZ").is_err());
    assert!(sid_to_nslc("MYDC:XX_TEST__BB_SS_ZZ").is_err());
    assert!(sid_to_nslc("FDSN:YY_STA").is_err());
}

#[test]
fn nslc_to_sid_cases() {
    assert_eq!(nslc_to_sid("XX", "TEST", "", "LHZ").unwrap(), "FDSN:XX_TEST__L_H_Z");
    assert_eq!(nslc_to_sid("XX", "TEST", "", "L_H_Z").unwrap(), "FDSN:XX_TEST__L_H_Z");
    assert_eq!(nslc_to_sid("XX", "TEST", "00", "BB_SS_ZZ").unwrap(), "FDSN:XX_TEST_00_BB_SS_ZZ");

    // SEED codes with spaces
    assert_eq!(nslc_to_sid(" XX", "TE ST ", "  ", " S ").unwrap(), "FDSN:XX_TEST___S_");

    // Too long for a source identifier
    assert!(nslc_to_sid(
        "NETWORK",
        "STATION",
        "LOCATION",
        "CHA_NN_EL_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    )
    .is_err());
}

#[test]
fn seedchan_to_xchan_cases() {
    assert_eq!(seedchan_to_xchan("LHZ").unwrap(), "L_H_Z");
    // Invalid spaces for band and orientation codes are skipped
    assert_eq!(seedchan_to_xchan(" H ").unwrap(), "_H_");
    assert!(seedchan_to_xchan("NOTAVALIDCHANNEL").is_none());
}

#[test]
fn xchan_to_seedchan_cases() {
    assert_eq!(xchan_to_seedchan("L_H_Z").unwrap(), "LHZ");
    // Unspecified band and subsource are legal Source ID but un-mappable to SEED
    assert!(xchan_to_seedchan("_H_").is_none());
    // Illegal spaces: behavior retained from C, not guaranteed
    assert_eq!(xchan_to_seedchan(" _H_ ").unwrap(), " H ");
    assert!(xchan_to_seedchan("BB_SS_SS").is_none());
}

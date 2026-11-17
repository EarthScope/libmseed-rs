//! Port of libmseed test/test-selection.c

use libmseed::NsTime;
use libmseed::selection::Selections;

fn t(s: &str) -> NsTime {
    NsTime::parse(s).unwrap()
}

#[test]
fn selection_match() {
    let starttime = t("2010-02-27T06:50:00.069539Z");
    let endtime = t("2010-02-27T07:55:51.069539Z");
    let mut sel = Selections::new();

    sel.add("FDSN:XX_*", None, None, None).unwrap();
    sel.add_comp(Some("YY"), Some("STA1"), Some(""), Some("B_H_Z"), None, None, None).unwrap();
    sel.add_comp(
        Some("YY"),
        Some("STA1"),
        Some(""),
        Some("LHZ"),
        Some(starttime),
        Some(endtime),
        Some(2),
    )
    .unwrap();
    sel.add("FDSN:ZZ_*_*_[BL]_H_[Z12]", None, None, None).unwrap();

    // Matches
    assert!(sel.matches("FDSN:XX_S2__L_H_Z", None, None, 1).is_some());

    let m = sel.matches("FDSN:YY_STA1__B_H_Z", Some(starttime), Some(endtime), 2).expect("match");
    assert!(m.time_window.is_some());

    let m = sel.matches("FDSN:YY_STA1__L_H_Z", Some(starttime), Some(endtime), 2).expect("match");
    assert!(m.time_window.is_some());

    assert!(sel.matches("FDSN:ZZ_STA1_LO_B_H_Z", Some(starttime), Some(endtime), 0).is_some());

    // Non matches
    assert!(sel.matches("FDSN:YY_STA2__B_H_Z", Some(starttime), Some(endtime), 0).is_none());
    assert!(
        sel.matches(
            "FDSN:YY_STA1__L_H_Z",
            Some(NsTime::from_nanos(0)),
            Some(NsTime::from_nanos(10)),
            0
        )
        .is_none()
    );
    assert!(sel.matches("FDSN:YY_STA1__L_H_Z", Some(starttime), Some(endtime), 3).is_none());
    assert!(sel.matches("FDSN:ZZ_STA1_LO_H_H_Z", Some(starttime), Some(endtime), 0).is_none());
}

#[test]
fn selection_openended() {
    let winstart = t("2010-01-01T00:00:00Z");
    let winend = t("2010-01-02T00:00:00Z");
    let before = t("2009-12-01T00:00:00Z");
    let after = t("2010-02-01T00:00:00Z");
    let sid = "FDSN:XX_TEST__B_H_Z";
    let mut sel = Selections::new();
    sel.add(sid, Some(winstart), Some(winend), None).unwrap();

    assert!(sel.matches(sid, Some(before), None, 0).is_some(), "open end, start before window end");
    assert!(sel.matches(sid, Some(winstart), None, 0).is_some(), "open end, start inside window");
    assert!(sel.matches(sid, Some(after), None, 0).is_none(), "open end, start after window end");
    assert!(sel.matches(sid, None, Some(after), 0).is_some(), "open start, end after window start");
    assert!(
        sel.matches(sid, None, Some(before), 0).is_none(),
        "open start, end before window start"
    );
    assert!(sel.matches(sid, None, None, 0).is_some(), "fully open query");
}

#[test]
fn selection_error() {
    let starttime = t("2010-02-27T06:50:00.069539Z");
    let endtime = t("2010-02-27T07:55:51.069539Z");
    let mut sel = Selections::new();

    // Inverted time window
    assert!(sel.add("FDSN:XX_*", Some(endtime), Some(starttime), None).is_err());
    // Open-ended windows are accepted on either side
    sel.add("FDSN:XX_*", None, Some(endtime), None).unwrap();
    sel.add("FDSN:XX_*", Some(starttime), None, None).unwrap();

    assert!(Selections::new().matches("FDSN:YY_STA1__L_H_Z", None, None, 1).is_none());
    assert!(sel.matches("FDSN:YY_STA1__L_H_Z", None, None, 1).is_none());
}

#[test]
fn selection_file_parsing() {
    let mut sel = Selections::new();
    let text = "# comment\n\
        FDSN:XX_TEST__B_H_Z 2010-01-01T00:00:00 2010-01-02T00:00:00 3\n\
        IU COLA 00 LHZ 2 2010-02-01 2010-03-01\n\
        FDSN:YY_*\n\
        \n";
    assert_eq!(sel.add_from_text(text, "test").unwrap(), 3);
    assert_eq!(sel.entries().len(), 3);
    let m = sel
        .matches("FDSN:IU_COLA_00_L_H_Z", Some(t("2010-02-15")), Some(t("2010-02-16")), 2)
        .unwrap();
    assert_eq!(m.selection.pub_version, Some(2));
    assert!(
        sel.matches("FDSN:IU_COLA_00_L_H_Z", Some(t("2010-04-15")), Some(t("2010-04-16")), 2)
            .is_none()
    );
    // A time value in the pubversion position is not parseable
    assert!(Selections::new().add_from_text("IU COLA 00 LHZ 2010-01-01\n", "bad").is_err());
}

#[test]
fn glob() {
    use libmseed::selection::glob_match as g;
    assert!(g("abc", "a*c"));
    assert!(g("abc", "*"));
    assert!(g("abc", "a?c"));
    assert!(!g("abc", "a?d"));
    assert!(g("FDSN:ZZ_STA_LO_B_H_1", "FDSN:ZZ_*_*_[BL]_H_[Z12]"));
    assert!(g("b", "[^a]"));
    assert!(!g("a", "[!a]"));
    assert!(g("-", "[a-]"));
    assert!(g("a*b", "a\\*b"));
    assert!(!g("axb", "a\\*b"));
    assert!(!g("abc", "[abc"));
}

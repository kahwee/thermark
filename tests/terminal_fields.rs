use thermark::{BleCandidate, InfoValue, RfidInfo};

#[test]
fn peer_metadata_is_escaped_only_at_human_output_boundaries() {
    let raw = "name\x1b]52;c;secret\x07\n\u{202e}bad";
    let candidate = BleCandidate::new("id", Some(raw.into()));
    let rfid = RfidInfo {
        tag_present: true,
        barcode: raw.into(),
        serial: raw.into(),
        ..Default::default()
    };
    let check = thermark::doctor::Check::pass("peer", raw);
    for shown in [
        candidate.to_string(),
        rfid.to_string(),
        InfoValue::Text(raw.into()).to_string(),
        check.to_string(),
    ] {
        assert!(!shown.contains('\x1b'));
        assert!(!shown.contains('\x07'));
        assert!(!shown.contains('\n'));
        assert!(!shown.contains('\u{202e}'));
    }
    assert_eq!(candidate.name.as_deref(), Some(raw));
    assert_eq!(rfid.barcode, raw);
    assert_eq!(check.detail, raw);
}

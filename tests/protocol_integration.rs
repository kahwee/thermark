//! Integration-style tests (no hardware required).

use thermark::errors::PrinterErrorCode;
use thermark::geometry::{LabelMm, PX_PER_MM};
use thermark::label::{make_qr_label, max_qr_side, render_qr_square, TextSide};
use thermark::packet::Packet;
use thermark::protocol::{self, Model};

#[test]
fn b1_print_start_roundtrip() {
    let p = protocol::print_start(Model::B1);
    assert_eq!(p.cmd, 0x01);
    assert_eq!(p.data.len(), 7);
    assert_eq!(&p.data[0..2], &1u16.to_be_bytes());
    let enc = p.encode();
    let dec = Packet::decode(&enc).unwrap();
    assert_eq!(dec, p);
}

#[test]
fn b1_page_size_matches_50x30() {
    let lp = LabelMm::parse("50x30")
        .unwrap()
        .to_pixels(Model::B1.max_width_px());
    let p = protocol::set_page_size_b1(lp.height_px as u16, lp.width_px as u16, 1);
    assert_eq!(p.cmd, 0x13);
    assert_eq!(p.data.len(), 6);
    let rows = u16::from_be_bytes([p.data[0], p.data[1]]);
    let cols = u16::from_be_bytes([p.data[2], p.data[3]]);
    let copies = u16::from_be_bytes([p.data[4], p.data[5]]);
    assert_eq!(rows, 240);
    assert_eq!(cols, 384);
    assert_eq!(copies, 1);
    Packet::decode(&p.encode()).unwrap();
}

#[test]
fn density_and_label_type_packets() {
    let d = protocol::set_density(4);
    assert_eq!(d.encode()[2], 0x21);
    let t = protocol::set_label_type(1);
    assert_eq!(t.encode()[2], 0x23);
}

#[test]
fn error_codes_cover_and_paper() {
    assert_eq!(PrinterErrorCode::from_u8(0x01), PrinterErrorCode::CoverOpen);
    assert_eq!(PrinterErrorCode::from_u8(0x02), PrinterErrorCode::LackPaper);
    let s = format!("{}", PrinterErrorCode::from_u8(0x01));
    assert!(s.contains("Cover") || s.contains("open") || s.contains("0x01"));
}

#[test]
fn model_max_widths() {
    assert_eq!(Model::B1.max_width_px(), 384);
    assert_eq!(Model::D11.max_width_px(), 96);
    assert_eq!(Model::parse("b1"), Some(Model::B1));
    assert_eq!(Model::parse("B21"), Some(Model::B21));
    assert!(Model::parse("nope").is_none());
}

#[test]
fn qr_label_exact_canvas_and_square_qr() {
    let lp = LabelMm::parse("50x30").unwrap().to_pixels(384);
    let img =
        make_qr_label("https://www.youtube.com", "ABC\n123", lp, TextSide::Right).expect("layout");
    assert_eq!(img.dimensions(), (384, 240));

    let side = max_qr_side(lp);
    assert!(
        side >= 200,
        "QR should use most of label height, got {side}"
    );
    let qr = render_qr_square("https://example.com", side).unwrap();
    assert_eq!(qr.dimensions(), (side, side));
}

#[test]
fn px_per_mm_constant() {
    assert!((PX_PER_MM - 8.0).abs() < f64::EPSILON);
    // 50mm * 8 = 400, clamped to 384
    assert_eq!(LabelMm::new(50.0, 30.0).to_pixels(384).width_px, 384);
}

#[test]
fn packet_buffer_resync() {
    let good = Packet::new(0x40, vec![0x0b]).encode();
    let mut buf = vec![0x00, 0xFF];
    buf.extend_from_slice(&good);
    let pkts = Packet::drain_buffer(&mut buf);
    assert_eq!(pkts.len(), 1);
    assert_eq!(pkts[0].cmd, 0x40);
}

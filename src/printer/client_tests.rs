use super::*;
use crate::errors::PrinterFault;
use crate::image_encode::Raster;
use crate::mock::MockTransport;
use crate::packet::Packet;
use crate::print_task::PrintTask;
use crate::printer::{OnTimeout, PrintOptions};
use crate::protocol::{self, Cmd, InfoKey};
use crate::types::Density;
use image::{GrayImage, Luma};

fn client_b1() -> PrinterClient<MockTransport> {
    PrinterClient::new(MockTransport::new(), Model::B1).with_pacing(Pacing::INSTANT)
}

#[tokio::test]
async fn b1_print_gray_sends_expected_command_order() {
    let mut c = client_b1();
    assert_eq!(c.print_task(), PrintTask::B1);

    let gray = GrayImage::from_pixel(16, 2, Luma([0]));
    c.print_gray_image(&gray, Density::DARK)
        .await
        .expect("print");

    let cmds = c.transport().tx_cmds();
    assert!(cmds.contains(&0x21), "density: {cmds:?}");
    assert!(cmds.contains(&0x23), "label type: {cmds:?}");
    assert!(cmds.contains(&0x01), "print start: {cmds:?}");
    assert!(cmds.contains(&0x03), "page start: {cmds:?}");
    assert!(cmds.contains(&0x13), "page size: {cmds:?}");
    assert!(
        cmds.iter().any(|c| *c == 0x85 || *c == 0x84),
        "row data: {cmds:?}"
    );
    assert!(cmds.contains(&0xe3), "page end: {cmds:?}");
    assert!(cmds.contains(&0xa3), "status: {cmds:?}");
    assert!(cmds.contains(&0xf3), "print end: {cmds:?}");

    let ps = c.transport().first_tx(0x13).expect("page size pkt");
    assert_eq!(ps.data.len(), 6);
    assert_eq!(u16::from_be_bytes([ps.data[0], ps.data[1]]), 2);
    assert_eq!(u16::from_be_bytes([ps.data[2], ps.data[3]]), 16);

    let st = c.transport().first_tx(0x01).expect("start");
    assert_eq!(st.data.len(), 7);
}

#[tokio::test]
async fn transceive_decodes_a_header_split_across_transport_reads() {
    let reply = Packet::new(0x31, vec![0x01]).encode().unwrap();
    let mut mock = MockTransport::new();
    mock.auto_reply(false);
    mock.push_rx_raw(reply[..1].to_vec());
    mock.push_rx_raw(reply[1..].to_vec());
    let mut client = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let packet = client
        .transceive_with(
            protocol::set_density(3),
            0x31,
            2,
            Duration::from_millis(1),
            OnTimeout::Resend,
        )
        .await
        .unwrap();
    assert_eq!(packet.data, vec![0x01]);
}

#[tokio::test]
async fn print_start_error_lack_paper() {
    let mut mock = MockTransport::new();
    mock.fail_cmd(0x01, 0x02);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    match err {
        Error::Printer(PrinterFault::NO_PAPER) => {}
        other => panic!("expected LackPaper, got {other:?}"),
    }
}

#[tokio::test]
async fn print_start_error_cover_open() {
    let mut mock = MockTransport::new();
    mock.fail_cmd(0x01, 0x01);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    match err {
        Error::Printer(PrinterFault::COVER_OPEN) => {}
        other => panic!("expected CoverOpen, got {other:?}"),
    }
}

#[tokio::test]
async fn simple_task_uses_short_print_start() {
    let mut c = PrinterClient::new(MockTransport::new(), Model::B1)
        .with_print_task(PrintTask::Simple)
        .with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    c.print_gray_image(&gray, Density::NORMAL).await.unwrap();
    let st = c.transport().first_tx(0x01).unwrap();
    assert_eq!(st.data, vec![0x01]);
    let ps = c.transport().first_tx(0x13).unwrap();
    assert_eq!(ps.data.len(), 4);
}

#[tokio::test]
async fn fetch_summary_reads_info_keys() {
    let mut c = client_b1();
    let s = c.fetch_summary().await.unwrap();
    assert!(s.serial.is_some());
    assert!(s.heartbeat.is_some());
    let cmds = c.transport().tx_cmds();
    assert!(cmds.contains(&0x40));
    assert!(cmds.contains(&0xdc));
}

#[tokio::test]
async fn fetch_summary_does_not_hide_total_transport_failure() {
    let mut mock = MockTransport::new();
    mock.fail_receives("link down");
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let err = c.fetch_summary().await.unwrap_err();
    assert!(matches!(err, Error::Transport(message) if message.contains("link down")));
}

#[tokio::test]
async fn rejects_zero_retry_budget() {
    let mut c = client_b1();
    let err = c
        .transceive(
            protocol::info(InfoKey::Battery),
            (Cmd::PrinterInfo as u8).wrapping_add(InfoKey::Battery as u8),
            0,
            Duration::ZERO,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidRetryBudget));
}

#[tokio::test]
async fn density_out_of_range_errors() {
    assert!(Density::new(0).is_err());
    assert!(Density::new(6).is_err());
    let mut c = client_b1();
    assert!(c.set_density(Density::NORMAL).await.is_ok());
}

#[tokio::test]
async fn print_not_confirmed_when_end_print_muted() {
    let mut mock = MockTransport::new();
    mock.mute_cmd(0xf3); // no PrintEnd reply
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::PrintNotConfirmed),
        "expected PrintNotConfirmed, got {err:?}"
    );
}

#[tokio::test]
async fn density_nack_is_hard_error() {
    let mut mock = MockTransport::new();
    mock.reject_cmd(0x21);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    match err {
        Error::CommandRejected { step, cmd } => {
            assert_eq!(step, "set_density");
            assert_eq!(cmd, 0x21);
        }
        other => panic!("expected CommandRejected, got {other:?}"),
    }
    // Must not have started streaming rows after density NACK.
    let cmds = c.transport().tx_cmds();
    assert!(!cmds.iter().any(|c| *c == 0x85 || *c == 0x84));
}

#[tokio::test]
async fn start_print_nack_is_hard_error() {
    let mut mock = MockTransport::new();
    mock.reject_cmd(0x01);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::CommandRejected {
                step: "start_print",
                cmd: 0x01
            }
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn preflight_blocks_open_cover() {
    let mut mock = MockTransport::new();
    mock.heartbeat_not_ready_cover_open();
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let err = c.preflight_ready().await.unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::COVER_OPEN)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn low_battery_warns_but_does_not_block() {
    // Level 1 is "low": dense pages may truncate, but ordinary labels
    // usually still print, so this must stay a warning.
    let mut mock = MockTransport::new();
    let mut d = [0u8; 13];
    d[9] = 0; // cover closed
    d[10] = LOW_BATTERY_LEVEL; // battery low
    d[11] = 0; // paper present
    d[12] = 1;
    mock.set_heartbeat(d);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    assert!(c.preflight_ready().await.is_ok());
}

#[tokio::test]
async fn empty_battery_still_blocks() {
    let mut mock = MockTransport::new();
    let mut d = [0u8; 13];
    d[9] = 0;
    d[10] = 0; // empty
    d[11] = 0;
    d[12] = 1;
    mock.set_heartbeat(d);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    assert!(c.preflight_ready().await.is_err());
}

#[tokio::test]
async fn preflight_blocks_no_paper() {
    let mut mock = MockTransport::new();
    mock.heartbeat_not_ready_no_paper();
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let err = c.preflight_ready().await.unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::NO_PAPER)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn print_image_file_opts_aborts_preflight() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dot.png");
    GrayImage::from_pixel(8, 8, Luma([0])).save(&path).unwrap();

    let mut mock = MockTransport::new();
    mock.heartbeat_not_ready_no_paper();
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let err = c
        .print_image_file_opts(
            &path,
            PrintOptions {
                density: Density::NORMAL,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::NO_PAPER)),
        "got {err:?}"
    );
    // Must not have entered the print sequence.
    let cmds = c.transport().tx_cmds();
    assert!(
        !cmds.contains(&0x01),
        "print start should not run: {cmds:?}"
    );
}

#[tokio::test]
async fn lost_write_is_recovered_by_resending_a_read() {
    // BLE writes are unacknowledged, so a dropped request can only be
    // recovered by sending it again — waiting longer never helps.
    let mut mock = MockTransport::new();
    mock.drop_first_writes(0x40, 2);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    let info = c
        .get_info(InfoKey::DeviceSerial)
        .await
        .expect("resend should recover the lost writes");
    assert_eq!(info.to_string(), "TESTMOCK01");

    let sends = c
        .transport()
        .tx_cmds()
        .iter()
        .filter(|c| **c == 0x40)
        .count();
    assert_eq!(sends, 3, "two dropped writes plus the one that landed");
}

#[tokio::test]
async fn state_advancing_commands_are_never_resent() {
    // Resending PrintStart after a lost *reply* would start a second job,
    // so it must go out exactly once no matter how long the reply takes.
    let mut mock = MockTransport::new();
    mock.mute_cmd(0x01);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    let err = c.start_print().await.unwrap_err();
    assert!(matches!(err, Error::Timeout { .. }), "got {err:?}");

    let sends = c
        .transport()
        .tx_cmds()
        .iter()
        .filter(|c| **c == 0x01)
        .count();
    assert_eq!(sends, 1, "PrintStart must not be retransmitted");
}

#[tokio::test]
async fn idempotent_settings_are_resent() {
    // SetDensity twice equals SetDensity once, so recovery is safe.
    let mut mock = MockTransport::new();
    mock.drop_first_writes(0x21, 1);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    assert!(c.set_density(Density::DARK).await.unwrap());
    let sends = c
        .transport()
        .tx_cmds()
        .iter()
        .filter(|c| **c == 0x21)
        .count();
    assert_eq!(sends, 2);
}

#[tokio::test]
async fn mid_job_printer_error_surfaces_instead_of_print_not_confirmed() {
    // The printer reports "out of paper" via 0xDB on the status poll. That
    // result used to be dropped with `let _ =`, so the user got the useless
    // PrintNotConfirmed after 50 pointless end_print retries.
    let mut mock = MockTransport::new();
    mock.fail_cmd(0xa3, 0x02);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::NO_PAPER)),
        "expected the printer's own reason, got {err:?}"
    );
    // And it stopped there rather than pressing on to PrintEnd.
    assert!(!c.transport().tx_cmds().contains(&0xf3));
}

#[tokio::test]
async fn fault_in_the_status_payload_aborts_the_job() {
    // A fault reported *inside* a successful 0xb3 reply, not as a 0xDB
    // error packet. The framing layer sees a normal response, so this is
    // only catchable by reading the payload — which thermark used to throw
    // away entirely.
    let mut mock = MockTransport::new();
    // page 1, imaged 73%, fed 0%, fault 0x03 = LowBattery.
    mock.set_print_status(vec![0x00, 0x01, 73, 0x00, 0, 0, 0x03, 0, 0, 0]);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    let err = c
        .print_gray_image(&gray, Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::LOW_BATTERY)),
        "expected the fault named in the status payload, got {err:?}"
    );
}

#[tokio::test]
async fn a_page_that_stalls_without_a_fault_code_still_completes() {
    // Progress stuck below 100 with no fault byte. There is nothing to
    // raise — `end_print` is the authority on completion — so the job must
    // finish rather than invent an error from the progress numbers.
    let mut mock = MockTransport::new();
    mock.set_print_status(vec![0x00, 0x01, 73, 0x00]);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    c.print_gray_image(&gray, Density::NORMAL)
        .await
        .expect("incomplete progress is a warning, not a failure");
}

#[tokio::test]
async fn a_complete_page_stops_polling_early() {
    // The default mock reports 100/100 on the first poll. Continuing to
    // poll after that is pure latency on every single print.
    let mut c = PrinterClient::new(MockTransport::new(), Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    c.print_gray_image(&gray, Density::NORMAL).await.unwrap();
    let polls = c
        .transport()
        .tx_cmds()
        .iter()
        .filter(|&&c| c == 0xa3)
        .count();
    assert_eq!(polls, 1, "should stop at the first complete-page report");
}

#[tokio::test]
async fn missing_status_reply_still_completes() {
    // A silent status poll is normal on some firmware — it must not abort.
    let mut mock = MockTransport::new();
    mock.mute_cmd(0xa3);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    c.print_gray_image(&gray, Density::NORMAL)
        .await
        .expect("print should finish without a status reply");
}

#[tokio::test]
async fn image_too_large_for_u16_page_size() {
    let mut c = client_b1();
    // Construct rows for absurd height via print_rows directly
    let err = c
        .print_raster(
            Raster::from_parts_unchecked(8, u32::from(u16::MAX) + 1, vec![]),
            Density::NORMAL,
        )
        .await
        .unwrap_err();
    match err {
        Error::ImageTooLarge { height, .. } => {
            assert!(height > u32::from(u16::MAX));
        }
        other => panic!("expected ImageTooLarge, got {other:?}"),
    }
}

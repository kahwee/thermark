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

#[tokio::test(start_paused = true)]
async fn write_failure_is_never_retried_even_for_idempotent_commands() {
    struct FailedWrite {
        sends: usize,
    }
    impl Transport for FailedWrite {
        async fn send_raw(&mut self, _data: &[u8]) -> crate::Result<()> {
            self.sends += 1;
            Err(Error::transport(
                "write timed out; bytes may have been sent",
            ))
        }
        async fn recv_raw(&mut self, _wait: Duration) -> crate::Result<Vec<u8>> {
            panic!("must stop after a write failure")
        }
    }
    let mut client = PrinterClient::new(FailedWrite { sends: 0 }, Model::B1);
    assert!(matches!(
        client.set_density(Density::NORMAL).await,
        Err(Error::Transport(_))
    ));
    assert_eq!(client.transport().sends, 1);
}

#[tokio::test(start_paused = true)]
async fn coalesced_fault_takes_priority_over_ack_in_either_order() {
    let ack = Packet::new(0x31, vec![0x01]).encode().unwrap();
    let fault = Packet::new(0xdb, vec![PrinterFault::COVER_OPEN.code()])
        .encode()
        .unwrap();
    for batch in [[ack.clone(), fault.clone()], [fault.clone(), ack.clone()]] {
        let mut mock = MockTransport::new();
        mock.auto_reply(false);
        mock.push_rx_raw(batch.concat());
        let mut client = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let error = client.set_density(Density::NORMAL).await.unwrap_err();
        assert!(matches!(error, Error::Printer(PrinterFault::COVER_OPEN)));
        assert_eq!(client.transport().tx_cmds(), [Cmd::SetDensity as u8]);
    }
}

#[tokio::test(start_paused = true)]
async fn fallback_heartbeat_preserves_fault_and_blocks_printing() {
    // Exhaust the standard heartbeat attempts, then return an alternate
    // heartbeat and a fault together in one transport read.
    for with_heartbeat in [false, true] {
        let mut mock = MockTransport::new();
        mock.mute_cmd(Cmd::Heartbeat as u8);
        for _ in 0..8 {
            mock.push_rx_raw(Vec::new());
        }
        let mut batch = Vec::new();
        if with_heartbeat {
            let mut ready = vec![0; 13];
            ready[10] = 3;
            ready[12] = 1;
            batch.extend(Packet::new(0xde, ready).encode().unwrap());
        }
        batch.extend(
            Packet::new(0xdb, vec![PrinterFault::NO_PAPER.code()])
                .encode()
                .unwrap(),
        );
        mock.push_rx_raw(batch);
        let mut client = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let error = client
            .print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Printer(PrinterFault::NO_PAPER)));
        assert_eq!(client.transport().tx_cmds(), vec![Cmd::Heartbeat as u8; 9]);
    }
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn transceive_decodes_one_byte_reads_without_resending() {
    for policy in [OnTimeout::Resend, OnTimeout::WaitOnly] {
        let mut mock = MockTransport::new();
        mock.auto_reply(false);
        for byte in Packet::new(0x31, vec![0x01]).encode().unwrap() {
            mock.push_rx_raw(vec![byte]);
        }
        let mut client = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
        let packet = client
            .transceive_with(
                protocol::set_density(3),
                0x31,
                1,
                Duration::from_millis(50),
                policy,
            )
            .await
            .unwrap();
        assert_eq!(packet.data, vec![0x01]);
        assert_eq!(client.transport().tx_cmds(), vec![0x21]);
    }
}

#[test]
fn try_new_handles_models_without_default_tasks() {
    let client = PrinterClient::try_new(MockTransport::new(), Model::B1).unwrap();
    assert_eq!(client.print_task(), PrintTask::B1);
    let Err(error) = PrinterClient::try_new(MockTransport::new(), Model::B18) else {
        panic!("B18 has no default task");
    };
    assert!(error.to_string().contains("new_with_task"));
    let client = PrinterClient::new_with_task(MockTransport::new(), Model::B18, PrintTask::B1);
    assert_eq!(client.print_task(), PrintTask::B1);
}

#[tokio::test(start_paused = true)]
async fn unrelated_packets_do_not_extend_deadlines() {
    struct ChattyTransport {
        sends: Vec<tokio::time::Instant>,
    }
    impl Transport for ChattyTransport {
        async fn send_raw(&mut self, _data: &[u8]) -> crate::Result<()> {
            self.sends.push(tokio::time::Instant::now());
            Ok(())
        }
        async fn recv_raw(&mut self, _wait: Duration) -> crate::Result<Vec<u8>> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(Packet::new(0x99, vec![1]).encode()?)
        }
    }
    let start = tokio::time::Instant::now();
    let mut client = PrinterClient::new(ChattyTransport { sends: Vec::new() }, Model::B1);
    let error = client
        .transceive_with(
            protocol::set_density(3),
            0x31,
            2,
            Duration::from_millis(50),
            OnTimeout::Resend,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }));
    assert_eq!(
        client.transport().sends,
        vec![start, start + Duration::from_millis(50)]
    );
    assert_eq!(
        tokio::time::Instant::now() - start,
        Duration::from_millis(100)
    );
}

#[tokio::test(start_paused = true)]
async fn partial_reply_can_finish_after_a_wait_only_deadline() {
    let reply = Packet::new(0x31, vec![1]).encode().unwrap();
    let mut mock = MockTransport::new();
    mock.auto_reply(false);
    mock.push_rx_raw(reply[..3].to_vec());
    mock.push_rx_raw(Vec::new());
    mock.push_rx_raw(reply[3..].to_vec());
    let mut client = PrinterClient::new(mock, Model::B1);
    let packet = client
        .transceive_with(
            protocol::set_density(3),
            0x31,
            2,
            Duration::from_millis(50),
            OnTimeout::WaitOnly,
        )
        .await
        .unwrap();
    assert_eq!(packet.data, vec![1]);
    assert_eq!(client.transport().tx_cmds(), vec![0x21]);
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn d110_task_uses_short_print_start() {
    let mut c = PrinterClient::new(MockTransport::new(), Model::B1)
        .with_print_task(PrintTask::D110)
        .with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    c.print_gray_image(&gray, Density::NORMAL).await.unwrap();
    let st = c.transport().first_tx(0x01).unwrap();
    assert_eq!(st.data, vec![0x01]);
    let ps = c.transport().first_tx(0x13).unwrap();
    assert_eq!(ps.data.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn d110_sequence_orders_clear_and_quantity_inside_the_job() {
    let mut c = PrinterClient::new(MockTransport::new(), Model::D110).with_pacing(Pacing::INSTANT);
    c.print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap();
    let cmds = c.transport().tx_cmds();
    let pos = |cmd| cmds.iter().position(|value| *value == cmd).unwrap();
    assert!(pos(0x01) < pos(0x20));
    assert!(pos(0x20) < pos(0x03));
    assert!(pos(0x13) < pos(0x15));
    assert_eq!(c.transport().first_tx(0x13).unwrap().data.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn d11v1_uses_height_only_page_size_and_page_index_completion() {
    let mut c = PrinterClient::new(MockTransport::new(), Model::D11).with_pacing(Pacing::INSTANT);
    c.print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap();
    assert_eq!(c.transport().first_tx(0x13).unwrap().data.len(), 2);
    assert!(c.transport().tx_cmds().contains(&0x15));
    assert!(!c.transport().tx_cmds().contains(&0xa3));
}

#[tokio::test(start_paused = true)]
async fn d110mv4_omits_page_start_and_uses_thirteen_byte_page_size() {
    let mut c = PrinterClient::new(MockTransport::new(), Model::B1Pro).with_pacing(Pacing::INSTANT);
    c.print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap();
    let start = c.transport().first_tx(0x01).unwrap();
    assert_eq!(start.data.len(), 9);
    assert_eq!(start.data[7], 1); // clarity/speed mode
    assert_eq!(c.transport().first_tx(0x13).unwrap().data.len(), 13);
    let commands = c.transport().tx_cmds();
    assert!(!commands.contains(&0x03));
    let position = |cmd| commands.iter().position(|value| *value == cmd).unwrap();
    assert!(position(0x01) < position(0xa3));
    assert!(position(0xa3) < position(0x13));
    assert_eq!(commands.last(), Some(&0xdc));
}

#[tokio::test(start_paused = true)]
async fn detected_identity_replaces_the_provisional_profile() {
    let mut mock = MockTransport::new();
    mock.set_model_id(4097);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let identity = c.identify().await.unwrap();
    let profile = c.apply_identity(&identity, true).unwrap();
    assert_eq!(profile.model, Model::B1Pro);
    assert_eq!(c.model(), Model::B1Pro);
    assert_eq!(c.print_task(), PrintTask::D110MV4);
    assert_eq!(identity.protocol_version, Some(5));
    assert_eq!(c.transport().tx_cmds()[0], 0xc1);
}

#[tokio::test(start_paused = true)]
async fn profile_identity_skips_presentation_metadata() {
    let mut c = client_b1();
    let identity = c.identify_profile().await.unwrap();

    assert_eq!(identity.model_id, 4096);
    assert_eq!(identity.protocol_version, Some(5));
    assert_eq!(identity.firmware, None);
    assert_eq!(identity.hardware, None);

    let info_keys: Vec<u8> = c
        .transport()
        .tx_packets
        .iter()
        .filter(|packet| packet.cmd == Cmd::PrinterInfo as u8)
        .filter_map(|packet| packet.data.first().copied())
        .collect();
    assert_eq!(info_keys, vec![InfoKey::DeviceType as u8]);
}

#[tokio::test(start_paused = true)]
async fn full_identity_still_reads_version_metadata() {
    let mut c = client_b1();
    let identity = c.identify().await.unwrap();

    assert!(identity.firmware.is_some());
    assert!(identity.hardware.is_some());
    let info_keys: Vec<u8> = c
        .transport()
        .tx_packets
        .iter()
        .filter(|packet| packet.cmd == Cmd::PrinterInfo as u8)
        .filter_map(|packet| packet.data.first().copied())
        .collect();
    assert_eq!(
        info_keys,
        vec![
            InfoKey::DeviceType as u8,
            InfoKey::SoftVersion as u8,
            InfoKey::HardVersion as u8,
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn fetch_summary_reads_info_keys() {
    let mut c = client_b1();
    let s = c.fetch_summary().await.unwrap();
    assert!(s.serial.is_some());
    assert!(s.heartbeat.is_some());
    let cmds = c.transport().tx_cmds();
    assert!(cmds.contains(&0x40));
    assert!(cmds.contains(&0xdc));
}

#[tokio::test(start_paused = true)]
async fn fetch_summary_does_not_hide_total_transport_failure() {
    let mut mock = MockTransport::new();
    mock.fail_receives("link down");
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let err = c.fetch_summary().await.unwrap_err();
    assert!(matches!(err, Error::Transport(message) if message.contains("link down")));
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn density_out_of_range_errors() {
    assert!(Density::new(0).is_err());
    assert!(Density::new(6).is_err());
    let mut c = client_b1();
    assert!(c.set_density(Density::NORMAL).await.is_ok());
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn preflight_preserves_fault_reported_by_heartbeat() {
    let mut mock = MockTransport::new();
    mock.fail_cmd(Cmd::Heartbeat as u8, PrinterFault::COVER_OPEN.code());
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    let err = c
        .print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::COVER_OPEN)),
        "got {err:?}"
    );
    assert_eq!(
        c.transport()
            .tx_cmds()
            .iter()
            .filter(|&&cmd| cmd == Cmd::Heartbeat as u8)
            .count(),
        1,
        "a printer fault must not trigger a fallback heartbeat"
    );
    assert!(!c.transport().tx_cmds().contains(&(Cmd::PrintStart as u8)));
}

#[tokio::test(start_paused = true)]
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
        cmds.contains(&(Cmd::Heartbeat as u8)),
        "heartbeat preflight must remain on the print path: {cmds:?}"
    );
    assert!(
        !cmds.contains(&(Cmd::RfidInfo as u8)),
        "ordinary printing must not query RFID: {cmds:?}"
    );
    assert!(
        !cmds.contains(&0x01),
        "print start should not run: {cmds:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn print_gray_image_aborts_preflight_before_starting_a_job() {
    let mut mock = MockTransport::new();
    mock.heartbeat_not_ready_no_paper();
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    let err = c
        .print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Printer(PrinterFault::NO_PAPER)),
        "got {err:?}"
    );

    let cmds = c.transport().tx_cmds();
    assert!(
        cmds.contains(&(Cmd::Heartbeat as u8)),
        "heartbeat preflight should run: {cmds:?}"
    );
    assert!(
        !cmds.contains(&(Cmd::PrintStart as u8)),
        "print start should not run: {cmds:?}"
    );
    let row_commands = [
        Cmd::PrintBitmapRowIndexed as u8,
        Cmd::PrintEmptyRow as u8,
        Cmd::PrintBitmapRow as u8,
    ];
    assert!(
        !cmds.iter().any(|cmd| row_commands.contains(cmd)),
        "row data should not be sent: {cmds:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn print_gray_image_continues_when_heartbeat_is_unavailable() {
    let mut mock = MockTransport::new();
    mock.mute_cmd(Cmd::Heartbeat as u8);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    c.print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap();

    let cmds = c.transport().tx_cmds();
    assert!(cmds.contains(&(Cmd::Heartbeat as u8)));
    assert!(cmds.contains(&(Cmd::PrintStart as u8)));
}

#[tokio::test(start_paused = true)]
async fn print_gray_image_stops_when_preflight_loses_transport() {
    let mut mock = MockTransport::new();
    mock.fail_receives("link down before printing");
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    let err = c
        .print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Transport(_)), "got {err:?}");
    assert!(!c.transport().tx_cmds().contains(&(Cmd::PrintStart as u8)));
}

#[tokio::test(start_paused = true)]
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

    let expected = protocol::info(InfoKey::DeviceSerial).encode().unwrap();
    let frames: Vec<&[u8]> = c
        .transport()
        .tx
        .iter()
        .filter(|frame| Packet::decode(frame).is_ok_and(|packet| packet.cmd == 0x40))
        .map(Vec::as_slice)
        .collect();
    assert_eq!(frames, vec![expected.as_slice(); 3]);
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn a_page_that_stalls_without_a_fault_code_is_not_confirmed() {
    let mut mock = MockTransport::new();
    mock.set_print_status(vec![0x00, 0x01, 73, 0x00]);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    assert!(matches!(
        c.print_gray_image(&gray, Density::NORMAL).await,
        Err(Error::PrintNotConfirmed)
    ));
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn missing_status_reply_is_not_confirmed() {
    let mut mock = MockTransport::new();
    mock.mute_cmd(0xa3);
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);
    let gray = GrayImage::from_pixel(8, 1, Luma([0]));
    assert!(matches!(
        c.print_gray_image(&gray, Density::NORMAL).await,
        Err(Error::PrintNotConfirmed)
    ));
}

#[tokio::test(start_paused = true)]
async fn transport_failure_during_status_poll_is_reported_immediately() {
    let mut mock = MockTransport::new();
    mock.fail_receives_after_cmd(0xa3, "link down during status poll");
    let mut c = PrinterClient::new(mock, Model::B1).with_pacing(Pacing::INSTANT);

    let err = c
        .print_gray_image(&GrayImage::from_pixel(8, 1, Luma([0])), Density::NORMAL)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Transport(ref message) if message == "link down during status poll"),
        "got {err:?}"
    );
    assert_eq!(
        c.transport()
            .tx_cmds()
            .iter()
            .filter(|&&cmd| cmd == 0xa3)
            .count(),
        1,
        "a broken link cannot recover through more status polls"
    );
    assert!(!c.transport().tx_cmds().contains(&0xf3));
}

#[tokio::test(start_paused = true)]
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

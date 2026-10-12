use thermark::testing::{FaultTransport, ReplayTransport};
use thermark::{Density, MockTransport, Model, PrinterClient, Transport};

#[tokio::test(start_paused = true)]
async fn synthetic_fragmented_density_reply_is_not_resent() {
    let replay = ReplayTransport::from_json(include_bytes!(
        "../fixtures/protocol/density-fragmented.json"
    ))
    .unwrap();
    let mut client = PrinterClient::new(replay, Model::B1);
    assert!(client.raw().set_density(Density::NORMAL).await.unwrap());
    assert_eq!(client.transport().writes, 1);
    assert!(client.transport().is_complete());
    client.close().await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn partial_write_failure_never_replays_a_state_advancing_command() {
    let mut transport = FaultTransport::new(MockTransport::new());
    transport.fail_command = Some(0x01);
    transport.accepted_prefix = 4;
    let mut client =
        PrinterClient::new(transport, Model::B1).with_pacing(thermark::Pacing::INSTANT);
    let image = image::GrayImage::from_pixel(32, 16, image::Luma([0]));
    assert!(
        client
            .print_gray_image(&image, Density::NORMAL)
            .await
            .is_err()
    );
    let starts: Vec<_> = client
        .transport()
        .writes
        .iter()
        .filter(|frame| frame.get(2) == Some(&0x01))
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].len(), 4);
    assert!(
        !client
            .transport()
            .writes
            .iter()
            .any(|frame| frame.get(2) == Some(&0x85))
    );
    client.close().await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn delayed_replies_remain_within_transport_budgets() {
    let mut transport = FaultTransport::new(MockTransport::new());
    transport.receive_delay = std::time::Duration::from_millis(10);
    let mut client = PrinterClient::new(transport, Model::B1);
    assert!(client.raw().set_density(Density::NORMAL).await.unwrap());
    assert_eq!(client.transport().writes.len(), 1);
    client.close().await.unwrap();
}
#[tokio::test]
async fn transcript_mismatch_does_not_consume_the_expected_step() {
    let mut replay = ReplayTransport::from_json(include_bytes!(
        "../fixtures/protocol/density-fragmented.json"
    ))
    .unwrap();
    assert!(replay.send_raw(&[0]).await.is_err());
    assert_eq!(replay.writes, 0);
    replay
        .send_raw(&hex::decode("555521010323aaaa").unwrap())
        .await
        .unwrap();
}

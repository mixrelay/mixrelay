use super::*;

/// Queues frames until the writer is blocked on a peer that does not read and the
/// outbound queue is full.
async fn saturate(link: &Link) {
    let frame = vec![b' '; 64 * 1024];
    while link.outbound.try_send(frame.clone()).is_ok() {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_cancel_behind_a_full_queue_is_sent_once_the_queue_drains() {
    let (editor, mut runtime) = UnixStream::pair().unwrap();
    let link = Link::start(editor, Duration::from_secs(5));
    saturate(&link).await;
    link.cancel(7, Duration::from_secs(5));
    assert!(!link.routes().closed);
    let mut received = Vec::new();
    let mut chunk = vec![0; 1 << 16];
    while !String::from_utf8_lossy(&received).contains("\"subscription\":7") {
        let read = runtime.read(&mut chunk).await.unwrap();
        assert_ne!(read, 0, "the cancel must arrive before the link closes");
        received.extend_from_slice(&chunk[..read]);
    }
}

#[tokio::test]
async fn a_cancel_that_cannot_be_queued_closes_the_link() {
    let (editor, _runtime) = UnixStream::pair().unwrap();
    let link = Link::start(editor, Duration::from_secs(5));
    let mailbox = Arc::new(Mailbox::default());
    assert!(link.register(
        1,
        Route::Telemetry {
            mailbox: Arc::clone(&mailbox),
            decoder: TelemetryDecoder::default(),
        },
        Duration::from_secs(10),
    ));
    saturate(&link).await;
    link.cancel(7, Duration::from_millis(20));
    assert_eq!(
        mailbox.take().await.unwrap_err(),
        "runtime connection closed"
    );
    assert!(link.routes().closed);
}

#[tokio::test]
async fn a_frame_a_closed_link_refused_was_never_sent() {
    let (editor, runtime) = UnixStream::pair().unwrap();
    drop(runtime);
    let link = Link::start(editor, Duration::from_secs(1));
    // The reader sees the peer leave and closes the link, stopping the writer.
    while !link.routes().closed {
        tokio::task::yield_now().await;
    }
    for _ in 0..OUTBOUND_FRAMES * 2 {
        if !link.send(b"{}\n".to_vec(), Duration::from_millis(20)).await {
            return;
        }
    }
    panic!("a closed link must refuse frames");
}

#[tokio::test]
async fn a_link_whose_peer_left_at_once_stops_both_tasks() {
    for _ in 0..50 {
        let (editor, runtime) = UnixStream::pair().unwrap();
        drop(runtime);
        let link = Link::start(editor, Duration::from_secs(1));
        while !link.routes().closed {
            tokio::task::yield_now().await;
        }
        // Both tasks were stopped, so nothing holds the queue open.
        tokio::time::timeout(Duration::from_secs(1), link.outbound.closed())
            .await
            .unwrap();
    }
}

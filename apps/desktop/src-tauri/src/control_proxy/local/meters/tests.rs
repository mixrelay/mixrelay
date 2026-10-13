use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::sync::mpsc;

type Chunk = Result<Option<Vec<u8>>, String>;
/// A prepared engine stream: its body, a refusal, or (`None`) an open that never completes.
type Prepared = Option<Result<mpsc::UnboundedReceiver<Chunk>, String>>;

/// An engine whose streams are fed by the test; each open takes the next prepared stream.
#[derive(Default)]
struct Engine {
    streams: Mutex<Vec<Prepared>>,
    opened: AtomicUsize,
}

impl Engine {
    fn stream(&self) -> mpsc::UnboundedSender<Chunk> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.streams.lock().unwrap().insert(0, Some(Ok(receiver)));
        sender
    }

    fn refuse(&self, reason: &str) {
        self.streams
            .lock()
            .unwrap()
            .insert(0, Some(Err(reason.to_owned())));
    }

    /// The next open never completes.
    fn hang(&self) {
        self.streams.lock().unwrap().insert(0, None);
    }
}

struct Body(mpsc::UnboundedReceiver<Chunk>);

impl MeterSource for Engine {
    type Chunks = Body;

    fn open(&self) -> impl Future<Output = Result<Body, String>> + Send {
        self.opened.fetch_add(1, Ordering::SeqCst);
        let next = self
            .streams
            .lock()
            .unwrap()
            .pop()
            .expect("a prepared stream");
        async move {
            match next {
                Some(stream) => stream.map(Body),
                None => std::future::pending().await,
            }
        }
    }
}

impl MeterChunks for Body {
    async fn next_chunk(&mut self) -> Chunk {
        self.0.recv().await.unwrap_or(Ok(None))
    }
}

/// One engine window as the bytes of a stream chunk.
fn event(sequence: u64, peak: f32) -> Vec<u8> {
    let window = serde_json::json!({
        "engineIncarnation": "engine",
        "sequence": sequence,
        "meters": {"protocolVersion": 1, "revision": 1, "nodes": [
            {"nodeId": "n", "portId": "out", "peaks": [peak]}
        ]}
    });
    format!("event: meters\ndata: {window}\n\n").into_bytes()
}

fn hub(engine: &Arc<Engine>) -> (Arc<MeterHub<Engine>>, watch::Sender<bool>) {
    let (stop, stopped) = watch::channel(false);
    (MeterHub::new(Arc::clone(engine), stopped), stop)
}

async fn settle() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn editors_share_one_engine_stream_and_a_slow_one_keeps_the_interval_maximum() {
    let engine = Arc::new(Engine::default());
    let feed = engine.stream();
    let (hub, _stop) = hub(&engine);
    let fast = hub.subscribe();
    let slow = hub.subscribe();
    feed.send(Ok(Some(event(1, 0.9)))).unwrap();
    assert_eq!(fast.take().await.unwrap().sequence, 1);
    feed.send(Ok(Some(b": keep-alive\n\ndata: not json\n\n".to_vec())))
        .unwrap();
    feed.send(Ok(Some(event(2, 0.1)))).unwrap();
    assert_eq!(fast.take().await.unwrap().sequence, 2);
    let merged = slow.take().await.unwrap();
    assert_eq!((merged.sequence, merged.meters.nodes[0].peaks[0]), (2, 0.9));
    assert_eq!(engine.opened.load(Ordering::SeqCst), 1);
    drop(feed);
    assert_eq!(fast.take().await.unwrap_err(), "telemetry stream ended");
    assert_eq!(slow.take().await.unwrap_err(), "telemetry stream ended");
}

#[tokio::test]
async fn the_stream_closes_with_its_last_reader_and_reopens_for_the_next() {
    let engine = Arc::new(Engine::default());
    let first = engine.stream();
    let (hub, _stop) = hub(&engine);
    drop(hub.subscribe());
    first.send(Ok(Some(event(1, 0.5)))).unwrap();
    settle().await;
    assert!(
        first.is_closed(),
        "the engine stream closes without readers"
    );
    let second = engine.stream();
    let reader = hub.subscribe();
    second.send(Ok(Some(event(2, 0.5)))).unwrap();
    assert_eq!(reader.take().await.unwrap().sequence, 2);
    assert_eq!(engine.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_retired_upstream_never_ends_its_successors_readers() {
    let engine = Arc::new(Engine::default());
    let (hub, _stop) = hub(&engine);
    // Generation 1 is retired; a reader of generation 2 must survive its cleanup.
    hub.readers().upstream = Some(2);
    let reader = Arc::new(Mailbox::default());
    hub.readers().mailboxes.push(Arc::downgrade(&reader));
    engine.refuse("old engine");
    Arc::clone(&hub).upstream(1).await;
    assert_eq!(hub.readers().upstream, Some(2));
    assert_eq!(hub.readers().mailboxes.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn readers_learn_why_the_engine_stream_failed() {
    let engine = Arc::new(Engine::default());
    let (hub, stop) = hub(&engine);
    engine.refuse("meter telemetry HTTP 404");
    assert_eq!(
        hub.subscribe().take().await.unwrap_err(),
        "meter telemetry HTTP 404"
    );
    engine.hang();
    assert_eq!(
        hub.subscribe().take().await.unwrap_err(),
        "telemetry stream did not open"
    );
    let silent = engine.stream();
    assert_eq!(
        hub.subscribe().take().await.unwrap_err(),
        "telemetry stream stalled"
    );
    drop(silent);
    let failing = engine.stream();
    failing.send(Err("connection reset".to_owned())).unwrap();
    assert_eq!(
        hub.subscribe().take().await.unwrap_err(),
        "connection reset"
    );
    let oversized = engine.stream();
    oversized
        .send(Ok(Some(vec![b'a'; 4 * 1024 * 1024 + 1])))
        .unwrap();
    assert_eq!(
        hub.subscribe().take().await.unwrap_err(),
        "telemetry event exceeds its limit"
    );
    let _open = engine.stream();
    let reader = hub.subscribe();
    stop.send(true).unwrap();
    assert_eq!(reader.take().await.unwrap_err(), "runtime stopped");
}

async fn frame(reader: &mut BufReader<tokio::io::DuplexStream>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn an_editor_relay_writes_windows_then_the_terminal_frame() {
    let engine = Arc::new(Engine::default());
    let feed = engine.stream();
    let (hub, stop) = hub(&engine);
    let (runtime, editor) = tokio::io::duplex(1 << 16);
    let (mut read, mut write) = tokio::io::split(runtime);
    let relay = tokio::spawn({
        let hub = Arc::clone(&hub);
        let stopped = stop.subscribe();
        async move { relay(7, &hub, &mut read, &mut write, stopped).await }
    });
    let mut editor = BufReader::new(editor);
    feed.send(Ok(Some(event(1, 0.5)))).unwrap();
    let window = frame(&mut editor).await;
    assert_eq!(
        (window["id"].clone(), window["status"].clone()),
        (7.into(), 206.into())
    );
    assert_eq!(window["body"]["sequence"], 1);
    drop(feed);
    let ended = frame(&mut editor).await;
    assert_eq!(ended["status"], 503);
    assert_eq!(
        ended["body"]["parameters"]["reason"],
        "telemetry stream ended"
    );
    relay.await.unwrap().unwrap();
}

#[tokio::test]
async fn an_editor_relay_ends_when_the_editor_leaves_or_the_runtime_stops() {
    let engine = Arc::new(Engine::default());
    let _feed = engine.stream();
    let (hub, stop) = hub(&engine);
    for leave in [true, false] {
        let (runtime, mut editor) = tokio::io::duplex(1 << 16);
        let (mut read, mut write) = tokio::io::split(runtime);
        let relay = tokio::spawn({
            let hub = Arc::clone(&hub);
            let stopped = stop.subscribe();
            async move { relay(1, &hub, &mut read, &mut write, stopped).await }
        });
        settle().await;
        if leave {
            editor.write_all(b"x").await.unwrap();
        } else {
            stop.send(true).unwrap();
        }
        relay.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn an_editor_relay_stops_while_a_write_is_blocked() {
    let engine = Arc::new(Engine::default());
    let feed = engine.stream();
    let (hub, stop) = hub(&engine);
    // A one-byte pipe the editor never drains blocks the first frame.
    let (runtime, _editor) = tokio::io::duplex(1);
    let (mut read, mut write) = tokio::io::split(runtime);
    let relay = tokio::spawn({
        let hub = Arc::clone(&hub);
        let stopped = stop.subscribe();
        async move { relay(1, &hub, &mut read, &mut write, stopped).await }
    });
    feed.send(Ok(Some(event(1, 0.5)))).unwrap();
    settle().await;
    stop.send(true).unwrap();
    relay.await.unwrap().unwrap();
}

#[tokio::test]
async fn an_editor_relay_leaves_a_blocked_terminal_frame_when_the_editor_leaves() {
    let engine = Arc::new(Engine::default());
    engine.refuse("engine gone");
    let (hub, stop) = hub(&engine);
    let (runtime, mut editor) = tokio::io::duplex(1);
    let (mut read, mut write) = tokio::io::split(runtime);
    let relay = tokio::spawn({
        let hub = Arc::clone(&hub);
        let stopped = stop.subscribe();
        async move { relay(1, &hub, &mut read, &mut write, stopped).await }
    });
    settle().await;
    assert!(!relay.is_finished(), "the terminal frame is blocked");
    editor.write_all(b"x").await.unwrap();
    relay.await.unwrap().unwrap();
}

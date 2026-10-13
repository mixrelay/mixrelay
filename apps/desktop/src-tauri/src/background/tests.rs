use super::*;
use std::{fs, os::unix::fs::PermissionsExt, time::Instant};
use tokio::net::UnixListener;

fn fixture_connection(directory: &std::path::Path) -> Connection {
    Connection {
        path: directory.join("service.sock"),
        uid: fs::metadata(directory).unwrap().uid(),
        idle: Arc::default(),
    }
}

#[tokio::test]
async fn heartbeat_keeps_one_command_alive_and_returns_the_service_error() {
    let directory = tempfile::tempdir().unwrap();
    let connection = fixture_connection(directory.path());
    let listener = UnixListener::bind(&connection.path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut frame = String::new();
        reader.read_line(&mut frame).await.unwrap();
        let request: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(request["command"]["name"], "driver_mutate");
        for body in [
            json!({"id":request["id"],"status":102,"body":{}}),
            json!({"id":request["id"],"status":200,"body":{"error":{"code":"feature_limit"}}}),
        ] {
            reader
                .get_mut()
                .write_all(format!("{body}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    assert_eq!(
        connection
            .invoke("driver_mutate", json!({}))
            .await
            .unwrap_err(),
        json!({"code":"feature_limit"})
    );
    server.await.unwrap();
}

/// Answers `count` requests on one accepted stream with `{"ok": id}`.
async fn answer(reader: &mut BufReader<tokio::net::UnixStream>, count: usize) {
    for _ in 0..count {
        let mut frame = String::new();
        reader.read_line(&mut frame).await.unwrap();
        let request: Value = serde_json::from_str(&frame).unwrap();
        let reply = json!({"id":request["id"],"status":200,"body":{"ok":request["id"]}});
        reader
            .get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn sequential_requests_reuse_one_open_connection() {
    let directory = tempfile::tempdir().unwrap();
    let connection = fixture_connection(directory.path());
    let listener = UnixListener::bind(&connection.path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        answer(&mut BufReader::new(stream), 2).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    for _ in 0..2 {
        assert!(
            connection
                .invoke("control_get_meters", json!({}))
                .await
                .is_ok()
        );
    }
    server.await.unwrap();
}

#[tokio::test]
async fn a_connection_the_controller_closed_is_replaced_not_reused() {
    let directory = tempfile::tempdir().unwrap();
    let connection = fixture_connection(directory.path());
    let listener = UnixListener::bind(&connection.path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        answer(&mut BufReader::new(stream), 1).await;
        // The first stream is closed here; the next request needs a new connection.
        let (stream, _) = listener.accept().await.unwrap();
        answer(&mut BufReader::new(stream), 1).await;
    });
    assert!(
        connection
            .invoke("control_get_meters", json!({}))
            .await
            .is_ok()
    );
    // Give the close time to reach the client before it chooses a connection.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(connection.invoke("driver_mutate", json!({})).await.is_ok());
    server.await.unwrap();
}

#[tokio::test]
async fn disconnect_after_delivery_never_replays_a_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let connection = fixture_connection(directory.path());
    let listener = UnixListener::bind(&connection.path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut frame = String::new();
        reader.read_line(&mut frame).await.unwrap();
        drop(reader);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let error = connection
        .invoke("control_apply_graph", json!({"document":{}}))
        .await
        .unwrap_err();
    assert!(error.as_str().unwrap().contains("not replayed"));
    server.await.unwrap();
}

#[tokio::test]
async fn another_users_socket_is_rejected_before_command_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = fixture_connection(directory.path());
    connection.uid = connection.uid.wrapping_add(1);
    let listener = UnixListener::bind(&connection.path).unwrap();
    assert!(
        connection
            .invoke("licensing_activate", json!({"key":"test"}))
            .await
            .unwrap_err()
            .as_str()
            .unwrap()
            .contains("different user")
    );
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut byte = [0];
    assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
}

#[test]
fn editor_process_fixture() {
    let Some(executable) = std::env::var_os("MIXRELAY_FIXTURE_CONTROLLER") else {
        return;
    };
    spawn_controller(std::path::Path::new(&executable)).unwrap();
}

#[test]
fn controller_survives_actual_editor_process_exit() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("controller.sh");
    let ready = directory.path().join("ready");
    let stop = directory.path().join("stop");
    let finished = directory.path().join("finished");
    // A bounded fixture always exits even if an assertion fails; no native audio is opened.
    fs::write(&executable, "#!/bin/sh\ncd \"$(dirname \"$0\")\"\nprintf ready > ready\ni=0\nwhile test ! -f stop && test \"$i\" -lt 100; do sleep 0.05; i=$((i+1)); done\nprintf finished > finished\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "background::tests::editor_process_fixture"])
        .env("MIXRELAY_FIXTURE_CONTROLLER", &executable)
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !finished.exists(),
        "editor exit must not stop the controller"
    );
    fs::write(stop, "stop").unwrap();
    while !finished.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

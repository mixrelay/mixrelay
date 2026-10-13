//! User-owned Unix socket transport. No network listener, detached command queue, or audio work.
use std::future::Future;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::{LocalControl, Request};

pub(crate) const FRAME_LIMIT: u64 = 4 * 1024 * 1024;
pub(crate) const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
/// A connection carries one request at a time and may be reused for the next. It closes
/// after this long without one; clients reuse an idle connection for a shorter window.
pub(crate) const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct LocalSocket {
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
    operation_liveness: Arc<tokio::sync::Notify>,
}

struct SocketPath {
    path: PathBuf,
    inode: u64,
    // Linux may immediately reuse an unlinked inode. Retain its VFS identity until
    // this guard is dropped so it cannot mistake a replacement socket for its own.
    #[cfg(target_os = "linux")]
    _inode_handle: std::fs::File,
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.ino() == self.inode && metadata.file_type().is_socket())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl LocalSocket {
    /// Binds only within an owner-only directory. Existing live sockets are never replaced.
    pub(crate) async fn start(
        directory: &Path,
        owner_uid: u32,
        control: LocalControl,
    ) -> io::Result<Self> {
        let (listener, path) = bind(directory, owner_uid).await?;
        let operation_liveness = Arc::clone(&control.client.operation_liveness);
        let (stop, stopped) = watch::channel(false);
        let (finished, done) = watch::channel(false);
        tokio::spawn(async move {
            serve(listener, Arc::new(control), stopped, owner_uid).await;
            drop(path);
            let _ = finished.send(true);
        });
        Ok(Self {
            stop,
            done,
            operation_liveness,
        })
    }

    /// Stop admission, cancel idle readers and join accepted commands before releasing engine ownership.
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        let _ = self.stop.send(true);
        let mut done = self.done.clone();
        super::super::wait_for_controls(
            async {
                while !*done.borrow() {
                    done.changed().await.map_err(|error| error.to_string())?;
                }
                Ok::<(), String>(())
            },
            &self.operation_liveness,
        )
        .await?
    }
}

impl Drop for LocalSocket {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

async fn bind(directory: &Path, uid: u32) -> io::Result<(UnixListener, SocketPath)> {
    match std::fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "local control directory must be owned by this user with mode 0700",
        ));
    }
    let path = directory.join("control.sock");
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if !metadata.file_type().is_socket() || metadata.uid() != uid {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "local control path is not an owned socket",
            ));
        }
        match tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(&path)).await {
            Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(&path)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "local control socket is active",
                ));
            }
        }
    }
    let listener = UnixListener::bind(&path)?;
    let owner = SocketPath {
        inode: std::fs::symlink_metadata(&path)?.ino(),
        #[cfg(target_os = "linux")]
        _inode_handle: std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?,
        path,
    };
    std::fs::set_permissions(&owner.path, std::fs::Permissions::from_mode(0o600))?;
    Ok((listener, owner))
}

async fn serve(
    listener: UnixListener,
    control: Arc<LocalControl>,
    mut stop: watch::Receiver<bool>,
    uid: u32,
) {
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            Some(_) = clients.join_next(), if !clients.is_empty() => {},
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break; };
                if clients.len() >= 32 || !stream.peer_cred().is_ok_and(|peer| peer.uid() == uid) { continue; }
                let control = Arc::clone(&control);
                let stopped = stop.clone();
                clients.spawn(async move { let _ = connection(stream, control, stopped).await; });
            }
        }
    }
    drop(listener);
    while clients.join_next().await.is_some() {}
}

async fn connection(
    stream: UnixStream,
    control: Arc<LocalControl>,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read.take(FRAME_LIMIT + 1));
    let mut frame = Vec::new();
    loop {
        frame.clear();
        read.get_mut().set_limit(FRAME_LIMIT + 1);
        let received = tokio::select! {
            biased;
            _ = stop.changed() => return Ok(()),
            result = tokio::time::timeout(IDLE_TIMEOUT, read.read_until(b'\n', &mut frame)) => result??,
        };
        if received == 0
            || *stop.borrow()
            || frame.len() as u64 > FRAME_LIMIT
            || frame.last() != Some(&b'\n')
        {
            return Ok(());
        }
        let request: Request = serde_json::from_slice(&frame).map_err(io::Error::other)?;
        let request_id = request.id;
        if !request.is_read() {
            let delivered = dispatch_mutation(
                request_id,
                &mut write,
                control.dispatch(request),
                HEARTBEAT_INTERVAL,
            )
            .await?;
            // A peer that stopped reading cannot be trusted with another request.
            if !delivered {
                return Ok(());
            }
            continue;
        }
        let response = {
            tokio::select! {
                biased;
                _ = stop.changed() => return Ok(()),
                response = control.dispatch(request) => response,
            }
        };
        write_frame(&mut write, &response).await?;
    }
}

async fn dispatch_mutation<W, F>(
    request_id: u64,
    write: &mut W,
    dispatch: F,
    heartbeat_interval: Duration,
) -> io::Result<bool>
where
    W: AsyncWrite + Unpin,
    F: Future<Output = serde_json::Value>,
{
    let mut dispatch = Box::pin(dispatch);
    let first_heartbeat = tokio::time::Instant::now() + heartbeat_interval;
    let mut heartbeat = tokio::time::interval_at(first_heartbeat, heartbeat_interval);
    loop {
        tokio::select! {
            response = &mut dispatch => return write_frame(write, &response).await.map(|()| true),
            _ = heartbeat.tick() => {
                let pending = serde_json::json!({"id": request_id, "status": 102, "body": {}});
                if write_frame(write, &pending).await.is_err() {
                    // The mutation was accepted before the peer failed. Keep it owned by this
                    // connection task and wait for completion, but never write to the peer again.
                    let _ = dispatch.await;
                    return Ok(false);
                }
            }
        }
    }
}

async fn write_frame<W>(write: &mut W, response: &serde_json::Value) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut bytes = serde_json::to_vec(response).map_err(io::Error::other)?;
    if bytes.len() > RESPONSE_LIMIT {
        return Err(io::Error::other("local control response exceeds limit"));
    }
    bytes.push(b'\n');
    tokio::time::timeout(IO_TIMEOUT, write.write_all(&bytes))
        .await
        .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error))??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn owned_socket_refuses_live_replacement_and_cleans_up() {
        let fixture = mixrelay_test_support::FixtureDir::new("ipc").expect("fixture");
        let directory = fixture.join("control");
        let uid = std::fs::metadata(fixture.path()).expect("metadata").uid();
        let (listener, owner) = bind(&directory, uid).await.expect("bind");
        assert_eq!(
            std::fs::metadata(&directory).expect("directory").mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&owner.path).expect("socket").mode() & 0o777,
            0o600
        );
        assert_eq!(
            bind(&directory, uid)
                .await
                .err()
                .expect("live rejection")
                .kind(),
            io::ErrorKind::AddrInUse
        );
        drop(listener);
        // A dead listener's inode is recoverable, but the old guard cannot delete its replacement.
        let (replacement, next) = bind(&directory, uid).await.expect("stale recovery");
        drop(owner);
        assert!(next.path.exists());
        drop(replacement);
        let path = next.path.clone();
        drop(next);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn unsafe_directories_and_non_socket_targets_are_not_overwritten() {
        let fixture = mixrelay_test_support::FixtureDir::new("ipc-path").expect("fixture");
        let uid = std::fs::metadata(fixture.path()).expect("metadata").uid();
        assert_eq!(
            bind(fixture.path(), uid)
                .await
                .err()
                .expect("permissions")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        let directory = fixture.join("control");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .expect("directory");
        let path = directory.join("control.sock");
        std::fs::write(&path, b"user data").expect("file");
        assert!(bind(&directory, uid).await.is_err());
        assert_eq!(std::fs::read(path).expect("retained file"), b"user data");
        let link = fixture.join("link");
        std::os::unix::fs::symlink(&directory, &link).expect("symlink");
        assert!(bind(&link, uid).await.is_err());
    }

    #[tokio::test]
    async fn shutdown_cancels_idle_readers_and_removes_only_owned_socket() {
        let fixture = mixrelay_test_support::FixtureDir::new("ipc-stop").expect("fixture");
        let directory = fixture.join("control");
        let uid = std::fs::metadata(fixture.path()).expect("metadata").uid();
        let control = LocalControl::new(
            crate::control_proxy::ControlProxyClient::default(),
            #[cfg(target_os = "macos")]
            Arc::default(),
        )
        .expect("control");
        let service = LocalSocket::start(&directory, uid, control)
            .await
            .expect("service");
        let mut client = UnixStream::connect(directory.join("control.sock"))
            .await
            .expect("connect");
        client
            .write_all(b"{\"id\":")
            .await
            .expect("incomplete frame");
        tokio::time::timeout(Duration::from_secs(1), service.shutdown())
            .await
            .expect("bounded shutdown")
            .expect("shutdown");
        assert!(!directory.join("control.sock").exists());
        service.shutdown().await.expect("idempotent shutdown");
    }

    #[tokio::test]
    async fn mutation_emits_pending_then_terminal_frame() {
        let (mut peer, mut writer) = tokio::io::duplex(4096);
        let dispatch = async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            serde_json::json!({"id": 7, "status": 200, "body": {"accepted": true}})
        };
        assert!(
            dispatch_mutation(7, &mut writer, dispatch, Duration::from_millis(5))
                .await
                .expect("frames")
        );
        drop(writer);

        let mut output = Vec::new();
        peer.read_to_end(&mut output).await.expect("read frames");
        let frames = output
            .split(|byte| *byte == b'\n')
            .filter(|frame| !frame.is_empty())
            .map(|frame| serde_json::from_slice::<Value>(frame).expect("json frame"))
            .collect::<Vec<_>>();
        assert!(frames.iter().any(|frame| frame["status"] == 102));
        assert_eq!(
            frames.last(),
            Some(&serde_json::json!({
                "id": 7,
                "status": 200,
                "body": {"accepted": true}
            }))
        );
    }

    #[tokio::test]
    async fn failed_heartbeat_does_not_cancel_accepted_mutation() {
        let (peer, mut writer) = tokio::io::duplex(64);
        drop(peer);
        let (completed, received) = oneshot::channel();
        let dispatch = async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = completed.send(());
            serde_json::json!({"id": 8, "status": 200, "body": {}})
        };
        assert!(
            !dispatch_mutation(8, &mut writer, dispatch, Duration::from_millis(5))
                .await
                .expect("broken peer is contained")
        );
        tokio::time::timeout(Duration::from_millis(100), received)
            .await
            .expect("mutation completion")
            .expect("completion signal");
    }

    #[tokio::test]
    async fn one_connection_carries_sequential_requests_until_the_peer_closes() {
        let fixture = mixrelay_test_support::FixtureDir::new("ipc-reuse").expect("fixture");
        let directory = fixture.join("control");
        let uid = std::fs::metadata(fixture.path()).expect("metadata").uid();
        let control = LocalControl::new(
            crate::control_proxy::ControlProxyClient::default(),
            #[cfg(target_os = "macos")]
            Arc::default(),
        )
        .expect("control");
        let service = LocalSocket::start(&directory, uid, control)
            .await
            .expect("service");
        let stream = UnixStream::connect(directory.join("control.sock"))
            .await
            .expect("connect");
        let mut reader = BufReader::new(stream);
        for id in [1, 2] {
            let request = serde_json::json!({"id": id, "command": {"type": "invoke", "name": "hello", "arguments": {}}});
            reader
                .get_mut()
                .write_all(format!("{request}\n").as_bytes())
                .await
                .expect("request");
            let mut reply = String::new();
            reader.read_line(&mut reply).await.expect("reply");
            let reply: Value = serde_json::from_str(&reply).expect("json");
            assert_eq!(reply["id"], id);
            assert_eq!(reply["status"], 503);
        }
        drop(reader);
        tokio::time::timeout(Duration::from_secs(1), service.shutdown())
            .await
            .expect("bounded shutdown")
            .expect("shutdown");
    }
}

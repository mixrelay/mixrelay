//! User-owned Unix socket transport. No network listener, detached command queue, or audio work.
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::{LocalControl, Request};

const FRAME_LIMIT: u64 = 16 * 1024;
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct LocalSocket {
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}

struct SocketPath {
    path: PathBuf,
    inode: u64,
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
        let (stop, stopped) = watch::channel(false);
        let (finished, done) = watch::channel(false);
        tokio::spawn(async move {
            serve(listener, Arc::new(control), stopped, owner_uid).await;
            drop(path);
            let _ = finished.send(true);
        });
        Ok(Self { stop, done })
    }

    /// Stop admission, cancel idle readers and join accepted commands before releasing engine ownership.
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        let _ = self.stop.send(true);
        let mut done = self.done.clone();
        tokio::time::timeout(Duration::from_secs(35), async {
            while !*done.borrow() {
                done.changed().await.map_err(|error| error.to_string())?;
            }
            Ok::<(), String>(())
        })
        .await
        .map_err(|_| {
            "Local controls are still finishing; audio ownership was retained.".to_owned()
        })?
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
                if clients.len() >= 8 || !stream.peer_cred().is_ok_and(|peer| peer.uid() == uid) { continue; }
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
    tokio::select! {
        biased;
        _ = stop.changed() => return Ok(()),
        result = tokio::time::timeout(IO_TIMEOUT, read.read_until(b'\n', &mut frame)) => { result??; }
    }
    if *stop.borrow() || frame.len() as u64 > FRAME_LIMIT || frame.last() != Some(&b'\n') {
        return Ok(());
    }
    let request: Request = serde_json::from_slice(&frame).map_err(io::Error::other)?;
    let response = if request.is_read() {
        tokio::select! {
            biased;
            _ = stop.changed() => return Ok(()),
            response = control.dispatch(request) => response,
        }
    } else {
        control.dispatch(request).await
    };
    let mut bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;
    if bytes.len() > RESPONSE_LIMIT {
        return Err(io::Error::other("local control response exceeds limit"));
    }
    bytes.push(b'\n');
    tokio::time::timeout(IO_TIMEOUT, write.write_all(&bytes)).await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

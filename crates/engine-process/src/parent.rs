use std::{
    io::{self, Read},
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_FAILURE_GRACE: Duration = Duration::from_millis(500);
const READY_EXIT_POLL: Duration = Duration::from_millis(1);

/// A controller-owned engine process and its private shutdown lease.
///
/// The lease is the only object used to request an orderly engine shutdown:
/// closing it closes the child's standard-input pipe. The child handle remains
/// owned here until the engine has exited, and is moved to a named reaper thread
/// when this value is dropped.
pub struct ControllerEngine {
    child: Option<Child>,
    lease: Option<ChildStdin>,
    id: u32,
    reaped: bool,
}

impl ControllerEngine {
    /// Spawns `executable` in controller-owned mode and waits for its readiness
    /// acknowledgement on the child's private standard-output pipe.
    ///
    /// The executable is invoked with exactly [`crate::CONTROLLER_ARGUMENT`]. Its
    /// standard input and output are piped, while standard error is inherited.
    /// Startup fails if the acknowledgement is not received within 30 seconds,
    /// is malformed, or the child exits before the startup check completes.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if spawning, piping, readiness acknowledgement, or
    /// the startup liveness check fails. On every post-spawn error the private
    /// lease is closed, the owned child is given a bounded grace period, and
    /// only that child is killed and reaped if it remains alive.
    pub fn start(executable: &Path) -> io::Result<Self> {
        let mut command = Command::new(executable);
        command
            .arg(crate::CONTROLLER_ARGUMENT)
            // The controller reaches its engine at the session's socket directory, so the
            // child must not follow a socket-directory override (MIXRELAY_IPC_DIR) it
            // happened to inherit.
            .env_remove("MIXRELAY_IPC_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn()?;
        let id = child.id();
        let Some(lease) = child.stdin.take() else {
            return Err(fail_start(
                child,
                io::Error::other("controller-owned engine stdin pipe was not created"),
            ));
        };
        let Some(stdout) = child.stdout.take() else {
            drop(lease);
            return Err(fail_start(
                child,
                io::Error::other("controller-owned engine stdout pipe was not created"),
            ));
        };

        if let Err(error) = wait_for_ready(stdout) {
            drop(lease);
            return Err(fail_start(child, error));
        }

        if let Err(error) = reject_child_that_exited_during_startup(&mut child) {
            drop(lease);
            return Err(fail_start(child, error));
        }

        Ok(Self {
            child: Some(child),
            lease: Some(lease),
            id,
            reaped: false,
        })
    }

    /// Returns the operating-system process identifier of the owned child.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Closes the private lease and requests the engine's existing graceful
    /// drain. This does not kill the child or wait for it to exit.
    pub fn request_shutdown(&mut self) {
        self.lease.take();
    }

    /// Polls the owned child without forcing termination.
    ///
    /// `Ok(None)` means that the process is still running. Callers that own an
    /// async UI should poll this method from their async shutdown state machine
    /// until it returns `Ok(Some(status))`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the operating system cannot inspect the child.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let result = self.child.as_mut().map_or(Ok(None), Child::try_wait);
        if matches!(result, Ok(Some(_))) {
            self.reaped = true;
        }
        result
    }
}

impl Drop for ControllerEngine {
    fn drop(&mut self) {
        self.lease.take();
        let Some(child) = self.child.take() else {
            return;
        };
        if self.reaped {
            return;
        }
        if let Err(error) = reap_without_blocking_owner(child) {
            tracing::error!(%error, "failed to reap owned controller engine");
        }
    }
}

fn wait_for_ready(reader: impl Read + Send + 'static) -> io::Result<()> {
    wait_for_ready_with_timeout(reader, HANDSHAKE_TIMEOUT)
}

fn wait_for_ready_with_timeout(
    mut reader: impl Read + Send + 'static,
    timeout: Duration,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("mixrelay-engine-ready".into())
        .spawn(move || {
            let result = read_ready(&mut reader);
            // The parent may have timed out and dropped the receiver. The
            // reader thread has no process ownership and may then exit quietly.
            let _ = sender.send(result);
        })
        .map_err(|error| {
            io::Error::new(error.kind(), format!("start readiness reader: {error}"))
        })?;

    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "controller-owned engine readiness timed out",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::other(
            "controller-owned engine readiness reader ended without a result",
        )),
    }
}

fn read_ready(reader: &mut impl Read) -> io::Result<()> {
    let mut acknowledgement = [0; crate::READY.len()];
    reader.read_exact(&mut acknowledgement)?;
    if acknowledgement != *crate::READY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "controller-owned engine readiness acknowledgement was invalid",
        ));
    }
    Ok(())
}

fn reject_child_that_exited_during_startup(child: &mut Child) -> io::Result<()> {
    if let Some(status) = child.try_wait()? {
        return Err(io::Error::other(format!(
            "controller-owned engine exited during startup: {status}"
        )));
    }
    Ok(())
}

fn fail_start(mut child: Child, error: io::Error) -> io::Error {
    // Closing stdin is the first cleanup action so a cooperative child can
    // leave its normal drain path before the bounded grace period expires.
    child.stdin.take();
    match reap_after_startup_failure(child) {
        Ok(()) => error,
        Err(cleanup_error) => io::Error::new(
            error.kind(),
            format!("{error}; startup cleanup failed: {cleanup_error}"),
        ),
    }
}

fn reap_after_startup_failure(mut child: Child) -> io::Result<()> {
    let deadline = Instant::now() + STARTUP_FAILURE_GRACE;
    let mut probe_error = None;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(error) => {
                probe_error = Some(error);
                break;
            }
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        thread::sleep((deadline - now).min(READY_EXIT_POLL));
    }

    let mut cleanup_error = probe_error;
    match child.kill() {
        Ok(()) => {}
        Err(error) => {
            combine_error(&mut cleanup_error, &error, "kill owned startup child");
        }
    }
    if let Err(error) = reap_without_blocking_owner(child) {
        combine_error(
            &mut cleanup_error,
            &error,
            "schedule owned startup child reaper",
        );
    }
    cleanup_error.map_or(Ok(()), Err)
}

fn combine_error(slot: &mut Option<io::Error>, error: &io::Error, operation: &str) {
    let message = format!("{operation}: {error}");
    if let Some(existing) = slot.take() {
        *slot = Some(io::Error::new(
            existing.kind(),
            format!("{existing}; {message}"),
        ));
    } else {
        *slot = Some(io::Error::other(message));
    }
}

fn reap_without_blocking_owner(child: Child) -> io::Result<()> {
    let slot = Arc::new(Mutex::new(Some(child)));
    let thread_slot = Arc::clone(&slot);
    let spawned = thread::Builder::new()
        .name("mixrelay-engine-reaper".into())
        .spawn(move || wait_for_owned_child(&thread_slot));

    match spawned {
        Ok(_) => Ok(()),
        Err(spawn_error) => {
            // Keep ownership recoverable if the OS refuses to create another
            // thread. This is the last-resort path; it is preferable to silently
            // dropping the child and leaving a permanent detached engine.
            let wait_result = wait_for_owned_child(&slot);
            match wait_result {
                Ok(()) => Err(io::Error::new(
                    spawn_error.kind(),
                    format!("start owned-child reaper: {spawn_error}"),
                )),
                Err(wait_error) => Err(io::Error::new(
                    wait_error.kind(),
                    format!("start owned-child reaper: {spawn_error}; wait: {wait_error}"),
                )),
            }
        }
    }
}

fn wait_for_owned_child(slot: &Arc<Mutex<Option<Child>>>) -> io::Result<()> {
    let child = match slot.lock() {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    if let Some(mut child) = child {
        let pid = child.id();
        if let Err(error) = child.wait() {
            tracing::error!(pid, %error, "owned controller engine wait failed");
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn readiness_requires_the_exact_acknowledgement() {
        assert!(
            wait_for_ready_with_timeout(Cursor::new(*crate::READY), Duration::from_secs(1)).is_ok()
        );
        let error = wait_for_ready_with_timeout(Cursor::new(b"MRREADY"), Duration::from_secs(1))
            .expect_err("short acknowledgement must fail");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);

        let error = wait_for_ready_with_timeout(Cursor::new(b"MRREADY!"), Duration::from_secs(1))
            .expect_err("wrong acknowledgement must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn readiness_timeout_is_bounded() {
        let started = Instant::now();
        let error = wait_for_ready_with_timeout(SlowReader, Duration::from_millis(10))
            .expect_err("a reader that never acknowledges must time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    struct SlowReader;

    impl Read for SlowReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            thread::sleep(Duration::from_millis(100));
            Ok(0)
        }
    }

    #[cfg(unix)]
    mod unix_processes {
        use super::*;
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            path::{Path, PathBuf},
        };
        use tempfile::{TempDir, tempdir};

        #[test]
        fn owned_fixture_starts_and_exits_after_lease_closure() {
            let (_directory, executable) = fixture("cat >/dev/null");
            let mut engine = ControllerEngine::start(&executable).expect("fixture should start");
            assert_ne!(engine.id(), 0);
            assert!(
                engine
                    .try_wait()
                    .expect("fixture status should be readable")
                    .is_none()
            );

            engine.request_shutdown();
            let status = wait_for_exit(&mut engine);
            assert!(status.success());
        }

        #[test]
        fn malformed_fixture_is_rejected_and_reaped() {
            let (_directory, executable) = fixture_without_ready("printf 'MRREADY?\\n'\nexit 0");
            let Err(error) = ControllerEngine::start(&executable) else {
                panic!("malformed fixture must not start");
            };
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }

        #[test]
        fn child_exiting_before_ready_is_rejected() {
            let (_directory, executable) = fixture_without_ready("exit 0");
            let Err(error) = ControllerEngine::start(&executable) else {
                panic!("early-exit fixture must not start");
            };
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        }

        #[test]
        fn drop_closes_lease_without_waiting_for_the_child() {
            let (_directory, executable) = fixture("cat >/dev/null");
            let engine = ControllerEngine::start(&executable).expect("fixture should start");
            let marker = marker_path(&executable);
            let started = Instant::now();
            drop(engine);
            assert!(started.elapsed() < Duration::from_secs(1));
            wait_for_marker(&marker);
        }

        fn fixture(body: &str) -> (TempDir, PathBuf) {
            fixture_script(&format!("printf 'MRREADY\\n'\n{body}"))
        }

        fn fixture_without_ready(body: &str) -> (TempDir, PathBuf) {
            fixture_script(body)
        }

        fn fixture_script(body: &str) -> (TempDir, PathBuf) {
            let directory = tempdir().expect("temporary fixture directory");
            let executable = directory.path().join("engine.sh");
            let script =
                format!("#!/bin/sh\nset -eu\ntrap 'printf done > \"$0.marker\"' EXIT\n{body}\n");
            fs::write(&executable, script).expect("fixture script");
            let mut permissions = fs::metadata(&executable)
                .expect("fixture metadata")
                .permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&executable, permissions).expect("fixture permissions");
            (directory, executable)
        }

        fn marker_path(executable: &Path) -> PathBuf {
            PathBuf::from(format!("{}.marker", executable.display()))
        }

        fn wait_for_exit(engine: &mut ControllerEngine) -> ExitStatus {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(status) = engine.try_wait().expect("fixture status") {
                    return status;
                }
                assert!(Instant::now() < deadline, "fixture did not exit");
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn wait_for_marker(marker: &Path) {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !marker.is_file() {
                assert!(
                    Instant::now() < deadline,
                    "fixture did not observe lease closure"
                );
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

//! Test-only subprocess protocol fixture; live devices are Rust-owned.
use crate::process::{OwnedChild, drain, nonblocking};
use std::{
    io::{self, Write},
    os::unix::process::CommandExt,
    process::{ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
const POLL_INTERVAL: Duration = Duration::from_millis(10);
pub(super) struct InteractiveFixture {
    child: Option<OwnedChild>,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    stderr: ChildStderr,
    pending: Vec<u8>,
    diagnostics: Vec<u8>,
    io_timeout: Duration,
    shutdown_timeout: Duration,
}

impl InteractiveFixture {
    pub(super) fn spawn(
        command: &mut Command,
        io_timeout: Duration,
        shutdown_timeout: Duration,
    ) -> Result<Self, String> {
        let mut child = OwnedChild(
            command
                .process_group(0)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| format!("launch interactive fixture: {error}"))?,
        );
        let stdin = child.0.stdin.take().expect("requested fixture stdin");
        let stdout = child.0.stdout.take().expect("requested fixture stdout");
        let stderr = child.0.stderr.take().expect("requested fixture stderr");
        nonblocking(&stdin)
            .and_then(|()| nonblocking(&stdout))
            .and_then(|()| nonblocking(&stderr))
            .map_err(|error| format!("configure fixture pipes: {error}"))?;
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout,
            stderr,
            pending: Vec::new(),
            diagnostics: Vec::new(),
            io_timeout,
            shutdown_timeout,
        })
    }

    pub(super) fn pid(&self) -> u32 {
        self.child.as_ref().expect("live fixture child").0.id()
    }

    pub(super) fn handshake(&mut self, expected: &str) -> Result<(), String> {
        let reply = self.read_line(Instant::now() + self.io_timeout)?;
        if reply != expected {
            return Err(self.error(&format!("malformed fixture handshake: {reply:?}")));
        }
        Ok(())
    }

    pub(super) fn command(&mut self, command: &str) -> Result<(), String> {
        let deadline = Instant::now() + self.io_timeout;
        self.write_line(command, deadline)?;
        let reply = self.read_line(deadline)?;
        if reply != "ok" {
            return Err(self.error(&format!("malformed fixture reply: {reply:?}")));
        }
        Ok(())
    }

    fn pump(&mut self) -> Result<(), String> {
        drain(&mut self.stdout, &mut self.pending)
            .and_then(|()| drain(&mut self.stderr, &mut self.diagnostics))
            .map_err(|error| self.error(&error.to_string()))
    }

    fn read_line(&mut self, deadline: Instant) -> Result<String, String> {
        loop {
            self.pump()?;
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line = self.pending.drain(..=end).collect::<Vec<_>>();
                return String::from_utf8(line)
                    .map(|line| line.trim().to_owned())
                    .map_err(|error| self.error(&format!("invalid fixture UTF-8: {error}")));
            }
            if let Some(status) = self
                .child
                .as_mut()
                .expect("live fixture child")
                .0
                .try_wait()
                .map_err(|error| self.error(&error.to_string()))?
            {
                return Err(self.error(&format!("fixture exited before reply: {status}")));
            }
            if Instant::now() >= deadline {
                return Err(self.error("fixture reply deadline exceeded"));
            }
            thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
        }
    }

    fn write_line(&mut self, command: &str, deadline: Instant) -> Result<(), String> {
        if command.len() > 128 || command.contains(['\r', '\n']) {
            return Err(self.error("invalid fixture command line"));
        }
        let line = format!("{command}\n");
        let mut remaining = line.as_bytes();
        while !remaining.is_empty() {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| "fixture stdin already closed".to_owned())?;
            match stdin.write(remaining) {
                Ok(0) => return Err(self.error("fixture stdin closed while writing")),
                Ok(count) => remaining = &remaining[count..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.pump()?;
                    if Instant::now() >= deadline {
                        return Err(self.error("fixture write deadline exceeded"));
                    }
                    thread::sleep(
                        POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                Err(error) => return Err(self.error(&format!("write fixture command: {error}"))),
            }
        }
        Ok(())
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        if self.child.is_none() {
            return Ok(());
        }
        let deadline = Instant::now() + self.shutdown_timeout;
        let mut protocol_error = self.write_line("quit", deadline).err();
        // Closing stdin also requests native aggregate cleanup if quit failed.
        drop(self.stdin.take());
        let result = (|| {
            loop {
                // A diagnostic pipe failure must not cut short the grace
                // period needed for native aggregate destruction.
                if let Err(error) = self.pump() {
                    protocol_error.get_or_insert(error);
                }
                if let Some(status) = self
                    .child
                    .as_mut()
                    .expect("live fixture child")
                    .0
                    .try_wait()
                    .map_err(|error| self.error(&error.to_string()))?
                {
                    if !status.success() {
                        return Err(self.error(&format!("fixture cleanup exited: {status}")));
                    }
                    return protocol_error.map_or(Ok(()), Err);
                }
                if Instant::now() >= deadline {
                    return Err(self.error("fixture cleanup deadline exceeded"));
                }
                thread::sleep(
                    POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        })();
        // OwnedChild kills only this group and reaps it even on timeout/error.
        drop(self.child.take());
        if !self.diagnostics.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&self.diagnostics));
        }
        result
    }

    fn error(&self, message: &str) -> String {
        format!(
            "{message}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&self.pending),
            String::from_utf8_lossy(&self.diagnostics)
        )
    }
}

impl Drop for InteractiveFixture {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            eprintln!("interactive fixture cleanup: {error}");
        }
    }
}

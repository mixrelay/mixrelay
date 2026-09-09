use crate::{
    RecorderAction, RecorderRequest, RecorderState, RecorderStatus,
    transport::{self, MAX_TOKEN, PACKETS, Packet, Transport},
    wave::Take,
};
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::Ordering,
    mpsc::{Receiver, RecvTimeoutError, SyncSender},
};
use std::time::Duration;

pub(crate) enum Command {
    Request(RecorderRequest, SyncSender<Result<RecorderStatus, String>>),
    Shutdown,
}

pub(crate) struct Worker {
    pub(crate) transport: Arc<Transport>,
    pub(crate) status: Arc<Mutex<RecorderStatus>>,
    take: Option<Take>,
    token: u64,
    packet: Packet,
}

impl Worker {
    pub(crate) fn new(transport: Arc<Transport>, status: Arc<Mutex<RecorderStatus>>) -> Self {
        Self {
            packet: Packet::new(transport.channels),
            transport,
            status,
            take: None,
            token: 0,
        }
    }

    pub(crate) fn run(mut self, receiver: &Receiver<Command>) {
        loop {
            if let Some(error) = transport::fault(self.transport.gate.load(Ordering::SeqCst)) {
                self.fail_admission(error.to_owned());
            }
            if let Err(error) = self.drain() {
                self.fail(error);
            }
            // Retirement is still serviced when no UI/status client is polling.
            if self.transport.taps.load(Ordering::SeqCst) == 0
                && self.take.is_some()
                && let Err(error) = self.stop(false)
            {
                self.fail(error);
            }
            let command = if self.take.is_some() {
                receiver.recv_timeout(Duration::from_millis(2))
            } else {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            match command {
                Ok(Command::Request(request, reply)) => {
                    let result = self.command(&request);
                    let _ = reply.send(result);
                }
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    if let Err(error) = self.stop(false) {
                        self.fail(error);
                    }
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn command(&mut self, request: &RecorderRequest) -> Result<RecorderStatus, String> {
        let result = match request.action {
            RecorderAction::Record => self.record(request),
            RecorderAction::Pause => self.stop(true),
            RecorderAction::Stop => self.stop(false),
        };
        result?;
        let status = self.status.lock().clone();
        if let Some(error) = &status.error {
            return Err(error.clone());
        }
        Ok(status)
    }

    fn record(&mut self, request: &RecorderRequest) -> Result<(), String> {
        let status = self.status.lock().clone();
        if status.state == RecorderState::Recording {
            return Err("recorder is already recording".into());
        }
        if self.transport.taps.load(Ordering::SeqCst) == 0 {
            return Err("recorder has no prepared audio tap".into());
        }
        let next = self
            .token
            .checked_add(1)
            .filter(|token| *token <= MAX_TOKEN)
            .ok_or("recorder admission generation exhausted")?;
        if status.state == RecorderState::Paused {
            validate_resume(request, &status)?;
            let observed = self.transport.rate.load(Ordering::Acquire);
            if let Some(take) = &mut self.take
                && let Err(error) = take.prepare_rate(observed)
            {
                self.fail(error.clone());
                return Err(error);
            }
        } else {
            let observed = self.transport.rate.load(Ordering::Acquire);
            let channels =
                u16::try_from(self.transport.channels).expect("validated recorder channel count");
            *self.status.lock() = RecorderStatus {
                path: request.path.clone(),
                sample_rate: request
                    .sample_rate
                    .or(Some(observed).filter(|rate| *rate != 0)),
                encoding: Some(request.encoding.unwrap_or(crate::Encoding::Pcm24)),
                ..RecorderStatus::default()
            };
            let take = match Take::new(request, channels, observed) {
                Ok(take) => take,
                Err(error) => {
                    self.fail(error.clone());
                    return Err(error);
                }
            };
            *self.status.lock() = RecorderStatus {
                state: RecorderState::Stopped,
                path: request.path.clone(),
                sample_rate: Some(take.rate()),
                encoding: Some(take.encoding()),
                frames: 0,
                error: None,
            };
            self.take = Some(take);
        }
        self.token = next;
        self.status.lock().state = RecorderState::Recording;
        self.transport.gate.store(next, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self, pause: bool) -> Result<(), String> {
        if let Some(error) = self.transport.cutoff() {
            self.fail_admission(error.to_owned());
            return Err(error.into());
        }
        if let Err(error) = self.drain() {
            self.fail(error.clone());
            return Err(error);
        }
        let result = if pause {
            self.take
                .as_mut()
                .map_or(Ok(()), Take::flush_segment)
                .map(|()| {
                    self.take
                        .as_ref()
                        .map_or(self.status.lock().frames, Take::frames)
                })
        } else {
            self.take
                .take()
                .map_or(Ok(self.status.lock().frames), Take::finish)
        };
        match result {
            Ok(frames) => {
                let mut status = self.status.lock();
                status.frames = frames;
                if status.state != RecorderState::Failed {
                    status.state = if pause && self.take.is_some() {
                        RecorderState::Paused
                    } else {
                        RecorderState::Stopped
                    };
                }
                Ok(())
            }
            Err(error) => {
                self.fail(error.clone());
                Err(error)
            }
        }
    }

    fn drain(&mut self) -> Result<(), String> {
        for _ in 0..PACKETS {
            if !self.transport.read_packet(&mut self.packet) {
                break;
            }
            let packet = &self.packet;
            let result = self.take.as_mut().map_or(Ok(()), |take| {
                take.write(
                    &packet.samples[..packet.frames * self.transport.channels],
                    packet.rate,
                )
            });
            result?;
        }
        if let Some(take) = &self.take {
            self.status.lock().frames = take.frames();
        }
        Ok(())
    }

    fn fail(&mut self, mut error: String) {
        self.transport.cutoff();
        for _ in 0..PACKETS {
            if !self.transport.read_packet(&mut self.packet) {
                break;
            }
        }
        if let Some(take) = self.take.take() {
            self.status.lock().frames = take.frames();
            match take.finish() {
                Ok(frames) => self.status.lock().frames = frames,
                Err(finalize) => {
                    error = format!("{error}; {finalize}");
                }
            }
        }
        let mut status = self.status.lock();
        status.state = RecorderState::Failed;
        status.error = Some(error);
        self.transport.acknowledge_failure();
    }

    fn fail_admission(&mut self, error: String) {
        // Admission faults do not invalidate packets already accepted into the
        // queue. Preserve that prefix; a real writer failure ends it immediately.
        self.transport.cutoff();
        let error = match self.drain() {
            Ok(()) => error,
            Err(write) => format!("{error}; {write}"),
        };
        self.fail(error);
    }
}

fn validate_resume(request: &RecorderRequest, status: &RecorderStatus) -> Result<(), String> {
    if request
        .path
        .as_ref()
        .is_some_and(|path| Some(path) != status.path.as_ref())
        || request
            .sample_rate
            .is_some_and(|rate| Some(rate) != status.sample_rate)
        || request
            .encoding
            .is_some_and(|encoding| Some(encoding) != status.encoding)
    {
        return Err("resuming a recording cannot change its path, rate or encoding".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;

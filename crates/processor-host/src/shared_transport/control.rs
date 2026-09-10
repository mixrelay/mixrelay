use std::{
    fs::File,
    mem::size_of,
    sync::{Arc, Mutex, MutexGuard, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use crate::{
    ProcessorStateError, STATE_CAPACITY, STATE_FAILED, STATE_OK, STATE_TOO_LARGE,
    STATE_UNSUPPORTED, SharedHeader, WORKER_READY,
};

use super::Mapping;

/// Control-only mapping; neither state mutex is reachable from an audio endpoint.
pub(crate) struct ControlMailbox {
    mapping: Mapping,
    writer: Mutex<()>,
}

/// Broker-only response owner. It cannot issue parent requests or borrow PCM.
pub(crate) struct WorkerControl {
    mapping: Mapping,
    response_writer: Mutex<()>,
}

pub(crate) struct ControlGuard<'a> {
    mailbox: &'a ControlMailbox,
    _writer: MutexGuard<'a, ()>,
}

impl ControlMailbox {
    pub(super) fn open(file: &File) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::open(file, None)?,
            writer: Mutex::new(()),
        })
    }

    pub(crate) fn header(&self) -> &SharedHeader {
        self.mapping.header()
    }

    pub(crate) fn last_request_kind(&self) -> u32 {
        self.header().state_kind.load(Ordering::Acquire)
    }

    pub(crate) fn lock(&self) -> ControlGuard<'_> {
        ControlGuard {
            mailbox: self,
            _writer: self
                .writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }

    pub(crate) fn try_lock(&self) -> Result<ControlGuard<'_>, ProcessorStateError> {
        Ok(ControlGuard {
            mailbox: self,
            _writer: self
                .writer
                .try_lock()
                .map_err(|_| ProcessorStateError::Busy)?,
        })
    }
}

impl WorkerControl {
    pub(super) fn open(file: &Arc<File>) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::open(file, Some(Arc::clone(file)))?,
            response_writer: Mutex::new(()),
        })
    }

    pub(crate) fn header(&self) -> &SharedHeader {
        self.mapping.header()
    }

    pub(crate) fn pending_request(&self) -> Option<(u64, u32)> {
        let header = self.header();
        let request = header.state_request.load(Ordering::Acquire);
        (request != header.state_response.load(Ordering::Acquire))
            .then(|| (request, header.state_kind.load(Ordering::Relaxed)))
    }

    /// Native capture runs before this call. Reentrant/late editor results must not overwrite
    /// an acknowledged response while the parent copies it or owns the next request.
    pub(crate) fn publish(&self, request: u64, state: Result<Vec<u8>, ProcessorStateError>) {
        let Ok(_writer) = self.response_writer.try_lock() else {
            return;
        };
        let header = self.header();
        if header.state_request.load(Ordering::Acquire) != request
            || header.state_response.load(Ordering::Acquire) == request
        {
            return;
        }
        let (status, bytes) = match state {
            Ok(bytes) if bytes.len() <= STATE_CAPACITY => (STATE_OK, bytes),
            Ok(_) => (STATE_TOO_LARGE, Vec::new()),
            Err(ProcessorStateError::Unsupported) => (STATE_UNSUPPORTED, Vec::new()),
            Err(_) => (STATE_FAILED, Vec::new()),
        };
        // SAFETY: this serialized publisher owns the requested state region until its response
        // release. The parent cannot issue another request while copying the previous response.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.mapping.raw.as_mut_ptr().add(size_of::<SharedHeader>()),
                bytes.len(),
            );
        }
        header.state_length.store(
            u32::try_from(bytes.len()).expect("state capacity fits u32"),
            Ordering::Relaxed,
        );
        header.state_result.store(status, Ordering::Relaxed);
        header.state_response.store(request, Ordering::Release);
    }
}

impl ControlGuard<'_> {
    pub(crate) fn request_blob(
        &mut self,
        kind: u32,
        timeout: Duration,
    ) -> Result<Vec<u8>, ProcessorStateError> {
        let header = self.mailbox.header();
        if header.worker_status.load(Ordering::Acquire) != WORKER_READY
            || header.shutdown.load(Ordering::Acquire) != 0
        {
            return Err(ProcessorStateError::Unavailable);
        }
        let previous = header.state_request.load(Ordering::Relaxed);
        if previous != header.state_response.load(Ordering::Acquire) {
            return Err(ProcessorStateError::Pending);
        }
        let request = previous.wrapping_add(1);
        header.state_kind.store(kind, Ordering::Relaxed);
        header.state_request.store(request, Ordering::Release);
        let deadline = Instant::now() + timeout.min(Duration::from_secs(5));
        while header.state_response.load(Ordering::Acquire) != request {
            if header.worker_status.load(Ordering::Acquire) != WORKER_READY
                || header.shutdown.load(Ordering::Acquire) != 0
            {
                return Err(ProcessorStateError::Unavailable);
            }
            if Instant::now() >= deadline {
                return Err(ProcessorStateError::Timeout);
            }
            thread::sleep(Duration::from_millis(1));
        }
        match header.state_result.load(Ordering::Relaxed) {
            STATE_OK => {}
            STATE_TOO_LARGE => return Err(ProcessorStateError::TooLarge),
            STATE_UNSUPPORTED => return Err(ProcessorStateError::Unsupported),
            _ => return Err(ProcessorStateError::NativeFailure),
        }
        let length = header.state_length.load(Ordering::Relaxed) as usize;
        if length > STATE_CAPACITY {
            return Err(ProcessorStateError::InvalidResponse);
        }
        // SAFETY: acquire of this exact response grants read ownership. This guard prevents all
        // other parent requests until the copy ends; stale/duplicate worker publications reject.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                self.mailbox
                    .mapping
                    .raw
                    .as_ptr()
                    .add(size_of::<SharedHeader>()),
                length,
            )
        };
        Ok(bytes.to_vec())
    }
}

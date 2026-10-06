use std::{
    fs::{File, OpenOptions},
    mem::{align_of, size_of},
    path::Path,
    sync::Arc,
};

use memmap2::MmapRaw;

use crate::{AudioLayout, SHARED_MAGIC, SHARED_VERSION};

mod audio;
mod control;
mod header;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod variable_pcm_tests;

#[cfg(test)]
mod response_measurement;

#[cfg(test)]
mod chunk_tests;

pub(super) use audio::{ParentAudio, WorkerAudio};
pub(super) use control::{ControlMailbox, WorkerControl};
pub(super) use header::SharedHeader;

/// One bounded state response chunk shared by the parent and worker mappings.
pub(super) const CHUNK_CAPACITY: usize = 256 * 1024;

/// Called once by the parent factory, after initializing a newly-created private transport.
pub(super) fn create(
    path: &Path,
    layout: AudioLayout,
) -> Result<(ParentAudio, ControlMailbox), String> {
    let length = shared_length(layout)?;
    for count in [layout.inputs, layout.outputs, layout.block_frames] {
        if u32::try_from(count).is_err() {
            return Err("processor transport dimensions exceed u32".to_owned());
        }
    }
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let result = (|| {
        file.set_len(
            u64::try_from(length)
                .map_err(|_| "processor transport length exceeds u64".to_owned())?,
        )
        .map_err(|error| error.to_string())?;
        initialize(&file, SharedHeader::new(layout))?;
        Ok((ParentAudio::open(&file)?, ControlMailbox::open(&file)?))
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Called once by the isolated worker entry point before starting any worker thread.
/// The parent creates exactly one child for this private transport path.
pub(super) fn worker(path: &Path) -> Result<(WorkerAudio, WorkerControl), String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.try_lock()
        .map_err(|error| format!("could not claim processor transport: {error}"))?;
    let lease = Arc::new(file);
    Ok((WorkerAudio::open(&lease)?, WorkerControl::open(&lease)?))
}

/// Owns one address-space view without ever borrowing the entire shared mapping.
/// Header atomics, the control chunk and the two PCM regions have independent ownership protocols.
struct Mapping {
    raw: MmapRaw,
    layout: AudioLayout,
    // The mapping closes before its share of the exclusive worker lease is released.
    _worker_lease: Option<Arc<File>>,
}

impl Mapping {
    fn open(file: &File, worker_lease: Option<Arc<File>>) -> Result<Self, String> {
        let raw = MmapRaw::map_raw(file).map_err(|error| error.to_string())?;
        if raw.len() < size_of::<SharedHeader>()
            || raw.as_ptr().align_offset(align_of::<SharedHeader>()) != 0
        {
            return Err("shared processor mapping is truncated or misaligned".to_owned());
        }
        // SAFETY: the private, fixed-size transport is initialized before either peer opens it.
        // Only this header is borrowed; its live mutable fields are atomics. PCM remains raw.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "The preceding admission check validates SharedHeader alignment."
        )]
        let header = unsafe { &*raw.as_ptr().cast::<SharedHeader>() };
        if header.magic != SHARED_MAGIC || header.version != SHARED_VERSION {
            return Err("shared processor mapping has an incompatible header".to_owned());
        }
        let layout = AudioLayout {
            inputs: header.input_channels as usize,
            outputs: header.output_channels as usize,
            block_frames: header.block_frames as usize,
        };
        if raw.len() != shared_length(layout)? {
            return Err("shared processor mapping has an invalid size".to_owned());
        }
        Ok(Self {
            raw,
            layout,
            _worker_lease: worker_lease,
        })
    }

    fn header(&self) -> &SharedHeader {
        // SAFETY: open validates header size/alignment. The owner retains the mapping; immutable
        // metadata never changes after creation, and every writable header field is atomic.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "Mapping admission validates SharedHeader alignment once before publication."
        )]
        unsafe {
            &*self.raw.as_ptr().cast::<SharedHeader>()
        }
    }

    fn input_samples(&self) -> usize {
        self.layout.inputs * self.layout.block_frames
    }

    fn input_sample_count(&self, frames: usize) -> Option<usize> {
        (frames <= self.layout.block_frames)
            .then_some(())
            .and_then(|()| self.layout.inputs.checked_mul(frames))
    }

    fn output_samples(&self) -> usize {
        self.layout.outputs * self.layout.block_frames
    }

    fn output_sample_count(&self, frames: usize) -> Option<usize> {
        (frames <= self.layout.block_frames)
            .then_some(())
            .and_then(|()| self.layout.outputs.checked_mul(frames))
    }

    fn output_offset(&self) -> usize {
        input_offset() + self.input_samples() * size_of::<f32>()
    }

    fn write_state_chunk(&self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= CHUNK_CAPACITY);
        // SAFETY: Mapping admission proves that the state chunk lies fully within this mapping.
        // The worker calls this only while state_written == state_consumed, so the parent has
        // finished reading the previous chunk. The release publication follows the copy.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.raw.as_mut_ptr().add(size_of::<SharedHeader>()),
                bytes.len(),
            );
        }
    }

    fn append_state_chunk(&self, bytes: &mut Vec<u8>, length: usize) {
        debug_assert!(length <= CHUNK_CAPACITY);
        // SAFETY: Mapping admission proves that the state chunk lies fully within this mapping.
        // The parent calls this only after acquiring state_written and before acknowledging the
        // same cumulative count through state_consumed, so the worker cannot overwrite it.
        let source = unsafe {
            std::slice::from_raw_parts(self.raw.as_ptr().add(size_of::<SharedHeader>()), length)
        };
        // The caller fallibly reserved this space before borrowing the published chunk.
        bytes.extend_from_slice(source);
    }
}

fn initialize(file: &File, header: SharedHeader) -> Result<(), String> {
    let raw = MmapRaw::map_raw(file).map_err(|error| error.to_string())?;
    if raw.len() < size_of::<SharedHeader>()
        || raw.as_ptr().align_offset(align_of::<SharedHeader>()) != 0
    {
        return Err("shared processor mapping is truncated or misaligned".to_owned());
    }
    // SAFETY: creation has not published the file to a worker or any control/audio owner. The
    // page-aligned region is large enough, and this is its only initialization before use.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "The preceding initialization check validates SharedHeader alignment."
    )]
    unsafe {
        raw.as_mut_ptr().cast::<SharedHeader>().write(header);
    }
    raw.flush().map_err(|error| error.to_string())
}

fn shared_length(layout: AudioLayout) -> Result<usize, String> {
    let samples = layout
        .inputs
        .checked_add(layout.outputs)
        .and_then(|channels| channels.checked_mul(layout.block_frames))
        .ok_or_else(|| "shared processor sample count overflowed".to_owned())?;
    input_offset()
        .checked_add(
            samples
                .checked_mul(size_of::<f32>())
                .ok_or_else(|| "shared processor byte count overflowed".to_owned())?,
        )
        .filter(|length| isize::try_from(*length).is_ok())
        .ok_or_else(|| "shared processor mapping size overflowed".to_owned())
}

const fn input_offset() -> usize {
    size_of::<SharedHeader>() + CHUNK_CAPACITY
}

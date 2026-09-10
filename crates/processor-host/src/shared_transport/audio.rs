use std::{
    fs::File,
    sync::{Arc, atomic::Ordering},
};

use crate::{AudioLayout, SharedHeader};

use super::{Mapping, input_offset};

/// The engine callback's non-cloneable producer/consumer endpoint.
pub(crate) struct ParentAudio {
    mapping: Mapping,
}

impl ParentAudio {
    pub(super) fn open(file: &File) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::open(file, None)?,
        })
    }

    pub(crate) fn header(&self) -> &SharedHeader {
        self.mapping.header()
    }

    #[cfg(test)]
    pub(crate) fn acknowledged_output(&self) -> u64 {
        self.header().output_consumed.load(Ordering::Acquire)
    }

    #[cfg(all(test, not(debug_assertions)))]
    pub(crate) fn acknowledge_benchmark_block(&self) {
        let header = self.header();
        let sequence = header.input_sequence.load(Ordering::Acquire);
        header.input_consumed.store(sequence, Ordering::Release);
        header.output_sequence.store(sequence, Ordering::Release);
    }

    pub(crate) fn submit(
        &mut self,
        sequence: u64,
        input: &[f32],
        frames: usize,
        before_publish: impl FnOnce(),
    ) -> bool {
        let mapping = &self.mapping;
        let header = mapping.header();
        let samples = mapping.layout.inputs * frames.min(mapping.layout.block_frames);
        if sequence == 0
            || frames > mapping.layout.block_frames
            || input.len() < samples
            || header.input_sequence.load(Ordering::Acquire)
                != header.input_consumed.load(Ordering::Acquire)
        {
            return false;
        }
        // SAFETY: this unique parent endpoint holds the input slot after acquiring the worker's
        // acknowledgment. The bounded borrow ends before the release publishes new input.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "Mapping admission validates header alignment; PCM offsets are multiples of f32 alignment."
        )]
        let destination = unsafe {
            std::slice::from_raw_parts_mut(
                mapping.raw.as_mut_ptr().add(input_offset()).cast::<f32>(),
                mapping.input_samples(),
            )
        };
        destination.fill(0.0);
        destination[..samples].copy_from_slice(&input[..samples]);
        before_publish();
        header.input_sequence.store(sequence, Ordering::Release);
        true
    }

    pub(crate) fn receive(&mut self, output: &mut [f32], frames: usize) -> Option<u64> {
        let mapping = &self.mapping;
        let header = mapping.header();
        let samples = mapping.layout.outputs * frames.min(mapping.layout.block_frames);
        let sequence = header.output_sequence.load(Ordering::Acquire);
        if frames > mapping.layout.block_frames
            || output.len() < samples
            || sequence == 0
            || sequence == header.output_consumed.load(Ordering::Relaxed)
        {
            return None;
        }
        // SAFETY: the worker's release grants this unique consumer the output slot until this
        // method acknowledges it. Only output bytes are borrowed, never peer-owned input/state.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "Mapping admission validates header alignment; PCM offsets are multiples of f32 alignment."
        )]
        let source = unsafe {
            std::slice::from_raw_parts(
                mapping
                    .raw
                    .as_ptr()
                    .add(mapping.output_offset())
                    .cast::<f32>(),
                samples,
            )
        };
        output[..samples].copy_from_slice(source);
        header.output_consumed.store(sequence, Ordering::Release);
        Some(sequence)
    }
}

/// The worker's single DSP endpoint retains its captured sequence while processing privately.
pub(crate) struct WorkerAudio {
    mapping: Mapping,
    output_sequence: Option<u64>,
}

impl WorkerAudio {
    pub(super) fn open(file: &Arc<File>) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::open(file, Some(Arc::clone(file)))?,
            output_sequence: None,
        })
    }

    pub(crate) fn header(&self) -> &SharedHeader {
        self.mapping.header()
    }
    pub(crate) fn layout(&self) -> AudioLayout {
        self.mapping.layout
    }

    pub(crate) fn input_pending(&self) -> bool {
        let header = self.header();
        let sequence = header.input_sequence.load(Ordering::Acquire);
        self.output_sequence.is_none()
            && sequence != 0
            && sequence != header.input_consumed.load(Ordering::Relaxed)
            && header.output_sequence.load(Ordering::Acquire)
                == header.output_consumed.load(Ordering::Acquire)
    }

    pub(crate) fn receive(&mut self, copy: impl FnOnce(&[f32])) -> bool {
        if !self.input_pending() {
            return false;
        }
        let mapping = &self.mapping;
        let header = mapping.header();
        let sequence = header.input_sequence.load(Ordering::Acquire);
        // SAFETY: input_pending acquired the parent's publication and reserved free output.
        // This sole worker cannot consume another input before publishing the captured output.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "Mapping admission validates header alignment; PCM offsets are multiples of f32 alignment."
        )]
        let source = unsafe {
            std::slice::from_raw_parts(
                mapping.raw.as_ptr().add(input_offset()).cast::<f32>(),
                mapping.input_samples(),
            )
        };
        copy(source);
        self.output_sequence = Some(sequence);
        header.input_consumed.store(sequence, Ordering::Release);
        true
    }

    pub(crate) fn publish(&mut self, copy: impl FnOnce(&mut [f32])) -> bool {
        let Some(sequence) = self.output_sequence else {
            return false;
        };
        let mapping = &self.mapping;
        // SAFETY: receive reserved this output slot after acquiring the parent's acknowledgment.
        // The unique worker holds it across private DSP; the slice ends before publication.
        #[expect(
            clippy::cast_ptr_alignment,
            reason = "Mapping admission validates header alignment; PCM offsets are multiples of f32 alignment."
        )]
        let destination = unsafe {
            std::slice::from_raw_parts_mut(
                mapping
                    .raw
                    .as_mut_ptr()
                    .add(mapping.output_offset())
                    .cast::<f32>(),
                mapping.output_samples(),
            )
        };
        copy(destination);
        mapping
            .header()
            .output_sequence
            .store(sequence, Ordering::Release);
        self.output_sequence = None;
        true
    }
}

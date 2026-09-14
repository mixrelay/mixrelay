use super::{
    NativeAudioConfig,
    callback_source::{HalRender, RenderTransport},
    callback_transport::{CaptureTransport, HalCapture, NativeCapture, recover_capture_epoch},
};
use mixrelay_audio_engine::{ExternalBinding, ExternalControlState, RenderPlan};
use mixrelay_audio_io::{
    VirtualEndpointId,
    process::{InputActivity, ProcessReport, TransferOutcome},
};
use mixrelay_macos_transport::endpoint_identity::parse_endpoint_id;

/// Control-prepared ownership for one canonical device. No graph reference owns
/// a second converter or a second capture writer for the same device.
pub(super) struct EndpointBoundary<T> {
    #[cfg(test)]
    pub(super) work: BoundaryWork,
    pub(super) identity: VirtualEndpointId,
    pub(super) id: [u8; 16],
    pub(super) transport: T,
    source: Option<HalRender>,
    sink: Option<HalCapture>,
    render_channels: usize,
    capture_channels: usize,
    render_pcm: Vec<f32>,
    capture_pcm: Vec<f32>,
    source_result: TransferOutcome,
    directions: Directions,
    active: Directions,
    source_indices: Vec<usize>,
    sink_indices: Vec<usize>,
    begun: Directions,
    native_frames: u32,
    frames: u32,
    observed_running: Option<bool>,
    format: BoundaryFormat,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct BoundaryWork {
    pub(super) source_begin: u64,
    pub(super) source_read: u64,
    pub(super) sink_begin: u64,
    pub(super) sink_push: u64,
    pub(super) sink_flush: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Directions {
    pub(super) source: bool,
    pub(super) sink: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BoundaryFormat {
    graph_rate: u32,
    device_rate: u32,
    capacity: u32,
}

pub(super) struct BoundaryConfig {
    pub(super) identity: VirtualEndpointId,
    pub(super) id: [u8; 16],
    pub(super) audio: NativeAudioConfig,
    pub(super) graph_rate: u32,
    pub(super) device_rate: u32,
    pub(super) directions: Directions,
}

impl<T: CaptureTransport + RenderTransport> EndpointBoundary<T> {
    pub(super) fn directions(&self) -> Directions {
        self.directions
    }

    pub(super) fn new(config: BoundaryConfig, transport: T) -> Result<Self, String> {
        let BoundaryConfig {
            identity,
            id,
            audio,
            graph_rate,
            device_rate,
            directions,
        } = config;
        let source = (directions.source && device_rate != 0)
            .then(|| HalRender::new(graph_rate, device_rate, audio))
            .transpose()?;
        let sink = (directions.sink && device_rate != 0)
            .then(|| HalCapture::new(graph_rate, device_rate, audio))
            .transpose()?;
        Ok(Self {
            #[cfg(test)]
            work: BoundaryWork::default(),
            identity,
            id,
            transport,
            source,
            sink,
            directions,
            active: directions,
            source_indices: Vec::new(),
            sink_indices: Vec::new(),
            begun: Directions::default(),
            native_frames: 0,
            render_channels: audio.render_channels as usize,
            capture_channels: audio.capture_channels as usize,
            render_pcm: vec![0.0; audio.render_channels as usize * 4096],
            capture_pcm: vec![0.0; audio.capture_channels as usize * 4096],
            source_result: TransferOutcome::complete(0),
            frames: 0,
            observed_running: None,
            format: BoundaryFormat {
                graph_rate,
                device_rate,
                capacity: audio.capacity_frames,
            },
        })
    }

    fn begin(&mut self, frames: u32) {
        self.transport.begin_callback();
        self.observed_running = self.transport.clock().map(|clock| clock.running != 0);
        self.native_frames = frames;
        self.begun = Directions::default();
    }

    fn set_activity(&mut self, active: Directions) {
        if self.active.source && !active.source {
            if let Some(source) = &mut self.source {
                source.suspend();
            }
            self.begun.source = false;
        }
        if self.active.sink && !active.sink {
            if let Some(sink) = &mut self.sink {
                sink.suspend(&mut self.transport);
            }
            self.begun.sink = false;
        }
        self.active = active;
    }

    fn begin_active(&mut self) {
        if self.active.source
            && !self.begun.source
            && let Some(source) = &mut self.source
        {
            #[cfg(test)]
            {
                self.work.source_begin += 1;
            }
            source.begin(&mut self.transport);
            self.begun.source = true;
        }
        if self.active.sink
            && !self.begun.sink
            && let Some(sink) = &mut self.sink
        {
            #[cfg(test)]
            {
                self.work.sink_begin += 1;
            }
            sink.begin(&mut self.transport, self.native_frames);
            self.begun.sink = true;
        }
    }

    fn prepare_block(&mut self, frames: u32) {
        self.begin_active();
        self.frames = frames;
        let render = &mut self.render_pcm[..frames as usize * self.render_channels];
        render.fill(0.0);
        self.source_result = self
            .source
            .as_mut()
            .filter(|_| self.active.source)
            .map_or_else(unavailable, |source| {
                #[cfg(test)]
                {
                    self.work.source_read += 1;
                }
                source.read(render)
            });
        self.capture_pcm[..frames as usize * self.capture_channels].fill(0.0);
    }

    fn finish_block(&mut self) -> ProcessReport {
        if !self.active.sink {
            return ProcessReport::default();
        }
        let Some(sink) = &mut self.sink else {
            return ProcessReport::default();
        };
        let pcm = &self.capture_pcm[..self.frames as usize * self.capture_channels];
        #[cfg(test)]
        {
            self.work.sink_push += 1;
            self.work.sink_flush += 1;
        }
        let pushed = sink.push(pcm, self.frames);
        let flushed = sink.flush(&mut self.transport);
        let mut report = sink.take_report();
        report.output_overflows += u64::from(pushed.frames != self.frames || !flushed);
        report.discontinuities += u64::from(pushed.discontinuity || !flushed);
        report
    }

    fn pending(&self) -> bool {
        (self.active.source && self.source.as_ref().is_none_or(HalRender::reconfigure))
            || (self.active.sink && self.sink.as_ref().is_none_or(HalCapture::reconfigure))
    }

    fn synchronized(&self) -> bool {
        // A stopped native clock moves no PCM. Pending control reconfiguration
        // is not drift in another device's currently running branch.
        if self.observed_running == Some(false) {
            return true;
        }
        (!self.active.source || self.source.as_ref().is_some_and(HalRender::synchronized))
            && (!self.active.sink || self.sink.as_ref().is_some_and(HalCapture::synchronized))
    }

    fn discontinuity(&mut self) {
        if let Some(source) = &mut self.source {
            source.source_discontinuity();
        }
        if let Some(sink) = &mut self.sink {
            sink.source_discontinuity();
        }
    }
}

/// Callback borrows this preallocated bank; replacement and destruction occur
/// only after the descriptor reader drain on its control owner.
pub(super) struct EndpointBoundaries<T> {
    pub(super) endpoints: Vec<EndpointBoundary<T>>,
}

impl<T: CaptureTransport + RenderTransport> EndpointBoundaries<T> {
    pub(super) fn empty() -> Self {
        Self {
            endpoints: Vec::new(),
        }
    }

    pub(super) fn begin(&mut self, frames: u32) {
        for endpoint in &mut self.endpoints {
            endpoint.begin(frames);
        }
    }

    pub(super) fn prepare_block(&mut self, frames: u32, controls: ExternalControlState<'_>) {
        self.set_controls(controls);
        self.prepare_active_block(frames);
    }

    pub(super) fn set_controls(&mut self, controls: ExternalControlState<'_>) {
        for endpoint in &mut self.endpoints {
            endpoint.set_activity(Directions {
                source: endpoint
                    .source_indices
                    .iter()
                    .any(|&index| controls.source_enabled(index) == Some(true)),
                sink: endpoint
                    .sink_indices
                    .iter()
                    .any(|&index| controls.sink_enabled(index) == Some(true)),
            });
        }
    }

    pub(super) fn prepare_active_block(&mut self, frames: u32) {
        for endpoint in &mut self.endpoints {
            endpoint.prepare_block(frames);
        }
    }

    /// Compile reference indices on the control owner, never in an audio cycle.
    pub(super) fn bind_controls(&mut self, plan: &RenderPlan) {
        for endpoint in &mut self.endpoints {
            let indices = |bindings: &[ExternalBinding]| {
                bindings
                    .iter()
                    .enumerate()
                    .filter_map(|(index, binding)| {
                        (binding.virtual_endpoint_id() == Some(&endpoint.identity)).then_some(index)
                    })
                    .collect()
            };
            endpoint.source_indices = indices(plan.source_bindings());
            endpoint.sink_indices = indices(plan.sink_bindings());
        }
    }

    pub(super) fn finish_block(&mut self) -> ProcessReport {
        let mut total = ProcessReport::default();
        for endpoint in &mut self.endpoints {
            let report = endpoint.finish_block();
            total.input_starvations += report.input_starvations;
            total.output_overflows += report.output_overflows;
            total.discontinuities += report.discontinuities;
        }
        total
    }

    pub(super) fn read(
        &self,
        binding: &ExternalBinding,
        pcm: &mut [f32],
    ) -> Option<TransferOutcome> {
        let id = binding.virtual_endpoint_id()?;
        let Some(endpoint) = self
            .endpoints
            .iter()
            .find(|endpoint| &endpoint.identity == id)
        else {
            pcm.fill(0.0);
            return Some(unavailable());
        };
        let count = endpoint.frames as usize * endpoint.render_channels;
        if pcm.len() != count {
            pcm.fill(0.0);
            return Some(unavailable());
        }
        pcm.copy_from_slice(&endpoint.render_pcm[..count]);
        Some(endpoint.source_result)
    }

    pub(super) fn write(
        &mut self,
        binding: &ExternalBinding,
        pcm: &[f32],
    ) -> Option<TransferOutcome> {
        let id = binding.virtual_endpoint_id()?;
        let Some(endpoint) = self
            .endpoints
            .iter_mut()
            .find(|endpoint| &endpoint.identity == id)
        else {
            return Some(unavailable());
        };
        let count = endpoint.frames as usize * endpoint.capture_channels;
        if pcm.len() != count {
            return Some(unavailable());
        }
        for (destination, sample) in endpoint.capture_pcm[..count].iter_mut().zip(pcm) {
            *destination += sample;
        }
        Some(TransferOutcome::complete(endpoint.frames))
    }

    pub(super) fn pending(&self) -> bool {
        self.endpoints.iter().any(EndpointBoundary::pending)
    }
    pub(super) fn synchronized(&self) -> bool {
        self.endpoints.iter().all(EndpointBoundary::synchronized)
    }
    pub(super) fn discontinuity(&mut self) {
        for endpoint in &mut self.endpoints {
            endpoint.discontinuity();
        }
    }

    /// Reconcile on the sole control/worker owner, never within its callback.
    /// Retained owners keep their PCM queues; only removed or changed writers
    /// are invalidated. Unused prepared candidates have never published PCM.
    pub(super) fn reconcile(&mut self, mut replacement: Self, mut retire: impl FnMut([u8; 16])) {
        for mut current in self.endpoints.drain(..) {
            let retained = replacement.endpoints.iter_mut().find(|candidate| {
                current.id == candidate.id
                    && current.directions == candidate.directions
                    && current.format == candidate.format
                    && current.render_channels == candidate.render_channels
                    && current.capture_channels == candidate.capture_channels
                    && !current.pending()
            });
            if let Some(candidate) = retained {
                std::mem::swap(&mut current.source_indices, &mut candidate.source_indices);
                std::mem::swap(&mut current.sink_indices, &mut candidate.sink_indices);
                std::mem::swap(candidate, &mut current);
            } else if current.directions.sink {
                retire(current.id);
            }
        }
        *self = replacement;
    }
}

impl EndpointBoundaries<NativeCapture> {
    pub(super) fn prepare(plan: &RenderPlan) -> Result<Self, String> {
        let mut requests = std::collections::BTreeMap::<VirtualEndpointId, Directions>::new();
        for (source, bindings) in [
            (true, plan.source_bindings()),
            (false, plan.sink_bindings()),
        ] {
            for binding in bindings {
                if let Some(id) = binding.virtual_endpoint_id() {
                    let directions = requests.entry(id.clone()).or_default();
                    if source {
                        directions.source = true;
                    } else {
                        directions.sink = true;
                    }
                }
            }
        }
        let mut endpoints = Vec::with_capacity(requests.len());
        for (identity, directions) in requests {
            let id = parse_endpoint_id(identity.as_str())
                .ok_or_else(|| format!("invalid virtual device identity: {}", identity.as_str()))?;
            let audio =
                mixrelay_engine_service::endpoint_service::audio_config(id).ok_or_else(|| {
                    format!(
                        "virtual device transport is not prepared: {}",
                        identity.as_str()
                    )
                })?;
            let device_rate = super::native_transport::snapshot(id)
                .map_or(0, |snapshot| snapshot.clock.sample_rate);
            endpoints.push(EndpointBoundary::new(
                BoundaryConfig {
                    identity,
                    id,
                    audio,
                    graph_rate: plan.format().sample_rate,
                    device_rate,
                    directions,
                },
                NativeCapture::new(id),
            )?);
        }
        let mut result = Self { endpoints };
        result.bind_controls(plan);
        Ok(result)
    }

    pub(super) fn retire(&self) {
        for endpoint in &self.endpoints {
            if endpoint.directions.sink {
                recover_capture_epoch(endpoint.id);
            }
        }
    }

    /// Called only with the renderer descriptor drained. Preserve healthy
    /// endpoint history when another device changes its rate or mapping.
    pub(super) fn replace_pending(&mut self, mut replacement: Self) -> Self {
        for candidate in &mut replacement.endpoints {
            if let Some(current) = self
                .endpoints
                .iter_mut()
                .find(|endpoint| endpoint.id == candidate.id)
                && current.pending()
            {
                std::mem::swap(current, candidate);
            }
        }
        replacement
    }

    pub(super) fn recover_pending(&self) {
        for endpoint in &self.endpoints {
            if endpoint.pending() && endpoint.directions.sink {
                recover_capture_epoch(endpoint.id);
            }
        }
    }

    pub(super) fn capture_ids(&self) -> Vec<[u8; 16]> {
        self.endpoints
            .iter()
            .filter(|endpoint| endpoint.directions.sink)
            .map(|endpoint| endpoint.id)
            .collect()
    }
}

fn unavailable() -> TransferOutcome {
    TransferOutcome {
        frames: 0,
        discontinuity: false,
        activity: InputActivity::Unknown,
    }
}

#[cfg(test)]
mod tests;

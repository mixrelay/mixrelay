//! Control-thread matching and audio-boundary ownership transfer for surviving plug-ins.

use crate::{ProcessorControlRegistry, WORKER_READY};
use mixrelay_audio_engine::{HostedAudioProcessor, HostedProcessorPrepareRequest};
use mixrelay_audio_graph::{ProviderNodeConfig, RenderFormat};
use std::sync::atomic::Ordering;

pub(super) struct ReuseSignature {
    config: ProviderNodeConfig,
    format: RenderFormat,
    inputs: u16,
    outputs: u16,
}

impl ReuseSignature {
    pub(super) fn new(request: &HostedProcessorPrepareRequest<'_>) -> Self {
        Self {
            config: request.config.clone(),
            format: request.format,
            inputs: request.input_channels,
            outputs: request.output_channels,
        }
    }

    fn matches(&self, request: &HostedProcessorPrepareRequest<'_>) -> bool {
        self.config == *request.config
            && self.format == request.format
            && self.inputs == request.input_channels
            && self.outputs == request.output_channels
    }

    pub(super) fn checkpoint(&mut self, parameters: &serde_json::Value) {
        self.config.parameters = parameters.clone();
    }
}

struct ReuseRequest(u64);

impl HostedAudioProcessor for ReuseRequest {
    fn process(&mut self, _input: &[f32], _output: &mut [f32], _frames: usize) -> bool {
        false
    }

    fn reuse_identity(&self) -> Option<u64> {
        Some(self.0)
    }

    fn requests_reuse(&self) -> bool {
        true
    }
}

impl ProcessorControlRegistry {
    pub(super) fn reuse_request(
        &self,
        request: &HostedProcessorPrepareRequest<'_>,
    ) -> Option<Box<dyn HostedAudioProcessor>> {
        let control = self.live_control(request.node_id).ok()?;
        let header = control.shared.header();
        if !control.active_owner.load(Ordering::Acquire)
            || header.worker_status.load(Ordering::Acquire) != WORKER_READY
            || header.shutdown.load(Ordering::Acquire) != 0
        {
            return None;
        }
        let signature = control.reuse.lock().ok()?;
        signature
            .as_ref()
            .filter(|signature| signature.matches(request))?;
        Some(Box::new(ReuseRequest(header.instance_id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_audio_graph::NodeId;

    #[test]
    fn reuse_requires_matching_type_format_channels_and_current_native_state() {
        let id = NodeId::from("graph-a/effect");
        let config = ProviderNodeConfig {
            type_id: "au:aufx:bpas:appl".into(),
            parameters: serde_json::json!({"stateBase64": "original"}),
        };
        let mut request = HostedProcessorPrepareRequest {
            node_id: &id,
            config: &config,
            format: RenderFormat {
                sample_rate: 48000,
                block_frames: 32,
            },
            input_channels: 2,
            output_channels: 2,
        };
        let mut signature = ReuseSignature::new(&request);
        assert!(signature.matches(&request));
        request.format.sample_rate = 96000;
        assert!(!signature.matches(&request));
        request.format.sample_rate = 48000;
        request.format.block_frames = 64;
        assert!(!signature.matches(&request));
        request.format.block_frames = 32;
        request.input_channels = 1;
        assert!(!signature.matches(&request));
        request.input_channels = 2;
        signature.checkpoint(&serde_json::json!({"stateBase64": "edited"}));
        assert!(!signature.matches(&request));
        let mut updated = config.clone();
        updated.parameters = serde_json::json!({"stateBase64": "edited"});
        request.config = &updated;
        assert!(signature.matches(&request));
        let mut other = updated.clone();
        other.type_id = "vst3:another".into();
        request.config = &other;
        assert!(!signature.matches(&request));
    }
}

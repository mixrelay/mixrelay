//! Canonical graph-referenced endpoint inventory and ID-directed native control.
use mixrelay_audio_io::{
    HostAudioBackendError, VirtualEndpointBackend, VirtualEndpointCapabilities, VirtualEndpointSpec,
};
use mixrelay_engine_service::endpoint_service;
use mixrelay_macos_transport::endpoint_identity::{
    endpoint_uid, format_endpoint_id, parse_endpoint_id,
};
use std::sync::atomic::{AtomicBool, Ordering};
pub(super) const MAX_ENDPOINT_CHANNELS: u16 = 64;
pub(super) struct MacVirtualEndpointBackend {
    capabilities: VirtualEndpointCapabilities,
    staged: AtomicBool,
}
impl MacVirtualEndpointBackend {
    pub(super) fn new(rates: Vec<u32>) -> Self {
        Self {
            capabilities: VirtualEndpointCapabilities {
                max_render_channels: MAX_ENDPOINT_CHANNELS,
                max_capture_channels: MAX_ENDPOINT_CHANNELS,
                supported_sample_rates: rates,
                supports_duplex: true,
                supports_dynamic_creation: true,
            },
            staged: AtomicBool::new(false),
        }
    }
    pub(super) fn activate_pending(&self) -> Result<bool, String> {
        if !self.staged.load(Ordering::Acquire) {
            return Ok(false);
        }
        // Called only inside the processor exchange's configured activation gate.
        if !mixrelay_engine_service::endpoint_service::activate_prepared_endpoints() {
            return Err("native endpoint inventory activation failed".into());
        }
        Ok(self.staged.swap(false, Ordering::AcqRel))
    }
    pub(super) fn activate_recovery() -> bool {
        endpoint_service::activate_recovered_endpoints()
    }
}
fn endpoint_identity(
    id: &mixrelay_audio_io::VirtualEndpointId,
) -> Result<[u8; 16], HostAudioBackendError> {
    parse_endpoint_id(id.as_str()).ok_or_else(|| HostAudioBackendError::CapabilityUnavailable {
        capability: format!(
            "virtual endpoint identity must be a canonical lowercase UUID: {}",
            id.as_str()
        ),
    })
}
impl VirtualEndpointBackend for MacVirtualEndpointBackend {
    fn cancel_prepared_graph_attachments(&self) {
        if self.staged.swap(false, Ordering::AcqRel) {
            mixrelay_engine_service::endpoint_service::cancel_prepared_endpoints();
        }
    }
    fn settings(&self) -> Option<mixrelay_audio_io::VirtualEndpointSettings> {
        let mut settings =
            mixrelay_audio_io::VirtualEndpointSettings::configurable(&self.capabilities);
        settings.creation_template = Some(mixrelay_audio_io::VirtualEndpointCreationTemplate {
            render: stream(2),
            capture: stream(2),
            supported_sample_rates: self.capabilities.supported_sample_rates.clone(),
            default_sample_rate: 48_000,
        });
        settings.devices = endpoint_service::endpoint_specs()
            .iter()
            .map(device_settings)
            .collect();
        Some(settings)
    }
    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.capabilities
    }
    fn sample_rate(
        &self,
        id: &mixrelay_audio_io::VirtualEndpointId,
    ) -> Result<u32, HostAudioBackendError> {
        mixrelay_engine_service::endpoint_service::device_sample_rate(endpoint_identity(id)?)
            .ok_or_else(|| HostAudioBackendError::BackendFailure {
                message: format!(
                    "virtual device {} has no native rate observation",
                    id.as_str()
                ),
            })
    }
    fn set_sample_rate(
        &self,
        id: &mixrelay_audio_io::VirtualEndpointId,
        rate: u32,
    ) -> Result<(), HostAudioBackendError> {
        if mixrelay_engine_service::endpoint_service::set_device_sample_rate(
            endpoint_identity(id)?,
            rate,
        ) {
            Ok(())
        } else {
            Err(HostAudioBackendError::BackendFailure {
                message: format!("virtual device {} rejected sample rate {rate}", id.as_str()),
            })
        }
    }
    fn prepare_graph_attachments(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        if self.staged.load(Ordering::Acquire) {
            return Err(HostAudioBackendError::BackendFailure {
                message: "an endpoint inventory is already staged".into(),
            });
        }
        let specs = desired
            .iter()
            .map(|endpoint| {
                Ok(mixrelay_engine_service::endpoint_registry::EndpointSpec {
                    id: endpoint_identity(&endpoint.id)?,
                    display_name: endpoint.display_name.clone(),
                    sample_rate: endpoint.default_sample_rate,
                    capacity_frames: 16_384,
                    render_channels: endpoint
                        .render
                        .map_or(0, |stream| u32::from(stream.channels)),
                    capture_channels: endpoint
                        .capture
                        .map_or(0, |stream| u32::from(stream.channels)),
                })
            })
            .collect::<Result<Vec<_>, HostAudioBackendError>>()?;
        mixrelay_engine_service::endpoint_service::prepare_endpoints(&specs).map_err(|error| {
            HostAudioBackendError::BackendFailure {
                message: format!("endpoint inventory preparation failed: {error}"),
            }
        })?;
        self.staged.store(true, Ordering::Release);
        Ok(())
    }
}

fn stream(channels: u16) -> Option<mixrelay_audio_io::StreamSpec> {
    use mixrelay_audio_io::{ChannelLayout, StreamSpec};
    (channels != 0).then_some(StreamSpec {
        channels,
        channel_layout: match channels {
            1 => ChannelLayout::Mono,
            2 => ChannelLayout::Stereo,
            _ => ChannelLayout::Discrete,
        },
    })
}
fn device_settings(
    native: &mixrelay_engine_service::endpoint_registry::EndpointSpec,
) -> mixrelay_audio_io::VirtualEndpointDeviceSettings {
    let observed_sample_rate = endpoint_service::device_sample_rate(native.id);
    let supported_sample_rates = mixrelay_audio_io::format_contract::supported_rates(|rate| {
        endpoint_service::device_supports_sample_rate(native.id, rate)
    });
    observed_device_settings(native, observed_sample_rate, supported_sample_rates)
}
fn observed_device_settings(
    native: &mixrelay_engine_service::endpoint_registry::EndpointSpec,
    observed_sample_rate: Option<u32>,
    supported_sample_rates: Vec<u32>,
) -> mixrelay_audio_io::VirtualEndpointDeviceSettings {
    mixrelay_audio_io::VirtualEndpointDeviceSettings {
        native_device_ids: vec![endpoint_uid(native.id)],
        spec: VirtualEndpointSpec {
            id: mixrelay_audio_io::VirtualEndpointId::new(format_endpoint_id(native.id)),
            display_name: native.display_name.clone(),
            render: stream(
                u16::try_from(native.render_channels)
                    .expect("registry validates at most 64 channels"),
            ),
            capture: stream(
                u16::try_from(native.capture_channels)
                    .expect("registry validates at most 64 channels"),
            ),
            default_sample_rate: native.sample_rate,
            supported_sample_rates: supported_sample_rates.clone(),
        },
        can_set_sample_rate: observed_sample_rate.is_some() && !supported_sample_rates.is_empty(),
        observed_sample_rate,
        supported_sample_rates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_engine_service::endpoint_registry::EndpointSpec;

    #[test]
    fn native_boundary_accepts_only_canonical_uuid_not_names_or_default_aliases() {
        let text = "17e59d73-42c1-4f11-a8b4-6c2d913357e0";
        let id = endpoint_identity(&text.into()).unwrap();
        assert_eq!(format_endpoint_id(id), text);
        for invalid in [
            "mixrelay.virtual-io",
            "default",
            "17E59D73-42c1-4f11-a8b4-6c2d913357e0",
        ] {
            assert!(matches!(
                endpoint_identity(&invalid.into()),
                Err(HostAudioBackendError::CapabilityUnavailable { .. })
            ));
        }
    }

    #[test]
    fn per_device_settings_preserve_native_identity_name_geometry_and_actual_rate() {
        let native = EndpointSpec {
            id: [7; 16],
            display_name: "Shared studio".into(),
            sample_rate: 48_000,
            capacity_frames: 64,
            render_channels: 7,
            capture_channels: 0,
        };
        let settings = observed_device_settings(&native, Some(44_100), vec![44_100, 48_000]);
        assert_eq!(endpoint_identity(&settings.spec.id).unwrap(), native.id);
        assert_eq!(settings.spec.display_name, native.display_name);
        assert_eq!(settings.spec.render.unwrap().channels, 7);
        assert_eq!(settings.spec.capture, None);
        assert_eq!(settings.spec.default_sample_rate, 48_000);
        assert_eq!(settings.observed_sample_rate, Some(44_100));
        assert!(settings.can_set_sample_rate);
        let unavailable = observed_device_settings(&native, None, Vec::new());
        assert!(!unavailable.can_set_sample_rate);
        assert_eq!(unavailable.observed_sample_rate, None);
        assert_eq!(unavailable.spec.id, settings.spec.id);
        assert_eq!(settings.native_device_ids, vec![endpoint_uid(native.id)]);
        assert_eq!(unavailable.native_device_ids, settings.native_device_ids);
    }

    #[test]
    fn creation_template_has_no_implicitly_existing_native_identity() {
        let backend =
            MacVirtualEndpointBackend::new(mixrelay_audio_io::formats::SAMPLE_RATES.to_vec());
        let settings = backend.settings().unwrap();
        let template = settings.creation_template.unwrap();
        assert_eq!(template.render.unwrap().channels, 2);
        assert_eq!(template.capture.unwrap().channels, 2);
        assert_eq!(template.default_sample_rate, 48_000);
        assert_eq!(settings.render_channels.last(), Some(&64));
        assert_eq!(settings.capture_channels.last(), Some(&64));
    }
}

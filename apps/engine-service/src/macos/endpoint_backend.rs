//! Driver control reads actual devices; graph attachments own only `CoreAudio` clients.
use mixrelay_audio_io::{
    HostAudioBackendError, VirtualEndpointBackend, VirtualEndpointCapabilities,
    VirtualEndpointDeviceSettings, VirtualEndpointId, VirtualEndpointSpec,
};
use mixrelay_driver_control::{DriverClient, DriverCommand, DriverSnapshot};
use mixrelay_macos_transport::endpoint_identity::parse_endpoint_id;

pub(super) const MAX_ENDPOINT_CHANNELS: u16 = 64;

pub(super) struct MacVirtualEndpointBackend {
    capabilities: VirtualEndpointCapabilities,
}

impl MacVirtualEndpointBackend {
    /// Declares product channel/rate limits without starting Driver or graph IO.
    pub(super) fn new(rates: Vec<u32>) -> Self {
        Self {
            capabilities: VirtualEndpointCapabilities {
                max_render_channels: MAX_ENDPOINT_CHANNELS,
                max_capture_channels: MAX_ENDPOINT_CHANNELS,
                supported_sample_rates: rates,
                supports_duplex: true,
                supports_dynamic_creation: true,
            },
        }
    }

    /// Reads actual Driver settings on a non-async control worker.
    /// # Errors
    /// Returns service transport or native discovery failures; never call on DSP.
    pub(super) fn snapshot() -> Result<DriverSnapshot, HostAudioBackendError> {
        DriverClient::new()
            .and_then(|client| client.snapshot_blocking())
            .map_err(backend_error)
    }
}

fn backend_error(error: impl std::fmt::Display) -> HostAudioBackendError {
    HostAudioBackendError::BackendFailure {
        message: error.to_string(),
    }
}

fn device<'a>(
    snapshot: &'a DriverSnapshot,
    id: &VirtualEndpointId,
) -> Result<&'a VirtualEndpointDeviceSettings, HostAudioBackendError> {
    if parse_endpoint_id(id.as_str()).is_none() {
        return Err(HostAudioBackendError::CapabilityUnavailable {
            capability: format!(
                "virtual endpoint identity must be a canonical lowercase UUID: {}",
                id.as_str()
            ),
        });
    }
    snapshot
        .settings
        .devices
        .iter()
        .find(|device| device.spec.id == *id)
        .ok_or_else(|| backend_error(format!("Driver device {} is absent", id.as_str())))
}

fn resolve(
    snapshot: &DriverSnapshot,
    desired: &[VirtualEndpointSpec],
) -> Result<Vec<VirtualEndpointSpec>, HostAudioBackendError> {
    desired
        .iter()
        .map(|reference| {
            let actual = device(snapshot, &reference.id)?;
            if (reference.render.is_some() && actual.spec.render.is_none())
                || (reference.capture.is_some() && actual.spec.capture.is_none())
            {
                return Err(backend_error(format!(
                    "Driver device {} lacks a referenced direction",
                    reference.id.as_str()
                )));
            }
            let mut spec = actual.spec.clone();
            // Only referenced directions open graph clients; geometry belongs to Driver.
            if reference.render.is_none() {
                spec.render = None;
            }
            if reference.capture.is_none() {
                spec.capture = None;
            }
            Ok(spec)
        })
        .collect()
}

impl VirtualEndpointBackend for MacVirtualEndpointBackend {
    fn settings(&self) -> Option<mixrelay_audio_io::VirtualEndpointSettings> {
        Self::snapshot().ok().map(|snapshot| snapshot.settings)
    }

    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.capabilities
    }

    fn sample_rate(&self, id: &VirtualEndpointId) -> Result<u32, HostAudioBackendError> {
        device(&Self::snapshot()?, id)?
            .observed_sample_rate
            .ok_or_else(|| {
                backend_error(format!(
                    "Driver device {} has no native rate observation",
                    id.as_str()
                ))
            })
    }

    fn set_sample_rate(
        &self,
        id: &VirtualEndpointId,
        rate: u32,
    ) -> Result<(), HostAudioBackendError> {
        let client = DriverClient::new().map_err(backend_error)?;
        let snapshot = client.snapshot_blocking().map_err(backend_error)?;
        let actual = device(&snapshot, id)?;
        if !actual.can_set_sample_rate || !actual.supported_sample_rates.contains(&rate) {
            return Err(backend_error(format!(
                "Driver device {} does not support rate {rate}",
                id.as_str()
            )));
        }
        let mut spec = actual.spec.clone();
        spec.default_sample_rate = rate;
        let result = client
            .mutate_blocking(&DriverCommand::Configure {
                expected_revision: snapshot.revision,
                spec,
            })
            .map_err(backend_error)?;
        if device(&result, id)?.observed_sample_rate != Some(rate) {
            return Err(backend_error(
                "Driver rate readback differs from requested rate",
            ));
        }
        Ok(())
    }

    fn resolve_graph_devices(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<Vec<VirtualEndpointSpec>, HostAudioBackendError> {
        if desired.is_empty() {
            return Ok(Vec::new());
        }
        resolve(&Self::snapshot()?, desired)
    }

    fn prepare_graph_attachments(
        &self,
        desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        self.resolve_graph_devices(desired).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_driver_control::DriverState;

    #[test]
    fn graphs_without_virtual_endpoints_require_no_driver_connection() {
        let backend = MacVirtualEndpointBackend::new(vec![48_000]);
        assert!(backend.resolve_graph_devices(&[]).unwrap().is_empty());
        backend.prepare_graph_attachments(&[]).unwrap();
    }

    #[test]
    fn driver_resolution_uses_actual_geometry_without_retargeting_identity() {
        let capabilities = MacVirtualEndpointBackend::new(vec![48_000]).capabilities;
        let mut settings = mixrelay_audio_io::VirtualEndpointSettings::configurable(&capabilities);
        let actual = VirtualEndpointSpec {
            id: "01010101-0101-0101-0101-010101010101".into(),
            display_name: "Driver-owned device".into(),
            render: Some(mixrelay_audio_io::StreamSpec {
                channels: 7,
                channel_layout: mixrelay_audio_io::ChannelLayout::Discrete,
            }),
            capture: Some(mixrelay_audio_io::StreamSpec {
                channels: 3,
                channel_layout: mixrelay_audio_io::ChannelLayout::Discrete,
            }),
            supported_sample_rates: vec![48_000],
            default_sample_rate: 48_000,
        };
        settings.devices.push(VirtualEndpointDeviceSettings {
            spec: actual.clone(),
            native_device_ids: vec![],
            observed_sample_rate: Some(48_000),
            can_set_sample_rate: true,
            supported_sample_rates: vec![48_000],
            native_formats: mixrelay_audio_io::NativeDeviceFormats::default(),
        });
        let snapshot = DriverSnapshot {
            protocol_version: 1,
            revision: 3,
            state: DriverState::Ready,
            installed_version: None,
            owner_process_id: None,
            settings,
            persistence_error: None,
        };
        let mut requested = actual.clone();
        requested.render.as_mut().unwrap().channels = 2;
        requested.capture.as_mut().unwrap().channels = 2;
        assert_eq!(
            resolve(&snapshot, &[requested.clone()]).unwrap(),
            vec![actual.clone()]
        );
        requested.render = None;
        let resolved = resolve(&snapshot, &[requested.clone()]).unwrap();
        assert_eq!(resolved[0].render, None);
        assert_eq!(resolved[0].capture, actual.capture);
        requested.id = "02020202-0202-0202-0202-020202020202".into();
        assert!(resolve(&snapshot, &[requested]).is_err());
        assert!(resolve(&snapshot, &[]).unwrap().is_empty());
        assert_eq!(snapshot.settings.devices[0].spec, actual);
    }
}

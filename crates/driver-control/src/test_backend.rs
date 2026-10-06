use std::{collections::BTreeSet, sync::Arc};

use mixrelay_audio_io::{
    NativeDeviceFormats, NativeSampleFormat, NativeStreamFormat, NativeStreamFormats,
    VirtualEndpointCapabilities, VirtualEndpointDeviceSettings, VirtualEndpointSettings,
    formats::SAMPLE_RATES,
};
use parking_lot::Mutex;

use crate::{DriverBackend, DriverDeviceConfig, DriverError, DriverState};

pub(crate) struct FakeBackend(pub(crate) Arc<Mutex<Observation>>);

pub(crate) struct Observation {
    pub(crate) state: DriverState,
    pub(crate) settings: VirtualEndpointSettings,
    pub(crate) applies: usize,
    pub(crate) readbacks: usize,
    pub(crate) fail_apply: bool,
    pub(crate) fail_read: bool,
    pub(crate) failed_devices: BTreeSet<String>,
    pub(crate) recovered_devices: Vec<String>,
}

impl FakeBackend {
    pub(crate) fn ready() -> (Self, Arc<Mutex<Observation>>) {
        let observation = Arc::new(Mutex::new(Observation {
            state: DriverState::Ready,
            settings: VirtualEndpointSettings::configurable(&VirtualEndpointCapabilities {
                max_render_channels: 64,
                max_capture_channels: 64,
                supported_sample_rates: SAMPLE_RATES.to_vec(),
                supports_duplex: true,
                supports_dynamic_creation: true,
            }),
            applies: 0,
            readbacks: 0,
            fail_apply: false,
            fail_read: false,
            failed_devices: BTreeSet::new(),
            recovered_devices: Vec::new(),
        }));
        (Self(observation.clone()), observation)
    }
}

impl DriverBackend for FakeBackend {
    fn settings(&self) -> Result<VirtualEndpointSettings, DriverError> {
        let mut observation = self.0.lock();
        observation.readbacks += 1;
        if observation.fail_read {
            return Err(DriverError::Backend("fixture readback failed".into()));
        }
        Ok(observation.settings.clone())
    }

    fn replace(&mut self, desired: &[DriverDeviceConfig]) -> Result<(), DriverError> {
        let mut observation = self.0.lock();
        observation.applies += 1;
        if observation.fail_apply {
            return Err(DriverError::Backend("fixture apply failed".into()));
        }
        observation.settings.devices = desired
            .iter()
            .map(|device| VirtualEndpointDeviceSettings {
                spec: device.spec.clone(),
                native_device_ids: vec![format!("native:{}", device.spec.id.as_str())],
                observed_sample_rate: Some(device.spec.default_sample_rate),
                supported_sample_rates: device.spec.supported_sample_rates.clone(),
                can_set_sample_rate: true,
                native_formats: NativeDeviceFormats {
                    render: formats(
                        device.spec.render.map(|stream| stream.channels),
                        device.spec.default_sample_rate,
                        device.render_format,
                    ),
                    capture: formats(
                        device.spec.capture.map(|stream| stream.channels),
                        device.spec.default_sample_rate,
                        device.capture_format,
                    ),
                },
            })
            .collect();
        Ok(())
    }

    fn state(&self) -> DriverState {
        self.0.lock().state
    }

    fn recover(&mut self, desired: &[DriverDeviceConfig]) -> Result<bool, DriverError> {
        let failed: Vec<String> = {
            let observation = self.0.lock();
            desired
                .iter()
                .map(|device| device.spec.id.as_str())
                .filter(|id| observation.failed_devices.contains(*id))
                .map(str::to_owned)
                .collect()
        };
        if failed.is_empty() {
            return Ok(false);
        }
        self.replace(desired)?;
        let mut observation = self.0.lock();
        for id in &failed {
            observation.failed_devices.remove(id);
        }
        observation.recovered_devices.extend(failed);
        observation.state = DriverState::Ready;
        Ok(true)
    }
}

fn formats(
    channels: Option<u16>,
    rate: u32,
    requested: Option<NativeStreamFormat>,
) -> NativeStreamFormats {
    let Some(channels) = channels else {
        return NativeStreamFormats::default();
    };
    let float = NativeStreamFormat {
        sample_rate: rate,
        channels,
        sample_format: NativeSampleFormat::Float32,
    };
    let integer = NativeStreamFormat {
        sample_format: NativeSampleFormat::Signed16,
        ..float
    };
    NativeStreamFormats {
        current: Some(requested.unwrap_or(float)),
        supported: vec![float, integer],
    }
}

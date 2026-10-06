use super::{
    backend,
    loopback::{Loopback, MAX_FRAMES},
    native_name, native_names,
};
use mixrelay_audio_io::{
    NativeDeviceFormats, NativeSampleFormat, NativeStreamFormat, NativeStreamFormats,
    VirtualEndpointCapabilities, VirtualEndpointDeviceSettings,
};
use mixrelay_driver_control::{DriverDeviceConfig, DriverError};
use mixrelay_pipewire::{DriverFormat, GraphDriver, NativeStream, StreamProperties, StreamRole};
use std::time::{Duration, Instant};

pub(super) fn capabilities() -> VirtualEndpointCapabilities {
    VirtualEndpointCapabilities {
        max_render_channels: 64,
        max_capture_channels: 64,
        supported_sample_rates: mixrelay_audio_io::formats::SAMPLE_RATES.to_vec(),
        supports_duplex: true,
        supports_dynamic_creation: true,
    }
}

pub(super) fn validate(config: &DriverDeviceConfig) -> Result<(), DriverError> {
    config
        .spec
        .validate(&capabilities())
        .map_err(|error| DriverError::Invalid(error.to_string()))?;
    for (shape, requested) in [
        (config.spec.render, config.render_format),
        (config.spec.capture, config.capture_format),
    ] {
        if let Some(requested) = requested
            && (shape.is_none_or(|shape| shape.channels != requested.channels)
                || requested.sample_rate != config.spec.default_sample_rate)
        {
            return Err(DriverError::Invalid(
                "native tuple differs from device direction/nominal rate".into(),
            ));
        }
    }
    Ok(())
}

pub(super) struct DevicePreparation {
    pub(super) config: DriverDeviceConfig,
    name: String,
    render_format: Option<NativeStreamFormat>,
    capture_format: Option<NativeStreamFormat>,
    process: Loopback,
}

fn requested_formats(config: &DriverDeviceConfig) -> [Option<NativeStreamFormat>; 2] {
    let default = |shape: mixrelay_audio_io::StreamSpec| NativeStreamFormat {
        channels: shape.channels,
        sample_rate: config.spec.default_sample_rate,
        sample_format: NativeSampleFormat::Float32,
    };
    [
        config
            .render_format
            .or_else(|| config.spec.render.map(default)),
        config
            .capture_format
            .or_else(|| config.spec.capture.map(default)),
    ]
}

impl DevicePreparation {
    pub(super) fn new(config: &DriverDeviceConfig) -> Result<Self, DriverError> {
        validate(config)?;
        let [render_format, capture_format] = requested_formats(config);
        let render_channels = render_format.map_or(0, |format| usize::from(format.channels));
        let capture_channels = capture_format.map_or(0, |format| usize::from(format.channels));
        Ok(Self {
            config: config.clone(),
            name: native_name(&config.spec.id),
            render_format,
            capture_format,
            process: Loopback {
                render: None,
                capture: None,
                render_channels,
                capture_channels,
                input: vec![0.0; render_channels * MAX_FRAMES].into_boxed_slice(),
                output: vec![
                    0.0;
                    if render_channels == capture_channels {
                        0
                    } else {
                        capture_channels * MAX_FRAMES
                    }
                ]
                .into_boxed_slice(),
                mapper: mixrelay_audio_io::frame_budget::FrameBudgetMapper::new(
                    config.spec.default_sample_rate,
                )
                .map_err(backend)?,
            },
        })
    }
}

pub(super) struct DeviceRuntime {
    pub(super) config: DriverDeviceConfig,
    render: Option<StreamProperties>,
    capture: Option<StreamProperties>,
    driver: GraphDriver<Loopback>,
}

impl DeviceRuntime {
    pub(super) fn matches_config(&self, config: &DriverDeviceConfig) -> bool {
        self.config.spec == config.spec
            && requested_formats(&self.config) == requested_formats(config)
    }

    pub(super) fn open(mut preparation: DevicePreparation) -> Result<Self, DriverError> {
        let description = &preparation.config.spec.display_name;
        if let Some(format) = preparation.render_format {
            preparation.process.render = Some(
                NativeStream::open_pcm_format(
                    &format!("{}.render", preparation.name),
                    description,
                    StreamRole::VirtualRender,
                    format,
                    128,
                )
                .map_err(backend)?,
            );
        }
        if let Some(format) = preparation.capture_format {
            preparation.process.capture = Some(
                NativeStream::open_pcm_format(
                    &format!("{}.capture", preparation.name),
                    description,
                    StreamRole::VirtualCapture,
                    format,
                    128,
                )
                .map_err(backend)?,
            );
        }
        let render = preparation
            .process
            .render
            .as_ref()
            .map(NativeStream::properties);
        let capture = preparation
            .process
            .capture
            .as_ref()
            .map(NativeStream::properties);
        let format =
            DriverFormat::new(preparation.config.spec.default_sample_rate, 128).map_err(backend)?;
        let driver = GraphDriver::open(preparation.process, format).map_err(backend)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let exact = [
                (render.as_ref(), preparation.render_format),
                (capture.as_ref(), preparation.capture_format),
            ]
            .iter()
            .all(|(properties, requested)| {
                properties.map_or(requested.is_none(), |properties| {
                    properties
                        .native_formats()
                        .is_some_and(|(_, current)| current == *requested)
                })
            });
            if exact && driver.ready() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(DriverError::Backend(
                    "native device tuple readback timed out".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(Self {
            config: preparation.config,
            render,
            capture,
            driver,
        })
    }

    pub(super) fn ready(&self) -> bool {
        self.driver.ready()
            && self.render.iter().chain(self.capture.iter()).all(|stream| {
                stream
                    .native_formats()
                    .is_some_and(|(_, current)| current.is_some())
            })
    }

    pub(super) fn failed(&self) -> bool {
        self.driver.failed()
            || self
                .render
                .iter()
                .chain(self.capture.iter())
                .any(StreamProperties::failed)
    }

    pub(super) fn settings(&self) -> Result<VirtualEndpointDeviceSettings, DriverError> {
        let render = formats(self.render.as_ref())?;
        let capture = formats(self.capture.as_ref())?;
        let rate = render
            .current
            .or(capture.current)
            .map(|format| format.sample_rate);
        Ok(VirtualEndpointDeviceSettings {
            spec: self.config.spec.clone(),
            native_device_ids: native_names(&self.config.spec),
            observed_sample_rate: rate,
            supported_sample_rates: rate.into_iter().collect(),
            can_set_sample_rate: rate.is_some() && self.ready(),
            native_formats: NativeDeviceFormats { render, capture },
        })
    }
}

fn formats(properties: Option<&StreamProperties>) -> Result<NativeStreamFormats, DriverError> {
    let Some(properties) = properties else {
        return Ok(NativeStreamFormats::default());
    };
    let (_, current) = properties
        .native_formats()
        .ok_or_else(|| DriverError::Unavailable("native owner retired".into()))?;
    // These are Driver-reconfiguration capabilities at this opened nominal rate,
    // not a claim that the selected acquisition advertises alternative formats.
    // Each selection opens a singleton EnumFormat and verifies actual readback.
    let supported = current
        .into_iter()
        .flat_map(|current| {
            [
                NativeSampleFormat::Float32,
                NativeSampleFormat::Signed16,
                NativeSampleFormat::Signed24,
                NativeSampleFormat::Signed32,
            ]
            .map(|sample_format| NativeStreamFormat {
                sample_format,
                ..current
            })
        })
        .collect();
    Ok(NativeStreamFormats { current, supported })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_driver_control::DriverBackend;
    use mixrelay_pipewire::{DriverCycle, DriverProcess};

    #[test]
    fn implicit_float_formats_match_the_manager_actual_readback() {
        let implicit = crate::tests::config(4);
        let mut observed = implicit.clone();
        let [render, capture] = requested_formats(&implicit);
        observed.render_format = render;
        observed.capture_format = capture;
        assert_eq!(requested_formats(&implicit), requested_formats(&observed));
        observed.render_format.as_mut().unwrap().sample_format = NativeSampleFormat::Signed24;
        assert_ne!(requested_formats(&implicit), requested_formats(&observed));
    }

    #[test]
    #[ignore = "requires the existing isolated native lab and pw-cli; serial only"]
    fn native_driver_failed_stream_recovers_same_config_and_preserves_healthy_owner() {
        assert!(std::env::var_os("MIXRELAY_LAB_APPLICATION_FIXTURE").is_some());
        let mut failed = crate::tests::config(2);
        let format = NativeStreamFormat {
            sample_rate: 48_000,
            channels: 2,
            sample_format: NativeSampleFormat::Signed24,
        };
        failed.render_format = Some(format);
        failed.capture_format = Some(format);
        let healthy = crate::tests::config(4);
        let desired = [failed, healthy];
        let mut owner = crate::LinuxDriver::open().unwrap();
        owner.replace(&desired).unwrap();
        let before = owner.settings().unwrap();
        let node_ids = |runtime: &DeviceRuntime| {
            runtime
                .render
                .iter()
                .chain(runtime.capture.iter())
                .map(|properties| properties.node_id().unwrap())
                .collect::<Vec<_>>()
        };
        let failed_key = desired[0].spec.id.as_str();
        let healthy_key = desired[1].spec.id.as_str();
        let failed_nodes = node_ids(&owner.devices[failed_key]);
        let healthy_nodes = node_ids(&owner.devices[healthy_key]);
        let mut observed_intent = desired.clone();
        for config in &mut observed_intent {
            let [render, capture] = requested_formats(config);
            config.render_format = render;
            config.capture_format = capture;
        }
        assert!(!owner.recover(&observed_intent).unwrap());
        let command = std::process::Command::new("pw-cli")
            .args(["destroy", &failed_nodes[0].to_string()])
            .output()
            .unwrap();
        assert!(command.status.success(), "{command:?}");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !owner.devices[failed_key].failed() {
            assert!(
                Instant::now() < deadline,
                "destroyed stream did not report native failure"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!owner.devices[healthy_key].failed());
        assert!(owner.recover(&observed_intent).unwrap());
        assert_eq!(owner.settings().unwrap(), before);
        assert_eq!(node_ids(&owner.devices[healthy_key]), healthy_nodes);
        assert!(!owner.devices[failed_key].failed());
        assert!(!owner.recover(&desired).unwrap());
        owner.replace(&[]).unwrap();
        assert!(!owner.recover(&[]).unwrap());
        assert!(owner.devices.is_empty());
    }

    #[test]
    fn preparation_reserves_output_only_for_channel_projection() {
        for (render, capture) in [
            (2, 2),
            (32, 32),
            (64, 64),
            (32, 64),
            (64, 32),
            (0, 64),
            (64, 0),
        ] {
            let mut config = crate::tests::config(render.max(capture));
            config.spec.render = (render != 0).then_some(mixrelay_audio_io::StreamSpec {
                channels: render,
                channel_layout: mixrelay_audio_io::ChannelLayout::Discrete,
            });
            config.spec.capture = (capture != 0).then_some(mixrelay_audio_io::StreamSpec {
                channels: capture,
                channel_layout: mixrelay_audio_io::ChannelLayout::Discrete,
            });
            let mut preparation = DevicePreparation::new(&config).unwrap();
            assert_eq!(
                preparation.process.input.len(),
                usize::from(render) * MAX_FRAMES
            );
            let expected_output = if render == capture {
                0
            } else {
                usize::from(capture) * MAX_FRAMES
            };
            assert_eq!(preparation.process.output.len(), expected_output);
            preparation.process.process(&DriverCycle {
                position: 0,
                monotonic_ns: 0,
                clock_id: 1,
                rate: 48_000,
                frames: 128,
            });
        }
    }
}
